// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Suhina's own project file, `.suhina`: the project model (ADR 0001) as versioned JSON, so a
//! project can be saved and opened again with nothing lost, and read by people and agents
//! (ADR 0010, `docs/spec/suhina-format.md`).
//!
//! ```json
//! { "format": "suhina", "version": 1, "project": { "transport": { … }, "tracks": [ … ], … } }
//! ```
//!
//! Binary data (plug-in state, extensions) is base64. Audio files are referred to by path,
//! relative to the project file; [`rebase_audio`] moves those paths when a project is saved
//! somewhere else.

use std::fmt;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Map, Value, json};
use suhina_model::{
    Arrangement, ArrangementLane, AudioInput, AudioRegion, AutomationLane, AutomationPoint, Branch, Channel, ChannelId,
    ChannelRole, Clip, ClipContent, ClipId, ContentId, ContentKinds, ContentLane, Device, DeviceGroup, DeviceKind,
    DeviceParam, Extension, Extensions, FadeCurve, FadeShape, FollowAction, FollowKind, InputChannels, LaneData,
    LaneId, LaunchMapping, LaunchMode, LaunchQuantization, LaunchSettings, Launcher, LauncherColumn, LoopRange,
    MAX_CHANGE_BEAT, MAX_INPUT_CHANNEL, MAX_TEMPO_CHANGES, MAX_TIME_SIGNATURE_CHANGES, Macro, MacroMapping,
    MappingSource, MappingTarget, Marker, MidiInput, MidiPort, Monitor, Note, Project, Rgb, Scale, ScaleMode, Scene,
    SceneId, Send, Slot, StretchMode, TempoPoint, TimeSignature, TimeSignatureChange, Track, TrackId, Transport,
    WarpMarker, automation, has_groups, valid_bpm,
};

/// The file extension, without the dot.
pub const EXTENSION: &str = "suhina";
/// The `format` every file names.
const FORMAT: &str = "suhina";
/// The newest version this crate reads, and the one it writes for a project that holds a device
/// group. A project without one is written as [`VERSION_WITHOUT_GROUPS`], so a Suhina from before
/// device groups still opens it (`docs/spec/device-groups.md` › Project file).
pub const VERSION: u64 = 2;
/// The version a project without device groups is written as.
pub const VERSION_WITHOUT_GROUPS: u64 = 1;
/// A larger file is refused before it is parsed.
const MAX_FILE: u64 = 512 * 1024 * 1024;

/// Why a file could not be read as a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFileError(pub String);

impl fmt::Display for ProjectFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProjectFileError {}

impl From<String> for ProjectFileError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

impl From<&str> for ProjectFileError {
    fn from(message: &str) -> Self {
        Self(message.to_string())
    }
}

/// Whether `path` names a project file by its extension, `.suhina` in either case.
pub fn is_project_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

/// Reads a project file.
pub fn read_file(path: &Path) -> Result<Project, ProjectFileError> {
    let file = std::fs::File::open(path).map_err(|e| format!("could not open the project: {e}"))?;
    let mut text = String::new();
    file.take(MAX_FILE + 1).read_to_string(&mut text).map_err(|e| format!("could not read the project: {e}"))?;
    if text.len() as u64 > MAX_FILE {
        return Err(format!("the project is larger than {} MiB", MAX_FILE / 1024 / 1024).into());
    }
    from_json(&text)
}

/// Writes `project` to `path`: to a file beside it first, then moved over it, so a failed save
/// never leaves half a project.
pub fn write_file(path: &Path, project: &Project) -> Result<(), ProjectFileError> {
    let failed = |e: std::io::Error| ProjectFileError(format!("could not save {}: {e}", path.display()));
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".saving");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, to_json(project)).map_err(failed)?;
    std::fs::rename(&temporary, path).map_err(|e| {
        let _gone = std::fs::remove_file(&temporary);
        failed(e)
    })
}

/// The project as the text of a `.suhina` file.
pub fn to_json(project: &Project) -> String {
    let file = json!({ "format": FORMAT, "version": version_of(project), "project": project_value(project) });
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// The version `project` is written as: [`VERSION`] when a channel holds a device group, else
/// [`VERSION_WITHOUT_GROUPS`].
pub fn version_of(project: &Project) -> u64 {
    if project.channels.iter().any(|c| has_groups(&c.devices)) { VERSION } else { VERSION_WITHOUT_GROUPS }
}

/// Reads the text of a `.suhina` file. The project must validate (ids unique, references
/// resolve), as an imported one must.
pub fn from_json(text: &str) -> Result<Project, ProjectFileError> {
    let root = serde_json::from_str::<Value>(text).map_err(|e| format!("the project is not JSON: {e}"))?;
    let root = root.as_object().ok_or("a project file is a JSON object")?;
    match root.get("format") {
        Some(Value::String(format)) if format == FORMAT => {}
        Some(Value::String(format)) => {
            // Named in the message, cut short: the text is the file's, so it may be anything.
            let found: String = format.chars().take(40).collect();
            return Err(format!("this is not a Suhina project: its format is \"{found}\", not \"{FORMAT}\"").into());
        }
        _ => return Err(format!("this is not a Suhina project: it does not name the format \"{FORMAT}\"").into()),
    }
    let version = root.get("version").and_then(Value::as_u64).ok_or("the project's version is missing")?;
    if version == 0 || version > VERSION {
        return Err(format!("the project is version {version}; this Suhina reads up to version {VERSION}").into());
    }
    let project = read_project(root.get("project").ok_or("the project is missing")?)?;
    project.validate().map_err(|errors| {
        let list: Vec<String> = errors.iter().take(5).map(ToString::to_string).collect();
        format!("the project does not hold together: {}", list.join("; "))
    })?;
    Ok(project)
}

/// Moves the audio file paths of `project` from `from` (the folder they are relative to now)
/// to `to` (the folder of the file it is saved in): each relative path is written relative to
/// `to` where both are on one drive, and the absolute path is kept for when it does not resolve.
pub fn rebase_audio(project: &mut Project, from: &Path, to: &Path) {
    for device in project.channels.iter_mut().flat_map(|c| &mut c.devices) {
        let Some(source) = device.sample_file() else { continue };
        let here = from.join(source);
        let file = if here.is_file() {
            here
        } else {
            device.sample_absolute_file().map(PathBuf::from).filter(|p| p.is_file()).unwrap_or(here)
        };
        let absolute = normalise(&std::path::absolute(&file).unwrap_or(file));
        let relative = relative_to(&absolute, &normalise(&std::path::absolute(to).unwrap_or(to.into())))
            .map_or_else(|| source.to_owned(), |p| slashes(&p));
        device.set_sample_reference(&relative, &absolute.display().to_string());
    }
    for content in &mut project.contents {
        for lane in &mut content.lanes {
            let LaneData::Audio(region) = &mut lane.data else { continue };
            // An empty audio clip has no file yet: nothing to move.
            if region.file.is_empty() {
                continue;
            }
            let here = from.join(&region.file);
            let file = if here.is_file() {
                here
            } else {
                match region.absolute_file.as_deref().map(PathBuf::from) {
                    Some(absolute) if absolute.is_file() => absolute,
                    _ => here,
                }
            };
            let absolute = normalise(&std::path::absolute(&file).unwrap_or(file));
            if let Some(relative) = relative_to(&absolute, &normalise(&std::path::absolute(to).unwrap_or(to.into()))) {
                region.file = slashes(&relative);
            }
            region.absolute_file = Some(absolute.display().to_string());
        }
    }
}

/// `path` with `.` and `..` resolved without touching the disk.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `path` relative to the folder `base`, both absolute; `None` across drives.
fn relative_to(path: &Path, base: &Path) -> Option<PathBuf> {
    let (mut a, mut b) = (path.components().peekable(), base.components().peekable());
    match (a.peek(), b.peek()) {
        (Some(Component::Prefix(x)), Some(Component::Prefix(y))) if x != y => return None,
        _ => {}
    }
    while let (Some(x), Some(y)) = (a.peek(), b.peek()) {
        if x != y {
            break;
        }
        a.next();
        b.next();
    }
    let mut relative = PathBuf::new();
    for _ in b {
        relative.push("..");
    }
    relative.extend(a);
    Some(relative)
}

/// A path written with `/`, the same on every system.
fn slashes(path: &Path) -> String {
    path.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

// Writing.

fn project_value(p: &Project) -> Value {
    let mut out = Map::new();
    let mut transport = json!({
        "tempo_bpm": number(p.transport.tempo_bpm),
        "time_signature": [p.transport.time_signature.numerator, p.transport.time_signature.denominator],
    });
    // Left out when there is no loop region, so such a project is written as before.
    if let Some(range) = p.transport.loop_range {
        transport["loop"] =
            json!({ "start": number(range.start), "end": number(range.end), "enabled": p.transport.loop_enabled });
    }
    // The changes are written only when there are some, so a project without them is saved as
    // it always was.
    if let Some(t) = transport.as_object_mut() {
        if !p.transport.tempo_changes.is_empty() {
            let changes = p.transport.tempo_changes.iter();
            t.insert(
                "tempo_changes".into(),
                changes.map(|c| json!({ "beat": number(c.beat), "bpm": number(c.bpm), "ramp": c.ramp })).collect(),
            );
        }
        if !p.transport.time_signature_changes.is_empty() {
            let changes = p.transport.time_signature_changes.iter();
            t.insert(
                "time_signature_changes".into(),
                changes
                    .map(|c| {
                        json!({ "bar": c.bar, "time_signature": [c.time_signature.numerator, c.time_signature.denominator] })
                    })
                    .collect(),
            );
        }
    }
    out.insert("transport".into(), transport);
    out.insert("tracks".into(), p.tracks.iter().map(track_value).collect());
    out.insert("channels".into(), p.channels.iter().map(channel_value).collect());
    out.insert("contents".into(), p.contents.iter().map(content_value).collect());
    let mut arrangement = json!({ "lanes": p.arrangement.lanes.iter().map(|lane| {
        let mut out = json!({
            "name": lane.name,
            "track": lane.track.map(|t| t.0),
            "clips": lane.clips.iter().map(clip_value).collect::<Vec<_>>(),
        });
        with_id(&mut out, lane.id.0);
        out
    }).collect::<Vec<_>>() });
    // Left out when there are none, so a project without markers is written as before.
    if !p.arrangement.markers.is_empty() {
        arrangement["markers"] =
            p.arrangement.markers.iter().map(|m| json!({ "beat": number(m.beat), "name": m.name })).collect();
    }
    out.insert("arrangement".into(), arrangement);
    let mut launcher = json!({
        "scenes": p.launcher.scenes.iter().map(scene_value).collect::<Vec<_>>(),
        "columns": p.launcher.columns.iter().map(|c| json!({
            "track": c.track.0,
            "slots": c.slots.iter().map(|s| json!({ "clip": s.clip.as_ref().map(clip_value), "has_stop": s.has_stop })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    // Left out when there are none, so a project without mappings is written as before.
    if let (Some(map), false) = (launcher.as_object_mut(), p.launcher.mappings.is_empty()) {
        map.insert("mappings".into(), p.launcher.mappings.iter().map(mapping_value).collect());
    }
    out.insert("launcher".into(), launcher);
    // Left out when there is none, so a project without one is written as before.
    if let Some(scale) = p.default_scale {
        out.insert("default_scale".into(), scale_value(scale));
    }
    // The counter the ids came from, so an id is never handed out twice (ADR 0025); left out
    // while nothing has an id.
    if p.next_id != 0 {
        out.insert("next_id".into(), json!(p.next_id));
    }
    with_extensions(&mut out, &p.extensions);
    Value::Object(out)
}

/// Writes an object's `id`, unless it has none yet (0): a reader gives it one then.
fn with_id(value: &mut Value, id: u32) {
    if let (Some(map), true) = (value.as_object_mut(), id != 0) {
        map.insert("id".into(), json!(id));
    }
}

/// An object's `id`, or 0 (none: [`Project::assign_ids`] gives it one) when it is missing, as in
/// a file written before ids. An id is from 1 to `u32::MAX - 1`; the largest value is never one,
/// so the counter can always name the next.
fn read_id(o: &Obj, at: &str) -> Result<u32, ProjectFileError> {
    match o.get("id") {
        None | Some(Value::Null) => Ok(0),
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|&n| n != 0 && n != u32::MAX)
            .ok_or_else(|| ProjectFileError(format!("{at}.id: expected a whole number from 1 to {}", u32::MAX - 1))),
    }
}

fn track_value(t: &Track) -> Value {
    let mut out = json!({
        "id": t.id.0,
        "name": t.name,
        "color": t.color.map(|Rgb(r, g, b)| format!("#{r:02X}{g:02X}{b:02X}")),
        "notes": t.content.notes,
        "audio": t.content.audio,
        "channel": t.channel.map(|c| c.0),
        "parent": t.parent.map(|p| p.0),
    });
    if let Some(map) = out.as_object_mut() {
        // Left out while it is the default: every port, every channel.
        if t.midi_input != MidiInput::default() {
            let port = match &t.midi_input.port {
                MidiPort::All => Value::String("all".into()),
                MidiPort::Nothing => Value::String("none".into()),
                MidiPort::Named(name) => json!({ "name": name }),
            };
            let monitor = match t.midi_input.monitor {
                Monitor::Auto => "auto",
                Monitor::In => "in",
                Monitor::Off => "off",
            };
            map.insert(
                "midi_input".into(),
                json!({ "port": port, "channel": t.midi_input.channel, "monitor": monitor }),
            );
        }
        // Left out while it is the default: no input, monitored automatically.
        if t.audio_input != AudioInput::default() {
            let channels = match t.audio_input.channels {
                InputChannels::None => json!([]),
                InputChannels::Mono(c) => json!([c]),
                InputChannels::Stereo(c) => json!([c, c.saturating_add(1)]),
            };
            let monitor = match t.audio_input.monitor {
                Monitor::Auto => "auto",
                Monitor::In => "in",
                Monitor::Off => "off",
            };
            map.insert("audio_input".into(), json!({ "channels": channels, "monitor": monitor }));
        }
        with_extensions(map, &t.extensions);
    }
    out
}

fn channel_value(c: &Channel) -> Value {
    let role = match c.role {
        ChannelRole::Regular => "regular",
        ChannelRole::Master => "master",
        ChannelRole::Effect => "effect",
        ChannelRole::Submix => "submix",
    };
    let mut out = json!({
        "id": c.id.0,
        "name": c.name,
        "role": role,
        "volume": number(c.volume),
        "pan": number(c.pan),
        "mute": c.mute,
        "solo": c.solo,
        "output": c.output.map(|o| o.0),
        "sends": c.sends.iter().map(|s| json!({ "target": s.target.0, "level": number(s.level), "pre_fader": s.pre_fader })).collect::<Vec<_>>(),
        "devices": c.devices.iter().map(device_value).collect::<Vec<_>>(),
    });
    if let Some(map) = out.as_object_mut() {
        // Written only when on, so a project without a cued track reads as it did before.
        if c.cue {
            map.insert("cue".into(), json!(true));
        }
        if !c.automation.is_empty() {
            map.insert("automation".into(), lanes_value(&c.automation));
        }
        with_extensions(map, &c.extensions);
    }
    out
}

/// Automation lanes: a lane's `enabled` and a point's `curve` are written only when they are not
/// the default (on, a straight line), so files keep their shape.
fn lanes_value(lanes: &[AutomationLane]) -> Value {
    let lanes: Vec<Value> = lanes
        .iter()
        .map(|lane| {
            let points: Vec<Value> = lane
                .points
                .iter()
                .map(|p| {
                    let mut point = json!({ "beat": number(p.beat), "value": number(p.value) });
                    if p.curve != 0.0
                        && let Some(map) = point.as_object_mut()
                    {
                        map.insert("curve".into(), json!(number(p.curve)));
                    }
                    point
                })
                .collect();
            let mut out = json!({ "param": lane.param, "points": points });
            if !lane.enabled
                && let Some(map) = out.as_object_mut()
            {
                map.insert("enabled".into(), Value::Bool(false));
            }
            out
        })
        .collect();
    Value::Array(lanes)
}

fn follow_value(f: &FollowAction) -> Value {
    json!({
        "enabled": f.enabled,
        "time": number(f.time),
        "linked": f.linked,
        "a": f.action_a.name(),
        "b": f.action_b.name(),
        "chance_a": f.chance_a,
        "chance_b": f.chance_b,
    })
}

fn device_value(d: &Device) -> Value {
    let kind = match &d.kind {
        DeviceKind::Builtin(id) => json!({ "type": "builtin", "id": id }),
        DeviceKind::Sd => json!({ "type": "sd" }),
        DeviceKind::Clap { id } => json!({ "type": "clap", "id": id }),
        DeviceKind::Vst3 { id } => json!({ "type": "vst3", "id": id }),
        DeviceKind::Vst2 { id } => json!({ "type": "vst2", "id": id }),
        DeviceKind::Foreign { format, id } => json!({ "type": "foreign", "format": format, "id": id }),
        DeviceKind::Group(group) => group_value(group),
    };
    let mut out = json!({
        "name": d.name,
        "kind": kind,
        "enabled": d.enabled,
        "state": d.state.as_ref().map(|s| BASE64.encode(s)),
        "params": d.params.iter().map(|p| json!({ "id": p.id, "value": number(p.value) })).collect::<Vec<_>>(),
    });
    if let Some(map) = out.as_object_mut() {
        // Only devices with automation say so, so files without any keep their shape.
        if !d.automation.is_empty() {
            map.insert("automation".into(), lanes_value(&d.automation));
        }
        with_extensions(map, &d.extensions);
    }
    out
}

fn group_value(g: &DeviceGroup) -> Value {
    let branches: Vec<Value> = g
        .branches
        .iter()
        .map(|b| {
            let mut out = json!({
                "name": b.name,
                "volume": number(b.volume),
                "pan": number(b.pan),
                "mute": b.mute,
                "solo": b.solo,
                "devices": b.devices.iter().map(device_value).collect::<Vec<_>>(),
            });
            if let Some(map) = out.as_object_mut() {
                with_extensions(map, &b.extensions);
            }
            out
        })
        .collect();
    let macros: Vec<Value> = g
        .macros
        .iter()
        .map(|m| {
            let mappings: Vec<Value> = m
                .mappings
                .iter()
                .map(|x| json!({ "device": x.device, "param": x.param, "min": number(x.min), "max": number(x.max) }))
                .collect();
            json!({ "name": m.name, "mappings": mappings })
        })
        .collect();
    json!({ "type": "group", "branches": branches, "macros": macros })
}

fn content_value(c: &ClipContent) -> Value {
    let lanes: Vec<Value> = c
        .lanes
        .iter()
        .map(|lane| {
            let target = lane.target.map(|t| t.0);
            match &lane.data {
                LaneData::Notes(notes) => {
                    json!({ "target": target, "notes": notes.iter().map(note_value).collect::<Vec<_>>() })
                }
                LaneData::Audio(a) => json!({ "target": target, "audio": audio_value(a) }),
            }
        })
        .collect();
    let mut out = json!({ "id": c.id.0, "name": c.name, "length": number(c.length), "lanes": lanes });
    if let Some(map) = out.as_object_mut() {
        // Left out when the content has none, so a project without scales is written as before.
        if let Some(scale) = c.scale {
            map.insert("scale".into(), scale_value(scale));
        }
        with_extensions(map, &c.extensions);
    }
    out
}

/// A scale as the file writes it: `{ "root": 0 to 11, "mode": "<name>" }`.
fn scale_value(scale: Scale) -> Value {
    json!({ "root": scale.root(), "mode": scale.mode.name() })
}

fn audio_value(a: &AudioRegion) -> Value {
    json!({
        "file": a.file,
        "absolute_file": a.absolute_file,
        "sample_rate": a.sample_rate,
        "channels": a.channels,
        "duration_seconds": a.duration_seconds.map(number),
        "warped": a.warped,
        "stretch": stretch_name(a.stretch),
        "warp_markers": a.warp_markers.iter().map(|m| json!({ "time": number(m.time), "audio_time": number(m.audio_time) })).collect::<Vec<_>>(),
        "gain": number(a.gain),
        "transpose": number(a.transpose),
        "stream": a.stream,
    })
}

/// One note as the file writes it. `muted` is written only on a muted note, so a project without
/// muted notes is byte-identical to a file written before muted notes existed.
fn note_value(n: &Note) -> Value {
    let mut out = json!({
        "time": number(n.time),
        "duration": number(n.duration),
        "key": n.key,
        "velocity": number(f64::from(n.velocity)),
        "release_velocity": number(f64::from(n.release_velocity)),
        "channel": n.channel,
    });
    if n.muted
        && let Some(map) = out.as_object_mut()
    {
        map.insert("muted".into(), Value::Bool(true));
    }
    out
}

fn clip_value(c: &Clip) -> Value {
    let mut out = json!({
        "content": c.content.0,
        "start": number(c.start),
        "length": number(c.length),
        "offset": number(c.offset),
        "loop": c.loop_range.map(|l| json!([number(l.start), number(l.end)])),
        "enabled": c.enabled,
        "fade_in": number(c.fade_in),
        "fade_out": number(c.fade_out),
        "fade_in_shape": c.fade_in_curve.shape.name(),
        "fade_in_curve": number(c.fade_in_curve.curve),
        "fade_out_shape": c.fade_out_curve.shape.name(),
        "fade_out_curve": number(c.fade_out_curve.curve),
    });
    with_id(&mut out, c.id.0);
    if let Some(map) = out.as_object_mut() {
        if c.follow_action != FollowAction::default() {
            map.insert("follow".into(), follow_value(&c.follow_action));
        }
        // Only a clip whose launch settings were changed carries them, so most clips (and every
        // arrangement clip that never was in a slot) stay as they were written before.
        if !c.launch.is_default() {
            map.insert("launch".into(), launch_value(&c.launch));
        }
        // Only a clip with a colour of its own carries one; the others show their track's.
        if let Some(Rgb(r, g, b)) = c.color {
            map.insert("color".into(), json!(format!("#{r:02X}{g:02X}{b:02X}")));
        }
        with_extensions(map, &c.extensions);
    }
    out
}

/// A launch mapping; its scene counts from 1, as in the commands.
fn mapping_value(m: &LaunchMapping) -> Value {
    let mut out = json!({ "source": m.source.name(), "target": m.target.kind() });
    if let Some(map) = out.as_object_mut() {
        if let Some(track) = m.target.track() {
            map.insert("track".into(), json!(track.0));
        }
        if let Some(scene) = m.target.scene() {
            map.insert("scene".into(), json!(scene.saturating_add(1)));
        }
    }
    out
}

fn launch_value(l: &LaunchSettings) -> Value {
    json!({ "mode": l.mode.name(), "legato": l.legato, "quantization": l.quantization.name() })
}

fn with_extensions(map: &mut Map<String, Value>, extensions: &Extensions) {
    if !extensions.0.is_empty() {
        let list = extensions
            .0
            .iter()
            .map(|e| json!({ "format": e.format, "key": e.key, "data": BASE64.encode(&e.data) }))
            .collect();
        map.insert("extensions".into(), Value::Array(list));
    }
}

/// JSON has no NaN or infinity; a number that is not finite is written as 0.
fn number(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

fn stretch_name(mode: StretchMode) -> &'static str {
    match mode {
        StretchMode::Repitch => "repitch",
        StretchMode::Rhythmic => "rhythmic",
        StretchMode::Tonal => "tonal",
        StretchMode::Texture => "texture",
        StretchMode::Smooth => "smooth",
        StretchMode::HighQuality => "high_quality",
    }
}

// Reading. Every value is checked; a wrong one is an error naming where it is.

type Obj = Map<String, Value>;

fn object<'a>(value: &'a Value, at: &str) -> Result<&'a Obj, ProjectFileError> {
    value.as_object().ok_or_else(|| ProjectFileError(format!("{at}: expected an object")))
}

fn list<'a>(o: &'a Obj, key: &str, at: &str) -> Result<&'a [Value], ProjectFileError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(format!("{at}.{key}: expected a list").into()),
    }
}

fn float(o: &Obj, key: &str, at: &str) -> Result<f64, ProjectFileError> {
    o.get(key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or_else(|| ProjectFileError(format!("{at}.{key}: expected a number")))
}

fn float_or(o: &Obj, key: &str, default: f64, at: &str) -> Result<f64, ProjectFileError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(_) => float(o, key, at),
    }
}

fn optional_float(o: &Obj, key: &str, at: &str) -> Result<Option<f64>, ProjectFileError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => float(o, key, at).map(Some),
    }
}

fn whole<T: TryFrom<u64>>(o: &Obj, key: &str, at: &str) -> Result<T, ProjectFileError> {
    o.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| T::try_from(n).ok())
        .ok_or_else(|| ProjectFileError(format!("{at}.{key}: expected a whole number in range")))
}

fn optional_whole<T: TryFrom<u64>>(o: &Obj, key: &str, at: &str) -> Result<Option<T>, ProjectFileError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => whole(o, key, at).map(Some),
    }
}

fn flag(o: &Obj, key: &str, default: bool, at: &str) -> Result<bool, ProjectFileError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(format!("{at}.{key}: expected true or false").into()),
    }
}

fn text(o: &Obj, key: &str, at: &str) -> Result<String, ProjectFileError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(_) => Err(format!("{at}.{key}: expected text").into()),
    }
}

fn required_text(o: &Obj, key: &str, at: &str) -> Result<String, ProjectFileError> {
    match o.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        _ => Err(format!("{at}.{key}: expected text").into()),
    }
}

fn bytes(o: &Obj, key: &str, at: &str) -> Result<Option<Vec<u8>>, ProjectFileError> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            BASE64.decode(s).map(Some).map_err(|e| ProjectFileError(format!("{at}.{key}: not base64: {e}")))
        }
        Some(_) => Err(format!("{at}.{key}: expected base64 text").into()),
    }
}

fn extensions(o: &Obj, at: &str) -> Result<Extensions, ProjectFileError> {
    let mut out = Vec::new();
    for (i, item) in list(o, "extensions", at)?.iter().enumerate() {
        let at = format!("{at}.extensions[{i}]");
        let e = object(item, &at)?;
        out.push(Extension {
            format: required_text(e, "format", &at)?,
            key: required_text(e, "key", &at)?,
            data: bytes(e, "data", &at)?.unwrap_or_default(),
        });
    }
    Ok(Extensions(out))
}

fn scene_value(s: &Scene) -> Value {
    let mut scene = json!({ "name": s.name, "tempo_bpm": s.tempo_bpm.map(number) });
    with_id(&mut scene, s.id.0);
    if let (Some(ts), Some(object)) = (s.time_signature, scene.as_object_mut()) {
        object.insert("time_signature".into(), json!([ts.numerator, ts.denominator]));
    }
    scene
}

/// A time signature written as `[numerator, denominator]`, or `None` when absent or null.
fn read_signature(value: Option<&Value>, at: &str) -> Result<Option<TimeSignature>, ProjectFileError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(pair)) if pair.len() == 2 => {
            let part = |v: &Value| v.as_u64().and_then(|n| u8::try_from(n).ok()).filter(|n| *n > 0);
            match (pair.first().and_then(part), pair.get(1).and_then(part)) {
                (Some(numerator), Some(denominator)) => Ok(Some(TimeSignature { numerator, denominator })),
                _ => Err(format!("{at}: expected two whole numbers").into()),
            }
        }
        Some(_) => Err(format!("{at}: expected [numerator, denominator]").into()),
    }
}

/// A time signature written as `[numerator, denominator]`, each a whole number from 1 to 255.
fn time_signature(value: &Value, at: &str) -> Result<TimeSignature, ProjectFileError> {
    let Value::Array(pair) = value else {
        return Err(format!("{at}: expected [numerator, denominator]").into());
    };
    let part = |v: &Value| v.as_u64().and_then(|n| u8::try_from(n).ok()).filter(|n| *n > 0);
    match (pair.len(), pair.first().and_then(part), pair.get(1).and_then(part)) {
        (2, Some(numerator), Some(denominator)) => Ok(TimeSignature { numerator, denominator }),
        (2, _, _) => Err(format!("{at}: expected two whole numbers").into()),
        _ => Err(format!("{at}: expected [numerator, denominator]").into()),
    }
}

/// The transport's tempo changes (`docs/spec/tempo-map.md`): each after beat 0, at a tempo from
/// 1 to 999 bpm.
fn tempo_changes(t: &Obj) -> Result<Vec<TempoPoint>, ProjectFileError> {
    let items = list(t, "tempo_changes", "project.transport")?;
    if items.len() > MAX_TEMPO_CHANGES {
        return Err(format!("project.transport.tempo_changes: more than {MAX_TEMPO_CHANGES}").into());
    }
    items
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let at = format!("project.transport.tempo_changes[{i}]");
            let c = object(v, &at)?;
            let beat = float(c, "beat", &at)?;
            if !(beat > 0.0 && beat <= MAX_CHANGE_BEAT) {
                return Err(format!("{at}.beat: {beat} is not after the start of the song").into());
            }
            let bpm = float(c, "bpm", &at)?;
            if !valid_bpm(bpm) {
                return Err(format!("{at}.bpm: {bpm} is not a tempo").into());
            }
            Ok(TempoPoint { beat, bpm, ramp: flag(c, "ramp", false, &at)? })
        })
        .collect()
}

/// The transport's time-signature changes: each at a bar after the first, with a signature a
/// project keeps.
fn time_signature_changes(t: &Obj) -> Result<Vec<TimeSignatureChange>, ProjectFileError> {
    let items = list(t, "time_signature_changes", "project.transport")?;
    if items.len() > MAX_TIME_SIGNATURE_CHANGES {
        return Err(format!("project.transport.time_signature_changes: more than {MAX_TIME_SIGNATURE_CHANGES}").into());
    }
    items
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let at = format!("project.transport.time_signature_changes[{i}]");
            let c = object(v, &at)?;
            let bar: u32 = whole(c, "bar", &at)?;
            if bar == 0 {
                return Err(format!("{at}.bar: the first bar has the start signature").into());
            }
            let value = c.get("time_signature").unwrap_or(&Value::Null);
            let time_signature = time_signature(value, &format!("{at}.time_signature"))?;
            if !time_signature.is_valid() {
                return Err(format!("{at}.time_signature: not a time signature").into());
            }
            Ok(TimeSignatureChange { bar, time_signature })
        })
        .collect()
}

fn read_project(value: &Value) -> Result<Project, ProjectFileError> {
    let p = object(value, "project")?;
    let transport = match p.get("transport") {
        None | Some(Value::Null) => Transport::default(),
        Some(t) => {
            let t = object(t, "project.transport")?;
            let signature = read_signature(t.get("time_signature"), "project.transport.time_signature")?
                .unwrap_or(TimeSignature { numerator: 4, denominator: 4 });
            let tempo = float_or(t, "tempo_bpm", 120.0, "project.transport")?;
            if !(1.0..=999.0).contains(&tempo) {
                return Err(format!("project.transport.tempo_bpm: {tempo} is not a tempo").into());
            }
            let (loop_range, loop_enabled) = match t.get("loop") {
                None | Some(Value::Null) => (None, false),
                Some(l) => {
                    let at = "project.transport.loop";
                    let l = object(l, at)?;
                    let range = LoopRange { start: float(l, "start", at)?, end: float(l, "end", at)? };
                    if !range.is_valid_region() {
                        return Err(format!("{at}: expected a start from 0 on and an end after it").into());
                    }
                    (Some(range), flag(l, "enabled", false, at)?)
                }
            };
            Transport {
                tempo_bpm: tempo,
                time_signature: signature,
                loop_range,
                loop_enabled,
                tempo_changes: tempo_changes(t)?,
                time_signature_changes: time_signature_changes(t)?,
            }
        }
    };
    let tracks = list(p, "tracks", "project")?
        .iter()
        .enumerate()
        .map(|(i, v)| read_track(v, &format!("project.tracks[{i}]")))
        .collect::<Result<_, _>>()?;
    let channels = list(p, "channels", "project")?
        .iter()
        .enumerate()
        .map(|(i, v)| read_channel(v, &format!("project.channels[{i}]")))
        .collect::<Result<_, _>>()?;
    let contents = list(p, "contents", "project")?
        .iter()
        .enumerate()
        .map(|(i, v)| read_content(v, &format!("project.contents[{i}]")))
        .collect::<Result<_, _>>()?;
    let arrangement = match p.get("arrangement") {
        None | Some(Value::Null) => Arrangement::default(),
        Some(a) => {
            let a = object(a, "project.arrangement")?;
            let mut lanes = Vec::new();
            for (i, v) in list(a, "lanes", "project.arrangement")?.iter().enumerate() {
                let at = format!("project.arrangement.lanes[{i}]");
                let lane = object(v, &at)?;
                let clips = list(lane, "clips", &at)?
                    .iter()
                    .enumerate()
                    .map(|(j, c)| read_clip(c, &format!("{at}.clips[{j}]")))
                    .collect::<Result<_, _>>()?;
                lanes.push(ArrangementLane {
                    id: LaneId(read_id(lane, &at)?),
                    name: text(lane, "name", &at)?,
                    track: optional_whole(lane, "track", &at)?.map(TrackId),
                    clips,
                });
            }
            let mut markers = Vec::new();
            for (i, v) in list(a, "markers", "project.arrangement")?.iter().enumerate() {
                let at = format!("project.arrangement.markers[{i}]");
                let m = object(v, &at)?;
                let beat = float(m, "beat", &at)?;
                if beat < 0.0 {
                    return Err(format!("{at}.beat: expected a beat from 0 on").into());
                }
                markers.push(Marker { beat, name: text(m, "name", &at)? });
            }
            let mut arrangement = Arrangement { lanes, markers };
            arrangement.sort_markers();
            arrangement
        }
    };
    let launcher = match p.get("launcher") {
        None | Some(Value::Null) => Launcher::default(),
        Some(l) => {
            let l = object(l, "project.launcher")?;
            let mut scenes = Vec::new();
            for (i, v) in list(l, "scenes", "project.launcher")?.iter().enumerate() {
                let at = format!("project.launcher.scenes[{i}]");
                let s = object(v, &at)?;
                scenes.push(Scene {
                    id: SceneId(read_id(s, &at)?),
                    name: text(s, "name", &at)?,
                    tempo_bpm: optional_float(s, "tempo_bpm", &at)?,
                    time_signature: read_signature(s.get("time_signature"), &format!("{at}.time_signature"))?,
                });
            }
            let mut columns = Vec::new();
            for (i, v) in list(l, "columns", "project.launcher")?.iter().enumerate() {
                let at = format!("project.launcher.columns[{i}]");
                let c = object(v, &at)?;
                let mut slots = Vec::new();
                for (j, s) in list(c, "slots", &at)?.iter().enumerate() {
                    let at = format!("{at}.slots[{j}]");
                    let s = object(s, &at)?;
                    let clip = match s.get("clip") {
                        None | Some(Value::Null) => None,
                        Some(clip) => Some(read_clip(clip, &format!("{at}.clip"))?),
                    };
                    slots.push(Slot { clip, has_stop: flag(s, "has_stop", false, &at)? });
                }
                columns.push(LauncherColumn { track: TrackId(whole(c, "track", &at)?), slots });
            }
            let mut mappings: Vec<LaunchMapping> = Vec::new();
            for (i, v) in list(l, "mappings", "project.launcher")?.iter().enumerate() {
                let mapping = read_mapping(v, &format!("project.launcher.mappings[{i}]"))?;
                if mappings.iter().any(|m| m.source == mapping.source) {
                    return Err(ProjectFileError(format!(
                        "project.launcher.mappings[{i}]: {} is mapped twice",
                        mapping.source.name()
                    )));
                }
                mappings.push(mapping);
            }
            Launcher { scenes, columns, mappings }
        }
    };
    let mut project = Project {
        transport,
        tracks,
        channels,
        contents,
        arrangement,
        launcher,
        extensions: extensions(p, "project")?,
        default_scale: read_scale(p, "default_scale", "project")?,
        next_id: optional_whole(p, "next_id", "project")?.unwrap_or(0),
    };
    // A file written before ids, or an object added by hand without one: the objects without an
    // id get the next ones in document order, so the same file always reads with the same ids.
    // A stale `next_id` moves past the ids in use.
    project.assign_ids().map_err(|e| ProjectFileError(format!("project: {e}")))?;
    Ok(project)
}

fn read_track(value: &Value, at: &str) -> Result<Track, ProjectFileError> {
    let t = object(value, at)?;
    let color = match t.get("color") {
        None | Some(Value::Null) => None,
        Some(Value::String(hex)) => {
            let digits = hex.strip_prefix('#').unwrap_or(hex);
            let parsed = (digits.len() == 6 && digits.chars().all(|c| c.is_ascii_hexdigit()))
                .then(|| u32::from_str_radix(digits, 16).ok())
                .flatten();
            let rgb = parsed.ok_or_else(|| ProjectFileError(format!("{at}.color: expected #RRGGBB")))?;
            Some(Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8))
        }
        Some(_) => return Err(format!("{at}.color: expected #RRGGBB").into()),
    };
    Ok(Track {
        id: TrackId(whole(t, "id", at)?),
        name: text(t, "name", at)?,
        color,
        content: ContentKinds { notes: flag(t, "notes", false, at)?, audio: flag(t, "audio", false, at)? },
        channel: optional_whole(t, "channel", at)?.map(ChannelId),
        parent: optional_whole(t, "parent", at)?.map(TrackId),
        midi_input: read_midi_input(t.get("midi_input"), at)?,
        audio_input: read_audio_input(t.get("audio_input"), at)?,
        extensions: extensions(t, at)?,
    })
}

/// A track's `audio_input`: `{ "channels": [] | [n] | [n, n + 1], "monitor": "auto" | "in" |
/// "off" }`, channels counted from 1; no input, monitored automatically, when it is left out.
fn read_audio_input(value: Option<&Value>, at: &str) -> Result<AudioInput, ProjectFileError> {
    let at = format!("{at}.audio_input");
    let Some(value) = value.filter(|v| !v.is_null()) else { return Ok(AudioInput::default()) };
    let a = object(value, &at)?;
    let channel =
        |v: &Value| v.as_u64().and_then(|c| u16::try_from(c).ok()).filter(|c| (1..=MAX_INPUT_CHANNEL).contains(c));
    let bad = || {
        ProjectFileError(format!("{at}.channels: expected [], [n] or [n, n + 1] with n from 1 to {MAX_INPUT_CHANNEL}"))
    };
    let channels = match a.get("channels") {
        None | Some(Value::Null) => InputChannels::None,
        Some(Value::Array(list)) => match list.as_slice() {
            [] => InputChannels::None,
            [one] => InputChannels::Mono(channel(one).ok_or_else(bad)?),
            [left, right] => {
                let (left, right) = (channel(left).ok_or_else(bad)?, channel(right).ok_or_else(bad)?);
                if left.checked_add(1) != Some(right) {
                    return Err(bad());
                }
                InputChannels::Stereo(left)
            }
            _ => return Err(bad()),
        },
        Some(_) => return Err(bad()),
    };
    let monitor = match a.get("monitor").and_then(Value::as_str) {
        None if a.get("monitor").is_none_or(Value::is_null) => Monitor::Auto,
        Some("auto") => Monitor::Auto,
        Some("in") => Monitor::In,
        Some("off") => Monitor::Off,
        _ => return Err(format!("{at}.monitor: expected \"auto\", \"in\" or \"off\"").into()),
    };
    Ok(AudioInput { channels, monitor })
}

/// A track's `midi_input`: `{ "port": "all" | "none" | { "name" }, "channel": 1–16 or null }`,
/// every port and channel when it is left out.
fn read_midi_input(value: Option<&Value>, at: &str) -> Result<MidiInput, ProjectFileError> {
    let at = format!("{at}.midi_input");
    let Some(value) = value.filter(|v| !v.is_null()) else { return Ok(MidiInput::default()) };
    let m = object(value, &at)?;
    let port = match m.get("port") {
        None | Some(Value::Null) => MidiPort::All,
        Some(Value::String(s)) if s == "all" => MidiPort::All,
        Some(Value::String(s)) if s == "none" => MidiPort::Nothing,
        Some(Value::Object(port)) => match port.get("name").and_then(Value::as_str) {
            Some(name) if !name.trim().is_empty() => MidiPort::Named(name.to_string()),
            _ => return Err(format!("{at}.port.name: expected a port's name").into()),
        },
        Some(_) => return Err(format!("{at}.port: expected \"all\", \"none\" or {{ \"name\" }}").into()),
    };
    let channel = match m.get("channel") {
        None | Some(Value::Null) => None,
        Some(v) => match v.as_u64() {
            Some(c @ 1..=16) => u8::try_from(c).ok(),
            _ => return Err(format!("{at}.channel: expected 1 to 16 or null").into()),
        },
    };
    let monitor = match m.get("monitor") {
        None | Some(Value::Null) => Monitor::Auto,
        Some(Value::String(s)) => match s.as_str() {
            "auto" => Monitor::Auto,
            "in" => Monitor::In,
            "off" => Monitor::Off,
            _ => return Err(format!("{at}.monitor: expected auto, in or off").into()),
        },
        Some(_) => return Err(format!("{at}.monitor: expected auto, in or off").into()),
    };
    Ok(MidiInput { port, channel, monitor })
}

fn read_channel(value: &Value, at: &str) -> Result<Channel, ProjectFileError> {
    let c = object(value, at)?;
    let role = match c.get("role").and_then(Value::as_str) {
        None | Some("regular") => ChannelRole::Regular,
        Some("master") => ChannelRole::Master,
        Some("effect") => ChannelRole::Effect,
        Some("submix") => ChannelRole::Submix,
        Some(other) => return Err(format!("{at}.role: {other} is not a channel role").into()),
    };
    let mut sends = Vec::new();
    for (i, v) in list(c, "sends", at)?.iter().enumerate() {
        let at = format!("{at}.sends[{i}]");
        let s = object(v, &at)?;
        sends.push(Send {
            target: ChannelId(whole(s, "target", &at)?),
            level: float_or(s, "level", 1.0, &at)?,
            pre_fader: flag(s, "pre_fader", false, &at)?,
        });
    }
    let devices = list(c, "devices", at)?
        .iter()
        .enumerate()
        .map(|(i, v)| read_device(v, &format!("{at}.devices[{i}]")))
        .collect::<Result<_, _>>()?;
    let volume = float_or(c, "volume", 1.0, at)?;
    let pan = float_or(c, "pan", 0.0, at)?;
    if volume < 0.0 || !(-1.0..=1.0).contains(&pan) {
        return Err(format!("{at}: volume must not be negative and pan must be from -1 to 1").into());
    }
    Ok(Channel {
        id: ChannelId(whole(c, "id", at)?),
        name: text(c, "name", at)?,
        role,
        volume,
        pan,
        mute: flag(c, "mute", false, at)?,
        solo: flag(c, "solo", false, at)?,
        cue: flag(c, "cue", false, at)?,
        output: optional_whole(c, "output", at)?.map(ChannelId),
        sends,
        devices,
        automation: read_lanes(c, at)?,
        extensions: extensions(c, at)?,
    })
}

/// The `automation` lanes of a device or channel: points in beat order (those at one beat keep
/// their order), a curve from -1 to 1, at most `MAX_POINTS` points a lane.
fn read_lanes(map: &Obj, at: &str) -> Result<Vec<AutomationLane>, ProjectFileError> {
    let mut automation = Vec::new();
    for (i, v) in list(map, "automation", at)?.iter().enumerate() {
        let at = format!("{at}.automation[{i}]");
        let lane = object(v, &at)?;
        let listed = list(lane, "points", &at)?;
        if listed.len() > automation::MAX_POINTS {
            return Err(format!("{at}.points: more than {} points", automation::MAX_POINTS).into());
        }
        let mut points = Vec::with_capacity(listed.len());
        for (j, p) in listed.iter().enumerate() {
            let at = format!("{at}.points[{j}]");
            let p = object(p, &at)?;
            let curve = float_or(p, "curve", 0.0, &at)?;
            if !(-1.0..=1.0).contains(&curve) {
                return Err(format!("{at}.curve: must be from -1 to 1, got {curve}").into());
            }
            points.push(AutomationPoint { beat: float(p, "beat", &at)?, value: float(p, "value", &at)?, curve });
        }
        // In beat order, as the model keeps them; points at the same beat keep their order.
        points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        automation.push(AutomationLane {
            param: required_text(lane, "param", &at)?,
            points,
            enabled: flag(lane, "enabled", true, &at)?,
        });
    }
    Ok(automation)
}

fn read_device(value: &Value, at: &str) -> Result<Device, ProjectFileError> {
    let d = object(value, at)?;
    let kind_at = format!("{at}.kind");
    let k = object(d.get("kind").unwrap_or(&Value::Null), &kind_at)?;
    let kind = match k.get("type").and_then(Value::as_str) {
        Some("builtin") => DeviceKind::Builtin(required_text(k, "id", &kind_at)?),
        Some("sd") => DeviceKind::Sd,
        Some("clap") => DeviceKind::Clap { id: required_text(k, "id", &kind_at)? },
        Some("vst3") => DeviceKind::Vst3 { id: required_text(k, "id", &kind_at)? },
        Some("vst2") => DeviceKind::Vst2 { id: whole(k, "id", &kind_at)? },
        Some("foreign") => {
            DeviceKind::Foreign { format: required_text(k, "format", &kind_at)?, id: required_text(k, "id", &kind_at)? }
        }
        Some("group") => DeviceKind::Group(Box::new(read_group(k, &kind_at)?)),
        // Dropping a device that is not understood could drop the devices inside it.
        Some(other) => {
            return Err(format!("{kind_at}.type: this project uses a device a newer Suhina knows ({other})").into());
        }
        None => return Err(format!("{kind_at}.type: expected builtin, sd, clap, vst3, vst2, foreign or group").into()),
    };
    let mut params = Vec::new();
    for (i, v) in list(d, "params", at)?.iter().enumerate() {
        let at = format!("{at}.params[{i}]");
        let p = object(v, &at)?;
        params.push(DeviceParam { id: required_text(p, "id", &at)?, value: float(p, "value", &at)? });
    }
    let automation = read_lanes(d, at)?;
    Ok(Device {
        name: text(d, "name", at)?,
        kind,
        enabled: flag(d, "enabled", true, at)?,
        state: bytes(d, "state", at)?,
        params,
        automation,
        extensions: extensions(d, at)?,
    })
}

/// A device group. Its limits (branch and macro counts, depth, ranges) are checked by
/// `Project::validate` after the whole project is read; the JSON parser's own nesting limit
/// bounds how deep this recursion goes before that.
fn read_group(k: &Obj, at: &str) -> Result<DeviceGroup, ProjectFileError> {
    let mut branches = Vec::new();
    for (i, v) in list(k, "branches", at)?.iter().enumerate() {
        let at = format!("{at}.branches[{i}]");
        let b = object(v, &at)?;
        let mut devices = Vec::new();
        for (j, d) in list(b, "devices", &at)?.iter().enumerate() {
            devices.push(read_device(d, &format!("{at}.devices[{j}]"))?);
        }
        branches.push(Branch {
            name: text(b, "name", &at)?,
            volume: float_or(b, "volume", 1.0, &at)?,
            pan: float_or(b, "pan", 0.0, &at)?,
            mute: flag(b, "mute", false, &at)?,
            solo: flag(b, "solo", false, &at)?,
            devices,
            extensions: extensions(b, &at)?,
        });
    }
    let mut macros = Vec::new();
    for (i, v) in list(k, "macros", at)?.iter().enumerate() {
        let at = format!("{at}.macros[{i}]");
        let m = object(v, &at)?;
        let mut mappings = Vec::new();
        for (j, v) in list(m, "mappings", &at)?.iter().enumerate() {
            let at = format!("{at}.mappings[{j}]");
            let x = object(v, &at)?;
            let mut device = Vec::new();
            for step in list(x, "device", &at)? {
                let step = step.as_u64().and_then(|n| u32::try_from(n).ok());
                device.push(step.ok_or_else(|| format!("{at}.device: expected a list of whole numbers"))?);
            }
            mappings.push(MacroMapping {
                device,
                param: required_text(x, "param", &at)?,
                min: float_or(x, "min", 0.0, &at)?,
                max: float_or(x, "max", 1.0, &at)?,
            });
        }
        macros.push(Macro { name: text(m, "name", &at)?, mappings });
    }
    Ok(DeviceGroup { branches, macros })
}

fn read_content(value: &Value, at: &str) -> Result<ClipContent, ProjectFileError> {
    let c = object(value, at)?;
    let mut lanes = Vec::new();
    for (i, v) in list(c, "lanes", at)?.iter().enumerate() {
        let at = format!("{at}.lanes[{i}]");
        let lane = object(v, &at)?;
        let target = optional_whole(lane, "target", &at)?.map(TrackId);
        let data = match (lane.get("notes"), lane.get("audio")) {
            (Some(_), None) => {
                let mut notes = Vec::new();
                for (j, n) in list(lane, "notes", &at)?.iter().enumerate() {
                    let at = format!("{at}.notes[{j}]");
                    let n = object(n, &at)?;
                    let key: u8 = whole(n, "key", &at)?;
                    let channel: u8 = optional_whole(n, "channel", &at)?.unwrap_or(0);
                    if key > 127 || channel > 15 {
                        return Err(format!("{at}: key must be 0 to 127 and channel 0 to 15").into());
                    }
                    notes.push(Note {
                        time: float(n, "time", &at)?,
                        duration: float(n, "duration", &at)?,
                        key,
                        velocity: float_or(n, "velocity", 1.0, &at)?.clamp(0.0, 1.0) as f32,
                        release_velocity: float_or(n, "release_velocity", 0.0, &at)?.clamp(0.0, 1.0) as f32,
                        channel,
                        // A file from before muted notes reads every note as unmuted.
                        muted: flag(n, "muted", false, &at)?,
                    });
                }
                LaneData::Notes(notes)
            }
            (None, Some(audio)) => LaneData::Audio(read_audio(audio, &format!("{at}.audio"))?),
            _ => return Err(format!("{at}: a lane has either notes or audio").into()),
        };
        lanes.push(ContentLane { target, data });
    }
    Ok(ClipContent {
        id: ContentId(whole(c, "id", at)?),
        name: text(c, "name", at)?,
        length: float(c, "length", at)?,
        lanes,
        extensions: extensions(c, at)?,
        scale: read_scale(c, "scale", at)?,
    })
}

fn read_audio(value: &Value, at: &str) -> Result<AudioRegion, ProjectFileError> {
    let a = object(value, at)?;
    let stretch = match a.get("stretch").and_then(Value::as_str) {
        Some("repitch") => StretchMode::Repitch,
        Some("rhythmic") => StretchMode::Rhythmic,
        Some("tonal") => StretchMode::Tonal,
        Some("texture") => StretchMode::Texture,
        None | Some("smooth") => StretchMode::Smooth,
        Some("high_quality") => StretchMode::HighQuality,
        Some(other) => return Err(format!("{at}.stretch: {other} is not a stretch mode").into()),
    };
    let mut warp_markers = Vec::new();
    for (i, v) in list(a, "warp_markers", at)?.iter().enumerate() {
        let at = format!("{at}.warp_markers[{i}]");
        let m = object(v, &at)?;
        warp_markers.push(WarpMarker { time: float(m, "time", &at)?, audio_time: float(m, "audio_time", &at)? });
    }
    let absolute_file = match a.get("absolute_file") {
        None | Some(Value::Null) => None,
        Some(_) => Some(required_text(a, "absolute_file", at)?),
    };
    Ok(AudioRegion {
        file: required_text(a, "file", at)?,
        absolute_file,
        sample_rate: optional_whole(a, "sample_rate", at)?,
        channels: optional_whole(a, "channels", at)?,
        duration_seconds: optional_float(a, "duration_seconds", at)?,
        warped: flag(a, "warped", false, at)?,
        stretch,
        warp_markers,
        gain: float_or(a, "gain", 1.0, at)?,
        transpose: float_or(a, "transpose", 0.0, at)?,
        stream: optional_whole(a, "stream", at)?,
    })
}

fn read_clip(value: &Value, at: &str) -> Result<Clip, ProjectFileError> {
    let c = object(value, at)?;
    let loop_range = match c.get("loop") {
        None | Some(Value::Null) => None,
        Some(Value::Array(pair)) if pair.len() == 2 => {
            let at_point = |i: usize| pair.get(i).and_then(Value::as_f64).filter(|v| v.is_finite());
            match (at_point(0), at_point(1)) {
                (Some(start), Some(end)) if start <= end => Some(LoopRange { start, end }),
                _ => return Err(format!("{at}.loop: expected [start, end] with start before end").into()),
            }
        }
        Some(_) => return Err(format!("{at}.loop: expected [start, end]").into()),
    };
    Ok(Clip {
        id: ClipId(read_id(c, at)?),
        content: ContentId(whole(c, "content", at)?),
        start: float(c, "start", at)?,
        length: float(c, "length", at)?,
        offset: float_or(c, "offset", 0.0, at)?,
        loop_range,
        enabled: flag(c, "enabled", true, at)?,
        fade_in: float_or(c, "fade_in", 0.0, at)?,
        fade_out: float_or(c, "fade_out", 0.0, at)?,
        fade_in_curve: fade_curve(c, "fade_in", at)?,
        fade_out_curve: fade_curve(c, "fade_out", at)?,
        follow_action: read_follow(c, at)?,
        launch: read_launch(c, at)?,
        color: read_color(c.get("color"), at)?,
        extensions: extensions(c, at)?,
    })
}

/// A `color` field, `#RRGGBB`; missing or null is none.
fn read_color(value: Option<&Value>, at: &str) -> Result<Option<Rgb>, ProjectFileError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(hex)) => {
            let digits = hex.strip_prefix('#').unwrap_or(hex);
            let parsed = (digits.len() == 6 && digits.chars().all(|c| c.is_ascii_hexdigit()))
                .then(|| u32::from_str_radix(digits, 16).ok())
                .flatten();
            let rgb = parsed.ok_or_else(|| ProjectFileError(format!("{at}.color: expected #RRGGBB")))?;
            Ok(Some(Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)))
        }
        Some(_) => Err(format!("{at}.color: expected #RRGGBB").into()),
    }
}

/// A clip's follow action, `follow`: off when missing (files written before follow actions).
fn read_follow(c: &Obj, at: &str) -> Result<FollowAction, ProjectFileError> {
    let at = format!("{at}.follow");
    let f = match c.get("follow") {
        None | Some(Value::Null) => return Ok(FollowAction::default()),
        Some(value) => object(value, &at)?,
    };
    let default = FollowAction::default();
    let kind = |key: &str, default: FollowKind| -> Result<FollowKind, ProjectFileError> {
        match f.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(Value::String(name)) => FollowKind::from_name(name)
                .ok_or_else(|| ProjectFileError(format!("{at}.{key}: unknown follow action {name:?}"))),
            Some(_) => Err(format!("{at}.{key}: expected a follow action name").into()),
        }
    };
    let chance = |key: &str, default: u32| -> Result<u32, ProjectFileError> {
        match f.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(value) => value
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .filter(|v| *v <= FollowAction::MAX_CHANCE)
                .ok_or_else(|| ProjectFileError(format!("{at}.{key}: expected a whole number from 0 to 100"))),
        }
    };
    let follow = FollowAction {
        enabled: flag(f, "enabled", default.enabled, &at)?,
        time: float_or(f, "time", default.time, &at)?,
        linked: flag(f, "linked", default.linked, &at)?,
        action_a: kind("a", default.action_a)?,
        action_b: kind("b", default.action_b)?,
        chance_a: chance("chance_a", default.chance_a)?,
        chance_b: chance("chance_b", default.chance_b)?,
    };
    if !follow.is_valid() {
        return Err(format!("{at}.time: expected more than 0 and at most {} beats", FollowAction::MAX_TIME).into());
    }
    Ok(follow)
}

/// A launch mapping: `source` (`key:Q`, `note:1:60`), `target` (slot, scene or stop) and the
/// `track` and 1-based `scene` the target needs. The track need not exist: a mapping whose track
/// is gone is kept, unused.
fn read_mapping(value: &Value, at: &str) -> Result<LaunchMapping, ProjectFileError> {
    let m = object(value, at)?;
    let name = match m.get("source") {
        Some(Value::String(name)) => name.as_str(),
        _ => return Err(ProjectFileError(format!("{at}.source: expected a key or a note"))),
    };
    let source = MappingSource::from_name(name)
        .ok_or_else(|| ProjectFileError(format!("{at}.source: unknown source {name:?}")))?;
    let scene = || -> Result<usize, ProjectFileError> {
        let number: usize = whole(m, "scene", at)?;
        number.checked_sub(1).ok_or_else(|| ProjectFileError(format!("{at}.scene: scenes count from 1")))
    };
    let track = || whole(m, "track", at).map(TrackId);
    let target = match m.get("target").and_then(Value::as_str) {
        Some("slot") => MappingTarget::Slot { track: track()?, scene: scene()? },
        Some("scene") => MappingTarget::Scene { scene: scene()? },
        Some("stop") => MappingTarget::Stop { track: track()? },
        _ => return Err(ProjectFileError(format!("{at}.target: expected slot, scene or stop"))),
    };
    Ok(LaunchMapping { source, target })
}

/// A clip's launch settings, `launch`: the defaults when missing (files written before launch
/// settings), and for each of its keys that is missing.
fn read_launch(c: &Obj, at: &str) -> Result<LaunchSettings, ProjectFileError> {
    let at = format!("{at}.launch");
    let l = match c.get("launch") {
        None | Some(Value::Null) => return Ok(LaunchSettings::default()),
        Some(value) => object(value, &at)?,
    };
    let name = |key: &str| match l.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(name)) => Ok(Some(name.as_str())),
        Some(_) => Err(ProjectFileError(format!("{at}.{key}: expected a name"))),
    };
    let mode = match name("mode")? {
        None => LaunchMode::default(),
        Some(text) => LaunchMode::from_name(text)
            .ok_or_else(|| ProjectFileError(format!("{at}.mode: unknown launch mode {text:?}")))?,
    };
    let quantization = match name("quantization")? {
        None => LaunchQuantization::default(),
        Some(text) => LaunchQuantization::from_name(text)
            .ok_or_else(|| ProjectFileError(format!("{at}.quantization: unknown launch quantisation {text:?}")))?,
    };
    Ok(LaunchSettings { mode, legato: flag(l, "legato", false, &at)?, quantization })
}

/// A scale, `key` in `o`: `None` when missing (files written before scales), else
/// `{ "root", "mode" }` with a root 0 to 11 and a mode the model knows; anything else is an error
/// naming where it is.
fn read_scale(o: &Obj, key: &str, at: &str) -> Result<Option<Scale>, ProjectFileError> {
    let at = format!("{at}.{key}");
    let s = match o.get(key) {
        None | Some(Value::Null) => return Ok(None),
        Some(value) => object(value, &at)?,
    };
    let root = match s.get("root").and_then(Value::as_u64) {
        Some(root) if root <= 11 => root as u8,
        Some(root) => return Err(format!("{at}.root: expected 0 to 11, got {root}").into()),
        None => return Err(format!("{at}.root: expected a whole number 0 to 11").into()),
    };
    let mode = match s.get("mode") {
        Some(Value::String(name)) => ScaleMode::from_name(name)
            .ok_or_else(|| ProjectFileError(format!("{at}.mode: unknown scale mode {name:?}")))?,
        _ => return Err(format!("{at}.mode: expected a scale mode name").into()),
    };
    Scale::new(root, mode).map(Some).ok_or_else(|| ProjectFileError(format!("{at}.root: expected 0 to 11")))
}

/// A fade's shape and curve, `<fade>_shape` and `<fade>_curve`: linear and 0 when missing (files
/// written before fade shapes).
fn fade_curve(c: &Obj, fade: &str, at: &str) -> Result<FadeCurve, ProjectFileError> {
    let shape_key = format!("{fade}_shape");
    let shape = match c.get(&shape_key) {
        None | Some(Value::Null) => FadeShape::default(),
        Some(Value::String(name)) => FadeShape::from_name(name)
            .ok_or_else(|| ProjectFileError(format!("{at}.{shape_key}: unknown fade shape {name:?}")))?,
        Some(_) => return Err(format!("{at}.{shape_key}: expected a fade shape name").into()),
    };
    let curve_key = format!("{fade}_curve");
    let curve = float_or(c, &curve_key, 0.0, at)?;
    if !(-1.0..=1.0).contains(&curve) {
        return Err(format!("{at}.{curve_key}: expected -1 to 1, got {curve}").into());
    }
    Ok(FadeCurve { shape, curve })
}

#[cfg(test)]
mod tests;
