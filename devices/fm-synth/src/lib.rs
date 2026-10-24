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
//! frames (1.3 ms at 48 kHz). The engine is mono; both sides carry the same signal.
//!
//! Sixteen voices; a new note takes the voice already on its pitch, then a silent one, then the
//! oldest released one, then the oldest. The engine files carry their own notices (Apache-2.0);
//! the rest of this crate is Suhina's own.

mod core;
mod env;
mod lfo;
pub mod params;
pub mod patch;
pub mod tables;
mod voice;

#[cfg(test)]
mod tests;

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, NoteEvent, ParamBank, ProcessContext, Role,
    check_sample_rate, controller,
};

use crate::core::Buses;
use crate::lfo::Lfo;
use crate::params::{COUNT, GLOBAL, OP_PARAMS, PARAMS, PERFORMANCE, op};
pub use crate::patch::Patch;
use crate::tables::{N, Tables};
use crate::voice::{Controllers, ModRoute, Note};

pub const INFO: DeviceInfo = DeviceInfo { id: "fm-synth", name: "FM Synth", role: Role::Instrument, params: &PARAMS };

/// Notes that can sound at once.
pub const VOICES: usize = 16;

/// One voice at full scale in the engine's output.
const FULL_SCALE: f32 = (1u32 << 28) as f32;

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
    block: [f32; N],
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
            block: [0.0; N],
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

    /// Loads a voice: every voice parameter takes its value (performance parameters stay).
    pub fn load_patch(&mut self, patch: &Patch) {
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
                _ => {}
            }
        }
    }

    /// The voice for a new note sounding `sounding`, in this order: the voice already sounding
    /// that pitch, so a repeated key neither doubles nor cancels itself; a silent voice; the
    /// released voice that started first; the voice that started first.
    fn pick_voice(&self, sounding: i32) -> usize {
        let first_started = |wanted: &dyn Fn(&Slot) -> bool| {
            self.slots.iter().enumerate().filter(|(_, s)| wanted(s)).min_by_key(|(_, s)| s.seq).map(|(i, _)| i)
        };
        first_started(&|s| s.live && s.note.is_playing() && s.sounding == sounding)
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
        let Some(slot) = self.slots.get_mut(index) else { return };
        // A voice taken over while it sounds keeps its phases, so the change does not click.
        let was_sounding = slot.live && slot.note.is_playing();
        *slot = Slot { key, sounding, velocity, down: true, sustained: sustain, live: true, seq, note: slot.note };
        slot.note.init(&self.patch, sounding, velocity, &self.tables);
        if sync && !was_sounding {
            slot.note.sync_oscillators();
        }
        if let Some(from) = glide_from {
            slot.note.take_portamento(&from);
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
                slot.note.key_up(tables);
            }
        }
    }

    fn set_sustain(&mut self, down: bool) {
        self.sustain = down;
        if !down {
            for slot in &mut self.slots {
                if slot.sustained && !slot.down {
                    slot.note.key_up(&self.tables);
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
                slot.note.key_up(&self.tables);
            }
        }
    }

    fn render_block(&mut self) {
        if self.changed {
            for slot in self.slots.iter_mut().filter(|s| s.live) {
                slot.note.retune(&self.patch, slot.sounding, slot.velocity, &self.tables);
            }
            self.changed = false;
        }
        let lfo = self.lfo.next(&self.tables);
        let delay = self.lfo.delay();
        let mut sum = [0.0f32; N];
        for slot in self.slots.iter_mut().filter(|s| s.live && s.note.is_playing()) {
            let mut out = [0i32; N];
            slot.note.compute(&mut out, lfo, delay, &self.controllers, &mut self.buses, &self.tables);
            for (s, v) in sum.iter_mut().zip(out) {
                *s += (v as f32 / FULL_SCALE).clamp(-1.0, 1.0);
            }
        }
        let start = self.gain;
        let step = (self.target_gain - start) / N as f32;
        for (i, (out, s)) in self.block.iter_mut().zip(sum).enumerate() {
            *out = s * (start + step * (i + 1) as f32);
        }
        self.gain = self.target_gain;
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
        self.block = [0.0; N];
        self.position = N;
        self.gain = self.target_gain;
        self.changed = false;
    }

    fn set_param(&mut self, index: usize, value: f64) {
        let Some(value) = self.params.set(index, value) else { return };
        self.apply(index, value);
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    fn process(&mut self, _: &ProcessContext, buffer: &mut [f32]) {
        for frame in buffer.as_chunks_mut::<2>().0 {
            if self.position >= N {
                self.render_block();
                self.position = 0;
            }
            let s = self.block.get(self.position).copied().unwrap_or(0.0);
            *frame = [s, s];
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
