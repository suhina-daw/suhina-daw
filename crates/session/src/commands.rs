// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The command registry. Each command is a [`CommandSpec`]; new commands are added to
//! [`COMMANDS`] with tests below.

use serde_json::{Value, json};
use suhina_engine::{ColumnStatus, LaunchCommand, MAX_START_BEAT, Quantization, Queued};
use suhina_model::{
    ArrangementLane, Channel, ChannelId, ChannelRole, Clip, ClipContent, ClipId, ClipPlace, FadeCurve, FadeShape,
    LaneData, LoopRange, MAX_GROUP_DEPTH, Marker, MidiPort, Project, Scene, Slot, TimeSignature, Track, TrackId,
};

use crate::{
    CommandError, Session, automation, device_groups, devices, editing, files, follow, launch, mapping, meters,
    note_tools, notes, preview, tempo,
};

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
            Self::Text => "text",
            Self::Tracks => "tracks",
            Self::Texts => "texts",
            Self::Integers => "integers",
            Self::Notes => "notes",
            Self::Numbers => "numbers",
            Self::Clips => "clips",
            Self::Clip => "clip",
            Self::DevicePath => "device path",
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
    /// A clip id (integer, from `project.clips` or `project.launcher`) or a clip name (string) that
    /// only one clip has.
    Clip,
    /// A device on a track: its index (an integer), or a path through device groups as text,
    /// `"2/1/0"` (docs/spec/device-groups.md).
    DevicePath,
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
    ParamType::Text,
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
    "the parameter: volume, pan or mute of the track's mixer (no device), or a device's parameter id, from devices.list (a plug-in's from device.pluginParams)",
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
}

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
                | ParamType::Clips => json!("array"),
                ParamType::Number => json!("number"),
                ParamType::Integer => json!("integer"),
                ParamType::Bool => json!("boolean"),
                ParamType::Track | ParamType::Clip | ParamType::DevicePath => json!(["integer", "string"]),
                ParamType::Decibels => json!(["number", "string"]),
                ParamType::Text => json!("string"),
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

    /// The command as `commands.list` and the MCP server's `command_list` show it.
    pub fn describe(&self) -> Value {
        json!({ "id": self.id, "label": self.label, "kind": self.kind.name(), "params": self.input_schema() })
    }
}

/// Every command, in the order `commands.list` shows them.
pub fn commands() -> &'static [CommandSpec] {
    COMMANDS
}

static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "commands.list",
        label: "List every command with its parameters",
        params: &[],
        kind: CommandKind::Read,
        run: list,
    },
    CommandSpec {
        id: "project.save",
        label: "Save the project as a Suhina project (.suhina) with its plug-ins' state as it is now; without a path, to the project file it was opened from or last saved to",
        params: &[
            optional("path", ParamType::Text, "file to save to; .suhina is added when missing"),
            optional(
                "copy",
                ParamType::Bool,
                "true writes a copy (a template, a backup) and the project stays with its own file",
            ),
        ],
        kind: CommandKind::Read,
        run: save,
    },
    CommandSpec {
        id: "project.info",
        label: "Summarise the open project: tempo, time signature, counts of tracks, scenes, clips and markers, undo state",
        params: &[],
        kind: CommandKind::Read,
        run: info,
    },
    CommandSpec {
        id: "project.tracks",
        label: "List tracks with id, name, volume (dB), pan, mute, whether armed, the MIDI and audio inputs it hears, and devices",
        params: &[],
        kind: CommandKind::Read,
        run: tracks,
    },
    CommandSpec {
        id: "project.clips",
        label: "List arrangement clips with their id (what clip commands take), track, index, start and length in beats, and note count",
        params: &[optional("track", ParamType::Track, "only clips on this track (id or name)")],
        kind: CommandKind::Read,
        run: clips,
    },
    CommandSpec {
        id: "arrangement.audioOverlaps",
        label: "List the pairs of audio clips that sound together on one track because their arrangement spans overlap; clips that only touch do not",
        params: &[optional("track", ParamType::Track, "only overlaps on this track (id or name)")],
        kind: CommandKind::Read,
        run: audio_overlaps,
    },
    CommandSpec {
        id: "transport.setTempo",
        label: "Set the project tempo",
        params: &[param("bpm", ParamType::Number, "beats per minute, 20 to 999")],
        kind: CommandKind::Edit,
        run: set_tempo,
    },
    CommandSpec {
        id: "transport.setLoop",
        label: "Set the arrangement's loop region, the span playback repeats while looping is on",
        params: &[
            param("start", ParamType::Number, "where the loop starts, in beats from the beginning, 0 … 120000"),
            param("end", ParamType::Number, "where it ends, in beats, after start and at most 120000"),
            optional("enabled", ParamType::Bool, "true turns looping on, false off (default: as it is)"),
        ],
        kind: CommandKind::Edit,
        run: set_loop,
    },
    CommandSpec {
        id: "transport.setLoopEnabled",
        label: "Turn looping on or off; on without a loop region first makes one over the first 4 bars",
        params: &[optional("enabled", ParamType::Bool, "true turns looping on, false off (default: the other way)")],
        kind: CommandKind::Edit,
        run: set_loop_enabled,
    },
    CommandSpec {
        id: "tempo.points",
        label: "List the tempo map: the start tempo and every tempo change (numbered from 1), the start time signature and every time-signature change",
        params: &[],
        kind: CommandKind::Read,
        run: tempo::points,
    },
    CommandSpec {
        id: "tempo.addPoint",
        label: "Add a tempo change at a beat: a step, or a ramp that glides from the tempo before it (a beat holds a ramp and a step; one of the same kind already there is replaced)",
        params: &[
            param("beat", ParamType::Number, "where the change is, in beats from the start, after 0"),
            param("bpm", ParamType::Number, "the tempo from there, 20 to 999"),
            optional(
                "ramp",
                ParamType::Bool,
                "glide from the tempo before to this one, reaching it at the beat (default false: a step)",
            ),
        ],
        kind: CommandKind::Edit,
        run: tempo::add_point,
    },
    CommandSpec {
        id: "tempo.movePoint",
        label: "Move a tempo change to another beat, or change its tempo or whether it ramps",
        params: &[
            param("point", ParamType::Integer, "tempo point number, from 1, as in tempo.points"),
            optional("beat", ParamType::Number, "the new beat, after 0"),
            optional("bpm", ParamType::Number, "the new tempo, 20 to 999"),
            optional("ramp", ParamType::Bool, "whether it glides from the tempo before"),
        ],
        kind: CommandKind::Edit,
        run: tempo::move_point,
    },
    CommandSpec {
        id: "tempo.deletePoint",
        label: "Remove a tempo change",
        params: &[param("point", ParamType::Integer, "tempo point number, from 1, as in tempo.points")],
        kind: CommandKind::Edit,
        run: tempo::delete_point,
    },
    CommandSpec {
        id: "timeSignature.add",
        label: "Change the time signature from the start of a bar (one already at that bar is replaced)",
        params: &[
            param("bar", ParamType::Integer, "bar number, from 2 (bar 1 has the start signature)"),
            param("numerator", ParamType::Integer, "beats in a bar, 1 to 99"),
            param("denominator", ParamType::Integer, "the note value of a beat: 1, 2, 4, 8, 16, 32 or 64"),
        ],
        kind: CommandKind::Edit,
        run: tempo::add_signature,
    },
    CommandSpec {
        id: "timeSignature.move",
        label: "Move a time-signature change to another bar, or change its signature",
        params: &[
            param("bar", ParamType::Integer, "the bar the change is at, from 2"),
            optional("to", ParamType::Integer, "the bar to move it to, from 2"),
            optional("numerator", ParamType::Integer, "beats in a bar, 1 to 99"),
            optional("denominator", ParamType::Integer, "1, 2, 4, 8, 16, 32 or 64"),
        ],
        kind: CommandKind::Edit,
        run: tempo::move_signature,
    },
    CommandSpec {
        id: "timeSignature.delete",
        label: "Remove a time-signature change",
        params: &[param("bar", ParamType::Integer, "the bar the change is at, from 2")],
        kind: CommandKind::Edit,
        run: tempo::delete_signature,
    },
    CommandSpec {
        id: "track.setVolume",
        label: "Set the volume of each named track",
        params: &[
            TRACKS,
            param("db", ParamType::Decibels, "level in dB, at most +6; \"-inf\" or below -150 is silence"),
            CONTINUE,
        ],
        kind: CommandKind::Edit,
        run: set_volume,
    },
    CommandSpec {
        id: "main.setVolume",
        label: "Set the main output's volume, which every track, group and return plays through",
        params: &[
            param("db", ParamType::Decibels, "level in dB, at most +6; \"-inf\" or below -150 is silence"),
            optional(
                "continue",
                ParamType::Bool,
                "true adds the change to the undo step of the previous call of this command, when no other \
                 edit came between (one drag, one step)",
            ),
        ],
        kind: CommandKind::Edit,
        run: set_main_volume,
    },
    CommandSpec {
        id: "track.adjustVolume",
        label: "Move the volume of each named track by the same number of decibels, keeping their differences",
        params: &[TRACKS, param("db", ParamType::Number, "how far to move it, in dB; positive is louder")],
        kind: CommandKind::Edit,
        run: adjust_volume,
    },
    CommandSpec {
        id: "track.setPan",
        label: "Set the pan of each named track",
        params: &[TRACKS, param("pan", ParamType::Number, "-1 is full left, 0 centre, 1 full right"), CONTINUE],
        kind: CommandKind::Edit,
        run: set_pan,
    },
    CommandSpec {
        id: "track.adjustPan",
        label: "Move the pan of each named track by the same amount, keeping their differences",
        params: &[TRACKS, param("pan", ParamType::Number, "how far to move it, -1 … +1; positive is right")],
        kind: CommandKind::Edit,
        run: adjust_pan,
    },
    CommandSpec {
        id: "track.setMute",
        label: "Mute or unmute each named track",
        params: &[TRACKS, param("mute", ParamType::Bool, "true mutes the tracks")],
        kind: CommandKind::Edit,
        run: set_mute,
    },
    CommandSpec {
        id: "track.group",
        label: "Put tracks into a new group track, which plays into where they played",
        params: &[
            param("tracks", ParamType::Tracks, "the tracks to group (ids or names), all in the same group or none"),
            optional("name", ParamType::Text, "the group's name (\"Group\" if left out)"),
        ],
        kind: CommandKind::Edit,
        run: group_tracks,
    },
    CommandSpec {
        id: "track.setGroup",
        label: "Move a track (a group with its tracks) into a group track, or out of every group, and play into it",
        params: &[
            TRACK,
            param("group", ParamType::Track, "a group track (id or name) to move into, or \"none\" for the top level"),
            optional(
                "index",
                ParamType::Integer,
                "its place among the tracks directly in that group (or at the top level), from 0; the end if left out",
            ),
        ],
        kind: CommandKind::Edit,
        run: set_group,
    },
    CommandSpec {
        id: "track.ungroup",
        label: "Dissolve a group track: its tracks stay, playing into where the group played",
        params: &[TRACK],
        kind: CommandKind::Edit,
        run: ungroup_track,
    },
    CommandSpec {
        id: "track.move",
        label: "Move a track (a group with its tracks) to another place in the track order",
        params: &[TRACK, param("to", ParamType::Integer, "its new position, from 0, in project.tracks order")],
        kind: CommandKind::Edit,
        run: move_track,
    },
    CommandSpec {
        id: "track.rename",
        label: "Rename a track",
        params: &[TRACK, param("name", ParamType::Text, "the new name, not empty")],
        kind: CommandKind::Edit,
        run: rename_track,
    },
    CommandSpec {
        id: "track.setColor",
        label: "Set a track's colour",
        params: &[TRACK, param("color", ParamType::Text, "\"#rrggbb\", or \"none\" for the default")],
        kind: CommandKind::Edit,
        run: set_color,
    },
    CommandSpec {
        id: "track.delete",
        label: "Delete a track with its clips; a deleted group's tracks stay",
        params: &[TRACK],
        kind: CommandKind::Edit,
        run: delete_track,
    },
    CommandSpec {
        id: "clip.delete",
        label: "Delete arrangement clips as one undo step, or the clip in a track's slot of a scene",
        params: &[CLIP, DELETE_CLIPS, DELETE_TRACK, CLIP_INDEX, CLIP_SCENE],
        kind: CommandKind::Edit,
        run: delete_clips,
    },
    CommandSpec {
        id: "clip.rename",
        label: "Rename a clip: by its id, or (deprecated) an arrangement clip by index; a slot's clip by scene; clips that play the same content share the name",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param("name", ParamType::Text, "the new name (empty clears it)"),
        ],
        kind: CommandKind::Edit,
        run: rename_clip,
    },
    CommandSpec {
        id: "clip.renameMany",
        label: "Give several arrangement clips the same name, as one undo step; clips that play the same content share the name",
        params: &[CLIPS, param("name", ParamType::Text, "the new name (empty clears it)")],
        kind: CommandKind::Edit,
        run: rename_clips,
    },
    CommandSpec {
        id: "clip.setGain",
        label: "Set an audio clip's gain (every clip that plays the same audio content follows)",
        params: &[
            CLIP_TRACK,
            CLIP,
            optional(
                "index",
                ParamType::Integer,
                "clip index on the track, from project.clips (deprecated, use clip; accepted until the project format's next version (ADR 0025))",
            ),
            param("db", ParamType::Decibels, "gain in dB, at most +24; \"-inf\" or below -150 is silence"),
        ],
        kind: CommandKind::Edit,
        run: set_clip_gain,
    },
    CommandSpec {
        id: "clip.setFades",
        label: "Set an audio clip's fade-in and fade-out: lengths in beats (together at most the clip's length), shapes and curves; an arrangement clip by index, a launcher clip by scene",
        params: &[
            CLIP_TRACK,
            CLIP,
            optional(
                "index",
                ParamType::Integer,
                "arrangement clip index on the track, from project.clips (deprecated, use clip; accepted until the project format's next version (ADR 0025))",
            ),
            optional(
                "scene",
                ParamType::Integer,
                "instead of index: the clip in the track's slot of this scene, from 1",
            ),
            optional("in", ParamType::Number, "fade-in length in beats from the clip's start, at least 0; 0 is none"),
            optional("out", ParamType::Number, "fade-out length in beats before the clip's end, at least 0; 0 is none"),
            optional("inShape", ParamType::Text, "fade-in shape: linear, equal_power, fast, slow or s_curve"),
            optional("outShape", ParamType::Text, "fade-out shape: linear, equal_power, fast, slow or s_curve"),
            optional(
                "inCurve",
                ParamType::Number,
                "fade-in curve, -1 (slower start) to +1 (faster start); 0 is the shape",
            ),
            optional(
                "outCurve",
                ParamType::Number,
                "fade-out curve, -1 (slower start) to +1 (faster start); 0 is the shape",
            ),
        ],
        kind: CommandKind::Edit,
        run: set_clip_fades,
    },
    CommandSpec {
        id: "clip.shiftFades",
        label: "Lengthen or shorten the fade-in or fade-out of several arrangement clips by the same beats, each from its own, kept from 0 to what the clip's other fade leaves; as one undo step",
        params: &[
            CLIPS,
            param("edge", ParamType::Text, "in or out"),
            param("beats", ParamType::Number, "how much longer (or, below 0, shorter) each fade gets"),
        ],
        kind: CommandKind::Edit,
        run: shift_fades,
    },
    CommandSpec {
        id: "clip.copy",
        label: "Copy clips, with what they play, for clip.paste: arrangement clips by clips, or a clip slot's by track and scene",
        params: &[
            CLIP,
            optional(
                "clips",
                ParamType::Clips,
                "the arrangement clips to copy, each { \"clip\": id } as project.clips lists them (or, deprecated, { \"track\": id or name, \"index\": n })",
            ),
            optional(
                "track",
                ParamType::Track,
                "instead of clip or clips, with scene: the track whose clip slot to copy",
            ),
            optional(
                "scene",
                ParamType::Integer,
                "with track: the clip in this scene's slot, from 1, as in project.launcher",
            ),
        ],
        kind: CommandKind::Read,
        run: copy_clips,
    },
    CommandSpec {
        id: "clip.cut",
        label: "Copy arrangement clips for clip.paste and delete them, as one undo step",
        params: &[CLIPS],
        kind: CommandKind::Edit,
        run: cut_clips,
    },
    CommandSpec {
        id: "clip.paste",
        label: "Place the copied clip on a track's arrangement at start, replacing what it covers unless stacked, or in its clip slot at scene, replacing the slot's clip",
        params: &[
            TRACK,
            optional(
                "start",
                ParamType::Number,
                "where it starts in the arrangement, in beats from the beginning, at least 0",
            ),
            optional(
                "scene",
                ParamType::Integer,
                "instead of start: the scene, from 1, whose slot on the track takes the clip",
            ),
            optional("stack", ParamType::Bool, "with start, true leaves clips under it as they are"),
        ],
        kind: CommandKind::Edit,
        run: paste_clip,
    },
    CommandSpec {
        id: "clip.duplicate",
        label: "Place a copy of each arrangement clip right after it, replacing what the copies cover unless stacked",
        params: &[
            CLIPS,
            optional(
                "stack",
                ParamType::Bool,
                "true leaves clips under the copies as they are; false (default) shortens, splits or removes them",
            ),
        ],
        kind: CommandKind::Edit,
        run: duplicate_clips,
    },
    CommandSpec {
        id: "clip.setEnabled",
        label: "Turn arrangement clips on or off: a clip that is off is drawn dimmed and plays nothing",
        params: &[CLIPS, param("enabled", ParamType::Bool, "true plays the clips, false silences them")],
        kind: CommandKind::Edit,
        run: set_clip_enabled,
    },
    CommandSpec {
        id: "clip.setColor",
        label: "Give a clip a colour of its own (by index), or every clip of a track one when no index is given; \"none\" makes them show the track's colour again",
        params: &[
            CLIP_TRACK,
            param("color", ParamType::Text, "\"#rrggbb\", or \"none\" for the track's colour"),
            CLIP,
            optional(
                "index",
                ParamType::Integer,
                "clip index on the track, from project.clips (deprecated, use clip); every clip of the track when neither is given",
            ),
        ],
        kind: CommandKind::Edit,
        run: crate::clip_color::set_clip_color,
    },
    CommandSpec {
        id: "track.renameClips",
        label: "Name every clip of a track, in the arrangement and in its slots, after the track",
        params: &[TRACK],
        kind: CommandKind::Edit,
        run: crate::clip_color::rename_clips,
    },
    CommandSpec {
        id: "clip.trim",
        label: "Move an arrangement clip's edges (song beats), the content moving with the left one; an unlooped clip ends at most where its audio does, a looping one repeats as far as it goes, and loop turns the loop on first",
        params: &[
            CLIP,
            CLIP_TRACK,
            CLIP_INDEX,
            optional("start", ParamType::Number, "where the clip's left edge goes, in beats from the beginning"),
            optional("end", ParamType::Number, "where the clip's right edge goes, in beats from the beginning"),
            optional(
                "loop",
                ParamType::Bool,
                "true turns the clip's loop on (over what it plays now) before the edges move",
            ),
            optional("stack", ParamType::Bool, "true leaves clips under it as they are; false (default) replaces them"),
        ],
        kind: CommandKind::Edit,
        run: crate::clip_edges::trim_clip,
    },
    CommandSpec {
        id: "clip.stretch",
        label: "Move an audio clip's edges with its audio time-stretched to fill the new length, its pitch kept",
        params: &[
            CLIP,
            CLIP_TRACK,
            CLIP_INDEX,
            optional("start", ParamType::Number, "where the clip's left edge goes, in beats from the beginning"),
            optional("end", ParamType::Number, "where the clip's right edge goes, in beats from the beginning"),
            optional("stack", ParamType::Bool, "true leaves clips under it as they are; false (default) replaces them"),
        ],
        kind: CommandKind::Edit,
        run: crate::clip_edges::stretch_clip,
    },
    CommandSpec {
        id: "range.split",
        label: "Split the clips of some tracks at both edges of a time selection, so the span is clips of its own; answers with the clips inside it",
        params: &[TRACKS, RANGE_START, RANGE_END],
        kind: CommandKind::Edit,
        run: crate::range::split_range,
    },
    CommandSpec {
        id: "range.insertTime",
        label: "Insert empty time as long as a time selection at its start, on every track: clips crossing it are split there, and clips, markers, automation, tempo changes and the loop region after it move later by as much (time-signature changes too, when the length is whole bars)",
        params: &[RANGE_START, RANGE_END],
        kind: CommandKind::Edit,
        run: crate::range::insert_time,
    },
    CommandSpec {
        id: "range.delete",
        label: "Delete what plays inside a time selection on some tracks: clips crossing an edge keep their parts outside it, and the gap stays",
        params: &[TRACKS, RANGE_START, RANGE_END],
        kind: CommandKind::Edit,
        run: crate::range::delete_range,
    },
    CommandSpec {
        id: "range.copy",
        label: "Copy what plays inside a time selection on some tracks, cut to it, for clip.paste; a paste with no track puts each part back on its own track",
        params: &[TRACKS, RANGE_START, RANGE_END],
        kind: CommandKind::Read,
        run: crate::range::copy_range,
    },
    CommandSpec {
        id: "range.cut",
        label: "Copy what plays inside a time selection for clip.paste and delete it, as one undo step",
        params: &[TRACKS, RANGE_START, RANGE_END],
        kind: CommandKind::Edit,
        run: crate::range::cut_range,
    },
    CommandSpec {
        id: "range.duplicate",
        label: "Put a copy of what plays inside a time selection right after it, on the same tracks",
        params: &[
            TRACKS,
            RANGE_START,
            RANGE_END,
            optional(
                "stack",
                ParamType::Bool,
                "true leaves clips under the copy as they are; false (default) replaces them",
            ),
        ],
        kind: CommandKind::Edit,
        run: crate::range::duplicate_range,
    },
    CommandSpec {
        id: "clip.moveMany",
        label: "Move several arrangement clips by the same number of beats at once, so none replaces another on the way; to_track takes clips of one track to another",
        params: &[
            param("clips", ParamType::Clips, "the clips to move, each {track, index} from project.clips"),
            param("beats", ParamType::Number, "how far to move them, in beats; negative is earlier"),
            optional("to_track", ParamType::Track, "track to move them to, when they are all on one track"),
            optional(
                "color",
                ParamType::Text,
                "\"#rrggbb\": the colour they show now, kept on the other track by those with none of their own",
            ),
            optional(
                "copy",
                ParamType::Bool,
                "true leaves the clips where they are and puts copies (each with a content of its own) where they would have gone",
            ),
            optional(
                "stack",
                ParamType::Bool,
                "true leaves clips under them as they are; false (default) replaces them",
            ),
        ],
        kind: CommandKind::Edit,
        run: crate::range::move_clips,
    },
    CommandSpec {
        id: "clip.split",
        label: "Cut an arrangement clip in two at a song position; the second part plays on from where the clip was",
        params: &[
            CLIP_TRACK,
            CLIP,
            optional(
                "index",
                ParamType::Integer,
                "clip index on the track, from project.clips (deprecated, use clip; accepted until the project format's next version (ADR 0025))",
            ),
            param("at", ParamType::Number, "where to cut, in beats from the beginning of the song, inside the clip"),
        ],
        kind: CommandKind::Edit,
        run: editing::split_clip,
    },
    CommandSpec {
        id: "clip.consolidate",
        label: "Turn a range of one track into one clip: MIDI clips by joining their notes, audio clips by rendering them (no devices) to a new file next to the project",
        params: &[
            TRACK,
            param("start", ParamType::Number, "where the range starts, in beats from the beginning, at least 0"),
            param("end", ParamType::Number, "where the range ends, in beats, after start"),
        ],
        kind: CommandKind::Edit,
        run: editing::consolidate,
    },
    CommandSpec {
        id: "track.setOutput",
        label: "Route a track's audio to the main output or into a group track",
        params: &[TRACK, param("to", ParamType::Track, "\"main\", or a group track (id or name) to play into")],
        kind: CommandKind::Edit,
        run: set_output,
    },
    CommandSpec {
        id: "track.setSolo",
        label: "Solo the named tracks or end their solo: while any track is soloed, only soloed tracks, the groups they play into and return tracks are heard",
        params: &[
            TRACKS,
            param("solo", ParamType::Bool, "true solos the tracks"),
            optional("exclusive", ParamType::Bool, "true also ends every other solo (false by default)"),
        ],
        kind: CommandKind::Edit,
        run: set_solo,
    },
    CommandSpec {
        id: "track.setCue",
        label: "Turn a track's cue on or off: a cued track is also heard on the cue output (headphones, output.setCuePair), after its own volume and pan, while the main mix stays as it is",
        params: &[TRACK, param("cue", ParamType::Bool, "true cues the track")],
        kind: CommandKind::Edit,
        run: set_cue,
    },
    CommandSpec {
        id: "clip.move",
        label: "Move an arrangement clip to a new start position, and to another track if asked; it replaces what it covers unless stacked",
        params: &[
            CLIP_TRACK,
            CLIP,
            optional(
                "index",
                ParamType::Integer,
                "clip index on the track, from project.clips (deprecated, use clip; accepted until the project format's next version (ADR 0025))",
            ),
            param("start", ParamType::Number, "new start in beats from the beginning, at least 0"),
            optional(
                "to_track",
                ParamType::Track,
                "track to move the clip to (id or name); it must take the clip's kind: notes or audio",
            ),
            optional(
                "stack",
                ParamType::Bool,
                "true leaves clips under it as they are (they all play); false (default) shortens, splits or removes them",
            ),
            optional(
                "color",
                ParamType::Text,
                "\"#rrggbb\": the colour the clip shows now, which it keeps on another track when it has none of its own",
            ),
        ],
        kind: CommandKind::Edit,
        run: move_clip,
    },
    CommandSpec {
        id: "clip.moveSlot",
        label: "Move a slot's clip to another track's slot; the two slots swap when the target has a clip",
        params: &[
            TRACK,
            SCENE,
            optional(
                "to_track",
                ParamType::Track,
                "track to move the clip to (id or name); it must take the clip's kind: notes or audio (default: the same track)",
            ),
            optional("to_scene", ParamType::Integer, "the scene to move it to, from 1 (default: the same scene)"),
        ],
        kind: CommandKind::Edit,
        run: move_slot,
    },
    CommandSpec {
        id: "project.markers",
        label: "List the arrangement's markers (named positions on the timeline) in beat order, with index, beat and name",
        params: &[],
        kind: CommandKind::Read,
        run: markers,
    },
    CommandSpec {
        id: "marker.add",
        label: "Add a named marker on the arrangement's timeline",
        params: &[
            param("beat", ParamType::Number, "where, in beats from the beginning, 0 … 120000"),
            optional("name", ParamType::Text, "the marker's name (default \"Marker n\")"),
        ],
        kind: CommandKind::Edit,
        run: add_marker,
    },
    CommandSpec {
        id: "marker.move",
        label: "Move a marker to another beat; markers stay in beat order, so its index can change",
        params: &[MARKER, param("beat", ParamType::Number, "the new beat, 0 … 120000")],
        kind: CommandKind::Edit,
        run: move_marker,
    },
    CommandSpec {
        id: "marker.rename",
        label: "Rename a marker",
        params: &[MARKER, param("name", ParamType::Text, "the new name, not empty")],
        kind: CommandKind::Edit,
        run: rename_marker,
    },
    CommandSpec {
        id: "marker.delete",
        label: "Delete a marker",
        params: &[MARKER],
        kind: CommandKind::Edit,
        run: delete_marker,
    },
    CommandSpec {
        id: "marker.jump",
        label: "Move the play start marker to a marker; while playing, playback jumps there at once",
        params: &[MARKER],
        kind: CommandKind::Transport,
        run: jump_to_marker,
    },
    CommandSpec {
        id: "project.launcher",
        label: "List the clip launcher: scenes (name, tempo) and each track's slots (clip name, follow action, stop button)",
        params: &[],
        kind: CommandKind::Read,
        run: launcher_grid,
    },
    CommandSpec {
        id: "launcher.state",
        label: "What the audio engine is playing: transport, tempo, position, each track's playing and queued clip, and the engine's CPU load",
        params: &[],
        kind: CommandKind::Transport,
        run: launcher_state,
    },
    CommandSpec {
        id: "transport.play",
        label: "Start the transport from the play start marker (beat 0 unless moved), or from an arrangement clip's start: the arrangement plays, launches follow the beat",
        params: &[
            optional(
                "from",
                ParamType::Text,
                "marker (default): from the play start marker; clip: the marker moves to the start of the clip track and index name, and playback starts there",
            ),
            optional(
                "clip",
                ParamType::Clip,
                "with from: clip, the clip's id from project.clips, or a name only one clip has",
            ),
            optional("track", ParamType::Track, "with from: clip and index, the clip's track (id or name)"),
            optional(
                "index",
                ParamType::Integer,
                "with from: clip and track, the clip's index on the track, from project.clips (deprecated, use clip; accepted until the project format's next version (ADR 0025))",
            ),
        ],
        kind: CommandKind::Transport,
        run: play,
    },
    CommandSpec {
        id: "transport.stop",
        label: "Stop the transport, the arrangement and every launched clip at once; stopping while stopped moves the play start marker to beat 0",
        params: &[optional(
            "return",
            ParamType::Text,
            "marker (default): the playhead goes back to the play start marker; here: the marker moves to where the playhead stopped",
        )],
        kind: CommandKind::Transport,
        run: stop,
    },
    CommandSpec {
        id: "transport.setPosition",
        label: "Move the play start marker, where the transport starts; while playing, playback jumps there at once",
        params: &[param("beats", ParamType::Number, "beats from the beginning, 0 … 120000")],
        kind: CommandKind::Transport,
        run: set_position,
    },
    CommandSpec {
        id: "transport.followTempoMap",
        label: "Play the arrangement's tempo and time signature again after a launched scene's tempo or time signature took over; starting the transport does the same",
        params: &[],
        kind: CommandKind::Transport,
        run: follow_tempo_map,
    },
    CommandSpec {
        id: "transport.setMetronome",
        label: "Turn the metronome on or off: a click on every beat while playing, higher on the bar",
        params: &[
            param("on", ParamType::Bool, "true turns the click on"),
            optional(
                "db",
                ParamType::Decibels,
                "click level in dB, at most +6; keeps the current level (-6 at first) if left out",
            ),
            optional(
                "sound",
                ParamType::Text,
                "the click's sound: beep (the first), wood or soft; keeps it if left out",
            ),
        ],
        kind: CommandKind::Transport,
        run: set_metronome,
    },
    CommandSpec {
        id: "mixer.levels",
        label: "Read the level meters of the playing engine: the main output's and each track's peak and RMS level per side in dB (null for silence), and whether it clipped",
        params: &[],
        kind: CommandKind::Transport,
        run: meters::levels,
    },
    CommandSpec {
        id: "mixer.resetClip",
        label: "Turn off clip indicators: a track's, the main output's, or every one when neither is given",
        params: &[
            optional("track", ParamType::Track, "track id or name, from project.tracks"),
            optional("main", ParamType::Bool, "true turns off the main output's"),
        ],
        kind: CommandKind::Transport,
        run: meters::reset_clip,
    },
    CommandSpec {
        id: "track.setMidiInput",
        label: "Choose which MIDI controllers a track hears while armed: every port, one port by name (from midi.inputs), or none, and every channel or one; saved with the project",
        params: &[
            TRACK,
            optional(
                "port",
                ParamType::Text,
                "\"all\" (the default), \"none\", or a port's name; left out keeps the port",
            ),
            optional("channel", ParamType::Integer, "1 to 16, or 0 for every channel; left out keeps the channel"),
        ],
        kind: CommandKind::Edit,
        run: set_midi_input,
    },
    CommandSpec {
        id: "track.setAudioInput",
        label: "Choose which channels of the audio input device a track hears (none, one channel, or a stereo pair) and when it is heard through the track: auto (while armed), in (always) or off; saved with the project",
        params: &[
            TRACK,
            optional(
                "input",
                ParamType::Text,
                "\"none\", a channel from 1 (\"3\"), or a pair (\"1/2\"); left out keeps it",
            ),
            optional("monitor", ParamType::Text, "auto (the default), in or off; left out keeps it"),
        ],
        kind: CommandKind::Edit,
        run: set_audio_input,
    },
    CommandSpec {
        id: "track.setArm",
        label: "Arm a track or disarm it: an armed track's instrument plays what the MIDI inputs play (midi.inputs), whether the transport runs or not. Arming is not an edit: it is not undone and not saved",
        params: &[
            TRACK,
            param("armed", ParamType::Bool, "true arms the track"),
            optional("exclusive", ParamType::Bool, "true also disarms every other track (false by default)"),
        ],
        kind: CommandKind::Transport,
        run: set_arm,
    },
    CommandSpec {
        id: "clip.record",
        label: "Record into an empty clip slot of an armed track from the next launch boundary (after the count-in when the transport is stopped); clip.launch on the slot ends the take at the next boundary and plays it, looped",
        params: &[TRACK, SCENE],
        kind: CommandKind::Transport,
        run: crate::recording::record_clip,
    },
    CommandSpec {
        id: "transport.record",
        label: "Record every armed track into the arrangement from the playhead (from the play start marker after the count-in when stopped), or end every take; each take is one undo step",
        params: &[param("on", ParamType::Bool, "true starts recording, false ends every take")],
        kind: CommandKind::Transport,
        run: crate::recording::record_arrangement,
    },
    CommandSpec {
        id: "transport.setCountIn",
        label: "Set the count-in recording waits for when it starts the transport: 0 (none), 1, 2 or 4 bars of clicks",
        params: &[param("bars", ParamType::Integer, "0, 1, 2 or 4")],
        kind: CommandKind::Transport,
        run: crate::recording::set_count_in,
    },
    CommandSpec {
        id: "record.state",
        label: "What is being recorded: whether the arrangement records, each take (track, slot or arrangement, start and end beat, notes, frames, file), the count-in setting and the beats it has left",
        params: &[],
        kind: CommandKind::Transport,
        run: crate::recording::record_state,
    },
    CommandSpec {
        id: "clip.launch",
        label: "Launch the clip in a track's slot at the next quantisation boundary (an empty slot with a stop button stops the track)",
        params: &[TRACK, SCENE],
        kind: CommandKind::Transport,
        run: launch_clip,
    },
    CommandSpec {
        id: "clip.setLoop",
        label: "Switch a clip's loop (an arrangement clip by index, or the clip in a track's slot by scene): on, it repeats its whole length, or the loop it has; off, it plays once. With start and end, the loop is set to that part of the content and turned on",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            optional(
                "loop",
                ParamType::Bool,
                "true loops the clip, false plays it once; needed unless start or end is given",
            ),
            optional("start", ParamType::Number, "the loop's start in content beats, 0 to 120000"),
            optional("end", ParamType::Number, "the loop's end in content beats, after start, at most 120000"),
        ],
        kind: CommandKind::Edit,
        run: crate::clip_shape::set_loop,
    },
    CommandSpec {
        id: "clip.setLength",
        label: "Set how long a clip plays, in beats (an arrangement clip by index, or a slot's clip by scene); an arrangement clip keeps its start and may not run into the next clip on its lane",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param("length", ParamType::Number, "the new length in beats, above 0 and at most 120000"),
        ],
        kind: CommandKind::Edit,
        run: crate::clip_shape::set_length,
    },
    CommandSpec {
        id: "clip.setStart",
        label: "Set a clip's start marker: where in its content it starts playing. A clip that does not loop keeps its end marker, so its length changes by as much; a looping clip keeps its length",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param(
                "offset",
                ParamType::Number,
                "the start marker in content beats, 0 to 120000; before the loop's end, or the clip's end",
            ),
        ],
        kind: CommandKind::Edit,
        run: crate::clip_shape::set_start,
    },
    CommandSpec {
        id: "scene.launch",
        label: "Launch a scene: every track launches its slot in that row at the next quantisation boundary",
        params: &[SCENE],
        kind: CommandKind::Transport,
        run: launch_scene,
    },
    CommandSpec {
        id: "scene.stop",
        label: "Stop a scene's row: every track whose slot in that row has a clip stops at the next quantisation boundary",
        params: &[SCENE],
        kind: CommandKind::Transport,
        run: stop_scene,
    },
    CommandSpec {
        id: "scene.insert",
        label: "Insert an empty scene (a row of empty slots with stop buttons) at a position, or after the last scene",
        params: &[
            optional(
                "scene",
                ParamType::Integer,
                "number the new scene gets, from 1 to one past the last scene; after the last scene if left out",
            ),
            optional("name", ParamType::Text, "the new scene's name; empty if left out"),
        ],
        kind: CommandKind::Edit,
        run: insert_scene,
    },
    CommandSpec {
        id: "scene.duplicate",
        label: "Copy a scene, with its name, tempo, time signature and the clips in its row, to a new scene right below it",
        params: &[SCENE],
        kind: CommandKind::Edit,
        run: duplicate_scene,
    },
    CommandSpec {
        id: "scene.delete",
        label: "Delete a scene and the clips in its row; the last scene left cannot be deleted",
        params: &[SCENE],
        kind: CommandKind::Edit,
        run: delete_scene,
    },
    CommandSpec {
        id: "scene.rename",
        label: "Rename a scene",
        params: &[SCENE, param("name", ParamType::Text, "the new name; empty clears it")],
        kind: CommandKind::Edit,
        run: rename_scene,
    },
    CommandSpec {
        id: "scene.setTempo",
        label: "Set the tempo a scene switches the project to when it launches, or clear it",
        params: &[
            SCENE,
            optional("bpm", ParamType::Number, "20 … 999; left out or null, the scene keeps the tempo playing"),
        ],
        kind: CommandKind::Edit,
        run: set_scene_tempo,
    },
    CommandSpec {
        id: "scene.setTimeSignature",
        label: "Set the time signature a scene switches to when it launches, or clear it",
        params: &[
            SCENE,
            optional(
                "numerator",
                ParamType::Integer,
                "beats in a bar, 1 … 99; left out with the denominator, the scene keeps the time signature playing",
            ),
            optional("denominator", ParamType::Integer, "the note value of a beat: 1, 2, 4, 8 or 16"),
        ],
        kind: CommandKind::Edit,
        run: set_scene_time_signature,
    },
    CommandSpec {
        id: "track.stop",
        label: "Stop the clip a track is playing, at the next quantisation boundary",
        params: &[TRACK],
        kind: CommandKind::Transport,
        run: stop_track,
    },
    CommandSpec {
        id: "launcher.stopAll",
        label: "Stop every launched clip at the next quantisation boundary",
        params: &[],
        kind: CommandKind::Transport,
        run: stop_all,
    },
    CommandSpec {
        id: "launcher.setQuantization",
        label: "Set the grid launches wait for and the Arrangement view snaps to (default 1 bar)",
        params: &[param(
            "value",
            ParamType::Text,
            "none, 8 bars, 4 bars, 2 bars, 1 bar, 1/2, 1/2T, 1/4, 1/4T, 1/8, 1/8T, 1/16, 1/16T or 1/32",
        )],
        kind: CommandKind::Transport,
        run: set_quantization,
    },
    CommandSpec {
        id: "launcher.backToArrangement",
        label: "Return every track that launched a clip to playing its arrangement clips",
        params: &[],
        kind: CommandKind::Transport,
        run: back_to_arrangement,
    },
    CommandSpec {
        id: "clip.setFollowAction",
        label: "Set what a slot clip does after it has played for a while: action A or B, by chance",
        params: &[
            TRACK,
            SCENE,
            optional("enabled", ParamType::Bool, "whether the clip runs its follow action"),
            optional(
                "time",
                ParamType::Number,
                "beats from the launch to the action, more than 0; used when not linked",
            ),
            optional("linked", ParamType::Bool, "true: the time is the clip's length"),
            optional(
                "actionA",
                ParamType::Text,
                "nothing, stop, again, previous, next, first, last, any, other, or \"jump N\" for the slot of scene N",
            ),
            optional("actionB", ParamType::Text, "like actionA"),
            optional("chanceA", ParamType::Integer, "weight of action A, 0 to 100"),
            optional("chanceB", ParamType::Integer, "weight of action B, 0 to 100"),
        ],
        kind: CommandKind::Edit,
        run: follow::set_follow_action,
    },
    CommandSpec {
        id: "clip.release",
        label: "Release a slot's launch button (clip.launch is the press): stops a clip in gate mode, ends the repeats of one in repeat mode",
        params: &[TRACK, SCENE],
        kind: CommandKind::Transport,
        run: launch::release_clip,
    },
    CommandSpec {
        id: "clip.setLaunchMode",
        label: "Set what launching a slot's clip does: trigger (plays until stopped), gate (plays while held), toggle (a second launch stops it) or repeat (relaunches at its quantisation while held)",
        params: &[TRACK, SCENE, param("mode", ParamType::Text, "trigger, gate, toggle or repeat")],
        kind: CommandKind::Edit,
        run: launch::set_launch_mode,
    },
    CommandSpec {
        id: "clip.setLegato",
        label: "Set whether a slot's clip starts at the position of the clip it replaces on its track (legato) instead of at its start",
        params: &[TRACK, SCENE, param("on", ParamType::Bool, "true for legato")],
        kind: CommandKind::Edit,
        run: launch::set_legato,
    },
    CommandSpec {
        id: "clip.setQuantization",
        label: "Set the grid a slot's clip waits for when launched: global (the launcher's), none, or its own",
        params: &[
            TRACK,
            SCENE,
            param(
                "value",
                ParamType::Text,
                "global, none, 8 bars, 4 bars, 2 bars, 1 bar, 1/2, 1/2T, 1/4, 1/4T, 1/8, 1/8T, 1/16, 1/16T or 1/32",
            ),
        ],
        kind: CommandKind::Edit,
        run: launch::set_quantization,
    },
    CommandSpec {
        id: "launcher.addMapping",
        label: "Map a computer key or a MIDI note to a slot, a scene or a track's stop button: its press launches it (and its release releases a gate or repeat clip); a source already mapped moves to the new target",
        params: &[
            param(
                "source",
                ParamType::Text,
                "key:<name> (key:Q, key:1, key:Space) or note:<channel 1-16>:<note 0-127>",
            ),
            param("target", ParamType::Text, "slot, scene or stop"),
            optional("track", ParamType::Track, "track id or name, for slot and stop"),
            optional("scene", ParamType::Integer, "scene number, from 1, for slot and scene"),
        ],
        kind: CommandKind::Edit,
        run: mapping::add_mapping,
    },
    CommandSpec {
        id: "launcher.removeMapping",
        label: "Remove a key or note mapping: the one of a source, or every mapping of a target",
        params: &[
            optional("source", ParamType::Text, "key:<name> or note:<channel>:<note>"),
            optional("target", ParamType::Text, "slot, scene or stop, instead of source"),
            optional("track", ParamType::Track, "track id or name, for slot and stop"),
            optional("scene", ParamType::Integer, "scene number, from 1, for slot and scene"),
        ],
        kind: CommandKind::Edit,
        run: mapping::remove_mapping,
    },
    CommandSpec {
        id: "launcher.mappings",
        label: "List the keys and MIDI notes mapped to slots, scenes and stop buttons (source, target, track, scene, whether the target is gone)",
        params: &[],
        kind: CommandKind::Read,
        run: mapping::list_mappings,
    },
    CommandSpec {
        id: "devices.list",
        label: "List the built-in effects, instruments and MIDI effects (id, name, role) with their parameters (id, range, default, unit, options)",
        params: &[],
        kind: CommandKind::Read,
        run: devices::list,
    },
    CommandSpec {
        id: "devices.sdNodes",
        label: "Describe every .sd SDK node: ports, units, defaults, configuration examples and processing rules",
        params: &[],
        kind: CommandKind::Read,
        run: devices::sd_nodes,
    },
    CommandSpec {
        id: "track.devices",
        label: "List the devices on a track in order, with each built-in and .sd device's role and parameter values",
        params: &[TRACK],
        kind: CommandKind::Read,
        run: devices::track_devices,
    },
    CommandSpec {
        id: "device.add",
        label: "Add a built-in effect, instrument or MIDI effect to a track; MIDI effects go first and change the notes, the instrument right after them plays the notes, an effect goes last unless a position is given",
        params: &[
            TRACK,
            optional(
                "id",
                ParamType::Text,
                "id of a built-in effect, instrument or MIDI effect, from devices.list (not needed with a preset)",
            ),
            optional(
                "position",
                ParamType::DevicePath,
                "where in the chain, from 0 (default: last; an instrument always goes right after the MIDI effects, a MIDI effect among them), or inside a group as a path such as \"2/1/3\"",
            ),
            optional("preset", ParamType::Text, "a preset file to load: DX7 voice data (.syx) for the FM synth"),
            optional("voice", ParamType::Integer, "which voice of a preset bank, from 1 (default 1)"),
        ],
        kind: CommandKind::Edit,
        run: devices::add,
    },
    CommandSpec {
        id: "device.addSd",
        label: "Load a data-only .sd AudioEffect and add it to a track",
        params: &[
            TRACK,
            param("path", ParamType::Text, "path to a validated .sd AudioEffect graph"),
            optional(
                "position",
                ParamType::DevicePath,
                "where in the chain, from 0 (default: last; an instrument always goes right after the MIDI effects, a MIDI effect among them, an effect after both), or inside a group as a path such as \"2/1/3\"",
            ),
        ],
        kind: CommandKind::Edit,
        run: devices::add_sd,
    },
    CommandSpec {
        id: "device.updateSd",
        label: "Update an embedded .sd device to a newer revision and migrate its saved parameter values",
        params: &[
            TRACK,
            DEVICE,
            param(
                "path",
                ParamType::Text,
                "path to a newer .sd file with the same device id and a migration from the embedded revision",
            ),
        ],
        kind: CommandKind::Edit,
        run: devices::update_sd,
    },
    CommandSpec {
        id: "track.add",
        label: "Add an audio or MIDI track with its mixer channel, clip slots and arrangement lane, optionally with a device (an instrument or MIDI effect makes a MIDI track, an effect an audio track)",
        params: &[
            optional("kind", ParamType::Text, "audio or midi (default: from the device)"),
            optional("name", ParamType::Text, "the track's name (default: the device's, or Audio n / MIDI n)"),
            optional(
                "after",
                ParamType::Track,
                "put it after this track, in its group (default: after the last regular track)",
            ),
            optional(
                "device",
                ParamType::Text,
                "the id of a built-in effect or instrument, from devices.list, or a plug-in from plugins.list",
            ),
            optional("preset", ParamType::Text, "a preset file for the device (DX7 voice data for the FM synth)"),
            optional("sd", ParamType::Text, "a data-only .sd AudioEffect graph to add to the new audio track"),
            optional("voice", ParamType::Integer, "which voice of a preset bank, from 1"),
        ],
        kind: CommandKind::Edit,
        run: files::add_track,
    },
    CommandSpec {
        id: "project.missingFiles",
        label: "List the audio files the project's clips name that cannot be found, with where they were and how many clips play each",
        params: &[],
        kind: CommandKind::Read,
        run: crate::relink::missing_files,
    },
    CommandSpec {
        id: "project.relinkFiles",
        label: "Point every clip that plays a missing (or any) audio file at another file, as one undo step; library.findFiles finds candidates",
        params: &[
            param("files", ParamType::Texts, "the files as project.missingFiles names them"),
            param("to", ParamType::Texts, "for each, the file to play instead (the same order); at most 4096"),
        ],
        kind: CommandKind::Edit,
        run: crate::relink::relink_files,
    },
    CommandSpec {
        id: "clip.addFiles",
        label: "Place samples and MIDI files as clips: on a track (or new tracks when none is given), one after another from a beat in the arrangement or down the clip slots from a scene; separate gives each clip a track of its own",
        params: &[
            param(
                "paths",
                ParamType::Texts,
                "the files: samples (wav, aiff, flac, mp3, ogg) and MIDI files (.mid); at most 256",
            ),
            optional(
                "track",
                ParamType::Track,
                "the track to place them on; files it cannot hold go on a new track after it (default: new tracks)",
            ),
            optional("start", ParamType::Number, "arrangement beat the first clip starts at"),
            optional("scene", ParamType::Integer, "scene number, from 1, of the first clip slot"),
            optional(
                "separate",
                ParamType::Bool,
                "one track per clip, side by side, instead of one after another (default false)",
            ),
            optional("stack", ParamType::Bool, "in the arrangement, keep the clips the new ones cover (default false)"),
            optional(
                "warp",
                ParamType::Text,
                "stretch mode for samples with a known tempo: repitch, rhythmic (default), tonal, texture, smooth, high-quality",
            ),
        ],
        kind: CommandKind::Edit,
        run: files::add_files,
    },
    CommandSpec {
        id: "clip.create",
        label: "Make an empty clip on a track that holds clips: MIDI on a notes track, audio with no file yet on an audio track; in an empty clip slot, or at a beat in the arrangement, replacing what it covers unless stacked",
        params: &[
            TRACK,
            optional("scene", ParamType::Integer, "scene number, from 1, of an empty clip slot"),
            optional("start", ParamType::Number, "arrangement beat it starts at, at least 0"),
            optional("length", ParamType::Number, "length in beats (default one bar of the time signature)"),
            optional("stack", ParamType::Bool, "in the arrangement, keep the clips the new one covers (default false)"),
        ],
        kind: CommandKind::Edit,
        run: files::create_clip,
    },
    CommandSpec {
        id: "preview.play",
        label: "Preview a sample or a MIDI file once, at its own speed, after the main channel; a MIDI file plays through a track's instrument or the FM synth",
        params: &[
            param("path", ParamType::Text, "the sample or MIDI file"),
            optional("from", ParamType::Number, "seconds into the file to start at (default 0)"),
            optional("track", ParamType::Track, "a MIDI file plays through this track's instrument when it has one"),
            optional("gain", ParamType::Number, "level in dB (default -6)"),
        ],
        kind: CommandKind::Transport,
        run: preview::play,
    },
    CommandSpec {
        id: "preview.setGain",
        label: "Change the level of the preview playing now, gliding there without a click; the next preview gets its own gain",
        params: &[param("gain", ParamType::Number, "level in dB, at most 12")],
        kind: CommandKind::Transport,
        run: preview::set_gain,
    },
    CommandSpec {
        id: "preview.stop",
        label: "Stop the preview",
        params: &[],
        kind: CommandKind::Transport,
        run: preview::stop,
    },
    CommandSpec {
        id: "preview.noteOn",
        label: "Play a note, or a chord, through a track's instrument and hold it until preview.noteOff, as the piano roll's key column does",
        params: &[
            TRACK,
            optional("key", ParamType::Integer, "the key to play, 0 to 127 (60 is C3)"),
            optional("keys", ParamType::Integers, "instead: several keys to play together, 1 to 128 of them"),
            optional("velocity", ParamType::Number, "how hard, 1 to 127 (default 100)"),
        ],
        kind: CommandKind::Transport,
        run: preview::note_on,
    },
    CommandSpec {
        id: "preview.noteOff",
        label: "Let go of the note preview.noteOn started on a track, or of every held preview note",
        params: &[TRACK, optional("key", ParamType::Integer, "the key to let go of; every held key when left out")],
        kind: CommandKind::Transport,
        run: preview::note_off,
    },
    CommandSpec {
        id: "device.remove",
        label: "Remove a device from a track",
        params: &[TRACK, DEVICE],
        kind: CommandKind::Edit,
        run: devices::remove,
    },
    CommandSpec {
        id: "device.removeMany",
        label: "Remove several devices from a track as one undo step",
        params: &[TRACK, DEVICES],
        kind: CommandKind::Edit,
        run: devices::remove_many,
    },
    CommandSpec {
        id: "device.copy",
        label: "Copy devices of a track, with their parameters, automation and a running plug-in's state as it is now, for device.paste",
        params: &[TRACK, DEVICES],
        kind: CommandKind::Read,
        run: devices::copy,
    },
    CommandSpec {
        id: "device.paste",
        label: "Put the copied devices into a track's chain, in the order they were copied, as one undo step",
        params: &[TRACK, POSITION],
        kind: CommandKind::Edit,
        run: devices::paste,
    },
    CommandSpec {
        id: "device.group",
        label: "Put consecutive devices of a track (or of a group's branch) into a new device group of one branch, in the place of the first; the sound does not change",
        params: &[
            TRACK,
            DEVICE,
            optional("count", ParamType::Integer, "how many consecutive devices from that one, at least 1 (default 1)"),
            optional("name", ParamType::Text, "the group's name (default: the device's, or Group)"),
        ],
        kind: CommandKind::Edit,
        run: device_groups::group,
    },
    CommandSpec {
        id: "device.ungroup",
        label: "Put the devices of a device group of one branch in its place",
        params: &[TRACK, GROUP],
        kind: CommandKind::Edit,
        run: device_groups::ungroup,
    },
    CommandSpec {
        id: "device.addGroup",
        label: "Add an empty device group (one empty branch, a clean passthrough) to a track or inside a group",
        params: &[TRACK, POSITION, optional("name", ParamType::Text, "the group's name (default Group)")],
        kind: CommandKind::Edit,
        run: device_groups::add_group,
    },
    CommandSpec {
        id: "device.move",
        label: "Move a device along a track's chain, between a group's branches, or into or out of a group",
        params: &[
            TRACK,
            DEVICE,
            param(
                "to",
                ParamType::DevicePath,
                "the position to move it before: an index on the track, or a path such as \"2/1/0\" into a group's branch",
            ),
        ],
        kind: CommandKind::Edit,
        run: device_groups::move_device,
    },
    CommandSpec {
        id: "device.duplicate",
        label: "Copy a device (a group with everything in it) next to itself; a plug-in's copy starts with its state as it is now",
        params: &[TRACK, DEVICE],
        kind: CommandKind::Edit,
        run: device_groups::duplicate,
    },
    CommandSpec {
        id: "device.moveToTrack",
        label: "Move a device (a group with everything in it) to another track, or copy it there; it goes where its kind goes on that track, a running plug-in's state with it, as one undo step",
        params: &[
            TRACK,
            DEVICE,
            param("to_track", ParamType::Track, "the track to move or copy it to, by id or name"),
            optional("copy", ParamType::Bool, "true copies it and leaves it where it is (default false)"),
        ],
        kind: CommandKind::Edit,
        run: devices::move_to_track,
    },
    CommandSpec {
        id: "device.duplicateMany",
        label: "Copy several devices of a track (groups with everything in them) right after the last of them, in their order, as one undo step; the device clipboard stays as it was",
        params: &[TRACK, DEVICES],
        kind: CommandKind::Edit,
        run: devices::duplicate_many,
    },
    CommandSpec {
        id: "branch.add",
        label: "Add an empty branch to a device group",
        params: &[
            TRACK,
            GROUP,
            optional("position", ParamType::Integer, "where among the branches, from 0 (default: last)"),
            optional("name", ParamType::Text, "the branch's name (default Branch N)"),
        ],
        kind: CommandKind::Edit,
        run: device_groups::branch_add,
    },
    CommandSpec {
        id: "branch.remove",
        label: "Remove a branch of a device group with its devices; the last branch stays",
        params: &[TRACK, GROUP, BRANCH],
        kind: CommandKind::Edit,
        run: device_groups::branch_remove,
    },
    CommandSpec {
        id: "branch.duplicate",
        label: "Copy a branch of a device group next to itself",
        params: &[TRACK, GROUP, BRANCH],
        kind: CommandKind::Edit,
        run: device_groups::branch_duplicate,
    },
    CommandSpec {
        id: "branch.rename",
        label: "Rename a branch of a device group",
        params: &[TRACK, GROUP, BRANCH, param("name", ParamType::Text, "the new name")],
        kind: CommandKind::Edit,
        run: device_groups::branch_rename,
    },
    CommandSpec {
        id: "branch.setVolume",
        label: "Set a branch's volume in a device group, as track.setVolume",
        params: &[
            TRACK,
            GROUP,
            BRANCH,
            param("db", ParamType::Decibels, "level in dB, at most +6; \"-inf\" or below -150 is silence"),
        ],
        kind: CommandKind::Edit,
        run: device_groups::branch_set_volume,
    },
    CommandSpec {
        id: "branch.setPan",
        label: "Set a branch's pan in a device group, as track.setPan",
        params: &[TRACK, GROUP, BRANCH, param("pan", ParamType::Number, "-1 hard left, 0 centre, 1 hard right")],
        kind: CommandKind::Edit,
        run: device_groups::branch_set_pan,
    },
    CommandSpec {
        id: "branch.setMute",
        label: "Mute or unmute a branch of a device group; its devices keep running",
        params: &[TRACK, GROUP, BRANCH, param("mute", ParamType::Bool, "true mutes the branch")],
        kind: CommandKind::Edit,
        run: device_groups::branch_set_mute,
    },
    CommandSpec {
        id: "branch.setSolo",
        label: "Solo a branch within its device group: while any branch is soloed only soloed branches sound",
        params: &[TRACK, GROUP, BRANCH, param("solo", ParamType::Bool, "true solos the branch")],
        kind: CommandKind::Edit,
        run: device_groups::branch_set_solo,
    },
    CommandSpec {
        id: "device.setEnabled",
        label: "Switch a device on or off; a device that is off passes the sound through",
        params: &[TRACK, DEVICE, param("enabled", ParamType::Bool, "false switches the device off")],
        kind: CommandKind::Edit,
        run: devices::set_enabled,
    },
    CommandSpec {
        id: "device.setNamFile",
        label: "Load a .nam model into a NAM Amp Modeler, or a .wav impulse response into a NAM Cab Loader (docs/spec/nam-amp-and-cab.md)",
        params: &[
            TRACK,
            DEVICE,
            param("path", ParamType::Text, "path to a .nam model file, or a .wav impulse response for the Cab Loader"),
        ],
        kind: CommandKind::Edit,
        run: devices::set_file,
    },
    CommandSpec {
        id: "device.file",
        label: "The file a NAM Amp Modeler or NAM Cab Loader runs: its absolute path (empty when none is set) and the model's or cab's name",
        params: &[TRACK, DEVICE],
        kind: CommandKind::Read,
        run: devices::device_file,
    },
    CommandSpec {
        id: "device.loadSample",
        label: "Load an audio file into a Sampler instrument",
        params: &[TRACK, DEVICE, param("path", ParamType::Text, "sample audio file")],
        kind: CommandKind::Edit,
        run: devices::load_sample,
    },
    CommandSpec {
        id: "device.setSampleSlices",
        label: "Replace a Sampler's manual slice boundaries",
        params: &[
            TRACK,
            DEVICE,
            param("points", ParamType::Numbers, "normalized source positions strictly between 0 and 1"),
            optional("continue", ParamType::Bool, "continue the preceding slice drag undo step"),
        ],
        kind: CommandKind::Edit,
        run: devices::set_sample_slices,
    },
    CommandSpec {
        id: "device.recordSample",
        label: "Record the armed track's selected audio input into its Sampler; stop commits the source as one undo step",
        params: &[TRACK, DEVICE, param("on", ParamType::Bool, "start or stop sample recording")],
        kind: CommandKind::Transport,
        run: crate::recording::record_sample,
    },
    CommandSpec {
        id: "device.setParam",
        label: "Set a parameter of a device on a track, heard from the next block; the value is limited to the parameter's range",
        params: &[
            TRACK,
            DEVICE,
            param(
                "param",
                ParamType::Text,
                "parameter id: a built-in effect or instrument's from devices.list, an .sd device's stable id, or a plug-in's number from device.pluginParams",
            ),
            param(
                "value",
                ParamType::Number,
                "value in the parameter's unit (dB, Hz, 0..1 for percentages, option index for choices); a plug-in's in its own range, from device.pluginParams",
            ),
            optional(
                "continuous",
                ParamType::Bool,
                "true while a UI control is being dragged; updates share one undo step",
            ),
            optional(
                "gesture",
                ParamType::Text,
                "shared custom-control gesture id, 1..128 bytes; continuous=false ends it",
            ),
        ],
        kind: CommandKind::Edit,
        run: devices::set_param,
    },
    CommandSpec {
        id: "device.setAutomation",
        label: "Automate a built-in, .sd or plug-in device parameter over the arrangement: points of beat and value, a straight line between them; no points removes the automation (automation.addPoint edits one point)",
        params: &[
            TRACK,
            DEVICE,
            param(
                "param",
                ParamType::Text,
                "parameter id: a built-in effect or instrument's from devices.list, an .sd device's stable id, or a plug-in's number from device.pluginParams",
            ),
            param(
                "points",
                ParamType::Numbers,
                "a beat then a value for each point, in the parameter's range: [0, 0.2, 4, 0.8] moves from 0.2 at beat 0 to 0.8 at beat 4; two points at one beat jump; [] removes the lane",
            ),
        ],
        kind: CommandKind::Edit,
        run: devices::set_automation,
    },
    CommandSpec {
        id: "automation.addPoint",
        label: "Add a point to a parameter's automation (envelope) over the arrangement, making the lane if it has none; the value moves from point to point along each segment's curve",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            AUTOMATED,
            param("beat", ParamType::Number, "arrangement beat, from 0; a second point at one beat makes a jump"),
            param(
                "value",
                ParamType::Number,
                "value in the unit the parameter's set command takes: volume in dB (at most +6; below -150 is silence), pan -1 to 1, mute 0 or 1, a device's as device.setParam",
            ),
            CURVE,
        ],
        kind: CommandKind::Edit,
        run: automation::add_point,
    },
    CommandSpec {
        id: "automation.movePoint",
        label: "Move a point of a parameter's automation to another beat or value, or change its curve",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            AUTOMATED,
            POINT,
            optional("beat", ParamType::Number, "new arrangement beat"),
            optional("value", ParamType::Number, "new value, as for automation.addPoint"),
            CURVE,
        ],
        kind: CommandKind::Edit,
        run: automation::move_point,
    },
    CommandSpec {
        id: "automation.deletePoint",
        label: "Delete a point of a parameter's automation; the lane goes with its last point",
        params: &[TRACK, AUTOMATED_DEVICE, AUTOMATED, POINT],
        kind: CommandKind::Edit,
        run: automation::delete_point,
    },
    CommandSpec {
        id: "automation.movePoints",
        label: "Move several points of one parameter's automation together, by a number of beats and by a value, in one undo step; values stay in the parameter's range",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            AUTOMATED,
            param(
                "points",
                ParamType::Numbers,
                "point indices on the lane, from 0 in beat order, as in automation.list",
            ),
            optional(
                "beats",
                ParamType::Number,
                "beats to move the points by: later above 0, earlier below; they must stay from beat 0",
            ),
            optional(
                "value",
                ParamType::Number,
                "how much to change each value by, in the unit of automation.addPoint (dB for the volume, where silence stays silent)",
            ),
            optional("curve", ParamType::Number, "a curve to give every moved point's segment, -1 to 1"),
        ],
        kind: CommandKind::Edit,
        run: automation::move_points,
    },
    CommandSpec {
        id: "automation.deletePoints",
        label: "Delete several points of one parameter's automation in one undo step; the lane goes with its last point",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            AUTOMATED,
            param(
                "points",
                ParamType::Numbers,
                "point indices on the lane, from 0 in beat order, as in automation.list",
            ),
        ],
        kind: CommandKind::Edit,
        run: automation::delete_points,
    },
    CommandSpec {
        id: "automation.clear",
        label: "Remove a parameter's automation, or without param every lane of the track's mixer or of the device",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            optional("param", ParamType::Text, "the parameter, as for automation.addPoint; left out for every lane"),
        ],
        kind: CommandKind::Edit,
        run: automation::clear,
    },
    CommandSpec {
        id: "automation.setEnabled",
        label: "Turn a parameter's automation off or on again; a manual change to an automated parameter turns it off, and the value set by hand holds until it is on again",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            AUTOMATED,
            param("enabled", ParamType::Bool, "false turns the lane off, true on"),
        ],
        kind: CommandKind::Edit,
        run: automation::set_enabled,
    },
    CommandSpec {
        id: "automation.list",
        label: "List a track's automation: its mixer's lanes and each device's, with their points (beat, value, curve) and whether they play",
        params: &[TRACK],
        kind: CommandKind::Read,
        run: automation::list,
    },
    CommandSpec {
        id: "device.pluginParams",
        label: "List the parameters of a running plug-in device: id, name, range, default, current value and the plug-in's own text for it",
        params: &[TRACK, DEVICE],
        kind: CommandKind::Plugin,
        run: devices::plugin_params,
    },
    CommandSpec {
        id: "device.addPlugin",
        label: "Add an installed plug-in to a track; an instrument goes right after the MIDI effects and plays the track's notes, an effect goes last unless a position is given. It starts while the project plays",
        params: &[
            TRACK,
            param("plugin", ParamType::Text, "plug-in name, id or file, from plugins.list"),
            optional(
                "position",
                ParamType::DevicePath,
                "where in the chain, from 0 (default: last; an instrument always goes right after the MIDI effects), or inside a group as a path such as \"2/1/3\"",
            ),
        ],
        kind: CommandKind::Edit,
        run: devices::add_plugin,
    },
    CommandSpec {
        id: "device.plugin",
        label: "Where a plug-in device stands: loading, running, missing, crashed or failed, the installed plug-in found for it, its latency, whether its editor is open",
        params: &[TRACK, DEVICE],
        kind: CommandKind::Read,
        run: devices::plugin_status,
    },
    CommandSpec {
        id: "device.openEditor",
        label: "Open a plug-in device's editor in a window, or inside the device view when Suhina embeds editors (Options › Settings › Plug-ins); changes there are heard at once",
        params: &[TRACK, DEVICE],
        kind: CommandKind::Plugin,
        run: devices::open_editor,
    },
    CommandSpec {
        id: "device.closeEditor",
        label: "Close a plug-in device's editor",
        params: &[TRACK, DEVICE],
        kind: CommandKind::Plugin,
        run: devices::close_editor,
    },
    CommandSpec {
        id: "device.restart",
        label: "Restart a plug-in device that crashed or stopped answering, in a fresh process with its last state",
        params: &[TRACK, DEVICE],
        kind: CommandKind::Plugin,
        run: devices::restart,
    },
    CommandSpec {
        id: "track.setSend",
        label: "Set how much of a track goes to a return track (its send); the return runs its devices over what it is sent",
        params: &[
            TRACK,
            RETURN,
            param("level", ParamType::Decibels, "send level in dB, at most +6; \"-inf\" or below -150 is silence"),
            CONTINUE,
        ],
        kind: CommandKind::Edit,
        run: crate::sends::set_send,
    },
    CommandSpec {
        id: "track.setSendMode",
        label: "Take a track's send to a return before its volume and pan (pre) or after them (post, the default)",
        params: &[TRACK, RETURN, param("mode", ParamType::Text, "\"pre\" or \"post\"")],
        kind: CommandKind::Edit,
        run: crate::sends::set_send_mode,
    },
    CommandSpec {
        id: "return.add",
        label: "Add a return track, after every other track, playing into the main channel; tracks reach it through sends",
        params: &[optional("name", ParamType::Text, "its name (default: A Return, B Return and so on)")],
        kind: CommandKind::Edit,
        run: crate::sends::add_return,
    },
    CommandSpec {
        id: "return.remove",
        label: "Remove a return track and every send to it",
        params: &[RETURN],
        kind: CommandKind::Edit,
        run: crate::sends::remove_return,
    },
    CommandSpec {
        id: "return.rename",
        label: "Rename a return track",
        params: &[RETURN, param("name", ParamType::Text, "the new name, not empty")],
        kind: CommandKind::Edit,
        run: crate::sends::rename_return,
    },
    CommandSpec {
        id: "note.list",
        label: "List a MIDI clip's notes: index, time and length in beats, key (60 is C3), velocity 1 to 127 and whether it is muted",
        params: &[CLIP_TRACK, CLIP, CLIP_INDEX, CLIP_SCENE],
        kind: CommandKind::Read,
        run: notes::list,
    },
    CommandSpec {
        id: "note.add",
        label: "Add notes to a MIDI clip; they go after its other notes, and the result gives their indices",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            param("notes", ParamType::Notes, "the notes, each {time, length, key, velocity?}; at most 10000"),
        ],
        kind: CommandKind::Edit,
        run: notes::add,
    },
    CommandSpec {
        id: "note.delete",
        label: "Delete notes of a MIDI clip; later notes move up in the list",
        params: &[CLIP_TRACK, CLIP, CLIP_INDEX, CLIP_SCENE, NOTES],
        kind: CommandKind::Edit,
        run: notes::delete,
    },
    CommandSpec {
        id: "note.move",
        label: "Move notes of a MIDI clip in time and in pitch; fails if any note would start before 0 or leave keys 0 to 127",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            NOTES,
            optional("beats", ParamType::Number, "beats to move by, later when positive"),
            optional("semitones", ParamType::Integer, "semitones to move by, higher when positive"),
            optional(
                "replace",
                ParamType::Bool,
                "true removes a note the moved ones land on: the same key and the same start (default false: \
                 both play)",
            ),
        ],
        kind: CommandKind::Edit,
        run: notes::move_notes,
    },
    CommandSpec {
        id: "note.resize",
        label: "Set the length of notes of a MIDI clip, change it by a number of beats, or move their starts and keep their ends",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            NOTES,
            optional("length", ParamType::Number, "the new length in beats, above 0"),
            optional("by", ParamType::Number, "instead: beats to add to each length (negative shortens)"),
            optional(
                "startBy",
                ParamType::Number,
                "instead: beats to move each start by (negative is earlier), each end kept where it is",
            ),
        ],
        kind: CommandKind::Edit,
        run: notes::resize,
    },
    CommandSpec {
        id: "note.setVelocity",
        label: "Set the velocity of notes of a MIDI clip, or change it by an amount, each kept within 1 to 127",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            NOTES,
            optional("velocity", ParamType::Number, "the new velocity, 1 to 127"),
            optional("by", ParamType::Number, "instead: an amount to add to each velocity (negative lowers)"),
            optional(
                "velocities",
                ParamType::Numbers,
                "instead: one velocity (1 to 127) per entry of notes, in the same order",
            ),
        ],
        kind: CommandKind::Edit,
        run: notes::set_velocity,
    },
    CommandSpec {
        id: "note.quantize",
        label: "Quantise notes of a MIDI clip: move their starts, and with ends their ends, towards the nearest grid line",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            optional("notes", ParamType::Integers, "note indices, from note.list; every note when left out"),
            param("grid", ParamType::Text, "the grid, as launcher.setQuantization takes it: 1/16, 1/8T, 1 bar, ..."),
            optional("amount", ParamType::Number, "how far towards the line, 0 to 100 percent (default 100: onto it)"),
            optional("ends", ParamType::Bool, "also move each note's end to the nearest line (default false)"),
        ],
        kind: CommandKind::Edit,
        run: notes::quantize,
    },
    CommandSpec {
        id: "note.copy",
        label: "Copy notes of a MIDI clip for note.paste; changes nothing",
        params: &[CLIP_TRACK, CLIP, CLIP_INDEX, CLIP_SCENE, NOTES],
        kind: CommandKind::Read,
        run: notes::copy,
    },
    CommandSpec {
        id: "note.cut",
        label: "Copy notes of a MIDI clip for note.paste and delete them",
        params: &[CLIP_TRACK, CLIP, CLIP_INDEX, CLIP_SCENE, NOTES],
        kind: CommandKind::Edit,
        run: notes::cut,
    },
    CommandSpec {
        id: "note.paste",
        label: "Add the copied notes to a MIDI clip, keeping their spacing; the result gives their indices",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            optional(
                "time",
                ParamType::Number,
                "where the first copied note starts, in beats; default: right after the copied notes",
            ),
        ],
        kind: CommandKind::Edit,
        run: notes::paste,
    },
    CommandSpec {
        id: "note.duplicate",
        label: "Add a copy of notes of a MIDI clip right after their end, or beats and semitones away; the result gives the copies' indices",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            NOTES,
            optional("beats", ParamType::Number, "instead of right after: beats from the notes to the copies"),
            optional("semitones", ParamType::Integer, "semitones from the notes to the copies, higher when positive"),
        ],
        kind: CommandKind::Edit,
        run: notes::duplicate,
    },
    CommandSpec {
        id: "note.split",
        label: "Split notes of a MIDI clip at a beat, each note that spans it into two; the result gives the new notes' indices",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            NOTES,
            param("at", ParamType::Number, "the beat to split at, in beats from the content's start"),
        ],
        kind: CommandKind::Edit,
        run: notes::split,
    },
    CommandSpec {
        id: "note.setMuted",
        label: "Mute or unmute notes of a MIDI clip; with muted left out each named note is flipped; the result gives the state of each",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            NOTES,
            optional("muted", ParamType::Bool, "true mutes them, false unmutes them; left out flips each note"),
        ],
        kind: CommandKind::Edit,
        run: note_tools::set_muted,
    },
    CommandSpec {
        id: "note.humanize",
        label: "Move the starts and velocities of notes of a MIDI clip by a random draw; the same seed gives the same result, and the result gives the seed",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            NOTE_SELECTION,
            optional("time", ParamType::Number, "how far a start may move, in beats, 0 to 1 (default 0.02)"),
            optional("velocity", ParamType::Number, "how far a velocity may move, in MIDI steps, 0 to 127 (default 8)"),
            optional(
                "seed",
                ParamType::Integer,
                "the draw to repeat, 0 or more; left out, a fresh one is used and returned",
            ),
        ],
        kind: CommandKind::Edit,
        run: note_tools::humanize,
    },
    CommandSpec {
        id: "note.fitToScale",
        label: "Move notes of a MIDI clip to the nearest key of a scale over a root; a tie goes to the lower key",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            NOTE_SELECTION,
            optional(
                "root",
                ParamType::Text,
                "the key the scale starts on: a name such as C, F# or Bb, or 0 to 11 (default C; with neither root nor scale, the clip's own scale when it has one)",
            ),
            optional(
                "scale",
                ParamType::Text,
                "major, minor, harmonicMinor, melodicMinor, dorian, phrygian, lydian, mixolydian, locrian, majorPentatonic, minorPentatonic or blues (default major)",
            ),
        ],
        kind: CommandKind::Edit,
        run: note_tools::fit_to_scale,
    },
    CommandSpec {
        id: "note.invert",
        label: "Mirror the keys of notes of a MIDI clip around the middle of the range they cover, each kept within 0 to 127",
        params: &[CLIP_TRACK, CLIP, CLIP_INDEX, CLIP_SCENE, NOTE_SELECTION],
        kind: CommandKind::Edit,
        run: note_tools::invert,
    },
    CommandSpec {
        id: "note.reverse",
        label: "Turn notes of a MIDI clip round in time inside their own span (the earliest start to the latest end), lengths unchanged",
        params: &[CLIP_TRACK, CLIP, CLIP_INDEX, CLIP_SCENE, NOTE_SELECTION],
        kind: CommandKind::Edit,
        run: note_tools::reverse,
    },
    CommandSpec {
        id: "note.legato",
        label: "Make each note of a MIDI clip last until the next note of its key starts, at least a hundredth of a beat; the last of a key keeps its length",
        params: &[CLIP_TRACK, CLIP, CLIP_INDEX, CLIP_SCENE, NOTE_SELECTION],
        kind: CommandKind::Edit,
        run: note_tools::legato,
    },
    CommandSpec {
        id: "note.stretch",
        label: "Stretch notes of a MIDI clip in time about the earliest of their starts; 2 is twice as long, 0.5 half of it",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            NOTE_SELECTION,
            param("factor", ParamType::Number, "how much longer or shorter, 0.01 to 100"),
        ],
        kind: CommandKind::Edit,
        run: note_tools::stretch,
    },
    CommandSpec {
        id: "note.randomizeVelocity",
        label: "Move the velocities of notes of a MIDI clip by a random draw, each kept within 1 to 127; the same seed gives the same result",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            NOTE_SELECTION,
            optional("amount", ParamType::Number, "how far a velocity may move, in MIDI steps, 0 to 127 (default 16)"),
            optional(
                "seed",
                ParamType::Integer,
                "the draw to repeat, 0 or more; left out, a fresh one is used and returned",
            ),
        ],
        kind: CommandKind::Edit,
        run: note_tools::randomize_velocity,
    },
    CommandSpec {
        id: "note.rampVelocity",
        label: "Spread the velocities of notes of a MIDI clip evenly from one value to another in time order; one note takes the last value",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            NOTE_SELECTION,
            param("from", ParamType::Integer, "the velocity of the first note in time order, 1 to 127"),
            param("to", ParamType::Integer, "the velocity of the last note in time order, 1 to 127"),
        ],
        kind: CommandKind::Edit,
        run: note_tools::ramp_velocity,
    },
    CommandSpec {
        id: "clip.scale",
        label: "The scale of a MIDI clip (root and mode, or null for none) and the project's default scale for new clips",
        params: &[CLIP_TRACK, CLIP, CLIP_INDEX, CLIP_SCENE],
        kind: CommandKind::Read,
        run: crate::scales::clip_scale,
    },
    CommandSpec {
        id: "clip.setScale",
        label: "Set the scale of a MIDI clip (the piano roll marks its keys), or clear it with off; a content other clips also play is copied for this clip first",
        params: &[
            CLIP_TRACK,
            CLIP,
            CLIP_INDEX,
            CLIP_SCENE,
            SCALE_ROOT,
            SCALE_MODE,
            optional("off", ParamType::Bool, "true clears the scale; not with root or mode"),
        ],
        kind: CommandKind::Edit,
        run: crate::scales::set_scale,
    },
    CommandSpec {
        id: "project.setDefaultScale",
        label: "Set the scale new MIDI clips take (clip.create, recorded and dropped MIDI clips), or clear it with off; clips that exist keep theirs",
        params: &[
            SCALE_ROOT,
            SCALE_MODE,
            optional("off", ParamType::Bool, "true clears the default scale; not with root or mode"),
        ],
        kind: CommandKind::Edit,
        run: crate::scales::set_default_scale,
    },
    CommandSpec { id: "edit.undo", label: "Undo the last edit", params: &[], kind: CommandKind::History, run: undo },
    CommandSpec {
        id: "edit.redo",
        label: "Redo the last undone edit",
        params: &[],
        kind: CommandKind::History,
        run: redo,
    },
];

fn list(_: &mut Session, _: &Value) -> Result<Value, CommandError> {
    Ok(Value::Array(
        COMMANDS
            .iter()
            .map(|c| {
                json!({ "id": c.id, "label": c.label, "params": c.input_schema()["properties"], "kind": c.kind.name() })
            })
            .collect(),
    ))
}

fn save(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let copy = match params.get("copy") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("copy must be true or false".into()),
    };
    let path = match params.get("path") {
        Some(Value::String(path)) if !path.trim().is_empty() => std::path::PathBuf::from(path.trim()),
        Some(Value::String(_)) | None | Some(Value::Null) => match session.source() {
            Some(source) if suhina_project_file::is_project_file(source) => source.to_path_buf(),
            _ => return Err("the project has no .suhina file yet; give a path to save it to".into()),
        },
        Some(_) => return Err("path must be text".into()),
    };
    let written = session.save(&path, copy)?;
    Ok(json!({ "path": written.display().to_string(), "copy": copy }))
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
        let name = p.tracks.iter().find(|t| t.id == track).map(|t| t.name.as_str()).unwrap_or_default();
        for (index, clip) in lane.clips.iter().enumerate() {
            out.push(clip_json(p, name, track, index, clip));
        }
    }
    Ok(Value::Array(out))
}

/// `arrangement.audioOverlaps`: [`Project::audio_overlaps`], each clip named as `clip.move`
/// addresses it (ADR 0028).
fn audio_overlaps(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = session.project();
    let only = match params.get("track") {
        Some(_) => Some(track_id(p, params)?),
        None => None,
    };
    let track_name = |id: TrackId| p.tracks.iter().find(|t| t.id == id).map(|t| t.name.as_str()).unwrap_or_default();
    let clip = |place: suhina_model::OverlapClip| {
        let found = p.arrangement.lanes.get(place.lane).and_then(|l| l.clips.get(place.index));
        let name = found.and_then(|c| p.content(c.content)).map(|c| c.name.as_str());
        json!({
            "id": found.map(|c| c.id.0),
            "track": place.track.0,
            "track_name": track_name(place.track),
            "index": place.index,
            "name": name,
        })
    };
    let found = p.audio_overlaps();
    let overlaps: Vec<Value> = found
        .pairs
        .iter()
        .filter(|o| only.is_none_or(|t| t == o.track))
        .map(|o| {
            json!({
                "track": o.track.0,
                "track_name": track_name(o.track),
                "start": o.start,
                "end": o.end,
                "kind": if o.contained { "contained" } else { "partial" },
                "clips": [clip(o.first), clip(o.second)],
            })
        })
        .collect();
    Ok(json!({ "overlaps": overlaps, "truncated": found.truncated }))
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

fn set_tempo(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let bpm = number(params, "bpm")?;
    if !(20.0..=999.0).contains(&bpm) {
        return Err(format!("bpm must be between 20 and 999, got {bpm}").into());
    }
    session.project.transport.tempo_bpm = bpm;
    Ok(json!({ "tempo_bpm": bpm }))
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

fn set_loop_enabled(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let transport = &mut session.project.transport;
    let enabled = optional_enabled(params)?.unwrap_or(!transport.loop_enabled);
    if enabled && transport.loop_range.is_none() {
        let signature = transport.time_signature;
        let bar = f64::from(signature.numerator) * 4.0 / f64::from(signature.denominator.max(1));
        let end = if bar.is_finite() && bar > 0.0 { (4.0 * bar).min(MAX_START_BEAT) } else { 16.0 };
        transport.loop_range = Some(LoopRange { start: 0.0, end });
    }
    transport.loop_enabled = enabled;
    Ok(loop_value(&session.project))
}

fn set_volume(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let db = number_or_neg_infinity(params, "db")?;
    if db > MAX_VOLUME_DB {
        return Err(format!("db must be at most +{MAX_VOLUME_DB}, got {db}").into());
    }
    let channels = channel_ids(&session.project, params)?;
    let gain = gain_of_db(db);
    let mut off = false;
    for (_, id) in &channels {
        if let Some(channel) = session.project.channels.iter_mut().find(|c| c.id == *id) {
            channel.volume = gain;
            off |= automation::override_lane(&mut channel.automation, |l| l.param == suhina_model::automation::VOLUME);
        }
    }
    Ok(with_override(json!({ "tracks": track_ids(&channels), "volume_db": vec![to_db(gain); channels.len()] }), off))
}

fn set_main_volume(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let db = number_or_neg_infinity(params, "db")?;
    if db > MAX_VOLUME_DB {
        return Err(format!("db must be at most +{MAX_VOLUME_DB}, got {db}").into());
    }
    let Some(main) = session.project.channels.iter_mut().find(|c| c.role == ChannelRole::Master) else {
        return Err("the project has no main channel".into());
    };
    main.volume = gain_of_db(db);
    Ok(json!({ "volume_db": to_db(main.volume) }))
}

fn adjust_volume(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let db = number(params, "db")?;
    let channels = channel_ids(&session.project, params)?;
    let mut volumes = Vec::with_capacity(channels.len());
    let mut off = false;
    for (_, id) in &channels {
        let Some(channel) = session.project.channels.iter_mut().find(|c| c.id == *id) else { continue };
        let now = to_db(channel.volume).unwrap_or(f64::NEG_INFINITY);
        let next = (now + db).clamp(MIN_VOLUME_DB, MAX_VOLUME_DB);
        channel.volume = gain_of_db(next);
        off |= automation::override_lane(&mut channel.automation, |l| l.param == suhina_model::automation::VOLUME);
        volumes.push(to_db(channel.volume));
    }
    Ok(with_override(json!({ "tracks": track_ids(&channels), "volume_db": volumes }), off))
}

fn set_pan(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let pan = number(params, "pan")?;
    if !(-1.0..=1.0).contains(&pan) {
        return Err(format!("pan must be between -1 and 1, got {pan}").into());
    }
    let channels = channel_ids(&session.project, params)?;
    let mut off = false;
    for (_, id) in &channels {
        if let Some(channel) = session.project.channels.iter_mut().find(|c| c.id == *id) {
            channel.pan = pan;
            off |= automation::override_lane(&mut channel.automation, |l| l.param == suhina_model::automation::PAN);
        }
    }
    Ok(with_override(json!({ "tracks": track_ids(&channels), "pan": vec![pan; channels.len()] }), off))
}

fn adjust_pan(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let pan = number(params, "pan")?;
    if !(-1.0..=1.0).contains(&pan) {
        return Err(format!("pan must be between -1 and 1, got {pan}").into());
    }
    let channels = channel_ids(&session.project, params)?;
    let mut pans = Vec::with_capacity(channels.len());
    let mut off = false;
    for (_, id) in &channels {
        let Some(channel) = session.project.channels.iter_mut().find(|c| c.id == *id) else { continue };
        channel.pan = (channel.pan + pan).clamp(-1.0, 1.0);
        off |= automation::override_lane(&mut channel.automation, |l| l.param == suhina_model::automation::PAN);
        pans.push(channel.pan);
    }
    Ok(with_override(json!({ "tracks": track_ids(&channels), "pan": pans }), off))
}

fn set_mute(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let mute = params.get("mute").and_then(Value::as_bool).ok_or("mute must be true or false")?;
    let channels = channel_ids(&session.project, params)?;
    let mut off = false;
    for (_, id) in &channels {
        if let Some(channel) = session.project.channels.iter_mut().find(|c| c.id == *id) {
            channel.mute = mute;
            off |= automation::override_lane(&mut channel.automation, |l| l.param == suhina_model::automation::MUTE);
        }
    }
    Ok(with_override(json!({ "tracks": track_ids(&channels), "mute": mute }), off))
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

/// The mixer channel of every track `params.tracks` names, in that order, checked.
fn channel_ids(p: &Project, params: &Value) -> Result<Vec<(TrackId, ChannelId)>, CommandError> {
    let mut out = Vec::new();
    for track in track_list(p, params)? {
        let found = p.tracks.iter().find(|t| t.id == track).ok_or("the track is missing")?;
        let channel = found.channel.ok_or_else(|| format!("track {:?} has no mixer channel", found.name))?;
        out.push((track, channel));
    }
    Ok(out)
}

/// The track ids of [`channel_ids`], for a command's result.
fn track_ids(channels: &[(TrackId, ChannelId)]) -> Vec<u32> {
    channels.iter().map(|(track, _)| track.0).collect()
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
    p.tracks.iter().find(|t| t.id == track).and_then(|t| t.channel)
}

pub(crate) fn group_tracks(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let members = track_list(p, params)?;
    let parents: Vec<Option<TrackId>> =
        members.iter().map(|id| p.tracks.iter().find(|t| t.id == *id).and_then(|t| t.parent)).collect();
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

fn ungroup_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let group = track_id(p, params)?;
    let channel = channel_id_of(p, group).and_then(|id| p.channels.iter().find(|c| c.id == id));
    let Some(channel) = channel.filter(|c| c.role == ChannelRole::Submix) else {
        return Err("only a group track can be ungrouped".into());
    };
    let (group_channel, output) = (channel.id, channel.output);
    let parent = p.tracks.iter().find(|t| t.id == group).and_then(|t| t.parent);
    for track in p.tracks.iter_mut().filter(|t| t.parent == Some(group)) {
        track.parent = parent;
    }
    for c in p.channels.iter_mut().filter(|c| c.output == Some(group_channel)) {
        c.output = output;
    }
    remove_track(p, group, group_channel);
    Ok(json!({ "ungrouped": group.0 }))
}

/// Moves a track, with everything inside it, into a group or to the top level: it becomes the
/// group's track at `index` among the group's own tracks, and its channel plays into the group.
fn set_group(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let track = track_id(p, params)?;
    let role_of = |p: &Project, t: TrackId| {
        channel_id_of(p, t).and_then(|id| p.channels.iter().find(|c| c.id == id)).map(|c| c.role)
    };
    if matches!(role_of(p, track), Some(ChannelRole::Effect | ChannelRole::Master)) {
        return Err("a return track cannot go into a group".into());
    }
    let group = match params.get("group") {
        Some(Value::String(none)) if none.eq_ignore_ascii_case("none") => None,
        Some(_) => Some(track_at(p, params, "group")?),
        None => return Err("group must be a group track or \"none\"".into()),
    };
    let index = match params.get("index") {
        None => None,
        Some(v) => Some(v.as_u64().ok_or("index must be a whole number from 0")?),
    };
    let family = p.with_descendants(track);
    let output = match group {
        None => None,
        Some(g) => {
            let name = p.tracks.iter().find(|t| t.id == g).map(|t| t.name.clone()).unwrap_or_default();
            if role_of(p, g) != Some(ChannelRole::Submix) {
                return Err(format!("{name:?} is not a group track").into());
            }
            if family.contains(&g) {
                return Err(format!("{name:?} is inside the track being moved, or is that track").into());
            }
            let depth = p.group_depth(g).unwrap_or(0) + 1 + p.nesting_below(track);
            if depth > MAX_GROUP_DEPTH {
                return Err(format!(
                    "groups nest at most {MAX_GROUP_DEPTH} deep; moving into {name:?} would go deeper"
                )
                .into());
            }
            channel_id_of(p, g)
        }
    };

    let block: Vec<Track> = p.tracks.iter().filter(|t| family.contains(&t.id)).cloned().collect();
    p.tracks.retain(|t| !family.contains(&t.id));
    // The group's own tracks, top-level returns aside, and where the block goes among them.
    let is_return = |p: &Project, t: &Track| {
        t.channel.and_then(|id| p.channels.iter().find(|c| c.id == id)).is_some_and(|c| c.role == ChannelRole::Effect)
    };
    let siblings: Vec<TrackId> =
        p.tracks.iter().filter(|t| t.parent == group && !is_return(p, t)).map(|t| t.id).collect();
    let index = index.map_or(siblings.len(), |i| usize::try_from(i).unwrap_or(usize::MAX).min(siblings.len()));
    let position_of = |p: &Project, id: TrackId| p.tracks.iter().position(|t| t.id == id);
    let at = match (siblings.get(index), index.checked_sub(1).and_then(|i| siblings.get(i)), group) {
        (Some(&next), _, _) => position_of(p, next),
        // After the last sibling and everything inside it.
        (None, Some(&last), _) => p.with_descendants(last).last().and_then(|&t| position_of(p, t)).map(|i| i + 1),
        (None, None, Some(g)) => position_of(p, g).map(|i| i + 1),
        (None, None, None) => None,
    }
    .unwrap_or(p.tracks.len())
    .min(p.tracks.len());
    p.tracks.splice(at..at, block);
    if let Some(t) = p.tracks.iter_mut().find(|t| t.id == track) {
        t.parent = group;
    }
    if let Some(channel) = channel_id_of(p, track)
        && let Some(c) = p.channels.iter_mut().find(|c| c.id == channel)
    {
        c.output = output;
    }
    Ok(json!({ "track": track.0, "group": group.map(|g| g.0), "position": at }))
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

fn move_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let track = track_id(p, params)?;
    let to = params.get("to").and_then(Value::as_u64).ok_or("to must be a whole number from 0")?;
    let family = p.with_descendants(track);
    let block: Vec<Track> = p.tracks.iter().filter(|t| family.contains(&t.id)).cloned().collect();
    p.tracks.retain(|t| !family.contains(&t.id));
    let at = usize::try_from(to).unwrap_or(usize::MAX).min(p.tracks.len());
    p.tracks.splice(at..at, block);
    Ok(json!({ "track": track.0, "position": at }))
}

fn rename_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let name = params.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty());
    let name = name.ok_or("name must be a text, not empty")?.to_string();
    let track = track_id(&session.project, params)?;
    if let Some(t) = session.project.tracks.iter_mut().find(|t| t.id == track) {
        t.name.clone_from(&name);
    }
    if let Some(channel) = channel_id_of(&session.project, track)
        && let Some(c) = session.project.channels.iter_mut().find(|c| c.id == channel)
    {
        c.name.clone_from(&name);
    }
    Ok(json!({ "name": name }))
}

fn set_color(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let text = params.get("color").and_then(Value::as_str).ok_or("color must be \"#rrggbb\" or \"none\"")?;
    let color = if text.eq_ignore_ascii_case("none") {
        None
    } else {
        let hex = text.strip_prefix('#').filter(|h| h.len() == 6).ok_or("color must be \"#rrggbb\" or \"none\"")?;
        let part = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (part(0), part(2), part(4)) {
            (Some(r), Some(g), Some(b)) => Some(suhina_model::Rgb(r, g, b)),
            _ => return Err("color must be \"#rrggbb\" or \"none\"".into()),
        }
    };
    let track = track_id(&session.project, params)?;
    if let Some(t) = session.project.tracks.iter_mut().find(|t| t.id == track) {
        t.color = color;
    }
    Ok(json!({ "color": text }))
}

fn delete_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_id(&session.project, params)?;
    let p = &session.project;
    let is_group = channel_id_of(p, track)
        .and_then(|id| p.channels.iter().find(|c| c.id == id))
        .is_some_and(|c| c.role == ChannelRole::Submix);
    if is_group {
        ungroup_track(session, params)?;
        return Ok(json!({ "deleted": track.0 }));
    }
    let channel = channel_id_of(&session.project, track);
    let p = &mut session.project;
    match channel {
        Some(channel) => remove_track(p, track, channel),
        None => {
            p.tracks.retain(|t| t.id != track);
            p.arrangement.lanes.retain(|l| l.track != Some(track));
            p.launcher.columns.retain(|c| c.track != track);
        }
    }
    Ok(json!({ "deleted": track.0 }))
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

/// The clip `params` names: an arrangement clip by `index`, or the clip in a launcher slot by
/// `scene`.
fn clip_at(p: &Project, params: &Value) -> Result<ClipAt, CommandError> {
    match (params.get("index"), params.get("scene")) {
        (Some(_), Some(_)) => Err("give index or scene, not both".into()),
        (None, Some(_)) => {
            let (column, scene) = (column_index(p, params)?, scene_index(p, params)?);
            match p.launcher.columns.get(column).and_then(|c| c.slots.get(scene)) {
                Some(slot) if slot.clip.is_some() => Ok(ClipAt::Slot { column, scene }),
                _ => Err(format!("the slot of scene {} is empty", scene + 1).into()),
            }
        }
        (Some(_), None) => lane_and_index(p, params).map(|(lane, index)| ClipAt::Arrangement { lane, index }),
        (None, None) => Err("give index (an arrangement clip) or scene (a launcher clip)".into()),
    }
}

fn set_clip_gain(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let db = number_or_neg_infinity(params, "db")?;
    if db > 24.0 {
        return Err(format!("db must be at most +24, got {db}").into());
    }
    let gain = if db < -150.0 { 0.0 } else { 10f64.powf(db / 20.0) };
    let (lane, index) = lane_and_index(&session.project, params)?;
    let content = session
        .project
        .arrangement
        .lanes
        .get(lane)
        .and_then(|l| l.clips.get(index))
        .map(|c| c.content)
        .ok_or("the clip is missing")?;
    let content =
        session.project.contents.iter_mut().find(|c| c.id == content).ok_or("the clip's content is missing")?;
    let mut audio = false;
    for lane in &mut content.lanes {
        if let LaneData::Audio(region) = &mut lane.data {
            region.gain = gain;
            audio = true;
        }
    }
    if !audio {
        return Err("only an audio clip has a gain".into());
    }
    Ok(json!({ "gain_db": to_db(gain) }))
}

fn set_clip_fades(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let length_of = |name: &str| -> Result<Option<f64>, CommandError> {
        match params.get(name) {
            None => Ok(None),
            Some(_) => {
                let beats = number(params, name)?;
                if beats < 0.0 {
                    return Err(format!("{name} must be at least 0 beats, got {beats}").into());
                }
                Ok(Some(beats))
            }
        }
    };
    let shape_of = |name: &str| -> Result<Option<FadeShape>, CommandError> {
        match params.get(name) {
            None => Ok(None),
            Some(Value::String(s)) => FadeShape::from_name(s).map(Some).ok_or_else(|| {
                let names: Vec<&str> = FadeShape::ALL.iter().map(|s| s.name()).collect();
                format!("{name} must be one of {}, got {s:?}", names.join(", ")).into()
            }),
            Some(_) => Err(format!("{name} must be a shape name").into()),
        }
    };
    let curve_of = |name: &str| -> Result<Option<f64>, CommandError> {
        match params.get(name) {
            None => Ok(None),
            Some(_) => {
                let curve = number(params, name)?;
                if !(-1.0..=1.0).contains(&curve) {
                    return Err(format!("{name} must be from -1 to +1, got {curve}").into());
                }
                Ok(Some(curve))
            }
        }
    };
    let (fade_in, fade_out) = (length_of("in")?, length_of("out")?);
    let (in_shape, out_shape) = (shape_of("inShape")?, shape_of("outShape")?);
    let (in_curve, out_curve) = (curve_of("inCurve")?, curve_of("outCurve")?);
    if fade_in.is_none()
        && fade_out.is_none()
        && in_shape.is_none()
        && out_shape.is_none()
        && in_curve.is_none()
        && out_curve.is_none()
    {
        return Err("give in, out (beats), inShape, outShape, inCurve or outCurve".into());
    }
    let at = clip_at(&session.project, params)?;
    let p = &mut session.project;
    let clip = at.get(p).ok_or("the clip is missing")?;
    let audio = p.content(clip.content).is_some_and(|c| c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))));
    if !audio {
        return Err("only an audio clip has fades".into());
    }
    let changed = Clip {
        fade_in: fade_in.unwrap_or(clip.fade_in),
        fade_out: fade_out.unwrap_or(clip.fade_out),
        fade_in_curve: FadeCurve {
            shape: in_shape.unwrap_or(clip.fade_in_curve.shape),
            curve: in_curve.unwrap_or(clip.fade_in_curve.curve),
        },
        fade_out_curve: FadeCurve {
            shape: out_shape.unwrap_or(clip.fade_out_curve.shape),
            curve: out_curve.unwrap_or(clip.fade_out_curve.curve),
        },
        ..clip.clone()
    };
    if !changed.fades_valid() {
        if !(changed.fade_in_curve.is_valid() && changed.fade_out_curve.is_valid()) {
            return Err("the clip's fade curves are outside -1 to +1; set inCurve and outCurve".into());
        }
        return Err(format!(
            "the fades ({} + {} beats) are longer than the clip ({} beats)",
            changed.fade_in, changed.fade_out, changed.length
        )
        .into());
    }
    let result = json!({
        "fade_in": changed.fade_in,
        "fade_out": changed.fade_out,
        "fade_in_shape": changed.fade_in_curve.shape.name(),
        "fade_in_curve": changed.fade_in_curve.curve,
        "fade_out_shape": changed.fade_out_curve.shape.name(),
        "fade_out_curve": changed.fade_out_curve.curve,
    });
    if let Some(target) = at.get_mut(p) {
        *target = changed;
    }
    Ok(result)
}

fn copy_clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &session.project;
    let mut copied: Vec<(Clip, ClipContent)> = Vec::new();
    let mut from: Vec<TrackId> = Vec::new();
    match (params.get("clips"), params.get("scene")) {
        (Some(_), None) => {
            if params.get("track").is_some() {
                return Err("track goes with scene (a clip slot); clips names arrangement clips".into());
            }
            for (track, index) in clip_list(p, params)? {
                let clip = p
                    .arrangement
                    .lanes
                    .iter()
                    .find(|l| l.track == Some(track))
                    .and_then(|l| l.clips.get(index))
                    .cloned()
                    .ok_or("the clip is missing")?;
                let content = p.content(clip.content).cloned().ok_or("the clip's content is missing")?;
                copied.push((clip, content));
                from.push(track);
            }
        }
        (None, Some(_)) => {
            let (column, scene) = (column_index(p, params)?, scene_index(p, params)?);
            let slot = p.launcher.columns.get(column).and_then(|c| c.slots.get(scene));
            let clip =
                slot.and_then(|s| s.clip.clone()).ok_or_else(|| format!("the slot in scene {} is empty", scene + 1))?;
            let content = p.content(clip.content).cloned().ok_or("the clip's content is missing")?;
            copied.push((clip, content));
        }
        _ => return Err("give either clips (arrangement clips) or track and scene (a clip slot)".into()),
    }
    let names: Vec<String> = copied.iter().map(|(_, content)| content.name.clone()).collect();
    session.clipboard = copied;
    session.clipboard_tracks = from;
    Ok(json!({ "copied": names }))
}

/// An id no clip content of `p` has yet.
pub(crate) fn new_content_id(p: &Project) -> Result<suhina_model::ContentId, CommandError> {
    let id = p.contents.iter().map(|c| c.id.0).max().unwrap_or(0).checked_add(1).ok_or("too many clips")?;
    Ok(suhina_model::ContentId(id))
}

fn cut_clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let copied = copy_clips(session, params)?;
    delete_clips(session, params)?;
    Ok(copied)
}

/// Where `clip.paste` puts the copy: an arrangement beat, or a clip slot (scene from 0).
#[derive(Clone, Copy)]
enum PasteAt {
    Beat(f64),
    Slot(usize),
}

fn paste_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    if session.clipboard.is_empty() {
        return Err("nothing has been copied".into());
    }
    let at = match (params.get("start"), params.get("scene")) {
        (Some(_), None) => {
            let start = number(params, "start")?;
            if !(0.0..=MAX_START_BEAT).contains(&start) {
                return Err(format!("start must be from 0 to {MAX_START_BEAT}, got {start}").into());
            }
            PasteAt::Beat(start)
        }
        (None, Some(_)) => {
            if params.get("stack").is_some() {
                return Err("stack is for the arrangement (start); a slot holds one clip".into());
            }
            PasteAt::Slot(scene_index(&session.project, params)?)
        }
        _ => return Err("give either start (a beat in the arrangement) or scene (a clip slot)".into()),
    };
    let stack = match params.get("stack") {
        None => false,
        Some(v) => v.as_bool().ok_or("stack must be true or false")?,
    };
    // No track: each copied clip goes back to the track it came from.
    if params.get("track").is_none()
        && let PasteAt::Beat(start) = at
    {
        return crate::range::paste_to_sources(session, start, stack);
    }
    let track = track_id(&session.project, params)?;
    {
        let p = &session.project;
        let target = p.tracks.iter().find(|t| t.id == track).ok_or("the track is missing")?;
        for (_, content) in &session.clipboard {
            let notes = content.lanes.iter().any(|l| matches!(l.data, LaneData::Notes(_)));
            let audio = content.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_)));
            if (notes && !target.content.notes) || (audio && !target.content.audio) {
                let what = if audio { "an audio clip" } else { "a MIDI clip" };
                return Err(format!("track {:?} cannot hold {what}", target.name).into());
            }
        }
    }
    let column = match at {
        PasteAt::Slot(_) => Some(column_index(&session.project, params)?),
        PasteAt::Beat(_) => None,
    };
    if let (PasteAt::Slot(scene), Some(column)) = (at, column) {
        let [(clip, content)] = session
            .clipboard
            .clone()
            .try_into()
            .map_err(|copied: Vec<_>| format!("a clip slot holds one clip, and {} are copied", copied.len()))?;
        let p = &mut session.project;
        let id = new_content_id(p)?;
        p.contents.push(suhina_model::ClipContent { id, ..content });
        // Gain, warp and the loop travel with the content and the clip; fades are fitted, as when
        // a clip is shortened, so a clip with odd fades still makes a valid project.
        // The pasted clip is a new clip, with an id of its own.
        let mut placed = Clip { id: p.new_clip_id()?, content: id, start: 0.0, ..clip };
        placed.fit_fades();
        let placed_id = placed.id;
        let slot =
            p.launcher.columns.get_mut(column).and_then(|c| c.slots.get_mut(scene)).ok_or("the slot is missing")?;
        let replaced = slot.clip.is_some();
        slot.clip = Some(placed);
        return Ok(json!({ "clip": placed_id.0, "track": track.0, "scene": scene + 1, "replaced": replaced }));
    }
    let start = match at {
        PasteAt::Beat(start) => start,
        PasteAt::Slot(_) => 0.0,
    };
    // The earliest copied clip lands on `start`; the others keep their places relative to it.
    let earliest = session.clipboard.iter().map(|(clip, _)| clip.start).fold(f64::INFINITY, f64::min);
    let offset = if earliest.is_finite() { start - earliest } else { 0.0 };
    let copied = session.clipboard.clone();
    let p = &mut session.project;
    let lane = match p.arrangement.lanes.iter().position(|l| l.track == Some(track)) {
        Some(lane) => lane,
        None => {
            let name = p.tracks.iter().find(|t| t.id == track).map(|t| t.name.clone()).unwrap_or_default();
            let id = p.new_lane_id()?;
            p.arrangement.lanes.push(ArrangementLane { id, name, track: Some(track), clips: Vec::new() });
            p.arrangement.lanes.len() - 1
        }
    };
    let mut placed: Vec<Value> = Vec::with_capacity(copied.len());
    for (clip, content) in copied {
        // The copy plays its own content, so editing one does not change the other.
        let id = new_content_id(p)?;
        p.contents.push(suhina_model::ClipContent { id, ..content });
        let at = clip.start + offset;
        let mut copy = Clip { id: p.new_clip_id()?, content: id, start: at, ..clip };
        copy.fit_fades();
        let lane = p.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?;
        if !stack {
            make_room(&mut lane.clips, at, at + copy.length);
        }
        let copy_id = copy.id;
        lane.clips.push(copy);
        lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        let index = lane.clips.iter().position(|c| c.id == copy_id).unwrap_or_default();
        placed.push(json!({ "clip": copy_id.0, "track": track.0, "index": index, "start": at }));
    }
    Ok(json!({ "clips": placed }))
}

fn delete_clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    // A list of arrangement clips goes as one; otherwise a launcher slot's clip is named by
    // `scene`, an arrangement clip by `index`, the way the note commands name the clip they edit.
    if params.get("clips").is_some_and(|v| !v.is_null()) {
        if ["track", "index", "scene"].iter().any(|k| params.get(*k).is_some_and(|v| !v.is_null())) {
            return Err("give clips, or track with index or scene, not both".into());
        }
        let clips = clip_list(&session.project, params)?;
        let deleted = remove_clips(&mut session.project, &clips);
        return Ok(json!({ "deleted": deleted }));
    }
    let whole = |key: &str| -> Result<Option<u64>, CommandError> {
        match params.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_u64().map(Some).ok_or_else(|| format!("{key} must be a whole number").into()),
        }
    };
    match (whole("index")?, whole("scene")?) {
        (Some(_), Some(_)) => Err("give index or scene, not both".into()),
        (None, None) => Err("give clips, or track with index (an arrangement clip) or scene (a launcher clip)".into()),
        (Some(_), None) => {
            let (lane, index) = lane_and_index(&session.project, params)?;
            if let Some(lane) = session.project.arrangement.lanes.get_mut(lane) {
                lane.clips.remove(index);
            }
            Ok(json!({ "deleted": index }))
        }
        (None, Some(scene)) => {
            let column = column_index(session.project(), params)?;
            let s = scene_index(session.project(), params)?;
            let slot = session
                .project
                .launcher
                .columns
                .get_mut(column)
                .and_then(|c| c.slots.get_mut(s))
                .ok_or("the slot is missing")?;
            if slot.clip.is_none() {
                return Err(format!("the slot of scene {scene} is empty").into());
            }
            slot.clip = None;
            Ok(json!({ "deleted": scene }))
        }
    }
}

/// `clip.rename`: the clip's name is its content's, so clips that play the same content share
/// the new name (a pasted copy has a content of its own and keeps its name). An empty name
/// clears it, and the launcher's slots then show "clip".
fn rename_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let name = match params.get("name") {
        Some(Value::String(name)) => name.trim().to_string(),
        _ => return Err("name must be a text (empty clears it)".into()),
    };
    let (_, content, _) = crate::notes::locate(&session.project, params)?;
    let content =
        session.project.contents.iter_mut().find(|c| c.id == content).ok_or("the clip's content is missing")?;
    content.name.clone_from(&name);
    Ok(json!({ "name": name }))
}

/// `clip.shiftFades {clips, edge, beats}` (#715).
fn shift_fades(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let fade_in = match params.get("edge").and_then(Value::as_str) {
        Some("in") => true,
        Some("out") => false,
        _ => return Err("edge must be in or out".into()),
    };
    let beats = number(params, "beats")?;
    let p = &mut session.project;
    let chosen = clip_list(p, params)?;
    let mut changed = 0;
    for (track, index) in chosen {
        let clip = p
            .arrangement
            .lanes
            .iter_mut()
            .find(|l| l.track == Some(track))
            .and_then(|l| l.clips.get_mut(index))
            .ok_or("the clip is missing")?;
        let (own, other) = if fade_in { (clip.fade_in, clip.fade_out) } else { (clip.fade_out, clip.fade_in) };
        let most = (clip.length - other).max(0.0);
        let now = (own + beats).clamp(0.0, most);
        if fade_in {
            clip.fade_in = now;
        } else {
            clip.fade_out = now;
        }
        changed += 1;
    }
    Ok(json!({ "clips": changed }))
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

/// Removes the clips named by `clips`, which must exist; returns how many went. Indices are read
/// against the project as it was, so a lane loses all its clips at once.
fn remove_clips(p: &mut Project, clips: &[(TrackId, usize)]) -> usize {
    let mut deleted = 0;
    for (track, indexes) in clips_by_track(clips) {
        let Some(lane) = p.arrangement.lanes.iter_mut().find(|l| l.track == Some(track)) else { continue };
        let before = lane.clips.len();
        let mut at = 0usize;
        lane.clips.retain(|_| {
            let keep = !indexes.contains(&at);
            at += 1;
            keep
        });
        deleted += before - lane.clips.len();
    }
    deleted
}

/// The clip indexes of `clips`, gathered by the track they are on.
fn clips_by_track(clips: &[(TrackId, usize)]) -> Vec<(TrackId, Vec<usize>)> {
    let mut out: Vec<(TrackId, Vec<usize>)> = Vec::new();
    for (track, index) in clips {
        match out.iter_mut().find(|(t, _)| t == track) {
            Some((_, list)) => list.push(*index),
            None => out.push((*track, vec![*index])),
        }
    }
    out
}

fn duplicate_clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let stack = match params.get("stack") {
        None => false,
        Some(v) => v.as_bool().ok_or("stack must be true or false")?,
    };
    let clips = clip_list(&session.project, params)?;
    let p = &mut session.project;
    let mut made: Vec<Value> = Vec::new();
    for (track, indexes) in clips_by_track(&clips) {
        // The copies, worked out before the lane changes under them.
        let copies: Vec<Clip> = match p.arrangement.lanes.iter().find(|l| l.track == Some(track)) {
            Some(lane) => indexes
                .iter()
                .filter_map(|i| lane.clips.get(*i))
                .map(|c| Clip { start: c.start + c.length, ..c.clone() })
                .collect(),
            None => continue,
        };
        // Each copy is a new clip with an id of its own.
        let mut copies = copies;
        for copy in &mut copies {
            copy.id = p.new_clip_id()?;
        }
        let Some(lane) = p.arrangement.lanes.iter_mut().find(|l| l.track == Some(track)) else { continue };
        for copy in copies {
            if !stack {
                make_room(&mut lane.clips, copy.start, copy.start + copy.length);
            }
            let (copy_id, copy_start) = (copy.id, copy.start);
            lane.clips.push(copy);
            lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
            let index = lane.clips.iter().position(|c| c.id == copy_id).unwrap_or_default();
            made.push(json!({ "clip": copy_id.0, "track": track.0, "index": index, "start": copy_start }));
        }
    }
    Ok(json!({ "clips": made }))
}

fn set_clip_enabled(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let enabled = params.get("enabled").and_then(Value::as_bool).ok_or("enabled must be true or false")?;
    let clips = clip_list(&session.project, params)?;
    let p = &mut session.project;
    let mut changed = 0;
    for (track, indexes) in clips_by_track(&clips) {
        let Some(lane) = p.arrangement.lanes.iter_mut().find(|l| l.track == Some(track)) else { continue };
        for index in indexes {
            if let Some(clip) = lane.clips.get_mut(index) {
                clip.enabled = enabled;
                changed += 1;
            }
        }
    }
    Ok(json!({ "enabled": enabled, "clips": changed }))
}

fn set_output(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let from = channel_param(session, params)?.id;
    let p = &session.project;
    let to = match params.get("to") {
        Some(Value::String(main)) if main.eq_ignore_ascii_case("main") => None,
        Some(_) => {
            let track = track_at(p, params, "to")?;
            let target = p
                .tracks
                .iter()
                .find(|t| t.id == track)
                .and_then(|t| t.channel)
                .and_then(|id| p.channels.iter().find(|c| c.id == id))
                .ok_or("the target track has no mixer channel")?;
            if target.role != ChannelRole::Submix {
                return Err(format!("{:?} is not a group track; route to \"main\" or a group", target.name).into());
            }
            // The group must not play, directly or through other groups, into the track itself.
            let mut next = Some(target.id);
            for _ in 0..=p.channels.len() {
                match next {
                    Some(id) if id == from => {
                        return Err(format!("routing into {:?} would make a loop", target.name).into());
                    }
                    Some(id) => next = p.channels.iter().find(|c| c.id == id).and_then(|c| c.output),
                    None => break,
                }
            }
            Some(target.id)
        }
        None => return Err("to must be \"main\" or a group track".into()),
    };
    let channel = channel_param(session, params)?;
    channel.output = to;
    let name = to
        .and_then(|id| session.project.tracks.iter().find(|t| t.channel == Some(id)))
        .map_or_else(|| "main".to_string(), |t| t.name.clone());
    Ok(json!({ "to": name }))
}

fn set_solo(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let solo = params.get("solo").and_then(Value::as_bool).ok_or("solo must be true or false")?;
    let exclusive = match params.get("exclusive") {
        None => false,
        Some(v) => v.as_bool().ok_or("exclusive must be true or false")?,
    };
    let channels = channel_ids(&session.project, params)?;
    let ids: Vec<ChannelId> = channels.iter().map(|(_, id)| *id).collect();
    for channel in &mut session.project.channels {
        if ids.contains(&channel.id) {
            channel.solo = solo;
        } else if exclusive {
            channel.solo = false;
        }
    }
    Ok(json!({ "tracks": track_ids(&channels), "solo": solo }))
}

fn set_cue(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let cue = params.get("cue").and_then(Value::as_bool).ok_or("cue must be true or false")?;
    channel_param(session, params)?.cue = cue;
    Ok(json!({ "cue": cue }))
}

fn set_midi_input(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let id = track_id(&session.project, params)?;
    let port = match params.get("port") {
        None => None,
        Some(Value::String(s)) => Some(match s.trim() {
            "" => return Err("port must be \"all\", \"none\" or a port's name".into()),
            "all" => MidiPort::All,
            "none" => MidiPort::Nothing,
            name => MidiPort::Named(name.to_string()),
        }),
        Some(_) => return Err("port must be a text".into()),
    };
    let channel = match params.get("channel") {
        None => None,
        Some(v) => match v.as_u64() {
            Some(0) => Some(None),
            Some(c @ 1..=16) => Some(u8::try_from(c).ok()),
            _ => return Err("channel must be 0 (every channel) or 1 to 16".into()),
        },
    };
    let track = session.project.tracks.iter_mut().find(|t| t.id == id).ok_or("the track is missing")?;
    if let Some(port) = port {
        track.midi_input.port = port;
    }
    if let Some(channel) = channel {
        track.midi_input.channel = channel;
    }
    Ok(json!({ "midi_input": midi_input_json(&track.midi_input) }))
}

fn set_audio_input(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    use suhina_model::{InputChannels, MAX_INPUT_CHANNEL, Monitor};
    let id = track_id(&session.project, params)?;
    let bad = || {
        CommandError(format!("input must be \"none\", a channel from 1 to {MAX_INPUT_CHANNEL} or a pair like \"1/2\""))
    };
    let channel = |text: &str| text.trim().parse::<u16>().ok().filter(|c| (1..=MAX_INPUT_CHANNEL).contains(c));
    let channels = match params.get("input") {
        None => None,
        Some(Value::String(text)) => Some(match text.trim() {
            "none" => InputChannels::None,
            pair if pair.contains('/') => {
                let (left, right) = pair.split_once('/').ok_or_else(bad)?;
                let (left, right) = (channel(left).ok_or_else(bad)?, channel(right).ok_or_else(bad)?);
                if left.checked_add(1) != Some(right) {
                    return Err("a stereo pair is two neighbouring channels, like \"1/2\" or \"3/4\"".into());
                }
                InputChannels::Stereo(left)
            }
            one => InputChannels::Mono(channel(one).ok_or_else(bad)?),
        }),
        Some(_) => return Err(bad()),
    };
    let monitor = match params.get("monitor") {
        None => None,
        Some(v) => Some(match v.as_str() {
            Some("auto") => Monitor::Auto,
            Some("in") => Monitor::In,
            Some("off") => Monitor::Off,
            _ => return Err("monitor must be auto, in or off".into()),
        }),
    };
    let track = session.project.tracks.iter_mut().find(|t| t.id == id).ok_or("the track is missing")?;
    if let Some(channels) = channels {
        track.audio_input.channels = channels;
    }
    if let Some(monitor) = monitor {
        track.audio_input.monitor = monitor;
    }
    Ok(json!({ "audio_input": audio_input_json(&track.audio_input) }))
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
    json!({ "port": port, "channel": input.channel })
}

fn set_arm(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let armed = params.get("armed").and_then(Value::as_bool).ok_or("armed must be true or false")?;
    let exclusive = match params.get("exclusive") {
        None => false,
        Some(v) => v.as_bool().ok_or("exclusive must be true or false")?,
    };
    let track = track_id(&session.project, params)?;
    session.set_armed(track, armed, exclusive);
    let tracks: Vec<u32> = session.armed().iter().map(|t| t.0).collect();
    Ok(json!({ "armed": armed, "armed_tracks": tracks, "playing": session.player().is_some() }))
}

fn set_metronome(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let on = params.get("on").and_then(Value::as_bool).ok_or("on must be true or false")?;
    // Without a level the metronome keeps the one it has (-6 dB to begin with).
    let gain = match params.get("db") {
        None => None,
        Some(_) => {
            let db = number_or_neg_infinity(params, "db")?;
            if db > 6.0 {
                return Err(format!("db must be at most +6, got {db}").into());
            }
            Some(if db < -150.0 { 0.0 } else { 10f64.powf(db / 20.0) })
        }
    };
    let sound = match params.get("sound") {
        None | Some(Value::Null) => None,
        Some(Value::String(name)) => Some(
            suhina_engine::ClickSound::from_name(name)
                .ok_or_else(|| CommandError(format!("sound must be beep, wood or soft, got {name}")))?,
        ),
        Some(other) => return Err(format!("sound must be beep, wood or soft, got {other}").into()),
    };
    send(session, LaunchCommand::SetMetronome { on, gain: gain.map(|g| g as f32) })?;
    if let Some(sound) = sound {
        send(session, LaunchCommand::SetMetronomeSound(sound))?;
    }
    session.metronome.0 = on;
    if let Some(gain) = gain {
        session.metronome.1 = to_db(gain).unwrap_or(f64::NEG_INFINITY);
    }
    if let Some(sound) = sound {
        session.metronome.2 = sound;
    }
    let (_, db, sound) = session.metronome;
    Ok(json!({ "metronome": on, "db": if db.is_finite() { json!(db) } else { json!("-inf") }, "sound": sound.name() }))
}

fn move_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_id(&session.project, params)?;
    let index = params.get("index").and_then(Value::as_u64).ok_or("index must be a whole number from project.clips")?;
    let start = number(params, "start")?;
    if start < 0.0 {
        return Err(format!("start must be at least 0, got {start}").into());
    }
    let stack = match params.get("stack") {
        None => false,
        Some(v) => v.as_bool().ok_or("stack must be true or false")?,
    };
    let to = match params.get("to_track") {
        Some(_) => track_at(&session.project, params, "to_track")?,
        None => track,
    };
    // The colour the clip shows on its track, kept when it goes to another (`color`).
    let keep = match params.get("color") {
        None | Some(Value::Null) => None,
        Some(_) => crate::clip_color::parse_color(params, "color")?,
    };
    let p = &mut session.project;
    let from_lane =
        p.arrangement.lanes.iter().position(|l| l.track == Some(track)).ok_or("the track has no arrangement lane")?;
    let count = p.arrangement.lanes.get(from_lane).map_or(0, |l| l.clips.len());
    let index = usize::try_from(index)
        .ok()
        .filter(|i| *i < count)
        .ok_or_else(|| format!("clip index {index} does not exist; the track has {count} arrangement clips"))?;
    if to != track {
        let clip = p.arrangement.lanes.get(from_lane).and_then(|l| l.clips.get(index)).ok_or("the clip is missing")?;
        let kinds = p.content(clip.content).map_or((false, false), |c| {
            (
                c.lanes.iter().any(|l| matches!(l.data, LaneData::Notes(_))),
                c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))),
            )
        });
        let target = p.tracks.iter().find(|t| t.id == to).ok_or("the target track is missing")?;
        let fits = (!kinds.0 || target.content.notes) && (!kinds.1 || target.content.audio) && (kinds.0 || kinds.1);
        if !fits {
            let what = if kinds.1 { "an audio clip" } else { "a MIDI clip" };
            return Err(format!("track {:?} cannot hold {what}", target.name).into());
        }
    }
    let to_lane = match p.arrangement.lanes.iter().position(|l| l.track == Some(to)) {
        Some(lane) => lane,
        None => {
            let name = p.tracks.iter().find(|t| t.id == to).map(|t| t.name.clone()).unwrap_or_default();
            let id = p.new_lane_id()?;
            p.arrangement.lanes.push(ArrangementLane { id, name, track: Some(to), clips: Vec::new() });
            p.arrangement.lanes.len() - 1
        }
    };
    let mut clip = p.arrangement.lanes.get_mut(from_lane).ok_or("the lane is missing")?.clips.remove(index);
    clip.start = start;
    if to != track && clip.color.is_none() {
        clip.color = keep;
    }
    let lane = p.arrangement.lanes.get_mut(to_lane).ok_or("the lane is missing")?;
    if !stack {
        make_room(&mut lane.clips, start, start + clip.length);
    }
    // A moved clip is the same clip: it keeps its id, on whichever track it lands.
    let clip_id = clip.id;
    lane.clips.push(clip);
    lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    let new_index = lane.clips.iter().position(|c| c.id == clip_id).unwrap_or_default();
    Ok(json!({ "clip": clip_id.0, "start": start, "track": to.0, "index": new_index }))
}

/// `clip.moveSlot`: the dragged slot's clip goes to the target's slot; an occupied target's
/// clip comes back to the source's slot (the two swap, nothing lost). The target must take the
/// clip's kind: notes or audio.
fn move_slot(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let from_column = column_index(session.project(), params)?;
    let from_scene = scene_index(session.project(), params)?;
    let to_track = match params.get("to_track") {
        None | Some(Value::Null) => None,
        Some(_) => Some(track_at(&session.project, params, "to_track")?),
    };
    let to_scene: Option<usize> = match params.get("to_scene") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .and_then(|n| n.checked_sub(1))
                .ok_or("to_scene must be a whole number from 1")?,
        ),
    };

    let project = &mut session.project;
    let from_track = project.launcher.columns.get(from_column).map(|c| c.track).ok_or("the track has no clip slots")?;
    let to_track = to_track.unwrap_or(from_track);
    let to_scene = to_scene.unwrap_or(from_scene);
    if (to_track, to_scene) == (from_track, from_scene) {
        return Ok(json!({ "moved": false }));
    }
    // The clip's kind must be one the target's track plays: notes to a notes track, audio to
    // an audio track, as `clip.move` checks for the arrangement.
    let clip = project
        .launcher
        .columns
        .get(from_column)
        .and_then(|c| c.slots.get(from_scene))
        .and_then(|s| s.clip.as_ref())
        .ok_or("the slot has no clip to move")?;
    let kinds = project.content(clip.content).map_or((false, false), |c| {
        (
            c.lanes.iter().any(|l| matches!(l.data, LaneData::Notes(_))),
            c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))),
        )
    });
    let target = project.tracks.iter().find(|t| t.id == to_track).ok_or("the target track is missing")?;
    let source_track = project.tracks.iter().find(|t| t.id == from_track).ok_or("the source track is missing")?;
    let fits = (!kinds.0 || target.content.notes) && (!kinds.1 || target.content.audio) && (kinds.0 || kinds.1);
    if !fits {
        let what = if kinds.1 { "an audio clip" } else { "a MIDI clip" };
        return Err(format!("track {:?} cannot hold {what}", target.name).into());
    }
    let Some(to_column) = project.launcher.columns.iter().position(|c| c.track == to_track) else {
        return Err(format!("track {:?} has no clip slots", target.name).into());
    };
    // The swap's other direction: a clip coming back to the source's slot must be one the
    // source's track plays, checked before anything moves, so a refusal leaves both as they are.
    let displaced =
        project.launcher.columns.get(to_column).and_then(|c| c.slots.get(to_scene)).and_then(|s| s.clip.as_ref());
    if let Some(displaced) = displaced {
        let kinds = project.content(displaced.content).map_or((false, false), |c| {
            (
                c.lanes.iter().any(|l| matches!(l.data, LaneData::Notes(_))),
                c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))),
            )
        });
        let fits = (!kinds.0 || source_track.content.notes)
            && (!kinds.1 || source_track.content.audio)
            && (kinds.0 || kinds.1);
        if !fits {
            let what = if kinds.1 { "an audio clip" } else { "a MIDI clip" };
            return Err(format!("track {:?} cannot hold {what}", source_track.name).into());
        }
    }
    let from_clip = project
        .launcher
        .columns
        .get_mut(from_column)
        .and_then(|c| c.slots.get_mut(from_scene))
        .ok_or("the slot is missing")?
        .clip
        .take();
    let from_clip = match from_clip {
        Some(clip) => clip,
        None => return Err("the slot has no clip to move".into()),
    };
    let back = project
        .launcher
        .columns
        .get_mut(to_column)
        .and_then(|c| c.slots.get_mut(to_scene))
        .ok_or("the slot is missing")?
        .clip
        .replace(from_clip);
    let swapped = back.is_some();
    project
        .launcher
        .columns
        .get_mut(from_column)
        .and_then(|c| c.slots.get_mut(from_scene))
        .ok_or("the slot is missing")?
        .clip = back;
    Ok(json!({
        "moved": true,
        "track": to_track.0,
        "scene": to_scene + 1,
        "swapped": swapped,
    }))
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

/// `params.beat`, checked to be a beat a marker can be at.
fn marker_beat(params: &Value) -> Result<f64, CommandError> {
    let beat = number(params, "beat")?;
    if !(0.0..=MAX_START_BEAT).contains(&beat) {
        return Err(format!("beat must be between 0 and {MAX_START_BEAT}, got {beat}").into());
    }
    Ok(beat)
}

/// `params.index`, checked to name a marker.
fn marker_index(p: &Project, params: &Value) -> Result<usize, CommandError> {
    let index =
        params.get("index").and_then(Value::as_u64).ok_or("index must be a whole number from project.markers")?;
    let count = p.arrangement.markers.len();
    usize::try_from(index)
        .ok()
        .filter(|i| *i < count)
        .ok_or_else(|| format!("marker {index} does not exist; the arrangement has {count} markers").into())
}

fn add_marker(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let beat = marker_beat(params)?;
    let markers = &mut session.project.arrangement;
    let name = match params.get("name") {
        None | Some(Value::Null) => {
            // The first "Marker n" not in use, counting from how many there are.
            let mut n = markers.markers.len() + 1;
            while markers.markers.iter().any(|m| m.name == format!("Marker {n}")) {
                n += 1;
            }
            format!("Marker {n}")
        }
        Some(v) => {
            v.as_str().map(str::trim).filter(|n| !n.is_empty()).ok_or("name must be a text, not empty")?.to_string()
        }
    };
    let index = markers.insert_marker(Marker::new(beat, name.clone()));
    Ok(json!({ "index": index, "beat": beat, "name": name }))
}

fn move_marker(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let index = marker_index(&session.project, params)?;
    let beat = marker_beat(params)?;
    let arrangement = &mut session.project.arrangement;
    if index >= arrangement.markers.len() {
        return Err("the marker does not exist".into());
    }
    let mut marker = arrangement.markers.remove(index);
    marker.beat = beat;
    let name = marker.name.clone();
    let index = arrangement.insert_marker(marker);
    Ok(json!({ "index": index, "beat": beat, "name": name }))
}

fn rename_marker(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let name = params.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty());
    let name = name.ok_or("name must be a text, not empty")?.to_string();
    let index = marker_index(&session.project, params)?;
    let marker = session.project.arrangement.markers.get_mut(index).ok_or("the marker does not exist")?;
    marker.name.clone_from(&name);
    Ok(json!({ "index": index, "name": name }))
}

fn delete_marker(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let index = marker_index(&session.project, params)?;
    let markers = &mut session.project.arrangement.markers;
    if index >= markers.len() {
        return Err("the marker does not exist".into());
    }
    let marker = markers.remove(index);
    Ok(json!({ "deleted": index, "name": marker.name }))
}

fn jump_to_marker(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    // Without an engine there is nothing to jump; say so before looking at the marker.
    session.player_mut()?;
    let index = marker_index(session.project(), params)?;
    let beat = session.project().arrangement.markers.get(index).map_or(0.0, |m| m.beat).clamp(0.0, MAX_START_BEAT);
    send(session, LaunchCommand::SetPosition { beat })?;
    Ok(json!({ "index": index, "start_beat": beat }))
}

fn launcher_grid(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let p = session.project();
    let scenes: Vec<Value> = p
        .launcher
        .scenes
        .iter()
        .zip(1..)
        .map(|(scene, number): (_, u32)| {
            json!({
                "scene": number,
                "id": scene.id.0,
                "name": scene.name,
                "tempo_bpm": scene.tempo_bpm,
                "time_signature": scene.time_signature.map(signature_text),
            })
        })
        .collect();
    let tracks: Vec<Value> = p
        .launcher
        .columns
        .iter()
        .map(|column| {
            let slots: Vec<Value> = column
                .slots
                .iter()
                .zip(1..)
                .map(|(slot, number): (_, u32)| match &slot.clip {
                    Some(clip) => json!({
                        "scene": number,
                        "id": clip.id.0,
                        "clip": p.content(clip.content).map(|c| c.name.as_str()),
                        "length": clip.length,
                        "loop": clip.loop_range.is_some(),
                        "enabled": clip.enabled,
                        "follow_action": follow::describe(&clip.follow_action),
                        "launch_mode": clip.launch.mode.name(),
                        "legato": clip.launch.legato,
                        "quantization": clip.launch.quantization.name(),
                    }),
                    None => json!({ "scene": number, "clip": null, "stop_button": slot.has_stop }),
                })
                .collect();
            json!({ "track": column.track.0, "track_name": track_name(p, column.track), "slots": slots })
        })
        .collect();
    Ok(json!({ "scenes": scenes, "tracks": tracks }))
}

fn launcher_state(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let preview_state = session.player().map(|player| {
        let preview = player.preview_monitor();
        json!({ "playing": preview.playing(), "seconds": (preview.seconds() * 1000.0).round() / 1000.0 })
    });
    let cpu = session.player().map(|player| player.cpu_monitor().load_percent());
    let monitor = session.player_mut()?.monitor();
    let (running, beat, tempo) = (monitor.running(), monitor.beat(), monitor.tempo_bpm());
    let start_beat = monitor.start_beat();
    let monitor_metronome = monitor.metronome();
    let meter = monitor.meter();
    let statuses: Vec<Option<ColumnStatus>> = (0..monitor.columns()).map(|i| monitor.column(i)).collect();
    let p = session.project();
    let project_meter = p.transport.meter();
    let tracks: Vec<Value> = p
        .launcher
        .columns
        .iter()
        .zip(statuses)
        .map(|(column, status)| {
            let status = status.unwrap_or(ColumnStatus { playing: None, queued: None, overrides_arrangement: false });
            let clip_name = |slot: usize| {
                column
                    .slots
                    .get(slot)
                    .and_then(|s| s.clip.as_ref())
                    .and_then(|c| p.content(c.content))
                    .map(|c| c.name.clone())
            };
            json!({
                "track": column.track.0,
                "track_name": track_name(p, column.track),
                "playing": status.playing.map(|slot| json!({ "scene": slot + 1, "clip": clip_name(slot) })),
                "queued": match status.queued {
                    None => Value::Null,
                    Some(Queued::Stop) => json!("stop"),
                    Some(Queued::Launch(slot)) => json!({ "scene": slot + 1, "clip": clip_name(slot) }),
                },
                "plays_arrangement": !status.overrides_arrangement,
            })
        })
        .collect();
    // Rounded once, so the beat and the position agree.
    let beat = if beat.is_finite() { (beat.max(0.0) * 1000.0).round() / 1000.0 } else { 0.0 };
    // Bars through the time-signature changes while the clock follows the tempo map, or
    // through the time signature a scene set (`docs/spec/tempo-map.md`).
    let (bar, into_bar) = meter.locate(beat, &project_meter);
    let position = format!("{bar}.{}", into_bar.floor() as u64 + 1);
    let signature = if meter.follows_map { project_meter.position(beat).time_signature } else { meter.time_signature };
    Ok(json!({
        "running": running,
        "metronome": monitor_metronome,
        "tempo_bpm": tempo,
        "beat": beat,
        "time_signature": signature_text(signature),
        "position": position,
        "follows_tempo_map": meter.follows_map,
        "start_beat": if start_beat.is_finite() { (start_beat.max(0.0) * 1000.0).round() / 1000.0 } else { 0.0 },
        "tracks": tracks,
        "preview": preview_state,
        "cpu": cpu,
    }))
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

fn set_position(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let beats = number(params, "beats")?;
    if !(0.0..=MAX_START_BEAT).contains(&beats) {
        return Err(format!("beats must be between 0 and {MAX_START_BEAT}, got {beats}").into());
    }
    send(session, LaunchCommand::SetPosition { beat: beats })?;
    Ok(json!({ "start_beat": beats }))
}

fn launch_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let column = column_index(session.project(), params)?;
    let scene = scene_index(session.project(), params)?;
    // A slot that records ends its take instead (docs/spec/recording.md).
    if let Some(ended) = session.end_slot_take(column, scene) {
        return Ok(ended);
    }
    let slot = session.project().launcher.columns.get(column).and_then(|c| c.slots.get(scene));
    let queued = match slot {
        Some(slot) if slot.clip.as_ref().is_some_and(|c| c.enabled) => "launch",
        Some(slot) if slot.clip.is_some() => return Err("the clip in that slot is deactivated".into()),
        Some(slot) if slot.has_stop => "stop",
        _ => return Err("that slot is empty and has no stop button".into()),
    };
    send(session, LaunchCommand::LaunchClip { column, slot: scene })?;
    Ok(json!({ "scene": scene + 1, "queued": queued }))
}

fn launch_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(session.project(), params)?;
    send(session, LaunchCommand::LaunchScene { scene })?;
    Ok(json!({ "scene": scene + 1, "queued": "launch" }))
}

fn stop_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(session.project(), params)?;
    send(session, LaunchCommand::StopScene { scene })?;
    Ok(json!({ "scene": scene + 1, "queued": "stop" }))
}

/// Scenes a project can have; inserting stops there.
const MAX_SCENES: usize = 1000;

/// Gives every launcher column one slot per scene, so a row can be inserted or removed in all of
/// them at the same index.
fn even_slots(p: &mut Project) {
    let scenes = p.launcher.scenes.len();
    for column in &mut p.launcher.columns {
        column.slots.resize_with(scenes, || Slot { clip: None, has_stop: true });
    }
}

/// An optional scene name: trimmed text, empty when left out.
fn scene_name(params: &Value) -> Result<String, CommandError> {
    match params.get("name") {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(name)) => Ok(name.trim().to_string()),
        Some(_) => Err("name must be a text".into()),
    }
}

fn insert_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let count = p.launcher.scenes.len();
    if count >= MAX_SCENES {
        return Err(format!("the project has {count} scenes, as many as it can have").into());
    }
    let at = match params.get("scene") {
        None | Some(Value::Null) => count,
        Some(value) => value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .and_then(|n| n.checked_sub(1))
            .filter(|i| *i <= count)
            .ok_or_else(|| format!("scene must be a whole number from 1 to {}", count + 1))?,
    };
    let name = scene_name(params)?;
    even_slots(p);
    let id = p.new_scene_id()?;
    p.launcher.scenes.insert(at, Scene { id, name, ..Scene::default() });
    for column in &mut p.launcher.columns {
        column.slots.insert(at, Slot { clip: None, has_stop: true });
    }
    Ok(json!({ "scene": at + 1 }))
}

fn duplicate_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(&session.project, params)?;
    let p = &mut session.project;
    if p.launcher.scenes.len() >= MAX_SCENES {
        return Err(format!("the project has {} scenes, as many as it can have", p.launcher.scenes.len()).into());
    }
    even_slots(p);
    let mut copy = p.launcher.scenes.get(scene).cloned().unwrap_or_default();
    copy.id = p.new_scene_id()?;
    p.launcher.scenes.insert(scene + 1, copy);
    let mut next_content = p.contents.iter().map(|c| c.id.0).max().unwrap_or(0);
    for column in &mut p.launcher.columns {
        let mut slot = column.slots.get(scene).cloned().unwrap_or(Slot { clip: None, has_stop: true });
        // Each copied clip plays its own content, so editing one does not change the other.
        if let Some(clip) = slot.clip.as_mut()
            && let Some(content) = p.contents.iter().find(|c| c.id == clip.content).cloned()
        {
            next_content = next_content.checked_add(1).ok_or("the project has too many clip contents")?;
            let id = suhina_model::ContentId(next_content);
            p.contents.push(suhina_model::ClipContent { id, ..content });
            clip.content = id;
        }
        // The copy is a new clip: it gets its own id when the command is done.
        if let Some(clip) = slot.clip.as_mut() {
            clip.id = suhina_model::ClipId::NONE;
        }
        column.slots.insert(scene + 1, slot);
    }
    Ok(json!({ "scene": scene + 2 }))
}

fn delete_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(&session.project, params)?;
    let p = &mut session.project;
    if p.launcher.scenes.len() <= 1 {
        return Err("the last scene cannot be deleted".into());
    }
    even_slots(p);
    p.launcher.scenes.remove(scene);
    for column in &mut p.launcher.columns {
        column.slots.remove(scene);
    }
    Ok(json!({ "deleted": scene + 1 }))
}

fn rename_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(&session.project, params)?;
    let name = match params.get("name") {
        Some(Value::String(name)) => name.trim().to_string(),
        _ => return Err("name must be a text (empty clears it)".into()),
    };
    if let Some(s) = session.project.launcher.scenes.get_mut(scene) {
        s.name.clone_from(&name);
    }
    Ok(json!({ "scene": scene + 1, "name": name }))
}

fn set_scene_tempo(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(&session.project, params)?;
    let bpm = match params.get("bpm") {
        None | Some(Value::Null) => None,
        Some(_) => {
            let bpm = number(params, "bpm")?;
            if !(20.0..=999.0).contains(&bpm) {
                return Err(format!("bpm must be between 20 and 999, got {bpm}").into());
            }
            Some(bpm)
        }
    };
    if let Some(s) = session.project.launcher.scenes.get_mut(scene) {
        s.tempo_bpm = bpm;
    }
    Ok(json!({ "scene": scene + 1, "tempo_bpm": bpm }))
}

fn set_scene_time_signature(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(&session.project, params)?;
    let part = |key: &str| match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| CommandError::from(format!("{key} must be a whole number"))),
    };
    let signature = match (part("numerator")?, part("denominator")?) {
        (None, None) => None,
        (Some(numerator), Some(denominator)) => {
            if !(1..=99).contains(&numerator) {
                return Err(format!("numerator must be between 1 and 99, got {numerator}").into());
            }
            if ![1, 2, 4, 8, 16].contains(&denominator) {
                return Err(format!("denominator must be 1, 2, 4, 8 or 16, got {denominator}").into());
            }
            Some(TimeSignature { numerator, denominator })
        }
        _ => return Err("give both numerator and denominator, or neither to clear the time signature".into()),
    };
    if let Some(s) = session.project.launcher.scenes.get_mut(scene) {
        s.time_signature = signature;
    }
    Ok(json!({ "scene": scene + 1, "time_signature": signature.map(signature_text) }))
}

fn signature_text(ts: TimeSignature) -> String {
    format!("{}/{}", ts.numerator, ts.denominator)
}

fn stop_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let column = column_index(session.project(), params)?;
    send(session, LaunchCommand::StopColumn { column })?;
    Ok(json!({ "queued": "stop" }))
}

fn stop_all(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    send(session, LaunchCommand::StopAll)?;
    Ok(json!({ "queued": "stop" }))
}

fn set_quantization(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let text = params.get("value").and_then(Value::as_str).ok_or("value must be a string such as \"1 bar\"")?;
    let quantization = parse_quantization(text)?;
    session.set_quantization(quantization)?;
    Ok(json!({ "quantization": text.trim() }))
}

fn follow_tempo_map(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    send(session, LaunchCommand::FollowTempoMap)?;
    Ok(json!({ "follows_tempo_map": true }))
}

fn back_to_arrangement(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    send(session, LaunchCommand::BackToArrangement)?;
    Ok(json!({ "arrangement": true }))
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
    p.tracks.iter().find(|t| t.id == track).map_or_else(|| track.0.to_string(), |t| t.name.clone())
}

fn undo(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    if session.undo() { Ok(json!({ "undone": true })) } else { Err("nothing to undo".into()) }
}

fn redo(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    if session.redo() { Ok(json!({ "redone": true })) } else { Err("nothing to redo".into()) }
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
    session.project.channels.iter_mut().find(|c| c.id == channel).ok_or_else(|| "the track's channel is missing".into())
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
    session.project.channels.iter().find(|c| c.id == channel).ok_or_else(|| "the track's channel is missing".into())
}

fn channel_of<'a>(p: &'a Project, track: &Track) -> Option<&'a Channel> {
    let id = track.channel?;
    p.channels.iter().find(|c| c.id == id)
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
