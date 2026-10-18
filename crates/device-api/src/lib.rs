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
//! between blocks, splitting a block where an event falls, as it does for automation.
//!
//! This crate depends on nothing in Suhina, so devices can be built, tested and later shipped
//! as plug-ins without the engine.

pub mod conformance;

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
}

/// How a continuous parameter maps onto a knob's travel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    Linear,
    /// Equal steps of travel multiply the value by the same factor (frequencies). Needs a
    /// positive range; otherwise the parameter behaves as linear.
    Logarithmic,
}

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
        Self { id, name, kind: ParamKind::Continuous(scale), min: range.0, max: range.1, default, unit }
    }

    pub const fn toggle(id: &'static str, name: &'static str, default: bool) -> Self {
        let default = if default { 1.0 } else { 0.0 };
        Self { id, name, kind: ParamKind::Toggle, min: 0.0, max: 1.0, default, unit: Unit::None }
    }

    /// A whole number in `range`.
    pub const fn integer(id: &'static str, name: &'static str, range: (i32, i32), default: i32, unit: Unit) -> Self {
        Self {
            id,
            name,
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
        Self { id, name, kind: ParamKind::Choice(options), min: 0.0, max, default: default as f64, unit: Unit::None }
    }

    /// `value` made valid for this parameter: limited to its range, rounded to a whole number
    /// for toggles, choices and integers, and the default when it is not a finite number.
    pub fn clamp(&self, value: f64) -> f64 {
        if !value.is_finite() {
            return self.default;
        }
        let value = value.clamp(self.min, self.max);
        match self.kind {
            ParamKind::Continuous(_) => value,
            ParamKind::Toggle | ParamKind::Choice(_) | ParamKind::Integer => value.round(),
        }
    }

    fn logarithmic(&self) -> bool {
        matches!(self.kind, ParamKind::Continuous(Scale::Logarithmic)) && self.min > 0.0 && self.max > self.min
    }

    /// Where `value` sits on the knob, from 0.0 to 1.0.
    pub fn to_normalized(&self, value: f64) -> f64 {
        let value = self.clamp(value);
        let n = if self.max <= self.min {
            0.0
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
        let value = if self.logarithmic() {
            self.min * (self.max / self.min).powf(n)
        } else {
            self.min + n * (self.max - self.min)
        };
        self.clamp(value)
    }
}

/// Whether a device changes the sound it gets or makes its own from notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Processes the signal in the buffer.
    Effect,
    /// Plays the notes it gets through [`Device::event`] and writes its output over the
    /// buffer, whatever was in it.
    Instrument,
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

/// A built-in instrument or effect.
///
/// `new`-style constructors, [`prepare`](Self::prepare) and dropping run off the audio thread
/// and may allocate. [`reset`](Self::reset), [`set_param`](Self::set_param),
/// [`param`](Self::param) and [`process`](Self::process) run on the audio thread: they never
/// allocate, free, lock, block, log or do I/O, and never panic, whatever values they get.
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

    /// Takes a note or a performance control, at the frame where the next [`process`]
    /// (Self::process) call starts: the host splits a block where an event falls, as it does
    /// for automation. Effects ignore events; [`reset`](Self::reset) silences every note.
    fn event(&mut self, event: NoteEvent) {
        let _ = event;
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
    fn checks_sample_rates() {
        assert_eq!(check_sample_rate(44_100.0), Ok(44_100.0));
        for bad in [0.0, -48_000.0, f64::NAN, f64::INFINITY, 1e9] {
            assert!(check_sample_rate(bad).is_err());
        }
    }
}
