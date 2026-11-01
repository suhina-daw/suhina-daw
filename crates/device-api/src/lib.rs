// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The interface between Suhina's built-in effects and instruments and whatever runs them (ADR 0004).
//!
//! A device describes itself with a static [`DeviceInfo`]: a stable id, a name and its
//! parameters ([`ParamInfo`]). The host prepares it off the audio thread, then calls
//! [`Device::process`] block by block on the audio thread. Parameter values are plain numbers in
//! the parameter's own unit (decibels, hertz, a fraction for percentages), never normalised, so
//! a stored value keeps its meaning if a range changes; [`ParamInfo`] converts to and from the
//! 0..1 range a knob or an automation lane uses.
//!
//! Instruments also take notes: the host hands them [`NoteEvent`]s through [`Device::event`]
//! between blocks, splitting a block where an event falls, as it does for automation. MIDI
//! effects change the notes on their way to the instrument: the host hands them a span's events
//! through [`Device::process_events`] and passes on what they send.
//!
//! This crate depends on nothing in Suhina, so devices can be built, tested and later shipped
//! as plug-ins without the engine.

pub mod conformance;
pub mod sample_edits;

use std::fmt;

/// How a parameter's value is shown and what it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    None,
    Decibels,
    Hertz,
    Seconds,
    /// A fraction shown as a percentage: 1.0 is 100 %.
    Percent,
    /// -1.0 is fully left, 0.0 centre, 1.0 fully right.
    Balance,
    /// A bandwidth in octaves.
    Octaves,
    /// A distance in pitch, in semitones.
    Semitones,
    /// A distance in steps along the scale the notes are in.
    ScaleDegrees,
    /// A MIDI key, shown as a note name (60 is C3).
    Key,
}

/// How a continuous parameter maps onto a knob's travel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    Linear,
    /// Equal steps of travel multiply the value by the same factor (frequencies). Needs a
    /// positive range; otherwise the parameter behaves as linear.
    Logarithmic,
    /// A gain in decibels on a knob that is even in decibels over the top three quarters of its
    /// travel, 70 dB per whole travel, from `max` down to 52.5 dB below it; under that the gain
    /// is a power of the travel (the one that keeps the slope in decibels unbroken), to silence
    /// at the bottom, where the value is `min`. Utility's Gain: 0 dB in the middle for a top of
    /// +35 dB. `docs/spec/utility.md`.
    Gain,
}

/// Decibels per whole travel over a [`Scale::Gain`] knob's top three quarters.
const GAIN_SLOPE: f64 = 70.0;
/// Where a [`Scale::Gain`] knob stops being even in decibels.
const GAIN_KNEE: f64 = 0.25;
/// The power of the travel under the knee: `GAIN_SLOPE * GAIN_KNEE * ln 10 / 20`, so the slope in
/// decibels does not break at the knee.
const GAIN_POWER: f64 = GAIN_SLOPE * GAIN_KNEE * std::f64::consts::LN_10 / 20.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    Continuous(Scale),
    /// 0.0 is off, 1.0 is on.
    Toggle,
    /// The value is the index of the chosen option.
    Choice(&'static [&'static str]),
    /// A whole number between `min` and `max` (a level from 0 to 99, a number of semitones).
    Integer,
}

/// One parameter of a device. `id` is stable: projects store values by it, so renaming it is a
/// breaking change; `name` is what the user sees and may change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamInfo {
    pub id: &'static str,
    pub name: &'static str,
    /// The part of the device it belongs to ("Op 1", "LFO"), or empty: lists of many
    /// parameters (the automation lanes' menu) gather a group's parameters under its name.
    pub group: &'static str,
    pub kind: ParamKind,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub unit: Unit,
}

impl ParamInfo {
    pub const fn continuous(
        id: &'static str,
        name: &'static str,
        range: (f64, f64),
        default: f64,
        unit: Unit,
        scale: Scale,
    ) -> Self {
        Self { id, name, group: "", kind: ParamKind::Continuous(scale), min: range.0, max: range.1, default, unit }
    }

    pub const fn toggle(id: &'static str, name: &'static str, default: bool) -> Self {
        let default = if default { 1.0 } else { 0.0 };
        Self { id, name, group: "", kind: ParamKind::Toggle, min: 0.0, max: 1.0, default, unit: Unit::None }
    }

    /// A whole number in `range`.
    pub const fn integer(id: &'static str, name: &'static str, range: (i32, i32), default: i32, unit: Unit) -> Self {
        Self {
            id,
            name,
            group: "",
            kind: ParamKind::Integer,
            min: range.0 as f64,
            max: range.1 as f64,
            default: default as f64,
            unit,
        }
    }

    /// A choice between `options`; the default is the option at index `default`.
    pub const fn choice(
        id: &'static str,
        name: &'static str,
        options: &'static [&'static str],
        default: usize,
    ) -> Self {
        let max = options.len().saturating_sub(1) as f64;
        Self {
            id,
            name,
            group: "",
            kind: ParamKind::Choice(options),
            min: 0.0,
            max,
            default: default as f64,
            unit: Unit::None,
        }
    }

    /// The same parameter in `group` ("Op 1", "LFO").
    pub const fn in_group(self, group: &'static str) -> Self {
        Self { group, ..self }
    }

    /// `value` made valid for this parameter: limited to its range, rounded to a whole number
    /// for toggles, choices and integers, and the default when it is not a finite number.
    pub fn clamp(&self, value: f64) -> f64 {
        let value = self.limit(value);
        match self.kind {
            ParamKind::Continuous(_) => value,
            ParamKind::Toggle | ParamKind::Choice(_) | ParamKind::Integer => value.round(),
        }
    }

    /// `value` limited to the range, not rounded; the default when it is not a finite number.
    fn limit(&self, value: f64) -> f64 {
        if value.is_finite() { value.clamp(self.min, self.max) } else { self.default }
    }

    /// The value the parameter takes when an automation lane gives it `value`: an integer's with
    /// its fraction cut off toward zero (2.97 takes 2, -2.97 takes -2; a value within 1e-9 of a
    /// whole number is that number, so a lane's arithmetic does not lose it), otherwise as
    /// [`Self::clamp`] makes it (`docs/spec/automation.md` › Lanes and points).
    pub fn played(&self, value: f64) -> f64 {
        match self.kind {
            ParamKind::Integer if value.is_finite() => {
                let whole = value.round();
                let value = if (value - whole).abs() <= 1e-9 { whole } else { value.trunc() };
                // Adding zero makes -0 (from -0.94) plain 0.
                self.limit(value) + 0.0
            }
            _ => self.clamp(value),
        }
    }

    fn logarithmic(&self) -> bool {
        matches!(self.kind, ParamKind::Continuous(Scale::Logarithmic)) && self.min > 0.0 && self.max > self.min
    }

    fn gain(&self) -> bool {
        matches!(self.kind, ParamKind::Continuous(Scale::Gain)) && self.min < self.max - GAIN_SLOPE * (1.0 - GAIN_KNEE)
    }

    /// Where `value` sits on the knob, from 0.0 to 1.0. An integer's value between two whole
    /// numbers, which an automation lane can hold, sits where it falls between them, unrounded.
    pub fn to_normalized(&self, value: f64) -> f64 {
        let value = if matches!(self.kind, ParamKind::Integer) { self.limit(value) } else { self.clamp(value) };
        let n = if self.max <= self.min {
            0.0
        } else if self.gain() {
            let knee = self.max - GAIN_SLOPE * (1.0 - GAIN_KNEE);
            if value <= self.min {
                0.0
            } else if value >= knee {
                1.0 - (self.max - value) / GAIN_SLOPE
            } else {
                GAIN_KNEE * 10f64.powf((value - knee) / (20.0 * GAIN_POWER))
            }
        } else if self.logarithmic() {
            (value / self.min).ln() / (self.max / self.min).ln()
        } else {
            (value - self.min) / (self.max - self.min)
        };
        n.clamp(0.0, 1.0)
    }

    /// The value at knob position `normalized` (0.0 to 1.0).
    pub fn from_normalized(&self, normalized: f64) -> f64 {
        let n = if normalized.is_finite() { normalized.clamp(0.0, 1.0) } else { self.to_normalized(self.default) };
        let value = if self.gain() {
            let knee = self.max - GAIN_SLOPE * (1.0 - GAIN_KNEE);
            if n <= 0.0 {
                self.min
            } else if n >= GAIN_KNEE {
                self.max - GAIN_SLOPE * (1.0 - n)
            } else {
                (knee + 20.0 * GAIN_POWER * (n / GAIN_KNEE).log10()).max(self.min)
            }
        } else if self.logarithmic() {
            self.min * (self.max / self.min).powf(n)
        } else {
            self.min + n * (self.max - self.min)
        };
        self.clamp(value)
    }
}

/// Whether a device changes the sound it gets, makes its own from notes, or changes the notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Processes the signal in the buffer.
    Effect,
    /// Plays the notes it gets through [`Device::event`] and writes its output over the
    /// buffer, whatever was in it.
    Instrument,
    /// Changes the notes on their way to the instrument through [`Device::process_events`];
    /// the sound is not its business, and the host never asks it to process a buffer.
    MidiEffect,
}

/// What a device is: its stable id (the model's `DeviceKind::Builtin` id), the name the user
/// sees, whether it is an effect or an instrument, and its parameters in a fixed order. A
/// parameter's index in `params` is how the host addresses it while playing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DeviceInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub role: Role,
    pub params: &'static [ParamInfo],
}

impl DeviceInfo {
    /// The index of the parameter with this id.
    pub fn param_index(&self, id: &str) -> Option<usize> {
        self.params.iter().position(|p| p.id == id)
    }
}

/// The transport as one block starts. Devices that follow the tempo (an LFO synced to beats)
/// read it; the rest ignore it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessContext {
    /// Beats per minute.
    pub tempo: f64,
    /// Song position of the block's first frame, in beats.
    pub beat: f64,
    /// Whether the transport is running.
    pub playing: bool,
}

impl Default for ProcessContext {
    fn default() -> Self {
        Self { tempo: 120.0, beat: 0.0, playing: false }
    }
}

impl ProcessContext {
    /// The tempo a device can follow: [`tempo`](Self::tempo) limited to 1..=1000 BPM, or
    /// [`DEFAULT_TEMPO`] when the host gives none that makes sense (not finite, or not above 0).
    pub fn tempo_or_default(&self) -> f64 {
        if self.tempo.is_finite() && self.tempo > 0.0 { self.tempo.clamp(1.0, 1_000.0) } else { DEFAULT_TEMPO }
    }
}

/// The tempo [`ProcessContext::tempo_or_default`] falls back on, in beats per minute.
pub const DEFAULT_TEMPO: f64 = 120.0;

/// A note or a performance control for an instrument. Values are plain and in range for a
/// well-behaved host, but a device must accept any value without misbehaving: keys above 127
/// are ignored, numbers that are not finite are ignored, and the rest are limited to their
/// range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NoteEvent {
    /// A key goes down. `key` is a MIDI key (60 is middle C); `velocity` runs from 0.0 to 1.0,
    /// and 0.0 counts as a release.
    On { key: u8, velocity: f32 },
    /// A key comes up; `velocity` (0.0 to 1.0) is how fast.
    Off { key: u8, velocity: f32 },
    /// A MIDI controller moved: `number` 0 to 127 (1 is the modulation wheel, 64 the sustain
    /// pedal), `value` from 0.0 to 1.0.
    Controller { number: u8, value: f32 },
    /// The pitch bend wheel, from -1.0 (full down) through 0.0 to 1.0 (full up).
    PitchBend(f32),
    /// Channel pressure (aftertouch), from 0.0 to 1.0.
    Pressure(f32),
    /// Every held note is released, as if each key came up and the sustain pedal was lifted.
    AllNotesOff,
    /// The scale of the notes that follow: the host sends it before the first note of a clip
    /// whose scale differs from the last one sent, `None` for a clip without a scale. MIDI
    /// effects that follow the scale keep it and pass it on; everything else ignores it.
    Scale(Option<MusicalScale>),
}

/// The scale the notes a track plays are written in: a root and the pitch classes it holds.
/// Always valid: the root is 0 (C) to 11 (B) and the scale holds at least one pitch class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MusicalScale {
    root: u8,
    /// Bit `n` is set when pitch class `n` (0 is C) is in the scale.
    classes: u16,
}

impl MusicalScale {
    /// The scale over `root` (0 to 11) holding the pitch classes `intervals` semitones above
    /// it; `None` for a root over 11, an interval over 11, or no intervals.
    pub fn new(root: u8, intervals: &[u8]) -> Option<Self> {
        if root > 11 || intervals.is_empty() {
            return None;
        }
        let mut classes = 0u16;
        for &interval in intervals {
            if interval > 11 {
                return None;
            }
            classes |= 1 << ((root + interval) % 12);
        }
        Some(Self { root, classes })
    }

    /// The root pitch class, 0 (C) to 11 (B).
    pub fn root(self) -> u8 {
        self.root
    }

    /// Whether `key` (any whole number of semitones, 60 being C3) is in the scale.
    pub fn contains(self, key: i32) -> bool {
        self.classes & (1 << key.rem_euclid(12)) != 0
    }

    /// The key nearest to `key` that the scale holds; a tie goes to the lower key.
    pub fn nearest(self, key: i32) -> i32 {
        for distance in 0..=6 {
            if self.contains(key - distance) {
                return key - distance;
            }
            if self.contains(key + distance) {
                return key + distance;
            }
        }
        key
    }

    /// The key `degrees` steps along the scale from `key`, counted from the scale's key nearest
    /// to it ([`nearest`](Self::nearest)): in C major one step up from C is D, from B is the C
    /// above, and one step down from C is the B below. Steps beyond a thousand either way count
    /// as a thousand.
    pub fn step(self, key: i32, degrees: i32) -> i32 {
        let mut key = self.nearest(key);
        let direction = degrees.signum();
        for _ in 0..degrees.unsigned_abs().min(1_000) {
            key += direction;
            // At most eleven semitones to the next key of the scale, which holds one at least.
            for _ in 0..11 {
                if self.contains(key) {
                    break;
                }
                key += direction;
            }
        }
        key
    }
}

/// MIDI controller numbers instruments commonly read from [`NoteEvent::Controller`].
pub mod controller {
    pub const MODULATION: u8 = 1;
    pub const BREATH: u8 = 2;
    pub const FOOT: u8 = 4;
    pub const PORTAMENTO_TIME: u8 = 5;
    pub const SUSTAIN: u8 = 64;
    pub const PORTAMENTO: u8 = 65;
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeviceError {
    InvalidSampleRate(f64),
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSampleRate(sr) => write!(f, "sample rate {sr} is not supported"),
        }
    }
}

impl std::error::Error for DeviceError {}

/// The sample rate a device is prepared for when it is made, before the host calls
/// [`Device::prepare`].
pub const DEFAULT_SAMPLE_RATE: f64 = 48_000.0;

/// Lowest and highest sample rates a device must accept in [`Device::prepare`].
pub const SAMPLE_RATE_RANGE: (f64, f64) = (8_000.0, 384_000.0);

/// Checks a sample rate for [`Device::prepare`].
pub fn check_sample_rate(sample_rate: f64) -> Result<f64, DeviceError> {
    if sample_rate.is_finite() && (SAMPLE_RATE_RANGE.0..=SAMPLE_RATE_RANGE.1).contains(&sample_rate) {
        Ok(sample_rate)
    } else {
        Err(DeviceError::InvalidSampleRate(sample_rate))
    }
}

/// A device's parameter values, in [`DeviceInfo::params`] order: the plumbing every device's
/// [`Device::set_param`] and [`Device::param`] share. A value is always valid for its parameter
/// ([`ParamInfo::clamp`]), and an index past the end is ignored. Real-time safe.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamBank<const N: usize> {
    params: &'static [ParamInfo; N],
    values: [f64; N],
}

impl<const N: usize> ParamBank<N> {
    /// Every parameter at its default.
    pub fn new(params: &'static [ParamInfo; N]) -> Self {
        Self { params, values: params.each_ref().map(|p| p.default) }
    }

    /// The value of parameter `index`, or 0.0 for an index past the end.
    pub fn get(&self, index: usize) -> f64 {
        self.values.get(index).copied().unwrap_or(0.0)
    }

    /// Whether parameter `index` (a toggle) is on: its value is at least 0.5.
    pub fn on(&self, index: usize) -> bool {
        self.get(index) >= 0.5
    }

    /// Sets parameter `index` to `value` made valid for it, and returns what was stored; `None`
    /// (and nothing changes) for an index past the end. What [`Device::set_param`] calls before
    /// it reacts to the change.
    pub fn set(&mut self, index: usize, value: f64) -> Option<f64> {
        let info = self.params.get(index)?;
        let slot = self.values.get_mut(index)?;
        *slot = info.clamp(value);
        Some(*slot)
    }

    /// The value of parameter `index`, `None` past the end: what [`Device::param`] returns.
    pub fn param(&self, index: usize) -> Option<f64> {
        self.values.get(index).copied()
    }

    /// Every value, in parameter order.
    pub fn values(&self) -> &[f64; N] {
        &self.values
    }
}

/// A built-in instrument or effect.
///
/// `new`-style constructors, [`prepare`](Self::prepare) and dropping run off the audio thread
/// and may allocate. [`reset`](Self::reset), [`set_param`](Self::set_param),
/// [`param`](Self::param) and [`process`](Self::process) run on the audio thread: they never
/// allocate, free, lock, block, log or do I/O, and never panic, whatever values they get.
/// Most live values a device hands its panel ([`Device::readouts`]).
pub const MAX_READOUTS: usize = 32;

pub trait Device: Send {
    /// Current normalized sample read position, when a sample voice is active.
    fn sample_position(&self) -> Option<f32> {
        None
    }
    /// Independent sample read heads; inactive voices are None. No allocation.
    fn sample_positions(&self) -> [Option<f32>; 32] {
        let mut positions = [None; 32];
        positions[0] = self.sample_position();
        positions
    }

    /// Live values for the device's panel (a band's level, its gain change), read after each
    /// block: up to [`MAX_READOUTS`], in an order the device's panel knows. `None` for a device
    /// that shows none. Runs on the audio thread: no allocation.
    fn readouts(&self) -> Option<[f32; MAX_READOUTS]> {
        None
    }

    fn info(&self) -> &'static DeviceInfo;

    /// Refresh block context before MIDI events, including events at frame zero. Never allocates.
    fn set_context(&mut self, context: &ProcessContext) {
        let _ = context;
    }

    /// Share host-owned decoded PCM without copying it. Called off the audio thread.
    fn load_shared_sample(
        &mut self,
        sample_rate: u32,
        channels: u16,
        samples: std::sync::Arc<dyn AsRef<[f32]> + Send + Sync>,
    ) {
        let _ = (sample_rate, channels, samples);
    }

    /// Installs decoded source audio off the audio thread. Replacing or dropping the
    /// source may free storage; hosts must rebuild the device graph for replacement.
    fn load_sample(&mut self, sample_rate: u32, channels: u16, samples: std::sync::Arc<[f32]>) {
        let _ = (sample_rate, channels, samples);
    }

    /// Installs an immutable manual sample map off the audio thread. None uses legacy parameters.
    fn load_sample_slices(&mut self, slices: Option<std::sync::Arc<[f64]>>) {
        let _ = slices;
    }

    /// Exchanges a validated, sorted sample map during graph adoption, without freeing storage.
    fn swap_sample_slices(&mut self, slices: &mut Option<std::sync::Arc<[f64]>>) {
        let _ = slices;
    }

    /// Install source corrections off the audio thread.
    fn load_sample_edits(&mut self, edits: Option<std::sync::Arc<sample_edits::SampleEdits>>) {
        let _ = edits;
    }

    /// Exchange prepared corrections during adoption; never release their last owner here.
    fn swap_sample_edits(&mut self, edits: &mut Option<std::sync::Arc<sample_edits::SampleEdits>>) {
        let _ = edits;
    }

    /// Gets ready to run at `sample_rate`, allocating whatever that needs, and clears all state.
    /// A device is usable right after construction, prepared for 48 kHz.
    fn prepare(&mut self, sample_rate: f64) -> Result<(), DeviceError>;

    /// Clears all signal state (filter memories, delay lines, envelopes) as if the device had
    /// just been prepared, and jumps smoothed parameters to their values. Parameters keep their
    /// values.
    fn reset(&mut self);

    /// Sets parameter `index` to `value`, after [`ParamInfo::clamp`]. Unknown indices are
    /// ignored. A device smooths changes that would click; the host splits a block where an
    /// automation point falls, so a change lands on its frame.
    fn set_param(&mut self, index: usize, value: f64);

    /// The current (target) value of parameter `index`.
    fn param(&self, index: usize) -> Option<f64>;

    /// Processes `buffer`, interleaved stereo (`buffer.len() / 2` frames), in place. An
    /// instrument writes its output over the buffer. A trailing odd sample is left as it is. The
    /// output must not depend on how the host divides a stream into blocks.
    fn process(&mut self, context: &ProcessContext, buffer: &mut [f32]);

    /// Takes a note or a performance control, at the frame where the next [`Device::process`]
    /// call starts: the host splits a block where an event falls, as it does
    /// for automation. Effects ignore events; [`reset`](Self::reset) silences every note.
    fn event(&mut self, event: NoteEvent) {
        let _ = event;
    }

    /// A MIDI effect's work on a span of `frames` frames: `input` holds the events that reach
    /// it, in frame order, at frames counted from the span's start; the device writes the events
    /// it sends on into `output`, in frame order and before `frames`, and returns how many it
    /// wrote. It writes no more than `output.len()`: at least `input.len()`, and twice that
    /// while the block has space. With more to send than fits, it leaves out note-ons (and
    /// then their note-offs) before any note-off of a note it sent, keeping room for the
    /// note-offs and all-notes-off still in `input`, so no note it started is left sounding.
    /// The host splits a span where a parameter changes, so a change lands
    /// between the events either side of it; the result must not depend on how the stream is
    /// divided otherwise. [`reset`](Self::reset) forgets the notes it holds. Effects and
    /// instruments are never asked; by default the events pass unchanged.
    fn process_events(
        &mut self,
        context: &ProcessContext,
        frames: usize,
        input: &[(usize, NoteEvent)],
        output: &mut [(usize, NoteEvent)],
    ) -> usize {
        let _ = (context, frames);
        let written = input.len().min(output.len());
        if let (Some(to), Some(from)) = (output.get_mut(..written), input.get(..written)) {
            to.copy_from_slice(from);
        }
        written
    }

    /// Delay the device adds, in frames, for the host to compensate.
    fn latency(&self) -> u32 {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FREQ: ParamInfo =
        ParamInfo::continuous("freq", "Frequency", (20.0, 20_000.0), 1_000.0, Unit::Hertz, Scale::Logarithmic);
    const GAIN: ParamInfo = ParamInfo::continuous("gain", "Gain", (-12.0, 12.0), 0.0, Unit::Decibels, Scale::Linear);
    const MODE: ParamInfo = ParamInfo::choice("mode", "Mode", &["A", "B", "C"], 1);
    const ON: ParamInfo = ParamInfo::toggle("on", "On", true);
    const LEVEL: ParamInfo = ParamInfo::integer("level", "Level", (0, 99), 99, Unit::None);

    #[test]
    fn clamps_values_to_the_parameter() {
        assert_eq!(GAIN.clamp(30.0), 12.0);
        assert_eq!(GAIN.clamp(f64::NAN), 0.0);
        assert_eq!(GAIN.clamp(f64::NEG_INFINITY), 0.0);
        assert_eq!(MODE.clamp(1.6), 2.0);
        assert_eq!(MODE.clamp(-3.0), 0.0);
        assert_eq!(ON.clamp(0.4), 0.0);
        assert_eq!(ON.default, 1.0);
        assert_eq!(LEVEL.clamp(41.6), 42.0);
        assert_eq!(LEVEL.clamp(120.0), 99.0);
        assert_eq!(LEVEL.from_normalized(0.5), 50.0);
    }

    #[test]
    fn an_integer_sits_unrounded_and_plays_cut_toward_zero() {
        // A lane can hold 40.5: it sits half-way between 40 and 41, not at 41.
        assert_eq!(LEVEL.to_normalized(49.5), 0.5);
        assert_eq!(LEVEL.to_normalized(120.0), 1.0);
        assert_eq!(LEVEL.played(36.98), 36.0);
        assert_eq!(LEVEL.played(40.5), 40.0);
        assert_eq!(LEVEL.played(40.999_999_999_99), 41.0);
        assert_eq!(LEVEL.played(150.0), 99.0);
        assert_eq!(LEVEL.played(f64::NAN), 99.0);
        let transpose = ParamInfo::integer("t", "T", (-48, 48), 0, Unit::Semitones);
        assert_eq!(transpose.played(0.94), 0.0);
        assert!(transpose.played(-0.94) == 0.0 && transpose.played(-0.94).is_sign_positive());
        assert_eq!(transpose.played(-2.97), -2.0);
        // Other kinds play as they are kept.
        assert_eq!(MODE.played(1.6), 2.0);
        assert_eq!(GAIN.played(3.25), 3.25);
    }

    #[test]
    fn normalises_linear_and_logarithmic_ranges() {
        assert_eq!(GAIN.to_normalized(0.0), 0.5);
        assert_eq!(GAIN.from_normalized(1.0), 12.0);
        // 20 Hz to 20 kHz is three decades, so the middle of the knob is 632 Hz.
        assert!((FREQ.from_normalized(0.5) - 632.455).abs() < 0.01);
        assert!((FREQ.to_normalized(200.0) - 1.0 / 3.0).abs() < 1e-9);
        for n in [0.0, 0.1, 0.37, 0.9, 1.0] {
            assert!((FREQ.to_normalized(FREQ.from_normalized(n)) - n).abs() < 1e-9);
        }
        assert_eq!(MODE.from_normalized(0.8), 2.0);
        assert_eq!(GAIN.from_normalized(f64::NAN), 0.0);
        assert_eq!(GAIN.to_normalized(f64::NAN), 0.5);
    }

    #[test]
    fn a_logarithmic_scale_over_a_non_positive_range_is_linear() {
        let odd = ParamInfo::continuous("x", "X", (-1.0, 1.0), 0.0, Unit::None, Scale::Logarithmic);
        assert_eq!(odd.from_normalized(0.75), 0.5);
        let empty = ParamInfo::choice("x", "X", &[], 0);
        assert_eq!(empty.to_normalized(0.0), 0.0);
        assert_eq!(empty.from_normalized(0.5), 0.0);
    }

    #[test]
    fn a_gain_scale_over_a_range_that_does_not_reach_below_its_knee_is_linear() {
        let narrow = ParamInfo::continuous("x", "X", (-12.0, 12.0), 0.0, Unit::Decibels, Scale::Gain);
        assert_eq!(narrow.from_normalized(0.5), 0.0);
        assert_eq!(narrow.to_normalized(-12.0), 0.0);
        assert_eq!(narrow.to_normalized(0.0), 0.5);
    }

    /// A scale knows its keys in every octave, snaps a key outside it to the nearest one (a tie going
    /// down) and steps by degrees, whatever its root.
    #[test]
    fn a_scale_snaps_and_steps_along_its_keys() {
        let major = MusicalScale::new(0, &[0, 2, 4, 5, 7, 9, 11]).unwrap();
        assert!(major.contains(60) && !major.contains(61) && major.contains(-1));
        // C# and D# go down (ties), F# down to F.
        assert_eq!([61, 63, 66, 68, 70].map(|k| major.nearest(k)), [60, 62, 65, 67, 69]);
        assert_eq!([60, 64, 71].map(|k| major.step(k, 1)), [62, 65, 72]);
        assert_eq!(major.step(60, -1), 59);
        assert_eq!(major.step(60, 7), 72);
        assert_eq!(major.step(61, 0), 60);
        // A scale's root does not change its keys.
        let dorian = MusicalScale::new(2, &[0, 2, 3, 5, 7, 9, 10]).unwrap();
        assert!((0..128).all(|k| dorian.step(k, 3) == major.step(k, 3)));
        assert_eq!(dorian.root(), 2);
        // Five keys to the octave; E (a tie between E flat and F) goes down.
        let pentatonic = MusicalScale::new(0, &[0, 3, 5, 7, 10]).unwrap();
        assert_eq!(pentatonic.step(60, 5), 72);
        assert_eq!([60, 67, 70, 62, 64].map(|k| pentatonic.step(k, 1)), [63, 70, 72, 65, 65]);
        // One key only, and steps far past any keyboard.
        let one = MusicalScale::new(5, &[0]).unwrap();
        assert_eq!(one.step(60, 2), 89);
        assert_eq!(one.step(0, i32::MIN), 5 - 12 * 1_000);
        assert_eq!(major.step(0, i32::MAX), 12 * (1_000 / 7) + 11);
        for bad in [MusicalScale::new(12, &[0]), MusicalScale::new(0, &[]), MusicalScale::new(0, &[12])] {
            assert!(bad.is_none());
        }
    }

    #[test]
    fn a_bank_keeps_valid_values_and_ignores_unknown_indices() {
        static PARAMS: [ParamInfo; 3] = [GAIN, MODE, ON];
        let mut bank = ParamBank::new(&PARAMS);
        assert_eq!(bank.values(), &[0.0, 1.0, 1.0]);
        assert!(bank.on(2) && !bank.on(0));
        assert_eq!(bank.set(0, 30.0), Some(12.0));
        assert_eq!(bank.set(1, f64::NAN), Some(1.0));
        assert_eq!(bank.set(2, 0.2), Some(0.0));
        assert_eq!(bank.set(3, 1.0), None);
        assert_eq!(bank.set(usize::MAX, 1.0), None);
        assert_eq!((bank.param(0), bank.param(3)), (Some(12.0), None));
        assert_eq!((bank.get(1), bank.get(9)), (1.0, 0.0));
    }

    #[test]
    fn a_tempo_that_makes_no_sense_falls_back() {
        let at = |tempo| ProcessContext { tempo, ..ProcessContext::default() }.tempo_or_default();
        assert_eq!([at(90.0), at(0.5), at(5_000.0)], [90.0, 1.0, 1_000.0]);
        for bad in [0.0, -10.0, f64::NAN, f64::INFINITY] {
            assert_eq!(at(bad), DEFAULT_TEMPO);
        }
    }

    #[test]
    fn checks_sample_rates() {
        assert_eq!(check_sample_rate(44_100.0), Ok(44_100.0));
        for bad in [0.0, -48_000.0, f64::NAN, f64::INFINITY, 1e9] {
            assert!(check_sample_rate(bad).is_err());
        }
    }
}
