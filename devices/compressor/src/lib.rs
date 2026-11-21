// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Compressor: a stereo-linked, feed-forward compressor with a soft knee, a high-pass on the
//! detector and a dry/wet mix for parallel compression.
//!
//! Per frame: the input gain applies to both sides; the detector high-passes each side (12
//! dB/octave, so bass moves the gain less when the corner is raised) and takes the louder of the
//! two as the level, so both sides always get the same gain and the stereo image stays put. The
//! level in decibels goes through the gain computer (threshold, ratio, a quadratic soft knee
//! centred on the threshold), and the gain reduction it asks for is smoothed in decibels: it
//! moves toward a larger reduction with the attack time and back with the release time (each a
//! time constant, 63 % of a step after that long). The reduced signal is blended with the
//! uncompressed one (both after the input gain) by the dry/wet mix, and the output gain applies
//! last. The gain computer and smoothing are the textbook design (as in Giannoulis, Massberg
//! and Reiss, *Digital Dynamic Range Compressor Design*, JAES 2012); the code is our own.
//!
//! Gains, threshold, ratio, knee and mix are ramped, so they can be moved while playing. There
//! is no lookahead, so the device adds no latency.
//!
//! After each block the device hands its panel what it heard and did ([`readout`]): the level
//! the detector read, the input and output peaks and the gain reduction, for the panel's
//! transfer curve, activity history and meters (docs/spec/compressor.md › The panel). The panel
//! itself is [`panel`].

pub mod panel;

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, MAX_READOUTS, ParamBank, ParamInfo, ProcessContext, Role,
    Scale, Unit, check_sample_rate,
};
use suhina_dsp::biquad::BUTTERWORTH_Q;
use suhina_dsp::gain::{db_to_gain, db_to_gain_f64};
use suhina_dsp::{AttackRelease, Biquad, Coefficients, Smoother};

/// Ramp for gain, threshold, ratio, knee and mix changes.
pub const RAMP_SECONDS: f64 = 0.02;
/// Largest gain reduction the device applies, in decibels; more is only reached with input no
/// real signal has, and stopping here keeps the arithmetic finite.
pub const MAX_REDUCTION_DB: f64 = 120.0;
/// Detector levels below this (about -200 dB) count as silence.
const LEVEL_FLOOR: f64 = 1e-10;
/// The lowest level the readouts hand the panel, in dBFS: silence reads as this.
pub const READOUT_FLOOR_DB: f64 = -100.0;
/// How fast the readouts' level peaks fall back, a time constant in seconds (about 60 dB per
/// second), so a peak between two of the panel's frames still shows.
const LEVEL_FALL_SECONDS: f64 = 0.145;
/// How fast the readout's gain reduction falls back once the reduction itself has, a time
/// constant in seconds: short, so the activity history keeps the shape of the release.
const REDUCTION_FALL_SECONDS: f64 = 0.03;

/// What the device hands its panel after each block (`Device::readouts`), from the block's
/// frames (docs/spec/compressor.md › Readouts).
pub mod readout {
    /// The level the detector read (the key while keyed), after the input gain and the
    /// side-chain high-pass, the louder side, dBFS, a falling peak.
    pub const DETECTOR: usize = 0;
    /// The device's own input after the input gain, the louder side, dBFS, a falling peak.
    pub const INPUT: usize = 1;
    /// The output, the louder side, dBFS, a falling peak.
    pub const OUTPUT: usize = 2;
    /// The gain reduction, dB (zero or positive), the block's largest, falling back.
    pub const REDUCTION: usize = 3;
    /// 1 while the detector reads a key, else 0.
    pub const KEYED: usize = 4;
}

/// Parameter indices, in [`INFO`] order.
pub mod param {
    pub const INPUT: usize = 0;
    pub const THRESHOLD: usize = 1;
    pub const RATIO: usize = 2;
    pub const ATTACK: usize = 3;
    pub const RELEASE: usize = 4;
    pub const KNEE: usize = 5;
    pub const SIDECHAIN_HIGH_PASS: usize = 6;
    pub const OUTPUT: usize = 7;
    pub const MIX: usize = 8;
    pub(crate) const COUNT: usize = 9;
}

static PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::continuous("input", "Input", (-24.0, 24.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("threshold", "Threshold", (-60.0, 0.0), -12.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("ratio", "Ratio", (1.0, 20.0), 3.0, Unit::None, Scale::Logarithmic),
    ParamInfo::continuous("attack", "Attack", (0.0001, 0.1), 0.003, Unit::Seconds, Scale::Logarithmic),
    ParamInfo::continuous("release", "Release", (0.01, 2.0), 0.3, Unit::Seconds, Scale::Logarithmic),
    ParamInfo::continuous("knee", "Knee", (0.0, 24.0), 6.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous(
        "sidechainHighPass",
        "Side-Chain High-Pass",
        (20.0, 1_000.0),
        20.0,
        Unit::Hertz,
        Scale::Logarithmic,
    ),
    ParamInfo::continuous("output", "Output", (-24.0, 24.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("mix", "Dry/Wet", (0.0, 1.0), 1.0, Unit::Percent, Scale::Linear),
];

pub static INFO: DeviceInfo = DeviceInfo::new("compressor", "Compressor", Role::Effect, &PARAMS);

/// Gain change, in decibels (zero or negative), the gain computer asks for at `level_db` with
/// `threshold` (dB), `slope` (1 - 1/ratio) and a soft knee `knee` dB wide centred on the
/// threshold. The panel's transfer curve is drawn from it, so the curve is what sounds.
#[inline]
pub fn gain_change(level_db: f64, threshold: f64, slope: f64, knee: f64) -> f64 {
    let over = level_db - threshold;
    if knee > 1e-6 && over.abs() <= knee / 2.0 {
        let into = over + knee / 2.0;
        -slope * into * into / (2.0 * knee)
    } else if over > 0.0 {
        -slope * over
    } else {
        0.0
    }
}

pub struct Compressor {
    sample_rate: f64,
    params: ParamBank<{ param::COUNT }>,
    input: Smoother,
    threshold: Smoother,
    ratio: Smoother,
    knee: Smoother,
    output: Smoother,
    mix: Smoother,
    dynamics: AttackRelease,
    detectors: [Biquad; 2],
    /// Smoothed gain reduction in decibels, zero or positive.
    reduction: f64,
    /// The panel's falling peaks, linear: detector, input, output; and the reduction in dB.
    meters: Meters,
    /// The per-frame factors the meters fall by: the levels' and the reduction's.
    fall: (f64, f64),
    readouts: [f32; MAX_READOUTS],
}

/// The falling peaks the readouts are taken from.
#[derive(Debug, Clone, Copy, Default)]
struct Meters {
    detector: f64,
    input: f64,
    output: f64,
    reduction: f64,
}

/// A level, linear, as the readouts write it: dBFS, never under [`READOUT_FLOOR_DB`].
fn readout_db(level: f64) -> f32 {
    if level > 0.0 { (20.0 * level.log10()).max(READOUT_FLOOR_DB) as f32 } else { READOUT_FLOOR_DB as f32 }
}

/// The readouts of a device that has heard nothing.
fn idle_readouts() -> [f32; MAX_READOUTS] {
    let mut readouts = [0.0; MAX_READOUTS];
    for index in [readout::DETECTOR, readout::INPUT, readout::OUTPUT] {
        if let Some(slot) = readouts.get_mut(index) {
            *slot = READOUT_FLOOR_DB as f32;
        }
    }
    readouts
}

impl Default for Compressor {
    fn default() -> Self {
        Self::new()
    }
}

impl Compressor {
    pub fn new() -> Self {
        Self::at(DEFAULT_SAMPLE_RATE)
    }

    /// The same, made for `sample_rate` (ADR 0004 › Amendment: a device is made at its rate);
    /// fails for a rate a device cannot run at. Allocates.
    pub fn with_rate(sample_rate: f64) -> Result<Self, DeviceError> {
        check_sample_rate(sample_rate).map(Self::at)
    }

    /// Everything for `sample_rate`, which has been checked.
    fn at(sample_rate: f64) -> Self {
        let mut compressor = Self {
            sample_rate,
            params: ParamBank::new(&PARAMS),
            input: Smoother::new(1.0, 1),
            threshold: Smoother::new(0.0, 1),
            ratio: Smoother::new(1.0, 1),
            knee: Smoother::new(0.0, 1),
            output: Smoother::new(1.0, 1),
            mix: Smoother::new(1.0, 1),
            dynamics: AttackRelease::default(),
            detectors: [Biquad::default(); 2],
            reduction: 0.0,
            meters: Meters::default(),
            fall: (0.0, 0.0),
            readouts: idle_readouts(),
        };
        compressor.rebuild();
        compressor
    }

    /// The gain reduction applied to the last frame, in decibels (zero or positive), for a
    /// meter.
    pub fn gain_reduction_db(&self) -> f64 {
        self.reduction
    }

    fn value(&self, index: usize) -> f64 {
        self.params.get(index)
    }

    fn targets(&self) -> [f32; 6] {
        [
            db_to_gain(self.value(param::INPUT) as f32),
            self.value(param::THRESHOLD) as f32,
            self.value(param::RATIO) as f32,
            self.value(param::KNEE) as f32,
            db_to_gain(self.value(param::OUTPUT) as f32),
            self.value(param::MIX) as f32,
        ]
    }

    fn smoothers(&mut self) -> [&mut Smoother; 6] {
        [&mut self.input, &mut self.threshold, &mut self.ratio, &mut self.knee, &mut self.output, &mut self.mix]
    }

    /// Attack and release coefficients and the detector filter from the parameters.
    fn tune(&mut self) {
        let sr = self.sample_rate;
        self.dynamics = AttackRelease::new(self.value(param::ATTACK), self.value(param::RELEASE), sr);
        let high_pass = Coefficients::highpass(sr, self.value(param::SIDECHAIN_HIGH_PASS), BUTTERWORTH_Q);
        for detector in &mut self.detectors {
            detector.set(high_pass);
        }
    }

    /// Smoothers and filters for the current sample rate, every state cleared.
    fn rebuild(&mut self) {
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, self.sample_rate);
        let targets = self.targets();
        for (smoother, target) in self.smoothers().into_iter().zip(targets) {
            smoother.set_ramp(ramp);
            smoother.reset(target);
        }
        self.detectors = [Biquad::default(); 2];
        self.tune();
        self.reduction = 0.0;
        let fall = |seconds: f64| (-1.0 / (seconds * self.sample_rate)).exp();
        self.fall = (fall(LEVEL_FALL_SECONDS), fall(REDUCTION_FALL_SECONDS));
        self.meters = Meters::default();
        self.readouts = idle_readouts();
    }
}

impl Device for Compressor {
    fn info(&self) -> &DeviceInfo {
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
        if self.params.set(index, value).is_none() {
            return;
        }
        let targets = self.targets();
        for (smoother, target) in self.smoothers().into_iter().zip(targets) {
            smoother.set_target(target);
        }
        if matches!(index, param::ATTACK | param::RELEASE | param::SIDECHAIN_HIGH_PASS) {
            self.tune();
        }
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    fn readouts(&self) -> Option<[f32; MAX_READOUTS]> {
        Some(self.readouts)
    }

    fn process(&mut self, _context: &ProcessContext, buffer: &mut [f32]) {
        self.compress(buffer, None);
    }

    /// The detector reads `key` in place of the device's own input (`docs/spec/sidechain.md`):
    /// the key's level, through the input gain and the sidechain high-pass, sets the gain
    /// reduction, which the device's own sound takes.
    fn process_keyed(&mut self, _context: &ProcessContext, key: &[f32], buffer: &mut [f32]) {
        self.compress(buffer, Some(key));
    }
}

impl Compressor {
    /// Compresses `buffer` in place; the detector reads `key` frame by frame when given (a
    /// frame the key does not cover reads silence), else the buffer itself. Never allocates.
    fn compress(&mut self, buffer: &mut [f32], key: Option<&[f32]>) {
        let keys = key.map(|k| k.as_chunks::<2>().0);
        let (level_fall, reduction_fall) = self.fall;
        let mut meters = self.meters;
        for (index, frame) in buffer.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let [input, threshold, ratio, knee, output, mix] = self.smoothers().map(|s| f64::from(s.next_value()));
            let [left, right] = frame.map(|s| f64::from(s) * input);
            let heard = match keys {
                Some(keys) => keys.get(index).map_or([0.0, 0.0], |k| k.map(|s| f64::from(s) * input)),
                None => [left, right],
            };
            let mut level = 0.0f64;
            for (detector, x) in self.detectors.iter_mut().zip(heard) {
                let x = if x.is_finite() { x } else { 0.0 };
                level = level.max(detector.process(x).abs());
            }
            let level_db = 20.0 * level.clamp(LEVEL_FLOOR, 1e30).log10();
            let slope = 1.0 - 1.0 / ratio.max(1.0);
            let wanted = (-gain_change(level_db, threshold, slope, knee)).clamp(0.0, MAX_REDUCTION_DB);
            let reduction = self.dynamics.rise(self.reduction, wanted);
            self.reduction = if reduction > 1e-9 { reduction.min(MAX_REDUCTION_DB) } else { 0.0 };
            let gain = db_to_gain_f64(-self.reduction);
            let applied = output * (1.0 + mix * (gain - 1.0));
            for (sample, x) in frame.iter_mut().zip([left, right]) {
                *sample = (x * applied) as f32;
            }
            let peak = |meter: f64, x: f64| if x > meter { x } else { meter * level_fall };
            let finite = |x: f64| if x.is_finite() { x.abs() } else { 0.0 };
            meters.detector = peak(meters.detector, level);
            meters.input = peak(meters.input, finite(left).max(finite(right)));
            meters.output = peak(meters.output, finite(left * applied).max(finite(right * applied)));
            meters.reduction = self.reduction.max(meters.reduction * reduction_fall);
        }
        self.meters = meters;
        self.readouts[readout::DETECTOR] = readout_db(meters.detector);
        self.readouts[readout::INPUT] = readout_db(meters.input);
        self.readouts[readout::OUTPUT] = readout_db(meters.output);
        self.readouts[readout::REDUCTION] = meters.reduction as f32;
        self.readouts[readout::KEYED] = if key.is_some() { 1.0 } else { 0.0 };
    }
}

#[cfg(test)]
mod tests;
