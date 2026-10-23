// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Saturator: a waveshaper with eight curves (docs/spec/saturator.md).
//!
//! Per side: an optional DC filter, then at twice the sample rate the Color filters (a low shelf
//! and a bell), the drive, the curve, the Color filters' exact inverse and the post clip; then
//! the output level and the dry/wet mix. Because the inverse filters undo the Color filters,
//! Color changes only how hard each band is driven, never the tone of what stays clean. The dry
//! signal is delayed to line up with the oversampled path: the device always reports
//! [`LATENCY`] frames, and with the mix at 0 the output is the input delayed exactly.

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, ParamBank, ParamInfo, ProcessContext, Role, Scale, Unit,
    check_sample_rate,
};
use suhina_dsp::gain::db_to_gain;
use suhina_dsp::shape::{Waveshaper, knee_clip, sigmoid, sine_fold, soft_sine};
use suhina_dsp::{Biquad, Coefficients, DcBlocker, FirstOrder, FirstOrderCoefficients, IirOversampler2x, Smoother};

/// Delay the device adds, in frames.
pub const LATENCY: u32 = IirOversampler2x::LATENCY;
/// Ramp for every continuous change, and the crossfade when the curve changes.
pub const RAMP_SECONDS: f64 = 0.02;
/// Where Analog Clip stops being a straight line (-3 dB).
const ANALOG_KNEE: f64 = 0.707_946;
/// Medium Curve is Analog Clip with this much more gain.
const MEDIUM_GAIN: f64 = 2.0;
/// Hard Curve's slope at zero and how sharply it turns.
const HARD_GAIN: f64 = 6.7;
const HARD_SHARPNESS: f64 = 1.75;
/// Color Low at ±100 %: the low shelf's gain in decibels.
const COLOR_LOW_DB: f64 = 37.0;
/// Color Low's shelf is half way (in decibels) at this frequency, and its Q.
const COLOR_LOW_HZ: f64 = 400.0;
/// Color High at ±100 %: the bell's gain in decibels.
const COLOR_HIGH_DB: f64 = 24.0;
/// Color Width 0 and the extra at 100 %: the bell's width between its half-gain points, in
/// octaves, is `NARROW + WIDER × width²`.
const COLOR_NARROW_OCTAVES: f64 = 0.71;
const COLOR_WIDER_OCTAVES: f64 = 2.89;
/// While the Color controls move, the filters are retuned every this many frames.
const RETUNE_FRAMES: u32 = 16;

/// The curves, in [`param::TYPE`] order.
pub const TYPE_NAMES: [&str; 8] = [
    "Analog Clip",
    "Soft Sine",
    "Bass Shaper",
    "Medium Curve",
    "Hard Curve",
    "Sinoid Fold",
    "Digital Clip",
    "Waveshaper",
];
/// What happens after the curve, in [`param::POST_CLIP`] order.
pub const POST_CLIP_NAMES: [&str; 3] = ["Off", "Soft", "Hard"];

/// Parameter indices, in [`INFO`] order.
pub mod param {
    pub const DRIVE: usize = 0;
    pub const TYPE: usize = 1;
    pub const THRESHOLD: usize = 2;
    pub const POST_CLIP: usize = 3;
    pub const OUTPUT: usize = 4;
    pub const MIX: usize = 5;
    pub const COLOR: usize = 6;
    pub const COLOR_LOW: usize = 7;
    pub const COLOR_HIGH: usize = 8;
    pub const COLOR_FREQUENCY: usize = 9;
    pub const COLOR_WIDTH: usize = 10;
    pub const SHAPER_DRIVE: usize = 11;
    pub const SHAPER_CURVE: usize = 12;
    pub const SHAPER_DEPTH: usize = 13;
    pub const SHAPER_LINEAR: usize = 14;
    pub const SHAPER_DAMP: usize = 15;
    pub const SHAPER_PERIOD: usize = 16;
    pub const DC_FILTER: usize = 17;
    pub(crate) const COUNT: usize = 18;
}

const PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::continuous("drive", "Drive", (-36.0, 36.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::choice("type", "Type", &TYPE_NAMES, 0),
    ParamInfo::continuous("threshold", "Bass Threshold", (-50.0, 0.0), -50.0, Unit::Decibels, Scale::Linear),
    ParamInfo::choice("postClip", "Post Clip", &POST_CLIP_NAMES, 0),
    ParamInfo::continuous("output", "Output", (-36.0, 0.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("mix", "Dry/Wet", (0.0, 1.0), 1.0, Unit::Percent, Scale::Linear),
    ParamInfo::toggle("color", "Color", true).in_group("Color"),
    ParamInfo::continuous("colorLow", "Color Low", (-1.0, 1.0), 0.0, Unit::Percent, Scale::Linear).in_group("Color"),
    ParamInfo::continuous("colorHigh", "Color High", (-1.0, 1.0), 0.0, Unit::Percent, Scale::Linear).in_group("Color"),
    ParamInfo::continuous(
        "colorFrequency",
        "Color Frequency",
        (30.0, 18_500.0),
        1_000.0,
        Unit::Hertz,
        Scale::Logarithmic,
    )
    .in_group("Color"),
    ParamInfo::continuous("colorWidth", "Color Width", (0.0, 1.0), 0.3, Unit::Percent, Scale::Linear).in_group("Color"),
    ParamInfo::continuous("shaperDrive", "Shaper Drive", (0.0, 1.0), 1.0, Unit::Percent, Scale::Linear)
        .in_group("Waveshaper"),
    ParamInfo::continuous("shaperCurve", "Shaper Curve", (0.0, 1.0), 0.05, Unit::Percent, Scale::Linear)
        .in_group("Waveshaper"),
    ParamInfo::continuous("shaperDepth", "Shaper Depth", (0.0, 1.0), 0.0, Unit::Percent, Scale::Linear)
        .in_group("Waveshaper"),
    ParamInfo::continuous("shaperLinear", "Shaper Linear", (0.0, 1.0), 0.5, Unit::Percent, Scale::Linear)
        .in_group("Waveshaper"),
    ParamInfo::continuous("shaperDamp", "Shaper Damp", (0.0, 1.0), 0.0, Unit::Percent, Scale::Linear)
        .in_group("Waveshaper"),
    ParamInfo::continuous("shaperPeriod", "Shaper Period", (0.0, 1.0), 0.0, Unit::Percent, Scale::Linear)
        .in_group("Waveshaper"),
    ParamInfo::toggle("dcFilter", "DC Filter", false),
];

pub const INFO: DeviceInfo = DeviceInfo { id: "saturator", name: "Saturator", role: Role::Effect, params: &PARAMS };

const DELAY: usize = LATENCY as usize;

/// One of the curves of [`TYPE_NAMES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    AnalogClip,
    SoftSine,
    BassShaper,
    MediumCurve,
    HardCurve,
    SinoidFold,
    DigitalClip,
    Waveshaper,
}

impl Shape {
    fn from_index(index: f64) -> Self {
        match index as usize {
            1 => Self::SoftSine,
            2 => Self::BassShaper,
            3 => Self::MediumCurve,
            4 => Self::HardCurve,
            5 => Self::SinoidFold,
            6 => Self::DigitalClip,
            7 => Self::Waveshaper,
            _ => Self::AnalogClip,
        }
    }

    /// The curve at `u` (the driven signal). `knee` is the Bass Shaper's threshold as a gain.
    #[inline]
    fn apply(self, u: f64, knee: f64, shaper: &Waveshaper) -> f64 {
        match self {
            Self::AnalogClip => knee_clip(u, ANALOG_KNEE),
            Self::SoftSine => soft_sine(u),
            Self::BassShaper => knee_clip(u, knee),
            Self::MediumCurve => knee_clip(MEDIUM_GAIN * u, ANALOG_KNEE),
            Self::HardCurve => sigmoid(u, HARD_GAIN, HARD_SHARPNESS),
            Self::SinoidFold => sine_fold(u),
            Self::DigitalClip => {
                if u.is_finite() {
                    u.clamp(-1.0, 1.0)
                } else {
                    0.0
                }
            }
            Self::Waveshaper => shaper.apply(u),
        }
    }
}

/// The post clip of [`POST_CLIP_NAMES`] at `y`; its ceiling is 1, before the output level.
#[inline]
fn post_clip(mode: usize, y: f64) -> f64 {
    match mode {
        1 => knee_clip(y, ANALOG_KNEE),
        2 if y.is_finite() => y.clamp(-1.0, 1.0),
        2 => 0.0,
        _ => y,
    }
}

/// Smoothed values, by index into [`Saturator::smoothers`].
mod smoothed {
    pub const DRIVE: usize = 0;
    pub const OUTPUT: usize = 1;
    pub const MIX: usize = 2;
    pub const DC: usize = 3;
    pub const KNEE: usize = 4;
    /// From 0 (the old curve) to 1 (the new one).
    pub const FADE: usize = 5;
    /// Color Low and High as they act: 0 while Color is off.
    pub const COLOR_LOW: usize = 6;
    pub const COLOR_HIGH: usize = 7;
    /// The bell's frequency in octaves above 1 Hz, so it glides evenly.
    pub const COLOR_OCTAVE: usize = 8;
    pub const COLOR_WIDTH: usize = 9;
    pub const SHAPER: usize = 10;
    pub const COUNT: usize = 16;
}

/// The signal path of one side.
#[derive(Clone, Copy)]
struct Path {
    dc: DcBlocker,
    oversampler: IirOversampler2x,
    /// The Color filters before the curve and their inverses after it, at twice the rate.
    shelf: FirstOrder,
    bell: Biquad,
    unbell: Biquad,
    unshelf: FirstOrder,
}

impl Path {
    fn new(sample_rate: f64) -> Self {
        Self {
            dc: DcBlocker::new(sample_rate),
            oversampler: IirOversampler2x::new(),
            shelf: FirstOrder::default(),
            bell: Biquad::default(),
            unbell: Biquad::default(),
            unshelf: FirstOrder::default(),
        }
    }
}

/// What the curve stage needs for one frame.
struct Frame {
    drive: f64,
    knee: f64,
    fade: f64,
    shaper: Waveshaper,
    color: bool,
    post_clip: usize,
}

pub struct Saturator {
    sample_rate: f64,
    params: ParamBank<{ param::COUNT }>,
    smoothers: [Smoother; smoothed::COUNT],
    /// The smoothers' values for the current frame, and whether any of them is still moving.
    values: [f64; smoothed::COUNT],
    moving: bool,
    /// The curve now, and the one it fades from.
    shape: Shape,
    previous: Shape,
    /// Whether the Color filters are doing anything (they pass the signal unchanged at 0).
    color_active: bool,
    retune_in: u32,
    /// Whether the Color controls were moving at the last frame.
    gliding: bool,
    paths: [Path; 2],
    dry: [[f32; DELAY]; 2],
    dry_at: usize,
}

impl Default for Saturator {
    fn default() -> Self {
        Self::new()
    }
}

impl Saturator {
    pub fn new() -> Self {
        let sr = DEFAULT_SAMPLE_RATE;
        let mut saturator = Self {
            sample_rate: sr,
            params: ParamBank::new(&PARAMS),
            smoothers: [Smoother::new(0.0, 1); smoothed::COUNT],
            values: [0.0; smoothed::COUNT],
            moving: true,
            shape: Shape::AnalogClip,
            previous: Shape::AnalogClip,
            color_active: false,
            retune_in: 0,
            gliding: false,
            paths: [Path::new(sr); 2],
            dry: [[0.0; DELAY]; 2],
            dry_at: 0,
        };
        saturator.rebuild();
        saturator
    }

    fn value(&self, index: usize) -> f64 {
        self.params.get(index)
    }

    fn targets(&self) -> [f32; smoothed::COUNT] {
        let color = if self.value(param::COLOR) >= 0.5 { 1.0 } else { 0.0 };
        let shaper = [
            param::SHAPER_DRIVE,
            param::SHAPER_CURVE,
            param::SHAPER_DEPTH,
            param::SHAPER_LINEAR,
            param::SHAPER_DAMP,
            param::SHAPER_PERIOD,
        ]
        .map(|index| self.value(index) as f32);
        let [d, c, de, l, da, p] = shaper;
        [
            db_to_gain(self.value(param::DRIVE) as f32),
            db_to_gain(self.value(param::OUTPUT) as f32),
            self.value(param::MIX) as f32,
            self.value(param::DC_FILTER) as f32,
            db_to_gain(self.value(param::THRESHOLD) as f32),
            1.0,
            (color * self.value(param::COLOR_LOW)) as f32,
            (color * self.value(param::COLOR_HIGH)) as f32,
            self.value(param::COLOR_FREQUENCY).log2() as f32,
            self.value(param::COLOR_WIDTH) as f32,
            d,
            c,
            de,
            l,
            da,
            p,
        ]
    }

    /// Sets the Color filters from the smoothed values.
    fn tune(&mut self) {
        let sr = 2.0 * self.sample_rate;
        let value = |i: usize| f64::from(self.smoothers.get(i).map_or(0.0, Smoother::current));
        let (low, high) = (value(smoothed::COLOR_LOW), value(smoothed::COLOR_HIGH));
        let frequency = 2.0f64.powf(value(smoothed::COLOR_OCTAVE));
        let width = value(smoothed::COLOR_WIDTH);
        let q = Coefficients::q_for_octaves(COLOR_NARROW_OCTAVES + COLOR_WIDER_OCTAVES * width * width);
        let shelf_gain = suhina_dsp::gain::db_to_gain_f64(COLOR_LOW_DB * low);
        let shelf = FirstOrderCoefficients::low_shelf(sr, COLOR_LOW_HZ, shelf_gain);
        let unshelf = FirstOrderCoefficients::low_shelf(sr, COLOR_LOW_HZ, 1.0 / shelf_gain);
        let bell = Coefficients::peak(sr, frequency, q, COLOR_HIGH_DB * high);
        let unbell = Coefficients::peak(sr, frequency, q, -COLOR_HIGH_DB * high);
        for path in &mut self.paths {
            path.shelf.set(shelf);
            path.unshelf.set(unshelf);
            path.bell.set(bell);
            path.unbell.set(unbell);
        }
        let settled = [smoothed::COLOR_LOW, smoothed::COLOR_HIGH]
            .iter()
            .all(|&i| self.smoothers.get(i).is_some_and(|s| s.is_settled() && s.target() == 0.0));
        self.color_active = !settled;
    }

    /// Smoothers and filters for the current sample rate, every state cleared.
    fn rebuild(&mut self) {
        let sr = self.sample_rate;
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, sr);
        let targets = self.targets();
        for ((smoother, target), value) in self.smoothers.iter_mut().zip(targets).zip(&mut self.values) {
            smoother.set_ramp(ramp);
            smoother.reset(target);
            *value = f64::from(target);
        }
        self.moving = false;
        self.shape = Shape::from_index(self.value(param::TYPE));
        self.previous = self.shape;
        self.paths = [Path::new(sr); 2];
        self.tune();
        self.retune_in = 0;
        self.gliding = false;
        self.dry = [[0.0; DELAY]; 2];
        self.dry_at = 0;
    }

    fn color_moving(&self) -> bool {
        [smoothed::COLOR_LOW, smoothed::COLOR_HIGH, smoothed::COLOR_OCTAVE, smoothed::COLOR_WIDTH]
            .iter()
            .any(|&i| self.smoothers.get(i).is_some_and(|s| !s.is_settled()))
    }
}

/// One sample of one side through the oversampled stage.
#[inline]
fn shape_sample(path: &mut Path, x: f64, f: &Frame, shapes: (Shape, Shape)) -> f64 {
    let Path { oversampler, shelf, bell, unbell, unshelf, .. } = path;
    oversampler.process(x, |v| {
        let v = if f.color { bell.process(shelf.process(v)) } else { v };
        let u = f.drive * v;
        let (before, now) = shapes;
        let mut y = now.apply(u, f.knee, &f.shaper);
        if f.fade < 1.0 {
            let old = before.apply(u, f.knee, &f.shaper);
            y = old + (y - old) * f.fade;
        }
        let y = if f.color { unshelf.process(unbell.process(y)) } else { y };
        post_clip(f.post_clip, y)
    })
}

impl Device for Saturator {
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
        if index == param::TYPE {
            let shape = Shape::from_index(self.value(param::TYPE));
            if shape != self.shape {
                self.previous = self.shape;
                self.shape = shape;
                if let Some(fade) = self.smoothers.get_mut(smoothed::FADE) {
                    fade.reset(0.0);
                }
            }
        }
        let targets = self.targets();
        for (smoother, target) in self.smoothers.iter_mut().zip(targets) {
            smoother.set_target(target);
        }
        self.moving = true;
        if self.color_moving() {
            self.color_active = true;
        }
    }

    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    fn latency(&self) -> u32 {
        LATENCY
    }

    fn process(&mut self, _context: &ProcessContext, buffer: &mut [f32]) {
        let post = self.value(param::POST_CLIP).round() as usize;
        for frame in buffer.as_chunks_mut::<2>().0 {
            if self.moving {
                for (value, smoother) in self.values.iter_mut().zip(&mut self.smoothers) {
                    *value = f64::from(smoother.next_value());
                }
                self.moving = self.smoothers.iter().any(|s| !s.is_settled());
            }
            let values = self.values;
            let get = |i: usize| values.get(i).copied().unwrap_or(0.0);
            if self.moving && self.color_moving() {
                if self.retune_in == 0 {
                    self.tune();
                    self.retune_in = RETUNE_FRAMES;
                }
                self.retune_in -= 1;
                self.gliding = true;
            } else if self.gliding {
                // The end of a glide lands on the target.
                self.tune();
                self.gliding = false;
                self.retune_in = 0;
            }
            let at = |i: usize| get(smoothed::SHAPER + i);
            let f = Frame {
                drive: get(smoothed::DRIVE),
                knee: get(smoothed::KNEE),
                fade: get(smoothed::FADE),
                shaper: Waveshaper {
                    drive: at(0),
                    curve: at(1),
                    depth: at(2),
                    linear: at(3),
                    damp: at(4),
                    period: at(5),
                },
                color: self.color_active,
                post_clip: post,
            };
            let (dc, output, mix) = (get(smoothed::DC), get(smoothed::OUTPUT), get(smoothed::MIX));
            let shapes = (self.previous, self.shape);
            for (side, sample) in frame.iter_mut().enumerate() {
                let x = f64::from(*sample);
                let mut dry = 0.0;
                if let Some(slot) = self.dry.get_mut(side).and_then(|d| d.get_mut(self.dry_at)) {
                    dry = f64::from(*slot);
                    *slot = *sample;
                }
                let wet = self.paths.get_mut(side).map_or(0.0, |path| {
                    let filtered = path.dc.process(x);
                    let input = x + (filtered - x) * dc;
                    shape_sample(path, input, &f, shapes) * output
                });
                let mixed = if mix == 0.0 { dry } else { dry + (wet - dry) * mix };
                *sample = mixed as f32;
            }
            self.dry_at = (self.dry_at + 1) % DELAY;
        }
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod tests;
