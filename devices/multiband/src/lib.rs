// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multiband: three bands split by Linkwitz-Riley crossovers, each with an above stage (downward
//! compression or upward expansion over a threshold) and a below stage (upward compression or
//! downward expansion under a second threshold). `docs/spec/multiband.md` is the specification.
//!
//! Per frame: each side splits at the low crossover into low and the rest, the rest at the high
//! crossover into mid and high, and the low band goes through the high crossover's all-pass so the
//! bands add up flat. Per band, after its input gain, one level follower shared by both sides
//! follows the mean of the two (`|x|` in Peak mode, `x²` in RMS mode) with time constants of a
//! setting divided by 8 and scaled by Time, and acts within each cycle at short times. The level
//! in decibels goes through both stages (quadratic knees, a shared limit near 36 dB), the gains
//! add in decibels, Amount scales every gain of the device, and the bands are summed. The knee and
//! follower are the textbook designs (Giannoulis, Massberg and Reiss, *Digital Dynamic Range
//! Compressor Design*, JAES 2012); the code and the fitted constants are our own.

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, ParamBank, ParamInfo, ProcessContext, Role, Scale, Unit,
    check_sample_rate,
};
use suhina_dsp::gain::db_to_gain_f64;
use suhina_dsp::{AttackRelease, LinkwitzRiley, Smoother};

/// How long a gain or threshold change takes to arrive.
pub const RAMP_SECONDS: f64 = 0.02;
/// How long a crossover change takes to arrive.
pub const FREQUENCY_RAMP_SECONDS: f64 = 0.05;
/// While a crossover moves, the filters are retuned every this many frames.
const RETUNE_FRAMES: u64 = 16;
/// The level follower's time constant is a time setting divided by this.
pub const FOLLOWER_DIVISOR: f64 = 8.0;
/// Knee widths in decibels: the above stage with Soft Knee on and off, and the below stage.
pub const SOFT_KNEE_DB: f64 = 23.6;
pub const HARD_KNEE_DB: f64 = 10.0;
pub const BELOW_KNEE_DB: f64 = 16.3;
/// A stage's gain change approaches this many decibels and never passes it.
pub const GAIN_LIMIT_DB: f64 = 36.0;
/// How sharply a stage's gain bends into [`GAIN_LIMIT_DB`].
const LIMIT_SHARPNESS: f64 = 7.04;
/// Levels under this count as this, in decibels.
pub const LEVEL_FLOOR_DB: f64 = -100.0;

/// The bands, in parameter order.
pub const BANDS: [&str; 3] = ["low", "mid", "high"];

/// Parameter indices, in [`INFO`] order.
pub mod param {
    pub const LOW_CROSSOVER: usize = 0;
    pub const HIGH_CROSSOVER: usize = 1;
    pub const PEAK: usize = 2;
    pub const SOFT_KNEE: usize = 3;
    pub const AMOUNT: usize = 4;
    pub const TIME: usize = 5;
    pub const OUTPUT: usize = 6;
    /// The first band parameter; band `b`'s parameter `p` is `BAND + b * PER_BAND + p`.
    pub const BAND: usize = 7;
    pub const PER_BAND: usize = 9;
    pub const ON: usize = 0;
    pub const INPUT: usize = 1;
    pub const ABOVE_THRESHOLD: usize = 2;
    pub const ABOVE_SLOPE: usize = 3;
    pub const BELOW_THRESHOLD: usize = 4;
    pub const BELOW_SLOPE: usize = 5;
    pub const ATTACK: usize = 6;
    pub const RELEASE: usize = 7;
    pub const GAIN: usize = 8;
    /// Which band the panel shows the controls of (0 low, 1 mid, 2 high); it changes no sound.
    pub const SHOWN_BAND: usize = BAND + 3 * PER_BAND;
    pub(crate) const COUNT: usize = SHOWN_BAND + 1;

    /// The index of band `band`'s parameter `which`.
    pub const fn band(band: usize, which: usize) -> usize {
        BAND + band * PER_BAND + which
    }
}

const PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::continuous("lowCrossover", "Low Crossover", (30.0, 3_000.0), 120.0, Unit::Hertz, Scale::Logarithmic),
    ParamInfo::continuous(
        "highCrossover",
        "High Crossover",
        (300.0, 15_000.0),
        2_500.0,
        Unit::Hertz,
        Scale::Logarithmic,
    ),
    ParamInfo::toggle("peak", "Peak", false),
    ParamInfo::toggle("softKnee", "Soft Knee", true),
    ParamInfo::continuous("amount", "Amount", (0.0, 1.0), 1.0, Unit::Percent, Scale::Linear),
    ParamInfo::continuous("time", "Time", (0.1, 10.0), 1.0, Unit::Percent, Scale::Logarithmic),
    ParamInfo::continuous("output", "Output", (-24.0, 24.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::toggle("lowOn", "Low On", true),
    ParamInfo::continuous("lowInput", "Low Input", (-24.0, 24.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous(
        "lowAboveThreshold",
        "Low Above Threshold",
        (-80.0, 0.0),
        -20.0,
        Unit::Decibels,
        Scale::Linear,
    ),
    ParamInfo::continuous("lowAboveSlope", "Low Above Slope", (0.0, 2.0), 1.0, Unit::None, Scale::Linear),
    ParamInfo::continuous(
        "lowBelowThreshold",
        "Low Below Threshold",
        (-80.0, 0.0),
        -60.0,
        Unit::Decibels,
        Scale::Linear,
    ),
    ParamInfo::continuous("lowBelowSlope", "Low Below Slope", (0.0, 4.0), 1.0, Unit::None, Scale::Linear),
    ParamInfo::continuous("lowAttack", "Low Attack", (0.0001, 5.0), 0.05, Unit::Seconds, Scale::Logarithmic),
    ParamInfo::continuous("lowRelease", "Low Release", (0.0001, 5.0), 0.3, Unit::Seconds, Scale::Logarithmic),
    ParamInfo::continuous("lowGain", "Low Gain", (-24.0, 24.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::toggle("midOn", "Mid On", true),
    ParamInfo::continuous("midInput", "Mid Input", (-24.0, 24.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous(
        "midAboveThreshold",
        "Mid Above Threshold",
        (-80.0, 0.0),
        -20.0,
        Unit::Decibels,
        Scale::Linear,
    ),
    ParamInfo::continuous("midAboveSlope", "Mid Above Slope", (0.0, 2.0), 1.0, Unit::None, Scale::Linear),
    ParamInfo::continuous(
        "midBelowThreshold",
        "Mid Below Threshold",
        (-80.0, 0.0),
        -60.0,
        Unit::Decibels,
        Scale::Linear,
    ),
    ParamInfo::continuous("midBelowSlope", "Mid Below Slope", (0.0, 4.0), 1.0, Unit::None, Scale::Linear),
    ParamInfo::continuous("midAttack", "Mid Attack", (0.0001, 5.0), 0.01, Unit::Seconds, Scale::Logarithmic),
    ParamInfo::continuous("midRelease", "Mid Release", (0.0001, 5.0), 0.2, Unit::Seconds, Scale::Logarithmic),
    ParamInfo::continuous("midGain", "Mid Gain", (-24.0, 24.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::toggle("highOn", "High On", true),
    ParamInfo::continuous("highInput", "High Input", (-24.0, 24.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous(
        "highAboveThreshold",
        "High Above Threshold",
        (-80.0, 0.0),
        -20.0,
        Unit::Decibels,
        Scale::Linear,
    ),
    ParamInfo::continuous("highAboveSlope", "High Above Slope", (0.0, 2.0), 1.0, Unit::None, Scale::Linear),
    ParamInfo::continuous(
        "highBelowThreshold",
        "High Below Threshold",
        (-80.0, 0.0),
        -60.0,
        Unit::Decibels,
        Scale::Linear,
    ),
    ParamInfo::continuous("highBelowSlope", "High Below Slope", (0.0, 4.0), 1.0, Unit::None, Scale::Linear),
    ParamInfo::continuous("highAttack", "High Attack", (0.0001, 5.0), 0.005, Unit::Seconds, Scale::Logarithmic),
    ParamInfo::continuous("highRelease", "High Release", (0.0001, 5.0), 0.1, Unit::Seconds, Scale::Logarithmic),
    ParamInfo::continuous("highGain", "High Gain", (-24.0, 24.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::choice("band", "Band", &["Low", "Mid", "High"], 1),
];

pub const INFO: DeviceInfo = DeviceInfo { id: "multiband", name: "Multiband", role: Role::Effect, params: &PARAMS };

/// `u` through a quadratic knee `width` decibels wide centred on zero: 0 under it, `u` over it.
#[inline]
pub fn knee(u: f64, width: f64) -> f64 {
    if width > 1e-6 && u.abs() <= width / 2.0 {
        let into = u + width / 2.0;
        into * into / (2.0 * width)
    } else if u > 0.0 {
        u
    } else {
        0.0
    }
}

/// A stage's gain change in decibels, bent so it never passes [`GAIN_LIMIT_DB`] either way.
#[inline]
pub fn limit(gain_db: f64) -> f64 {
    let ratio = (gain_db / GAIN_LIMIT_DB).abs();
    gain_db / (1.0 + ratio.powf(LIMIT_SHARPNESS)).powf(1.0 / LIMIT_SHARPNESS)
}

/// The two stages' gain change at `level_db`, before Amount.
#[inline]
pub fn static_gain(level_db: f64, stage: &Stage, knee_db: f64) -> f64 {
    let above = (stage.above_slope - 1.0) * knee(level_db - stage.above_threshold, knee_db);
    let below = (1.0 - stage.below_slope) * knee(stage.below_threshold - level_db, BELOW_KNEE_DB);
    limit(above) + limit(below)
}

/// One band's static settings, read at the frame they apply.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stage {
    pub above_threshold: f64,
    pub above_slope: f64,
    pub below_threshold: f64,
    pub below_slope: f64,
}

/// The two crossovers and the all-pass of one side.
#[derive(Clone, Copy)]
struct Side {
    low_split: LinkwitzRiley,
    high_split: LinkwitzRiley,
    low_allpass: LinkwitzRiley,
}

impl Side {
    fn new(sample_rate: f64, low: f64, high: f64) -> Self {
        Self {
            low_split: LinkwitzRiley::new(sample_rate, low),
            high_split: LinkwitzRiley::new(sample_rate, high),
            low_allpass: LinkwitzRiley::new(sample_rate, high),
        }
    }

    fn tune(&mut self, sample_rate: f64, low: f64, high: f64) {
        self.low_split.set_frequency(sample_rate, low);
        self.high_split.set_frequency(sample_rate, high);
        self.low_allpass.set_frequency(sample_rate, high);
    }

    /// The (low, mid, high) bands of `x`, the low band phase-aligned with the others.
    #[inline]
    fn bands(&mut self, x: f64) -> [f64; 3] {
        let (low, rest) = self.low_split.split(x);
        let (mid, high) = self.high_split.split(rest);
        [self.low_allpass.allpass(low), mid, high]
    }
}

/// One band's running state.
#[derive(Clone, Copy)]
struct Band {
    input: Smoother,
    gain: Smoother,
    above_threshold: Smoother,
    below_threshold: Smoother,
    follower: AttackRelease,
    /// The follower's value: mean `|x|` (Peak) or mean `x²` (RMS).
    level: f64,
}

pub struct Multiband {
    sample_rate: f64,
    params: ParamBank<{ param::COUNT }>,
    bands: [Band; 3],
    output: Smoother,
    /// Crossovers, smoothed as log2 of hertz.
    frequencies: [Smoother; 2],
    sides: [Side; 2],
    frame: u64,
}

impl Default for Multiband {
    fn default() -> Self {
        Self::new()
    }
}

impl Multiband {
    pub fn new() -> Self {
        let band = Band {
            input: Smoother::new(0.0, 1),
            gain: Smoother::new(0.0, 1),
            above_threshold: Smoother::new(0.0, 1),
            below_threshold: Smoother::new(0.0, 1),
            follower: AttackRelease::default(),
            level: 0.0,
        };
        let mut device = Self {
            sample_rate: DEFAULT_SAMPLE_RATE,
            params: ParamBank::new(&PARAMS),
            bands: [band; 3],
            output: Smoother::new(0.0, 1),
            frequencies: [Smoother::new(0.0, 1); 2],
            sides: [Side::new(DEFAULT_SAMPLE_RATE, 120.0, 2_500.0); 2],
            frame: 0,
        };
        device.rebuild();
        device
    }

    fn value(&self, index: usize) -> f64 {
        self.params.get(index)
    }

    fn band_value(&self, band: usize, which: usize) -> f64 {
        self.value(param::band(band, which))
    }

    fn on(&self, band: usize) -> bool {
        self.band_value(band, param::ON) >= 0.5
    }

    fn peak(&self) -> bool {
        self.value(param::PEAK) >= 0.5
    }

    fn frequency_targets(&self) -> [f32; 2] {
        [self.value(param::LOW_CROSSOVER).log2() as f32, self.value(param::HIGH_CROSSOVER).log2() as f32]
    }

    fn current_frequencies(&self) -> (f64, f64) {
        let [low, high] = &self.frequencies;
        (2.0f64.powf(f64::from(low.current())), 2.0f64.powf(f64::from(high.current())))
    }

    /// Ramp targets of every smoothed value: band input, gain and thresholds in dB, output in dB.
    fn set_targets(&mut self, settle: bool) {
        for b in 0..BANDS.len() {
            let targets = [
                self.band_value(b, param::INPUT),
                self.band_value(b, param::GAIN),
                self.band_value(b, param::ABOVE_THRESHOLD),
                self.band_value(b, param::BELOW_THRESHOLD),
            ]
            .map(|v| v as f32);
            if let Some(band) = self.bands.get_mut(b) {
                let smoothers = [&mut band.input, &mut band.gain, &mut band.above_threshold, &mut band.below_threshold];
                for (smoother, target) in smoothers.into_iter().zip(targets) {
                    if settle { smoother.reset(target) } else { smoother.set_target(target) }
                }
            }
        }
        let output = self.value(param::OUTPUT) as f32;
        if settle {
            self.output.reset(output)
        } else {
            self.output.set_target(output)
        }
        let targets = self.frequency_targets();
        for (smoother, target) in self.frequencies.iter_mut().zip(targets) {
            if settle { smoother.reset(target) } else { smoother.set_target(target) }
        }
    }

    /// The followers' coefficients from the times, Time and the sample rate.
    fn tune_followers(&mut self) {
        let time = self.value(param::TIME);
        let sr = self.sample_rate;
        for b in 0..BANDS.len() {
            let attack = self.band_value(b, param::ATTACK) * time / FOLLOWER_DIVISOR;
            let release = self.band_value(b, param::RELEASE) * time / FOLLOWER_DIVISOR;
            if let Some(band) = self.bands.get_mut(b) {
                band.follower = AttackRelease::new(attack, release, sr);
            }
        }
    }

    /// Ramps, filters and followers for the current sample rate and values, settled and cleared.
    fn rebuild(&mut self) {
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, self.sample_rate);
        for band in &mut self.bands {
            for smoother in [&mut band.input, &mut band.gain, &mut band.above_threshold, &mut band.below_threshold] {
                smoother.set_ramp(ramp);
            }
            band.level = 0.0;
        }
        self.output.set_ramp(ramp);
        let frequency_ramp = Smoother::ramp_frames(FREQUENCY_RAMP_SECONDS, self.sample_rate);
        self.frequencies.iter_mut().for_each(|f| f.set_ramp(frequency_ramp));
        self.set_targets(true);
        let (low, high) = self.current_frequencies();
        self.sides = [Side::new(self.sample_rate, low, high); 2];
        self.tune_followers();
        self.frame = 0;
    }
}

impl Device for Multiband {
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
        self.set_targets(false);
        let band_param = index.checked_sub(param::BAND).map(|i| i % param::PER_BAND);
        if index == param::TIME || matches!(band_param, Some(param::ATTACK | param::RELEASE)) {
            self.tune_followers();
        }
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    fn process(&mut self, _context: &ProcessContext, buffer: &mut [f32]) {
        let amount = self.value(param::AMOUNT).clamp(0.0, 1.0);
        let peak = self.peak();
        let knee_db = if self.value(param::SOFT_KNEE) >= 0.5 { SOFT_KNEE_DB } else { HARD_KNEE_DB };
        let on = [self.on(0), self.on(1), self.on(2)];
        let slopes = [0, 1, 2].map(|b| {
            (
                self.band_value(b, param::ABOVE_SLOPE).clamp(0.0, 2.0),
                self.band_value(b, param::BELOW_SLOPE).clamp(0.0, 4.0),
            )
        });
        let sample_rate = self.sample_rate;
        for frame in buffer.as_chunks_mut::<2>().0 {
            let moving = !self.frequencies.iter().all(Smoother::is_settled);
            for f in &mut self.frequencies {
                f.next_value();
            }
            if moving && (self.frame.is_multiple_of(RETUNE_FRAMES) || self.frequencies.iter().all(Smoother::is_settled))
            {
                let (low, high) = self.current_frequencies();
                for side in &mut self.sides {
                    side.tune(sample_rate, low, high);
                }
            }
            self.frame = self.frame.wrapping_add(1);
            let [left, right] = frame.map(|s| if s.is_finite() { f64::from(s) } else { 0.0 });
            let [left_side, right_side] = &mut self.sides;
            let (lefts, rights) = (left_side.bands(left), right_side.bands(right));
            let mut sum = [0.0f64; 2];
            for ((((band, l), r), on), (above_slope, below_slope)) in
                self.bands.iter_mut().zip(lefts).zip(rights).zip(on).zip(slopes)
            {
                let input_db = f64::from(band.input.next_value());
                let gain_db = f64::from(band.gain.next_value());
                let above_threshold = f64::from(band.above_threshold.next_value());
                let below_threshold = f64::from(band.below_threshold.next_value());
                if !on {
                    sum[0] += l;
                    sum[1] += r;
                    continue;
                }
                let input = db_to_gain_f64(input_db * amount);
                let (l, r) = (l * input, r * input);
                let detected = if peak { 0.5 * (l.abs() + r.abs()) } else { 0.5 * (l * l + r * r) };
                band.level = band.follower.rise(band.level, detected);
                if band.level < 1e-30 {
                    band.level = 0.0;
                }
                let level_db =
                    if peak { 20.0 * band.level.max(1e-12).log10() } else { 10.0 * band.level.max(1e-24).log10() }
                        .max(LEVEL_FLOOR_DB);
                let stage = Stage { above_threshold, above_slope, below_threshold, below_slope };
                let gain = db_to_gain_f64((static_gain(level_db, &stage, knee_db) + gain_db) * amount);
                sum[0] += l * gain;
                sum[1] += r * gain;
            }
            let output = db_to_gain_f64(f64::from(self.output.next_value()) * amount);
            for (sample, x) in frame.iter_mut().zip(sum) {
                *sample = (x * output) as f32;
            }
        }
    }
}

#[cfg(test)]
mod tests;
