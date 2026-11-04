// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Saturator: a waveshaper with eight curves (docs/spec/saturator.md).
//!
//! Per side: an optional DC filter, then at four times the sample rate the Color filters (a low
//! shelf and a bell), the drive, the curve, the Color filters' exact inverse and the post clip; then
//! the output level and the dry/wet mix. Because the inverse filters undo the Color filters,
//! Color changes only how hard each band is driven, never the tone of what stays clean. A change
//! of curve or post clip fades over 20 ms from the sound that is playing; one that arrives while
//! a fade still runs waits for it, so clicking quickly through the list morphs in turn instead of
//! jumping. The dry signal is delayed to line up with the oversampled path: the device always
//! reports [`LATENCY`] frames, and with the mix at 0 the output is the input delayed exactly.

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, ParamBank, ParamInfo, ProcessContext, Role, Scale, Unit,
    check_sample_rate,
};
use suhina_dsp::gain::db_to_gain;
use suhina_dsp::shape::{Waveshaper, arctan_curve, knee_clip, quadratic_clip, sine_fold, soft_sine};
use suhina_dsp::{Coefficients, DcBlocker, Emphasis, FirstOrder, FirstOrderCoefficients, IirOversampler4x, Smoother};

/// Delay the device adds, in frames.
pub const LATENCY: u32 = IirOversampler4x::LATENCY + 1;
/// How many times the sample rate the curve and the Color filters run at.
const OVERSAMPLING: f64 = 4.0;
/// The oversampler delays the audio band by 3.29 frames; this many frames at the high rate (0.75
/// at the base rate) bring the wet signal to 4.04, so that against the dry signal, delayed by
/// [`LATENCY`], only the oversampler's phase turn above 5 kHz remains.
const ALIGN: usize = 3;
/// Ramp for every continuous change, and the crossfade when the curve changes.
pub const RAMP_SECONDS: f64 = 0.02;
/// Where Analog Clip stops being a straight line (-2.7 dB).
const ANALOG_KNEE: f64 = 0.73;
/// Medium Curve doubles the signal into a quadratic clip with its knee here: straight to a third
/// of full scale before the doubling, flat from two thirds.
const MEDIUM_GAIN: f64 = 2.0;
const MEDIUM_KNEE: f64 = 2.0 / 3.0;
/// Hard Curve is `atan(HARD_GAIN · u) / atan(HARD_GAIN)`: slope 6.8 at zero, 1 at `u` = 1.
const HARD_GAIN: f64 = 10.0;
/// Color Low at ±100 %: the gain of its very wide bell in decibels, at its centre.
const COLOR_LOW_DB: f64 = 36.0;
/// Color Low's bell: its centre and Q. So wide that above 100 Hz it falls about 6 dB per octave
/// like a shelf, and it is gone by 5 kHz; below the centre it falls again.
const COLOR_LOW_HZ: f64 = 41.4;
const COLOR_LOW_Q: f64 = 0.103;
/// Color High at ±100 %: the bell's gain in decibels.
const COLOR_HIGH_DB: f64 = 24.0;
/// The bell's Q is `NARROW_Q - WIDER_Q × width`: 2.04 at Color Width 0, 0.30 at 100 %.
const COLOR_NARROW_Q: f64 = 2.04;
const COLOR_WIDER_Q: f64 = 1.74;
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
            Self::MediumCurve => quadratic_clip(MEDIUM_GAIN * u, MEDIUM_KNEE),
            Self::HardCurve => arctan_curve(u, HARD_GAIN),
            Self::SinoidFold => sine_fold(held(u, FOLD_LIMIT)),
            Self::DigitalClip => {
                if u.is_finite() {
                    u.clamp(-1.0, 1.0)
                } else {
                    0.0
                }
            }
            Self::Waveshaper => shaper.apply(held(u, SHAPER_LIMIT)),
        }
    }
}

/// Sinoid Fold stops folding at its fifth peak: the driven signal is held to ±9 (the original's
/// fold at Drive +24 dB, gain and THD, from −5 to 0 dBFS).
const FOLD_LIMIT: f64 = 9.0;
/// The Waveshaper's driven signal is held to ±10, as measured at Drive +24 dB.
const SHAPER_LIMIT: f64 = 10.0;

/// `u` held to ±`limit`; non-finite input stays as it is for the curve to reject.
#[inline]
fn held(u: f64, limit: f64) -> f64 {
    if u.is_finite() { u.clamp(-limit, limit) } else { u }
}

/// Where the soft post clip stops being a straight line, as measured.
const SOFT_POST_KNEE: f64 = 0.6;

/// The post clip of [`POST_CLIP_NAMES`] at `y`; its ceiling is 1, before the output level.
/// Soft is straight to 0.6 and then a `tanh` towards ±1 (slope 1 at the knee); Hard holds ±1.
#[inline]
fn post_clip(mode: usize, y: f64) -> f64 {
    if !y.is_finite() {
        return if mode == 0 { y } else { 0.0 };
    }
    match mode {
        1 => {
            let a = y.abs();
            if a <= SOFT_POST_KNEE {
                y
            } else {
                let room = 1.0 - SOFT_POST_KNEE;
                (SOFT_POST_KNEE + room * ((a - SOFT_POST_KNEE) / room).tanh()).copysign(y)
            }
        }
        2 => y.clamp(-1.0, 1.0),
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

/// Corner of each of the DC filter's two one-pole high-passes: the original's, from its step
/// response and magnitude.
const DC_FILTER_HZ: f64 = 27.0;

/// The signal path of one side.
#[derive(Clone, Copy)]
struct Path {
    /// The DC filter: two identical one-pole high-passes at `DC_FILTER_HZ`.
    dc: [FirstOrder; 2],
    /// Takes the offset of the Waveshaper's squared term out, at four times the rate.
    shaper_dc: DcBlocker,
    oversampler: IirOversampler4x,
    /// The last [`ALIGN`] samples at the high rate, and where the next goes.
    align: [f64; ALIGN],
    align_at: usize,
    /// The Color filters before the curve and their exact inverses after it, at four times the
    /// rate: the inverses undo them sample for sample even while they glide.
    shelf: Emphasis,
    bell: Emphasis,
}

impl Path {
    fn new(sample_rate: f64) -> Self {
        Self {
            dc: [FirstOrder::new(FirstOrderCoefficients::highpass(sample_rate, DC_FILTER_HZ)); 2],
            shaper_dc: DcBlocker::new(OVERSAMPLING * sample_rate),
            oversampler: IirOversampler4x::new(),
            align: [0.0; ALIGN],
            align_at: 0,
            shelf: Emphasis::default(),
            bell: Emphasis::default(),
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
    /// The curve it fades from and the curve now.
    shapes: (Shape, Shape),
    /// The post clip of the stream it fades from and the one it fades to.
    posts: (usize, usize),
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
    /// The post clip of the stream it fades from, and the one it fades to.
    post: usize,
    post_next: usize,
    /// A curve and post clip waiting for the running fade to finish.
    pending: Option<(Shape, usize)>,
    /// Whether the Color filters run (at 0 they pass the signal unchanged).
    color_active: bool,
    /// Whether the Color amounts are at 0: the filters stop running once they have settled.
    color_idle: bool,
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
            post: 0,
            post_next: 0,
            pending: None,
            color_active: false,
            color_idle: true,
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
        let sr = OVERSAMPLING * self.sample_rate;
        let value = |i: usize| f64::from(self.smoothers.get(i).map_or(0.0, Smoother::current));
        let (low, high) = (value(smoothed::COLOR_LOW), value(smoothed::COLOR_HIGH));
        let frequency = 2.0f64.powf(value(smoothed::COLOR_OCTAVE));
        let width = value(smoothed::COLOR_WIDTH);
        let q = COLOR_NARROW_Q - COLOR_WIDER_Q * width.clamp(0.0, 1.0);
        let shelf = Coefficients::peak(sr, COLOR_LOW_HZ, COLOR_LOW_Q, COLOR_LOW_DB * low);
        let bell = Coefficients::peak(sr, frequency, q, COLOR_HIGH_DB * high);
        for path in &mut self.paths {
            path.shelf.set(shelf);
            path.bell.set(bell);
        }
        let settled = [smoothed::COLOR_LOW, smoothed::COLOR_HIGH]
            .iter()
            .all(|&i| self.smoothers.get(i).is_some_and(|s| s.is_settled() && s.target() == 0.0));
        self.color_idle = settled;
        self.color_active = !(settled && self.color_transparent());
    }

    /// Whether every Color filter passes the signal unchanged, with nothing left ringing.
    fn color_transparent(&self) -> bool {
        self.paths.iter().all(|path| path.shelf.is_transparent() && path.bell.is_transparent())
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
        let post = self.value(param::POST_CLIP).round() as usize;
        self.post = post;
        self.post_next = post;
        self.pending = None;
        self.paths = [Path::new(sr); 2];
        self.tune();
        self.retune_in = 0;
        self.gliding = false;
        self.dry = [[0.0; DELAY]; 2];
        self.dry_at = 0;
    }

    /// A change of the curve or the post clip. When the fade is settled it applies at once and
    /// the crossfade runs from the sound that is playing; while a fade runs, it waits for it, so
    /// two quick changes morph in turn instead of jumping.
    fn queue_wet_change(&mut self, shape: Option<Shape>, post: Option<usize>) {
        let settled = self.smoothers.get(smoothed::FADE).is_some_and(Smoother::is_settled);
        if settled {
            // The crossfade runs from the sound that is playing, whichever of the two changed:
            // with the fade settled that is the curve in use, so that a post clip change after
            // a curve change does not morph from a curve that is no longer playing.
            self.previous = self.shape;
            let mut changed = false;
            if let Some(new_shape) = shape.filter(|new_shape| *new_shape != self.shape) {
                self.shape = new_shape;
                changed = true;
            }
            if let Some(new_post) = post.filter(|new_post| *new_post != self.post_next) {
                self.post = self.post_next;
                self.post_next = new_post;
                changed = true;
            }
            if changed {
                if let Some(fade) = self.smoothers.get_mut(smoothed::FADE) {
                    fade.reset(0.0);
                    // reset settles at the value it is given; arm the ramp to the settled fade.
                    fade.set_target(1.0);
                }
                self.moving = true;
            }
        } else {
            let (base_shape, base_post) = self.pending.unwrap_or((self.shape, self.post_next));
            self.pending = Some((shape.unwrap_or(base_shape), post.unwrap_or(base_post)));
        }
    }

    fn color_moving(&self) -> bool {
        [smoothed::COLOR_LOW, smoothed::COLOR_HIGH, smoothed::COLOR_OCTAVE, smoothed::COLOR_WIDTH]
            .iter()
            .any(|&i| self.smoothers.get(i).is_some_and(|s| !s.is_settled()))
    }
}

/// One sample of one side through the oversampled stage.
#[inline]
fn shape_sample(path: &mut Path, x: f64, f: &Frame) -> f64 {
    let Path { oversampler, align, align_at, shelf, bell, shaper_dc, .. } = path;
    let y = oversampler.process(x, |v| {
        let v = if f.color { bell.emphasise(shelf.emphasise(v)) } else { v };
        let u = f.drive * v;
        let (before, now) = f.shapes;
        let fading = f.fade < 1.0;
        if now != Shape::Waveshaper && !(fading && before == Shape::Waveshaper) {
            shaper_dc.reset();
        }
        // The Waveshaper's squared term leaves an offset; the original's output has none.
        let mut branch = |shape: Shape| {
            let y = shape.apply(u, f.knee, &f.shaper);
            if shape == Shape::Waveshaper { shaper_dc.process(y) } else { y }
        };
        let mut y = branch(now);
        if fading {
            // Both are the Waveshaper only when its own fade-in is still running: nothing to blend.
            let old = if before == now { y } else { branch(before) };
            y = old + (y - old) * f.fade;
        }
        let y = if f.color { shelf.deemphasise(bell.deemphasise(y)) } else { y };
        let slot = align.get_mut(*align_at).map_or(y, |slot| std::mem::replace(slot, y));
        *align_at = (*align_at + 1) % ALIGN;
        slot
    });
    // The post clip runs at the base rate, after the downsampler, so it also holds the
    // downsampler's overshoot (the original's hard clip peaks at exactly 1).
    let (from, to) = f.posts;
    if from == to {
        post_clip(from, y)
    } else {
        let a = post_clip(from, y);
        let b = post_clip(to, y);
        a + (b - a) * f.fade
    }
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
            self.queue_wet_change(Some(shape), None);
        } else if index == param::POST_CLIP {
            let post = self.value(param::POST_CLIP).round() as usize;
            self.queue_wet_change(None, Some(post));
        } else {
            let targets = self.targets();
            for (smoother, target) in self.smoothers.iter_mut().zip(targets) {
                smoother.set_target(target);
            }
            self.moving = true;
            if self.color_moving() {
                self.color_active = true;
            }
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
            if self.moving {
                for (value, smoother) in self.values.iter_mut().zip(&mut self.smoothers) {
                    *value = f64::from(smoother.next_value());
                }
                self.moving = self.smoothers.iter().any(|s| !s.is_settled());
            }
            // A queued curve or post clip applies once the running fade has finished; with
            // nothing waiting, the post clip pair collapses to the one in use.
            if self.smoothers.get(smoothed::FADE).is_some_and(Smoother::is_settled) {
                if let Some((shape, post)) = self.pending.take() {
                    self.previous = self.shape;
                    self.shape = shape;
                    self.post = self.post_next;
                    self.post_next = post;
                    if let Some(fade) = self.smoothers.get_mut(smoothed::FADE) {
                        fade.reset(0.0);
                        self.values[smoothed::FADE] = 0.0;
                        // reset settles at the value it is given; arm the ramp to the settled fade.
                        fade.set_target(1.0);
                    }
                    self.moving = true;
                } else if self.post != self.post_next {
                    self.post = self.post_next;
                }
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
            } else if self.color_active && self.color_idle && self.color_transparent() {
                // Back at 0 the filters run on until what they held has died away: switched off
                // while it rings, what reaches the curve would jump.
                self.color_active = false;
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
                shapes: (self.previous, self.shape),
                posts: (self.post, self.post_next),
            };
            let (dc, output, mix) = (get(smoothed::DC), get(smoothed::OUTPUT), get(smoothed::MIX));
            for (side, sample) in frame.iter_mut().enumerate() {
                let x = f64::from(*sample);
                let mut dry = 0.0;
                if let Some(slot) = self.dry.get_mut(side).and_then(|d| d.get_mut(self.dry_at)) {
                    dry = f64::from(*slot);
                    *slot = *sample;
                }
                let wet = self.paths.get_mut(side).map_or(0.0, |path| {
                    let first = path.dc[0].process(x);
                    let filtered = path.dc[1].process(first);
                    let input = x + (filtered - x) * dc;
                    shape_sample(path, input, &f) * output
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
