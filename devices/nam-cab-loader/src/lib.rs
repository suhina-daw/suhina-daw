// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The NAM Cab Loader: a signal through a speaker cabinet's impulse response, with a phase
//! invert, a high pass, bass/mid/treble cuts, a trim delay and a volume.
//!
//! Per side ([docs/spec/nam-amp-and-cab.md]): the phase invert, a second-order high pass at
//! 100 Hz when HP is on, the cuts — a low shelf below 100 Hz, a dip at the mid frequency and a
//! high shelf above 4 kHz, each to −24 dB — then the IR convolution, the delay line and the
//! volume. The convolution is entirely wet. An IR whose rate is not the project's is resampled
//! once, at load, before the device sees it. Nothing on the audio path allocates.
//!
//! The stream runs in fixed chunks of [`CHUNK`] frames counted from a reset, and the output
//! trails the input by one chunk — [`CHUNK`], told through `latency()` — so a host's block
//! sizes never change the sound. An IR at another rate than the project's is not run.

use suhina_device_api::{
    Device, DeviceError, DeviceInfo, ParamInfo, ProcessContext, Role, Scale, Unit, check_sample_rate,
};
use suhina_dsp::biquad::BUTTERWORTH_Q;
use suhina_dsp::gain::db_to_gain;
use suhina_dsp::{Biquad, Coefficients, Convolver, DelayLine, Ir, Smoother};

/// Ramp for the volume and the delay.
pub const RAMP_SECONDS: f64 = 0.02;
/// The high pass that HP turns on, and the bass cut's shelf.
pub const BASS_HERTZ: f64 = 100.0;
/// The treble cut's shelf.
pub const TREBLE_HERTZ: f64 = 4_000.0;
/// The mid cut dip's width, in Q.
pub const MID_Q: f64 = 0.8;
/// The longest delay the knob sets, in seconds.
pub const MAX_DELAY_SECONDS: f64 = 0.02;
/// Frames of one fixed chunk — the schedule the stream runs on, the convolver's partition size
/// and the delay the device tells the engine's compensation. A power of two.
const CHUNK: usize = 512;

/// Parameter indices, in [`INFO`] order.
pub mod param {
    pub const PHASE: usize = 0;
    pub const HP: usize = 1;
    pub const BASS: usize = 2;
    pub const MID: usize = 3;
    pub const TREBLE: usize = 4;
    pub const MID_FREQ: usize = 5;
    pub const DELAY: usize = 6;
    pub const VOLUME: usize = 7;
    pub(crate) const COUNT: usize = 8;
}

const PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::toggle("phase", "Phase", false),
    ParamInfo::toggle("hpOn", "HP", true),
    ParamInfo::continuous("cutBass", "Bass", (-24.0, 0.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("cutMid", "Mid", (-24.0, 0.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("cutTreble", "Treble", (-24.0, 0.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("cutMidFreq", "Mid Freq", (200.0, 2_000.0), 600.0, Unit::Hertz, Scale::Logarithmic),
    ParamInfo::continuous("delay", "Delay", (0.0, 0.02), 0.0, Unit::Seconds, Scale::Linear),
    ParamInfo::continuous("volume", "Volume", (-24.0, 24.0), 0.0, Unit::Decibels, Scale::Linear),
];

pub const INFO: DeviceInfo =
    DeviceInfo { id: "nam-cab-loader", name: "NAM Cab Loader", role: Role::Effect, params: &PARAMS };

/// Reading `.wav` files.
pub mod ir;

const DEFAULT_SAMPLE_RATE: f64 = 48_000.0;

/// One side's filters.
#[derive(Clone, Copy)]
struct Side {
    hp: Biquad,
    bass: Biquad,
    mid: Biquad,
    treble: Biquad,
}

impl Side {
    fn new(sample_rate: f64) -> Self {
        Self {
            hp: Biquad::new(Coefficients::highpass(sample_rate, BASS_HERTZ, BUTTERWORTH_Q)),
            bass: Biquad::new(Coefficients::low_shelf(sample_rate, BASS_HERTZ, BUTTERWORTH_Q, 0.0)),
            mid: Biquad::new(Coefficients::peak(sample_rate, 600.0, MID_Q, 0.0)),
            treble: Biquad::new(Coefficients::high_shelf(sample_rate, TREBLE_HERTZ, BUTTERWORTH_Q, 0.0)),
        }
    }
}

/// The NAM Cab Loader. [`CabLoader::with_ir`] convolves with a decoded IR ([`Ir`]); without one
/// the signal passes through the trims, whatever the cuts say.
pub struct CabLoader {
    sample_rate: f64,
    values: [f64; param::COUNT],
    /// The IR the loader was made with, for `prepare` and `reset` rebuilds.
    ir: Option<Ir>,
    sides: [Side; 2],
    convs: [Option<Convolver>; 2],
    /// One fixed chunk of frames going through the head stages, per side.
    chunk: [Box<[f32; CHUNK]>; 2],
    /// The readied outputs waiting for their delivery, per side, a ring of one chunk each.
    ready_left: Box<[f32; CHUNK]>,
    ready_right: Box<[f32; CHUNK]>,
    /// The stream positions of the next input frame and delivery, in frames since the last
    /// reset.
    in_pos: u64,
    out_pos: u64,
    /// Staging for one chunk's convolution, per side.
    conv_in: [Vec<f32>; 2],
    conv_out: [Vec<f32>; 2],
    delay: [DelayLine; 2],
    /// The volume in linear gain, and the delay in frames, both smoothed.
    volume: Smoother,
    delay_frames: Smoother,
}

impl Default for CabLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl CabLoader {
    /// A cab loader with no IR.
    pub fn new() -> Self {
        Self::build(DEFAULT_SAMPLE_RATE, None)
    }

    /// A cab loader that convolves with `ir`.
    pub fn with_ir(ir: Ir) -> Self {
        Self::build(DEFAULT_SAMPLE_RATE, Some(ir))
    }

    /// Everything for `sample_rate` and `ir`. `prepare` and `reset` run it again; whatever
    /// clears the state, counters included, calls it.
    fn build(sample_rate: f64, ir: Option<Ir>) -> Self {
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, sample_rate);
        let mut loader = Self {
            sample_rate,
            values: PARAMS.map(|p| p.default),
            ir,
            sides: [Side::new(sample_rate), Side::new(sample_rate)],
            convs: [None, None],
            chunk: [Box::new([0.0; CHUNK]), Box::new([0.0; CHUNK])],
            ready_left: Box::new([0.0; CHUNK]),
            ready_right: Box::new([0.0; CHUNK]),
            in_pos: 0,
            out_pos: 0,
            conv_in: [vec![0.0; CHUNK], vec![0.0; CHUNK]],
            conv_out: [vec![0.0; CHUNK], vec![0.0; CHUNK]],
            delay: [DelayLine::new(sample_rate, MAX_DELAY_SECONDS), DelayLine::new(sample_rate, MAX_DELAY_SECONDS)],
            volume: Smoother::new(1.0, 1),
            delay_frames: Smoother::new(0.0, 1),
        };
        loader.volume.set_ramp(ramp);
        loader.delay_frames.set_ramp(ramp);
        loader.rebuild();
        loader
    }

    /// Makes the two sides' convolutions from the IR at the project's rate. A mono IR feeds
    /// both sides. One recorded at another rate, or one the convolver cannot hold, stays out.
    fn build_convs(&mut self) {
        self.convs = [None, None];
        let Some(ir) = self.ir.as_ref() else { return };
        if ir.channels.max(1) > 2 || ir.sample_rate == 0 {
            return;
        }
        for (side_index, conv_slot) in self.convs.iter_mut().enumerate() {
            let side = ir.side(if ir.channels.max(1) == 1 { 0 } else { side_index });
            if side.is_empty() {
                return;
            }
            let mut conv = Convolver::default();
            if conv.init(side).is_err() {
                return;
            }
            *conv_slot = Some(conv);
        }
    }

    /// The cuts' coefficients for the current values.
    fn tune(&mut self) {
        let sr = self.sample_rate;
        let (bass, mid, mid_freq, treble) =
            (self.value(param::BASS), self.value(param::MID), self.value(param::MID_FREQ), self.value(param::TREBLE));
        for side in &mut self.sides {
            side.bass.set(Coefficients::low_shelf(sr, BASS_HERTZ, BUTTERWORTH_Q, bass));
            side.mid.set(Coefficients::peak(sr, mid_freq, MID_Q, mid));
            side.treble.set(Coefficients::high_shelf(sr, TREBLE_HERTZ, BUTTERWORTH_Q, treble));
        }
    }

    fn value(&self, index: usize) -> f64 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    /// Filters, smoothers, delays, convolution memory and the schedule, everything cleared.
    fn rebuild(&mut self) {
        let sr = self.sample_rate;
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, sr);
        self.sides = [Side::new(sr), Side::new(sr)];
        self.build_convs();
        self.chunk[0].fill(0.0);
        self.chunk[1].fill(0.0);
        self.ready_left.fill(0.0);
        self.ready_right.fill(0.0);
        self.in_pos = 0;
        self.out_pos = 0;
        self.delay = [DelayLine::new(sr, MAX_DELAY_SECONDS), DelayLine::new(sr, MAX_DELAY_SECONDS)];
        let volume_now = db_to_gain(self.value(param::VOLUME) as f32);
        self.volume.reset(volume_now);
        self.volume.set_ramp(ramp);
        let frames = (self.value(param::DELAY) * sr).clamp(0.0, (MAX_DELAY_SECONDS * sr).floor());
        self.delay_frames.reset(frames as f32);
        self.delay_frames.set_ramp(ramp);
        for delay in &mut self.delay {
            delay.set_frames(frames);
        }
        self.tune();
    }
}

impl CabLoader {
    /// The heads' stages of one incoming frame, and its store in the filling chunk.
    fn head(&mut self, sample: [f32; 2], slot: usize) {
        let (hp_on, phase_on) = (self.value(param::HP) >= 0.5, self.value(param::PHASE) >= 0.5);
        for (side, x_in) in [(0usize, sample[0]), (1usize, sample[1])] {
            let mut x = f64::from(x_in);
            if phase_on {
                x = -x;
            }
            let side_fields = &mut self.sides[side];
            if hp_on {
                x = side_fields.hp.process(x);
            }
            x = side_fields.bass.process(x);
            x = side_fields.mid.process(x);
            x = side_fields.treble.process(x);
            let staged = x.clamp(-1e30, 1e30) as f32;
            self.chunk[side][slot] = staged;
        }
    }

    /// The completed chunk's convolutions and tail stages: its frames, in stream order, ready
    /// where their deliveries will read them.
    fn complete_chunk(&mut self, base: u64) {
        for side in 0..2 {
            self.conv_in[side].copy_from_slice(&self.chunk[side][..]);
            match self.convs[side].as_mut() {
                Some(conv) => conv.process(&self.conv_in[side], &mut self.conv_out[side]),
                None => self.conv_out[side].copy_from_slice(&self.conv_in[side]),
            }
        }
        for index in 0..CHUNK {
            self.delay_frames.next_value();
            self.volume.next_value();
            for side in 0..2 {
                self.delay[side].set_frames(f64::from(self.delay_frames.current()));
                let delayed = self.delay[side].process(f64::from(self.conv_out[side][index]));
                let value = (delayed * f64::from(self.volume.current())) as f32;
                let slot = ((base + index as u64) as usize) & (CHUNK - 1);
                if side == 0 {
                    self.ready_left[slot] = value;
                } else {
                    self.ready_right[slot] = value;
                }
            }
        }
    }

    /// Same as [`Device::process`]'s, as the loop the fixed schedule runs: everything a frame
    /// does is in position terms, so the host's splits cannot be heard.
    fn stream(&mut self, buffer: &mut [f32]) {
        for pair in buffer.as_chunks_mut::<2>().0 {
            let in_at = self.in_pos;
            let input = [pair[0], pair[1]];
            // The output CHUNK frames behind: ready the moment this frame arrives.
            match in_at.checked_sub(CHUNK as u64) {
                Some(p) => {
                    let slot = (p as usize) & (CHUNK - 1);
                    pair[0] = self.ready_left[slot];
                    pair[1] = self.ready_right[slot];
                }
                None => *pair = [0.0, 0.0],
            }
            let slot = (in_at as usize) & (CHUNK - 1);
            self.head(input, slot);
            if slot == CHUNK - 1 {
                self.complete_chunk(in_at.saturating_sub((CHUNK - 1) as u64));
            }
            self.in_pos = in_at.wrapping_add(1);
        }
    }
}

impl Device for CabLoader {
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
            param::VOLUME => self.volume.set_target(db_to_gain(self.value(param::VOLUME) as f32)),
            param::DELAY => {
                let rate = self.sample_rate.max(1.0);
                let frames = (self.value(param::DELAY) * rate).clamp(0.0, (MAX_DELAY_SECONDS * rate).floor());
                self.delay_frames.set_target(frames as f32);
            }
            param::BASS | param::MID | param::TREBLE | param::MID_FREQ => self.tune(),
            _ => {}
        }
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.values.get(index).copied()
    }

    fn latency(&self) -> u32 {
        // One fixed chunk: the schedule holds the last chunk back, compensation aligns the rest.
        CHUNK as u32
    }

    fn process(&mut self, _context: &ProcessContext, buffer: &mut [f32]) {
        self.stream(buffer);
    }
}

#[cfg(test)]
mod tests;
