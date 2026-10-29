// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! DJ EQ: three bands split by Linkwitz-Riley crossovers, each with a gain and a kill switch.
//!
//! Each side is split at the low frequency into low and the rest, and the rest at the high
//! frequency into mid and high. The low band is the low crossover's low-pass alone: no all-pass
//! is added anywhere, so with every gain at 0 dB the three bands keep a small dip at the low
//! crossover (−0.75 dB at most in the 24 dB setting, −2.14 dB in the 48 dB one, measured with
//! crossovers at 500 Hz and 2 kHz) instead of summing flat. A kill takes its band to silence; a
//! gain at its minimum does the same. Gains run from silence up to +6 dB. Gains, kills and
//! crossover frequencies are all smoothed, so they can be turned and automated while playing.
//! The crossovers are independent: a low frequency above the high one leaves the mid band
//! almost empty and the sum flat, as measured.

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, ParamBank, ParamInfo, ProcessContext, Role, Scale, Unit,
    check_sample_rate,
};
use suhina_dsp::gain::{SILENCE_DB, db_to_gain_or_silence};
use suhina_dsp::{LinkwitzRiley, Slope, Smoother};

/// How long a gain or kill change takes to arrive.
pub const RAMP_SECONDS: f64 = 0.02;
/// How long a crossover frequency change takes to arrive.
pub const FREQUENCY_RAMP_SECONDS: f64 = 0.05;
/// While a frequency moves, the filters are retuned every this many frames.
const RETUNE_FRAMES: u64 = 16;

/// Parameter indices, in [`INFO`] order.
pub mod param {
    pub const LOW: usize = 0;
    pub const MID: usize = 1;
    pub const HIGH: usize = 2;
    pub const LOW_KILL: usize = 3;
    pub const MID_KILL: usize = 4;
    pub const HIGH_KILL: usize = 5;
    pub const LOW_FREQUENCY: usize = 6;
    pub const HIGH_FREQUENCY: usize = 7;
    pub const SLOPE: usize = 8;
    pub(crate) const COUNT: usize = 9;
}

/// Band gains run from silence up to +6 dB.
pub const GAIN_RANGE: (f64, f64) = (SILENCE_DB as f64, 6.0);

const PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::continuous("low", "Low", GAIN_RANGE, 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("mid", "Mid", GAIN_RANGE, 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("high", "High", GAIN_RANGE, 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::toggle("lowKill", "Low Kill", false),
    ParamInfo::toggle("midKill", "Mid Kill", false),
    ParamInfo::toggle("highKill", "High Kill", false),
    ParamInfo::continuous("lowFrequency", "Low Frequency", (50.0, 5_000.0), 250.0, Unit::Hertz, Scale::Logarithmic),
    ParamInfo::continuous(
        "highFrequency",
        "High Frequency",
        (200.0, 18_000.0),
        2_500.0,
        Unit::Hertz,
        Scale::Logarithmic,
    ),
    ParamInfo::choice("slope", "Slope", &["24 dB/oct", "48 dB/oct"], 0),
];

pub const INFO: DeviceInfo = DeviceInfo { id: "dj-eq", name: "DJ EQ", role: Role::Effect, params: &PARAMS };

/// The two filters of one side.
#[derive(Clone, Copy)]
struct Side {
    low_split: LinkwitzRiley,
    high_split: LinkwitzRiley,
}

impl Side {
    fn new(sample_rate: f64, low: f64, high: f64, slope: Slope) -> Self {
        Self {
            low_split: LinkwitzRiley::with_slope(sample_rate, low, slope),
            high_split: LinkwitzRiley::with_slope(sample_rate, high, slope),
        }
    }

    fn filters(&mut self) -> [&mut LinkwitzRiley; 2] {
        [&mut self.low_split, &mut self.high_split]
    }

    fn tune(&mut self, sample_rate: f64, low: f64, high: f64) {
        self.low_split.set_frequency(sample_rate, low);
        self.high_split.set_frequency(sample_rate, high);
    }

    /// The (low, mid, high) bands of `x`: the low band is the low crossover's low-pass alone,
    /// the mid and high bands carry its high-pass.
    #[inline]
    fn bands(&mut self, x: f64) -> (f64, f64, f64) {
        let (low, rest) = self.low_split.split(x);
        let (mid, high) = self.high_split.split(rest);
        (low, mid, high)
    }
}

pub struct DjEq {
    sample_rate: f64,
    params: ParamBank<{ param::COUNT }>,
    gains: [Smoother; 3],
    /// Crossover frequencies, smoothed as log2 of hertz so a sweep sounds even.
    frequencies: [Smoother; 2],
    sides: [Side; 2],
    /// Frames since the last reset, for retuning at fixed frames whatever the block size.
    frame: u64,
}

impl Default for DjEq {
    fn default() -> Self {
        Self::new()
    }
}

impl DjEq {
    pub fn new() -> Self {
        let mut eq = Self {
            sample_rate: DEFAULT_SAMPLE_RATE,
            params: ParamBank::new(&PARAMS),
            gains: [Smoother::new(1.0, 1); 3],
            frequencies: [Smoother::new(0.0, 1); 2],
            sides: [Side::new(DEFAULT_SAMPLE_RATE, 250.0, 2_500.0, Slope::Db24); 2],
            frame: 0,
        };
        eq.rebuild();
        eq
    }

    fn value(&self, index: usize) -> f64 {
        self.params.get(index)
    }

    fn slope(&self) -> Slope {
        if self.value(param::SLOPE) >= 0.5 { Slope::Db48 } else { Slope::Db24 }
    }

    /// Crossover targets in log2 hertz.
    fn frequency_targets(&self) -> [f32; 2] {
        let low = self.value(param::LOW_FREQUENCY);
        let high = self.value(param::HIGH_FREQUENCY);
        [low.log2() as f32, high.log2() as f32]
    }

    fn gain_targets(&self) -> [f32; 3] {
        let band = |gain, kill| {
            if self.value(kill) >= 0.5 { 0.0 } else { db_to_gain_or_silence(self.value(gain) as f32) }
        };
        [band(param::LOW, param::LOW_KILL), band(param::MID, param::MID_KILL), band(param::HIGH, param::HIGH_KILL)]
    }

    fn current_frequencies(&self) -> (f64, f64) {
        let [low, high] = &self.frequencies;
        (2.0f64.powf(f64::from(low.current())), 2.0f64.powf(f64::from(high.current())))
    }

    /// Ramps, filters and smoothers for the current sample rate and values, settled.
    fn rebuild(&mut self) {
        let sr = self.sample_rate;
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, sr);
        let frequency_ramp = Smoother::ramp_frames(FREQUENCY_RAMP_SECONDS, sr);
        self.gains.iter_mut().for_each(|g| g.set_ramp(ramp));
        self.frequencies.iter_mut().for_each(|f| f.set_ramp(frequency_ramp));
        let targets = self.gain_targets();
        for (smoother, target) in self.gains.iter_mut().zip(targets) {
            smoother.reset(target);
        }
        let targets = self.frequency_targets();
        for (smoother, target) in self.frequencies.iter_mut().zip(targets) {
            smoother.reset(target);
        }
        let (low, high) = self.current_frequencies();
        self.sides = [Side::new(sr, low, high, self.slope()); 2];
        self.frame = 0;
    }
}

impl Device for DjEq {
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
        match index {
            param::LOW_FREQUENCY | param::HIGH_FREQUENCY => {
                let targets = self.frequency_targets();
                for (smoother, target) in self.frequencies.iter_mut().zip(targets) {
                    smoother.set_target(target);
                }
            }
            param::SLOPE => {
                let slope = self.slope();
                for side in &mut self.sides {
                    side.filters().into_iter().for_each(|f| f.set_slope(slope));
                }
            }
            _ => {
                let targets = self.gain_targets();
                for (smoother, target) in self.gains.iter_mut().zip(targets) {
                    smoother.set_target(target);
                }
            }
        }
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    fn process(&mut self, _context: &ProcessContext, buffer: &mut [f32]) {
        for frame in buffer.as_chunks_mut::<2>().0 {
            let moving = !self.frequencies.iter().all(Smoother::is_settled);
            self.frequencies.iter_mut().for_each(|f| {
                f.next_value();
            });
            if moving && (self.frame.is_multiple_of(RETUNE_FRAMES) || self.frequencies.iter().all(Smoother::is_settled))
            {
                let (sr, (low, high)) = (self.sample_rate, self.current_frequencies());
                self.sides.iter_mut().for_each(|side| side.tune(sr, low, high));
            }
            self.frame = self.frame.wrapping_add(1);

            let [low_gain, mid_gain, high_gain] = self.gains.each_mut().map(|g| f64::from(g.next_value()));
            for (sample, side) in frame.iter_mut().zip(&mut self.sides) {
                let (low, mid, high) = side.bands(f64::from(*sample));
                *sample = (low * low_gain + mid * mid_gain + high * high_gain) as f32;
            }
        }
    }
}

#[cfg(test)]
mod tests;
