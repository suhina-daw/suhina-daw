// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The command registry. Each command is a [`CommandSpec`] in the module of its namespace
//! (`commands/clip.rs` holds `clip.*`), next to the functions that run it; a new namespace's
//! module is added to [`NAMESPACES`]. Tests are in `crates/session/tests/`.

use serde_json::{Value, json};
use suhina_engine::{ColumnStatus, LaunchCommand, MAX_START_BEAT, Quantization, Queued};
use suhina_model::{
    ArrangementLane, Channel, ChannelId, ChannelRole, Clip, ClipContent, ClipId, ClipPlace, FadeCurve, FadeShape,
    LaneData, LoopRange, MAX_GROUP_DEPTH, Marker, MidiPort, Project, Scene, Slot, TimeSignature, Track, TrackId,
};

use crate::{
    CommandError, Session, device_groups, devices, editing, files, follow, launch, mapping, meters, note_tools, notes,
};

mod args;
mod automation;
mod branch;
mod clip;
mod device;
mod history;
mod launcher;
mod marker;
mod mixer;
mod note;
mod preview;
mod project;
mod range;
mod registry;
mod returns;
mod scene;
mod tempo;
mod time_signature;
mod track;
mod transport;

pub(crate) use args::Args;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    /// Reads the project; never changes it.
    Read,
    /// Changes the project and records one undo step.
    Edit,
    /// Moves through the undo history.
    History,
    /// Controls playback in the attached audio engine (transport, clip launching); never
    /// changes the project or its undo history.
    Transport,
    /// Opens, switches or closes whole projects (the open projects, like tabs); never changes a
    /// project or its undo history. Runs on [`crate::Projects`], not on one session.
    Projects,
    /// Scans for plug-ins or changes where scans look (ADR 0006); belongs to Suhina, not to
    /// one project. Runs on [`crate::Projects`].
    Plugins,
    /// Acts on one of the project's running plug-ins (opens or closes its editor, restarts
    /// it); never changes the project or its undo history.
    Plugin,
    /// The browser's library: folders, search, pins, recent items, tags (ADR 0012); belongs
    /// to Suhina, not to one project. Runs on [`crate::Projects`].
    Library,
    /// Lists or chooses the audio output (host, device, sample rate, buffer size) or the MIDI
    /// inputs, or reports on them; belongs to Suhina, not to one project. Runs on
    /// [`crate::Projects`].
    Audio,
}

impl ParamType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::Integer => "integer",
            Self::Bool => "bool",
            Self::Track => "track",
            Self::Decibels => "decibels",
            Self::AutomationValue => "automation value",
            Self::Text => "text",
            Self::Tracks => "tracks",
            Self::Texts => "texts",
            Self::Integers => "integers",
            Self::Notes => "notes",
            Self::Numbers => "numbers",
            Self::Clips => "clips",
            Self::Range => "range",
            Self::Clip => "clip",
            Self::DevicePath => "device path",
            Self::NameOrNumber => "name or number",
            Self::Path(PathKind::File) => "file",
            Self::Path(PathKind::Files) => "files",
            Self::Path(PathKind::Folder) => "folder",
            Self::Path(PathKind::WriteFile) => "file to write",
            Self::Path(PathKind::WriteFolder) => "folder to write",
            Self::Path(PathKind::LibraryItem) => "library item",
            Self::Path(PathKind::Place) => "place",
        }
    }
}

impl ParamType {
    /// The type as an error message says what a value must be: "a whole number".
    pub fn phrase(self) -> &'static str {
        match self {
            Self::Number => "a number",
            Self::Integer => "a whole number",
            Self::Bool => "true or false",
            Self::Track => "a track id or name",
            Self::Decibels => "a number of decibels or \"-inf\"",
            Self::AutomationValue => "a number, \"-inf\", or true or false",
            Self::Text => "a text",
            Self::Tracks => "a list of track ids or names",
            Self::Texts | Self::Path(PathKind::Files) => "a list of texts",
            Self::Integers => "a list of whole numbers",
            Self::Notes => "a list of notes",
            Self::Numbers => "a list of numbers",
            Self::Clips => "a list of clips",
            Self::Range => "a range, {start, end}",
            Self::Clip => "a clip id or name",
            Self::DevicePath => "a device index or path",
            Self::NameOrNumber => "a name or a number",
            Self::Path(_) => "a path",
        }
    }

    /// Whether `value` has this type's JSON shape: a number for a number, a list for a list. A
    /// `Clips` item may be an object or a bare clip id.
    pub fn accepts(self, value: &Value) -> bool {
        let id_or_name = |v: &Value| v.is_u64() || v.is_i64() || v.is_string();
        match self {
            Self::Number => value.is_number(),
            Self::Integer => value.is_i64() || value.is_u64() || value.as_f64().is_some_and(|n| n.fract() == 0.0),
            Self::Bool => value.is_boolean(),
            Self::Track | Self::Clip | Self::DevicePath => id_or_name(value),
            Self::NameOrNumber => value.is_number() || value.is_string(),
            Self::Decibels => value.is_number() || value.is_string(),
            Self::AutomationValue => value.is_number() || value.is_string() || value.is_boolean(),
            Self::Text
            | Self::Path(PathKind::File | PathKind::Folder | PathKind::WriteFile)
            | Self::Path(PathKind::WriteFolder | PathKind::LibraryItem | PathKind::Place) => value.is_string(),
            Self::Tracks | Self::Texts | Self::Integers | Self::Notes | Self::Numbers | Self::Clips => value.is_array(),
            Self::Path(PathKind::Files) => value.is_array(),
            Self::Range => value.is_object(),
        }
    }
}

impl CommandKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Edit => "edit",
            Self::History => "history",
            Self::Transport => "transport",
            Self::Projects => "projects",
            Self::Plugins => "plugins",
            Self::Plugin => "plugin",
            Self::Library => "library",
            Self::Audio => "audio",
        }
    }
}

/// How a command runs: on one session, or (for [`CommandKind::Projects`]) on the open projects.
pub type Run<T> = fn(&mut T, &Value) -> Result<Value, CommandError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    Number,
    Integer,
    Bool,
    /// A track id (integer) or a track name (string).
    Track,
    /// A number, or the string "-inf".
    Decibels,
    /// A number, the string "-inf", or a bool: the value of an automation point.
    AutomationValue,
    Text,
    /// A list of tracks, each an id or a name.
    Tracks,
    /// A list of texts (file paths).
    Texts,
    /// A list of whole numbers (note indices).
    Integers,
    /// A list of new notes, each `{time, length, key, velocity?}`.
    Notes,
    /// A list of numbers.
    Numbers,
    /// A list of arrangement clips, each `{ "clip": id }` or (deprecated) `{ "track": …, "index": n }`.
    Clips,
    /// A range of beats, `{start, end}` from the arrangement's start.
    Range,
    /// A clip id (integer, from `project.clips` or `project.launcher`) or a clip name (string) that
    /// only one clip has.
    Clip,
    /// A device on a track: its index (an integer), or a path through device groups as text,
    /// `"2/1/0"` (docs/spec/device-groups.md).
    DevicePath,
    /// A name, or a number standing for it: a scale's root, "C#" or 1.
    NameOrNumber,
    /// A place on disk the command reads or writes, as text (a list of them for
    /// [`PathKind::Files`]). An agent's is checked against its roots before the command runs
    /// (`suhina-automation`'s `confine`), by this kind.
    Path(PathKind),
}

/// What a [`ParamType::Path`] parameter names on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    /// A file the command reads.
    File,
    /// A list of files the command reads.
    Files,
    /// A folder the command reads.
    Folder,
    /// A file the command writes.
    WriteFile,
    /// A folder the command writes into.
    WriteFolder,
    /// A library item: a file (`<bank>.syx#<voice>` for a voice in a bank), or `device:<id>` or
    /// `plugin:<spec>`, which are not on disk.
    LibraryItem,
    /// A place in the library: a folder, `pinned` or `recent:<group>`.
    Place,
}

#[derive(Debug, Clone, Copy)]
pub struct Param {
    pub name: &'static str,
    pub ty: ParamType,
    pub required: bool,
    pub doc: &'static str,
}

pub(crate) const fn param(name: &'static str, ty: ParamType, doc: &'static str) -> Param {
    Param { name, ty, required: true, doc }
}

pub(crate) const fn optional(name: &'static str, ty: ParamType, doc: &'static str) -> Param {
    Param { name, ty, required: false, doc }
}

const TRACK: Param = param("track", ParamType::Track, "track id or name, from project.tracks");
/// Lets the calls of one drag make one undo step.
const CONTINUE: Param = optional(
    "continue",
    ParamType::Bool,
    "true adds the change to the undo step of the previous call of this command on this track, \
     when no other edit came between (one drag, one step)",
);
const TRACKS: Param = param("tracks", ParamType::Tracks, "the tracks to act on (ids or names), at least one");
const RANGE_START: Param =
    param("start", ParamType::Number, "where the time selection starts, in beats from the beginning, at least 0");
const RANGE_END: Param = param("end", ParamType::Number, "where the time selection ends, in beats, after start");
const DEVICE: Param = param(
    "device",
    ParamType::DevicePath,
    "device index on the track, from 0, or a path through device groups such as \"2/1/0\" (device 2, its branch 1, device 0 there), as in track.devices",
);
const GROUP: Param = param("device", ParamType::DevicePath, "a device group: its index or path, as in track.devices");
const BRANCH: Param = param("branch", ParamType::Integer, "branch index in the group, from 0, as in track.devices");
const POSITION: Param = optional(
    "position",
    ParamType::DevicePath,
    "where in the chain, from 0, or inside a group as a path such as \"2/1/3\" (branch 1 of the group at 2, before its device 3); the end when left out",
);
const DEVICES: Param = param(
    "devices",
    ParamType::Integers,
    "device indices on the track, from 0, or paths through groups such as \"2/1/0\", as in track.devices; at least one",
);
const RETURN: Param = param("return", ParamType::Track, "return track id or name, from project.tracks");
const SCENE: Param = param("scene", ParamType::Integer, "scene number, from 1, as in project.launcher");
/// A clip command's clip by its id (ADR 0025): it stays the same when other clips move.
const CLIP: Param = optional(
    "clip",
    ParamType::Clip,
    "the clip: its id, from project.clips or project.launcher, or a name only one clip has; instead of track with index or scene",
);
/// The clip's track, for the positional forms `index` and `scene`; not needed with `clip`.
const CLIP_TRACK: Param =
    optional("track", ParamType::Track, "with index or scene: the clip's track, id or name (not needed with clip)");
/// A note command's clip: an arrangement clip by index, or a launcher clip by scene.
const CLIP_INDEX: Param = optional(
    "index",
    ParamType::Integer,
    "an arrangement clip's index on the track, from project.clips (deprecated, use clip; accepted until the project format's next version (ADR 0025))",
);
const CLIP_SCENE: Param =
    optional("scene", ParamType::Integer, "instead of index: the clip in the track's slot of this scene, from 1");
/// A scale's root and mode, for `clip.setScale` and `project.setDefaultScale`.
const SCALE_ROOT: Param = optional(
    "root",
    ParamType::NameOrNumber,
    "the key the scale starts on: a name such as C, F# or Bb, or 0 to 11 (default: the scale's root now, else C)",
);
const SCALE_MODE: Param = optional(
    "mode",
    ParamType::Text,
    "major, minor, harmonicMinor, melodicMinor, dorian, phrygian, lydian, mixolydian, locrian, majorPentatonic, minorPentatonic or blues (default: the scale's mode now, else major)",
);
const NOTES: Param = param("notes", ParamType::Integers, "note indices, from note.list; at most 10000");
/// The notes a MIDI tool acts on: the named ones, or every note of the clip.
const NOTE_SELECTION: Param =
    optional("notes", ParamType::Integers, "note indices, from note.list; every note when left out");
const AUTOMATED: Param = param(
    "param",
    ParamType::Text,
    "the parameter: volume, pan or mute of the track's mixer (no device), or a device's parameter id, from device.list (a plug-in's from device.pluginParams)",
);
const AUTOMATED_DEVICE: Param = optional(
    "device",
    ParamType::Integer,
    "device index on the track, from 0, as in track.devices; left out for the track's mixer",
);
const POINT: Param =
    param("point", ParamType::Integer, "point index on the lane, from 0 in beat order, as in automation.list");
const CURVE: Param = optional(
    "curve",
    ParamType::Number,
    "shape of the segment to the next point, -1 to 1: 0 a straight line (the default), above 0 fast first, below 0 slow first",
);
const MARKER: Param = param("index", ParamType::Integer, "marker index, from 0, as in project.markers");
/// `clip.delete` takes a list of arrangement clips, or one clip by `track` with `index` or `scene`.
const DELETE_CLIPS: Param = optional(
    "clips",
    ParamType::Clips,
    "arrangement clips, each { \"clip\": id } as project.clips lists them (or, deprecated, \
     { \"track\": id or name, \"index\": n }); instead of clip, or track with index or scene",
);
const DELETE_TRACK: Param =
    optional("track", ParamType::Track, "with index or scene instead of clip or clips: the clip's track, id or name");
const CLIPS: Param = param(
    "clips",
    ParamType::Clips,
    "the clips to act on, each { \"clip\": id } as project.clips lists them (or, deprecated, \
     { \"track\": id or name, \"index\": n }); at least one",
);

pub struct CommandSpec<T: 'static = Session> {
    /// Stable id in `area.verb` form; part of the public interface for agents.
    pub id: &'static str,
    /// One line saying what the command does.
    pub label: &'static str,
    pub params: &'static [Param],
    pub kind: CommandKind,
    pub run: Run<T>,
    /// What else follows from running it, and who may (#764 › 1): kept here, with the command,
    /// rather than in lists of ids elsewhere.
    pub policy: Policy,
}

/// The rules a command carries beside what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// An edit the player hears, so the playing graph is rebuilt after it. `false` for an edit
    /// that changes only what is shown (a name, a colour, a marker): the graph is kept.
    pub heard: bool,
    /// Hard to undo or slow to redo, so a backup is written just before it when backups are set
    /// to critical operations (`docs/spec/backups.md`).
    pub critical: bool,
    /// Whether an agent may run it over MCP (ADR 0028).
    pub agents: Agents,
}

impl Policy {
    /// Heard, not critical, open to agents.
    pub const DEFAULT: Self = Self { heard: true, critical: false, agents: Agents::Allowed };
    /// An edit only seen: a name, a colour, a marker.
    pub const UNHEARD: Self = Self { heard: false, ..Self::DEFAULT };
    /// Backed up before it runs.
    pub const CRITICAL: Self = Self { critical: true, ..Self::DEFAULT };
}

/// Who may run a command over MCP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agents {
    Allowed,
    /// Never: what it changes is the person's choice, said in the reason.
    Denied(&'static str),
    /// Not by an agent attached to the app's open tabs, which works on projects; the reason
    /// says why, usually [`Agents::SETTINGS`].
    NotWhenAttached(&'static str),
}

impl Agents {
    /// Why most commands an attached agent may not run are closed to it.
    pub const SETTINGS: &'static str =
        "changes the app's settings or the person's library; agents attached to the app work on projects";
}

// By hand: a derive would ask `T: Copy` of the session the command runs on.
impl<T> Clone for CommandSpec<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for CommandSpec<T> {}

impl<T> CommandSpec<T> {
    /// The parameters as a JSON Schema object, for MCP tool listings.
    pub fn input_schema(&self) -> Value {
        let mut properties = serde_json::Map::new();
        for p in self.params {
            if matches!(
                p.ty,
                ParamType::Tracks
                    | ParamType::Texts
                    | ParamType::Integers
                    | ParamType::Notes
                    | ParamType::Numbers
                    | ParamType::Clips
                    | ParamType::Path(PathKind::Files)
            ) {
                let items = match p.ty {
                    ParamType::Tracks => json!({ "type": ["integer", "string"] }),
                    ParamType::Integers => json!({ "type": "integer", "minimum": 0 }),
                    ParamType::Notes => json!({
                        "type": "object",
                        "properties": {
                            "time": { "type": "number", "description": "start in beats from the clip's start" },
                            "length": { "type": "number", "description": "length in beats, above 0" },
                            "key": { "type": "integer", "description": "MIDI key 0 to 127; 60 is C3" },
                            "velocity": { "type": "number", "description": "1 to 127 (default 100)" },
                        },
                        "required": ["time", "length", "key"],
                        "additionalProperties": false,
                    }),
                    ParamType::Numbers => json!({ "type": "number" }),
                    ParamType::Clips => json!({
                        "type": "object",
                        "properties": {
                            "clip": { "type": ["integer", "string"], "description": "the clip's id, or a name only one clip has" },
                            "track": { "type": ["integer", "string"], "description": "deprecated, with index: the clip's track" },
                            "index": { "type": "integer", "description": "deprecated, with track: the clip's index on it" },
                        },
                        "additionalProperties": false,
                    }),
                    _ => json!({ "type": "string" }),
                };
                properties.insert(p.name.to_string(), json!({ "type": "array", "items": items, "description": p.doc }));
                continue;
            }
            let ty = match p.ty {
                ParamType::Tracks
                | ParamType::Texts
                | ParamType::Integers
                | ParamType::Notes
                | ParamType::Numbers
                | ParamType::Clips
                | ParamType::Path(PathKind::Files) => json!("array"),
                ParamType::Number => json!("number"),
                ParamType::Integer => json!("integer"),
                ParamType::Bool => json!("boolean"),
                ParamType::AutomationValue => json!(["number", "string", "boolean"]),
                ParamType::Track | ParamType::Clip | ParamType::DevicePath => json!(["integer", "string"]),
                ParamType::Decibels | ParamType::NameOrNumber => json!(["number", "string"]),
                ParamType::Range => json!("object"),
                ParamType::Text | ParamType::Path(_) => json!("string"),
            };
            properties.insert(p.name.to_string(), json!({ "type": ty, "description": p.doc }));
        }
        let required: Vec<&str> = self.params.iter().filter(|p| p.required).map(|p| p.name).collect();
        json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
    }

    /// The command as one row of the table in `docs/commands.md`.
    pub fn markdown_row(&self) -> String {
        let cell = |text: &str| text.replace('|', r"\|");
        let params: Vec<String> = self
            .params
            .iter()
            .map(|p| {
                let optional = if p.required { "" } else { ", optional" };
                format!("`{}` ({}{optional}): {}", p.name, p.ty.name(), cell(p.doc))
            })
            .collect();
        let params = if params.is_empty() { "none".to_string() } else { params.join("; ") };
        format!("| `{}` | {} | {} | {params} |", self.id, self.kind.name(), cell(self.label))
    }

    /// The command as `command.list` and the MCP server's `command_list` show it.
    pub fn describe(&self) -> Value {
        json!({ "id": self.id, "label": self.label, "kind": self.kind.name(), "params": self.input_schema() })
    }
}

/// The one check every command's parameters pass before it runs, on a session or on the open
/// projects: an object (or nothing), no key the command does not take, every required one there,
/// and each of the type its [`Param`] says. A `null` stands for an optional parameter left out.
/// What a value means (a track that exists, a range in order) is the command's to check.
pub(crate) fn check_params(id: &str, expected: &[Param], params: &Value) -> Result<(), CommandError> {
    if !(params.is_object() || params.is_null()) {
        return Err(format!("{id}: parameters must be a JSON object").into());
    }
    let empty = serde_json::Map::new();
    let object = params.as_object().unwrap_or(&empty);
    if let Some(unknown) = object.keys().find(|key| !expected.iter().any(|p| p.name == key.as_str())) {
        let known: Vec<&str> = expected.iter().map(|p| p.name).collect();
        return Err(format!("{id}: unknown parameter {unknown:?}; expected {known:?}").into());
    }
    for param in expected {
        match object.get(param.name) {
            None | Some(Value::Null) if param.required => {
                return Err(format!("{id}: {} must be given: {}", param.name, param.doc).into());
            }
            None | Some(Value::Null) => {}
            Some(value) if !param.ty.accepts(value) => {
                return Err(format!("{id}: {} must be {}, not {value}", param.name, param.ty.phrase()).into());
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// Every command, in the order `command.list` shows them.
pub fn commands() -> &'static [CommandSpec] {
    static ALL: std::sync::OnceLock<Vec<CommandSpec>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| NAMESPACES.iter().flat_map(|commands| commands.iter().copied()).collect())
}

/// Every command on one project, namespace by namespace: each module registers its own.
static NAMESPACES: &[&[CommandSpec]] = &[
    registry::COMMANDS,
    project::COMMANDS,
    clip::COMMANDS,
    transport::COMMANDS,
    tempo::COMMANDS,
    time_signature::COMMANDS,
    track::COMMANDS,
    mixer::COMMANDS,
    range::COMMANDS,
    marker::COMMANDS,
    launcher::COMMANDS,
    scene::COMMANDS,
    device::COMMANDS,
    preview::COMMANDS,
    branch::COMMANDS,
    automation::COMMANDS,
    returns::COMMANDS,
    note::COMMANDS,
    history::COMMANDS,
];

fn list(_: &mut Session, _: &Value) -> Result<Value, CommandError> {
    Ok(Value::Array(
        commands()
            .iter()
            .map(|c| {
                json!({ "id": c.id, "label": c.label, "params": c.input_schema()["properties"], "kind": c.kind.name() })
            })
            .collect(),
    ))
}

fn info(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let p = session.project();
    let arrangement_clips: usize = p.arrangement.lanes.iter().map(|l| l.clips.len()).sum();
    let launcher_clips = p.launcher.columns.iter().flat_map(|c| &c.slots).filter(|s| s.clip.is_some()).count();
    Ok(json!({
        "source": session.source().map(|s| s.display().to_string()),
        "unsaved_changes": session.has_unsaved_changes(),
        "tempo_bpm": p.transport.tempo_bpm,
        "time_signature": format!("{}/{}", p.transport.time_signature.numerator, p.transport.time_signature.denominator),
        "tracks": p.tracks.len(),
        "scenes": p.launcher.scenes.len(),
        "arrangement_clips": arrangement_clips,
        "launcher_clips": launcher_clips,
        "markers": p.arrangement.markers.len(),
        "loop": loop_value(p),
        "default_scale": crate::scales::scale_value(p.default_scale),
        "can_undo": session.can_undo(),
        "can_redo": session.can_redo(),
    }))
}

fn tracks(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let p = session.project();
    Ok(Value::Array(
        p.tracks
            .iter()
            .map(|t| {
                let channel = channel_of(p, t);
                let clips =
                    p.arrangement.lanes.iter().filter(|l| l.track == Some(t.id)).map(|l| l.clips.len()).sum::<usize>();
                json!({
                    "id": t.id.0,
                    "name": t.name,
                    "plays": if t.content.notes { "notes" } else if t.content.audio { "audio" } else { "none" },
                    "volume_db": channel.map(|c| to_db(c.volume)),
                    "pan": channel.map(|c| c.pan),
                    "mute": channel.map(|c| c.mute),
                    "armed": session.is_armed(t.id),
                    "midi_input": midi_input_json(&t.midi_input),
                    "audio_input": audio_input_json(&t.audio_input),
                    "devices": channel.map(|c| c.devices.iter().map(|d| d.name.clone()).collect::<Vec<_>>()),
                    "sends": channel.map(|c| crate::sends::sends_json(p, c)),
                    "arrangement_clips": clips,
                })
            })
            .collect(),
    ))
}

fn clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = session.project();
    let only = match params.get("track") {
        Some(_) => Some(track_id(p, params)?),
        None => None,
    };
    let mut out = Vec::new();
    for lane in &p.arrangement.lanes {
        let Some(track) = lane.track else { continue };
        if only.is_some_and(|t| t != track) {
            continue;
        }
        let name = p.track(track).map(|t| t.name.as_str()).unwrap_or_default();
        for (index, clip) in lane.clips.iter().enumerate() {
            out.push(clip_json(p, name, track, index, clip));
        }
    }
    Ok(Value::Array(out))
}

fn clip_json(p: &Project, track_name: &str, track: TrackId, index: usize, clip: &Clip) -> Value {
    let content = p.content(clip.content);
    let (notes, audio) = content.map_or((0, None), |c| {
        c.lanes.iter().fold((0, None), |(n, a), lane| match &lane.data {
            LaneData::Notes(notes) => (n + notes.len(), a),
            LaneData::Audio(region) => (n, Some(region)),
        })
    });
    json!({
        "id": clip.id.0,
        "track": track.0,
        "track_name": track_name,
        "index": index,
        "name": content.map(|c| c.name.as_str()),
        "start": clip.start,
        "length": clip.length,
        "enabled": clip.enabled,
        "fade_in": clip.fade_in,
        "fade_out": clip.fade_out,
        "fade_in_shape": clip.fade_in_curve.shape.name(),
        "fade_in_curve": clip.fade_in_curve.curve,
        "fade_out_shape": clip.fade_out_curve.shape.name(),
        "fade_out_curve": clip.fade_out_curve.curve,
        "notes": notes,
        "audio_file": audio.map(|r| r.file.as_str()),
        "audio_stream": audio.and_then(|r| r.stream),
    })
}

/// The loop region and whether it is on, as `transport.setLoop` and `project.info` report it.
fn loop_value(p: &Project) -> Value {
    match p.transport.loop_range {
        Some(range) => json!({ "start": range.start, "end": range.end, "enabled": p.transport.loop_enabled }),
        None => Value::Null,
    }
}

/// `params.enabled`: true or false, or `None` when left out.
fn optional_enabled(params: &Value) -> Result<Option<bool>, CommandError> {
    match params.get("enabled") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or_else(|| "enabled must be true or false".into()),
    }
}

fn set_loop(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (start, end) = (number(params, "start")?, number(params, "end")?);
    if !(0.0..MAX_START_BEAT).contains(&start) {
        return Err(format!("start must be from 0 and before {MAX_START_BEAT}, got {start}").into());
    }
    if !(end > start && end <= MAX_START_BEAT) {
        return Err(format!("end must be after start ({start}) and at most {MAX_START_BEAT}, got {end}").into());
    }
    let enabled = optional_enabled(params)?;
    let transport = &mut session.project.transport;
    transport.loop_range = Some(LoopRange { start, end });
    if let Some(enabled) = enabled {
        transport.loop_enabled = enabled;
    }
    Ok(loop_value(&session.project))
}

/// A set command's answer, saying `"automation": "off"` when the change turned the parameter's
/// lane off (#395).
pub(crate) fn with_override(mut answer: Value, off: bool) -> Value {
    if off && let Some(map) = answer.as_object_mut() {
        map.insert("automation".into(), json!("off"));
    }
    answer
}

/// The loudest a track's volume can be, in dB, and the floor `track.adjustVolume` stops at
/// (silence starts below it).
pub(crate) const MAX_VOLUME_DB: f64 = 6.0;

const MIN_VOLUME_DB: f64 = -150.0;

/// A volume in dB as a linear gain: silence at or below [`MIN_VOLUME_DB`].
pub(crate) fn gain_of_db(db: f64) -> f64 {
    if db < MIN_VOLUME_DB { 0.0 } else { 10f64.powf(db / 20.0) }
}

/// The ids in `params.tracks`, each resolved like a track parameter.
pub(crate) fn track_list(p: &Project, params: &Value) -> Result<Vec<TrackId>, CommandError> {
    let list = params.get("tracks").and_then(Value::as_array).ok_or("tracks must be a list of track ids or names")?;
    let mut ids = Vec::new();
    for value in list {
        let id = track_at(p, &json!({ "track": value }), "track")?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return Err("tracks must name at least one track".into());
    }
    Ok(ids)
}

fn channel_id_of(p: &Project, track: TrackId) -> Option<ChannelId> {
    p.track(track).and_then(|t| t.channel)
}

pub(crate) fn group_tracks(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let members = track_list(p, params)?;
    let parents: Vec<Option<TrackId>> = members.iter().map(|id| p.track(*id).and_then(|t| t.parent)).collect();
    let parent = parents.first().copied().flatten();
    if parents.iter().any(|x| *x != parent) {
        return Err("the tracks to group must all be in the same group, or in none".into());
    }
    // The new group adds a level above every track it takes.
    let deepest = members.iter().map(|&m| p.group_depth(m).unwrap_or(0) + p.nesting_below(m)).max().unwrap_or(0);
    if deepest + 1 > MAX_GROUP_DEPTH {
        return Err(format!("groups nest at most {MAX_GROUP_DEPTH} deep; this grouping would go deeper").into());
    }
    let name = match params.get("name") {
        None => "Group".to_string(),
        Some(v) => v.as_str().filter(|n| !n.trim().is_empty()).ok_or("name must be a text, not empty")?.to_string(),
    };
    let id = TrackId(p.tracks.iter().map(|t| t.id.0).max().unwrap_or(0) + 1);
    let channel_id = ChannelId(p.channels.iter().map(|c| c.id.0).max().unwrap_or(0) + 1);
    let output = parent.and_then(|t| channel_id_of(p, t));
    p.channels.push(Channel {
        id: channel_id,
        name: name.clone(),
        role: ChannelRole::Submix,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        solo: false,
        cue: false,
        output,
        sends: Vec::new(),
        devices: Vec::new(),
        extensions: Default::default(),
        automation: Vec::new(),
    });
    // The group takes the place of its first track; its tracks (with theirs) follow it.
    let family: Vec<TrackId> = members.iter().flat_map(|m| p.with_descendants(*m)).collect();
    let first = p.tracks.iter().position(|t| family.contains(&t.id)).unwrap_or(p.tracks.len());
    let moved: Vec<Track> = p.tracks.iter().filter(|t| family.contains(&t.id)).cloned().collect();
    p.tracks.retain(|t| !family.contains(&t.id));
    let at = first.min(p.tracks.len());
    let group = Track {
        id,
        name,
        color: None,
        content: Default::default(),
        channel: Some(channel_id),
        parent,
        midi_input: Default::default(),
        audio_input: Default::default(),
        extensions: Default::default(),
    };
    p.tracks.splice(at..at, std::iter::once(group).chain(moved));
    for track in p.tracks.iter_mut().filter(|t| members.contains(&t.id)) {
        track.parent = Some(id);
    }
    let member_channels: Vec<ChannelId> = members.iter().filter_map(|m| channel_id_of(p, *m)).collect();
    for channel in p.channels.iter_mut().filter(|c| member_channels.contains(&c.id)) {
        channel.output = Some(channel_id);
    }
    Ok(json!({ "track": id.0 }))
}

/// Removes a track, its channel, its lanes and slots, and every reference to them.
pub(crate) fn remove_track(p: &mut Project, track: TrackId, channel: ChannelId) {
    p.tracks.retain(|t| t.id != track);
    p.channels.retain(|c| c.id != channel);
    for c in &mut p.channels {
        if c.output == Some(channel) {
            c.output = None;
        }
        c.sends.retain(|s| s.target != channel);
    }
    p.arrangement.lanes.retain(|l| l.track != Some(track));
    p.launcher.columns.retain(|c| c.track != track);
    for content in &mut p.contents {
        content.lanes.retain(|l| l.target != Some(track));
    }
}

/// The clips `params.clips` names, each `{ "track": …, "index": n }` as `project.clips` lists
/// them: at least one, without duplicates, every one of them there.
pub(crate) fn clip_list(p: &Project, params: &Value) -> Result<Vec<(TrackId, usize)>, CommandError> {
    let list = params.get("clips").and_then(Value::as_array).ok_or("clips must be a list of {track, index}")?;
    let mut out: Vec<(TrackId, usize)> = Vec::new();
    for entry in list {
        let track = track_at(p, entry, "track")?;
        let index = entry.get("index").and_then(Value::as_u64).ok_or("each clip needs an index from project.clips")?;
        let lane = p
            .arrangement
            .lanes
            .iter()
            .position(|l| l.track == Some(track))
            .ok_or("the track has no arrangement lane")?;
        let count = p.arrangement.lanes.get(lane).map_or(0, |l| l.clips.len());
        let index = usize::try_from(index)
            .ok()
            .filter(|i| *i < count)
            .ok_or_else(|| format!("clip index {index} does not exist; the track has {count} arrangement clips"))?;
        if !out.contains(&(track, index)) {
            out.push((track, index));
        }
    }
    if out.is_empty() {
        return Err("clips must name at least one clip".into());
    }
    Ok(out)
}

/// The lane of `params.track` and the clip index in it, checked.
pub(crate) fn lane_and_index(p: &Project, params: &Value) -> Result<(usize, usize), CommandError> {
    let track = track_id(p, params)?;
    let index = params.get("index").and_then(Value::as_u64).ok_or("index must be a whole number from project.clips")?;
    let lane =
        p.arrangement.lanes.iter().position(|l| l.track == Some(track)).ok_or("the track has no arrangement lane")?;
    let count = p.arrangement.lanes.get(lane).map_or(0, |l| l.clips.len());
    let index = usize::try_from(index)
        .ok()
        .filter(|i| *i < count)
        .ok_or_else(|| format!("clip index {index} does not exist; the track has {count} arrangement clips"))?;
    Ok((lane, index))
}

/// Where a clip an edit command names is: on the arrangement (`index`) or in a launcher slot
/// (`scene`).
#[derive(Debug, Clone, Copy)]
enum ClipAt {
    Arrangement { lane: usize, index: usize },
    Slot { column: usize, scene: usize },
}

impl ClipAt {
    fn get(self, p: &Project) -> Option<&Clip> {
        match self {
            Self::Arrangement { lane, index } => p.arrangement.lanes.get(lane)?.clips.get(index),
            Self::Slot { column, scene } => p.launcher.columns.get(column)?.slots.get(scene)?.clip.as_ref(),
        }
    }

    fn get_mut(self, p: &mut Project) -> Option<&mut Clip> {
        match self {
            Self::Arrangement { lane, index } => p.arrangement.lanes.get_mut(lane)?.clips.get_mut(index),
            Self::Slot { column, scene } => p.launcher.columns.get_mut(column)?.slots.get_mut(scene)?.clip.as_mut(),
        }
    }
}

/// An id no clip content of `p` has yet.
pub(crate) fn new_content_id(p: &Project) -> Result<suhina_model::ContentId, CommandError> {
    let id = p.contents.iter().map(|c| c.id.0).max().unwrap_or(0).checked_add(1).ok_or("too many clips")?;
    Ok(suhina_model::ContentId(id))
}

/// `clip.renameMany {clips, name}` (#693).
fn rename_clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let name = match params.get("name") {
        Some(Value::String(name)) => name.trim().to_string(),
        _ => return Err("name must be a text (empty clears it)".into()),
    };
    let p = &mut session.project;
    let mut contents = Vec::new();
    for (track, index) in clip_list(p, params)? {
        let clip = p
            .arrangement
            .lanes
            .iter()
            .find(|l| l.track == Some(track))
            .and_then(|l| l.clips.get(index))
            .ok_or("the clip is missing")?;
        contents.push(clip.content);
    }
    for content in p.contents.iter_mut().filter(|c| contents.contains(&c.id)) {
        content.name.clone_from(&name);
    }
    Ok(json!({ "name": name, "clips": contents.len() }))
}

/// A track's audio input as commands show it: `input` "none", "3" or "1/2", and `monitor`.
pub(crate) fn audio_input_json(input: &suhina_model::AudioInput) -> Value {
    use suhina_model::{InputChannels, Monitor};
    let channels = match input.channels {
        InputChannels::None => "none".to_string(),
        InputChannels::Mono(c) => c.to_string(),
        InputChannels::Stereo(c) => format!("{c}/{}", c.saturating_add(1)),
    };
    let monitor = match input.monitor {
        Monitor::Auto => "auto",
        Monitor::In => "in",
        Monitor::Off => "off",
    };
    json!({ "input": channels, "monitor": monitor })
}

/// A track's MIDI input as commands show it: `port` "all", "none" or the port's name, and
/// `channel` 1 to 16 or null for every one.
pub(crate) fn midi_input_json(input: &suhina_model::MidiInput) -> Value {
    let port = match &input.port {
        MidiPort::All => "all",
        MidiPort::Nothing => "none",
        MidiPort::Named(name) => name.as_str(),
    };
    let monitor = match input.monitor {
        suhina_model::Monitor::Auto => "auto",
        suhina_model::Monitor::In => "in",
        suhina_model::Monitor::Off => "off",
    };
    json!({ "port": port, "channel": input.channel, "monitor": monitor })
}

/// Clears `[from, to)` on a lane for a clip placed there (docs/spec/arrangement-editing.md):
/// clips entirely inside are removed, clips reaching in are shortened, and a clip around it is
/// split in two, the second part continuing its content from where it was. A split clip's
/// first part keeps its fade-in and the second its fade-out; fades are fitted to what is left.
/// The first part keeps the clip's id; the second is a new clip and gets an id when the command
/// is done ([`Project::assign_ids`]).
pub(crate) fn make_room(clips: &mut Vec<Clip>, from: f64, to: f64) {
    let mut kept = Vec::with_capacity(clips.len() + 1);
    for clip in clips.drain(..) {
        let (start, end) = (clip.start, clip.start + clip.length);
        if end <= from || start >= to {
            kept.push(clip);
            continue;
        }
        let split = start < from && end > to;
        if start < from {
            let mut first = Clip { length: from - start, ..clip.clone() };
            if split {
                first.fade_out = 0.0;
            }
            first.fit_fades();
            kept.push(first);
        }
        if end > to {
            let offset = content_at(&clip, to - start, 1.0);
            let mut second = Clip { start: to, length: end - to, offset, ..clip };
            if start < from {
                second.id = suhina_model::ClipId::NONE;
            }
            if split {
                second.fade_in = 0.0;
            }
            second.fit_fades();
            kept.push(second);
        }
    }
    *clips = kept;
}

/// Where in its content a clip is `beats` after it starts, moving `rate` content beats per beat
/// and following its loop, as the engine plays it: a start marker at or past the loop end starts
/// at the loop start.
pub(crate) fn content_at(clip: &Clip, beats: f64, rate: f64) -> f64 {
    let loop_range = clip.loop_range.filter(|r| r.start.is_finite() && r.end.is_finite() && r.end > r.start);
    let from = match loop_range {
        Some(r) if clip.offset >= r.end => r.start,
        _ => clip.offset,
    };
    let at = from + beats * rate;
    match loop_range {
        Some(r) if at >= r.end => r.start + (at - r.end).rem_euclid(r.end - r.start),
        _ => at,
    }
}

fn markers(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let markers = &session.project().arrangement.markers;
    Ok(Value::Array(
        markers
            .iter()
            .enumerate()
            .map(|(index, m)| json!({ "index": index, "beat": m.beat, "name": m.name }))
            .collect(),
    ))
}

fn send(session: &mut Session, command: LaunchCommand) -> Result<(), CommandError> {
    session.player_mut()?.send(command)
}

fn play(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    session.player_mut()?;
    let start = match params.get("from") {
        None => None,
        Some(Value::String(s)) if s == "marker" => None,
        Some(Value::String(s)) if s == "clip" => Some(clip_start(&session.project, params)?),
        Some(other) => return Err(format!("from must be \"marker\" or \"clip\", got {other}").into()),
    };
    if start.is_none() && (params.get("track").is_some() || params.get("index").is_some()) {
        return Err("track and index go with from: \"clip\"".into());
    }
    // The marker moves first, so playback starts there; while playing, that is a jump.
    if let Some(beat) = start {
        send(session, LaunchCommand::SetPosition { beat })?;
    }
    send(session, LaunchCommand::Play)?;
    Ok(match start {
        Some(beat) => json!({ "transport": "playing", "start_beat": beat }),
        None => json!({ "transport": "playing" }),
    })
}

/// Where the arrangement clip `track` and `index` name starts, in beats, as a play start marker.
fn clip_start(p: &Project, params: &Value) -> Result<f64, CommandError> {
    let (lane, index) = lane_and_index(p, params)?;
    let start = p.arrangement.lanes.get(lane).and_then(|l| l.clips.get(index)).ok_or("the clip is missing")?.start;
    if !(0.0..=MAX_START_BEAT).contains(&start) {
        return Err(format!("the clip starts at beat {start}, outside 0 … {MAX_START_BEAT}").into());
    }
    Ok(start)
}

fn stop(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let command = match params.get("return") {
        None => LaunchCommand::Stop,
        Some(Value::String(s)) if s == "marker" => LaunchCommand::Stop,
        Some(Value::String(s)) if s == "here" => LaunchCommand::StopHere,
        Some(other) => return Err(format!("return must be \"marker\" or \"here\", got {other}").into()),
    };
    // Takes end where the transport stops (docs/spec/recording.md).
    session.end_takes_now();
    send(session, command)?;
    Ok(json!({ "transport": "stopped" }))
}

fn signature_text(ts: TimeSignature) -> String {
    format!("{}/{}", ts.numerator, ts.denominator)
}

fn set_quantization(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let text = params.get("value").and_then(Value::as_str).ok_or("value must be a string such as \"1 bar\"")?;
    let quantization = parse_quantization(text)?;
    session.set_quantization(quantization)?;
    Ok(json!({ "quantization": text.trim() }))
}

/// Parses a launch quantisation: "none", "1 bar", "2 bars", "1/4", "1/8T" and so on.
pub fn parse_quantization(text: &str) -> Result<Quantization, CommandError> {
    let normal = text.trim().to_ascii_lowercase();
    match normal.as_str() {
        "none" | "off" => return Ok(Quantization::None),
        "8 bars" => return Ok(Quantization::Bars(8)),
        "4 bars" => return Ok(Quantization::Bars(4)),
        "2 bars" => return Ok(Quantization::Bars(2)),
        "1 bar" => return Ok(Quantization::Bars(1)),
        _ => {}
    }
    let (value, triplet) = match normal.strip_suffix('t') {
        Some(value) => (value, true),
        None => (normal.as_str(), false),
    };
    let beats = match (value, triplet) {
        ("1/2", _) => 2.0,
        ("1/4", _) => 1.0,
        ("1/8", _) => 0.5,
        ("1/16", _) => 0.25,
        ("1/32", false) => 0.125,
        _ => {
            return Err(format!(
                "unknown quantisation {text:?}; use none, 8 bars, 4 bars, 2 bars, 1 bar, 1/2, 1/2T, 1/4, 1/4T, 1/8, \
                 1/8T, 1/16, 1/16T or 1/32"
            )
            .into());
        }
    };
    Ok(Quantization::Beats(if triplet { beats * 2.0 / 3.0 } else { beats }))
}

/// The launcher column of `params.track`.
pub(crate) fn column_index(p: &Project, params: &Value) -> Result<usize, CommandError> {
    let track = track_id(p, params)?;
    p.launcher
        .columns
        .iter()
        .position(|c| c.track == track)
        .ok_or_else(|| format!("track {} has no clip slots", track_name(p, track)).into())
}

/// The 0-based scene index of the 1-based `params.scene`.
pub(crate) fn scene_index(p: &Project, params: &Value) -> Result<usize, CommandError> {
    let count = p.launcher.scenes.len();
    let number = params.get("scene").and_then(Value::as_u64).ok_or("scene must be a whole number from 1")?;
    usize::try_from(number)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .filter(|i| *i < count)
        .ok_or_else(|| format!("scene {number} does not exist; the project has {count} scenes").into())
}

fn track_name(p: &Project, track: TrackId) -> String {
    p.track(track).map_or_else(|| track.0.to_string(), |t| t.name.clone())
}

fn undo(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    if session.undo() { Ok(json!({ "undone": true })) } else { Err("nothing to undo".into()) }
}

/// Resolves `params.track`: a track id (number) or a name. Names match exactly first, then
/// ignoring case, and must be unambiguous.
pub(crate) fn track_id(p: &Project, params: &Value) -> Result<TrackId, CommandError> {
    track_at(p, params, "track")
}

/// Resolves the track id or name in `params[key]`.
pub(crate) fn track_at(p: &Project, params: &Value, key: &str) -> Result<TrackId, CommandError> {
    match params.get(key) {
        Some(Value::Number(n)) => {
            let id = n.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or("track id must be a whole number")?;
            p.tracks
                .iter()
                .find(|t| t.id.0 == id)
                .map(|t| t.id)
                .ok_or_else(|| format!("track {id} does not exist").into())
        }
        Some(Value::String(name)) => {
            if let Some(t) = p.tracks.iter().find(|t| t.name == *name) {
                return Ok(t.id);
            }
            let matches: Vec<&Track> = p.tracks.iter().filter(|t| t.name.eq_ignore_ascii_case(name)).collect();
            match matches.as_slice() {
                [t] => Ok(t.id),
                [] => Err(format!("no track named {name:?}; project.tracks lists them").into()),
                _ => Err(format!("several tracks are named {name:?}; use the track id").into()),
            }
        }
        _ => Err(format!("{key} must be a track id or name").into()),
    }
}

pub(crate) fn channel_param<'a>(session: &'a mut Session, params: &Value) -> Result<&'a mut Channel, CommandError> {
    let track = track_id(&session.project, params)?;
    let channel = session
        .project
        .tracks
        .iter()
        .find(|t| t.id == track)
        .and_then(|t| t.channel)
        .ok_or("the track has no mixer channel")?;
    session.project.channel_mut(channel).ok_or_else(|| "the track's channel is missing".into())
}

/// The read-only form of [`channel_param`], for the `Read` commands that do not edit.
pub(crate) fn channel_read<'a>(session: &'a Session, params: &Value) -> Result<&'a Channel, CommandError> {
    let track = track_id(&session.project, params)?;
    let channel = session
        .project
        .tracks
        .iter()
        .find(|t| t.id == track)
        .and_then(|t| t.channel)
        .ok_or("the track has no mixer channel")?;
    session.project.channel(channel).ok_or_else(|| "the track's channel is missing".into())
}

fn channel_of<'a>(p: &'a Project, track: &Track) -> Option<&'a Channel> {
    let id = track.channel?;
    p.channel(id)
}

pub(crate) fn number(params: &Value, name: &str) -> Result<f64, CommandError> {
    params
        .get(name)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("{name} must be a number").into())
}

/// Like [`number`], but also accepts the string "-inf".
pub(crate) fn number_or_neg_infinity(params: &Value, name: &str) -> Result<f64, CommandError> {
    match params.get(name) {
        Some(Value::String(s)) if s == "-inf" => Ok(f64::NEG_INFINITY),
        _ => number(params, name),
    }
}

/// Linear gain to decibels, rounded to 0.01 dB; `None` (JSON null) for silence.
pub(crate) fn to_db(gain: f64) -> Option<f64> {
    (gain > 0.0).then(|| (20.0 * gain.log10() * 100.0).round() / 100.0)
}

/// The clip id or name in `value`: an id from `project.clips` or `project.launcher`, or a name
/// (its content's) that only one clip has.
pub(crate) fn clip_param(p: &Project, value: &Value) -> Result<ClipId, CommandError> {
    match value {
        Value::Number(n) => {
            let id = n.as_u64().and_then(|n| u32::try_from(n).ok()).ok_or("a clip id is a whole number")?;
            let id = ClipId(id);
            p.clip_place(id)
                .map(|_| id)
                .ok_or_else(|| format!("clip {id} does not exist; project.clips lists them").into())
        }
        Value::String(name) => {
            let named = |exact: bool| -> Vec<ClipId> {
                let content_named = |c: &Clip| {
                    p.content(c.content)
                        .is_some_and(|c| if exact { c.name == *name } else { c.name.eq_ignore_ascii_case(name) })
                };
                p.all_clips().filter(|c| content_named(c)).map(|c| c.id).collect()
            };
            let exact = named(true);
            let matches = if exact.is_empty() { named(false) } else { exact };
            match matches.as_slice() {
                [id] => Ok(*id),
                [] => Err(format!("no clip is named {name:?}; project.clips lists them").into()),
                ids => {
                    let ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
                    Err(format!("several clips are named {name:?} (ids {}); use the clip id", ids.join(", ")).into())
                }
            }
        }
        _ => Err("clip must be a clip id or name".into()),
    }
}

/// The track a clip plays on and where the clip is.
fn clip_track(p: &Project, id: ClipId) -> Result<(TrackId, ClipPlace), CommandError> {
    let place = p.clip_place(id).ok_or_else(|| format!("clip {id} does not exist"))?;
    let track = match place {
        ClipPlace::Lane { lane, .. } => p.arrangement.lanes.get(lane).and_then(|l| l.track),
        ClipPlace::Slot { column, .. } => p.launcher.columns.get(column).map(|c| c.track),
    };
    let track = track.ok_or_else(|| format!("clip {id} is on a lane that belongs to no track"))?;
    Ok((track, place))
}

/// `params` with the clips it names by id (`clip`, and `clip` in the items of a `clips` list)
/// written as the commands read them: `track` with `index` (an arrangement clip) or `scene` (a
/// clip slot), looked up in the project as it is now, so a clip is found wherever earlier
/// edits moved it (ADR 0025). `None` when `params` names no clip by id.
///
/// During the deprecation window a command still takes `track` with `index`; that form is
/// removed with the project format's next version ([ADR 0025](../../docs/decisions/0025-object-identity.md) › 4).
pub(crate) fn resolve_clip_ids<T>(
    p: &Project,
    spec: &CommandSpec<T>,
    params: &Value,
) -> Result<Option<Value>, CommandError> {
    let takes = |name: &str| spec.params.iter().any(|q| q.name == name);
    let Some(object) = params.as_object() else { return Ok(None) };
    let given = |key: &str| object.get(key).filter(|v| !v.is_null());
    let single = given("clip").filter(|_| spec.params.iter().any(|q| q.ty == ParamType::Clip));
    let listed = spec.params.iter().any(|q| q.ty == ParamType::Clips)
        && given("clips").and_then(Value::as_array).is_some_and(|l| l.iter().any(|e| e.get("clip").is_some()));
    if single.is_none() && !listed {
        return Ok(None);
    }
    let mut out = object.clone();
    if let Some(value) = single {
        for key in ["index", "scene", "clips"] {
            if given(key).is_some() {
                return Err(format!("give clip or {key}, not both").into());
            }
        }
        let id = clip_param(p, value)?;
        let (track, place) = clip_track(p, id)?;
        if given("track").is_some() && track_id(p, params)? != track {
            return Err(format!("clip {id} is not on that track").into());
        }
        out.remove("clip");
        out.insert("track".into(), json!(track.0));
        match place {
            ClipPlace::Lane { index, .. } if takes("index") => {
                out.insert("index".into(), json!(index));
            }
            ClipPlace::Lane { index, .. } if takes("clips") => {
                out.remove("track");
                out.insert("clips".into(), json!([{ "track": track.0, "index": index }]));
            }
            ClipPlace::Slot { slot, .. } if takes("scene") => {
                out.insert("scene".into(), json!(slot + 1));
            }
            ClipPlace::Lane { .. } => {
                return Err(format!("clip {id} is on the arrangement; {} takes a clip slot's clip", spec.id).into());
            }
            ClipPlace::Slot { .. } => {
                return Err(format!("clip {id} is in a clip slot; {} takes arrangement clips", spec.id).into());
            }
        }
    }
    if listed && let Some(Value::Array(entries)) = out.get_mut("clips") {
        for entry in entries.iter_mut() {
            let Some(value) = entry.get("clip") else { continue };
            if entry.get("track").is_some() || entry.get("index").is_some() {
                return Err("each clip in clips is { \"clip\": id }, or { \"track\", \"index\" }, not both".into());
            }
            let id = clip_param(p, value)?;
            match clip_track(p, id)? {
                (track, ClipPlace::Lane { index, .. }) => *entry = json!({ "track": track.0, "index": index }),
                (_, ClipPlace::Slot { .. }) => {
                    return Err(format!("clip {id} is in a clip slot; clips names arrangement clips").into());
                }
            }
        }
    }
    Ok(Some(Value::Object(out)))
}
