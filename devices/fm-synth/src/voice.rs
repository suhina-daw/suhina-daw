// SPDX-License-Identifier: Apache-2.0

// Ported from MSFA (music-synthesizer-for-android) as found in Dexed: dx7note.cc and
// controllers.h.
// Copyright 2012-2013 Google Inc.; Copyright 2016-2025 Pascal Gauthier.
// Licensed under the Apache License, Version 2.0 (see LICENSE-APACHE in this crate).
// Changes: translated to Rust; standard tuning only (no scale files, no MTS-ESP, no MPE);
// `retune` replaces `update` and keeps envelope stages; overflow-prone arithmetic is done in 64
// bits or wraps as the C code did; a pitch offset for a note's detuned twin (spread); hooks for
// Suhina's own parts (`ext.rs`, `kernel.rs`), neutral unless they are used.

//! One sounding note: six operators with their envelopes and pitches, the pitch envelope, and
//! modulation from the LFO and the controllers.

use crate::core::{self, Buses, OpParams};
use crate::env::{Env, PitchEnv, scale_out_level};
use crate::ext::{Frame, Mods, NoteExt, is_silent};
use crate::kernel;
use crate::patch::{self, Patch};
use crate::tables::{N, Tables};

/// Feedback 1..=7 shifts the fed-back signal by `FEEDBACK_BITDEPTH - feedback`.
const FEEDBACK_BITDEPTH: i32 = 8;

/// Q24 log frequency of MIDI key 0 at A = 440 Hz: (1 << 24) * (log2(440) - 69 / 12).
const KEY_0_LOGFREQ: i32 = 50_857_777;
/// One semitone in Q24 log frequency.
const SEMITONE: i32 = (1 << 24) / 12;

const COARSE: [i32; 32] = [
    -16_777_216,
    0,
    16_777_216,
    26_591_258,
    33_554_432,
    38_955_489,
    43_368_474,
    47_099_600,
    50_331_648,
    53_182_516,
    55_732_705,
    58_039_632,
    60_145_690,
    62_083_076,
    63_876_816,
    65_546_747,
    67_108_864,
    68_576_247,
    69_959_732,
    71_268_397,
    72_509_921,
    73_690_858,
    74_816_848,
    75_892_776,
    76_922_906,
    77_910_978,
    78_860_292,
    79_773_775,
    80_654_032,
    81_503_396,
    82_323_963,
    83_117_622,
];

const VELOCITY: [u8; 64] = [
    0, 70, 86, 97, 106, 114, 121, 126, 132, 138, 142, 148, 152, 156, 160, 163, 166, 170, 173, 174, 178, 181, 184, 186,
    189, 190, 194, 196, 198, 200, 202, 205, 206, 209, 211, 214, 216, 218, 220, 222, 224, 225, 227, 229, 230, 232, 233,
    235, 237, 238, 240, 241, 242, 243, 244, 246, 246, 248, 249, 250, 251, 252, 253, 254,
];

const EXP_SCALE: [u8; 33] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 14, 16, 19, 23, 27, 33, 39, 47, 56, 66, 80, 94, 110, 126, 142, 158, 174, 190,
    206, 222, 238, 250,
];

const PITCH_MOD_SENS: [i32; 8] = [0, 10, 20, 33, 55, 92, 153, 255];
const AMP_MOD_SENS: [i32; 4] = [0, 4_342_338, 7_171_437, 16_777_216];

/// Decibels per doubling of a level, which is `ONE_DOUBLING` in the engine's Q24 log.
const DB_PER_DOUBLING: f32 = 6.0206;
const ONE_DOUBLING: f32 = (1u32 << 24) as f32;

/// How much a controller (wheel, foot, breath, pressure) does and to what.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModRoute {
    /// 0..=99
    pub range: i32,
    pub pitch: bool,
    pub amp: bool,
    pub eg: bool,
}

/// Performance state shared by every voice: the wheels and pedals and what they do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Controllers {
    /// 14-bit pitch bend, 0x2000 at rest.
    pub pitch_bend: i32,
    /// Semitones for a full bend up and down.
    pub bend_range: i32,
    /// 0 bends smoothly; otherwise the bend moves in steps of this many semitones.
    pub bend_step: i32,
    pub wheel: i32,
    pub breath: i32,
    pub foot: i32,
    pub pressure: i32,
    pub wheel_route: ModRoute,
    pub breath_route: ModRoute,
    pub foot_route: ModRoute,
    pub pressure_route: ModRoute,
    pub portamento: bool,
    pub glissando: bool,
    /// 0..=127
    pub portamento_time: usize,
    amp_mod: i32,
    pitch_mod: i32,
    eg_mod: i32,
}

impl Default for Controllers {
    fn default() -> Self {
        let mut c = Self {
            pitch_bend: 0x2000,
            bend_range: 2,
            bend_step: 0,
            wheel: 0,
            breath: 0,
            foot: 0,
            pressure: 0,
            wheel_route: ModRoute::default(),
            breath_route: ModRoute::default(),
            foot_route: ModRoute::default(),
            pressure_route: ModRoute::default(),
            portamento: false,
            glissando: false,
            portamento_time: 0,
            amp_mod: 0,
            pitch_mod: 0,
            eg_mod: 0,
        };
        c.refresh();
        c
    }
}

impl Controllers {
    /// Recomputes the modulation amounts after a controller or a route changed.
    pub fn refresh(&mut self) {
        self.amp_mod = 0;
        self.pitch_mod = 0;
        self.eg_mod = 0;
        let sources = [
            (self.wheel, self.wheel_route),
            (self.breath, self.breath_route),
            (self.foot, self.foot_route),
            (self.pressure, self.pressure_route),
        ];
        for (value, route) in sources {
            let total = (f64::from(value) * (0.01 * f64::from(route.range))) as i32;
            if route.amp {
                self.amp_mod = self.amp_mod.max(total);
            }
            if route.pitch {
                self.pitch_mod = self.pitch_mod.max(total);
            }
            if route.eg {
                self.eg_mod = self.eg_mod.max(total);
            }
        }
        if !sources.iter().any(|(_, route)| route.eg) {
            self.eg_mod = 127;
        }
    }
}

fn scale_velocity(velocity: i32, sensitivity: i32) -> i32 {
    let clamped = velocity.clamp(0, 127);
    let value = i32::from(VELOCITY.get((clamped >> 1) as usize).copied().unwrap_or(0)) - 239;
    ((sensitivity * value + 7) >> 3) << 4
}

fn scale_rate(key: i32, sensitivity: i32) -> i32 {
    let x = (key / 3 - 7).clamp(0, 31);
    (sensitivity * x) >> 3
}

fn scale_curve(group: i32, depth: i32, curve: i32) -> i32 {
    let scale = if curve == 0 || curve == 3 {
        (group * depth * 329) >> 12
    } else {
        let raw = i32::from(EXP_SCALE.get(group.clamp(0, 32) as usize).copied().unwrap_or(0));
        (raw * depth * 329) >> 15
    };
    if curve < 2 { -scale } else { scale }
}

fn scale_level(
    key: i32,
    break_point: i32,
    left_depth: i32,
    right_depth: i32,
    left_curve: i32,
    right_curve: i32,
) -> i32 {
    let offset = key - break_point - 17;
    if offset >= 0 {
        scale_curve((offset + 1) / 3, right_depth, right_curve)
    } else {
        scale_curve(-(offset - 1) / 3, left_depth, left_curve)
    }
}

fn round_to_semitone(logfreq: i32) -> i32 {
    logfreq - (logfreq - KEY_0_LOGFREQ) % SEMITONE
}

/// The Q24 log frequency of an operator for `key`.
fn osc_freq(key: i32, fixed: bool, coarse: i32, fine: i32, detune: i32) -> i32 {
    if !fixed {
        let mut logfreq = KEY_0_LOGFREQ + SEMITONE * key.clamp(0, 127);
        // Measured on hardware: detune moves less at higher pitches.
        let ratio = 0.0209 * (-0.396 * f64::from(logfreq as f32 / (1 << 24) as f32)).exp() / 7.0;
        logfreq = (f64::from(logfreq) + ratio * f64::from(logfreq) * f64::from(detune - 7)) as i32;
        logfreq += COARSE.get((coarse & 31) as usize).copied().unwrap_or(0);
        if fine != 0 {
            // (1 << 24) / ln(2)
            logfreq += (24_204_406.323_123 * (1.0 + 0.01 * f64::from(fine)).ln() + 0.5).floor() as i32;
        }
        logfreq
    } else {
        // ((1 << 24) * log2(10) * 0.01) << 3
        let mut logfreq = (4_458_616 * ((coarse & 3) * 100 + fine)) >> 3;
        if detune > 7 {
            logfreq += 13_457 * (detune - 7);
        }
        logfreq
    }
}

/// One note.
#[derive(Debug, Clone, Copy, Default)]
pub struct Note {
    initialised: bool,
    env: [Env; 6],
    params: [OpParams; 6],
    pitch_env: PitchEnv,
    base_pitch: [i32; 6],
    fb: [i32; 2],
    fb_shift: i32,
    amp_mod_sens: [i32; 6],
    fixed: [bool; 6],
    op_on: [bool; 6],
    amp_mod_depth: i32,
    algorithm: usize,
    pitch_mod_depth: i32,
    pitch_mod_sens: i32,
    porta_pitch: [i32; 6],
    /// Added to the pitch of ratio operators, Q24 per octave.
    pitch_offset: i32,
    /// Each operator's output level as the envelopes read it, for a time envelope.
    out_level: [i32; 6],
    /// The block's modulation, for what follows the kernel.
    mods: Option<Mods>,
    x: NoteExt,
}

impl Note {
    /// Starts the note: `key` 0..=127 (transposed already), `velocity` 1..=127.
    pub fn init(&mut self, p: &Patch, key: i32, velocity: i32, t: &Tables) {
        self.initialised = true;
        for op in 0..6 {
            let o = p.op(op);
            let out_level = (scale_out_level(o.level)
                + scale_level(key, o.break_point, o.left_depth, o.right_depth, o.left_curve, o.right_curve))
            .min(127);
            let out_level = ((out_level << 5) + scale_velocity(velocity, o.velocity_sens)).max(0);
            if let Some(slot) = self.out_level.get_mut(op) {
                *slot = out_level;
            }
            let rate_scaling = scale_rate(key, o.rate_scaling);
            if let Some(env) = self.env.get_mut(op) {
                env.init(o.rates, o.levels, out_level, rate_scaling, t);
            }
            let freq = osc_freq(key, o.fixed, o.coarse, o.fine, o.detune);
            self.set_op(op, freq, o.fixed, o.on, o.amp_mod_sens);
            if let Some(slot) = self.porta_pitch.get_mut(op) {
                *slot = freq;
            }
        }
        self.pitch_env.set(p.pitch_rates(), p.pitch_levels(), t);
        self.set_global(p);
    }

    /// Follows changed settings while the note sounds, keeping where its envelopes are.
    pub fn retune(&mut self, p: &Patch, key: i32, velocity: i32, t: &Tables) {
        for op in 0..6 {
            let o = p.op(op);
            let freq = osc_freq(key, o.fixed, o.coarse, o.fine, o.detune);
            self.set_op(op, freq, o.fixed, o.on, o.amp_mod_sens);
            let out_level = (scale_out_level(o.level)
                + scale_level(key, o.break_point, o.left_depth, o.right_depth, o.left_curve, o.right_curve))
            .min(127);
            let out_level = ((out_level << 5) + scale_velocity(velocity, o.velocity_sens)).max(0);
            if let Some(slot) = self.out_level.get_mut(op) {
                *slot = out_level;
            }
            if let Some(env) = self.env.get_mut(op) {
                env.retune(o.rates, o.levels, out_level, scale_rate(key, o.rate_scaling), t);
            }
        }
        self.pitch_env.retune(p.pitch_rates(), p.pitch_levels());
        self.set_global(p);
    }

    fn set_op(&mut self, op: usize, freq: i32, fixed: bool, on: bool, amp_mod_sens: i32) {
        if let Some(slot) = self.base_pitch.get_mut(op) {
            *slot = freq;
        }
        if let Some(slot) = self.fixed.get_mut(op) {
            *slot = fixed;
        }
        if let Some(slot) = self.op_on.get_mut(op) {
            *slot = on;
        }
        if let Some(slot) = self.amp_mod_sens.get_mut(op) {
            *slot = AMP_MOD_SENS.get((amp_mod_sens & 3) as usize).copied().unwrap_or(0);
        }
    }

    fn set_global(&mut self, p: &Patch) {
        self.algorithm = p.algorithm();
        let feedback = p.feedback();
        self.fb_shift = if feedback != 0 { FEEDBACK_BITDEPTH - feedback } else { 16 };
        self.pitch_mod_depth = (p.byte(patch::LFO_PITCH_DEPTH) * 165) >> 6;
        self.pitch_mod_sens = PITCH_MOD_SENS.get((p.byte(patch::PITCH_MOD_SENS) & 7) as usize).copied().unwrap_or(0);
        self.amp_mod_depth = (p.byte(patch::LFO_AMP_DEPTH) * 165) >> 6;
    }

    /// Starts Suhina's own parts for a key going down.
    pub fn start_ext(&mut self, p: &Patch, frame: &Frame, key: i32, velocity: i32, random: f32) {
        let coarse: [i32; 6] = std::array::from_fn(|op| p.op(op).coarse);
        self.x.start(frame, key, velocity, random, &coarse);
        // A note started without sync keeps its phases unless its operator retriggers.
        for (op, o) in frame.ext.ops.iter().enumerate() {
            if o.retrigger
                && let Some(param) = self.params.get_mut(op)
            {
                param.phase = o.phase;
            }
        }
    }

    /// Starts operators with the phases the synth gives them, where sync restarts them.
    pub fn set_start_phases(&mut self, frame: &Frame) {
        for (param, o) in self.params.iter_mut().zip(&frame.ext.ops) {
            param.phase = o.phase;
        }
    }

    /// Detunes the note: `offset` is Q24 per octave, 0 is none.
    pub fn set_pitch_offset(&mut self, offset: i32) {
        self.pitch_offset = offset;
    }

    /// Glides from where `from` is heading.
    pub fn take_portamento(&mut self, from: &Note) {
        self.porta_pitch = from.porta_pitch;
    }

    /// Starts every operator at phase 0 and silent.
    pub fn sync_oscillators(&mut self) {
        for p in &mut self.params {
            p.gain_out = 0;
            p.phase = 0;
        }
    }

    pub fn key_up(&mut self, t: &Tables) {
        self.x.key_up();
        for env in &mut self.env {
            env.key_down(false, t);
        }
        self.pitch_env.key_down(false);
    }

    /// Whether any operator heard directly still sounds.
    pub fn is_playing(&self) -> bool {
        self.initialised
            && self.env.iter().enumerate().any(|(op, env)| {
                core::is_carrier(self.algorithm, op) && self.x.op_active(op).unwrap_or_else(|| env.is_active())
            })
    }

    /// Adds one block of the note to `out`.
    pub fn compute(
        &mut self,
        out: &mut [i32; N],
        lfo: i32,
        lfo_delay: i32,
        c: &Controllers,
        buses: &mut Buses,
        t: &Tables,
        frame: &Frame,
    ) {
        let carriers: [bool; 6] = std::array::from_fn(|op| core::is_carrier(self.algorithm, op));
        let mods = self.x.begin_block(frame, &carriers);
        // Pitch: the LFO (faded in by its delay) or a controller, whichever is deeper, plus the
        // pitch envelope and the bend.
        let depth = (self.pitch_mod_depth as u32).wrapping_mul(lfo_delay as u32); // Q32
        let sens_lfo = i64::from(self.pitch_mod_sens) * i64::from(lfo - (1 << 23));
        let pmod1 = ((i64::from(depth) * sens_lfo) >> 39).abs();
        let pmod2 = ((i64::from(c.pitch_mod) * sens_lfo) >> 14).abs();
        let mut pitch_mod = pmod1.max(pmod2) as i32;
        if sens_lfo < 0 {
            pitch_mod = pitch_mod.wrapping_neg();
        }
        let pitch_mod = self.pitch_env.next().wrapping_add(pitch_mod);
        let mut bend = c.pitch_bend - 0x2000;
        if bend != 0 {
            if c.bend_step == 0 {
                bend = (f64::from((bend << 11) as f32 * c.bend_range as f32) / 12.0) as i32;
            } else {
                let step = 12 / c.bend_step.clamp(1, 12);
                bend = bend * step / 8_191;
                bend = (bend * (8_191 / step)) << 11;
            }
        }
        let pitch_base = bend;
        let pitch_mod = pitch_mod.wrapping_add(pitch_base).wrapping_add(self.pitch_offset);

        // Amplitude: the LFO or a controller, and the EG bias.
        let lfo_amp = i64::from((1 << 24) - lfo);
        let amod1 = ((i64::from(self.amp_mod_depth) * i64::from(lfo_delay)) >> 8) as u32;
        let amod1 = ((i64::from(amod1) * lfo_amp) >> 24) as u32;
        let amod2 = ((i64::from(c.amp_mod) * lfo_amp) >> 7) as u32;
        let amod3 = ((c.eg_mod + 1) as u32) << 17;
        let amp_mod = amod1.max(amod2).max((1u32 << 24).wrapping_sub(amod3));

        let porta_rate = if c.portamento {
            let rates = if c.glissando { &t.porta_glissando } else { &t.porta };
            rates.get(c.portamento_time).copied().unwrap_or(0)
        } else {
            t.porta.first().copied().unwrap_or(0)
        };

        for op in 0..6 {
            let (Some(env), Some(param)) = (self.env.get_mut(op), self.params.get_mut(op)) else {
                continue;
            };
            if !self.op_on.get(op).copied().unwrap_or(false) {
                env.next(t);
                param.level_in = 0;
                continue;
            }
            let target = self.base_pitch.get(op).copied().unwrap_or(0);
            let moved = mods.pitch.get(op).copied().unwrap_or(0);
            if self.fixed.get(op).copied().unwrap_or(false) {
                param.freq = t.freq(target.wrapping_add(pitch_base).wrapping_add(mods.fixed));
            } else {
                let mut pitch = target;
                if let Some(current) = self.porta_pitch.get_mut(op)
                    && *current != target
                {
                    pitch = *current;
                    if c.glissando {
                        pitch = round_to_semitone(pitch);
                    }
                    let up = *current < target;
                    let next = if up { current.saturating_add(porta_rate) } else { current.saturating_sub(porta_rate) };
                    *current = if (up && next > target) || (!up && next < target) { target } else { next };
                }
                param.freq = t.freq(pitch.wrapping_add(pitch_mod).wrapping_add(moved));
            }

            let mut level = env.next(t);
            if let Some(Some(db)) = mods.time_db.get(op) {
                level = if is_silent(*db) {
                    0
                } else {
                    // The level the DX7 envelope reaches at full, then the envelope in decibels.
                    let full = (self.out_level.get(op).copied().unwrap_or(0) + 4_032 - 4_256).max(16) << 16;
                    full.saturating_add((*db / DB_PER_DOUBLING * ONE_DOUBLING) as i32).max(0)
                };
            }
            let offset = mods.level_db.get(op).copied().unwrap_or(0.0);
            if offset != 0.0 {
                level = level.saturating_add((offset / DB_PER_DOUBLING * ONE_DOUBLING) as i32).max(0);
            }
            let sens = self.amp_mod_sens.get(op).copied().unwrap_or(0);
            if sens != 0 {
                let amount = ((u64::from(amp_mod) * sens as u64) >> 24) as u32;
                // Fitted by ear in the original.
                let pt = ((amount as f32 / 262_144.0) as f64 * 0.07 + 12.2).exp() as u32;
                let diff = ((u64::from(level as u32) * (u64::from(pt) << 4)) >> 28) as u32;
                level = (level as u32).wrapping_sub(diff) as i32;
            }
            param.level_in = level;
        }
        if mods.kernel {
            kernel::render(
                out,
                &mut self.params,
                self.algorithm,
                &mut self.fb,
                self.fb_shift,
                &mut self.x,
                buses,
                t,
                frame,
                &mods,
            );
        } else {
            core::render(out, &mut self.params, self.algorithm, &mut self.fb, self.fb_shift, buses, t);
        }
        self.mods = Some(mods);
    }

    /// The block just computed, after the filter, ready to mix: samples, gain and pan.
    pub fn finish(&mut self, out: &[i32; N], frame: &Frame) -> ([f32; N], f32, f32) {
        match self.mods {
            Some(mods) => (self.x.finish(out, frame, &mods), mods.gain, mods.pan),
            None => (self.x.finish(out, frame, &Mods::off()), 1.0, 0.0),
        }
    }
}
