// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Auto-pan: an LFO that turns each side's level down and up. With the two sides half a cycle
//! apart (the default) the sound moves between left and right; with no phase difference both
//! sides move together, which is tremolo.
//!
//! The LFO runs freely in hertz, or follows the song position in note lengths, so a synced LFO
//! is at the same point of its cycle at the same beat however playback got there. When the
//! transport is stopped a synced LFO keeps running at the tempo. A bar is four beats.
//!
//! Each side's gain is `1 - amount * u`, where `u` is the LFO scaled to 0..1 (after shaping and
//! inversion). The gain goes through a 2 ms one-pole filter, so square and random shapes and
//! jumps in phase do not click.

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, ParamBank, ParamInfo, ProcessContext, Role, Scale, Unit,
    check_sample_rate,
};
use suhina_dsp::lfo::{Shape, wave};
use suhina_dsp::sync::LfoClock;
use suhina_dsp::{OnePole, Smoother};

/// Ramp for changes of the amount.
pub const RAMP_SECONDS: f64 = 0.02;
/// Time constant of the gain filter that softens edges.
pub const EDGE_SECONDS: f64 = 0.002;

/// Parameter indices, in [`INFO`] order.
pub mod param {
    pub const AMOUNT: usize = 0;
    pub const SYNC: usize = 1;
    pub const RATE: usize = 2;
    pub const SYNCED_RATE: usize = 3;
    pub const SHAPE: usize = 4;
    pub const PHASE: usize = 5;
    pub const OFFSET: usize = 6;
    pub const SHAPING: usize = 7;
    pub const INVERT: usize = 8;
    pub(crate) const COUNT: usize = 9;
}

/// Options of the synced rate, and their length in beats.
const SYNCED_RATES: [(&str, f64); 15] = [
    ("8 Bars", 32.0),
    ("4 Bars", 16.0),
    ("2 Bars", 8.0),
    ("1 Bar", 4.0),
    ("1/2", 2.0),
    ("1/4 Dotted", 1.5),
    ("1/2 Triplet", 4.0 / 3.0),
    ("1/4", 1.0),
    ("1/8 Dotted", 0.75),
    ("1/4 Triplet", 2.0 / 3.0),
    ("1/8", 0.5),
    ("1/8 Triplet", 1.0 / 3.0),
    ("1/16", 0.25),
    ("1/16 Triplet", 1.0 / 6.0),
    ("1/32", 0.125),
];
const SYNCED_RATE_NAMES: [&str; 15] = {
    let mut names = [""; 15];
    let mut i = 0;
    while i < SYNCED_RATES.len() {
        names[i] = SYNCED_RATES[i].0;
        i += 1;
    }
    names
};
/// Index of "1/4" in [`SYNCED_RATES`].
const DEFAULT_SYNCED_RATE: usize = 7;
const SHAPE_NAMES: [&str; 6] = ["Sine", "Triangle", "Saw Up", "Saw Down", "Square", "Random"];

const PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::continuous("amount", "Amount", (0.0, 1.0), 0.5, Unit::Percent, Scale::Linear),
    ParamInfo::toggle("sync", "Sync", false),
    ParamInfo::continuous("rate", "Rate", (0.01, 40.0), 1.0, Unit::Hertz, Scale::Logarithmic),
    ParamInfo::choice("syncedRate", "Synced Rate", &SYNCED_RATE_NAMES, DEFAULT_SYNCED_RATE),
    ParamInfo::choice("shape", "Shape", &SHAPE_NAMES, 0),
    // In cycles: 0.5 puts the sides half a cycle apart (panning), 0 moves them together.
    ParamInfo::continuous("phase", "Stereo Phase", (0.0, 1.0), 0.5, Unit::None, Scale::Linear),
    // In cycles: where in its cycle the LFO is at time (or beat) zero.
    ParamInfo::continuous("offset", "Offset", (0.0, 1.0), 0.0, Unit::None, Scale::Linear),
    // Pushes the waveform towards a square.
    ParamInfo::continuous("shaping", "Shaping", (0.0, 1.0), 0.0, Unit::Percent, Scale::Linear),
    ParamInfo::toggle("invert", "Invert", false),
];

pub const INFO: DeviceInfo = DeviceInfo { id: "auto-pan", name: "Auto Pan", role: Role::Effect, params: &PARAMS };

/// How much shaping at 1.0 steepens the waveform before it is clipped.
const MAX_SHAPING_GAIN: f64 = 8.0;

pub struct AutoPan {
    sample_rate: f64,
    params: ParamBank<{ param::COUNT }>,
    amount: Smoother,
    edges: [OnePole; 2],
    /// Whether the edge filters still have to start at the first gain after a reset.
    fresh: bool,
    /// Free-running position: whole cycles and the fraction of the current one.
    free_cycles: u64,
    free_phase: f64,
}

impl Default for AutoPan {
    fn default() -> Self {
        Self::new()
    }
}

impl AutoPan {
    pub fn new() -> Self {
        let sr = DEFAULT_SAMPLE_RATE;
        let params = ParamBank::new(&PARAMS);
        let amount = Smoother::new(params.get(param::AMOUNT) as f32, Smoother::ramp_frames(RAMP_SECONDS, sr));
        let mut pan = Self {
            sample_rate: sr,
            params,
            amount,
            edges: [OnePole::new(EDGE_SECONDS, sr); 2],
            fresh: true,
            free_cycles: 0,
            free_phase: 0.0,
        };
        pan.reset();
        pan
    }

    fn value(&self, index: usize) -> f64 {
        self.params.get(index)
    }

    fn shape(&self) -> Shape {
        Shape::ALL.get(self.value(param::SHAPE) as usize).copied().unwrap_or(Shape::Sine)
    }

    /// Length of one synced cycle in beats.
    fn synced_beats(&self) -> f64 {
        SYNCED_RATES.get(self.value(param::SYNCED_RATE) as usize).map_or(1.0, |r| r.1)
    }

    /// One side's level reduction, 0..1, at LFO position `cycles`.
    fn depth(&self, shape: Shape, cycles: f64) -> f64 {
        let mut v = wave(shape, cycles);
        if self.value(param::INVERT) >= 0.5 {
            v = -v;
        }
        let steepness = 1.0 + self.value(param::SHAPING) * (MAX_SHAPING_GAIN - 1.0);
        ((v * steepness).clamp(-1.0, 1.0) + 1.0) / 2.0
    }
}

impl Device for AutoPan {
    fn info(&self) -> &'static DeviceInfo {
        &INFO
    }

    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        let sr = check_sample_rate(sample_rate)?;
        self.sample_rate = sr;
        self.amount.set_ramp(Smoother::ramp_frames(RAMP_SECONDS, sr));
        self.edges = [OnePole::new(EDGE_SECONDS, sr); 2];
        self.reset();
        Ok(())
    }

    fn reset(&mut self) {
        self.amount.reset(self.amount.target());
        self.fresh = true;
        self.free_cycles = 0;
        self.free_phase = 0.0;
    }

    fn set_param(&mut self, index: usize, value: f64) {
        let Some(value) = self.params.set(index, value) else { return };
        if index == param::AMOUNT {
            self.amount.set_target(value as f32);
        }
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    fn process(&mut self, context: &ProcessContext, buffer: &mut [f32]) {
        let synced = self.value(param::SYNC) >= 0.5;
        let clock = LfoClock::new(
            context.tempo_or_default(),
            self.sample_rate,
            synced.then(|| self.synced_beats()),
            self.value(param::RATE),
        )
        .following(context.playing, context.beat);
        let cycles_per_frame = clock.cycles_per_frame();
        let (shape, offset, phase) = (self.shape(), self.value(param::OFFSET), self.value(param::PHASE));

        for (n, [left, right]) in buffer.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let position = clock.song_cycles(n).unwrap_or(self.free_cycles as f64 + self.free_phase);
            let amount = f64::from(self.amount.next_value());
            let targets = [
                1.0 - amount * self.depth(shape, position + offset),
                1.0 - amount * self.depth(shape, position + offset + phase),
            ];
            if self.fresh {
                for (edge, target) in self.edges.iter_mut().zip(targets) {
                    edge.reset(target);
                }
                self.fresh = false;
            }
            let [edge_left, edge_right] = &mut self.edges;
            *left *= edge_left.process(targets[0]) as f32;
            *right *= edge_right.process(targets[1]) as f32;

            if !clock.follows_song() {
                self.free_phase += cycles_per_frame;
                if self.free_phase >= 1.0 {
                    self.free_cycles = self.free_cycles.wrapping_add(self.free_phase as u64) % (1 << 32);
                    self.free_phase = self.free_phase.fract();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
