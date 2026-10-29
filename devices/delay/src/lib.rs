// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Delay: a stereo delay with one line per side, a band-pass inside the feedback loop, one LFO
//! that moves the delay time and the band, three ways to change the time (Repitch, Fade, Jump),
//! ping-pong and freeze. The design is in `docs/spec/delay.md`.
//!
//! Per side and frame: the line is read at the delay time, which gives the wet signal; what goes
//! back into the line is the input plus the wet signal times the feedback, through the band-pass,
//! so every repeat passes the filter once more. Ping-pong feeds each side's wet signal into the
//! other line and the mono sum of the input into the left one, so repeats alternate left and
//! right. Freeze writes the wet signal back unchanged instead (no input, no filter, gain 1), so
//! the lines hold what they have.
//!
//! The delay time is a number of sixteenth notes at the tempo, made longer or shorter by the
//! offset (sync), or a time in seconds. Repitch glides the time to a new value, so the pitch
//! bends as on a tape delay; Fade crossfades from the old time to the new one; Jump changes it at
//! once, in whole frames. The LFO swings the time by up to a quarter of it and the band's centre
//! by up to two octaves either way.

use suhina_device_api::{
    DEFAULT_SAMPLE_RATE, Device, DeviceError, DeviceInfo, ParamBank, ParamInfo, ProcessContext, Role, Scale, Unit,
    check_sample_rate,
};
use suhina_dsp::biquad::BUTTERWORTH_Q;
use suhina_dsp::lfo::{Shape, wander, wave};
use suhina_dsp::sync::{LfoClock, RateMode};
use suhina_dsp::{Biquad, Coefficients, DelayLine, OnePole, Smoother, time_constant};

/// Ramp for feedback, mix, freeze and filter changes.
pub const RAMP_SECONDS: f64 = 0.02;
/// Longest delay, the LFO's swing included; synced times at very low tempos stop here.
pub const MAX_DELAY_SECONDS: f64 = 8.0;
/// Fastest change of the delay time in Repitch, in seconds of delay per second: the pitch moves by
/// at most this fraction while the time glides.
pub const REPITCH_RATE: f64 = 0.5;
/// Time constant that rounds the start and end of a Repitch glide.
pub const REPITCH_SECONDS: f64 = 0.02;
/// Length of a Fade crossfade.
pub const FADE_SECONDS: f64 = 0.05;
/// How far the LFO at 100 % swings the delay time, as a fraction of it.
pub const LFO_TIME_SWING: f64 = 0.25;
/// How far the LFO at 100 % swings the band's centre, in octaves either way.
pub const LFO_FILTER_OCTAVES: f64 = 2.0;
/// Time constant that softens the edges of the square and sample-and-hold LFO shapes.
pub const LFO_EDGE_SECONDS: f64 = 0.002;
/// The band's corners stay between these frequencies.
pub const FILTER_RANGE: (f64, f64) = (50.0, 18_000.0);
/// Frames between filter coefficient updates while the band moves, counted from the last reset.
const CONTROL_FRAMES: u64 = 4;

/// Parameter indices, in [`INFO`] order.
pub mod param {
    /// Right side takes the left side's time settings (all but its offset).
    pub const LINK: usize = 0;
    /// Left time follows the tempo in sixteenth notes instead of seconds.
    pub const SYNC_LEFT: usize = 1;
    /// Right time follows the tempo in sixteenth notes instead of seconds.
    pub const SYNC_RIGHT: usize = 2;
    /// Left synced time, an index into the sixteenth-note buttons.
    pub const SIXTEENTHS_LEFT: usize = 3;
    /// Right synced time, an index into the sixteenth-note buttons.
    pub const SIXTEENTHS_RIGHT: usize = 4;
    /// Left free time in seconds.
    pub const TIME_LEFT: usize = 5;
    /// Right free time in seconds.
    pub const TIME_RIGHT: usize = 6;
    /// Left synced time made longer or shorter, as a fraction of it.
    pub const OFFSET_LEFT: usize = 7;
    /// Right synced time made longer or shorter, as a fraction of it.
    pub const OFFSET_RIGHT: usize = 8;
    /// How much of the wet signal goes back into the line.
    pub const FEEDBACK: usize = 9;
    /// Holds what the lines have, taking no new input.
    pub const FREEZE: usize = 10;
    /// Repeats alternate between the left and right sides.
    pub const PING_PONG: usize = 11;
    /// How the time follows a change: Repitch, Fade or Jump.
    pub const TRANSITION: usize = 12;
    /// Band-pass in the feedback loop on or off.
    pub const FILTER: usize = 13;
    /// Centre of the band-pass in hertz.
    pub const FILTER_FREQUENCY: usize = 14;
    /// Width of the band-pass in octaves.
    pub const FILTER_WIDTH: usize = 15;
    /// How the LFO's speed is set: rate, time, synced, triplet, dotted or sixteenths.
    pub const LFO_MODE: usize = 16;
    /// LFO speed in hertz, in the Rate mode.
    pub const LFO_RATE: usize = 17;
    /// Length of one LFO cycle in seconds, in the Time mode.
    pub const LFO_TIME: usize = 18;
    /// Synced LFO cycle, an index into the synced rates.
    pub const LFO_SYNCED: usize = 19;
    /// Length of one LFO cycle in sixteenth notes, in the Sixteenths mode.
    pub const LFO_SIXTEENTHS: usize = 20;
    /// LFO waveform, an index into the shape names.
    pub const LFO_SHAPE: usize = 21;
    /// Moves the middle of the LFO's cycle earlier or later.
    pub const LFO_MORPH: usize = 22;
    /// How far the LFO swings the delay time.
    pub const LFO_TO_TIME: usize = 23;
    /// How far the LFO swings the band's centre.
    pub const LFO_TO_FILTER: usize = 24;
    /// Share of the wet signal in the output.
    pub const MIX: usize = 25;
    /// Dry/wet mix by the equal-loudness law instead of the linear one.
    pub const EQUAL_LOUDNESS: usize = 26;
    /// How many parameters there are.
    pub(crate) const COUNT: usize = 27;
}

/// The sync buttons, and how many sixteenth notes each is.
const SIXTEENTHS: [(&str, f64); 8] =
    [("1", 1.0), ("2", 2.0), ("3", 3.0), ("4", 4.0), ("5", 5.0), ("6", 6.0), ("8", 8.0), ("16", 16.0)];
/// The sync buttons' labels.
const SIXTEENTH_NAMES: [&str; 8] = names(SIXTEENTHS);

/// The synced LFO rates, and one cycle's length in bars.
const SYNCED_RATES: [(&str, f64); 22] = [
    ("8 Bars", 8.0),
    ("6 Bars", 6.0),
    ("4 Bars", 4.0),
    ("3 Bars", 3.0),
    ("2 Bars", 2.0),
    ("1.5 Bars", 1.5),
    ("1 Bar", 1.0),
    ("3/4", 0.75),
    ("1/2", 0.5),
    ("3/8", 0.375),
    ("1/3", 1.0 / 3.0),
    ("5/16", 0.3125),
    ("1/4", 0.25),
    ("3/16", 0.1875),
    ("1/6", 1.0 / 6.0),
    ("1/8", 0.125),
    ("1/12", 1.0 / 12.0),
    ("1/16", 0.0625),
    ("1/24", 1.0 / 24.0),
    ("1/32", 0.03125),
    ("1/48", 1.0 / 48.0),
    ("1/64", 0.015625),
];
/// The synced LFO rates' labels.
const SYNCED_RATE_NAMES: [&str; 22] = names(SYNCED_RATES);

/// The labels of a table of labels and values, for a choice parameter.
const fn names<const N: usize>(table: [(&'static str, f64); N]) -> [&'static str; N] {
    let mut names = [""; N];
    let mut i = 0;
    while i < N {
        names[i] = table[i].0;
        i += 1;
    }
    names
}

/// How the delay time can follow a change, in [`Transition`] order.
const TRANSITIONS: [&str; 3] = ["Repitch", "Fade", "Jump"];
/// How the LFO's speed can be set.
const LFO_MODES: [&str; 6] = RateMode::NAMES;
/// The LFO's waveforms.
const LFO_SHAPES: [&str; 7] = ["Sine", "Triangle", "Ramp Up", "Ramp Down", "Square", "Sample and Hold", "Wander"];

/// How the delay time follows a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transition {
    /// The time glides to the new value, so the pitch bends.
    Repitch,
    /// The old time crossfades into the new one.
    Fade,
    /// The time changes at once, in whole frames.
    Jump,
}

/// The LFO's waveform.
#[derive(Debug, Clone, Copy, PartialEq)]
enum LfoShape {
    /// A periodic wave, its cycle bent by Morph.
    Wave(Shape),
    /// Sample and hold: a new random value every cycle.
    Random,
    /// A random curve that glides from one value to the next.
    Wander,
}

/// The LFO's waveform and where Morph puts the middle of its cycle.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Lfo {
    /// The waveform.
    shape: LfoShape,
    /// The first half of the waveform takes this much of the cycle, and the second half the rest.
    split: f64,
}

impl Lfo {
    /// The value, -1..1, at position `cycles`.
    #[inline]
    fn at(self, cycles: f64) -> f64 {
        match self.shape {
            LfoShape::Random => wave(Shape::Random, cycles),
            LfoShape::Wander => wander(cycles),
            LfoShape::Wave(shape) => {
                let whole = cycles.floor();
                let (phase, split) = (cycles - whole, self.split);
                let warped =
                    if phase < split { 0.5 * phase / split } else { 0.5 + 0.5 * (phase - split) / (1.0 - split) };
                wave(shape, whole + warped)
            }
        }
    }
}

/// The parameters, in [`param`] order.
const PARAMS: [ParamInfo; param::COUNT] = [
    ParamInfo::toggle("link", "Link", true).in_group("Time"),
    ParamInfo::toggle("syncLeft", "Sync Left", true).in_group("Time"),
    ParamInfo::toggle("syncRight", "Sync Right", true).in_group("Time"),
    ParamInfo::choice("sixteenthsLeft", "Sixteenths Left", &SIXTEENTH_NAMES, 2).in_group("Time"),
    ParamInfo::choice("sixteenthsRight", "Sixteenths Right", &SIXTEENTH_NAMES, 3).in_group("Time"),
    ParamInfo::continuous("timeLeft", "Time Left", (0.001, 5.0), 0.375, Unit::Seconds, Scale::Logarithmic)
        .in_group("Time"),
    ParamInfo::continuous("timeRight", "Time Right", (0.001, 5.0), 0.375, Unit::Seconds, Scale::Logarithmic)
        .in_group("Time"),
    ParamInfo::continuous("offsetLeft", "Offset Left", (-0.333, 0.333), 0.0, Unit::Percent, Scale::Linear)
        .in_group("Time"),
    ParamInfo::continuous("offsetRight", "Offset Right", (-0.333, 0.333), 0.0, Unit::Percent, Scale::Linear)
        .in_group("Time"),
    ParamInfo::continuous("feedback", "Feedback", (0.0, 0.95), 0.5, Unit::Percent, Scale::Linear).in_group("Feedback"),
    ParamInfo::toggle("freeze", "Freeze", false).in_group("Feedback"),
    ParamInfo::toggle("pingPong", "Ping Pong", false).in_group("Feedback"),
    ParamInfo::choice("transition", "Transition", &TRANSITIONS, 0).in_group("Feedback"),
    ParamInfo::toggle("filter", "Filter", true).in_group("Filter"),
    ParamInfo::continuous(
        "filterFrequency",
        "Filter Frequency",
        FILTER_RANGE,
        1_000.0,
        Unit::Hertz,
        Scale::Logarithmic,
    )
    .in_group("Filter"),
    ParamInfo::continuous("filterWidth", "Filter Width", (0.5, 9.0), 8.0, Unit::Octaves, Scale::Linear)
        .in_group("Filter"),
    ParamInfo::choice("lfoMode", "LFO Mode", &LFO_MODES, 0).in_group("LFO"),
    ParamInfo::continuous("lfoRate", "LFO Rate", (0.01, 40.0), 0.5, Unit::Hertz, Scale::Logarithmic).in_group("LFO"),
    ParamInfo::continuous("lfoTime", "LFO Time", (0.1, 200.0), 1.0, Unit::Seconds, Scale::Logarithmic).in_group("LFO"),
    ParamInfo::choice("lfoSynced", "LFO Synced Rate", &SYNCED_RATE_NAMES, 4).in_group("LFO"),
    ParamInfo::integer("lfoSixteenths", "LFO Sixteenths", (1, 64), 16, Unit::None).in_group("LFO"),
    ParamInfo::choice("lfoShape", "LFO Shape", &LFO_SHAPES, 1).in_group("LFO"),
    ParamInfo::continuous("lfoMorph", "LFO Morph", (-1.0, 1.0), 0.0, Unit::Percent, Scale::Linear).in_group("LFO"),
    ParamInfo::continuous("lfoToTime", "LFO to Time", (0.0, 1.0), 0.0, Unit::Percent, Scale::Linear).in_group("LFO"),
    ParamInfo::continuous("lfoToFilter", "LFO to Filter", (0.0, 1.0), 0.0, Unit::Percent, Scale::Linear)
        .in_group("LFO"),
    ParamInfo::continuous("mix", "Dry/Wet", (0.0, 1.0), 0.5, Unit::Percent, Scale::Linear).in_group("Output"),
    ParamInfo::toggle("equalLoudness", "Equal-Loudness Mix", false).in_group("Output"),
];

/// What the delay is and the parameters it has.
pub const INFO: DeviceInfo = DeviceInfo { id: "delay", name: "Delay", role: Role::Effect, params: &PARAMS };

/// Where a side's line is read, and how the read position follows the delay time.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Reader {
    /// Repitch: the time after the rate limit, then after the rounding filter (frames).
    slewed: f64,
    /// Repitch: the read position, the rate-limited time after the rounding filter (frames).
    glide: f64,
    /// Fade: the time being faded out and the one faded in, and how far the fade is (0..1; 1 when
    /// no fade runs).
    from: f64,
    /// Fade: the time being faded in (frames).
    to: f64,
    /// Fade: how far the fade is.
    fade: f64,
}

impl Reader {
    /// A reader settled at `frames`, with no glide or fade running.
    fn at(frames: f64) -> Self {
        Self { slewed: frames, glide: frames, from: frames, to: frames, fade: 1.0 }
    }
}

/// One side: its line and band-pass.
#[derive(Debug, Clone, PartialEq, Default)]
struct Side {
    /// The side's delay line.
    line: DelayLine,
    /// The band-pass's lower corner.
    high_pass: Biquad,
    /// The band-pass's upper corner.
    low_pass: Biquad,
    /// Where the line is read.
    reader: Reader,
}

impl Side {
    /// A silent side with a line long enough for the longest delay (allocates).
    fn new(sample_rate: f64) -> Self {
        Self { line: DelayLine::for_seconds(MAX_DELAY_SECONDS, sample_rate), ..Self::default() }
    }

    /// Silences the line and the band-pass and forgets the read position.
    fn clear(&mut self) {
        self.line.clear();
        self.high_pass.reset();
        self.low_pass.reset();
        self.reader = Reader::default();
    }

    /// The wet sample at `target` frames of delay, the read position following it by `transition`.
    fn read(&mut self, target: f64, transition: Transition, fresh: bool, rates: &Rates) -> f32 {
        let r = &mut self.reader;
        if fresh {
            *r = Reader::at(target);
        }
        match transition {
            Transition::Repitch => {
                let slewed = r.slewed + (target - r.slewed).clamp(-REPITCH_RATE, REPITCH_RATE);
                let glide = slewed + (r.glide - slewed) * rates.repitch;
                *r = Reader { slewed, ..Reader::at(glide) };
                self.line.read_hermite(glide)
            }
            Transition::Fade => {
                if r.fade >= 1.0 && (target - r.to).abs() >= 1.0 {
                    r.from = r.to;
                    r.to = target;
                    r.fade = 0.0;
                }
                let (from, to) = (r.from, r.to);
                let x = if r.fade >= 1.0 {
                    self.line.read_hermite(to)
                } else {
                    r.fade = (r.fade + rates.fade_step).min(1.0);
                    let w = (0.5 - 0.5 * (std::f64::consts::PI * r.fade).cos()) as f32;
                    self.line.read_hermite(from) * (1.0 - w) + self.line.read_hermite(to) * w
                };
                r.slewed = r.to;
                r.glide = r.to;
                x
            }
            Transition::Jump => {
                let whole = target.round();
                *r = Reader::at(whole);
                self.line.read(whole as usize)
            }
        }
    }

    /// Writes the next sample into the line: `x`, through the band-pass in proportion to
    /// `filter`, or `held` unchanged in proportion to `freeze`. The band-pass always runs, so
    /// switching it crossfades.
    fn write(&mut self, x: f64, held: f32, filter: f32, freeze: f32) {
        let filtered = self.low_pass.process(self.high_pass.process(x));
        let fed = x + (filtered - x) * f64::from(filter);
        self.line.push(fed as f32 * (1.0 - freeze) + held * freeze);
    }
}

/// Per-frame constants from the sample rate.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rates {
    /// Repitch's rounding filter coefficient.
    repitch: f64,
    /// How far a fade moves per frame.
    fade_step: f64,
}

impl Rates {
    /// The constants at `sample_rate`.
    fn new(sample_rate: f64) -> Self {
        Self { repitch: time_constant(REPITCH_SECONDS, sample_rate), fade_step: 1.0 / (FADE_SECONDS * sample_rate) }
    }
}

/// The dry and wet gains for the mix and the share of the equal-loudness law, worked out again
/// only when one of them changes.
struct MixLaw {
    /// The mix and the equal-loudness share the gains were worked out for.
    key: (f32, f32),
    /// The dry and wet gains.
    gains: (f64, f64),
}

impl MixLaw {
    /// A law that works the gains out on its first use.
    fn new() -> Self {
        // NaN never equals a key, so the first frame works the gains out.
        Self { key: (f32::NAN, f32::NAN), gains: (0.0, 0.0) }
    }

    /// The dry and wet gains for `mix` and `loudness`, the share of the equal-loudness law.
    #[inline]
    fn gains(&mut self, mix: f32, loudness: f32) -> (f64, f64) {
        if (mix, loudness) != self.key {
            self.key = (mix, loudness);
            let (mix, loudness) = (f64::from(mix), f64::from(loudness));
            let (sin, cos) = (mix * std::f64::consts::FRAC_PI_2).sin_cos();
            self.gains = ((1.0 - mix) + (cos - (1.0 - mix)) * loudness, mix + (sin - mix) * loudness);
        }
        self.gains
    }
}

/// The stereo delay: two lines with a band-pass in their feedback loop and an LFO.
pub struct Delay {
    /// The sample rate it was prepared for.
    sample_rate: f64,
    /// Per-frame constants from the sample rate.
    rates: Rates,
    /// Every parameter's value, in [`param`] order.
    params: ParamBank<{ param::COUNT }>,
    /// How much of the band-pass is heard (0 off, 1 on).
    filter: Smoother,
    /// Feedback gain.
    feedback: Smoother,
    /// Share of the wet signal.
    mix: Smoother,
    /// How much the lines hold what they have (0 off, 1 frozen).
    freeze: Smoother,
    /// log2 of the band's centre, and its width in octaves.
    centre: Smoother,
    /// The band's width in octaves.
    width: Smoother,
    /// The LFO's depths on the time and the band.
    to_time: Smoother,
    /// The LFO's depth on the band.
    to_filter: Smoother,
    /// How much of the equal-loudness mix law is used (0 linear, 1 equal loudness).
    loudness: Smoother,
    /// The left and right sides.
    sides: [Side; 2],
    /// Softens the edges of the square and sample-and-hold LFO shapes.
    lfo_edge: OnePole,
    /// Free-running LFO position: whole cycles and the fraction of the current one.
    lfo_cycles: u64,
    /// Free-running LFO position: the fraction of the current cycle.
    lfo_phase: f64,
    /// Frames since the last reset, for the filter update grid.
    frame: u64,
    /// Whether the readers and the LFO filter still have to start at their first values.
    fresh: bool,
}

impl Default for Delay {
    /// The same as [`Delay::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl Delay {
    /// A delay with every parameter at its default, prepared for the default sample rate
    /// (allocates).
    pub fn new() -> Self {
        let sr = DEFAULT_SAMPLE_RATE;
        let still = Smoother::new(0.0, 1);
        let mut delay = Self {
            sample_rate: sr,
            rates: Rates::new(sr),
            params: ParamBank::new(&PARAMS),
            filter: still,
            feedback: still,
            mix: still,
            freeze: still,
            centre: still,
            width: still,
            to_time: still,
            to_filter: still,
            loudness: still,
            sides: [Side::new(sr), Side::new(sr)],
            lfo_edge: OnePole::new(LFO_EDGE_SECONDS, sr),
            lfo_cycles: 0,
            lfo_phase: 0.0,
            frame: 0,
            fresh: true,
        };
        delay.rebuild();
        delay
    }

    /// The value of parameter `index`, 0.0 for an unknown one.
    fn value(&self, index: usize) -> f64 {
        self.params.get(index)
    }

    /// Whether switch `index` is on.
    fn on(&self, index: usize) -> bool {
        self.params.on(index)
    }

    /// Where each smoother is heading, in [`smoothers`](Self::smoothers) order.
    fn targets(&self) -> [f32; 9] {
        let flag = |index| if self.on(index) { 1.0 } else { 0.0 };
        [
            flag(param::FILTER),
            self.value(param::FEEDBACK) as f32,
            self.value(param::MIX) as f32,
            flag(param::FREEZE),
            self.value(param::FILTER_FREQUENCY).max(1.0).log2() as f32,
            self.value(param::FILTER_WIDTH) as f32,
            self.value(param::LFO_TO_TIME) as f32,
            self.value(param::LFO_TO_FILTER) as f32,
            flag(param::EQUAL_LOUDNESS),
        ]
    }

    /// The smoothed values, in [`targets`](Self::targets) order.
    fn smoothers(&mut self) -> [&mut Smoother; 9] {
        [
            &mut self.filter,
            &mut self.feedback,
            &mut self.mix,
            &mut self.freeze,
            &mut self.centre,
            &mut self.width,
            &mut self.to_time,
            &mut self.to_filter,
            &mut self.loudness,
        ]
    }

    /// Silences the delay and settles every smoother at its target, after a new sample rate or
    /// a reset.
    fn rebuild(&mut self) {
        let ramp = Smoother::ramp_frames(RAMP_SECONDS, self.sample_rate);
        let targets = self.targets();
        for (smoother, target) in self.smoothers().into_iter().zip(targets) {
            smoother.set_ramp(ramp);
            smoother.reset(target);
        }
        for side in &mut self.sides {
            side.clear();
        }
        self.lfo_cycles = 0;
        self.lfo_phase = 0.0;
        self.frame = 0;
        self.fresh = true;
        self.tune_filters(0.0);
    }

    /// How the delay time follows a change.
    fn transition(&self) -> Transition {
        match self.value(param::TRANSITION) as usize {
            1 => Transition::Fade,
            2 => Transition::Jump,
            _ => Transition::Repitch,
        }
    }

    /// One side's delay time in seconds before the LFO, at `tempo`. With Link on, the right side
    /// takes the left side's settings except its offset.
    fn base_seconds(&self, right: bool, tempo: f64) -> f64 {
        let own = |l: usize, r: usize| if right { r } else { l };
        let linked = |l: usize, r: usize| if right && !self.on(param::LINK) { r } else { l };
        let seconds = if self.on(linked(param::SYNC_LEFT, param::SYNC_RIGHT)) {
            let step = self.value(linked(param::SIXTEENTHS_LEFT, param::SIXTEENTHS_RIGHT)) as usize;
            let sixteenths = SIXTEENTHS.get(step).map_or(1.0, |s| s.1);
            sixteenths * 15.0 / tempo * (1.0 + self.value(own(param::OFFSET_LEFT, param::OFFSET_RIGHT)))
        } else {
            self.value(linked(param::TIME_LEFT, param::TIME_RIGHT))
        };
        seconds.min(MAX_DELAY_SECONDS / (1.0 + LFO_TIME_SWING))
    }

    /// The LFO's length of one cycle in beats when it follows the tempo, or `None` when it runs in
    /// hertz or seconds.
    fn lfo_beats(&self) -> Option<f64> {
        let synced = SYNCED_RATES.get(self.value(param::LFO_SYNCED) as usize).map_or(4.0, |r| r.1 * 4.0);
        RateMode::from_value(self.value(param::LFO_MODE)).cycle_beats(synced, self.value(param::LFO_SIXTEENTHS))
    }

    /// The LFO's speed in hertz when it runs free of the tempo.
    fn lfo_hertz(&self) -> f64 {
        match RateMode::from_value(self.value(param::LFO_MODE)) {
            RateMode::Time => 1.0 / self.value(param::LFO_TIME).max(1e-3),
            _ => self.value(param::LFO_RATE),
        }
    }

    /// The LFO's waveform. Morph moves the middle of the cycle: the first half of the waveform
    /// takes (1 + morph) / 2 of the cycle and the second half the rest.
    fn lfo(&self) -> Lfo {
        let shape = match self.value(param::LFO_SHAPE) as usize {
            1 => LfoShape::Wave(Shape::Triangle),
            2 => LfoShape::Wave(Shape::SawUp),
            3 => LfoShape::Wave(Shape::SawDown),
            4 => LfoShape::Wave(Shape::Square),
            5 => LfoShape::Random,
            6 => LfoShape::Wander,
            _ => LfoShape::Wave(Shape::Sine),
        };
        Lfo { shape, split: ((1.0 + self.value(param::LFO_MORPH)) / 2.0).clamp(0.02, 0.98) }
    }

    /// Band-pass coefficients for the current (smoothed) centre and width, moved by `lfo`
    /// (-1..1).
    fn tune_filters(&mut self, lfo: f64) {
        let sr = self.sample_rate;
        let swing = lfo * f64::from(self.to_filter.current()) * LFO_FILTER_OCTAVES;
        let centre = f64::from(self.centre.current()) + swing;
        let half = f64::from(self.width.current()) / 2.0;
        let top = FILTER_RANGE.1.min(sr * 0.49);
        let low = 2.0f64.powf(centre - half).clamp(FILTER_RANGE.0, top);
        let high = 2.0f64.powf(centre + half).clamp(FILTER_RANGE.0, top);
        let (hp, lp) = (Coefficients::highpass(sr, low, BUTTERWORTH_Q), Coefficients::lowpass(sr, high, BUTTERWORTH_Q));
        for side in &mut self.sides {
            side.high_pass.set(hp);
            side.low_pass.set(lp);
        }
    }
}

impl Device for Delay {
    /// The delay's [`INFO`].
    fn info(&self) -> &'static DeviceInfo {
        &INFO
    }

    /// Allocates the lines for `sample_rate` and starts silent.
    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError> {
        let sr = check_sample_rate(sample_rate)?;
        self.sample_rate = sr;
        self.rates = Rates::new(sr);
        self.sides = [Side::new(sr), Side::new(sr)];
        self.lfo_edge = OnePole::new(LFO_EDGE_SECONDS, sr);
        self.rebuild();
        Ok(())
    }

    /// Silences the lines and restarts the LFO.
    fn reset(&mut self) {
        self.rebuild();
    }

    /// Sets parameter `index`, clamped to its range; the smoothed values ramp to it.
    fn set_param(&mut self, index: usize, value: f64) {
        if self.params.set(index, value).is_none() {
            return;
        }
        let targets = self.targets();
        for (smoother, target) in self.smoothers().into_iter().zip(targets) {
            smoother.set_target(target);
        }
    }

    /// The value of parameter `index`, or `None` for an unknown one.
    fn param(&self, index: usize) -> Option<f64> {
        self.params.param(index)
    }

    /// Runs interleaved stereo `buffer` through the delay in place, at the tempo and song
    /// position in `context`.
    fn process(&mut self, context: &ProcessContext, buffer: &mut [f32]) {
        let sr = self.sample_rate;
        let tempo = context.tempo_or_default();
        let clock =
            LfoClock::new(tempo, sr, self.lfo_beats(), self.lfo_hertz()).following(context.playing, context.beat);
        let lfo_wave = self.lfo();
        let cycles_per_frame = clock.cycles_per_frame();
        let bases = [self.base_seconds(false, tempo) * sr, self.base_seconds(true, tempo) * sr];
        let transition = self.transition();
        let ping_pong = self.on(param::PING_PONG);
        let longest = MAX_DELAY_SECONDS * sr;
        let mut law = MixLaw::new();

        for (n, frame) in buffer.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let cycles = clock.song_cycles(n).unwrap_or(self.lfo_cycles as f64 + self.lfo_phase);
            let raw = lfo_wave.at(cycles);
            if self.fresh {
                self.lfo_edge.reset(raw);
            }
            let lfo = self.lfo_edge.process(raw);
            let (filter, feedback, mix, freeze) =
                (self.filter.next_value(), self.feedback.next_value(), self.mix.next_value(), self.freeze.next_value());
            let band_moves = !(self.centre.is_settled() && self.width.is_settled() && self.to_filter.is_settled())
                || self.to_filter.current() > 0.0;
            self.centre.next_value();
            self.width.next_value();
            let to_time = f64::from(self.to_time.next_value());
            self.to_filter.next_value();
            // The band is retuned every few frames while it moves, so it glides without zipper
            // noise; a settled band keeps its coefficients.
            if band_moves && self.frame.is_multiple_of(CONTROL_FRAMES) {
                self.tune_filters(lfo);
            }

            let fresh = self.fresh;
            let swing = 1.0 + LFO_TIME_SWING * to_time * lfo;
            let [left, right] = &mut self.sides;
            let wet = [
                left.read((bases[0] * swing).clamp(2.0, longest), transition, fresh, &self.rates),
                right.read((bases[1] * swing).clamp(2.0, longest), transition, fresh, &self.rates),
            ];
            let input = frame.map(|s| if s.is_finite() { f64::from(s) } else { 0.0 });
            let fb = f64::from(feedback);
            if ping_pong {
                left.write((input[0] + input[1]) / 2.0 + fb * f64::from(wet[1]), wet[1], filter, freeze);
                right.write(fb * f64::from(wet[0]), wet[0], filter, freeze);
            } else {
                left.write(input[0] + fb * f64::from(wet[0]), wet[0], filter, freeze);
                right.write(input[1] + fb * f64::from(wet[1]), wet[1], filter, freeze);
            }

            let (dry_gain, wet_gain) = law.gains(mix, self.loudness.next_value());
            for ((sample, x), y) in frame.iter_mut().zip(input).zip(wet) {
                *sample = (x * dry_gain + f64::from(y) * wet_gain) as f32;
            }

            self.fresh = false;
            self.frame = self.frame.wrapping_add(1);
            if clock.follows_song() {
                // Carry the song's position over, so stopping the transport runs on from here.
                let next = cycles + cycles_per_frame;
                if next.is_finite() {
                    self.lfo_cycles = next.floor().rem_euclid((1u64 << 32) as f64) as u64;
                    self.lfo_phase = next - next.floor();
                }
            } else {
                self.lfo_phase += cycles_per_frame;
                if self.lfo_phase >= 1.0 {
                    self.lfo_cycles = self.lfo_cycles.wrapping_add(self.lfo_phase as u64) % (1 << 32);
                    self.lfo_phase = self.lfo_phase.fract();
                }
            }
        }
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod tests;
