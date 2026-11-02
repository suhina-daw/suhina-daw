// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! FM Synth: a six-operator FM instrument whose engine is a port of MSFA, the engine inside the
//! Dexed synthesizer (ROADMAP › Built-in effects and instruments).
//!
//! Six sine operators, each with a four-stage envelope, keyboard level and rate scaling,
//! velocity and amplitude-modulation sensitivity, ratio or fixed frequency and detune, connect
//! through one of 32 algorithms, one operator with feedback; the pitch envelope, one LFO and
//! four controllers (wheel, foot, breath, pressure) shape the sound. Voices are the DX7 voice
//! format, so DX7 voice and bank system-exclusive files load ([`patch::read`]).
//!
//! The engine works in blocks of 64 frames ([`tables::N`]) on a grid that runs from the last
//! reset: a note, a controller or a parameter change takes effect at the next block boundary,
//! so the output never depends on how the host divides a stream, and timing is exact to 64
//! frames (1.3 ms at 48 kHz). Notes sum to a stereo pair: each has its pan, and spread plays a
//! second, detuned note panned away from the first.
//!
//! Sixteen voices by default, up to 32; at one, notes play legato. A new note takes the voice already on its pitch, then a silent one, then the
//! oldest released one, then the oldest. The engine files carry their own notices (Apache-2.0);
//! the rest of this crate is Suhina's own.

mod core;
mod env;
mod ext;
mod kernel;
mod lfo;
pub mod params;
pub mod patch;
pub mod random;
pub mod tables;
mod time_env;
mod voice;
mod wave;

#[cfg(test)]
mod tests;

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, NoteEvent, ParamBank, ProcessContext, Role,
    check_sample_rate, controller,
};
use suhina_dsp::{FirstOrder, FirstOrderCoefficients};

use crate::core::Buses;
use crate::ext::{Ext, Frame};
use crate::lfo::Lfo;
use crate::params::{COUNT, EXTENDED, GLOBAL, OP_PARAMS, PARAMS, PERFORMANCE, op};
pub use crate::patch::Patch;
use crate::tables::{N, Tables};
use crate::time_env::Clock;
use crate::voice::{Controllers, ModRoute, Note};
use crate::wave::DrawnWave;

pub const INFO: DeviceInfo = DeviceInfo { id: "fm-synth", name: "FM Synth", role: Role::Instrument, params: &PARAMS };

/// The most notes that can sound at once; the Voices parameter chooses how many do.
pub const VOICES: usize = 32;
/// How far spread at full detunes each of a note's two sides: 20 cents, Q24 per octave.
const SPREAD_DETUNE: f32 = (1u32 << 24) as f32 * 20.0 / 1200.0;
/// Where random pan starts after a reset.
const RANDOM_SEED: u32 = 0x9e37_79b9;

/// A voice and the key it plays.
#[derive(Debug, Clone, Copy)]
struct Slot {
    note: Note,
    /// The key as played, before transposition: what a release is matched against.
    key: u8,
    /// The key that sounds, after transposition.
    sounding: i32,
    velocity: i32,
    down: bool,
    sustained: bool,
    /// Started since the last reset.
    live: bool,
    /// Order of key presses, for stealing the oldest.
    seq: i64,
    /// The detuned second note, played while `spread` holds.
    twin: Note,
    spread: bool,
    /// Left and right gains of `note` and of `twin`, read when the note started.
    gains: [f32; 2],
    twin_gains: [f32; 2],
}

impl Slot {
    fn release(&mut self, tables: &Tables) {
        self.note.key_up(tables);
        if self.spread {
            self.twin.key_up(tables);
        }
    }
}

/// Left and right gains for `pan` (-1 to 1): the far side falls, the centre is untouched.
fn balance(pan: f32) -> [f32; 2] {
    let pan = pan.clamp(-1.0, 1.0);
    [(1.0 - pan).min(1.0), (1.0 + pan).min(1.0)]
}

/// Adds a block of a note to a stereo sum at `gains`, with the block's own gain and pan.
fn mix(sum: &mut [[f32; 2]; N], out: &[f32; N], gains: [f32; 2], gain: f32, pan: f32) {
    let gains = if pan == 0.0 && gain == 1.0 {
        gains
    } else {
        let moved = balance(pan);
        [gains[0] * moved[0] * gain, gains[1] * moved[1] * gain]
    };
    for (s, &v) in sum.iter_mut().zip(out) {
        s[0] += v * gains[0];
        s[1] += v * gains[1];
    }
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            note: Note::default(),
            key: 0,
            sounding: -1,
            velocity: 0,
            down: false,
            sustained: false,
            live: false,
            seq: -1,
            twin: Note::default(),
            spread: false,
            gains: [1.0; 2],
            twin_gains: [1.0; 2],
        }
    }
}

pub struct FmSynth {
    params: ParamBank<COUNT>,
    patch: Patch,
    controllers: Controllers,
    tables: Box<Tables>,
    slots: [Slot; VOICES],
    next_seq: i64,
    last_slot: Option<usize>,
    lfo: Lfo,
    sustain: bool,
    buses: Buses,
    block: [[f32; 2]; N],
    sample_rate: f64,
    voices: usize,
    spread: f32,
    pan_by_key: f32,
    pan_random: f32,
    /// For random pan; starts over at a reset, so a render repeats.
    random: u32,
    pan: [f32; 2],
    target_pan: [f32; 2],
    /// Below 1, a low-pass over the sum.
    tone: f64,
    tone_filter: [FirstOrder; 2],
    /// What the voice file does not hold.
    ext: Box<Ext>,
    /// By engine order, as [`Ext::ops`].
    drawn: Vec<DrawnWave>,
    /// A key on an already sounding pitch restarts that voice rather than taking another.
    retrigger: bool,
    clock: Clock,
    /// Seconds since the last reset.
    elapsed: f64,
    position: usize,
    gain: f32,
    target_gain: f32,
    /// The voice or the LFO changed: sounding notes follow at the next block.
    changed: bool,
}

impl Default for FmSynth {
    fn default() -> Self {
        Self::new()
    }
}

impl FmSynth {
    pub fn new() -> Self {
        wave::prepare_bank();
        let mut synth = Self {
            params: ParamBank::new(&PARAMS),
            patch: Patch::default(),
            controllers: Controllers::default(),
            tables: Box::new(Tables::new(DEFAULT_SAMPLE_RATE)),
            slots: [Slot::default(); VOICES],
            next_seq: 0,
            last_slot: None,
            lfo: Lfo::default(),
            sustain: false,
            buses: Buses::default(),
            block: [[0.0; 2]; N],
            sample_rate: DEFAULT_SAMPLE_RATE,
            voices: 16,
            spread: 0.0,
            pan_by_key: 0.0,
            pan_random: 0.0,
            random: RANDOM_SEED,
            pan: [1.0; 2],
            target_pan: [1.0; 2],
            tone: 1.0,
            tone_filter: [FirstOrder::new(FirstOrderCoefficients::lowpass(DEFAULT_SAMPLE_RATE, 20_000.0)); 2],
            ext: Box::default(),
            drawn: vec![DrawnWave::new(); 6],
            retrigger: true,
            clock: Clock::default(),
            elapsed: 0.0,
            position: N,
            gain: 1.0,
            target_gain: 1.0,
            changed: false,
        };
        for (i, value) in (*synth.params.values()).into_iter().enumerate() {
            synth.apply(i, value);
        }
        synth.reset();
        synth
    }

    /// The current voice, with the name it was loaded with.
    pub fn patch(&self) -> Patch {
        self.patch
    }

    /// Loads a voice: every voice parameter takes its value, what a voice file does not hold
    /// goes back to neutral (performance parameters stay).
    pub fn load_patch(&mut self, patch: &Patch) {
        for (i, info) in PARAMS.iter().enumerate().take(params::PANEL).skip(EXTENDED) {
            if i != params::VOICE_COUNT {
                self.set_param(i, info.default);
            }
        }
        for op_index in 0..6 {
            let stored = 5 - op_index;
            let o = patch.op(stored);
            let fields = [
                o.rates[0],
                o.rates[1],
                o.rates[2],
                o.rates[3],
                o.levels[0],
                o.levels[1],
                o.levels[2],
                o.levels[3],
                o.break_point,
                o.left_depth,
                o.right_depth,
                o.left_curve,
                o.right_curve,
                o.rate_scaling,
                o.amp_mod_sens,
                o.velocity_sens,
                o.level,
                i32::from(o.fixed),
                o.coarse,
                o.fine,
                o.detune - 7,
                i32::from(o.on),
            ];
            for (field, value) in fields.into_iter().enumerate() {
                self.set_param(op_index * OP_PARAMS + field, f64::from(value));
            }
        }
        for g in 0..params::GLOBAL_COUNT {
            let index = patch::PITCH_RATE + g;
            let value = match index {
                patch::ALGORITHM => patch.byte(index) + 1,
                patch::TRANSPOSE => patch.byte(index) - 24,
                _ => patch.byte(index),
            };
            self.set_param(GLOBAL + g, f64::from(value));
        }
        self.patch.set_name(&patch.name());
    }

    /// Writes parameter `index` (already clamped) into the voice or the performance state.
    fn apply(&mut self, index: usize, value: f64) {
        let byte = |v: f64| v.round().clamp(0.0, 255.0) as u8;
        if index < GLOBAL {
            let (op_index, field) = (index / OP_PARAMS, index % OP_PARAMS);
            let stored = 5 - op_index;
            if field == op::ON {
                let bit = 1u8 << stored;
                let switch = self.patch.byte(patch::OP_SWITCH) as u8;
                self.patch.set(patch::OP_SWITCH, if value >= 0.5 { switch | bit } else { switch & !bit });
            } else {
                let value = if field == op::DETUNE { value + 7.0 } else { value };
                self.patch.set(stored * 21 + field, byte(value));
            }
            self.changed = true;
        } else if index < PERFORMANCE {
            let at = patch::PITCH_RATE + (index - GLOBAL);
            let value = match at {
                patch::ALGORITHM => value - 1.0,
                patch::TRANSPOSE => value + 24.0,
                _ => value,
            };
            self.patch.set(at, byte(value));
            // The LFO is shared and a key going down reads its sync setting, so it follows at
            // once; its phase moves only when a block is rendered.
            self.lfo.set(self.patch.lfo(), &self.tables);
            self.changed = true;
        } else {
            let c = &mut self.controllers;
            let on = value >= 0.5;
            match index {
                params::PITCH_BEND_RANGE => c.bend_range = value as i32,
                params::PITCH_BEND_STEP => c.bend_step = value as i32,
                params::PORTAMENTO => c.portamento = on,
                params::GLISSANDO => c.glissando = on,
                params::PORTAMENTO_TIME => {
                    c.portamento_time = (value * 127.0 / 99.0).round().clamp(0.0, 127.0) as usize
                }
                params::VOLUME => self.target_gain = 10f32.powf(value as f32 / 20.0),
                params::PAN => self.target_pan = balance(value as f32),
                params::PAN_BY_KEY => self.pan_by_key = value as f32,
                params::PAN_RANDOM => self.pan_random = value as f32,
                params::SPREAD => self.spread = value as f32,
                params::VOICE_COUNT => self.voices = (value as usize).clamp(1, VOICES),
                params::RETRIGGER => self.retrigger = on,
                params::TONE => {
                    self.tone = value;
                    self.set_tone_filter();
                }
                i if (params::ROUTES..params::VOLUME).contains(&i) => {
                    let (source, field) = ((i - params::ROUTES) / 4, (i - params::ROUTES) % 4);
                    let route: &mut ModRoute = match source {
                        0 => &mut c.wheel_route,
                        1 => &mut c.foot_route,
                        2 => &mut c.breath_route,
                        _ => &mut c.pressure_route,
                    };
                    match field {
                        0 => route.range = value as i32,
                        1 => route.pitch = on,
                        2 => route.amp = on,
                        _ => route.eg = on,
                    }
                    c.refresh();
                }
                i => self.ext.apply(i, value),
            }
        }
    }

    /// Tone from 0 to 1 is a low-pass from 200 Hz to 20 kHz; at 1 there is no filter.
    fn set_tone_filter(&mut self) {
        let frequency = (200.0 * 100f64.powf(self.tone)).min(self.sample_rate * 0.45);
        let coefficients = FirstOrderCoefficients::lowpass(self.sample_rate, frequency);
        for filter in &mut self.tone_filter {
            filter.set(coefficients);
        }
    }

    /// The note's number for the matrix's Random source; drawn only when it is used, so a render
    /// that does not use it keeps its random pan.
    fn next_note_random(&mut self) -> f32 {
        if self.ext.matrix.get(params::MATRIX_SOURCES - 1).is_some_and(|r| r.iter().any(|(to, _)| *to != 0)) {
            self.next_random()
        } else {
            0.0
        }
    }

    /// The next number from -1 to 1 for random pan (xorshift).
    fn next_random(&mut self) -> f32 {
        let mut x = self.random;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.random = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// The voice for a new note sounding `sounding`, in this order: the voice already sounding
    /// that pitch, so a repeated key neither doubles nor cancels itself; a silent voice; the
    /// released voice that started first; the voice that started first.
    fn pick_voice(&self, sounding: i32) -> usize {
        let first_started = |wanted: &dyn Fn(&Slot) -> bool| {
            self.slots
                .iter()
                .take(self.voices)
                .enumerate()
                .filter(|(_, s)| wanted(s))
                .min_by_key(|(_, s)| s.seq)
                .map(|(i, _)| i)
        };
        let retrigger = self.retrigger;
        first_started(&|s| retrigger && s.live && s.note.is_playing() && s.sounding == sounding)
            .or_else(|| first_started(&|s| !s.live || !s.note.is_playing()))
            .or_else(|| first_started(&|s| !s.down && !s.sustained))
            .or_else(|| first_started(&|_| true))
            .unwrap_or(0)
    }

    fn key_down(&mut self, key: u8, velocity: i32) {
        let sounding = (i32::from(key) + self.patch.transpose()).clamp(0, 127);
        // The LFO starts over with the first key of a phrase.
        if !self.slots.iter().any(|s| s.down) {
            self.lfo.key_down();
        }
        // One voice plays legato: a key pressed over a held one moves the pitch, the envelopes
        // run on.
        if self.voices == 1
            && let Some(slot) = self.slots.first_mut()
            && slot.live
            && slot.down
            && slot.note.is_playing()
        {
            slot.key = key;
            slot.sounding = sounding;
            slot.note.retune(&self.patch, sounding, slot.velocity, &self.tables);
            if slot.spread {
                slot.twin.retune(&self.patch, sounding, slot.velocity, &self.tables);
            }
            return;
        }
        let index = self.pick_voice(sounding);
        let glide = self.controllers.portamento && self.controllers.portamento_time > 0;
        // A glide starts from wherever the previous note's pitch is now.
        let glide_from = self
            .last_slot
            .filter(|&last| glide && last != index)
            .and_then(|last| self.slots.get(last))
            .filter(|s| s.sounding >= 0)
            .map(|s| s.note);
        let (sustain, seq, sync) = (self.sustain, self.next_seq, self.patch.osc_key_sync());
        self.next_seq += 1;
        let random = if self.pan_random > 0.0 { self.next_random() * self.pan_random } else { 0.0 };
        let note_pan = (f32::from(key) - 60.0) / 60.0 * self.pan_by_key + random;
        let spread = self.spread;
        let matrix_random = self.next_note_random();
        let frame = Frame {
            ext: &self.ext,
            drawn: &self.drawn,
            sample_rate: self.sample_rate as f32,
            clock: self.clock,
            elapsed: self.elapsed,
            pressure: 0.0,
            bend: 0.0,
            wheel: 0.0,
            foot: 0.0,
            breath: 0.0,
        };
        let Some(slot) = self.slots.get_mut(index) else { return };
        // A voice taken over while it sounds keeps its phases, so the change does not click.
        let was_sounding = slot.live && slot.note.is_playing();
        *slot = Slot {
            key,
            sounding,
            velocity,
            down: true,
            sustained: sustain,
            live: true,
            seq,
            note: slot.note,
            twin: slot.twin,
            spread: spread > 0.0,
            gains: balance(note_pan - spread),
            twin_gains: balance(note_pan + spread),
        };
        slot.note.init(&self.patch, sounding, velocity, &self.tables);
        slot.note.start_ext(&self.patch, &frame, sounding, velocity, matrix_random);
        slot.note.set_pitch_offset((spread * SPREAD_DETUNE) as i32);
        if slot.spread {
            slot.twin.init(&self.patch, sounding, velocity, &self.tables);
            slot.twin.start_ext(&self.patch, &frame, sounding, velocity, matrix_random);
            slot.twin.set_pitch_offset(-(spread * SPREAD_DETUNE) as i32);
        }
        if sync && !was_sounding {
            slot.note.sync_oscillators();
            slot.twin.sync_oscillators();
            slot.note.set_start_phases(&frame);
            slot.twin.set_start_phases(&frame);
        }
        if let Some(from) = glide_from {
            slot.note.take_portamento(&from);
            slot.twin.take_portamento(&from);
        }
        self.last_slot = Some(index);
    }

    fn key_up(&mut self, key: u8) {
        let sustain = self.sustain;
        let tables = &self.tables;
        if let Some(slot) = self.slots.iter_mut().find(|s| s.down && s.key == key) {
            slot.down = false;
            if sustain {
                slot.sustained = true;
            } else {
                slot.release(tables);
            }
        }
    }

    fn set_sustain(&mut self, down: bool) {
        self.sustain = down;
        if !down {
            for slot in &mut self.slots {
                if slot.sustained && !slot.down {
                    slot.release(&self.tables);
                    slot.sustained = false;
                }
            }
        }
    }

    fn all_notes_off(&mut self) {
        self.sustain = false;
        for slot in &mut self.slots {
            if slot.down || slot.sustained {
                slot.down = false;
                slot.sustained = false;
                slot.release(&self.tables);
            }
        }
    }

    fn render_block(&mut self) {
        if self.changed {
            for slot in self.slots.iter_mut().filter(|s| s.live) {
                slot.note.retune(&self.patch, slot.sounding, slot.velocity, &self.tables);
                if slot.spread {
                    slot.twin.retune(&self.patch, slot.sounding, slot.velocity, &self.tables);
                }
            }
            self.changed = false;
        }
        for (op, wave) in self.drawn.iter_mut().enumerate() {
            if self.ext.drawn_dirty.get(op).copied().unwrap_or(false)
                && let Some(o) = self.ext.ops.get(op)
                && (o.wave == wave::DRAWN)
            {
                let played = wave::played(&o.partials, &o.drawn);
                let settled = wave.follow(&played, o.drawn.normalize, false);
                if settled && let Some(flag) = self.ext.drawn_dirty.get_mut(op) {
                    *flag = false;
                }
            }
        }
        let frame = Frame {
            ext: &self.ext,
            drawn: &self.drawn,
            sample_rate: self.sample_rate as f32,
            clock: self.clock,
            elapsed: self.elapsed,
            pressure: self.controllers.pressure as f32 / 127.0,
            bend: (self.controllers.pitch_bend - 0x2000) as f32 / 8_192.0,
            wheel: self.controllers.wheel as f32 / 127.0,
            foot: self.controllers.foot as f32 / 127.0,
            breath: self.controllers.breath as f32 / 127.0,
        };
        let lfo = self.lfo.next(&self.tables);
        let delay = self.lfo.delay();
        let mut sum = [[0.0f32; 2]; N];
        for slot in self.slots.iter_mut().filter(|s| s.live && s.note.is_playing()) {
            let mut out = [0i32; N];
            slot.note.compute(&mut out, lfo, delay, &self.controllers, &mut self.buses, &self.tables, &frame);
            let (samples, gain, pan) = slot.note.finish(&out, &frame);
            mix(&mut sum, &samples, slot.gains, gain, pan);
            if slot.spread {
                let mut out = [0i32; N];
                slot.twin.compute(&mut out, lfo, delay, &self.controllers, &mut self.buses, &self.tables, &frame);
                let (samples, gain, pan) = slot.twin.finish(&out, &frame);
                mix(&mut sum, &samples, slot.twin_gains, gain, pan);
            }
        }
        if self.tone < 1.0 {
            let [left, right] = &mut self.tone_filter;
            for s in &mut sum {
                s[0] = left.process(f64::from(s[0])) as f32;
                s[1] = right.process(f64::from(s[1])) as f32;
            }
        }
        let start = self.gain;
        let step = (self.target_gain - start) / N as f32;
        let pan = self.pan;
        let pan_step = [(self.target_pan[0] - pan[0]) / N as f32, (self.target_pan[1] - pan[1]) / N as f32];
        for (i, (out, s)) in self.block.iter_mut().zip(sum).enumerate() {
            let t = (i + 1) as f32;
            let gain = start + step * t;
            *out = [s[0] * gain * (pan[0] + pan_step[0] * t), s[1] * gain * (pan[1] + pan_step[1] * t)];
        }
        self.gain = self.target_gain;
        self.pan = self.target_pan;
        self.elapsed += N as f64 / self.sample_rate;
    }

    /// Brings every drawn wave to what its operator asks for at once.
    fn settle_drawn(&mut self) {
        for (op, wave) in self.drawn.iter_mut().enumerate() {
            if let Some(o) = self.ext.ops.get(op)
                && self.ext.drawn_dirty.get(op).copied().unwrap_or(false)
                && o.wave == wave::DRAWN
            {
                wave.follow(&wave::played(&o.partials, &o.drawn), o.drawn.normalize, true);
                if let Some(flag) = self.ext.drawn_dirty.get_mut(op) {
                    *flag = false;
                }
            }
        }
    }
}

/// A 0..=1 controller value as a 7-bit MIDI value.
fn seven_bit(value: f32) -> i32 {
    (value.clamp(0.0, 1.0) * 127.0).round() as i32
}

impl Device for FmSynth {
    fn info(&self) -> &'static DeviceInfo {
        &INFO
    }

    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        let sample_rate = check_sample_rate(sample_rate)?;
        *self.tables = Tables::new(sample_rate);
        self.sample_rate = sample_rate;
        self.set_tone_filter();
        self.reset();
        Ok(())
    }

    fn reset(&mut self) {
        self.slots = [Slot::default(); VOICES];
        self.next_seq = 0;
        self.last_slot = None;
        self.lfo = Lfo::default();
        self.lfo.set(self.patch.lfo(), &self.tables);
        self.sustain = false;
        let c = &mut self.controllers;
        c.pitch_bend = 0x2000;
        c.wheel = 0;
        c.breath = 0;
        c.foot = 0;
        c.pressure = 0;
        c.refresh();
        self.buses = Buses::default();
        self.block = [[0.0; 2]; N];
        self.position = N;
        self.gain = self.target_gain;
        self.pan = self.target_pan;
        self.random = RANDOM_SEED;
        self.elapsed = 0.0;
        self.settle_drawn();
        for filter in &mut self.tone_filter {
            filter.reset();
        }
        self.changed = false;
    }

    fn set_param(&mut self, index: usize, value: f64) {
        let Some(value) = self.params.set(index, value) else { return };
        self.apply(index, value);
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    fn process(&mut self, context: &ProcessContext, buffer: &mut [f32]) {
        self.clock = Clock { tempo: context.tempo_or_default(), beat: context.beat, playing: context.playing };
        for frame in buffer.as_chunks_mut::<2>().0 {
            if self.position >= N {
                self.render_block();
                self.position = 0;
            }
            *frame = self.block.get(self.position).copied().unwrap_or([0.0; 2]);
            self.position += 1;
        }
    }

    fn event(&mut self, event: NoteEvent) {
        match event {
            NoteEvent::On { key, velocity } if key <= 127 && velocity.is_finite() => match seven_bit(velocity) {
                0 => self.key_up(key),
                v => self.key_down(key, v),
            },
            NoteEvent::Off { key, .. } if key <= 127 => self.key_up(key),
            NoteEvent::Controller { number, value } if value.is_finite() => {
                let v = seven_bit(value);
                let c = &mut self.controllers;
                match number {
                    controller::MODULATION => c.wheel = v,
                    controller::BREATH => c.breath = v,
                    controller::FOOT => c.foot = v,
                    controller::SUSTAIN => self.set_sustain(v > 63),
                    // All notes off.
                    123 => self.all_notes_off(),
                    _ => {}
                }
                self.controllers.refresh();
            }
            NoteEvent::PitchBend(value) if value.is_finite() => {
                self.controllers.pitch_bend =
                    (0x2000 + (value.clamp(-1.0, 1.0) * 8_192.0).round() as i32).clamp(0, 0x3fff);
            }
            NoteEvent::Pressure(value) if value.is_finite() => {
                self.controllers.pressure = seven_bit(value);
                self.controllers.refresh();
            }
            NoteEvent::AllNotesOff => self.all_notes_off(),
            _ => {}
        }
    }
}
