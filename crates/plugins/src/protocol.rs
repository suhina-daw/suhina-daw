// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The control messages between the engine and a plug-in host process (ADR 0006): one JSON
//! object per line over a local socket. Audio does not go here; it goes through `suhina-ipc`.
//!
//! Every message has an `op`. Engine to host: `hello`, `load`, `unload`, `ping`, `get_state`,
//! `set_state`, `get_params`, `set_param`, `open_editor`, `place_editor`, `close_editor`,
//! `fit_editor`, `ara_select`, `ara_update`, `ara_export`, `ara_import`, `quit`. Host to engine: `hello`, `loaded`, `unloaded`, `pong`, `state`, `params`, `param`,
//! `editor_opened`, `editor_closed`, `editor_resized`, `latency_changed`, `ara_updated`, `ara_exported`, `ara_imported`, `memory`,
//! `transport_key`, `ara_playback`, `ara_notes`, `error`. Every message is answered, except `place_editor`, `fit_editor`,
//! `ara_select` and `quit` (so the engine never waits on them).
//! `editor_closed` also comes unasked, when the user closes an editor's window; `editor_resized`
//! only comes unasked, when a plug-in gives its editor a new size, `latency_changed` only
//! comes unasked, when a running plug-in reports another latency, `memory` only comes unasked,
//! once a second, `transport_key` only comes unasked, when Space is pressed in a plug-in's
//! window, `ara_playback` only comes unasked, when an ARA plug-in asks the transport to play,
//! stop or move, and `ara_notes` only comes unasked, when the notes it found in a region change.
//!
//! One host process runs several plug-in instances. The engine names each with an `instance`
//! number when it loads it; every message about one instance, and its reply, carries that
//! number. `hello`, `ping`/`pong` and `quit` are about the whole process. An `error` carries the
//! instance of the request it answers, or none for one about the process.

use serde_json::{Value, json};

/// Bumped whenever a message changes; both sides refuse a different version.
pub const VERSION: u64 = 13;
/// Longest line either side accepts; state blobs are hex, so this bounds them at 32 MiB.
pub const MAX_LINE: usize = 64 * 1024 * 1024;

/// Most parameters a plug-in's list carries.
pub const MAX_PARAMS: usize = 1 << 16;

/// One of a plug-in's parameters. Values are in the plug-in's own range, `min` to `max`: a
/// VST3 plug-in's are normalized (0 to 1), a CLAP plug-in's are its plain values.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamInfo {
    pub id: u32,
    pub name: String,
    /// Where the plug-in groups it (a CLAP module path, a VST3 unit), or empty.
    pub group: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub value: f64,
    /// The value as the plug-in shows it ("-3.2 dB"), or empty.
    pub text: String,
    /// Number of steps for a stepped parameter (a switch has 1), 0 for a continuous one.
    pub steps: u32,
    pub automatable: bool,
    pub read_only: bool,
    pub hidden: bool,
}

impl ParamInfo {
    fn to_json(&self) -> Value {
        json!({
            "id": self.id, "name": self.name, "group": self.group, "min": self.min, "max": self.max,
            "default": self.default, "value": self.value, "text": self.text, "steps": self.steps,
            "automatable": self.automatable, "read_only": self.read_only, "hidden": self.hidden,
        })
    }

    fn from_json(value: &Value) -> Option<Self> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
        let number = |key: &str| value.get(key).and_then(Value::as_f64).filter(|n| n.is_finite());
        let flag = |key: &str| value.get(key).and_then(Value::as_bool);
        Some(Self {
            id: u32::try_from(value.get("id")?.as_u64()?).ok()?,
            name: text("name")?,
            group: text("group")?,
            min: number("min")?,
            max: number("max")?,
            default: number("default")?,
            value: number("value")?,
            text: text("text")?,
            steps: u32::try_from(value.get("steps")?.as_u64()?).ok()?,
            automatable: flag("automatable")?,
            read_only: flag("read_only")?,
            hidden: flag("hidden")?,
        })
    }
}

/// Where an embedded editor sits in the window it is a child of: its origin, in the window's
/// own pixels from its top left (negative when the device panel is scrolled past it), and the
/// part of the window that shows it (the panel's visible area): what falls outside it is cut
/// off. An empty clip hides the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub clip_x: i32,
    pub clip_y: i32,
    pub clip_width: u32,
    pub clip_height: u32,
}

impl Placement {
    /// Nowhere: the editor is hidden.
    pub const HIDDEN: Self = Self { x: 0, y: 0, clip_x: 0, clip_y: 0, clip_width: 0, clip_height: 0 };

    fn to_json(self) -> Value {
        json!({
            "x": self.x, "y": self.y, "clip_x": self.clip_x, "clip_y": self.clip_y,
            "clip_width": self.clip_width, "clip_height": self.clip_height,
        })
    }

    fn from_json(value: &Value) -> Option<Self> {
        let signed = |key: &str| value.get(key).and_then(Value::as_i64).and_then(|n| i32::try_from(n).ok());
        let unsigned = |key: &str| value.get(key).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
        Some(Self {
            x: signed("x")?,
            y: signed("y")?,
            clip_x: signed("clip_x")?,
            clip_y: signed("clip_y")?,
            clip_width: unsigned("clip_width")?,
            clip_height: unsigned("clip_height")?,
        })
    }
}

/// Most audio sources, and playback regions, one ARA document describes.
pub const MAX_ARA_OBJECTS: usize = 4096;
/// Longest name or id in an ARA document, in bytes.
pub const MAX_ARA_TEXT: usize = 1024;
/// Most notes one `ara_notes` message carries for a region; more are cut, and the message says so.
pub const MAX_ARA_NOTES: usize = 16_384;

/// A note an ARA plug-in found in a clip (#720): `start` seconds after the region starts,
/// lasting `length` seconds, at MIDI pitch `pitch`, as loud as `volume` (0 to 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AraNote {
    pub start: f64,
    pub length: f64,
    pub pitch: u8,
    pub volume: f32,
}

impl AraNote {
    fn to_json(self) -> Value {
        json!([self.start, self.length, self.pitch, self.volume])
    }

    fn from_json(value: &Value) -> Option<Self> {
        let list = value.as_array().filter(|l| l.len() == 4)?;
        let number = |i: usize| list.get(i).and_then(Value::as_f64).filter(|v| v.is_finite());
        let (start, length) = (number(0)?, number(1)?);
        let pitch = u8::try_from(list.get(2)?.as_u64()?).ok().filter(|p| *p <= 127)?;
        let volume = number(3)?;
        if length < 0.0 || !(0.0..=1.0).contains(&volume) {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, reason = "a volume from 0 to 1")]
        Some(Self { start, length, pitch, volume: volume as f32 })
    }
}

/// The ARA document a plug-in instance is bound to as it loads ([ADR 0028]): the audio sources
/// it may read and the playback regions it renders and edits, on one region sequence (the
/// track) at one tempo. Ids are persistent: built from the project's stable ids, never from
/// positions.
///
/// [ADR 0028]: https://github.com/suhina-daw/suhina-daw/blob/main/docs/decisions/0028-ara-hosting.md
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AraDocument {
    pub name: String,
    /// The document this track belongs to: the instances of one plug-in in one host process
    /// with the same key share one document, each track a region sequence of it, so the
    /// plug-in sees every track (ADR 0028 decision 2). Empty: a document of its own.
    pub document: String,
    /// The region sequence's name: the track's.
    pub sequence: String,
    /// The track's place among the project's tracks (the sequence's order).
    pub order: u32,
    pub tempo_bpm: f64,
    /// The song's tempo map as ARA's tempo entries: (seconds, quarter notes), in order, at least
    /// two; between two entries the tempo is even. Empty: an even `tempo_bpm` from the start.
    pub tempo: Vec<(f64, f64)>,
    /// The song's time signatures as ARA's bar signatures: (numerator, denominator, the quarter
    /// note the bar starts at), in order. Empty: 4/4 from the start.
    pub bars: Vec<(u32, u32, f64)>,
    pub sources: Vec<AraSource>,
    pub regions: Vec<AraRegion>,
}

impl AraDocument {
    /// The tempo entries the plug-in is told: `tempo`, or an even `tempo_bpm` from the start.
    pub fn tempo_entries(&self) -> Vec<(f64, f64)> {
        if self.tempo.len() >= 2 {
            return self.tempo.clone();
        }
        let bpm = if self.tempo_bpm.is_finite() && self.tempo_bpm > 0.0 { self.tempo_bpm } else { 120.0 };
        vec![(0.0, 0.0), (60.0 / bpm, 1.0)]
    }

    /// The bar signatures the plug-in is told: `bars`, or 4/4 from the start.
    pub fn bar_signatures(&self) -> Vec<(u32, u32, f64)> {
        if self.bars.is_empty() { vec![(4, 4, 0.0)] } else { self.bars.clone() }
    }
}

/// An audio file's decoded samples, in a region the host maps read only
/// ([`suhina_ipc::AudioSource`]); its channels, length and rate are in the region's header.
#[derive(Debug, Clone, PartialEq)]
pub struct AraSource {
    pub id: String,
    pub name: String,
    pub address: String,
}

/// A clip as an ARA playback region: it plays `duration` seconds of source `source` (an index
/// into [`AraDocument::sources`]) from `offset` seconds into it, at `start` seconds of the song,
/// through the audio modification `modification` (shared by clips that share their content).
#[derive(Debug, Clone, PartialEq)]
pub struct AraRegion {
    pub id: String,
    pub name: String,
    pub source: usize,
    pub modification: String,
    pub start: f64,
    pub duration: f64,
    pub offset: f64,
    /// For a modification the plug-in does not have yet: the one it is a copy of, so it starts
    /// with that one's edits (`cloneAudioModification`) rather than none. A pasted, split or
    /// otherwise copied clip gets its own content, and with this its edits (#722).
    pub clone_of: Option<String>,
}

impl AraDocument {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name, "document": self.document, "sequence": self.sequence, "order": self.order,
            "tempo_bpm": self.tempo_bpm,
            "tempo": self.tempo.iter().map(|(s, q)| json!([s, q])).collect::<Vec<_>>(),
            "bars": self.bars.iter().map(|(n, d, q)| json!([n, d, q])).collect::<Vec<_>>(),
            "sources": self.sources.iter().map(|s| json!({ "id": s.id, "name": s.name, "address": s.address }))
                .collect::<Vec<_>>(),
            "regions": self.regions.iter().map(|r| json!({
                "id": r.id, "name": r.name, "source": r.source, "modification": r.modification,
                "start": r.start, "duration": r.duration, "offset": r.offset, "clone_of": r.clone_of,
            })).collect::<Vec<_>>(),
        })
    }

    /// Reads a document, refusing one with more objects or longer texts than the limits, a
    /// region whose source is not in the list, or times that are not finite and in order.
    fn from_json(value: &Value) -> Option<Self> {
        let text = |v: &Value, key: &str| {
            v.get(key).and_then(Value::as_str).filter(|t| t.len() <= MAX_ARA_TEXT).map(str::to_string)
        };
        let time = |v: &Value, key: &str| v.get(key).and_then(Value::as_f64).filter(|t| t.is_finite());
        let list = |key: &str| value.get(key).and_then(Value::as_array).filter(|l| l.len() <= MAX_ARA_OBJECTS);
        let sources = list("sources")?
            .iter()
            .map(|s| Some(AraSource { id: text(s, "id")?, name: text(s, "name")?, address: text(s, "address")? }))
            .collect::<Option<Vec<_>>>()?;
        let regions = list("regions")?
            .iter()
            .map(|r| {
                let source = usize::try_from(r.get("source")?.as_u64()?).ok().filter(|&i| i < sources.len())?;
                let (start, duration, offset) = (time(r, "start")?, time(r, "duration")?, time(r, "offset")?);
                if start < 0.0 || duration <= 0.0 || offset < 0.0 {
                    return None;
                }
                Some(AraRegion {
                    id: text(r, "id")?,
                    name: text(r, "name")?,
                    source,
                    modification: text(r, "modification")?,
                    start,
                    duration,
                    offset,
                    clone_of: match r.get("clone_of") {
                        None | Some(Value::Null) => None,
                        Some(_) => Some(text(r, "clone_of")?),
                    },
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let tempo_bpm = time(value, "tempo_bpm").filter(|t| *t > 0.0)?;
        // Tempo entries and bar signatures, in order; a document without them (an older engine)
        // has an even tempo in 4/4.
        let number = |v: &Value| v.as_f64().filter(|n| n.is_finite());
        let tempo = match value.get("tempo") {
            None | Some(Value::Null) => Vec::new(),
            Some(v) => v
                .as_array()
                .filter(|l| l.len() <= MAX_ARA_OBJECTS)?
                .iter()
                .map(|e| Some((number(e.get(0)?)?, number(e.get(1)?)?)))
                .collect::<Option<Vec<_>>>()?,
        };
        if tempo.windows(2).any(|w| !(w[1].0 > w[0].0 && w[1].1 > w[0].1)) {
            return None;
        }
        let bars = match value.get("bars") {
            None | Some(Value::Null) => Vec::new(),
            Some(v) => v
                .as_array()
                .filter(|l| l.len() <= MAX_ARA_OBJECTS)?
                .iter()
                .map(|e| {
                    // A project's numerators are 1 to 99 (`TimeSignature` in the model).
                    let n = u32::try_from(e.get(0)?.as_u64()?).ok().filter(|n| (1..=99).contains(n))?;
                    let d = u32::try_from(e.get(1)?.as_u64()?).ok().filter(|d| (1..=64).contains(d))?;
                    Some((n, d, number(e.get(2)?)?))
                })
                .collect::<Option<Vec<_>>>()?,
        };
        if bars.windows(2).any(|w| w[1].2 <= w[0].2) {
            return None;
        }
        Some(Self {
            name: text(value, "name")?,
            // An older engine's document is one of its own.
            document: match value.get("document") {
                None | Some(Value::Null) => String::new(),
                Some(_) => text(value, "document")?,
            },
            sequence: text(value, "sequence")?,
            order: value.get("order").and_then(Value::as_u64).and_then(|o| u32::try_from(o).ok()).unwrap_or(0),
            tempo_bpm,
            tempo,
            bars,
            sources,
            regions,
        })
    }
}

/// What an ARA plug-in asked of the host's transport (ARA's playback controller).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AraPlayback {
    Start,
    Stop,
    /// Move the play position to this many seconds of song time.
    Position(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Hello {
        version: u64,
    },
    /// Load `plugin` as `instance`, activate it at `sample_rate` and serve audio on the link
    /// at `link`.
    Load {
        instance: u32,
        plugin: String,
        link: String,
        sample_rate: f64,
        /// For an ARA plug-in (ADR 0028): the document to bind it to before it is activated.
        ara: Option<AraDocument>,
    },
    /// `edits_clips`: the plug-in supports ARA (a VST3 component that answers
    /// `IPlugInEntryPoint`), whether or not it was loaded with a document.
    Loaded {
        instance: u32,
        latency: u32,
        edits_clips: bool,
    },
    /// Stop and let go of `instance`; the other instances in the process go on.
    Unload {
        instance: u32,
    },
    Unloaded {
        instance: u32,
    },
    Ping {
        id: u64,
    },
    Pong {
        id: u64,
    },
    GetState {
        instance: u32,
    },
    /// `controller` is a VST3 plug-in's controller state, when it has one apart from the
    /// component's (`data`).
    SetState {
        instance: u32,
        data: Vec<u8>,
        controller: Option<Vec<u8>>,
    },
    State {
        instance: u32,
        data: Vec<u8>,
        controller: Option<Vec<u8>>,
    },
    GetParams {
        instance: u32,
    },
    Params {
        instance: u32,
        params: Vec<ParamInfo>,
    },
    /// Set parameter `id` to `value` (in its own range); answered with `Param`.
    SetParam {
        instance: u32,
        id: u32,
        value: f64,
    },
    Param {
        instance: u32,
        param: ParamInfo,
    },
    /// Open the plug-in's editor; answered with `EditorOpened` and the size of its inside.
    /// `owner` is the engine's window (a Win32 `HWND`, an X11 window id). Not `embedded`: the
    /// editor gets a window of its own that `owner` owns: it stays above it, has no taskbar
    /// entry of its own and is minimised with it (`None`: a window of its own). `embedded`:
    /// the editor is a child inside `owner` (which must then be given), hidden until the first
    /// `PlaceEditor` puts it somewhere.
    OpenEditor {
        instance: u32,
        owner: Option<u64>,
        embedded: bool,
    },
    EditorOpened {
        instance: u32,
        width: u32,
        height: u32,
    },
    /// Put an embedded editor at `placement` in its parent window. Not answered; sent again
    /// whenever the device panel moves, scrolls or resizes.
    PlaceEditor {
        instance: u32,
        placement: Placement,
    },
    /// Ask an embedded editor to take the size of the room the engine has for it, in pixels,
    /// when its plug-in lets its editor be resized (it may keep within its own limits). Not
    /// answered; the new size comes back as `EditorResized`.
    FitEditor {
        instance: u32,
        width: u32,
        height: u32,
    },
    /// Tell a plug-in bound to an ARA document (ADR 0028) which playback region the host has
    /// selected, by its persistent id (`clip-<id>`), so its editor shows it; `None` selects
    /// nothing. Not answered; a plug-in without a document ignores it.
    AraSelect {
        instance: u32,
        region: Option<String>,
    },
    /// Bring the ARA document of a plug-in loaded with one (ADR 0028) in line with `document`,
    /// its track's clips as they are now: moved or trimmed clips are updated in place, new
    /// ones added, gone ones taken out, matched by their persistent ids. Answered with
    /// `AraUpdated`, or an error after which the document is as it was or partly changed (the
    /// engine then loads the plug-in again).
    AraUpdate {
        instance: u32,
        document: AraDocument,
    },
    AraUpdated {
        instance: u32,
    },
    /// The edits of one audio modification of a plug-in loaded with an ARA document (ADR 0028),
    /// by its persistent id (`content-<id>`), with its audio source's (`source`, the pool key):
    /// stored on their own (a partial archive), to give another track's plug-in. Answered with
    /// `AraExported`, or an error when the plug-in cannot store them.
    AraExport {
        instance: u32,
        modification: String,
        source: String,
    },
    AraExported {
        instance: u32,
        /// The plug-in's document archive id the edits are stored for.
        archive_id: String,
        data: Vec<u8>,
    },
    /// Gives a plug-in loaded with an ARA document the edits `AraExport` stored in another
    /// track's, for its modification `to` (they were `from` there) of the same source: a clip
    /// moved or copied to this track with its edits kept. Answered with `AraImported`, or an
    /// error (no modification `to` in the document yet, or a plug-in that cannot restore).
    AraImport {
        instance: u32,
        archive_id: String,
        data: Vec<u8>,
        from: String,
        to: String,
        source: String,
    },
    AraImported {
        instance: u32,
    },
    /// The plug-in gave its editor a new size (unasked, in both modes).
    EditorResized {
        instance: u32,
        width: u32,
        height: u32,
    },
    /// The plug-in reports another latency, in frames, while it runs (unasked): the host
    /// restarted its processing as its format asks, and blocks come back that much later.
    LatencyChanged {
        instance: u32,
        latency: u32,
    },
    /// The host reports its process's resident memory, in bytes, once a second (unasked): the
    /// control bar adds every host's to the program's own RAM.
    Memory {
        bytes: u64,
    },
    /// Space was pressed in one of the host's editor windows (unasked): the engine plays or
    /// stops as Space does in its own window, since the key never reaches it there.
    TransportKey,
    /// An ARA plug-in asked the host's transport to play, stop or move (unasked): its own
    /// transport buttons and a click in its editor's timeline (#717).
    AraPlayback(AraPlayback),
    /// The notes an ARA plug-in found in one of the instance's regions, all of them, oldest
    /// first (unasked): sent when they change, and empty when it has none (any more) (#720).
    /// `truncated` when there were more than [`MAX_ARA_NOTES`].
    AraNotes {
        instance: u32,
        region: String,
        notes: Vec<AraNote>,
        truncated: bool,
    },
    /// How far an ARA plug-in's analysis of the instance's track has come, 0 to 1 (the least
    /// done of its sources), or `None` once nothing is being analysed (unasked): sent when it
    /// moves by a percent, so its clips can say so.
    AraAnalysis {
        instance: u32,
        progress: Option<f32>,
    },
    CloseEditor {
        instance: u32,
    },
    EditorClosed {
        instance: u32,
    },
    /// What went wrong with a request about `instance`, or with the process (`None`).
    Error {
        instance: Option<u32>,
        message: String,
    },
    Quit,
}

impl Message {
    /// The instance the message is about; `None` for the process-wide ones.
    pub fn instance(&self) -> Option<u32> {
        match self {
            Self::Load { instance, .. }
            | Self::Loaded { instance, .. }
            | Self::Unload { instance }
            | Self::Unloaded { instance }
            | Self::GetState { instance }
            | Self::SetState { instance, .. }
            | Self::State { instance, .. }
            | Self::GetParams { instance }
            | Self::Params { instance, .. }
            | Self::SetParam { instance, .. }
            | Self::Param { instance, .. }
            | Self::OpenEditor { instance, .. }
            | Self::EditorOpened { instance, .. }
            | Self::PlaceEditor { instance, .. }
            | Self::AraSelect { instance, .. }
            | Self::FitEditor { instance, .. }
            | Self::AraUpdate { instance, .. }
            | Self::AraUpdated { instance }
            | Self::AraExport { instance, .. }
            | Self::AraExported { instance, .. }
            | Self::AraImport { instance, .. }
            | Self::AraImported { instance }
            | Self::EditorResized { instance, .. }
            | Self::LatencyChanged { instance, .. }
            | Self::AraNotes { instance, .. }
            | Self::AraAnalysis { instance, .. }
            | Self::CloseEditor { instance }
            | Self::EditorClosed { instance } => Some(*instance),
            Self::Error { instance, .. } => *instance,
            Self::Hello { .. }
            | Self::Ping { .. }
            | Self::Pong { .. }
            | Self::Memory { .. }
            | Self::TransportKey
            | Self::AraPlayback(_)
            | Self::Quit => None,
        }
    }

    /// One line of JSON, without the newline.
    pub fn encode(&self) -> String {
        let value = match self {
            Self::Hello { version } => json!({ "op": "hello", "version": version }),
            Self::Load { instance, plugin, link, sample_rate, ara } => {
                let mut value = json!({
                    "op": "load", "instance": instance, "plugin": plugin, "link": link, "sample_rate": sample_rate,
                });
                if let Some(document) = ara {
                    value["ara"] = document.to_json();
                }
                value
            }
            Self::Loaded { instance, latency, edits_clips } => {
                json!({ "op": "loaded", "instance": instance, "latency": latency, "edits_clips": edits_clips })
            }
            Self::Unload { instance } => json!({ "op": "unload", "instance": instance }),
            Self::Unloaded { instance } => json!({ "op": "unloaded", "instance": instance }),
            Self::Ping { id } => json!({ "op": "ping", "id": id }),
            Self::Pong { id } => json!({ "op": "pong", "id": id }),
            Self::GetState { instance } => json!({ "op": "get_state", "instance": instance }),
            Self::SetState { instance, data, controller } => json!({
                "op": "set_state", "instance": instance, "data": hex(data), "controller": controller.as_deref().map(hex),
            }),
            Self::State { instance, data, controller } => json!({
                "op": "state", "instance": instance, "data": hex(data), "controller": controller.as_deref().map(hex),
            }),
            Self::Error { instance, message } => json!({ "op": "error", "instance": instance, "message": message }),
            Self::GetParams { instance } => json!({ "op": "get_params", "instance": instance }),
            Self::Params { instance, params } => json!({
                "op": "params", "instance": instance, "params": params.iter().map(ParamInfo::to_json).collect::<Vec<_>>(),
            }),
            Self::SetParam { instance, id, value } => {
                json!({ "op": "set_param", "instance": instance, "id": id, "value": value })
            }
            Self::Param { instance, param } => json!({ "op": "param", "instance": instance, "param": param.to_json() }),
            Self::OpenEditor { instance, owner, embedded } => {
                json!({ "op": "open_editor", "instance": instance, "owner": owner, "embedded": embedded })
            }
            Self::EditorOpened { instance, width, height } => {
                json!({ "op": "editor_opened", "instance": instance, "width": width, "height": height })
            }
            Self::PlaceEditor { instance, placement } => {
                json!({ "op": "place_editor", "instance": instance, "placement": placement.to_json() })
            }
            Self::FitEditor { instance, width, height } => {
                json!({ "op": "fit_editor", "instance": instance, "width": width, "height": height })
            }
            Self::AraUpdate { instance, document } => {
                json!({ "op": "ara_update", "instance": instance, "document": document.to_json() })
            }
            Self::AraUpdated { instance } => json!({ "op": "ara_updated", "instance": instance }),
            Self::AraExport { instance, modification, source } => {
                json!({ "op": "ara_export", "instance": instance, "modification": modification, "source": source })
            }
            Self::AraExported { instance, archive_id, data } => {
                json!({ "op": "ara_exported", "instance": instance, "archive_id": archive_id, "data": hex(data) })
            }
            Self::AraImport { instance, archive_id, data, from, to, source } => json!({
                "op": "ara_import", "instance": instance, "archive_id": archive_id, "data": hex(data),
                "from": from, "to": to, "source": source,
            }),
            Self::AraImported { instance } => json!({ "op": "ara_imported", "instance": instance }),
            Self::AraNotes { instance, region, notes, truncated } => json!({
                "op": "ara_notes", "instance": instance, "region": region, "truncated": truncated,
                "notes": notes.iter().map(|n| n.to_json()).collect::<Vec<_>>(),
            }),
            Self::AraAnalysis { instance, progress } => {
                json!({ "op": "ara_analysis", "instance": instance, "progress": progress })
            }
            Self::AraSelect { instance, region } => {
                json!({ "op": "ara_select", "instance": instance, "region": region })
            }
            Self::EditorResized { instance, width, height } => {
                json!({ "op": "editor_resized", "instance": instance, "width": width, "height": height })
            }
            Self::LatencyChanged { instance, latency } => {
                json!({ "op": "latency_changed", "instance": instance, "latency": latency })
            }
            Self::Memory { bytes } => json!({ "op": "memory", "bytes": bytes }),
            Self::TransportKey => json!({ "op": "transport_key" }),
            Self::AraPlayback(AraPlayback::Start) => json!({ "op": "ara_playback", "request": "start" }),
            Self::AraPlayback(AraPlayback::Stop) => json!({ "op": "ara_playback", "request": "stop" }),
            Self::AraPlayback(AraPlayback::Position(seconds)) => {
                json!({ "op": "ara_playback", "request": "position", "seconds": seconds })
            }
            Self::CloseEditor { instance } => json!({ "op": "close_editor", "instance": instance }),
            Self::EditorClosed { instance } => json!({ "op": "editor_closed", "instance": instance }),
            Self::Quit => json!({ "op": "quit" }),
        };
        value.to_string()
    }

    /// Parses one line; `None` for anything that is not a known, well-formed message.
    pub fn decode(line: &str) -> Option<Self> {
        if line.len() > MAX_LINE {
            return None;
        }
        let value: Value = serde_json::from_str(line).ok()?;
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
        let number = |key: &str| value.get(key).and_then(Value::as_u64);
        let small = |key: &str| number(key).and_then(|n| u32::try_from(n).ok());
        let bytes = |key: &str| value.get(key).and_then(Value::as_str).and_then(unhex);
        // Absent or null is `None`; anything else must be hex.
        let optional_bytes = |key: &str| match value.get(key) {
            None | Some(Value::Null) => Some(None),
            Some(v) => v.as_str().and_then(unhex).map(Some),
        };
        let instance = || small("instance");
        Some(match value.get("op")?.as_str()? {
            "hello" => Self::Hello { version: number("version")? },
            "load" => Self::Load {
                instance: instance()?,
                plugin: text("plugin")?,
                link: text("link")?,
                sample_rate: value.get("sample_rate").and_then(Value::as_f64).filter(|r| r.is_finite() && *r > 0.0)?,
                ara: match value.get("ara") {
                    None | Some(Value::Null) => None,
                    Some(document) => Some(AraDocument::from_json(document)?),
                },
            },
            "loaded" => Self::Loaded {
                instance: instance()?,
                latency: small("latency")?,
                edits_clips: match value.get("edits_clips") {
                    None => false,
                    Some(v) => v.as_bool()?,
                },
            },
            "unload" => Self::Unload { instance: instance()? },
            "unloaded" => Self::Unloaded { instance: instance()? },
            "ping" => Self::Ping { id: number("id")? },
            "pong" => Self::Pong { id: number("id")? },
            "get_state" => Self::GetState { instance: instance()? },
            "set_state" => Self::SetState {
                instance: instance()?,
                data: bytes("data")?,
                controller: optional_bytes("controller")?,
            },
            "state" => {
                Self::State { instance: instance()?, data: bytes("data")?, controller: optional_bytes("controller")? }
            }
            "error" => Self::Error {
                // Absent or null is the process; anything else must be an instance number.
                instance: match value.get("instance") {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(u32::try_from(v.as_u64()?).ok()?),
                },
                message: text("message")?,
            },
            "get_params" => Self::GetParams { instance: instance()? },
            "params" => {
                let list = value.get("params")?.as_array()?;
                if list.len() > MAX_PARAMS {
                    return None;
                }
                Self::Params {
                    instance: instance()?,
                    params: list.iter().map(ParamInfo::from_json).collect::<Option<Vec<_>>>()?,
                }
            }
            "set_param" => Self::SetParam {
                instance: instance()?,
                id: small("id")?,
                value: value.get("value").and_then(Value::as_f64).filter(|v| v.is_finite())?,
            },
            "param" => Self::Param { instance: instance()?, param: ParamInfo::from_json(value.get("param")?)? },
            "open_editor" => Self::OpenEditor {
                instance: instance()?,
                owner: match value.get("owner") {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(v.as_u64()?),
                },
                embedded: value.get("embedded")?.as_bool()?,
            },
            "editor_opened" => {
                Self::EditorOpened { instance: instance()?, width: small("width")?, height: small("height")? }
            }
            "place_editor" => {
                Self::PlaceEditor { instance: instance()?, placement: Placement::from_json(value.get("placement")?)? }
            }
            "fit_editor" => Self::FitEditor { instance: instance()?, width: small("width")?, height: small("height")? },
            "ara_update" => {
                Self::AraUpdate { instance: instance()?, document: AraDocument::from_json(value.get("document")?)? }
            }
            "ara_updated" => Self::AraUpdated { instance: instance()? },
            "ara_export" => Self::AraExport {
                instance: instance()?,
                modification: text("modification").filter(|t| t.len() <= MAX_ARA_TEXT)?,
                source: text("source").filter(|t| t.len() <= MAX_ARA_TEXT)?,
            },
            "ara_exported" => Self::AraExported {
                instance: instance()?,
                archive_id: text("archive_id").filter(|t| t.len() <= MAX_ARA_TEXT)?,
                data: bytes("data")?,
            },
            "ara_import" => Self::AraImport {
                instance: instance()?,
                archive_id: text("archive_id").filter(|t| t.len() <= MAX_ARA_TEXT)?,
                data: bytes("data")?,
                from: text("from").filter(|t| t.len() <= MAX_ARA_TEXT)?,
                to: text("to").filter(|t| t.len() <= MAX_ARA_TEXT)?,
                source: text("source").filter(|t| t.len() <= MAX_ARA_TEXT)?,
            },
            "ara_imported" => Self::AraImported { instance: instance()? },
            "ara_notes" => Self::AraNotes {
                instance: instance()?,
                region: value.get("region")?.as_str().filter(|t| t.len() <= MAX_ARA_TEXT)?.to_string(),
                notes: value
                    .get("notes")?
                    .as_array()
                    .filter(|l| l.len() <= MAX_ARA_NOTES)?
                    .iter()
                    .map(AraNote::from_json)
                    .collect::<Option<Vec<_>>>()?,
                truncated: value.get("truncated")?.as_bool()?,
            },
            "ara_analysis" => Self::AraAnalysis {
                instance: instance()?,
                progress: match value.get("progress") {
                    None | Some(Value::Null) => None,
                    Some(p) => Some((p.as_f64().filter(|p| p.is_finite())? as f32).clamp(0.0, 1.0)),
                },
            },
            "ara_select" => Self::AraSelect {
                instance: instance()?,
                region: match value.get("region") {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(v.as_str().filter(|t| t.len() <= MAX_ARA_TEXT)?.to_string()),
                },
            },
            "editor_resized" => {
                Self::EditorResized { instance: instance()?, width: small("width")?, height: small("height")? }
            }
            "latency_changed" => Self::LatencyChanged { instance: instance()?, latency: small("latency")? },
            "memory" => Self::Memory { bytes: number("bytes")? },
            "transport_key" => Self::TransportKey,
            "ara_playback" => Self::AraPlayback(match text("request")?.as_str() {
                "start" => AraPlayback::Start,
                "stop" => AraPlayback::Stop,
                "position" => AraPlayback::Position(
                    value.get("seconds").and_then(Value::as_f64).filter(|s| s.is_finite() && *s >= 0.0)?,
                ),
                _ => return None,
            }),
            "close_editor" => Self::CloseEditor { instance: instance()? },
            "editor_closed" => Self::EditorClosed { instance: instance()? },
            "quit" => Self::Quit,
            _ => return None,
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    let (pairs, rest) = text.as_bytes().as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    let digit = |c: u8| (c as char).to_digit(16);
    pairs.iter().map(|[high, low]| Some((digit(*high)? * 16 + digit(*low)?) as u8)).collect()
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
