// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The NAM Amp Modeler: a signal through a trained NAM model (a `.nam` capture of an amp,
//! preamp or pedal), with an input trim, the EQ's shelves and mid peak, an HQ toggle, a trim
//! delay and an output trim.
//!
//! Per side ([docs/spec/nam-amp-and-cab.md]): the input trim, the EQ, — when the project's rate
//! differs from the model's — the resampler into the model's rate, the model, a DC blocker, the
//! resampler back, the trim delay and the output trim. The model runs on one side at a time;
//! the device keeps one running instance per side, fed by `nam-rs` (MIT), whose
//! `process_buffer` is allocation-free. Nothing on the audio path allocates.
//!
//! The stream runs in fixed chunks of [`CHUNK`] frames counted from a reset, and the output
//! trails the input by one chunk — [`CHUNK`], told through `latency()` — so a host's block
//! sizes never change the sound. A missing model, or one the runtime refuses, leaves the trims,
//! EQ and delay in place and the model silent, so the rest of the chain still works.

use std::sync::Arc;

use nam_rs::Model;
pub use nam_rs::NamModel;
use suhina_device_api::{
    Device, DeviceError, DeviceInfo, ParamInfo, ProcessContext, Role, Scale, Unit, check_sample_rate,
};
use suhina_dsp::biquad::BUTTERWORTH_Q;
use suhina_dsp::gain::db_to_gain;
use suhina_dsp::resampler::HALF_TAPS;
use suhina_dsp::{Biquad, Coefficients, DcBlocker, DelayLine, Resampler, Smoother};

/// Ramp for the trims and the delay.
pub const RAMP_SECONDS: f64 = 0.02;
/// Where the EQ's bass keeps its weight, and the treble its edge.
pub const BASS_SHELF_HERTZ: f64 = 100.0;
pub const TREBLE_SHELF_HERTZ: f64 = 4_000.0;
/// The mid band's width, in Q.
pub const MID_Q: f64 = 0.8;
/// The longest delay the knob sets, in seconds.
pub const MAX_DELAY_SECONDS: f64 = 0.02;
/// Frames of one fixed chunk — the schedule the stream runs on and the delay the device tells
/// the engine's compensation. A power of two.
const CHUNK: usize = 512;
/// Slots of the delivery ring: one chunk's run plus the resampler legs' lead, safe for any
/// rate pair the devices prepare with.
const RING: usize = 2 * CHUNK;

/// Parameter indices, in [`INFO`] order.
pub mod param {
    pub const INPUT_GAIN: usize = 0;
    pub const EQ_BASS: usize = 1;
    pub const EQ_MID: usize = 2;
    pub const EQ_TREBLE: usize = 3;
    pub const EQ_MID_FREQ: usize = 4;
    pub const EQ_ON: usize = 5;
    pub const DELAY: usize = 6;
    pub const HQ: usize = 7;
    pub const OUTPUT_VOLUME: usize = 8;
    pub(crate) const COUNT: usize = 9;
}

const PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::continuous("inputGain", "Gain", (-24.0, 24.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("eqBass", "Bass", (-12.0, 12.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("eqMid", "Mid", (-12.0, 12.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("eqTreble", "Treble", (-12.0, 12.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("eqMidFreq", "Mid Freq", (200.0, 2_000.0), 600.0, Unit::Hertz, Scale::Logarithmic),
    ParamInfo::toggle("eqOn", "EQ", true),
    ParamInfo::continuous("delay", "Delay", (0.0, 0.02), 0.0, Unit::Seconds, Scale::Linear),
    ParamInfo::toggle("hq", "HQ", false),
    ParamInfo::continuous("outputVolume", "Volume", (-24.0, 24.0), 0.0, Unit::Decibels, Scale::Linear),
];

pub const INFO: DeviceInfo =
    DeviceInfo { id: "nam-amp-modeler", name: "NAM Amp Modeler", role: Role::Effect, params: &PARAMS };

/// Reading `.nam` files.
pub mod model;

const DEFAULT_SAMPLE_RATE: f64 = 48_000.0;

/// One side's stages: the EQ's filters, the DC blocker and the two resampler legs. All cleared
/// by `rebuild`.
#[derive(Clone)]
struct Side {
    bass: Biquad,
    mid: Biquad,
    treble: Biquad,
    dc: DcBlocker,
    into_model: Resampler,
    out_of_model: Resampler,
}

impl Side {
    fn new(sample_rate: f64, model_rate: f64) -> Self {
        Self {
            bass: Biquad::new(Coefficients::low_shelf(sample_rate, BASS_SHELF_HERTZ, BUTTERWORTH_Q, 0.0)),
            mid: Biquad::new(Coefficients::peak(sample_rate, 600.0, MID_Q, 0.0)),
            treble: Biquad::new(Coefficients::high_shelf(sample_rate, TREBLE_SHELF_HERTZ, BUTTERWORTH_Q, 0.0)),
            dc: DcBlocker::new(model_rate.max(1.0)),
            into_model: Resampler::new(sample_rate, model_rate),
            out_of_model: Resampler::new(model_rate, sample_rate),
        }
    }
}

/// The NAM Amp Modeler. [`NamAmp::with_model`] runs a parsed model ([`NamModel`], shared with
/// the session's cache); without one the trims, EQ and delay still run, the model silent.
pub struct NamAmp {
    sample_rate: f64,
    values: [f64; param::COUNT],
    /// The parsed model, for `prepare` and `reset` rebuilds.
    model: Option<Arc<NamModel>>,
    /// What the parse records as the model's rate.
    model_rate: f64,
    /// The running instances, built at rebuild time (off the audio thread).
    running: [Option<Model>; 2],
    /// Whether the runtime refused the model: it stays silent, and the panel says so.
    model_refused: bool,
    sides: [Side; 2],
    /// One fixed chunk of frames going through the head stages, per side.
    chunk: [Box<[f32; CHUNK]>; 2],
    /// The readied outputs waiting for their delivery, a ring of [`RING`] frames.
    ready_left: Box<[f32; RING]>,
    ready_right: Box<[f32; RING]>,
    /// The stream positions of the next input frame and of the next produced output.
    in_pos: u64,
    produced_pos: u64,
    /// Staging per side: the chunk staged into the model's rate, the model's own f32 buffer,
    /// the DC's output, and the side's produced frames waiting for the tails.
    chunk_f64: [Vec<f64>; 2],
    model_input: [Vec<f64>; 2],
    model_io: [Vec<f32>; 2],
    dc_staged: [Vec<f64>; 2],
    stage_out: [Vec<f64>; 2],
    delays: [DelayLine; 2],
    /// The trims and the delay, all smoothed.
    input_gain: Smoother,
    delay_frames: Smoother,
    output_volume: Smoother,
}

impl Default for NamAmp {
    fn default() -> Self {
        Self::new()
    }
}

impl NamAmp {
    /// An amp modeler with no model.
    pub fn new() -> Self {
        Self::build(DEFAULT_SAMPLE_RATE, None)
    }

    /// An amp modeler that runs `model`.
    pub fn with_model(model: Arc<NamModel>) -> Self {
        Self::build(DEFAULT_SAMPLE_RATE, Some(model))
    }

    /// Everything for `sample_rate` and the parsed model. `prepare` and `reset` run it again;
    /// whatever clears the state, counters included, calls it.
    fn build(sample_rate: f64, model: Option<Arc<NamModel>>) -> Self {
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, sample_rate);
        let mut amp = Self {
            sample_rate,
            values: PARAMS.map(|p| p.default),
            model,
            model_rate: DEFAULT_SAMPLE_RATE,
            running: [None, None],
            model_refused: false,
            sides: [Side::new(sample_rate, DEFAULT_SAMPLE_RATE), Side::new(sample_rate, DEFAULT_SAMPLE_RATE)],
            chunk: [Box::new([0.0; CHUNK]), Box::new([0.0; CHUNK])],
            ready_left: Box::new([0.0; RING]),
            ready_right: Box::new([0.0; RING]),
            in_pos: 0,
            produced_pos: 0,
            chunk_f64: [vec![0.0; CHUNK], vec![0.0; CHUNK]],
            model_input: [vec![0.0; CHUNK], vec![0.0; CHUNK]],
            model_io: [vec![0.0; CHUNK], vec![0.0; CHUNK]],
            dc_staged: [vec![0.0; CHUNK], vec![0.0; CHUNK]],
            stage_out: [vec![0.0; CHUNK], vec![0.0; CHUNK]],
            delays: [DelayLine::new(sample_rate, MAX_DELAY_SECONDS), DelayLine::new(sample_rate, MAX_DELAY_SECONDS)],
            input_gain: Smoother::new(1.0, 1),
            delay_frames: Smoother::new(0.0, 1),
            output_volume: Smoother::new(1.0, 1),
        };
        amp.input_gain.reset(amp.input_target());
        amp.input_gain.set_ramp(ramp);
        amp.output_volume.reset(amp.output_target());
        amp.output_volume.set_ramp(ramp);
        amp.delay_frames.reset(0.0);
        amp.delay_frames.set_ramp(ramp);
        amp.rebuild();
        amp
    }

    /// The input trim's linear gain.
    fn input_target(&self) -> f32 {
        db_to_gain(self.value(param::INPUT_GAIN) as f32)
    }

    /// The output trim's linear gain.
    fn output_target(&self) -> f32 {
        db_to_gain(self.value(param::OUTPUT_VOLUME) as f32)
    }

    fn value(&self, index: usize) -> f64 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    /// The model's expected rate; a model absent keeps the project's.
    fn model_hertz(&self) -> f64 {
        if self.model.is_some() && self.model_rate > 0.0 { self.model_rate } else { self.sample_rate }
    }

    /// Whether the project's rate is the model's, so the resampler legs stay out.
    fn rates_match(&self) -> bool {
        self.model.is_some() && self.model_rate == self.sample_rate
    }

    /// HQ's width dial where the model has one: off = the first (narrowest) sub-model, on = the
    /// last (widest). Plain WaveNet and LSTM models are untouched.
    fn apply_hq(&mut self) {
        let wide = self.value(param::HQ) >= 0.5;
        for slot in self.running.iter_mut().flatten() {
            if let Some(slim) = slot.as_slimmable_mut() {
                let widest = slim.len().saturating_sub(1);
                slim.select(if wide { widest } else { 0 });
            }
        }
    }

    /// The EQ's coefficients for the current values.
    fn tune(&mut self) {
        let sr = self.sample_rate;
        let (bass, mid, mid_freq, treble) = (
            self.value(param::EQ_BASS),
            self.value(param::EQ_MID),
            self.value(param::EQ_MID_FREQ),
            self.value(param::EQ_TREBLE),
        );
        for side in &mut self.sides {
            side.bass.set(Coefficients::low_shelf(sr, BASS_SHELF_HERTZ, BUTTERWORTH_Q, bass));
            side.mid.set(Coefficients::peak(sr, mid_freq, MID_Q, mid));
            side.treble.set(Coefficients::high_shelf(sr, TREBLE_SHELF_HERTZ, BUTTERWORTH_Q, treble));
        }
    }

    /// Filters, delays, resampler legs, the running model and the schedule, everything for the
    /// current sample rate and the model in hand, cleared. The running model's build allocates:
    /// it happens here, at prepare or reset, never on the audio thread.
    fn rebuild(&mut self) {
        let sr = self.sample_rate;
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, sr);
        self.model_rate = self.model.as_ref().map_or(0.0, |m| m.expected_sample_rate());
        let model_rate = self.model_hertz().max(1.0);
        self.sides = [Side::new(sr, model_rate), Side::new(sr, model_rate)];
        self.chunk[0].fill(0.0);
        self.chunk[1].fill(0.0);
        self.ready_left.fill(0.0);
        self.ready_right.fill(0.0);
        self.in_pos = 0;
        self.produced_pos = 0;
        self.delays = [DelayLine::new(sr, MAX_DELAY_SECONDS), DelayLine::new(sr, MAX_DELAY_SECONDS)];
        // The legs' staging is sized for the rate pair: what the legs move is within a chunk
        // times the ratio, plus the kernels' halves — all four stage lists sized alike so a
        // path never runs out.
        let ratio = model_rate / sr.max(1.0);
        let cap = CHUNK.max((CHUNK as f64 * ratio).ceil() as usize).max((CHUNK as f64 / ratio).ceil() as usize)
            + 2 * HALF_TAPS
            + 16;
        self.model_input = [vec![0.0; cap], vec![0.0; cap]];
        self.model_io = [vec![0.0; cap], vec![0.0; cap]];
        self.dc_staged = [vec![0.0; cap], vec![0.0; cap]];
        self.stage_out = [vec![0.0; cap], vec![0.0; cap]];
        self.chunk_f64 = [vec![0.0; CHUNK], vec![0.0; CHUNK]];
        let frames = (self.value(param::DELAY) * sr).clamp(0.0, (MAX_DELAY_SECONDS * sr).floor());
        self.input_gain.reset(self.input_target());
        self.input_gain.set_ramp(ramp);
        self.output_volume.reset(self.output_target());
        self.output_volume.set_ramp(ramp);
        self.delay_frames.reset(frames as f32);
        self.delay_frames.set_ramp(ramp);
        for delay in &mut self.delays {
            delay.set_frames(frames);
        }
        self.tune();
        // The running model, two instances (one per side), or its refusal.
        self.running = [None, None];
        self.model_refused = false;
        if let Some(parsed) = self.model.as_ref() {
            for slot in self.running.iter_mut() {
                match Model::from_nam(parsed.as_ref()) {
                    Ok(instance) => *slot = Some(instance),
                    Err(_) => {
                        self.model_refused = true;
                        self.running = [None, None];
                        break;
                    }
                }
            }
            if !self.model_refused {
                self.apply_hq();
            }
        }
    }
}

impl NamAmp {
    /// The head stages of one incoming frame — the input trim, then the EQ — stored in the
    /// chunk. The trim's smoother moves once per frame, both sides mixed the same way.
    fn head(&mut self, sample: [f32; 2], slot: usize) {
        let eq_on = self.value(param::EQ_ON) >= 0.5;
        let trim = f64::from(self.input_gain.next_value());
        for (side, x_in) in [(0usize, sample[0]), (1usize, sample[1])] {
            let mut x = f64::from(x_in) * trim;
            if eq_on {
                let fields = &mut self.sides[side];
                x = fields.bass.process(x);
                x = fields.mid.process(x);
                x = fields.treble.process(x);
            }
            self.chunk[side][slot] = x.clamp(-1e30, 1e30) as f32;
        }
    }

    /// The completed chunk: each side's model stage (a copy of the chunk into the model's
    /// buffer when the rates match, with the resampler legs around it when they do not), then
    /// the tail stages per produced frame. Both sides produce the same number of frames; the
    /// tails run them in stream order at their own positions.
    fn complete_chunk(&mut self) {
        let mut counts = [0usize; 2];
        for (side, count) in counts.iter_mut().enumerate() {
            *count = self.model_stage(side);
        }
        let count = counts[0].min(counts[1]).min(RING);
        for k in 0..count {
            self.delay_frames.next_value();
            self.output_volume.next_value();
            let volume = f64::from(self.output_volume.current());
            for side in 0..2 {
                self.delays[side].set_frames(f64::from(self.delay_frames.current()));
                let delayed = self.delays[side].process(self.stage_out[side][k]);
                let value = (delayed * volume) as f32;
                let slot = ((self.produced_pos + k as u64) as usize) & (RING - 1);
                if side == 0 {
                    self.ready_left[slot] = value;
                } else {
                    self.ready_right[slot] = value;
                }
            }
        }
        self.produced_pos += count as u64;
    }

    /// One side's stage between the head-stored chunk and the tails: the model, with the
    /// resampler legs when the model's rate is another one (and no legs without a model), plus
    /// its DC blocker. The produced frames land in `stage_out[side]`; the count comes back. A
    /// model the runtime refuses passes the chunk through.
    fn model_stage(&mut self, side: usize) -> usize {
        let chunk: [f32; CHUNK] = *self.chunk[side];
        if self.running[side].is_none() {
            // No model: the trims and EQ stand in for it, with no DC blocker to run.
            self.model_io[side][..CHUNK].copy_from_slice(&chunk);
            for (k, io) in self.model_io[side].iter().enumerate() {
                self.stage_out[side][k] = f64::from(*io);
            }
            return CHUNK;
        }
        let rate_match = self.rates_match();
        if rate_match {
            self.model_io[side][..CHUNK].copy_from_slice(&chunk);
            if let Some(model) = self.running.get_mut(side).and_then(|slot| slot.as_mut()) {
                model.process_buffer(&mut self.model_io[side][..CHUNK]);
            }
            for (k, io) in self.model_io[side].iter().enumerate() {
                self.stage_out[side][k] = self.sides[side].dc.process(f64::from(*io));
            }
            return CHUNK;
        }
        // The legs: into the model's rate, through the model, the DC blocker, and back.
        for (k, value) in self.chunk[side].iter().enumerate() {
            self.chunk_f64[side][k] = f64::from(*value);
        }
        let fed = self.sides[side].into_model.process(&self.chunk_f64[side][..CHUNK], &mut self.model_input[side]);
        for (k, value) in self.model_input[side].iter().take(fed).enumerate() {
            self.model_io[side][k] = *value as f32;
        }
        if let Some(model) = self.running.get_mut(side).and_then(|slot| slot.as_mut()) {
            model.process_buffer(&mut self.model_io[side][..fed]);
        }
        for (k, io) in self.model_io[side].iter().take(fed).enumerate() {
            self.dc_staged[side][k] = self.sides[side].dc.process(f64::from(*io));
        }
        self.sides[side].out_of_model.process(&self.dc_staged[side][..fed], &mut self.stage_out[side])
    }
}

impl NamAmp {
    /// The stream's per-frame loop for [`Device::process`], in position terms.
    fn stream(&mut self, buffer: &mut [f32]) {
        for pair in buffer.as_chunks_mut::<2>().0 {
            let in_at = self.in_pos;
            let input = [pair[0], pair[1]];
            // The output CHUNK frames behind: produced the moment this frame is fed.
            match in_at.checked_sub(CHUNK as u64) {
                Some(p) => {
                    let slot = (p as usize) & (RING - 1);
                    pair[0] = self.ready_left[slot];
                    pair[1] = self.ready_right[slot];
                }
                None => *pair = [0.0, 0.0],
            }
            let slot = (in_at as usize) & (CHUNK - 1);
            self.head(input, slot);
            if slot == CHUNK - 1 {
                self.complete_chunk();
            }
            self.in_pos = in_at.wrapping_add(1);
        }
    }
}

impl Device for NamAmp {
    fn info(&self) -> &'static DeviceInfo {
        &INFO
    }

    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        self.sample_rate = check_sample_rate(sample_rate)?;
        self.rebuild();
        Ok(())
    }

    fn reset(&mut self) {
        self.rebuild();
    }

    fn set_param(&mut self, index: usize, value: f64) {
        let Some(info) = PARAMS.get(index) else { return };
        let Some(slot) = self.values.get_mut(index) else { return };
        *slot = info.clamp(value);
        match index {
            param::INPUT_GAIN => self.input_gain.set_target(self.input_target()),
            param::OUTPUT_VOLUME => self.output_volume.set_target(self.output_target()),
            param::DELAY => {
                let rate = self.sample_rate.max(1.0);
                let frames = (self.value(param::DELAY) * rate).clamp(0.0, (MAX_DELAY_SECONDS * rate).floor());
                self.delay_frames.set_target(frames as f32);
            }
            param::EQ_BASS | param::EQ_MID | param::EQ_TREBLE | param::EQ_MID_FREQ => self.tune(),
            param::HQ => self.apply_hq(),
            _ => {}
        }
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.values.get(index).copied()
    }

    fn latency(&self) -> u32 {
        // One fixed chunk; the model's own stream placement is exact and the legs' production
        // is schedule-covered.
        CHUNK as u32
    }

    fn process(&mut self, _context: &ProcessContext, buffer: &mut [f32]) {
        self.stream(buffer);
    }
}

#[cfg(test)]
mod tests;
