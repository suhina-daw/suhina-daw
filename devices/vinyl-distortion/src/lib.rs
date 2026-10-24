// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Vinyl distortion: the ways a record and its stylus bend a sound, as three effects.
//!
//! - **Tracing:** a band multiplied by its own shaped copy, which adds even harmonics, like a
//!   stylus that does not follow the groove exactly, without flattening the band itself.
//! - **Pinch:** a band goes through a symmetric curve (odd harmonics). In stereo mode what it
//!   adds goes into the left side and, inverted, into the right, so the distortion comes out of
//!   phase between the speakers, as the lateral motion of a groove does, even from a mono
//!   source; in mono mode it acts on the mid signal and goes into both sides alike.
//! - **Crackle:** random clicks at a chosen density and level.
//!
//! Each effect's band is shaped at twice the sample rate ([`Oversampler2x`]) and only the
//! difference it makes is added to the signal, so with every drive at 0 and the crackle off the
//! output is the input exactly, delayed by the oversampler's [`LATENCY`] frames (reported to the
//! host for compensation, whatever the settings).

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, ParamBank, ParamInfo, ProcessContext, Role, Scale, Unit,
    check_sample_rate,
};
use suhina_dsp::gain::{SILENCE_DB, db_to_gain_or_silence};
use suhina_dsp::shape::Curve;
use suhina_dsp::{Biquad, Coefficients, DcBlocker, Oversampler2x, Smoother, time_constant};

/// Delay the device adds, in frames.
pub const LATENCY: u32 = Oversampler2x::LATENCY;
/// Ramp for drive and crackle level changes.
pub const RAMP_SECONDS: f64 = 0.02;
/// How long one crackle click takes to fade to about a third.
const CLICK_SECONDS: f64 = 0.000_25;
/// Tracing drive at 1.0 raises the band by this factor (+24 dB) before the curve.
const TRACING_MAX_GAIN: f64 = 16.0;
/// Pinch drive at 1.0 raises the band by this factor (+12 dB). Pinch flattens its band as it
/// distorts it, so it gets less gain than tracing.
const PINCH_MAX_GAIN: f64 = 4.0;
/// Level of the tracing distortion relative to its band, once the curve is saturated.
const TRACING_BIAS: f64 = 0.5;

/// Parameter indices, in [`INFO`] order.
pub mod param {
    pub const TRACING: usize = 0;
    pub const TRACING_DRIVE: usize = 1;
    pub const TRACING_FREQUENCY: usize = 2;
    pub const TRACING_WIDTH: usize = 3;
    pub const PINCH: usize = 4;
    pub const PINCH_DRIVE: usize = 5;
    pub const PINCH_FREQUENCY: usize = 6;
    pub const PINCH_WIDTH: usize = 7;
    pub const PINCH_MODE: usize = 8;
    pub const CURVE: usize = 9;
    pub const CRACKLE_LEVEL: usize = 10;
    pub const CRACKLE_DENSITY: usize = 11;
    pub(crate) const COUNT: usize = 12;
}

const FREQUENCY_RANGE: (f64, f64) = (50.0, 18_000.0);
const WIDTH_RANGE: (f64, f64) = (0.1, 4.0);

const PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::toggle("tracing", "Tracing", true),
    ParamInfo::continuous("tracingDrive", "Tracing Drive", (0.0, 1.0), 0.25, Unit::Percent, Scale::Linear),
    ParamInfo::continuous(
        "tracingFrequency",
        "Tracing Frequency",
        FREQUENCY_RANGE,
        2_500.0,
        Unit::Hertz,
        Scale::Logarithmic,
    ),
    ParamInfo::continuous("tracingWidth", "Tracing Width", WIDTH_RANGE, 1.0, Unit::Octaves, Scale::Linear),
    ParamInfo::toggle("pinch", "Pinch", false),
    ParamInfo::continuous("pinchDrive", "Pinch Drive", (0.0, 1.0), 0.25, Unit::Percent, Scale::Linear),
    ParamInfo::continuous(
        "pinchFrequency",
        "Pinch Frequency",
        FREQUENCY_RANGE,
        5_000.0,
        Unit::Hertz,
        Scale::Logarithmic,
    ),
    ParamInfo::continuous("pinchWidth", "Pinch Width", WIDTH_RANGE, 1.0, Unit::Octaves, Scale::Linear),
    ParamInfo::choice("pinchMode", "Pinch Mode", &["Stereo", "Mono"], 0),
    ParamInfo::choice("curve", "Curve", &["Soft", "Hard"], 0),
    ParamInfo::continuous(
        "crackleLevel",
        "Crackle",
        (SILENCE_DB as f64, 0.0),
        SILENCE_DB as f64,
        Unit::Decibels,
        Scale::Linear,
    ),
    // Clicks per second, on average.
    ParamInfo::continuous("crackleDensity", "Crackle Density", (0.1, 200.0), 10.0, Unit::Hertz, Scale::Logarithmic),
];

pub const INFO: DeviceInfo =
    DeviceInfo { id: "vinyl-distortion", name: "Vinyl Distortion", role: Role::Effect, params: &PARAMS };

/// Seed of the crackle's random numbers after a reset.
const SEED: u32 = 0x1234_5678;
const DELAY: usize = LATENCY as usize;

/// One distortion effect on one signal (a channel, or mid or side).
#[derive(Clone, Copy)]
struct Band {
    filter: Biquad,
    oversampler: Oversampler2x,
    dc: DcBlocker,
}

impl Band {
    fn new(sample_rate: f64, frequency: f64, octaves: f64) -> Self {
        Self {
            filter: Biquad::new(Coefficients::bandpass(sample_rate, frequency, Coefficients::q_for_octaves(octaves))),
            oversampler: Oversampler2x::new(),
            dc: DcBlocker::new(sample_rate),
        }
    }

    fn tune(&mut self, sample_rate: f64, frequency: f64, octaves: f64) {
        self.filter.set(Coefficients::bandpass(sample_rate, frequency, Coefficients::q_for_octaves(octaves)));
    }

    fn reset(&mut self) {
        self.filter.reset();
        self.oversampler.reset();
        self.dc.reset();
    }

    /// What distortion adds to the band of `x` at `drive` (0..1), delayed by [`LATENCY`].
    /// `added` gets the band and the drive gain and returns what to add to the band.
    #[inline]
    fn difference(&mut self, x: f64, drive: f64, max_gain: f64, added: impl Fn(f64, f64) -> f64) -> f64 {
        let band = self.filter.process(x);
        let gain = 1.0 + drive * (max_gain - 1.0);
        let added = self.oversampler.process(band, |v| added(v, gain));
        self.dc.process(added) * drive
    }
}

pub struct VinylDistortion {
    sample_rate: f64,
    params: ParamBank<{ param::COUNT }>,
    tracing_drive: Smoother,
    pinch_drive: Smoother,
    crackle_gain: Smoother,
    tracing: [Band; 2],
    /// Pinch per side (stereo mode); mono mode uses the first on the mid signal.
    pinch: [Band; 2],
    /// The input, delayed to line up with the oversampled differences.
    dry: [[f32; DELAY]; 2],
    dry_at: usize,
    seed: u32,
    /// The click that is sounding, per side.
    click: [f64; 2],
    click_decay: f64,
}

impl Default for VinylDistortion {
    fn default() -> Self {
        Self::new()
    }
}

impl VinylDistortion {
    pub fn new() -> Self {
        let sr = DEFAULT_SAMPLE_RATE;
        let params = ParamBank::new(&PARAMS);
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, sr);
        let mut vinyl = Self {
            sample_rate: sr,
            params,
            tracing_drive: Smoother::new(0.0, ramp),
            pinch_drive: Smoother::new(0.0, ramp),
            crackle_gain: Smoother::new(0.0, ramp),
            tracing: [Band::new(sr, 2_500.0, 1.0); 2],
            pinch: [Band::new(sr, 5_000.0, 1.0); 2],
            dry: [[0.0; DELAY]; 2],
            dry_at: 0,
            seed: SEED,
            click: [0.0; 2],
            click_decay: 0.0,
        };
        vinyl.rebuild();
        vinyl
    }

    fn value(&self, index: usize) -> f64 {
        self.params.get(index)
    }

    fn curve(&self) -> Curve {
        if self.value(param::CURVE) >= 0.5 { Curve::Hard } else { Curve::Soft }
    }

    fn targets(&self) -> [f32; 3] {
        let on = |toggle: usize, drive: usize| if self.value(toggle) >= 0.5 { self.value(drive) as f32 } else { 0.0 };
        [
            on(param::TRACING, param::TRACING_DRIVE),
            on(param::PINCH, param::PINCH_DRIVE),
            db_to_gain_or_silence(self.value(param::CRACKLE_LEVEL) as f32),
        ]
    }

    fn smoothers(&mut self) -> [&mut Smoother; 3] {
        [&mut self.tracing_drive, &mut self.pinch_drive, &mut self.crackle_gain]
    }

    fn tune(&mut self) {
        let sr = self.sample_rate;
        let (frequency, width) = (self.value(param::TRACING_FREQUENCY), self.value(param::TRACING_WIDTH));
        self.tracing.iter_mut().for_each(|band| band.tune(sr, frequency, width));
        let (frequency, width) = (self.value(param::PINCH_FREQUENCY), self.value(param::PINCH_WIDTH));
        self.pinch.iter_mut().for_each(|band| band.tune(sr, frequency, width));
    }

    /// Ramps and filters for the current sample rate, every state cleared and smoothers settled.
    fn rebuild(&mut self) {
        let sr = self.sample_rate;
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, sr);
        let targets = self.targets();
        for (smoother, target) in self.smoothers().into_iter().zip(targets) {
            smoother.set_ramp(ramp);
            smoother.reset(target);
        }
        self.tracing = [Band::new(sr, 2_500.0, 1.0); 2];
        self.pinch = [Band::new(sr, 5_000.0, 1.0); 2];
        self.tune();
        self.dry = [[0.0; DELAY]; 2];
        self.dry_at = 0;
        self.seed = SEED;
        self.click = [0.0; 2];
        self.click_decay = time_constant(CLICK_SECONDS, sr);
    }

    /// The next random number, from 0.0 to 1.0.
    fn random(&mut self) -> f64 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f64::from(self.seed >> 8) / f64::from(1u32 << 24)
    }

    /// The crackle for one frame, before its level.
    fn crackle(&mut self) -> [f64; 2] {
        let chance = self.value(param::CRACKLE_DENSITY) / self.sample_rate;
        if self.random() < chance {
            let size = 0.3 + 0.7 * self.random();
            let sign = if self.random() < 0.5 { -1.0 } else { 1.0 };
            // Clicks sit near the centre, a little to one side or the other.
            let pan = (self.random() - 0.5) * 0.6;
            self.click[0] += sign * size * (1.0 - pan);
            self.click[1] += sign * size * (1.0 + pan);
        }
        let out = self.click;
        let decay = self.click_decay;
        self.click.iter_mut().for_each(|c| *c *= decay);
        out
    }
}

/// Starts `band` from silence if its effect is about to come on after being fully off.
fn wake(band: &mut Band, smoother: &Smoother, target: f32) {
    if target > 0.0 && smoother.current() == 0.0 && smoother.is_settled() {
        band.reset();
    }
}

impl Device for VinylDistortion {
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
        let [tracing, pinch, crackle] = self.targets();
        for band in &mut self.tracing {
            wake(band, &self.tracing_drive, tracing);
        }
        for band in &mut self.pinch {
            wake(band, &self.pinch_drive, pinch);
        }
        self.tracing_drive.set_target(tracing);
        self.pinch_drive.set_target(pinch);
        self.crackle_gain.set_target(crackle);
        self.tune();
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    fn latency(&self) -> u32 {
        LATENCY
    }

    fn process(&mut self, _context: &ProcessContext, buffer: &mut [f32]) {
        let curve = self.curve();
        let stereo_pinch = self.value(param::PINCH_MODE) < 0.5;
        for frame in buffer.as_chunks_mut::<2>().0 {
            let input = frame.map(f64::from);
            let mut out = [0.0f64; 2];
            for (side, (sample, delayed)) in frame.iter().zip(&mut out).enumerate() {
                if let Some(slot) = self.dry.get_mut(side).and_then(|d| d.get_mut(self.dry_at)) {
                    *delayed = f64::from(*slot);
                    *slot = *sample;
                }
            }
            self.dry_at = (self.dry_at + 1) % DELAY;

            let tracing = f64::from(self.tracing_drive.next_value());
            if tracing > 0.0 {
                for ((band, x), y) in self.tracing.iter_mut().zip(input).zip(&mut out) {
                    // The band times its own shaped copy: an even function, so even harmonics
                    // only, at a level that follows the band.
                    *y += band
                        .difference(x, tracing, TRACING_MAX_GAIN, |v, gain| TRACING_BIAS * v * curve.apply(gain * v));
                }
            }

            let pinch = f64::from(self.pinch_drive.next_value());
            if pinch > 0.0 {
                let pinched = |v: f64, gain: f64| curve.apply(gain * v) / gain - v;
                let [l, r] = input;
                let [left_band, right_band] = &mut self.pinch;
                if stereo_pinch {
                    out[0] += left_band.difference(l, pinch, PINCH_MAX_GAIN, pinched);
                    out[1] -= right_band.difference(r, pinch, PINCH_MAX_GAIN, pinched);
                } else {
                    let added = left_band.difference((l + r) / 2.0, pinch, PINCH_MAX_GAIN, pinched);
                    out[0] += added;
                    out[1] += added;
                }
            }

            let level = f64::from(self.crackle_gain.next_value());
            let clicks = self.crackle();
            if level > 0.0 {
                out[0] += clicks[0] * level;
                out[1] += clicks[1] * level;
            }

            *frame = out.map(|v| v as f32);
        }
    }
}

#[cfg(test)]
mod tests;
