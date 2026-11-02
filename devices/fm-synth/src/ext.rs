// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the FM synth has beyond the DX7 voice (ADR 0034): the settings, kept beside the voice
//! ([`Patch`](crate::Patch)), and a note's state and modulation for one block. Everything is neutral by
//! default, and then the ported path runs untouched.

use suhina_dsp::{Circuit, Multimode, MultimodeSettings, Response};

use crate::params::{self, ENV_FIELDS, HARMONICS, MATRIX_SOURCES, OP_EXT_FIELDS, env, filter, lfo, opx, pitch};
use crate::tables::N;
use crate::time_env::{Clock, EnvSettings, LoopMode, SILENCE_DB, TimeEnv};
use crate::wave::{self, DrawnSettings, DrawnWave};

/// Destinations in [`params::DESTINATIONS`] order.
pub mod dest {
    pub const OP_LEVEL: usize = 1;
    pub const CROSSFADE_1_3: usize = 7;
    pub const CROSSFADE_2_4: usize = 8;
    pub const FEEDBACK: usize = 9;
    pub const FIXED: usize = 10;
    pub const FM_AMOUNT: usize = 11;
    pub const FILTER_FREQ: usize = 12;
    pub const FILTER_RES: usize = 13;
    pub const FILTER_MORPH: usize = 14;
    pub const FILTER_DRIVE: usize = 15;
    pub const FILTER_ENV: usize = 16;
    pub const SHAPER_DRIVE: usize = 17;
    pub const LFO_RATE: usize = 18;
    pub const LFO_AMOUNT: usize = 19;
    pub const PITCH_ENV: usize = 20;
    pub const VOLUME: usize = 21;
    pub const PAN: usize = 22;
    pub const TONE: usize = 23;
    pub const TIME: usize = 24;
    pub const COUNT: usize = 25;
}

/// How far a full modulation moves a level, in decibels.
const LEVEL_RANGE_DB: f32 = 48.0;
/// How far a full filter envelope moves the frequency, in octaves.
const FILTER_ENV_OCTAVES: f32 = 9.0;
/// How far a full LFO moves the frequency of the filter, in octaves.
const FILTER_LFO_OCTAVES: f32 = 4.0;
/// How far a full modulation moves fixed frequencies, in octaves.
const FIXED_OCTAVES: f32 = 2.0;
/// Key the filter and level tracking pivot on.
const CENTRE_KEY: f32 = 60.0;
/// Frames between filter updates.
const FILTER_STEP: usize = 16;
/// One voice at full scale in the engine's output.
pub const FULL_SCALE: f32 = (1u32 << 28) as f32;

/// One operator's settings, by engine order (the first is operator 6).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpExt {
    pub wave: usize,
    pub feedback: f32,
    /// Where a note starts it, 1 << 24 is a cycle.
    pub phase: i32,
    pub retrigger: bool,
    pub freq_by_velocity: f32,
    pub quantize: bool,
    pub level_by_key: f32,
    pub time_env: bool,
    pub env: EnvSettings,
    pub drawn: DrawnSettings,
    pub partials: [f32; HARMONICS],
}

impl Default for OpExt {
    fn default() -> Self {
        let mut partials = [0.0; HARMONICS];
        partials[0] = 1.0;
        Self {
            wave: wave::SINE,
            feedback: 0.0,
            phase: 0,
            retrigger: false,
            freq_by_velocity: 0.0,
            quantize: false,
            level_by_key: 0.0,
            time_env: false,
            env: EnvSettings::default(),
            drawn: DrawnSettings::default(),
            partials,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilterExt {
    pub on: bool,
    pub response: Response,
    pub steep: bool,
    pub circuit: Circuit,
    pub frequency: f32,
    pub resonance: f32,
    pub morph: f32,
    pub drive_db: f32,
    pub env_amount: f32,
    pub by_velocity: f32,
    pub by_key: f32,
    pub shaper: usize,
    pub shaper_drive_db: f32,
    pub shaper_mix: f32,
    pub env: EnvSettings,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LfoExt {
    pub on: bool,
    pub wave: usize,
    pub range: usize,
    pub rate: f32,
    pub sync_beats: f32,
    pub retrigger: bool,
    pub by_key: f32,
    pub by_velocity: f32,
    pub amount: f32,
    pub to_op: [bool; 6],
    pub to_filter: bool,
    pub dest_b: usize,
    pub amount_b: f32,
    pub env: EnvSettings,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchExt {
    pub on: bool,
    pub amount: f32,
    pub to_op: [bool; 6],
    pub to_lfo: bool,
    pub dest_b: usize,
    pub amount_b: f32,
    pub env: EnvSettings,
}

/// A matrix row: two destinations and their amounts.
pub type Row = [(usize, f32); 2];

/// See the module.
#[derive(Debug, Clone, PartialEq)]
pub struct Ext {
    pub ops: [OpExt; 6],
    /// Which operators' drawn waves have yet to catch up.
    pub drawn_dirty: [bool; 6],
    pub time: f32,
    pub antialias: bool,
    pub interpolate: bool,
    pub filter: FilterExt,
    pub lfo: LfoExt,
    pub pitch: PitchExt,
    pub matrix: [Row; MATRIX_SOURCES],
}

impl Default for Ext {
    fn default() -> Self {
        Self {
            ops: [OpExt::default(); 6],
            drawn_dirty: [true; 6],
            time: 0.0,
            antialias: true,
            interpolate: true,
            filter: FilterExt {
                on: false,
                response: Response::LowPass,
                steep: true,
                circuit: Circuit::Clean,
                frequency: 20_000.0,
                resonance: 0.0,
                morph: 0.0,
                drive_db: 0.0,
                env_amount: 0.0,
                by_velocity: 0.0,
                by_key: 0.0,
                shaper: 0,
                shaper_drive_db: 0.0,
                shaper_mix: 1.0,
                env: EnvSettings::default(),
            },
            lfo: LfoExt {
                on: false,
                wave: 0,
                range: 0,
                rate: 0.5,
                sync_beats: 1.0,
                retrigger: true,
                by_key: 0.0,
                by_velocity: 0.0,
                amount: 0.0,
                to_op: [true; 6],
                to_filter: false,
                dest_b: 0,
                amount_b: 0.0,
                env: EnvSettings { initial: 1.0, peak: 1.0, sustain: 1.0, end: 1.0, ..EnvSettings::default() },
            },
            pitch: PitchExt {
                on: false,
                amount: 1.0,
                to_op: [true; 6],
                to_lfo: false,
                dest_b: 0,
                amount_b: 0.0,
                env: EnvSettings::default(),
            },
            matrix: [[(0, 0.0); 2]; MATRIX_SOURCES],
        }
    }
}

fn apply_env(e: &mut EnvSettings, field: usize, value: f64) {
    let v = value as f32;
    match field {
        env::ATTACK => e.attack = v,
        env::DECAY => e.decay = v,
        env::RELEASE => e.release = v,
        env::INITIAL => e.initial = v,
        env::PEAK => e.peak = v,
        env::SUSTAIN => e.sustain = v,
        env::END => e.end = v,
        env::LOOP => e.mode = LoopMode::from_index(value as usize),
        env::LOOP_TIME => e.loop_time = v,
        env::REPEAT => e.repeat_beats = params::REPEAT_BEATS.get(value as usize).copied().unwrap_or(1.0) as f32,
        env::TIME_BY_VEL => e.time_by_velocity = v,
        env::TIME_BY_KEY => e.time_by_key = v,
        env::ATTACK_SLOPE => e.attack_slope = v,
        env::DECAY_SLOPE => e.decay_slope = v,
        env::RELEASE_SLOPE => e.release_slope = v,
        _ => {}
    }
}

fn set_flag(flags: &mut [bool; 6], operator: usize, on: bool) {
    if let Some(slot) = flags.get_mut(5usize.saturating_sub(operator)) {
        *slot = on;
    }
}

impl Ext {
    /// Takes the value of parameter `index`, at or past `params::TIME`.
    pub fn apply(&mut self, index: usize, value: f64) {
        let (v, on) = (value as f32, value >= 0.5);
        match index {
            params::TIME => self.time = v,
            params::ANTIALIAS => self.antialias = on,
            params::INTERPOLATION => self.interpolate = on,
            i if (params::FILTER..params::FILTER_ENV).contains(&i) => self.apply_filter(i - params::FILTER, value),
            i if (params::FILTER_ENV..params::LFO).contains(&i) => {
                apply_env(&mut self.filter.env, i - params::FILTER_ENV, value)
            }
            i if (params::LFO..params::LFO_ENV).contains(&i) => self.apply_lfo(i - params::LFO, value),
            i if (params::LFO_ENV..params::PITCH).contains(&i) => {
                apply_env(&mut self.lfo.env, i - params::LFO_ENV, value)
            }
            i if (params::PITCH..params::PITCH_ENV).contains(&i) => self.apply_pitch(i - params::PITCH, value),
            i if (params::PITCH_ENV..params::MATRIX).contains(&i) => {
                apply_env(&mut self.pitch.env, i - params::PITCH_ENV, value)
            }
            i if (params::MATRIX..params::OP_EXT).contains(&i) => {
                let at = i - params::MATRIX;
                if let Some(row) = self.matrix.get_mut(at / 4)
                    && let Some(slot) = row.get_mut((at % 4) / 2)
                {
                    if at.is_multiple_of(2) {
                        slot.0 = value as usize;
                    } else {
                        slot.1 = v;
                    }
                }
            }
            i if (params::OP_EXT..params::DRAWN).contains(&i) => {
                let at = i - params::OP_EXT;
                self.apply_op(at / OP_EXT_FIELDS, at % OP_EXT_FIELDS, value);
            }
            i if (params::DRAWN..params::PANEL).contains(&i) => {
                let at = i - params::DRAWN;
                let stored = 5usize.saturating_sub(at / HARMONICS);
                if let Some(op) = self.ops.get_mut(stored)
                    && let Some(slot) = op.partials.get_mut(at % HARMONICS)
                {
                    *slot = v;
                }
                if let Some(flag) = self.drawn_dirty.get_mut(stored) {
                    *flag = true;
                }
            }
            _ => {}
        }
    }

    fn apply_filter(&mut self, field: usize, value: f64) {
        let (v, f) = (value as f32, &mut self.filter);
        match field {
            filter::ON => f.on = value >= 0.5,
            filter::TYPE => {
                f.response = match value as usize {
                    1 => Response::HighPass,
                    2 => Response::BandPass,
                    3 => Response::Notch,
                    4 => Response::Morph,
                    _ => Response::LowPass,
                }
            }
            filter::SLOPE => f.steep = value >= 0.5,
            filter::CIRCUIT => {
                f.circuit = match value as usize {
                    1 => Circuit::Warm,
                    2 => Circuit::Edge,
                    3 => Circuit::Bite,
                    4 => Circuit::Grit,
                    _ => Circuit::Clean,
                }
            }
            filter::FREQ => f.frequency = v,
            filter::RES => f.resonance = v,
            filter::MORPH => f.morph = v,
            filter::DRIVE => f.drive_db = v,
            filter::ENV => f.env_amount = v,
            filter::BY_VEL => f.by_velocity = v,
            filter::BY_KEY => f.by_key = v,
            filter::SHAPER => f.shaper = value as usize,
            filter::SHAPER_DRIVE => f.shaper_drive_db = v,
            filter::SHAPER_MIX => f.shaper_mix = v,
            _ => {}
        }
    }

    fn apply_lfo(&mut self, field: usize, value: f64) {
        let (v, on, l) = (value as f32, value >= 0.5, &mut self.lfo);
        match field {
            lfo::ON => l.on = on,
            lfo::WAVE => l.wave = value as usize,
            lfo::RANGE => l.range = value as usize,
            lfo::RATE => l.rate = v,
            lfo::SYNC => l.sync_beats = params::REPEAT_BEATS.get(value as usize).copied().unwrap_or(1.0) as f32,
            lfo::RETRIGGER => l.retrigger = on,
            lfo::BY_KEY => l.by_key = v,
            lfo::BY_VEL => l.by_velocity = v,
            lfo::AMOUNT => l.amount = v,
            lfo::TO_FILTER => l.to_filter = on,
            lfo::DEST_B => l.dest_b = value as usize,
            lfo::AMOUNT_B => l.amount_b = v,
            f if (lfo::TO_OP..lfo::TO_OP + 6).contains(&f) => set_flag(&mut l.to_op, f - lfo::TO_OP, on),
            _ => {}
        }
    }

    fn apply_pitch(&mut self, field: usize, value: f64) {
        let (v, on, p) = (value as f32, value >= 0.5, &mut self.pitch);
        match field {
            pitch::ON => p.on = on,
            pitch::AMOUNT => p.amount = v,
            pitch::TO_LFO => p.to_lfo = on,
            pitch::DEST_B => p.dest_b = value as usize,
            pitch::AMOUNT_B => p.amount_b = v,
            f if (pitch::TO_OP..pitch::TO_OP + 6).contains(&f) => set_flag(&mut p.to_op, f - pitch::TO_OP, on),
            _ => {}
        }
    }

    fn apply_op(&mut self, operator: usize, field: usize, value: f64) {
        let stored = 5usize.saturating_sub(operator);
        let (v, on) = (value as f32, value >= 0.5);
        let Some(o) = self.ops.get_mut(stored) else { return };
        let mut redraw = false;
        match field {
            opx::WAVE => o.wave = value as usize,
            opx::FEEDBACK => o.feedback = v,
            opx::PHASE => o.phase = (value * f64::from(1 << 24)) as i32,
            opx::RETRIGGER => o.retrigger = on,
            opx::FREQ_BY_VEL => o.freq_by_velocity = v,
            opx::FREQ_QUANTIZE => o.quantize = on,
            opx::LEVEL_BY_KEY => o.level_by_key = v,
            opx::ENV_MODE => o.time_env = on,
            opx::PARTIALS => {
                o.drawn.partials = 16usize << (value as usize).min(2);
                redraw = true;
            }
            opx::REPEAT => {
                o.drawn.repeat = v;
                redraw = true;
            }
            opx::NORMALIZE => {
                o.drawn.normalize = on;
                redraw = true;
            }
            opx::ODD_EVEN => {
                o.drawn.odd_even = value as usize;
                redraw = true;
            }
            f if (opx::ENV..opx::ENV + ENV_FIELDS).contains(&f) => apply_env(&mut o.env, f - opx::ENV, value),
            _ => {}
        }
        if redraw && let Some(flag) = self.drawn_dirty.get_mut(stored) {
            *flag = true;
        }
    }

    /// Whether a block needs the sample-accurate kernel rather than the ported one.
    fn needs_kernel(&self) -> bool {
        self.ops.iter().any(|o| o.wave != wave::SINE || o.feedback > 0.0)
    }

    /// Whether any modulation is routed.
    fn has_routes(&self) -> bool {
        self.matrix.iter().flatten().any(|(to, amount)| *to != 0 && *amount != 0.0)
            || (self.lfo.on && self.lfo.dest_b != 0 && self.lfo.amount_b != 0.0)
            || (self.pitch.on && self.pitch.dest_b != 0 && self.pitch.amount_b != 0.0)
    }
}

/// What every note of a block reads.
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    pub ext: &'a Ext,
    pub drawn: &'a [DrawnWave],
    pub sample_rate: f32,
    pub clock: Clock,
    /// Seconds since the last reset, for a free-running LFO.
    pub elapsed: f64,
    pub pressure: f32,
    pub bend: f32,
    pub wheel: f32,
    pub foot: f32,
    pub breath: f32,
}

/// A note's modulation for one block.
#[derive(Debug, Clone, Copy)]
pub struct Mods {
    /// Added to each operator's level, decibels.
    pub level_db: [f32; 6],
    /// Each operator's level in decibels from its time envelope, or `None` when it uses the DX7
    /// envelope.
    pub time_db: [Option<f32>; 6],
    /// Added to each operator's pitch, Q24 per octave.
    pub pitch: [i32; 6],
    /// Added to fixed operators' pitch.
    pub fixed: i32,
    /// How far the sample LFO moves each operator's frequency, 1 is an octave's worth.
    pub lfo_depth: [f32; 6],
    pub feedback: f32,
    pub kernel: bool,
    /// Note gains for the block.
    pub gain: f32,
    pub pan: f32,
    sums: [f32; dest::COUNT],
    filter_env: f32,
    lfo_block: f32,
}

impl Mods {
    pub fn sum(&self, to: usize) -> f32 {
        self.sums.get(to).copied().unwrap_or(0.0)
    }

    /// Everything off: what a note reads while only the DX7 path runs.
    pub fn off() -> Self {
        Self::neutral()
    }

    fn neutral() -> Self {
        Self {
            level_db: [0.0; 6],
            time_db: [None; 6],
            pitch: [0; 6],
            fixed: 0,
            lfo_depth: [0.0; 6],
            feedback: 0.0,
            kernel: false,
            gain: 1.0,
            pan: 0.0,
            sums: [0.0; dest::COUNT],
            filter_env: 0.0,
            lfo_block: 0.0,
        }
    }
}

fn next_random(state: &mut u32) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
}

/// Hertz for the LFO's rate (0 to 1) in its low or high range: 50 s to 30 Hz, 8 Hz to 12 kHz.
fn rate_hertz(range: usize, rate: f32) -> f32 {
    let (low, high) = if range == 1 { (8.0f32, 12_000.0f32) } else { (0.02f32, 30.0f32) };
    low * (high / low).powf(rate.clamp(0.0, 1.0))
}

/// The state beyond the DX7 note.
#[derive(Debug, Clone, Copy)]
pub struct NoteExt {
    envs: [TimeEnv; 6],
    /// Per operator: its share of pitch from velocity, Q24 per octave.
    velocity_pitch: [i32; 6],
    filter_env: TimeEnv,
    lfo_env: TimeEnv,
    pitch_env: TimeEnv,
    velocity: f32,
    key: f32,
    random: f32,
    pub noise: u32,
    down: bool,
    time_env: [bool; 6],
    /// The operators' own feedback.
    pub op_fb: [[i32; 2]; 6],
    lfo_phase: f64,
    lfo_hold: f32,
    lfo_noise: f32,
    /// The LFO over the block, after its amount.
    pub lfo: [f32; N],
    filter: Multimode,
    filter_cutoff: f32,
    filter_set: bool,
    tone_z: f32,
}

impl Default for NoteExt {
    fn default() -> Self {
        Self {
            envs: [TimeEnv::default(); 6],
            velocity_pitch: [0; 6],
            filter_env: TimeEnv::default(),
            lfo_env: TimeEnv::default(),
            pitch_env: TimeEnv::default(),
            velocity: 1.0,
            key: CENTRE_KEY,
            random: 0.0,
            noise: 0x1234_5679,
            down: false,
            time_env: [false; 6],
            op_fb: [[0; 2]; 6],
            lfo_phase: 0.0,
            lfo_hold: 0.0,
            lfo_noise: 0.0,
            lfo: [0.0; N],
            filter: Multimode::default(),
            filter_cutoff: 0.0,
            filter_set: false,
            tone_z: 0.0,
        }
    }
}

impl NoteExt {
    /// Starts for a key going down, velocity 1..=127, with `random` from -1 to 1 for the matrix.
    pub fn start(&mut self, frame: &Frame, key: i32, velocity: i32, random: f32, ratios: &[i32; 6]) {
        let ext = frame.ext;
        let (key, velocity) = (key as f32, velocity as f32 / 127.0);
        self.key = key;
        self.velocity = velocity;
        self.random = random;
        self.noise = 0x1234_5679u32.wrapping_add((key as u32).wrapping_mul(97));
        self.down = true;
        self.op_fb = [[0; 2]; 6];
        for (op, o) in ext.ops.iter().enumerate() {
            if let Some(e) = self.envs.get_mut(op) {
                e.start(&o.env, velocity, key, true, &frame.clock);
            }
            if let Some(slot) = self.velocity_pitch.get_mut(op) {
                *slot = velocity_pitch(o, velocity, ratios.get(op).copied().unwrap_or(1));
            }
        }
        self.filter_env.start(&ext.filter.env, velocity, key, false, &frame.clock);
        self.lfo_env.start(&ext.lfo.env, velocity, key, false, &frame.clock);
        self.pitch_env.start(&ext.pitch.env, velocity, key, false, &frame.clock);
        if ext.lfo.retrigger {
            self.lfo_phase = 0.0;
        } else {
            let hertz = f64::from(rate_hertz(ext.lfo.range, ext.lfo.rate));
            self.lfo_phase = (frame.elapsed * hertz).fract();
        }
        self.filter.reset();
        self.filter_set = false;
        self.tone_z = 0.0;
    }

    pub fn key_up(&mut self) {
        self.down = false;
    }

    /// Whether operator `op` still sounds, by its time envelope; `None` when it runs the DX7 one.
    pub fn op_active(&self, op: usize) -> Option<bool> {
        if self.time_env.get(op).copied().unwrap_or(false) { self.envs.get(op).map(|e| !e.is_done()) } else { None }
    }

    /// Sets the block's modulation going and advances the envelopes and the LFO.
    pub fn begin_block(&mut self, frame: &Frame, carriers: &[bool; 6]) -> Mods {
        let ext = frame.ext;
        let mut mods = Mods::neutral();
        let seconds = N as f32 / frame.sample_rate.max(1.0);
        let routed = ext.has_routes();
        let any_time = ext.ops.iter().any(|o| o.time_env);
        self.time_env = std::array::from_fn(|op| ext.ops.get(op).is_some_and(|o| o.time_env));
        mods.kernel = ext.needs_kernel();
        if !routed
            && !any_time
            && !mods.kernel
            && !ext.lfo.on
            && !ext.pitch.on
            && !ext.filter.on
            && ext.ops.iter().all(|o| o.level_by_key == 0.0 && o.freq_by_velocity == 0.0)
        {
            return mods;
        }

        // Sources and the matrix; the LFO's own output comes from the last block's first value.
        let lfo_now = self.lfo.first().copied().unwrap_or(0.0);
        if routed {
            let sources = [
                self.velocity,
                (self.key - CENTRE_KEY) / 60.0,
                frame.pressure,
                frame.bend,
                frame.wheel,
                frame.foot,
                frame.breath,
                lfo_now,
                self.random,
            ];
            for (row, value) in ext.matrix.iter().zip(sources) {
                for (to, amount) in row {
                    if let Some(slot) = mods.sums.get_mut(*to) {
                        *slot += value * *amount;
                    }
                }
            }
        }

        let time_scale = 10f32.powf(-(ext.time + mods.sum(dest::TIME)).clamp(-1.0, 1.0));
        for (op, o) in ext.ops.iter().enumerate() {
            let number = 5 - op;
            let mut offset = mods.sum(dest::OP_LEVEL + number) * LEVEL_RANGE_DB;
            offset += o.level_by_key * (self.key - CENTRE_KEY) / 12.0 * 6.0;
            if number == 0 {
                offset += (-mods.sum(dest::CROSSFADE_1_3)).min(0.0) * LEVEL_RANGE_DB;
            } else if number == 2 {
                offset += mods.sum(dest::CROSSFADE_1_3).min(0.0) * LEVEL_RANGE_DB;
            } else if number == 1 {
                offset += (-mods.sum(dest::CROSSFADE_2_4)).min(0.0) * LEVEL_RANGE_DB;
            } else if number == 3 {
                offset += mods.sum(dest::CROSSFADE_2_4).min(0.0) * LEVEL_RANGE_DB;
            }
            if !carriers.get(op).copied().unwrap_or(false) {
                offset += mods.sum(dest::FM_AMOUNT) * LEVEL_RANGE_DB;
            }
            if let Some(slot) = mods.level_db.get_mut(op) {
                *slot = offset;
            }
            if let Some(e) = self.envs.get_mut(op) {
                let value = e.next(&o.env, seconds, time_scale, self.down, &frame.clock);
                if o.time_env
                    && let Some(slot) = mods.time_db.get_mut(op)
                {
                    *slot = Some(value);
                }
            }
            if let (Some(slot), Some(from)) = (mods.pitch.get_mut(op), self.velocity_pitch.get(op)) {
                *slot = *from;
            }
        }
        mods.feedback = mods.sum(dest::FEEDBACK);
        mods.fixed = (mods.sum(dest::FIXED) * FIXED_OCTAVES * (1u32 << 24) as f32) as i32;

        // The pitch envelope.
        let mut lfo_pitch = 1.0f32;
        if ext.pitch.on {
            let semitones = self.pitch_env.next(&ext.pitch.env, seconds, time_scale, self.down, &frame.clock);
            let amount = (ext.pitch.amount + mods.sum(dest::PITCH_ENV)).clamp(-1.0, 1.0);
            let q24 = (semitones * amount * (1u32 << 24) as f32 / 12.0) as i32;
            for (slot, to) in mods.pitch.iter_mut().zip(ext.pitch.to_op) {
                if to {
                    *slot = slot.wrapping_add(q24);
                }
            }
            if ext.pitch.to_lfo {
                lfo_pitch = 2f32.powf(semitones * amount / 12.0);
            }
            if let Some(slot) = mods.sums.get_mut(ext.pitch.dest_b) {
                *slot += semitones / 48.0 * ext.pitch.amount_b;
            }
        }

        // The LFO, sample by sample.
        if ext.lfo.on {
            let envelope = self.lfo_env.next(&ext.lfo.env, seconds, time_scale, self.down, &frame.clock);
            let by_velocity = 1.0 + ext.lfo.by_velocity * (self.velocity - 1.0);
            let amount = (ext.lfo.amount + mods.sum(dest::LFO_AMOUNT)).clamp(-1.0, 1.0) * by_velocity * envelope;
            let rate_scale =
                2f32.powf(mods.sum(dest::LFO_RATE) * 4.0 + ext.lfo.by_key * (self.key - CENTRE_KEY) / 12.0);
            let hertz = if ext.lfo.range == 2 {
                f64::from(ext.lfo.sync_beats).recip() * frame.clock.tempo / 60.0
            } else {
                f64::from(rate_hertz(ext.lfo.range, ext.lfo.rate) * rate_scale * lfo_pitch)
            };
            let step = hertz / f64::from(frame.sample_rate.max(1.0));
            let wave = ext.lfo.wave;
            for out in self.lfo.iter_mut() {
                let before = self.lfo_phase;
                self.lfo_phase = (self.lfo_phase + step).fract();
                let p = before as f32;
                let value = match wave {
                    1 => {
                        if p < 0.5 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                    2 => 4.0 * (p - (p + 0.5).floor()).abs() - 1.0,
                    3 => 1.0 - 2.0 * p,
                    4 => 2.0 * p - 1.0,
                    5 => {
                        if self.lfo_phase < before {
                            self.lfo_hold = next_random(&mut self.noise);
                        }
                        self.lfo_hold
                    }
                    6 => {
                        let coefficient = (step as f32 * 6.0).clamp(0.0, 1.0);
                        self.lfo_noise += (next_random(&mut self.noise) - self.lfo_noise) * coefficient;
                        self.lfo_noise
                    }
                    _ => (std::f32::consts::TAU * p).sin(),
                };
                *out = value * amount;
            }
            mods.lfo_block = self.lfo.first().copied().unwrap_or(0.0);
            for (slot, to) in mods.lfo_depth.iter_mut().zip(ext.lfo.to_op) {
                if to {
                    *slot = 1.0;
                }
            }
            if amount != 0.0 && ext.lfo.to_op.iter().any(|t| *t) {
                mods.kernel = true;
            }
            if let Some(slot) = mods.sums.get_mut(ext.lfo.dest_b) {
                *slot += mods.lfo_block * ext.lfo.amount_b;
            }
        }
        if mods.feedback > 0.0 {
            mods.kernel = true;
        }

        if ext.filter.on {
            let value = self.filter_env.next(&ext.filter.env, seconds, time_scale, self.down, &frame.clock);
            mods.filter_env = value;
        }
        let volume = mods.sum(dest::VOLUME);
        mods.gain = if volume != 0.0 { 10f32.powf(volume.clamp(-1.0, 0.25) * LEVEL_RANGE_DB / 20.0) } else { 1.0 };
        mods.pan = mods.sum(dest::PAN);
        mods
    }

    /// The note's block after the engine: filter, shaper and modulated tone, ready to mix.
    pub fn finish(&mut self, input: &[i32; N], frame: &Frame, mods: &Mods) -> [f32; N] {
        let mut out = [0.0f32; N];
        for (o, &v) in out.iter_mut().zip(input) {
            *o = (v as f32 / FULL_SCALE).clamp(-1.0, 1.0);
        }
        let ext = frame.ext;
        if ext.filter.on {
            self.run_filter(&mut out, frame, mods);
        }
        let tone = mods.sum(dest::TONE);
        if tone != 0.0 {
            // Modulated tone: one pole per note, closed as the modulation falls.
            let cutoff = 200.0 * 100f32.powf((1.0 + tone.min(0.0)).clamp(0.0, 1.0));
            let a = (1.0 - (-std::f32::consts::TAU * cutoff / frame.sample_rate.max(1.0)).exp()).clamp(0.0, 1.0);
            for o in &mut out {
                self.tone_z += (*o - self.tone_z) * a;
                *o = self.tone_z;
            }
        }
        out
    }

    fn run_filter(&mut self, out: &mut [f32; N], frame: &Frame, mods: &Mods) {
        let f = &frame.ext.filter;
        let by_velocity = f.by_velocity * (self.velocity - 0.5) * 2.0 * 4.0;
        let by_key = f.by_key * (self.key - CENTRE_KEY) / 12.0;
        let env_amount = (f.env_amount + mods.sum(dest::FILTER_ENV)).clamp(-1.0, 1.0);
        let moved = env_amount * mods.filter_env * FILTER_ENV_OCTAVES
            + by_velocity
            + by_key
            + mods.sum(dest::FILTER_FREQ) * FILTER_ENV_OCTAVES;
        let target = f.frequency * 2f32.powf(moved);
        let resonance = (f.resonance + mods.sum(dest::FILTER_RES)).clamp(0.0, 1.0);
        let q = 0.5 + 24.0 * resonance * resonance;
        let morph = (f.morph + mods.sum(dest::FILTER_MORPH)).rem_euclid(1.0);
        let drive_db = (f.drive_db + mods.sum(dest::FILTER_DRIVE) * 24.0).clamp(0.0, 36.0);
        let drive = 10f32.powf(drive_db / 20.0);
        let lfo_to_filter = frame.ext.lfo.on && frame.ext.lfo.to_filter;
        let from = if self.filter_set { self.filter_cutoff } else { target };
        let mut settings = MultimodeSettings {
            sample_rate: f64::from(frame.sample_rate),
            response: f.response,
            steep: f.steep,
            frequency: f64::from(target),
            q: f64::from(q),
            morph: f64::from(morph),
            circuit: f.circuit,
            drive: f64::from(drive),
        };
        for (chunk, part) in out.chunks_mut(FILTER_STEP).enumerate() {
            let t = (chunk + 1) as f32 * FILTER_STEP as f32 / N as f32;
            let mut cutoff = from * (target / from.max(1.0)).powf(t.min(1.0));
            if lfo_to_filter {
                let lfo = self.lfo.get(chunk * FILTER_STEP).copied().unwrap_or(0.0);
                cutoff *= 2f32.powf(lfo * FILTER_LFO_OCTAVES);
            }
            settings.frequency = f64::from(cutoff);
            self.filter.set(&settings);
            for s in part {
                *s = self.filter.process(f64::from(*s)) as f32;
            }
        }
        self.filter_cutoff = target;
        self.filter_set = true;

        if f.shaper != 0 && f.shaper_mix > 0.0 {
            let drive_db = (f.shaper_drive_db + mods.sum(dest::SHAPER_DRIVE) * 24.0).clamp(0.0, 36.0);
            let drive = 10f32.powf(drive_db / 20.0);
            for s in out.iter_mut() {
                let x = *s * drive;
                let shaped = match f.shaper {
                    1 => x.tanh(),
                    2 => x.clamp(-1.0, 1.0),
                    3 => (x * std::f32::consts::FRAC_PI_2).sin(),
                    _ => suhina_dsp::shape::sine_fold(f64::from(x)) as f32,
                };
                *s += (shaped - *s) * f.shaper_mix;
            }
        }
    }
}

/// The pitch an operator gains from velocity, Q24 per octave. Quantized, the ratio moves in whole
/// steps of the operator's coarse value.
fn velocity_pitch(o: &OpExt, velocity: f32, coarse: i32) -> i32 {
    if o.freq_by_velocity == 0.0 {
        return 0;
    }
    let coarse = coarse.max(1) as f32;
    let mut ratio = coarse * (1.0 + o.freq_by_velocity * velocity * 2.0).max(0.1);
    if o.quantize {
        ratio = ratio.round().max(1.0);
    }
    ((ratio / coarse).log2() * (1u32 << 24) as f32) as i32
}

/// Whether `db` from a time envelope is silence.
pub fn is_silent(db: f32) -> bool {
    db <= SILENCE_DB
}
