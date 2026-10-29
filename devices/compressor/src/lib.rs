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

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, ParamBank, ParamInfo, ProcessContext, Role, Scale, Unit,
    check_sample_rate,
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

const PARAMS: [ParamInfo; param::COUNT] = [
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

pub const INFO: DeviceInfo = DeviceInfo { id: "compressor", name: "Compressor", role: Role::Effect, params: &PARAMS };

/// Gain change, in decibels (zero or negative), the gain computer asks for at `level_db` with
/// `threshold` (dB), `slope` (1 - 1/ratio) and a soft knee `knee` dB wide centred on the
/// threshold.
#[inline]
fn gain_change(level_db: f64, threshold: f64, slope: f64, knee: f64) -> f64 {
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
}

impl Default for Compressor {
    fn default() -> Self {
        Self::new()
    }
}

impl Compressor {
    pub fn new() -> Self {
        let mut compressor = Self {
            sample_rate: DEFAULT_SAMPLE_RATE,
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
    }
}

impl Device for Compressor {
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

    fn process(&mut self, _context: &ProcessContext, buffer: &mut [f32]) {
        for frame in buffer.as_chunks_mut::<2>().0 {
            let [input, threshold, ratio, knee, output, mix] = self.smoothers().map(|s| f64::from(s.next_value()));
            let [left, right] = frame.map(|s| f64::from(s) * input);
            let mut level = 0.0f64;
            for (detector, x) in self.detectors.iter_mut().zip([left, right]) {
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
        }
    }
}

#[cfg(test)]
mod tests;
