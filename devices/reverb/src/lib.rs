// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reverb: an algorithmic room reverb in the stages a designer thinks in. The design is in
//! `docs/spec/reverb.md`.
//!
//! ```text
//! in -> mono sum -> 90 Hz roll-off, Low Cut, High Cut -> predelay line -+-> 32 taps (Spin) -> Reflect -+
//!                                                                        |                              +-> Stereo -> Dry/Wet
//!                                                                        +-> Shape delay -> 4 allpasses |
//!                                                                            -> network -> Diffuse -----+
//! ```
//!
//! The late reverb is a feedback delay network of 4, 8, 12 or 16 lines (Density) mixed by a
//! Hadamard matrix, the textbook design (Jot and Chaigne, 1991); each line's gain sets the
//! same decay time whatever its length, and shelves in the loop take away more of the highs or
//! lows per second. Size scales every delay with the cube root of the room's
//! volume. The delay lengths, tap pattern and constants are this device's own.

use suhina_device_api::{
    Device, DeviceError, DeviceInfo, ParamInfo, ProcessContext, Role, Scale, Unit, check_sample_rate,
};
use suhina_dsp::biquad::BUTTERWORTH_Q;
use suhina_dsp::{Allpass, Biquad, Coefficients, DelayLine, FirstOrder, FirstOrderCoefficients, Smoother};

/// Ramp for levels, mix, stereo and most other changes.
pub const RAMP_SECONDS: f64 = 0.02;
/// Ramp for predelay changes: slower, so a long jump bends the pitch less on the way.
pub const PREDELAY_RAMP_SECONDS: f64 = 0.1;
/// Size glides with this time constant when Smooth is Slow.
pub const SMOOTH_SLOW_SECONDS: f64 = 0.5;
/// Size glides with this time constant when Smooth is Fast.
pub const SMOOTH_FAST_SECONDS: f64 = 0.1;
/// Frames between updates of delay lengths, tap levels and filters, counted from the last reset.
const CONTROL_FRAMES: u64 = 32;

/// The reference room: delays below are for Size 100, and scale with `(size / 100)^(1/3)`.
const REFERENCE_SIZE: f64 = 100.0;
/// Early-reflection taps: time after the predelay (ms, Size 100), output side (0 left, 1 right)
/// and a level offset (dB).
const TAPS: [(f64, usize, f64); 32] = [
    (0.00, 0, 0.00),
    (2.24, 0, 0.46),
    (4.03, 0, 0.60),
    (6.11, 1, 0.94),
    (10.55, 0, -0.22),
    (14.10, 1, 0.77),
    (17.33, 1, -0.36),
    (20.65, 1, 1.47),
    (22.67, 1, -0.04),
    (26.38, 1, 0.98),
    (32.88, 1, -0.11),
    (36.63, 0, 0.80),
    (40.34, 1, -1.79),
    (42.86, 1, -0.05),
    (47.37, 1, -1.13),
    (51.30, 0, 1.08),
    (57.74, 1, 0.70),
    (62.17, 0, 0.58),
    (64.86, 1, -1.97),
    (68.97, 1, 1.37),
    (76.21, 0, 0.76),
    (78.69, 0, -0.01),
    (83.23, 0, 0.50),
    (88.23, 0, -1.53),
    (93.18, 1, -1.24),
    (99.04, 0, 0.21),
    (105.56, 0, -0.70),
    (109.34, 0, -1.67),
    (115.68, 1, -1.63),
    (118.70, 0, 0.94),
    (123.05, 0, -0.31),
    (130.75, 1, 0.58),
];
/// Each tap's side as a number, 0.0 left and 1.0 right.
const TAP_SIDES: [f64; 32] = {
    let mut sides = [0.0; 32];
    let mut k = 0;
    while k < 32 {
        sides[k] = TAPS[k].1 as f64;
        k += 1;
    }
    sides
};
/// Level of the first tap against the input.
const TAP_GAIN: f64 = 0.064;
/// How fast the reflections fade, in dB per ms after the first: this plus the next times Shape at
/// [`TAP_FADE_DECAY`], faster in proportion for shorter decays and slower for longer ones.
const TAP_FADE: f64 = 0.020;
/// How much faster the reflections fade per unit of Shape, in dB per ms.
const TAP_FADE_PER_SHAPE: f64 = 0.055;
/// Decay time (seconds) at which the reflections fade at [`TAP_FADE`] plus [`TAP_FADE_PER_SHAPE`]
/// times Shape.
const TAP_FADE_DECAY: f64 = 1.2;
/// The reflections are this many dB quieter per ms of predelay.
const TAP_PREDELAY_FADE: f64 = 0.057;
/// Peak swing of a tap's delay per unit of Spin Amount.
const SPIN_SECONDS: f64 = 0.000_7;
/// Peak swing of a line's delay per unit of Chorus Amount.
const CHORUS_SECONDS: f64 = 0.001_8;

/// Delay from the predelay to the diffusers: this plus the next times the size factor, plus the
/// next again times Shape - 0.5 and the size factor (ms). Puts the tail's onset where it sits for
/// the reference room.
const SHAPE_DELAY_MS: (f64, f64, f64) = (10.0, 14.0, 34.0);
/// The diffusers' delays at Scale 100 % in the reference room (ms).
const DIFFUSERS: [f64; 4] = [4.7, 7.3, 11.1, 16.9];
/// The network's lines in the reference room (ms), in the order Density turns them on: the first
/// 4, 8, 12 or all 16.
const LINES: [f64; 16] = [
    33.25, 72.60, 47.93, 103.32, 40.67, 85.46, 57.08, 127.18, 37.20, 78.39, 54.62, 115.08, 43.33, 93.96, 66.02, 140.45,
];
/// How many lines each Density runs.
const DENSITY_LINES: [usize; 4] = [4, 8, 12, 16];
/// Level of the tail for each Density against Low (dB), so the four sound equally loud, as
/// measured (docs/spec/reverb.md).
const DENSITY_TRIM_DB: [f64; 4] = [-4.0, 0.0, 0.0, 1.0];
/// Rooms larger than the reference one keep their tail's level: the network's input rises with
/// the size factor to this power (a larger network spreads the same energy over longer lines).
const LARGE_ROOM_POWER: f64 = 0.75;
/// Signs of the input into each line.
const INPUT_SIGNS: [f64; 16] = [1.0, 1.0, -1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, -1.0, -1.0, 1.0, -1.0, 1.0];
/// Signs of each line in the left and right outputs. The two patterns are orthogonal for every
/// Density, and lines of similar length, which carry similar energy, enter the two sides with the
/// same sign once and with opposite signs once, so the sides come out uncorrelated.
const OUTPUT_SIGNS: [[f64; 16]; 2] = [
    [1.0, -1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0, 1.0, -1.0],
    [-1.0, -1.0, 1.0, -1.0, -1.0, -1.0, 1.0, -1.0, 1.0, -1.0, -1.0, -1.0, -1.0, 1.0, -1.0, -1.0],
];
/// Level of the late reverb against the input, for the longest decays; shorter ones start lower,
/// as if the tail had already decayed for [`TAIL_FADE_SECONDS`] when it is heard.
const TAIL_GAIN: f64 = 1.525;
/// How long the shortest decays' tail counts as having decayed already when it is heard.
const TAIL_FADE_SECONDS: f64 = 0.027;
/// The damping filters' loss is per trip round a loop this long (seconds, reference room).
const LOSS_TRIP_SECONDS: f64 = 0.17;
/// The damping shelves are second-order (a cookbook shelf with this Q, half way at the set
/// frequency), steeper than first-order ones.
const SHELF_Q: f64 = BUTTERWORTH_Q;
/// The reflections after the first are this many dB louder than its level and the fade give.
const TAP_LATER_DB: f64 = 2.0;
/// A loss of the highs built into the loop: a high shelf half way at this frequency, this many dB
/// per trip up to [`AIR_DECAY`]; longer decays lose less, with the cube root of the decay.
const AIR_FREQUENCY: f64 = 8_000.0;
/// The fixed loss of the highs per trip, in dB, for decays up to [`AIR_DECAY`].
const AIR_LOSS_DB: f64 = 0.96;
/// Decay time (seconds) above which the fixed loss of the highs shrinks.
const AIR_DECAY: f64 = 3.46;
/// Decay time the network is tuned to, against the Decay setting.
const DECAY_RATIO: f64 = 0.9;
/// Low Cut: a first-order shelf this deep (linear), half way at the band's lower edge.
const LOW_CUT_DEPTH: f64 = 0.2;
/// The input always passes a first-order high-pass at this frequency: the reverb, reflections and
/// tail alike, holds 3 to 8 dB less at 63 Hz and 2 to 3 dB less at 125 Hz than a flat input gives,
/// whatever Low Cut does (docs/spec/reverb.md).
const RUMBLE_FREQUENCY: f64 = 90.0;
/// High Cut is left out above this frequency.
const HIGH_CUT_LIMIT: f64 = 20_000.0;
/// Inside a frozen loop that still takes input, levels above this are compressed so the tail
/// cannot grow without end.
const FREEZE_LIMIT: f64 = 2.0;

/// Longest delay the predelay line needs, with some room.
const PREDELAY_LINE_SECONDS: f64 = 0.6;
/// Longest delay a network line needs, with some room.
const LINE_SECONDS: f64 = 0.3;
/// Longest delay a diffuser needs, with some room.
const DIFFUSER_SECONDS: f64 = 0.035;

/// Parameter indices, in [`INFO`] order.
pub mod param {
    /// Time from the input to the first reflection, in seconds.
    pub const PREDELAY: usize = 0;
    /// Low Cut on the input band.
    pub const LOW_CUT: usize = 1;
    /// High Cut on the input band.
    pub const HIGH_CUT: usize = 2;
    /// Centre of the input band in hertz.
    pub const INPUT_FREQUENCY: usize = 3;
    /// Width of the input band in octaves.
    pub const INPUT_WIDTH: usize = 4;
    /// Spin, which swings the early reflections' delays, on or off.
    pub const SPIN: usize = 5;
    /// How fast Spin swings, in hertz.
    pub const SPIN_RATE: usize = 6;
    /// How far Spin swings.
    pub const SPIN_AMOUNT: usize = 7;
    /// How the reflections fade and when the tail sets in, 0..1.
    pub const SHAPE: usize = 8;
    /// High damping in the loop on or off.
    pub const HIGH_DAMPING: usize = 9;
    /// High damping as a shelf (0) or a low-pass (1).
    pub const HIGH_DAMPING_TYPE: usize = 10;
    /// High damping's frequency in hertz.
    pub const HIGH_DAMPING_FREQUENCY: usize = 11;
    /// High damping's gain per second, linear.
    pub const HIGH_DAMPING_GAIN: usize = 12;
    /// Low damping in the loop on or off.
    pub const LOW_DAMPING: usize = 13;
    /// Low damping's frequency in hertz.
    pub const LOW_DAMPING_FREQUENCY: usize = 14;
    /// Low damping's gain per second, linear.
    pub const LOW_DAMPING_GAIN: usize = 15;
    /// The diffusers' allpass gain.
    pub const DIFFUSION: usize = 16;
    /// The diffusers' delays as a fraction of their longest.
    pub const SCALE: usize = 17;
    /// Chorus, which swings the network's line lengths, on or off.
    pub const CHORUS: usize = 18;
    /// How fast Chorus swings, in hertz.
    pub const CHORUS_RATE: usize = 19;
    /// How far Chorus swings.
    pub const CHORUS_AMOUNT: usize = 20;
    /// The room's size; 100 is the reference room.
    pub const SIZE: usize = 21;
    /// How Size glides to a new value: None, Slow or Fast.
    pub const SMOOTH: usize = 22;
    /// Decay time of the tail, in seconds.
    pub const DECAY: usize = 23;
    /// Holds the tail without decay.
    pub const FREEZE: usize = 24;
    /// While frozen, leaves the damping out.
    pub const FLAT: usize = 25;
    /// While frozen, takes no new input.
    pub const CUT: usize = 26;
    /// Width of the wet signal, 0 (mono) to 120.
    pub const STEREO: usize = 27;
    /// How many lines the network runs: Sparse, Low, Mid or High.
    pub const DENSITY: usize = 28;
    /// Level of the early reflections in decibels.
    pub const REFLECT: usize = 29;
    /// Level of the late reverb in decibels.
    pub const DIFFUSE: usize = 30;
    /// Share of the wet signal in the output.
    pub const MIX: usize = 31;
    /// How many parameters there are.
    pub(crate) const COUNT: usize = 32;
}

/// Lowest Reflect and Diffuse level: a linear gain of 0.03.
pub const MIN_LEVEL_DB: f64 = -30.4576;

/// The parameters, in [`param`] order.
const PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::continuous("predelay", "Predelay", (0.000_5, 0.25), 0.002_5, Unit::Seconds, Scale::Logarithmic),
    ParamInfo::toggle("lowCut", "Low Cut", false),
    ParamInfo::toggle("highCut", "High Cut", true),
    ParamInfo::continuous(
        "inputFrequency",
        "Input Frequency",
        (50.0, 18_000.0),
        830.0,
        Unit::Hertz,
        Scale::Logarithmic,
    ),
    ParamInfo::continuous("inputWidth", "Input Width", (0.5, 9.0), 5.85, Unit::Octaves, Scale::Linear),
    ParamInfo::toggle("spin", "Spin", true),
    ParamInfo::continuous("spinRate", "Spin Rate", (0.07, 1.3), 0.3, Unit::Hertz, Scale::Logarithmic),
    ParamInfo::continuous("spinAmount", "Spin Amount", (2.0, 55.0), 17.5, Unit::None, Scale::Logarithmic),
    ParamInfo::continuous("shape", "Shape", (0.0, 1.0), 0.5, Unit::None, Scale::Linear),
    ParamInfo::toggle("highDamping", "High Damping", true),
    ParamInfo::choice("highDampingType", "High Damping Type", &["Shelf", "Low-Pass"], 0),
    ParamInfo::continuous(
        "highDampingFrequency",
        "High Damping Frequency",
        (20.0, 16_000.0),
        4_500.0,
        Unit::Hertz,
        Scale::Logarithmic,
    ),
    ParamInfo::continuous("highDampingGain", "High Damping Gain", (0.2, 1.0), 0.7, Unit::None, Scale::Linear),
    ParamInfo::toggle("lowDamping", "Low Damping", false),
    ParamInfo::continuous(
        "lowDampingFrequency",
        "Low Damping Frequency",
        (20.0, 15_000.0),
        90.0,
        Unit::Hertz,
        Scale::Logarithmic,
    ),
    ParamInfo::continuous("lowDampingGain", "Low Damping Gain", (0.2, 1.0), 0.75, Unit::None, Scale::Linear),
    ParamInfo::continuous("diffusion", "Diffusion", (0.001, 0.96), 0.6, Unit::Percent, Scale::Linear),
    ParamInfo::continuous("scale", "Scale", (0.05, 1.0), 0.4, Unit::Percent, Scale::Linear),
    ParamInfo::toggle("chorus", "Chorus", true),
    ParamInfo::continuous("chorusRate", "Chorus Rate", (0.01, 8.0), 0.02, Unit::Hertz, Scale::Logarithmic),
    ParamInfo::continuous("chorusAmount", "Chorus Amount", (0.01, 4.0), 0.02, Unit::None, Scale::Linear),
    ParamInfo::continuous("size", "Size", (0.22, 500.0), 100.0, Unit::None, Scale::Logarithmic),
    ParamInfo::choice("smooth", "Smooth", &["None", "Slow", "Fast"], 2),
    ParamInfo::continuous("decay", "Decay", (0.2, 60.0), 1.2, Unit::Seconds, Scale::Logarithmic),
    ParamInfo::toggle("freeze", "Freeze", false),
    ParamInfo::toggle("flat", "Flat", true),
    ParamInfo::toggle("cut", "Cut", true),
    ParamInfo::continuous("stereo", "Stereo", (0.0, 120.0), 100.0, Unit::None, Scale::Linear),
    ParamInfo::choice("density", "Density", &["Sparse", "Low", "Mid", "High"], 1),
    ParamInfo::continuous("reflect", "Reflect", (MIN_LEVEL_DB, 6.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("diffuse", "Diffuse", (MIN_LEVEL_DB, 6.0), 0.0, Unit::Decibels, Scale::Linear),
    ParamInfo::continuous("mix", "Dry/Wet", (0.0, 1.0), 0.55, Unit::Percent, Scale::Linear),
];

/// What the reverb is and the parameters it has.
pub const INFO: DeviceInfo = DeviceInfo { id: "reverb", name: "Reverb", role: Role::Effect, params: &PARAMS };

/// Sample rate assumed until [`Device::prepare`] gives one.
const DEFAULT_SAMPLE_RATE: f64 = 48_000.0;

/// What the control-rate update works out for the taps and lines.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Tuning {
    /// Each tap's delay after the predelay (frames).
    tap_delays: [f64; 32],
    /// Each tap's gain (linear).
    tap_gains: [f64; 32],
    /// Delay from the predelay to the diffusers (frames).
    shape_delay: f64,
    /// The diffusers' lengths (frames).
    diffusers: [f64; 4],
    /// The lines' lengths (frames).
    lines: [f64; 16],
    /// Each line's gain per trip.
    gains: [f64; 16],
    /// Gain of the input into the network.
    feed: f64,
}

impl Default for Tuning {
    /// Silent taps and lines at the shortest lengths, until the first tuning.
    fn default() -> Self {
        Self {
            tap_delays: [2.0; 32],
            tap_gains: [0.0; 32],
            shape_delay: 2.0,
            diffusers: [2.0; 4],
            lines: [2.0; 16],
            gains: [0.0; 16],
            feed: 0.0,
        }
    }
}

impl Tuning {
    /// The values `t` (0..1) of the way from `from` to `to`.
    fn between(&mut self, from: &Self, to: &Self, t: f64) {
        fn mix<const N: usize>(out: &mut [f64; N], a: &[f64; N], b: &[f64; N], t: f64) {
            for ((o, a), b) in out.iter_mut().zip(a).zip(b) {
                *o = a + (b - a) * t;
            }
        }
        mix(&mut self.tap_delays, &from.tap_delays, &to.tap_delays, t);
        mix(&mut self.tap_gains, &from.tap_gains, &to.tap_gains, t);
        mix(&mut self.diffusers, &from.diffusers, &to.diffusers, t);
        mix(&mut self.lines, &from.lines, &to.lines, t);
        mix(&mut self.gains, &from.gains, &to.gains, t);
        self.shape_delay = from.shape_delay + (to.shape_delay - from.shape_delay) * t;
        self.feed = from.feed + (to.feed - from.feed) * t;
    }
}

/// One line of the network and its damping: the high shelf and the low-pass (High damping's two
/// types), the low shelf and the fixed loss of the highs. Every filter always runs and the
/// switches crossfade, so turning one on, off or to the other type does not click.
#[derive(Debug, Clone, Default, PartialEq)]
struct Line {
    /// The line's delay.
    delay: DelayLine,
    /// High damping as a shelf.
    high_shelf: Biquad,
    /// High damping as a low-pass.
    low_pass: FirstOrder,
    /// Low damping.
    low_shelf: Biquad,
    /// The fixed loss of the highs.
    air: FirstOrder,
}

/// A filter section the damping runs.
trait Section {
    /// One sample through the section.
    fn run(&mut self, x: f64) -> f64;
    /// Silences the section.
    fn clear(&mut self);
}

impl Section for FirstOrder {
    fn run(&mut self, x: f64) -> f64 {
        self.process(x)
    }

    fn clear(&mut self) {
        self.reset();
    }
}

impl Section for Biquad {
    fn run(&mut self, x: f64) -> f64 {
        self.process(x)
    }

    fn clear(&mut self) {
        self.reset();
    }
}

impl Line {
    /// Silences the line and its damping.
    fn clear(&mut self) {
        self.delay.clear();
        self.high_shelf.reset();
        self.low_pass.reset();
        self.low_shelf.reset();
        self.air.reset();
    }

    /// `y` through the damping, each filter heard in proportion to its amount. A filter that is
    /// not heard does not run, and starts again from silence (faded in by its amount).
    #[inline]
    fn damp(&mut self, y: f64, amounts: &Damping) -> f64 {
        fn heard(filter: &mut impl Section, x: f64, amount: f64) -> f64 {
            if amount > 0.0 {
                filter.run(x)
            } else {
                filter.clear();
                x
            }
        }
        let y = if amounts.high > 0.0 {
            let shelf = heard(&mut self.high_shelf, y, 1.0 - amounts.low_pass);
            let pass = heard(&mut self.low_pass, y, amounts.low_pass);
            let high = shelf + (pass - shelf) * amounts.low_pass;
            y + (high - y) * amounts.high
        } else {
            self.high_shelf.reset();
            self.low_pass.reset();
            y
        };
        let y = y + (heard(&mut self.low_shelf, y, amounts.low) - y) * amounts.low;
        y + (heard(&mut self.air, y, amounts.air) - y) * amounts.air
    }
}

/// How much of each damping filter is heard, 0..1.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Damping {
    /// High damping.
    high: f64,
    /// The low-pass's share of high damping (the shelf has the rest).
    low_pass: f64,
    /// Low damping.
    low: f64,
    /// The fixed loss of the highs.
    air: f64,
}

/// Declares [`Ramps`] from one list of fields, so the smoothers and their targets are paired by
/// name and cannot fall out of step.
macro_rules! ramps {
    ($($(#[$doc:meta])* $field:ident,)*) => {
        /// Values that ramp to their settings, so moving a control does not click:
        /// `Ramps<Smoother>` holds the smoothers and `Ramps<f64>` their targets.
        #[derive(Debug, Clone, Copy, PartialEq)]
        struct Ramps<T> {
            $($(#[$doc])* $field: T,)*
        }

        impl<T: Copy> Ramps<T> {
            /// Every field set to `value`.
            fn splat(value: T) -> Self {
                Self { $($field: value,)* }
            }
        }

        impl Ramps<Smoother> {
            /// Calls `f` with each smoother and its target.
            fn each(&mut self, targets: &Ramps<f64>, mut f: impl FnMut(&mut Smoother, f32)) {
                $(f(&mut self.$field, targets.$field as f32);)*
            }
        }
    };
}

ramps! {
    /// Predelay (frames).
    predelay,
    /// Reflect (linear).
    reflect,
    /// Diffuse (linear).
    diffuse,
    /// Dry/Wet.
    mix,
    /// Stereo's mixing angle (radians).
    angle,
    /// Gain of the input into the network (0 while frozen with Cut on).
    feed,
    /// How much of Low Cut is heard (0 off, 1 on).
    low_cut,
    /// How much of High Cut is heard (0 off, 1 on).
    high_cut,
    /// Spin's swing (frames).
    spin,
    /// Chorus's swing (frames).
    chorus,
    /// Shape, read at the control rate.
    shape,
    /// Scale, read at the control rate.
    scale,
    /// The natural log of Decay, read at the control rate.
    decay,
    /// log2 of the input band's centre, read at the control rate.
    centre,
    /// The input band's width in octaves, read at the control rate.
    width,
    /// The diffusers' gain.
    diffusion,
    /// log2 of high damping's frequency, read at the control rate.
    high_frequency,
    /// High damping's gain, read at the control rate.
    high_gain,
    /// log2 of low damping's frequency, read at the control rate.
    low_frequency,
    /// Low damping's gain, read at the control rate.
    low_gain,
    /// How much high damping is heard (see [`Damping`]).
    high,
    /// The low-pass's share of high damping (see [`Damping`]).
    low_pass,
    /// How much low damping is heard (see [`Damping`]).
    low,
    /// How much of the fixed loss of the highs is heard (see [`Damping`]).
    air,
}

impl Ramps<Smoother> {
    /// Whether the values read at the control rate are still moving.
    fn control_moving(&self) -> bool {
        [
            &self.shape,
            &self.scale,
            &self.decay,
            &self.high_frequency,
            &self.high_gain,
            &self.low_frequency,
            &self.low_gain,
        ]
        .iter()
        .any(|s| !s.is_settled())
    }

    /// Whether the input band's centre or width is still moving.
    fn band_moving(&self) -> bool {
        !self.centre.is_settled() || !self.width.is_settled()
    }
}

/// The algorithmic room reverb: input filters, predelay, early reflections and a feedback delay
/// network.
pub struct Reverb {
    /// The sample rate it was prepared for.
    sample_rate: f64,
    /// Every parameter's value, in [`param`] order.
    values: [f64; param::COUNT],
    /// Values that ramp to their settings.
    ramps: Ramps<Smoother>,
    /// Low Cut, a first-order low shelf.
    low_cut: FirstOrder,
    /// The fixed roll-off of the lowest octaves at the input.
    rumble: FirstOrder,
    /// High Cut, a second-order low-pass.
    high_cut: Biquad,
    /// The predelay line, which the taps and the Shape delay read.
    input: DelayLine,
    /// The allpasses in front of the network.
    diffusers: [Allpass; 4],
    /// The network's lines; the first [`active`](Self::active) run.
    lines: [Line; 16],
    /// Lines running (Density).
    active: usize,
    /// Each line's share of the input.
    weights: [f64; 16],
    /// Cosine and sine of each tap's phase offset.
    spin_offsets: [(f64, f64); 32],
    /// Cosine and sine of each line's phase offset.
    chorus_offsets: [(f64, f64); 16],
    /// The size factor `(size / 100)^(1/3)` as it glides.
    room: f64,
    /// The tuning at the last control point.
    from: Tuning,
    /// The tuning at the next control point.
    to: Tuning,
    /// The tuning in between, for this frame.
    now: Tuning,
    /// Whether `from` and `to` differ, so frames in between need `now`.
    moving: bool,
    /// Spin's LFO position, in cycles (0..1).
    spin_phase: f64,
    /// Chorus's LFO position, in cycles (0..1).
    chorus_phase: f64,
    /// Frames since the last reset, for the control-rate grid.
    frame: u64,
    /// Whether a parameter changed since the last control update.
    dirty: bool,
}

impl Default for Reverb {
    /// The same as [`Reverb::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// A level in decibels as a linear gain.
fn db_to_gain(db: f64) -> f64 {
    10.0f64.powf(db / 20.0)
}

/// Cosine and sine of golden-angle phase offsets, from the `first`th on, so no two taps or lines
/// swing together.
fn phase_offsets<const N: usize>(first: usize) -> [(f64, f64); N] {
    std::array::from_fn(|k| {
        let p = std::f64::consts::TAU * ((k + first) as f64 * 0.381_966_011_250_105).fract();
        (p.cos(), p.sin())
    })
}

/// Each line's share of the network's input with `active` lines running: the longer lines take
/// more, so the tail builds up rather than starting at its loudest.
fn input_weights(active: usize) -> [f64; 16] {
    let norm = LINES.iter().take(active).map(|l| l * l).sum::<f64>().sqrt().max(1e-9);
    std::array::from_fn(|k| LINES.get(k).map_or(0.0, |l| l / norm) * INPUT_SIGNS.get(k).copied().unwrap_or(1.0))
}

/// Mixes the first `n` lines' outputs with an orthogonal matrix in which every line feeds every
/// other one equally: a Hadamard matrix for 4, 8 and 16 lines, and for 12 a Hadamard matrix of 4
/// within each group of four followed by a Householder reflection across the three groups.
#[inline]
fn mix_lines(v: &mut [f64; 16], n: usize) {
    /// The normalised fast Walsh-Hadamard transform; `v`'s length is a power of two.
    fn hadamard(v: &mut [f64]) {
        let mut h = 1;
        while h < v.len() {
            for block in v.chunks_exact_mut(2 * h) {
                let (a, b) = block.split_at_mut(h);
                for (x, y) in a.iter_mut().zip(b) {
                    (*x, *y) = (*x + *y, *x - *y);
                }
            }
            h *= 2;
        }
        let scale = 1.0 / (v.len() as f64).sqrt();
        for x in v.iter_mut() {
            *x *= scale;
        }
    }
    if n == 12 {
        for group in v.chunks_mut(4).take(3) {
            hadamard(group);
        }
        let (first, rest) = v.split_at_mut(4);
        let (second, third) = rest.split_at_mut(4);
        for ((a, b), c) in first.iter_mut().zip(second.iter_mut()).zip(third.iter_mut()) {
            let sum = (*a + *b + *c) * 2.0 / 3.0;
            *a -= sum;
            *b -= sum;
            *c -= sum;
        }
    } else if let Some(lines) = v.get_mut(..n) {
        hadamard(lines);
    }
    // Each line passes its share on to the next one: a Hadamard matrix is its own inverse, and
    // without the shift two trips round the loop would undo each other's mixing.
    if let Some(lines) = v.get_mut(..n) {
        lines.rotate_left(1);
    }
}

/// Soft limit for a frozen loop: unchanged up to [`FREEZE_LIMIT`], then compressed towards twice it.
#[inline]
fn limit(x: f64) -> f64 {
    let over = x.abs() - FREEZE_LIMIT;
    if over <= 0.0 { x } else { x.signum() * (FREEZE_LIMIT + over / (1.0 + over / FREEZE_LIMIT)) }
}

impl Reverb {
    /// A reverb with every parameter at its default, prepared for the default sample rate
    /// (allocates).
    pub fn new() -> Self {
        let sr = DEFAULT_SAMPLE_RATE;
        let mut reverb = Self {
            sample_rate: sr,
            values: PARAMS.map(|p| p.default),
            ramps: Ramps::splat(Smoother::new(0.0, 1)),
            low_cut: FirstOrder::default(),
            rumble: FirstOrder::default(),
            high_cut: Biquad::default(),
            input: DelayLine::default(),
            diffusers: Default::default(),
            lines: Default::default(),
            active: 8,
            weights: input_weights(8),
            spin_offsets: phase_offsets(0),
            chorus_offsets: phase_offsets(7),
            room: 1.0,
            from: Tuning::default(),
            to: Tuning::default(),
            now: Tuning::default(),
            moving: false,
            spin_phase: 0.0,
            chorus_phase: 0.0,
            frame: 0,
            dirty: true,
        };
        reverb.allocate();
        reverb.rebuild();
        reverb
    }

    /// Allocates the lines for the current sample rate.
    fn allocate(&mut self) {
        let sr = self.sample_rate;
        self.input = DelayLine::for_seconds(PREDELAY_LINE_SECONDS, sr);
        self.diffusers = std::array::from_fn(|_| Allpass::for_seconds(DIFFUSER_SECONDS, sr));
        self.lines =
            std::array::from_fn(|_| Line { delay: DelayLine::for_seconds(LINE_SECONDS, sr), ..Line::default() });
    }

    /// The value of parameter `index`, 0.0 for an unknown one.
    fn value(&self, index: usize) -> f64 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    /// Whether switch `index` is on.
    fn on(&self, index: usize) -> bool {
        self.value(index) >= 0.5
    }

    /// Whether Freeze is on.
    fn frozen(&self) -> bool {
        self.on(param::FREEZE)
    }

    /// Where each ramped value is heading, from the parameters.
    fn targets(&self) -> Ramps<f64> {
        let sr = self.sample_rate;
        let flag = |index| if self.on(index) { 1.0 } else { 0.0 };
        // Frozen with Flat on, the damping is left out; frozen at all, the fixed loss is.
        let filtered = if self.frozen() && self.on(param::FLAT) { 0.0 } else { 1.0 };
        Ramps {
            predelay: self.value(param::PREDELAY) * sr,
            reflect: db_to_gain(self.value(param::REFLECT)),
            diffuse: db_to_gain(self.value(param::DIFFUSE)),
            mix: self.value(param::MIX),
            angle: std::f64::consts::FRAC_PI_4 * (1.0 - self.value(param::STEREO) / 120.0),
            feed: if self.frozen() && self.on(param::CUT) { 0.0 } else { 1.0 },
            low_cut: flag(param::LOW_CUT),
            high_cut: flag(param::HIGH_CUT),
            spin: flag(param::SPIN) * SPIN_SECONDS * self.value(param::SPIN_AMOUNT) * sr,
            chorus: flag(param::CHORUS) * CHORUS_SECONDS * self.value(param::CHORUS_AMOUNT) * sr,
            shape: self.value(param::SHAPE),
            scale: self.value(param::SCALE),
            decay: self.value(param::DECAY).max(0.01).ln(),
            centre: self.value(param::INPUT_FREQUENCY).max(1.0).log2(),
            width: self.value(param::INPUT_WIDTH),
            diffusion: self.value(param::DIFFUSION),
            high_frequency: self.value(param::HIGH_DAMPING_FREQUENCY).max(1.0).log2(),
            high_gain: self.value(param::HIGH_DAMPING_GAIN),
            low_frequency: self.value(param::LOW_DAMPING_FREQUENCY).max(1.0).log2(),
            low_gain: self.value(param::LOW_DAMPING_GAIN),
            high: flag(param::HIGH_DAMPING) * filtered,
            low_pass: flag(param::HIGH_DAMPING_TYPE),
            low: flag(param::LOW_DAMPING) * filtered,
            air: if self.frozen() { 0.0 } else { 1.0 },
        }
    }

    /// The size factor `(size / 100)^(1/3)` for the Size setting.
    fn room_target(&self) -> f64 {
        (self.value(param::SIZE) / REFERENCE_SIZE).cbrt()
    }

    /// Every state cleared, every ramped value at its setting.
    fn rebuild(&mut self) {
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, self.sample_rate);
        let targets = self.targets();
        self.ramps.each(&targets, |smoother, target| {
            smoother.set_ramp(ramp);
            smoother.reset(target);
        });
        self.ramps.predelay.set_ramp(Smoother::ramp_frames(PREDELAY_RAMP_SECONDS, self.sample_rate));
        self.low_cut.reset();
        self.rumble = FirstOrder::new(FirstOrderCoefficients::highpass(self.sample_rate, RUMBLE_FREQUENCY));
        self.high_cut.reset();
        self.input.clear();
        for diffuser in &mut self.diffusers {
            diffuser.clear();
        }
        for line in &mut self.lines {
            line.clear();
        }
        self.set_active(self.density_lines());
        self.room = self.room_target();
        self.spin_phase = 0.0;
        self.chorus_phase = 0.0;
        self.frame = 0;
        self.tune_input();
        self.tune();
        self.from = self.to;
        self.now = self.to;
        self.moving = false;
        self.dirty = false;
    }

    /// Runs the first `lines` lines of the network.
    fn set_active(&mut self, lines: usize) {
        self.active = lines.clamp(1, LINES.len());
        self.weights = input_weights(self.active);
    }

    /// How many lines the Density setting runs.
    fn density_lines(&self) -> usize {
        DENSITY_LINES.get(self.value(param::DENSITY) as usize).copied().unwrap_or(8)
    }

    /// The input filters from the band's (ramped) centre and width.
    fn tune_input(&mut self) {
        let sr = self.sample_rate;
        let centre = 2.0f64.powf(f64::from(self.ramps.centre.current()));
        let half = f64::from(self.ramps.width.current()) / 2.0;
        let (low, high) = (centre / 2.0f64.powf(half), centre * 2.0f64.powf(half));
        self.low_cut.set(FirstOrderCoefficients::low_shelf(sr, low, LOW_CUT_DEPTH));
        self.high_cut.set(if high < HIGH_CUT_LIMIT.min(sr * 0.45) {
            Coefficients::lowpass(sr, high, BUTTERWORTH_Q)
        } else {
            Coefficients::IDENTITY
        });
    }

    /// Delay lengths, tap levels, line gains and damping for the current size and settings, into
    /// `to`.
    fn tune(&mut self) {
        let sr = self.sample_rate;
        let room = self.room;
        let ms = sr / 1_000.0;
        let shape = f64::from(self.ramps.shape.current());
        let scale = f64::from(self.ramps.scale.current());
        let setting = f64::from(self.ramps.decay.current()).exp();
        let fade = (TAP_FADE + TAP_FADE_PER_SHAPE * shape) * TAP_FADE_DECAY / setting;
        let predelay_ms = self.value(param::PREDELAY) * 1_000.0;
        let frozen = self.frozen();
        let decay = setting * DECAY_RATIO;
        // Once nothing moves, the lines are whole frames long, so a line without chorus is read
        // without interpolation, which would take away highs on every trip. While Size or Decay
        // glide they move smoothly between frames.
        let settled = self.room == self.room_target() && !self.ramps.control_moving();
        let density = DENSITY_TRIM_DB.get(self.value(param::DENSITY) as usize).copied().unwrap_or(0.0);
        let t = &mut self.to;
        for (k, ((delay, gain), &(time, _, level))) in
            t.tap_delays.iter_mut().zip(&mut t.tap_gains).zip(&TAPS).enumerate()
        {
            let time = time * room;
            *delay = time * ms;
            let later = if k > 0 { TAP_LATER_DB } else { 0.0 };
            *gain = TAP_GAIN * db_to_gain(level + later - TAP_PREDELAY_FADE * predelay_ms - fade * time);
        }
        let (a, b, c) = SHAPE_DELAY_MS;
        t.shape_delay = ((a + (b + c * (shape - 0.5)) * room) * ms).max(0.0);
        for (length, base) in t.diffusers.iter_mut().zip(DIFFUSERS) {
            *length = (base * scale * room * ms).max(2.0);
        }
        for ((length, gain), base) in t.lines.iter_mut().zip(t.gains.iter_mut()).zip(LINES) {
            let frames = base * room * ms;
            *length = if settled { frames.round() } else { frames }.max(2.0);
            *gain = if frozen { 1.0 } else { db_to_gain(-60.0 * base * room / 1_000.0 / decay) };
        }
        t.feed =
            TAIL_GAIN * db_to_gain(density - 60.0 * TAIL_FADE_SECONDS / setting) * room.max(1.0).powf(LARGE_ROOM_POWER);

        let air_db = AIR_LOSS_DB * (AIR_DECAY / setting).cbrt().min(1.0);
        let r = &self.ramps;
        let (high_f, high_g) = (2.0f64.powf(f64::from(r.high_frequency.current())), f64::from(r.high_gain.current()));
        let (low_f, low_g) = (2.0f64.powf(f64::from(r.low_frequency.current())), f64::from(r.low_gain.current()));
        for (line, base) in self.lines.iter_mut().zip(LINES) {
            // The loss is per trip round the reference loop; a line of another length, or in a
            // room of another size, loses in proportion to its length, so the loss per second
            // does not depend on the room.
            let trips = base * room / 1_000.0 / LOSS_TRIP_SECONDS;
            let db = |g: f64| 20.0 * g.max(1e-6).log10() * trips;
            line.high_shelf.set(Coefficients::high_shelf(sr, high_f, SHELF_Q, db(high_g)));
            line.low_pass.set(FirstOrderCoefficients::lowpass(sr, high_f / trips.sqrt()));
            line.low_shelf.set(Coefficients::low_shelf(sr, low_f, SHELF_Q, db(low_g)));
            line.air.set(FirstOrderCoefficients::high_shelf(sr, AIR_FREQUENCY, db_to_gain(-air_db * trips)));
        }
    }

    /// Size moves toward its setting, as fast as Smooth says, over one control period.
    fn glide_room(&mut self) {
        let target = self.room_target();
        let seconds = match self.value(param::SMOOTH) as usize {
            1 => SMOOTH_SLOW_SECONDS,
            2 => SMOOTH_FAST_SECONDS,
            _ => 0.0,
        };
        let frames = seconds * self.sample_rate;
        let keep = if frames > 1.0 { (-(CONTROL_FRAMES as f64) / frames).exp() } else { 0.0 };
        let room = target + (self.room - target) * keep;
        let room = if (room - target).abs() < 1e-6 { target } else { room };
        if room != self.room {
            self.room = room;
            self.dirty = true;
        }
    }

    /// At a control point: what the next stretch of frames glides to.
    fn control(&mut self) {
        self.from = self.to;
        self.glide_room();
        if self.ramps.band_moving() {
            self.tune_input();
        }
        let moving = self.ramps.control_moving();
        if self.dirty || moving {
            self.tune();
            // One more update once the ramps stop, so the lines settle on whole frames.
            self.dirty = moving;
        }
        self.moving = self.from != self.to;
    }
}

impl Device for Reverb {
    /// The reverb's [`INFO`].
    fn info(&self) -> &'static DeviceInfo {
        &INFO
    }

    /// Allocates the lines for `sample_rate` and starts silent.
    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        self.sample_rate = check_sample_rate(sample_rate)?;
        self.allocate();
        self.rebuild();
        Ok(())
    }

    /// Silences the reverb and settles every ramped value at its setting.
    fn reset(&mut self) {
        self.rebuild();
    }

    /// Sets parameter `index`, clamped to its range; the ramped values move to it.
    fn set_param(&mut self, index: usize, value: f64) {
        let Some(info) = PARAMS.get(index) else { return };
        let Some(slot) = self.values.get_mut(index) else { return };
        *slot = info.clamp(value);
        let targets = self.targets();
        self.ramps.each(&targets, |smoother, target| smoother.set_target(target));
        self.dirty = true;
    }

    /// The value of parameter `index`, or `None` for an unknown one.
    fn param(&self, index: usize) -> Option<f64> {
        self.values.get(index).copied()
    }

    /// Runs interleaved stereo `buffer` through the reverb in place.
    fn process(&mut self, _context: &ProcessContext, buffer: &mut [f32]) {
        let sr = self.sample_rate;
        let wanted = self.density_lines();
        if wanted != self.active {
            // Lines that start again begin silent.
            for line in self.lines.iter_mut().skip(self.active).take(wanted.saturating_sub(self.active)) {
                line.clear();
            }
            self.set_active(wanted);
        }
        let active = self.active;
        let norm = 1.0 / (active as f64).sqrt();
        let (spin_step, chorus_step) = (self.value(param::SPIN_RATE) / sr, self.value(param::CHORUS_RATE) / sr);
        let frozen = self.frozen();
        // Stereo's rotation and the mix's gains, worked out again only when their inputs move
        // (NaN never equals an input, so the first frame works them out).
        let mut output_key = (f64::NAN, f64::NAN);
        let (mut rotation, mut gains) = ((0.0, 0.0), (0.0, 0.0));

        for frame in buffer.as_chunks_mut::<2>().0 {
            let step = self.frame % CONTROL_FRAMES;
            if step == 0 {
                self.control();
            }
            if self.moving {
                self.now.between(&self.from, &self.to, (step + 1) as f64 / CONTROL_FRAMES as f64);
            }
            let r = &mut self.ramps;
            let [predelay, reflect, diffuse, mix, angle, feed, low_cut, high_cut, spin_swing, chorus_swing] = [
                &mut r.predelay,
                &mut r.reflect,
                &mut r.diffuse,
                &mut r.mix,
                &mut r.angle,
                &mut r.feed,
                &mut r.low_cut,
                &mut r.high_cut,
                &mut r.spin,
                &mut r.chorus,
            ]
            .map(|s| f64::from(s.next_value()));
            for s in [
                &mut r.shape,
                &mut r.scale,
                &mut r.decay,
                &mut r.centre,
                &mut r.width,
                &mut r.high_frequency,
                &mut r.high_gain,
                &mut r.low_frequency,
                &mut r.low_gain,
            ] {
                s.next_value();
            }
            let diffusion = r.diffusion.next_value();
            let damping = Damping {
                high: f64::from(r.high.next_value()),
                low_pass: f64::from(r.low_pass.next_value()),
                low: f64::from(r.low.next_value()),
                air: f64::from(r.air.next_value()),
            };

            let input = frame.map(|s| if s.is_finite() { f64::from(s) } else { 0.0 });
            // Both cuts always run, so switching one crossfades instead of clicking.
            let mut x = self.rumble.process((input[0] + input[1]) / 2.0);
            x += (self.low_cut.process(x) - x) * low_cut;
            x += (self.high_cut.process(x) - x) * high_cut;
            let t = if self.moving { &self.now } else { &self.to };

            // Early reflections.
            let mut early = [0.0f64; 2];
            if spin_swing > 0.0 {
                let (spin_sin, spin_cos) = (std::f64::consts::TAU * self.spin_phase).sin_cos();
                for (((&delay, &gain), &(c, s)), &side) in
                    t.tap_delays.iter().zip(&t.tap_gains).zip(&self.spin_offsets).zip(&TAP_SIDES)
                {
                    // Each tap swings either way round its place; the earliest ones by less, so
                    // they never read ahead of the input.
                    let at = predelay + delay;
                    let swing = spin_swing.min(0.9 * at) * (spin_sin * c + spin_cos * s);
                    let v = f64::from(self.input.read_hermite(at + swing)) * gain;
                    early[0] += v * (1.0 - side);
                    early[1] += v * side;
                }
            } else {
                // Without Spin, and with nothing moving, the taps fall on whole frames.
                let whole = !self.moving && self.ramps.predelay.is_settled();
                for ((&delay, &gain), &side) in t.tap_delays.iter().zip(&t.tap_gains).zip(&TAP_SIDES) {
                    let at = predelay + delay;
                    let v = f64::from(if whole {
                        self.input.read(at.round() as usize)
                    } else {
                        self.input.read_hermite(at)
                    }) * gain;
                    early[0] += v * (1.0 - side);
                    early[1] += v * side;
                }
            }

            // The network's input: after the Shape delay and the diffusers.
            let mut u = self.input.read_hermite(predelay + t.shape_delay) * feed as f32;
            for (diffuser, &length) in self.diffusers.iter_mut().zip(&t.diffusers) {
                u = diffuser.process(u, length, diffusion);
            }
            let u = f64::from(u) * t.feed;

            // The network. Without Chorus the swing is zero whatever the phase.
            let (chorus_sin, chorus_cos) =
                if chorus_swing > 0.0 { (std::f64::consts::TAU * self.chorus_phase).sin_cos() } else { (0.0, 0.0) };
            let mut damped = [0.0f64; 16];
            for ((((line, slot), &length), &gain), &(c, s)) in self
                .lines
                .iter_mut()
                .zip(&mut damped)
                .zip(&t.lines)
                .zip(&t.gains)
                .zip(&self.chorus_offsets)
                .take(active)
            {
                let swing = chorus_swing.min(0.5 * length) * (chorus_sin * c + chorus_cos * s);
                let y = f64::from(line.delay.read_hermite(length + swing));
                *slot = line.damp(y, &damping) * gain;
            }
            // The tail is heard after each line's gain and damping, so even a line's first arrival
            // has decayed for as long as the line is.
            let outs = damped;
            mix_lines(&mut damped, active);
            let mut tail = [0.0f64; 2];
            for (k, (((line, &y), &d), &weight)) in
                self.lines.iter_mut().zip(&outs).zip(&damped).zip(&self.weights).take(active).enumerate()
            {
                let mut next = d + u * weight;
                if frozen {
                    next = limit(next);
                }
                line.delay.push(next as f32);
                for (out, signs) in tail.iter_mut().zip(&OUTPUT_SIGNS) {
                    *out += signs.get(k).copied().unwrap_or(0.0) * y;
                }
            }
            self.input.push(x as f32);

            // Output: the two parts, Stereo's rotation, then the mix.
            let wet = [reflect * early[0] + diffuse * norm * tail[0], reflect * early[1] + diffuse * norm * tail[1]];
            if (angle, mix) != output_key {
                output_key = (angle, mix);
                rotation = angle.sin_cos();
                let (wet_gain, dry_gain) = (mix * std::f64::consts::FRAC_PI_2).sin_cos();
                gains = (dry_gain, wet_gain);
            }
            let ((sin, cos), (dry_gain, wet_gain)) = (rotation, gains);
            let wet = [cos * wet[0] + sin * wet[1], sin * wet[0] + cos * wet[1]];
            for ((sample, dry), wet) in frame.iter_mut().zip(input).zip(wet) {
                *sample = (dry * dry_gain + wet * wet_gain) as f32;
            }

            self.spin_phase = (self.spin_phase + spin_step).fract();
            self.chorus_phase = (self.chorus_phase + chorus_step).fract();
            self.frame = self.frame.wrapping_add(1);
        }
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod tests;
