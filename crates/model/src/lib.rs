// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Suhina project model: the project as pure data, with no audio or UI dependencies.
//!
//! The model is DAW-neutral (see `docs/decisions/0001-daw-neutral-model.md`): tracks and mixer
//! channels are separate, clip contents live in a pool and are placed by clips, and data a format
//! importer cannot map is kept verbatim in [`Extensions`].

use std::collections::HashSet;
use std::fmt;

pub mod automation;
mod build;
mod groups;
mod ids;
mod tempo;

pub use tempo::{
    BarPosition, MAX_BPM, MAX_CHANGE_BEAT, MAX_TEMPO_CHANGES, MAX_TIME_SIGNATURE_CHANGES, MIN_BPM, Meter, TempoMap,
    TempoPoint, TimeSignatureChange, valid_bpm,
};

pub use groups::{
    Branch, DeviceGroup, MACROS, MAX_BRANCHES, MAX_DEVICE_GROUP_DEPTH, MAX_DEVICES, MAX_MACRO_MAPPINGS, Macro,
    MacroMapping, count_devices, device_at, device_at_mut, group_depth as device_group_depth, has_groups, walk_devices,
};
pub use ids::{ClipId, ClipPlace, IdsExhausted, LaneId, SceneId};

mod launch;
mod mapping;
mod overlap;
mod scale;
pub use launch::{LaunchMode, LaunchQuantization, LaunchSettings};
pub use mapping::{KEY_NAME_MAX, LaunchMapping, MappingSource, MappingTarget};
pub use overlap::{AudioOverlap, AudioOverlaps, MAX_AUDIO_OVERLAPS, OverlapClip, TOUCH_TOLERANCE};
pub use scale::{ROOT_NAMES, Scale, ScaleMode};

/// Identifies a [`Track`] within a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TrackId(pub u32);

/// Identifies a [`Channel`] within a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChannelId(pub u32);

/// Identifies a [`ClipContent`] within a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContentId(pub u32);

/// How deep groups nest: a track is inside at most this many groups (its parent, the parent's
/// parent and so on). Deep enough for any real project, small enough that walking up a track's
/// groups is always short.
pub const MAX_GROUP_DEPTH: usize = 16;

/// A Suhina project.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Project {
    pub transport: Transport,
    pub tracks: Vec<Track>,
    pub channels: Vec<Channel>,
    /// Shared pool of clip contents; clips refer to these by id.
    pub contents: Vec<ClipContent>,
    pub arrangement: Arrangement,
    pub launcher: Launcher,
    pub extensions: Extensions,
    /// The scale a new MIDI clip takes, if any; clips made before it was set keep theirs
    /// ([ADR 0001](../../docs/decisions/0001-daw-neutral-model.md) › Amendment: clip scales).
    pub default_scale: Option<Scale>,
    /// The next id a clip, an arrangement lane or a scene gets: the three kinds share this
    /// counter, and a value is never handed out twice, deleted objects' included
    /// ([ADR 0025](../../docs/decisions/0025-object-identity.md)). Allocate with
    /// [`Project::new_clip_id`] and its neighbours; 0 means nothing was handed out yet.
    pub next_id: u32,
}

/// The song's tempo and time signature: `tempo_bpm` and `time_signature` hold from the start,
/// and the changes take over later in the song (`docs/spec/tempo-map.md`). Convert beats to time
/// and bars with [`Transport::tempo_map`] and [`Transport::meter`], never with `tempo_bpm` alone.
#[derive(Debug, Clone, PartialEq)]
pub struct Transport {
    pub tempo_bpm: f64,
    pub time_signature: TimeSignature,
    /// The arrangement's loop region, in beats from the start: kept while looping is off, so
    /// turning it on again repeats the same span ([`Transport::looping`]).
    pub loop_range: Option<LoopRange>,
    /// Whether playback repeats the loop region.
    pub loop_enabled: bool,
    /// Tempo changes after beat 0, in order of beat.
    pub tempo_changes: Vec<TempoPoint>,
    /// Time-signature changes after bar 0, in order of bar.
    pub time_signature_changes: Vec<TimeSignatureChange>,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            tempo_bpm: 120.0,
            time_signature: TimeSignature { numerator: 4, denominator: 4 },
            loop_range: None,
            loop_enabled: false,
            tempo_changes: Vec::new(),
            time_signature_changes: Vec::new(),
        }
    }
}

impl Transport {
    /// The span playback repeats: the loop region while looping is on and the region is valid.
    pub fn looping(&self) -> Option<LoopRange> {
        self.loop_range.filter(|r| self.loop_enabled && r.is_valid_region())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

/// What a track can play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ContentKinds {
    pub notes: bool,
    pub audio: bool,
}

/// A node in the project structure: an instrument or audio track, a group, or a return.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub color: Option<Rgb>,
    pub content: ContentKinds,
    /// The mixer strip this track plays through, if any.
    pub channel: Option<ChannelId>,
    /// The group track this track belongs to, if any. Groups nest: a group's own `parent` is
    /// the group it is inside, at most [`MAX_GROUP_DEPTH`] levels deep and never in a cycle.
    pub parent: Option<TrackId>,
    /// Which MIDI controllers the track hears while it is armed (ADR 0017).
    pub midi_input: MidiInput,
    /// Which channels of the audio input the track hears, and when (ADR 0017).
    pub audio_input: AudioInput,
    pub extensions: Extensions,
}

/// What a track hears of the audio input device (a microphone, a guitar), and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioInput {
    pub channels: InputChannels,
    pub monitor: Monitor,
}

/// The channels of the audio input device a track hears, counted from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputChannels {
    /// None: the track has no audio input.
    #[default]
    None,
    /// One channel, heard in the middle (then panned by the track).
    Mono(u16),
    /// This channel on the left and the next one on the right.
    Stereo(u16),
}

/// When a track's live input is heard through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Monitor {
    /// While the track is armed.
    #[default]
    Auto,
    /// Always.
    In,
    /// Never; the input is only recorded.
    Off,
}

/// The highest audio input channel a track can name.
pub const MAX_INPUT_CHANNEL: u16 = 256;

/// Which MIDI controllers a track records and monitors: a port, or every one, or none,
/// one channel or every one, and when the live input is heard.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MidiInput {
    pub port: MidiPort,
    /// 1 to 16, or `None` for every channel.
    pub channel: Option<u8>,
    pub monitor: Monitor,
}

/// The MIDI input port a track hears.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum MidiPort {
    /// Every port that is on.
    #[default]
    All,
    /// One port, by the name the system gives it; a port that is not there plays nothing.
    Named(String),
    /// No port: the track is not played from a controller.
    Nothing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelRole {
    #[default]
    Regular,
    Master,
    /// A return/effect channel fed by sends.
    Effect,
    Submix,
}

/// A mixer strip.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub id: ChannelId,
    pub name: String,
    pub role: ChannelRole,
    /// Linear gain; 1.0 is unity.
    pub volume: f64,
    /// -1.0 is hard left, 0.0 centre, 1.0 hard right.
    pub pan: f64,
    pub mute: bool,
    pub solo: bool,
    /// Also heard on the cue output (headphones), so it can be listened to apart from the main
    /// mix; the main mix does not change.
    pub cue: bool,
    /// Where the channel's output goes; `None` means the audio output (the master).
    pub output: Option<ChannelId>,
    pub sends: Vec<Send>,
    pub devices: Vec<Device>,
    /// The channel's mixer parameters over the arrangement: lanes for `volume` (linear gain, as
    /// `volume`), `pan` (-1 to 1) and `mute` (0 or 1).
    pub automation: Vec<AutomationLane>,
    pub extensions: Extensions,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Send {
    pub target: ChannelId,
    /// Linear gain; 1.0 is unity.
    pub level: f64,
    pub pre_fader: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeviceKind {
    /// A Suhina device, by its stable id.
    Builtin(String),
    /// A data-only effect graph, stored as `.sd` JSON in `Device::state`.
    Sd,
    Clap {
        id: String,
    },
    /// `id` is the 16-byte class id as 32 hex digits.
    Vst3 {
        id: String,
    },
    /// `id` is the plug-in's 32-bit unique id.
    Vst2 {
        id: u32,
    },
    /// A device native to another DAW that Suhina cannot run, kept as a placeholder so the
    /// chain keeps its shape. `format` names the source format (`"als"`), `id` the device type.
    Foreign {
        format: String,
        id: String,
    },
    /// Devices of Suhina's own that hold other devices in parallel branches
    /// (`docs/spec/device-groups.md`).
    Group(Box<DeviceGroup>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub name: String,
    pub kind: DeviceKind,
    pub enabled: bool,
    /// Plug-in state, handed back to the plug-in unchanged; for `DeviceKind::Sd`, the embedded
    /// validated `.sd` graph JSON. For VST2 it is the standard VST2
    /// bank (an `.fxb` file's contents), which also a VST3 version standing in for the plug-in
    /// reads; for VST3 it is the component's state.
    pub state: Option<Vec<u8>>,
    /// Parameter values set on the device. A built-in effect or instrument's
    /// (`DeviceKind::Builtin`) go by the parameter's stable id; a plug-in's by its parameter id
    /// (the number the plug-in gives it, written in decimal) in the plug-in's own range, and are
    /// set over the state it loads with. Parameters that are not listed keep their defaults (a
    /// plug-in's, what its state says).
    pub params: Vec<DeviceParam>,
    /// The device's parameters over the arrangement, one lane per parameter (by the same id as
    /// in `params`), for plug-ins and built-in effects and instruments alike.
    pub automation: Vec<AutomationLane>,
    pub extensions: Extensions,
}

/// Maximum persisted manual slice boundaries per source.
pub const MAX_SAMPLE_SLICES: usize = 4096;

impl Device {
    /// Manual source boundaries; None reads legacy fixed slice parameters.
    pub fn sample_slices(&self) -> Option<Vec<f64>> {
        let data = &self.extensions.0.iter().find(|e| e.format == "suhina" && e.key == "sample_slices")?.data;
        if !data.len().is_multiple_of(8) || data.len() > MAX_SAMPLE_SLICES * 8 {
            return None;
        }
        let mut points = Vec::with_capacity(data.len() / 8);
        for bytes in data.as_chunks::<8>().0 {
            let value = f64::from_le_bytes(*bytes);
            if !value.is_finite() || value <= 0. || value >= 1. {
                return None;
            }
            points.push(value);
        }
        points.sort_unstable_by(f64::total_cmp);
        points.dedup();
        Some(points)
    }
    pub fn set_sample_slices(&mut self, points: &[f64]) -> Result<(), &'static str> {
        if points.len() > MAX_SAMPLE_SLICES || points.iter().any(|p| !p.is_finite() || *p <= 0. || *p >= 1.) {
            return Err("slice map needs at most 4096 finite points strictly between 0 and 1");
        }
        self.extensions.0.retain(|e| !(e.format == "suhina" && e.key == "sample_slices"));
        self.extensions.0.push(Extension {
            format: "suhina".into(),
            key: "sample_slices".into(),
            data: points.iter().flat_map(|v| v.to_le_bytes()).collect(),
        });
        Ok(())
    }
    /// A sampler's source path, versioned independently of parameter values.
    pub fn sample_file(&self) -> Option<&str> {
        let bytes = if self.kind == DeviceKind::Sd {
            &self.extensions.0.iter().find(|e| e.format == "suhina" && e.key == "sample_reference")?.data
        } else if matches!(&self.kind, DeviceKind::Builtin(id) if id == "sampler") {
            self.state.as_deref()?
        } else {
            return None;
        };
        std::str::from_utf8(bytes).ok()?.strip_prefix("suhina-sample-v1\n")?.split('\0').next()
    }

    pub fn set_sample_file(&mut self, path: &str) {
        self.set_sample_reference(path, path);
    }

    pub fn sample_absolute_file(&self) -> Option<&str> {
        self.sample_file()?;
        let bytes = if self.kind == DeviceKind::Sd {
            &self.extensions.0.iter().find(|e| e.format == "suhina" && e.key == "sample_reference")?.data
        } else {
            self.state.as_deref()?
        };
        std::str::from_utf8(bytes).ok()?.split_once('\0').map(|(_, absolute)| absolute)
    }

    pub fn set_sample_reference(&mut self, file: &str, absolute: &str) {
        let data = format!("suhina-sample-v1\n{file}\0{absolute}").into_bytes();
        if self.kind == DeviceKind::Sd {
            self.extensions.0.retain(|e| !(e.format == "suhina" && e.key == "sample_reference"));
            self.extensions.0.push(Extension { format: "suhina".into(), key: "sample_reference".into(), data });
        } else {
            self.state = Some(data);
        }
    }
}

/// A parameter's value over the arrangement (an envelope): points in beat order, the value
/// moving from one to the next along the first one's curve and holding before the first and
/// after the last. Two points at the same beat make a jump. See `docs/spec/automation.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct AutomationLane {
    /// The parameter, by the id `Device::params` uses, or a mixer parameter of a channel
    /// (`volume`, `pan`, `mute`).
    pub param: String,
    pub points: Vec<AutomationPoint>,
    /// Whether the lane plays. A manual change to the parameter turns it off, so the value set
    /// by hand holds until the lane is turned on again.
    pub enabled: bool,
}

/// A value at an arrangement beat, in the parameter's own unit or range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutomationPoint {
    pub beat: f64,
    pub value: f64,
    /// The shape of the segment from this point to the next, -1 to 1: 0 is a straight line,
    /// above 0 it moves fast first and slow later, below 0 slow first (see
    /// [`automation::shape`]).
    pub curve: f64,
}

/// One parameter value of a built-in effect or instrument, by the parameter's stable id (ADR 0004). The value
/// is in the parameter's own unit (decibels, hertz, a fraction for a percentage, an option
/// index for a choice); the device limits it to its range when it loads it.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceParam {
    pub id: String,
    pub value: f64,
}

impl DeviceParam {
    pub fn new(id: impl Into<String>, value: f64) -> Self {
        Self { id: id.into(), value }
    }
}

/// Reusable musical content: the notes of a clip, a pattern shared by several tracks, an audio
/// region. How each source format maps onto it is in `docs/decisions/0001-daw-neutral-model.md`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipContent {
    pub id: ContentId,
    pub name: String,
    /// Length in beats.
    pub length: f64,
    pub lanes: Vec<ContentLane>,
    pub extensions: Extensions,
    /// The scale the notes are written in, if any: the piano roll marks its keys
    /// ([ADR 0001](../../docs/decisions/0001-daw-neutral-model.md) › Amendment: clip scales).
    pub scale: Option<Scale>,
}

/// One lane of a clip content. `target` is `None` when the lane plays on whichever track the
/// clip is placed on (a clip that belongs to one track); it names a track when one content drives
/// several tracks (a shared pattern).
#[derive(Debug, Clone, PartialEq)]
pub struct ContentLane {
    pub target: Option<TrackId>,
    pub data: LaneData,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LaneData {
    Notes(Vec<Note>),
    Audio(AudioRegion),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Note {
    /// Start in beats from the start of the content.
    pub time: f64,
    /// Length in beats.
    pub duration: f64,
    /// MIDI key, 0–127.
    pub key: u8,
    /// 0.0–1.0.
    pub velocity: f32,
    /// 0.0–1.0.
    pub release_velocity: f32,
    /// MIDI channel, 0–15.
    pub channel: u8,
    /// A muted note stays in the clip but is not played: the engine emits no voice for it, as if
    /// it were not there ([ADR 0001](../../docs/decisions/0001-daw-neutral-model.md) › Amendment:
    /// muted notes).
    pub muted: bool,
}

/// An audio file mapped onto musical time.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioRegion {
    /// Path relative to the project, as stored in the source file.
    pub file: String,
    /// Absolute path as stored in the source file, used when the relative path does not resolve.
    pub absolute_file: Option<String>,
    /// Sample rate of the file, if the source file records it.
    pub sample_rate: Option<u32>,
    /// Channel count of the file, if the source file records it.
    pub channels: Option<u16>,
    /// Length of the file in seconds, if known.
    pub duration_seconds: Option<f64>,
    /// When false the audio plays at its original speed and `warp_markers` are kept but unused.
    pub warped: bool,
    /// How a warped region keeps its pitch when it plays faster or slower (ADR 0003).
    pub stretch: StretchMode,
    /// Empty when the audio plays at its original speed from the content start.
    pub warp_markers: Vec<WarpMarker>,
    /// Linear gain; 1.0 is unity.
    pub gain: f64,
    /// Pitch shift in semitones (fractions are cents / 100).
    pub transpose: f64,
    /// Which audio track of a file with several plays: in a Stems file 0 is the master and 1, 2,
    /// and so on the stems. `None` for an ordinary file (its first audio track).
    pub stream: Option<u16>,
}

impl AudioRegion {
    /// The key the region's decoded audio has in the engine's audio pool: the file, and for one
    /// audio track of several, `#<stream>` after it. Always derived, never stored, so it follows
    /// the file when saving rewrites its path.
    pub fn pool_key(&self) -> std::borrow::Cow<'_, str> {
        match self.stream {
            None => std::borrow::Cow::Borrowed(&self.file),
            Some(stream) => std::borrow::Cow::Owned(stream_key(&self.file, stream)),
        }
    }
}

/// The audio pool key of one audio track of a file with several (see [`AudioRegion::pool_key`]).
pub fn stream_key(file: &str, stream: u16) -> String {
    format!("{file}#{stream}")
}

/// The time-stretch algorithm for a warped audio region (ADR 0003). The names describe intent;
/// which backend implements each one is decided where audio is prepared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StretchMode {
    /// No stretching: speed and pitch change together.
    Repitch,
    /// Drums and other material where transients matter.
    Rhythmic,
    /// Monophonic, pitched material.
    Tonal,
    /// Pads and noisy material without clear pitch or rhythm.
    Texture,
    /// General-purpose, full-spectrum stretching.
    #[default]
    Smooth,
    /// Best quality for full mixes and large tempo changes, at a higher cost.
    HighQuality,
}

/// Pins a position in the audio file to a position in the content.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WarpMarker {
    /// Beats from the start of the content.
    pub time: f64,
    /// Seconds into the audio file.
    pub audio_time: f64,
}

/// Places a clip content on the arrangement or in a launcher slot.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// Which clip this is; [`ClipId::NONE`] until the project gives it one
    /// ([`Project::assign_ids`]). A copy of a clip is a new clip and gets a new id.
    pub id: ClipId,
    pub content: ContentId,
    /// Start in beats on the arrangement; 0.0 in a launcher slot.
    pub start: f64,
    /// Length in beats.
    pub length: f64,
    /// Where playback starts inside the content, in beats.
    pub offset: f64,
    pub loop_range: Option<LoopRange>,
    pub enabled: bool,
    /// Fade-in and fade-out of an audio clip, on the arrangement or in a launcher slot, in beats
    /// from the clip's start and to its end; 0.0 is no fade. Both are at least 0 and together at
    /// most `length` ([`Clip::fades_valid`]). Notes ignore them.
    pub fade_in: f64,
    pub fade_out: f64,
    /// How the fade-in and the fade-out rise: a shape, bent by a curve. Linear and unbent by
    /// default.
    pub fade_in_curve: FadeCurve,
    pub fade_out_curve: FadeCurve,
    /// What a launched clip does after it has played for a while; arrangement clips ignore it.
    pub follow_action: FollowAction,
    /// How the clip launches from a launcher slot: mode, legato and its own quantisation.
    pub launch: LaunchSettings,
    /// The clip's own colour; `None` shows its track's, so a new clip takes the track's colour
    /// and follows it. A clip moved to another track keeps the colour it had.
    pub color: Option<Rgb>,
    pub extensions: Extensions,
}

impl Clip {
    /// Whether the fades are finite, not negative and together no longer than the clip, and
    /// their curves within −1 … +1.
    pub fn fades_valid(&self) -> bool {
        let fades = [self.fade_in, self.fade_out];
        fades.iter().all(|f| f.is_finite() && *f >= 0.0)
            && self.fade_in + self.fade_out <= self.length + FADE_EPSILON
            && self.fade_in_curve.is_valid()
            && self.fade_out_curve.is_valid()
    }

    /// Shortens the fades so they fit the clip, as when the clip itself is shortened. See
    /// [`Clip::fitted_fades`].
    pub fn fit_fades(&mut self) {
        (self.fade_in, self.fade_out) = self.fitted_fades();
    }

    /// The fades made to fit the clip: a fade that is negative or not finite is 0, and when
    /// together they are longer than the clip, both shrink in proportion.
    pub fn fitted_fades(&self) -> (f64, f64) {
        let length = if self.length.is_finite() { self.length.max(0.0) } else { 0.0 };
        let fit = |f: f64| if f.is_finite() { f.max(0.0) } else { 0.0 };
        let (fade_in, fade_out) = (fit(self.fade_in), fit(self.fade_out));
        let total = fade_in + fade_out;
        let scale = if total > length && total > 0.0 { length / total } else { 1.0 };
        (fade_in * scale, fade_out * scale)
    }
}

/// How much longer than the clip its fades may be together, for rounding in beat arithmetic.
const FADE_EPSILON: f64 = 1e-9;

/// The shape of a fade's gain over its length (`docs/spec/arrangement-editing.md` › Fades).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FadeShape {
    /// `t`: a straight line in amplitude.
    #[default]
    Linear,
    /// `sin(π t / 2)`: −3 dB half-way, so a fade-out and a fade-in over each other keep the power.
    EqualPower,
    /// `1 − (1 − t)²`: rises quickly, then levels off.
    Fast,
    /// `t²`: stays low, then rises.
    Slow,
    /// `t² (3 − 2t)`: flat at both ends.
    SCurve,
}

impl FadeShape {
    pub const ALL: [FadeShape; 5] = [Self::Linear, Self::EqualPower, Self::Fast, Self::Slow, Self::SCurve];

    /// The name used in project files and commands.
    pub fn name(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::EqualPower => "equal_power",
            Self::Fast => "fast",
            Self::Slow => "slow",
            Self::SCurve => "s_curve",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name() == name)
    }

    /// The gain at `t` raised to `exponent` (a curve's [`FadeCurve::exponent`]), 0 … 1. Never
    /// allocates; the audio thread calls it per frame inside a fade, with the exponent worked
    /// out beforehand.
    pub fn gain_bent(self, t: f64, exponent: f64) -> f64 {
        let shaped = self.gain(t);
        if exponent == 1.0 { shaped } else { shaped.powf(exponent) }
    }

    /// The gain at `t` (0 at the silent end of the fade, 1 at full level), 0 … 1.
    pub fn gain(self, t: f64) -> f64 {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
        match self {
            Self::Linear => t,
            Self::EqualPower => (t * std::f64::consts::FRAC_PI_2).sin(),
            Self::Fast => 1.0 - (1.0 - t) * (1.0 - t),
            Self::Slow => t * t,
            Self::SCurve => t * t * (3.0 - 2.0 * t),
        }
    }
}

/// How one fade rises: a shape, bent by `curve` (−1 … +1, 0 the shape exactly). The gain is
/// `shape(t)^(4^(−curve))`: + starts faster, − slower.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FadeCurve {
    pub shape: FadeShape,
    pub curve: f64,
}

impl FadeCurve {
    /// Whether the curve is a number within −1 … +1.
    pub fn is_valid(&self) -> bool {
        (-1.0..=1.0).contains(&self.curve)
    }

    /// The power the shape is raised to, `4^(−curve)`: 1 at curve 0 (and for a curve that is not
    /// a number), ¼ … 4 otherwise. Worked out once per clip, not per frame.
    pub fn exponent(&self) -> f64 {
        if self.curve == 0.0 || !self.curve.is_finite() { 1.0 } else { 4f64.powf(-self.curve.clamp(-1.0, 1.0)) }
    }

    /// The gain at `t` (0 at the silent end of the fade, 1 at full level), 0 … 1.
    pub fn gain(&self, t: f64) -> f64 {
        self.shape.gain_bent(t, self.exponent())
    }

    /// The curve that puts the gain half-way through a fade of `shape` at `gain`: what a curve
    /// handle dragged to that height sets. Within −1 … +1; 0 when `gain` is not a number.
    pub fn through(shape: FadeShape, gain: f64) -> f64 {
        let half = shape.gain(0.5);
        if !gain.is_finite() {
            return 0.0;
        }
        if gain <= 0.0 {
            return -1.0;
        }
        if gain >= 1.0 {
            return 1.0;
        }
        // gain = half^(4^(−curve)), so 4^(−curve) = ln gain / ln half.
        let curve = -((gain.ln() / half.ln()).ln() / 4f64.ln());
        if curve.is_finite() { curve.clamp(-1.0, 1.0) } else { 0.0 }
    }
}

/// Loop start and end inside the content, in beats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopRange {
    pub start: f64,
    pub end: f64,
}

impl LoopRange {
    /// Whether it can be the arrangement's loop region: finite, starting at 0 or later and
    /// ending after it starts.
    pub fn is_valid_region(&self) -> bool {
        self.start.is_finite() && self.end.is_finite() && self.start >= 0.0 && self.end > self.start
    }
}

/// What a launched clip does after it has played for its follow time: action A or action B,
/// picked by chance with their weights (`docs/spec/clip-launching.md` › Follow actions).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FollowAction {
    pub enabled: bool,
    /// Beats from the launch to the action, when not `linked`.
    pub time: f64,
    /// The follow time is the clip's length instead of `time`.
    pub linked: bool,
    pub action_a: FollowKind,
    pub action_b: FollowKind,
    /// Weights of A and B, 0 to [`FollowAction::MAX_CHANCE`]; both 0 means no action.
    pub chance_a: u32,
    pub chance_b: u32,
}

impl FollowAction {
    pub const MAX_CHANCE: u32 = 100;
    /// The longest follow time, in beats (over 13 hours at 120 bpm).
    pub const MAX_TIME: f64 = 100_000.0;

    /// Whether the time is finite and within 0 (exclusive) … [`FollowAction::MAX_TIME`], the
    /// chances at most [`FollowAction::MAX_CHANCE`].
    pub fn is_valid(&self) -> bool {
        self.time.is_finite()
            && self.time > 0.0
            && self.time <= Self::MAX_TIME
            && self.chance_a <= Self::MAX_CHANCE
            && self.chance_b <= Self::MAX_CHANCE
    }
}

impl Default for FollowAction {
    /// Off; when turned on, the next clip after one clip length.
    fn default() -> Self {
        Self {
            enabled: false,
            time: 4.0,
            linked: true,
            action_a: FollowKind::Next,
            action_b: FollowKind::Nothing,
            chance_a: Self::MAX_CHANCE,
            chance_b: 0,
        }
    }
}

/// One follow action. Slots are 0-based scene indices in the clip's column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowKind {
    /// The clip keeps playing.
    Nothing,
    /// The column stops.
    Stop,
    /// The clip is launched again from its start.
    Again,
    /// The clip above, or the last one at the top of the column.
    Previous,
    /// The clip below, or the first one at the bottom of the column.
    Next,
    First,
    Last,
    /// Any clip in the column, this one included.
    Any,
    /// Any other clip in the column.
    Other,
    /// The clip in this slot.
    Jump(u32),
}

impl FollowKind {
    /// The kinds without a slot, by name.
    pub const NAMED: [FollowKind; 9] = [
        Self::Nothing,
        Self::Stop,
        Self::Again,
        Self::Previous,
        Self::Next,
        Self::First,
        Self::Last,
        Self::Any,
        Self::Other,
    ];

    /// The name in files and commands: `next`, or `jump 3` for the slot of scene 3 (from 1).
    pub fn name(self) -> String {
        match self {
            Self::Nothing => "nothing".into(),
            Self::Stop => "stop".into(),
            Self::Again => "again".into(),
            Self::Previous => "previous".into(),
            Self::Next => "next".into(),
            Self::First => "first".into(),
            Self::Last => "last".into(),
            Self::Any => "any".into(),
            Self::Other => "other".into(),
            Self::Jump(slot) => format!("jump {}", u64::from(slot) + 1),
        }
    }

    /// Reads [`FollowKind::name`]; the case and spaces around are ignored.
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.trim().to_ascii_lowercase();
        if let Some(scene) = name.strip_prefix("jump") {
            let scene: u32 = scene.trim().parse().ok()?;
            return scene.checked_sub(1).map(Self::Jump);
        }
        Self::NAMED.into_iter().find(|kind| kind.name() == name)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Arrangement {
    pub lanes: Vec<ArrangementLane>,
    /// Named positions on the timeline, sorted by beat ([`Arrangement::sort_markers`]).
    pub markers: Vec<Marker>,
}

impl Arrangement {
    /// Puts the markers in order of their beats; markers on the same beat keep their order.
    pub fn sort_markers(&mut self) {
        self.markers.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    }

    /// Adds a marker after any already on its beat and returns its index.
    pub fn insert_marker(&mut self, marker: Marker) -> usize {
        let at = self.markers.partition_point(|m| m.beat <= marker.beat);
        self.markers.insert(at, marker);
        at
    }
}

/// A named position on the arrangement's timeline (a locator): playback can jump to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    /// Beats from the start, at least 0.
    pub beat: f64,
    pub name: String,
}

impl Marker {
    pub fn new(beat: f64, name: impl Into<String>) -> Self {
        Self { beat, name: name.into() }
    }

    /// Whether the beat is a finite number, not negative.
    pub fn is_valid(&self) -> bool {
        self.beat.is_finite() && self.beat >= 0.0
    }
}

/// A row of the arrangement. Bound to a track, or free (a row of a pattern playlist, whose clips
/// carry their own targets).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ArrangementLane {
    /// Which lane this is; [`LaneId::NONE`] until the project gives it one.
    pub id: LaneId,
    pub name: String,
    pub track: Option<TrackId>,
    pub clips: Vec<Clip>,
}

/// Clip launching: scenes are rows, columns belong to tracks.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Launcher {
    pub scenes: Vec<Scene>,
    pub columns: Vec<LauncherColumn>,
    /// Keys and MIDI notes that launch slots, scenes and stop buttons. A mapping whose track or
    /// scene is gone stays, unused, so an undo brings it back working.
    pub mappings: Vec<LaunchMapping>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    /// Which scene this is; [`SceneId::NONE`] until the project gives it one.
    pub id: SceneId,
    pub name: String,
    /// Tempo the scene switches to when launched, if any.
    pub tempo_bpm: Option<f64>,
    /// Time signature the scene switches to when launched, if any.
    pub time_signature: Option<TimeSignature>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LauncherColumn {
    pub track: TrackId,
    /// One slot per scene, in scene order.
    pub slots: Vec<Slot>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Slot {
    pub clip: Option<Clip>,
    /// An empty slot with a stop button stops the track when its scene launches.
    pub has_stop: bool,
}

/// Data a format importer could not map onto the core model, kept verbatim so the same format's
/// exporter can write it back.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Extensions(pub Vec<Extension>);

#[derive(Debug, Clone, PartialEq)]
pub struct Extension {
    /// The format that wrote it, such as `"als"` or `"flp"`.
    pub format: String,
    /// What it is within that format, such as an XML element name.
    pub key: String,
    pub data: Vec<u8>,
}

/// A reference in the project that does not resolve, or an id used twice.
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationError {
    DuplicateTrack(TrackId),
    DuplicateChannel(ChannelId),
    DuplicateContent(ContentId),
    MissingTrack(TrackId),
    MissingChannel(ChannelId),
    MissingContent(ContentId),
    /// Two clips have this id.
    DuplicateClip(ClipId),
    /// Two arrangement lanes have this id.
    DuplicateLane(LaneId),
    /// Two scenes have this id.
    DuplicateScene(SceneId),
    /// Objects of two kinds (a clip, a lane, a scene) have this id; the kinds share one counter,
    /// so a value names at most one object.
    SharedId(u32),
    /// An object has an id the project's counter has not handed out yet: the next new object
    /// would get it too.
    IdAhead(u32),
    /// This clip on this arrangement lane has fades that are negative, not finite, or together
    /// longer than the clip, or a fade curve outside −1 … +1.
    BadFades {
        lane: LaneId,
        clip: ClipId,
    },
    /// The arrangement marker at this index has a beat that is negative or not finite.
    BadMarker(usize),
    /// The transport's loop region is not finite, starts before 0 or does not end after it starts.
    BadLoop,
    /// Following this track's parents leads back to it.
    GroupCycle(TrackId),
    /// This track is inside more than [`MAX_GROUP_DEPTH`] groups.
    GroupTooDeep(TrackId),
    /// The track's MIDI input names an empty port or a channel outside 1 … 16.
    BadMidiInput(TrackId),
    /// The track's audio input names a channel outside 1 … 256.
    BadAudioInput(TrackId),
    /// The channel holds more than [`MAX_DEVICES`] devices, counting every device at every depth.
    TooManyDevices(ChannelId),
    /// The device group at this path on the channel has no branch or more than [`MAX_BRANCHES`].
    GroupBranches {
        channel: ChannelId,
        device: Vec<usize>,
    },
    /// The device group at this path does not have [`MACROS`] macros, or a macro has more than
    /// [`MAX_MACRO_MAPPINGS`] mappings.
    GroupMacros {
        channel: ChannelId,
        device: Vec<usize>,
    },
    /// The device group at this path is inside more than [`MAX_DEVICE_GROUP_DEPTH`] levels of groups,
    /// itself counted.
    DeviceGroupTooDeep {
        channel: ChannelId,
        device: Vec<usize>,
    },
    /// A branch of the device group at this path has a volume that is negative or not finite, or a
    /// pan outside -1 … 1.
    BadBranch {
        channel: ChannelId,
        device: Vec<usize>,
    },
    /// A macro of the device group at this path maps a device that is not in the group, or has a
    /// range outside 0 … 1.
    BadMacroMapping {
        channel: ChannelId,
        device: Vec<usize>,
    },
    /// A macro of the device group at this path maps a parameter another macro maps already.
    MappedTwice {
        channel: ChannelId,
        device: Vec<usize>,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateTrack(id) => write!(f, "track id {} is used twice", id.0),
            Self::DuplicateChannel(id) => write!(f, "channel id {} is used twice", id.0),
            Self::DuplicateContent(id) => write!(f, "clip content id {} is used twice", id.0),
            Self::MissingTrack(id) => write!(f, "track {} does not exist", id.0),
            Self::MissingChannel(id) => write!(f, "channel {} does not exist", id.0),
            Self::MissingContent(id) => write!(f, "clip content {} does not exist", id.0),
            Self::DuplicateClip(id) => write!(f, "clip id {id} is used twice"),
            Self::DuplicateLane(id) => write!(f, "arrangement lane id {id} is used twice"),
            Self::DuplicateScene(id) => write!(f, "scene id {id} is used twice"),
            Self::SharedId(id) => write!(f, "object id {id} is used by a clip, a lane or a scene at once"),
            Self::IdAhead(id) => write!(f, "object id {id} is not below the project's next id"),
            Self::GroupCycle(id) => write!(f, "track {} is inside itself through its groups", id.0),
            Self::GroupTooDeep(id) => {
                write!(f, "track {} is inside more than {MAX_GROUP_DEPTH} nested groups", id.0)
            }
            Self::BadAudioInput(id) => {
                write!(f, "track {} has an audio input on a channel outside 1 to {MAX_INPUT_CHANNEL}", id.0)
            }
            Self::BadMidiInput(id) => {
                write!(f, "track {} has a MIDI input with an empty port name or a channel outside 1 to 16", id.0)
            }
            Self::BadFades { lane, clip } => {
                write!(
                    f,
                    "clip {clip} on arrangement lane {lane} has fades that are negative or longer than the clip, \
                     or a fade curve outside -1 to +1"
                )
            }
            Self::BadMarker(index) => write!(f, "arrangement marker {index} is not at a beat from 0 on"),
            Self::BadLoop => write!(f, "the loop region does not start at a beat from 0 on and end after it starts"),
            Self::TooManyDevices(id) => write!(f, "channel {} holds more than {MAX_DEVICES} devices", id.0),
            Self::GroupBranches { channel, device } => {
                write!(
                    f,
                    "device group {} on channel {} does not have 1 to {MAX_BRANCHES} branches",
                    path(device),
                    channel.0
                )
            }
            Self::GroupMacros { channel, device } => write!(
                f,
                "device group {} on channel {} does not have {MACROS} macros of at most {MAX_MACRO_MAPPINGS} mappings",
                path(device),
                channel.0
            ),
            Self::DeviceGroupTooDeep { channel, device } => write!(
                f,
                "device group {} on channel {} is inside more than {MAX_DEVICE_GROUP_DEPTH} levels of groups",
                path(device),
                channel.0
            ),
            Self::BadBranch { channel, device } => write!(
                f,
                "device group {} on channel {} has a branch with a negative volume or a pan outside -1 to 1",
                path(device),
                channel.0
            ),
            Self::BadMacroMapping { channel, device } => write!(
                f,
                "device group {} on channel {} maps a device that is not in it, or a range outside 0 to 1",
                path(device),
                channel.0
            ),
            Self::MappedTwice { channel, device } => {
                write!(f, "device group {} on channel {} maps a parameter twice", path(device), channel.0)
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// A device path as the commands write it: `2/1/0`.
fn path(device: &[usize]) -> String {
    device.iter().map(ToString::to_string).collect::<Vec<_>>().join("/")
}

impl Project {
    /// Checks that ids are unique and every reference resolves. Importers call this so a
    /// malformed file becomes an error instead of a crash later.
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();

        let tracks = unique(self.tracks.iter().map(|t| t.id), ValidationError::DuplicateTrack, &mut errors);
        let channels = unique(self.channels.iter().map(|c| c.id), ValidationError::DuplicateChannel, &mut errors);
        let contents = unique(self.contents.iter().map(|c| c.id), ValidationError::DuplicateContent, &mut errors);
        // Objects without an id yet (0) are not duplicates of each other.
        let clip_ids = self.all_clips().map(|c| c.id).filter(|id| id.is_some());
        let clip_ids = unique(clip_ids, ValidationError::DuplicateClip, &mut errors);
        let lane_ids = self.arrangement.lanes.iter().map(|l| l.id).filter(|id| id.is_some());
        let lane_ids = unique(lane_ids, ValidationError::DuplicateLane, &mut errors);
        let scene_ids = self.launcher.scenes.iter().map(|s| s.id).filter(|id| id.is_some());
        let scene_ids = unique(scene_ids, ValidationError::DuplicateScene, &mut errors);
        // The three kinds share one counter, so a value names at most one object of any of them
        // (ADR 0025 › 2): a lane and a clip with the same value are refused as well.
        let mut kinds: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
        let values =
            clip_ids.iter().map(|c| c.0).chain(lane_ids.iter().map(|l| l.0)).chain(scene_ids.iter().map(|s| s.0));
        for value in values {
            *kinds.entry(value).or_default() += 1;
        }
        errors.extend(kinds.into_iter().filter(|&(_, n)| n > 1).map(|(value, _)| ValidationError::SharedId(value)));
        let highest = self.max_object_id();
        if highest != 0 && highest >= self.next_id {
            errors.push(ValidationError::IdAhead(highest));
        }

        let track_ref = |id: TrackId, errors: &mut Vec<ValidationError>| {
            if !tracks.contains(&id) {
                errors.push(ValidationError::MissingTrack(id));
            }
        };
        let channel_ref = |id: ChannelId, errors: &mut Vec<ValidationError>| {
            if !channels.contains(&id) {
                errors.push(ValidationError::MissingChannel(id));
            }
        };
        let content_ref = |id: ContentId, errors: &mut Vec<ValidationError>| {
            if !contents.contains(&id) {
                errors.push(ValidationError::MissingContent(id));
            }
        };

        for track in &self.tracks {
            if let Some(channel) = track.channel {
                channel_ref(channel, &mut errors);
            }
            if let Some(parent) = track.parent {
                track_ref(parent, &mut errors);
                match self.group_depth(track.id) {
                    None => errors.push(ValidationError::GroupCycle(track.id)),
                    Some(depth) if depth > MAX_GROUP_DEPTH => errors.push(ValidationError::GroupTooDeep(track.id)),
                    Some(_) => {}
                }
            }
            let bad_port = matches!(&track.midi_input.port, MidiPort::Named(name) if name.trim().is_empty());
            if bad_port || track.midi_input.channel.is_some_and(|c| !(1..=16).contains(&c)) {
                errors.push(ValidationError::BadMidiInput(track.id));
            }
            let audio_ok = match track.audio_input.channels {
                InputChannels::None => true,
                InputChannels::Mono(c) => (1..=MAX_INPUT_CHANNEL).contains(&c),
                InputChannels::Stereo(c) => (1..MAX_INPUT_CHANNEL).contains(&c),
            };
            if !audio_ok {
                errors.push(ValidationError::BadAudioInput(track.id));
            }
        }
        for channel in &self.channels {
            if let Some(output) = channel.output {
                channel_ref(output, &mut errors);
            }
            for send in &channel.sends {
                channel_ref(send.target, &mut errors);
            }
            groups::validate_devices(channel.id, &channel.devices, &mut errors);
        }
        for content in &self.contents {
            for lane in &content.lanes {
                if let Some(target) = lane.target {
                    track_ref(target, &mut errors);
                }
            }
        }
        for lane in &self.arrangement.lanes {
            if let Some(track) = lane.track {
                track_ref(track, &mut errors);
            }
            for clip in &lane.clips {
                content_ref(clip.content, &mut errors);
                if !clip.fades_valid() {
                    errors.push(ValidationError::BadFades { lane: lane.id, clip: clip.id });
                }
            }
        }
        for (index, marker) in self.arrangement.markers.iter().enumerate() {
            if !marker.is_valid() {
                errors.push(ValidationError::BadMarker(index));
            }
        }
        if self.transport.loop_range.is_some_and(|r| !r.is_valid_region()) {
            errors.push(ValidationError::BadLoop);
        }
        for column in &self.launcher.columns {
            track_ref(column.track, &mut errors);
            for clip in column.slots.iter().filter_map(|s| s.clip.as_ref()) {
                content_ref(clip.content, &mut errors);
            }
        }

        if errors.is_empty() { Ok(()) } else { Err(errors) }
    }

    /// The groups `track` is inside, nearest first: its parent, the parent's parent and so on.
    /// Stops at a parent that does not exist, and at a cycle (the track that would repeat is
    /// left out), so it always ends.
    pub fn ancestors(&self, track: TrackId) -> Vec<TrackId> {
        let mut out = Vec::new();
        let mut next = self.track(track).and_then(|t| t.parent);
        while let Some(id) = next {
            if id == track || out.contains(&id) || out.len() > self.tracks.len() {
                break;
            }
            out.push(id);
            next = self.track(id).and_then(|t| t.parent);
        }
        out
    }

    /// How many groups `track` is inside (0 at the top level); `None` when its parents lead
    /// back to a track already passed, a cycle a valid project never has.
    pub fn group_depth(&self, track: TrackId) -> Option<usize> {
        let mut seen = vec![track];
        let mut next = self.track(track).and_then(|t| t.parent);
        while let Some(id) = next {
            if seen.contains(&id) {
                return None;
            }
            seen.push(id);
            next = self.track(id).and_then(|t| t.parent);
        }
        Some(seen.len() - 1)
    }

    /// `track` and every track inside it at any depth (for a group), in project order. Bounded
    /// by the track count, so a parent cycle in a malformed project cannot loop.
    pub fn with_descendants(&self, track: TrackId) -> Vec<TrackId> {
        let mut family = vec![track];
        for _ in 0..self.tracks.len() {
            let before = family.len();
            for t in &self.tracks {
                if t.parent.is_some_and(|parent| family.contains(&parent)) && !family.contains(&t.id) {
                    family.push(t.id);
                }
            }
            if family.len() == before {
                break;
            }
        }
        self.tracks.iter().map(|t| t.id).filter(|id| family.contains(id)).collect()
    }

    /// How many levels of groups hang under `track`: 0 for a track with nothing inside it, 1
    /// for a group of plain tracks, 2 for a group holding a group, and so on.
    pub fn nesting_below(&self, track: TrackId) -> usize {
        let base = self.group_depth(track).unwrap_or(0);
        self.with_descendants(track)
            .iter()
            .filter_map(|&t| self.group_depth(t))
            .map(|d| d.saturating_sub(base))
            .max()
            .unwrap_or(0)
    }

    pub fn content(&self, id: ContentId) -> Option<&ClipContent> {
        self.contents.iter().find(|c| c.id == id)
    }

    /// Every clip, on the arrangement or in a launcher slot, that plays the content `id`.
    pub fn clips_of(&self, id: ContentId) -> impl Iterator<Item = &Clip> {
        self.all_clips().filter(move |c| c.content == id)
    }
}

fn unique<T: Copy + Eq + std::hash::Hash>(
    ids: impl Iterator<Item = T>,
    duplicate: impl Fn(T) -> ValidationError,
    errors: &mut Vec<ValidationError>,
) -> HashSet<T> {
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            errors.push(duplicate(id));
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(time: f64, key: u8) -> Note {
        Note::new(time, 0.25, key, 0.8)
    }

    fn track(id: u32, channel: u32) -> Track {
        Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(channel)),
            ..Track::new(TrackId(id), format!("Track {id}"))
        }
    }

    fn channel(id: u32) -> Channel {
        Channel::new(ChannelId(id), format!("Channel {id}"))
    }

    fn clip(content: u32, start: f64) -> Clip {
        Clip::new(ContentId(content), start, 4.0)
    }

    #[test]
    fn follow_kinds_read_back_their_names() {
        for kind in FollowKind::NAMED.into_iter().chain([FollowKind::Jump(0), FollowKind::Jump(41)]) {
            assert_eq!(FollowKind::from_name(&kind.name()), Some(kind));
        }
        assert_eq!(FollowKind::Jump(2).name(), "jump 3");
        assert_eq!(FollowKind::from_name(" Jump  12 "), Some(FollowKind::Jump(11)));
        for bad in ["", "jump", "jump 0", "jump -1", "jump 1.5", "jump 99999999999", "nextt", "stop 2"] {
            assert_eq!(FollowKind::from_name(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_follow_action_needs_a_time_and_chances_in_range() {
        assert!(FollowAction::default().is_valid());
        for time in [0.0, -1.0, f64::NAN, f64::INFINITY, FollowAction::MAX_TIME * 2.0] {
            assert!(!FollowAction { time, ..FollowAction::default() }.is_valid(), "{time}");
        }
        assert!(!FollowAction { chance_a: 101, ..FollowAction::default() }.is_valid());
        assert!(!FollowAction { chance_b: 101, ..FollowAction::default() }.is_valid());
    }

    #[test]
    fn default_transport_is_120_bpm_in_four_four() {
        let t = Project::default().transport;
        assert_eq!(t.tempo_bpm, 120.0);
        assert_eq!(t.time_signature, TimeSignature { numerator: 4, denominator: 4 });
    }

    /// Per-track clips: each clip has its own content with one untargeted lane, arrangement
    /// lanes are bound to tracks, and the same track has a launcher column.
    #[test]
    fn per_track_clip_project_validates() {
        let project = Project {
            tracks: vec![track(1, 1)],
            channels: vec![channel(1)],
            contents: vec![
                ClipContent {
                    id: ContentId(1),
                    name: "Bass A".into(),
                    length: 4.0,
                    lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![note(0.0, 36)]) }],
                    extensions: Extensions::default(),
                    scale: None,
                },
                ClipContent {
                    id: ContentId(2),
                    name: "Bass B".into(),
                    length: 4.0,
                    lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![note(1.0, 38)]) }],
                    extensions: Extensions::default(),
                    scale: None,
                },
            ],
            arrangement: Arrangement {
                lanes: vec![ArrangementLane {
                    id: Default::default(),
                    name: "Bass".into(),
                    track: Some(TrackId(1)),
                    clips: vec![clip(1, 0.0)],
                }],
                markers: Vec::new(),
            },
            launcher: Launcher {
                mappings: Vec::new(),
                scenes: vec![Scene::default(), Scene::default()],
                columns: vec![LauncherColumn {
                    track: TrackId(1),
                    slots: vec![
                        Slot { clip: Some(clip(1, 0.0)), has_stop: true },
                        Slot { clip: Some(clip(2, 0.0)), has_stop: true },
                    ],
                }],
            },
            ..Project::default()
        };
        assert_eq!(project.validate(), Ok(()));
    }

    /// Shared pattern: one content drives two tracks and is placed twice on a free arrangement
    /// lane; both placements share the same content.
    #[test]
    fn shared_pattern_project_validates_and_shares_content() {
        let pattern = ClipContent {
            id: ContentId(1),
            name: "Pattern 1".into(),
            length: 4.0,
            lanes: vec![
                ContentLane { target: Some(TrackId(1)), data: LaneData::Notes(vec![note(0.0, 36), note(2.0, 36)]) },
                ContentLane { target: Some(TrackId(2)), data: LaneData::Notes(vec![note(1.0, 38), note(3.0, 38)]) },
            ],
            extensions: Extensions::default(),
            scale: None,
        };
        let project = Project {
            tracks: vec![track(1, 1), track(2, 2)],
            channels: vec![channel(1), channel(2)],
            contents: vec![pattern],
            arrangement: Arrangement {
                lanes: vec![ArrangementLane {
                    id: Default::default(),
                    name: "Track 1".into(),
                    track: None,
                    clips: vec![clip(1, 0.0), clip(1, 4.0)],
                }],
                markers: Vec::new(),
            },
            ..Project::default()
        };
        assert_eq!(project.validate(), Ok(()));

        let placements = &project.arrangement.lanes[0].clips;
        assert_eq!(placements[0].content, placements[1].content);
        assert_eq!(project.content(ContentId(1)).map(|c| c.lanes.len()), Some(2));
    }

    #[test]
    fn dangling_references_are_reported() {
        let mut ch = channel(1);
        ch.output = Some(ChannelId(9));
        ch.sends.push(Send { target: ChannelId(8), level: 0.5, pre_fader: false });
        let project = Project {
            tracks: vec![track(1, 7)],
            channels: vec![ch],
            arrangement: Arrangement {
                lanes: vec![ArrangementLane {
                    id: Default::default(),
                    name: String::new(),
                    track: Some(TrackId(5)),
                    clips: vec![clip(3, 0.0)],
                }],
                markers: Vec::new(),
            },
            ..Project::default()
        };
        let errors = project.validate().unwrap_err();
        assert!(errors.contains(&ValidationError::MissingChannel(ChannelId(7))));
        assert!(errors.contains(&ValidationError::MissingChannel(ChannelId(9))));
        assert!(errors.contains(&ValidationError::MissingChannel(ChannelId(8))));
        assert!(errors.contains(&ValidationError::MissingTrack(TrackId(5))));
        assert!(errors.contains(&ValidationError::MissingContent(ContentId(3))));
    }

    /// Each shape's gain at the ends and half-way, as the spec's table gives it, and how the
    /// curve bends it (`docs/spec/arrangement-editing.md` › Fades).
    #[test]
    fn fade_shapes_and_curves_follow_the_spec() {
        let half = [
            (FadeShape::Linear, 0.5),
            (FadeShape::EqualPower, std::f64::consts::FRAC_1_SQRT_2),
            (FadeShape::Fast, 0.75),
            (FadeShape::Slow, 0.25),
            (FadeShape::SCurve, 0.5),
        ];
        for (shape, at_half) in half {
            for curve in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                let fade = FadeCurve { shape, curve };
                let expected = at_half.powf(4f64.powf(-curve));
                assert!((fade.gain(0.5) - expected).abs() < 1e-12, "{shape:?} {curve}: {}", fade.gain(0.5));
                assert_eq!((fade.gain(0.0), fade.gain(1.0)), (0.0, 1.0), "{shape:?} {curve}");
                // It rises the whole way.
                let gains: Vec<f64> = (0..=100).map(|i| fade.gain(f64::from(i) / 100.0)).collect();
                assert!(gains.windows(2).all(|w| w[1] >= w[0]), "{shape:?} {curve}");
            }
            assert_eq!(FadeShape::from_name(shape.name()), Some(shape));
        }
        // +1 starts faster than the shape, −1 slower.
        let linear = |curve| FadeCurve { shape: FadeShape::Linear, curve };
        assert!((linear(1.0).gain(0.5) - 0.5f64.powf(0.25)).abs() < 1e-12);
        assert!((linear(-1.0).gain(0.5) - 0.0625).abs() < 1e-12);
        // Hostile input stays within 0 … 1.
        assert_eq!(linear(0.0).gain(f64::NAN), 0.0);
        assert_eq!(linear(0.0).gain(7.0), 1.0);
        assert_eq!(linear(f64::NAN).gain(0.5), 0.5);
        assert_eq!(FadeShape::from_name("wobbly"), None);
        // The curve that puts the half-way gain at a height gives back that height.
        for shape in FadeShape::ALL {
            for curve in [-0.9, -0.3, 0.0, 0.4, 0.95] {
                let gain = FadeCurve { shape, curve }.gain(0.5);
                assert!((FadeCurve::through(shape, gain) - curve).abs() < 1e-9, "{shape:?} {curve}");
            }
            assert_eq!((FadeCurve::through(shape, 0.0), FadeCurve::through(shape, 1.0)), (-1.0, 1.0));
            assert_eq!(FadeCurve::through(shape, f64::NAN), 0.0);
        }
    }

    #[test]
    fn fades_must_be_non_negative_and_fit_the_clip() {
        let with = |fade_in: f64, fade_out: f64| Clip { fade_in, fade_out, ..clip(1, 0.0) };
        assert!(with(0.0, 0.0).fades_valid());
        assert!(with(1.5, 2.5).fades_valid(), "together exactly the clip's length");
        assert!(!with(-0.1, 0.0).fades_valid());
        assert!(!with(0.0, f64::NAN).fades_valid());
        assert!(!with(f64::INFINITY, 0.0).fades_valid());
        assert!(!with(3.0, 1.5).fades_valid(), "4.5 beats of fades on a 4-beat clip");
        let bent = |curve: f64| Clip { fade_out_curve: FadeCurve { shape: FadeShape::Slow, curve }, ..with(1.0, 1.0) };
        assert!(bent(1.0).fades_valid() && bent(-1.0).fades_valid());
        assert!(!bent(1.5).fades_valid() && !bent(f64::NAN).fades_valid(), "a curve outside -1 … +1");

        let content = ClipContent {
            id: ContentId(1),
            name: String::new(),
            length: 4.0,
            lanes: Vec::new(),
            extensions: Extensions::default(),
            scale: None,
        };
        let project = Project {
            contents: vec![content],
            arrangement: Arrangement {
                lanes: vec![ArrangementLane {
                    id: Default::default(),
                    name: String::new(),
                    track: None,
                    clips: vec![with(1.0, 1.0), with(3.0, 3.0)],
                }],
                markers: Vec::new(),
            },
            ..Project::default()
        };
        let mut project = project;
        project.assign_ids().unwrap();
        // The lane is object 1, its clips 2 and 3.
        assert_eq!(project.validate(), Err(vec![ValidationError::BadFades { lane: LaneId(1), clip: ClipId(3) }]));
    }

    #[test]
    fn fitting_fades_keeps_them_inside_the_clip() {
        let mut c = Clip { fade_in: 3.0, fade_out: 1.0, length: 2.0, ..clip(1, 0.0) };
        c.fit_fades();
        assert_eq!((c.fade_in, c.fade_out), (1.5, 0.5), "both shrink in proportion");
        let mut c = Clip { fade_in: -1.0, fade_out: f64::NAN, ..clip(1, 0.0) };
        c.fit_fades();
        assert_eq!((c.fade_in, c.fade_out), (0.0, 0.0));
        let mut c = Clip { fade_in: 1.0, fade_out: 0.5, ..clip(1, 0.0) };
        c.fit_fades();
        assert_eq!((c.fade_in, c.fade_out), (1.0, 0.5), "fades that fit are kept");
    }

    #[test]
    fn the_loop_region_repeats_only_while_on_and_must_end_after_it_starts() {
        let mut transport = Transport { loop_range: Some(LoopRange { start: 4.0, end: 8.0 }), ..Transport::default() };
        assert_eq!(transport.looping(), None, "off: kept, not repeated");
        transport.loop_enabled = true;
        assert_eq!(transport.looping(), Some(LoopRange { start: 4.0, end: 8.0 }));

        for bad in [(4.0, 4.0), (8.0, 4.0), (-1.0, 4.0), (0.0, f64::INFINITY), (f64::NAN, 1.0)] {
            let project = Project {
                transport: Transport {
                    loop_range: Some(LoopRange { start: bad.0, end: bad.1 }),
                    ..Transport::default()
                },
                ..Project::default()
            };
            assert_eq!(project.validate(), Err(vec![ValidationError::BadLoop]), "{bad:?}");
            assert_eq!(Transport { loop_enabled: true, ..project.transport }.looping(), None);
        }
    }

    #[test]
    fn markers_stay_in_beat_order_and_must_be_at_a_beat_from_0() {
        let mut arrangement = Arrangement::default();
        assert_eq!(arrangement.insert_marker(Marker::new(8.0, "Chorus")), 0);
        assert_eq!(arrangement.insert_marker(Marker::new(0.0, "Intro")), 0);
        assert_eq!(arrangement.insert_marker(Marker::new(8.0, "Drop")), 2, "after the one already on beat 8");
        let names: Vec<&str> = arrangement.markers.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Intro", "Chorus", "Drop"]);
        arrangement.markers[0].beat = 12.0;
        arrangement.sort_markers();
        let names: Vec<&str> = arrangement.markers.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Chorus", "Drop", "Intro"]);

        let project = Project {
            arrangement: Arrangement {
                lanes: Vec::new(),
                markers: vec![Marker::new(0.0, "ok"), Marker::new(-1.0, "before"), Marker::new(f64::NAN, "nan")],
            },
            ..Project::default()
        };
        assert_eq!(project.validate(), Err(vec![ValidationError::BadMarker(1), ValidationError::BadMarker(2)]));
    }

    #[test]
    fn duplicate_ids_are_reported() {
        let project =
            Project { tracks: vec![track(1, 1), track(1, 1)], channels: vec![channel(1)], ..Project::default() };
        assert_eq!(project.validate(), Err(vec![ValidationError::DuplicateTrack(TrackId(1))]));
    }
    /// Tracks 1..=n, each inside the one before it.
    fn chain_of_groups(n: u32) -> Project {
        let tracks = (1..=n).map(|id| Track { parent: (id > 1).then(|| TrackId(id - 1)), ..track(id, id) }).collect();
        Project { tracks, channels: (1..=n).map(channel).collect(), ..Project::default() }
    }

    #[test]
    fn nested_groups_report_their_ancestors_depth_and_descendants() {
        let project = chain_of_groups(4);
        assert_eq!(project.ancestors(TrackId(4)), vec![TrackId(3), TrackId(2), TrackId(1)]);
        assert_eq!(project.group_depth(TrackId(4)), Some(3));
        assert_eq!(project.group_depth(TrackId(1)), Some(0));
        assert_eq!(project.with_descendants(TrackId(2)), vec![TrackId(2), TrackId(3), TrackId(4)]);
        assert_eq!(project.nesting_below(TrackId(2)), 2);
        assert_eq!(project.nesting_below(TrackId(4)), 0);
        assert_eq!(project.validate(), Ok(()));
        // A track that does not exist is at the top level and holds nothing.
        assert_eq!(project.ancestors(TrackId(9)), Vec::<TrackId>::new());
        assert_eq!(project.with_descendants(TrackId(9)), vec![]);
    }

    #[test]
    fn groups_may_nest_up_to_the_cap_and_no_deeper() {
        let deepest = u32::try_from(MAX_GROUP_DEPTH).unwrap() + 1;
        assert_eq!(chain_of_groups(deepest).validate(), Ok(()));
        let too_deep = chain_of_groups(deepest + 1);
        assert_eq!(too_deep.validate(), Err(vec![ValidationError::GroupTooDeep(TrackId(deepest + 1))]));
    }

    #[test]
    fn a_group_cycle_is_reported_and_walking_it_ends() {
        let mut project = chain_of_groups(3);
        project.tracks[0].parent = Some(TrackId(3));
        assert_eq!(project.group_depth(TrackId(2)), None);
        assert_eq!(project.ancestors(TrackId(2)), vec![TrackId(1), TrackId(3)]);
        assert_eq!(project.with_descendants(TrackId(1)).len(), 3);
        let errors = project.validate().unwrap_err();
        assert_eq!(errors.len(), 3, "{errors:?}");
        assert!(errors.iter().all(|e| matches!(e, ValidationError::GroupCycle(_))));
        // A track that is its own parent too.
        let mut own = chain_of_groups(1);
        own.tracks[0].parent = Some(TrackId(1));
        assert_eq!(own.validate(), Err(vec![ValidationError::GroupCycle(TrackId(1))]));
        assert_eq!(own.ancestors(TrackId(1)), vec![]);
    }
}
