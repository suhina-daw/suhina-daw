// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Overdrive: a pedal-style distortion.
//!
//! Per side: a band-pass (a high-pass and a low-pass around the band frequency) picks what gets
//! driven, the drive raises it by up to +40 dB into a biased, asymmetric soft curve (odd and
//! even harmonics) at twice the sample rate, a low-pass (the tone) darkens the result, and the mix blends it with the dry
//! signal. The curve keeps the output level about the same however hard it is driven; the
//! dynamics control brings back the input's level changes, so quiet passages stay quieter. The
//! dry signal is delayed to line up with the oversampled path: the device always reports
//! [`LATENCY`] frames, and with the mix at 0 the output is the input delayed exactly.

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, ParamBank, ParamInfo, ProcessContext, Role, Scale, Unit,
    check_sample_rate,
};
use suhina_dsp::biquad::BUTTERWORTH_Q;
use suhina_dsp::gain::db_to_gain;
use suhina_dsp::shape::Curve;
use suhina_dsp::{Biquad, Coefficients, DcBlocker, OnePole, Oversampler2x, Smoother};

/// Delay the device adds, in frames.
pub const LATENCY: u32 = Oversampler2x::LATENCY;
/// Ramp for drive, dynamics, mix and output changes.
pub const RAMP_SECONDS: f64 = 0.02;
/// Drive at 1.0 raises the band by this many decibels.
const MAX_DRIVE_DB: f64 = 40.0;
/// How much lower the curve's negative half limits.
const ASYMMETRY: f64 = 0.3;
/// Offset added before the curve, as a bias does in a circuit. Moving the zero crossing off
/// the curve's centre makes the two halves of the wave unequal in length, which keeps even
/// harmonics coming even when the curve is driven flat; asymmetric limits alone would only
/// shift the level.
const BIAS: f64 = 0.3;
/// Level of the driven signal before the output control (-6 dB), so full drive is not much
/// louder than the dry signal.
const WET_LEVEL: f64 = 0.5;
/// Average input level (of the band) at which dynamics leaves the level as it is.
const DYNAMICS_REFERENCE: f64 = 0.25;
/// Time constant of the level follower for dynamics.
const FOLLOWER_SECONDS: f64 = 0.02;
/// Tone at 0 and at 1: the low-pass corner, in hertz.
const TONE_RANGE: (f64, f64) = (1_000.0, 16_000.0);

/// Parameter indices, in [`INFO`] order.
pub mod param {
    pub const DRIVE: usize = 0;
    pub const TONE: usize = 1;
    pub const BAND_FREQUENCY: usize = 2;
    pub const BAND_WIDTH: usize = 3;
    pub const DYNAMICS: usize = 4;
    pub const MIX: usize = 5;
    pub const OUTPUT: usize = 6;
    pub(crate) const COUNT: usize = 7;
}

const PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::continuous("drive", "Drive", (0.0, 1.0), 0.5, Unit::Percent, Scale::Linear),
    ParamInfo::continuous("tone", "Tone", (0.0, 1.0), 0.5, Unit::Percent, Scale::Linear),
    ParamInfo::continuous("bandFrequency", "Band Frequency", (50.0, 18_000.0), 800.0, Unit::Hertz, Scale::Logarithmic),
    ParamInfo::continuous("bandWidth", "Band Width", (0.5, 9.0), 6.5, Unit::Octaves, Scale::Linear),
    ParamInfo::continuous("dynamics", "Dynamics", (0.0, 1.0), 0.0, Unit::Percent, Scale::Linear),
    ParamInfo::continuous("mix", "Dry/Wet", (0.0, 1.0), 1.0, Unit::Percent, Scale::Linear),
    ParamInfo::continuous("output", "Output", (-24.0, 12.0), 0.0, Unit::Decibels, Scale::Linear),
];

pub const INFO: DeviceInfo = DeviceInfo { id: "overdrive", name: "Overdrive", role: Role::Effect, params: &PARAMS };

const DELAY: usize = LATENCY as usize;

/// The driven path of one side.
#[derive(Clone, Copy)]
struct Path {
    low_cut: Biquad,
    high_cut: Biquad,
    oversampler: Oversampler2x,
    dc: DcBlocker,
    tone: Biquad,
    follower: OnePole,
}

impl Path {
    fn new(sample_rate: f64) -> Self {
        Self {
            low_cut: Biquad::default(),
            high_cut: Biquad::default(),
            oversampler: Oversampler2x::new(),
            dc: DcBlocker::new(sample_rate),
            tone: Biquad::default(),
            follower: OnePole::new(FOLLOWER_SECONDS, sample_rate),
        }
    }

    /// The driven signal for input `x`, delayed by [`LATENCY`].
    #[inline]
    fn process(&mut self, x: f64, gain: f64, dynamics: f64) -> f64 {
        let band = self.high_cut.process(self.low_cut.process(x));
        let level = self.follower.process(band.abs());
        let rest = Curve::Soft.asymmetric(BIAS, ASYMMETRY);
        let shaped = self.oversampler.process(band, |v| Curve::Soft.asymmetric(gain * v + BIAS, ASYMMETRY) - rest);
        let wet = self.tone.process(self.dc.process(shaped)) * WET_LEVEL;
        let follow = (level / DYNAMICS_REFERENCE).min(1.0);
        wet * (1.0 + dynamics * (follow - 1.0))
    }
}

pub struct Overdrive {
    sample_rate: f64,
    params: ParamBank<{ param::COUNT }>,
    /// Drive as a linear gain into the curve.
    drive: Smoother,
    dynamics: Smoother,
    mix: Smoother,
    output: Smoother,
    paths: [Path; 2],
    dry: [[f32; DELAY]; 2],
    dry_at: usize,
}

impl Default for Overdrive {
    fn default() -> Self {
        Self::new()
    }
}

impl Overdrive {
    pub fn new() -> Self {
        let sr = DEFAULT_SAMPLE_RATE;
        let mut overdrive = Self {
            sample_rate: sr,
            params: ParamBank::new(&PARAMS),
            drive: Smoother::new(1.0, 1),
            dynamics: Smoother::new(0.0, 1),
            mix: Smoother::new(1.0, 1),
            output: Smoother::new(1.0, 1),
            paths: [Path::new(sr); 2],
            dry: [[0.0; DELAY]; 2],
            dry_at: 0,
        };
        overdrive.rebuild();
        overdrive
    }

    fn value(&self, index: usize) -> f64 {
        self.params.get(index)
    }

    fn targets(&self) -> [f32; 4] {
        [
            db_to_gain((self.value(param::DRIVE) * MAX_DRIVE_DB) as f32),
            self.value(param::DYNAMICS) as f32,
            self.value(param::MIX) as f32,
            db_to_gain(self.value(param::OUTPUT) as f32),
        ]
    }

    fn smoothers(&mut self) -> [&mut Smoother; 4] {
        [&mut self.drive, &mut self.dynamics, &mut self.mix, &mut self.output]
    }

    /// Sets the band and tone filters from the parameters.
    fn tune(&mut self) {
        let sr = self.sample_rate;
        let (centre, width) = (self.value(param::BAND_FREQUENCY), self.value(param::BAND_WIDTH));
        let half = 2.0f64.powf(width / 2.0);
        let low_cut = Coefficients::highpass(sr, centre / half, BUTTERWORTH_Q);
        let high_cut = Coefficients::lowpass(sr, centre * half, BUTTERWORTH_Q);
        let (bottom, top) = TONE_RANGE;
        let tone = Coefficients::lowpass(sr, bottom * (top / bottom).powf(self.value(param::TONE)), BUTTERWORTH_Q);
        for path in &mut self.paths {
            path.low_cut.set(low_cut);
            path.high_cut.set(high_cut);
            path.tone.set(tone);
        }
    }

    /// Filters and smoothers for the current sample rate, every state cleared.
    fn rebuild(&mut self) {
        let sr = self.sample_rate;
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, sr);
        let targets = self.targets();
        for (smoother, target) in self.smoothers().into_iter().zip(targets) {
            smoother.set_ramp(ramp);
            smoother.reset(target);
        }
        self.paths = [Path::new(sr); 2];
        self.tune();
        self.dry = [[0.0; DELAY]; 2];
        self.dry_at = 0;
    }
}

impl Device for Overdrive {
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
        if matches!(index, param::TONE | param::BAND_FREQUENCY | param::BAND_WIDTH) {
            self.tune();
        }
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    fn latency(&self) -> u32 {
        LATENCY
    }

    fn process(&mut self, _context: &ProcessContext, buffer: &mut [f32]) {
        for frame in buffer.as_chunks_mut::<2>().0 {
            let [gain, dynamics, mix, output] = self.smoothers().map(|s| f64::from(s.next_value()));
            for (side, sample) in frame.iter_mut().enumerate() {
                let x = f64::from(*sample);
                let mut dry = 0.0;
                if let Some(slot) = self.dry.get_mut(side).and_then(|d| d.get_mut(self.dry_at)) {
                    dry = f64::from(*slot);
                    *slot = *sample;
                }
                let wet = self.paths.get_mut(side).map_or(0.0, |p| p.process(x, gain, dynamics));
                let mixed = if mix == 0.0 { dry } else { dry + (wet - dry) * mix };
                *sample = (mixed * output) as f32;
            }
            self.dry_at = (self.dry_at + 1) % DELAY;
        }
    }
}

#[cfg(test)]
mod tests;
