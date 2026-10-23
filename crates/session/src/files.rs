// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Adding tracks, and placing samples and MIDI files as clips (`docs/spec/browser.md` › Files):
//! what the browser's drops and agents do. Each command is one undo step. A sample's decoded
//! audio joins the session's pool and reaches a playing engine with the next graph (ADR 0011).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use suhina_device_api::Role;
use suhina_engine::AudioData;
use suhina_media::info::{FileKind, Tempo, TempoSource, kind_of};
use suhina_model::{
    ArrangementLane, AudioRegion, Channel, ChannelId, ChannelRole, Clip, ClipContent, ContentId, ContentKinds,
    ContentLane, LaneData, LauncherColumn, LoopRange, Note, Project, Scene, Slot, StretchMode, Track, TrackId,
    WarpMarker,
};

use crate::commands::{make_room, number, track_at};
use crate::{CommandError, Session};

/// Files one command places at most.
pub const MAX_FILES: usize = 256;
/// Scenes a drop may add up to.
const MAX_SCENES: usize = 1024;
/// Tracks a project may have before adding refuses.
pub(crate) const MAX_TRACKS: usize = 1024;

/// What a new track plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Audio,
    Midi,
}

impl TrackKind {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "audio" => Some(Self::Audio),
            "midi" | "notes" => Some(Self::Midi),
            _ => None,
        }
    }

    fn contents(self) -> ContentKinds {
        match self {
            Self::Audio => ContentKinds { notes: false, audio: true },
            Self::Midi => ContentKinds { notes: true, audio: false },
        }
    }

    fn default_name(self) -> &'static str {
        match self {
            Self::Audio => "Audio",
            Self::Midi => "MIDI",
        }
    }

    fn of(track: &Track) -> Option<Self> {
        if track.content.audio {
            Some(Self::Audio)
        } else if track.content.notes {
            Some(Self::Midi)
        } else {
            None
        }
    }
}

/// Where new tracks go when no track is named: after the last track that is neither a return
/// nor in a group (before the returns).
fn end_of_regular_tracks(p: &Project) -> usize {
    let is_return = |t: &Track| t.channel.and_then(|c| p.channel(c)).is_some_and(|c| c.role == ChannelRole::Effect);
    p.tracks.iter().rposition(|t| !is_return(t)).map_or(0, |i| i + 1)
}

/// Adds a track of `kind` named `name` with its channel, clip slots for every scene and an
/// arrangement lane, right after `after` (in its group) or at the end of the regular tracks.
pub(crate) fn insert_track(
    p: &mut Project,
    kind: TrackKind,
    name: &str,
    after: Option<TrackId>,
) -> Result<TrackId, CommandError> {
    if p.tracks.len() >= MAX_TRACKS {
        return Err(format!("a project holds at most {MAX_TRACKS} tracks").into());
    }
    let id = TrackId(p.tracks.iter().map(|t| t.id.0).max().unwrap_or(0).checked_add(1).ok_or("too many tracks")?);
    let channel =
        ChannelId(p.channels.iter().map(|c| c.id.0).max().unwrap_or(0).checked_add(1).ok_or("too many channels")?);
    let (at, parent) = match after {
        Some(after) => {
            let index = p.tracks.iter().position(|t| t.id == after).ok_or("the track to add after is missing")?;
            let parent = p.tracks.get(index).and_then(|t| t.parent);
            // After the track and anything in it (a group's tracks).
            let mut end = index + 1;
            while let Some(next) = p.tracks.get(end) {
                if within(p, next.id, after) {
                    end += 1;
                } else {
                    break;
                }
            }
            (end, parent)
        }
        None => (end_of_regular_tracks(p), None),
    };
    // Into its group, or into the main channel like the other tracks.
    let main = p.channels.iter().find(|c| c.role == ChannelRole::Master).map(|c| c.id);
    let output = match parent {
        Some(group) => p.track(group).and_then(|t| t.channel),
        None => main,
    };
    p.channels.push(Channel {
        id: channel,
        name: name.to_string(),
        role: ChannelRole::Regular,
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
    let track = Track {
        id,
        name: name.to_string(),
        color: None,
        content: kind.contents(),
        channel: Some(channel),
        parent,
        midi_input: Default::default(),
        audio_input: Default::default(),
        extensions: Default::default(),
    };
    let at = at.min(p.tracks.len());
    p.tracks.insert(at, track);
    let slots = p.launcher.scenes.len();
    p.launcher.columns.push(LauncherColumn { track: id, slots: vec![Slot { clip: None, has_stop: true }; slots] });
    let lane_id = p.new_lane_id()?;
    p.arrangement.lanes.push(ArrangementLane {
        id: lane_id,
        name: name.to_string(),
        track: Some(id),
        clips: Vec::new(),
    });
    Ok(id)
}

/// Whether `track` is inside the group `group` (at any depth).
fn within(p: &Project, track: TrackId, group: TrackId) -> bool {
    let mut current = p.track(track).and_then(|t| t.parent);
    let mut steps = 0;
    while let Some(parent) = current {
        if parent == group {
            return true;
        }
        steps += 1;
        if steps > p.tracks.len() {
            return false;
        }
        current = p.track(parent).and_then(|t| t.parent);
    }
    false
}

/// "Audio 3": the next number among tracks of that kind.
fn numbered_name(p: &Project, kind: TrackKind) -> String {
    let count = p.tracks.iter().filter(|t| TrackKind::of(t) == Some(kind)).count();
    format!("{} {}", kind.default_name(), count + 1)
}

fn text<'a>(params: &'a Value, name: &str) -> Result<Option<&'a str>, CommandError> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => Ok(Some(
            v.as_str().map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| format!("{name} must be a text"))?,
        )),
    }
}

fn flag(params: &Value, name: &str) -> Result<bool, CommandError> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(false),
        Some(v) => v.as_bool().ok_or_else(|| format!("{name} must be true or false").into()),
    }
}

/// `track.add`: a new audio or MIDI track, with a device when one is named.
pub(crate) fn add_track(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let device = text(params, "device")?;
    let preset = text(params, "preset")?;
    let sd = text(params, "sd")?;
    // What the device is decides the kind when none is given.
    let device_role = match (device, preset, sd) {
        (_, Some(_), _) => Some(Role::Instrument),
        (_, _, Some(path)) => {
            let graph = suhina_device_sdk::DeviceGraph::read_file(Path::new(path)).map_err(|e| e.to_string())?;
            Some(match graph.role {
                suhina_device_sdk::DeviceRole::Effect => Role::Effect,
                suhina_device_sdk::DeviceRole::Instrument | suhina_device_sdk::DeviceRole::MidiEffect => {
                    Role::Instrument
                }
            })
        }
        (Some(id), None, None) => {
            match suhina_device_registry::info(id) {
                Some(info) => Some(info.role),
                None => {
                    let env = session
                        .plugins
                        .env()
                        .ok_or_else(|| format!("no built-in effect, instrument or plug-in {id:?}"))?;
                    let plugin =
                    env.catalog.find(id).or_else(|| env.catalog.plugins().find(|p| p.spec() == id)).ok_or_else(
                        || format!("no built-in effect, instrument or plug-in {id:?}; device.list and plugin.list list them"),
                    )?;
                    Some(if crate::is_instrument(plugin) { Role::Instrument } else { Role::Effect })
                }
            }
        }
        (None, None, None) => None,
    };
    let kind = match text(params, "kind")? {
        Some(k) => TrackKind::parse(k).ok_or("kind must be audio or midi")?,
        None => match device_role {
            Some(Role::Instrument | Role::MidiEffect) => TrackKind::Midi,
            Some(Role::Effect) => TrackKind::Audio,
            None => return Err("kind must be audio or midi (or name a device)".into()),
        },
    };
    if kind == TrackKind::Audio && device_role == Some(Role::Instrument) {
        return Err("an instrument goes on a MIDI track".into());
    }
    if kind == TrackKind::Audio && device_role == Some(Role::MidiEffect) {
        return Err("a MIDI effect goes on a MIDI track".into());
    }
    let after = match params.get("after") {
        None | Some(Value::Null) => None,
        Some(_) => Some(track_at(&session.project, params, "after")?),
    };
    let name = match text(params, "name")? {
        Some(n) => n.to_string(),
        None => match (device, preset, sd) {
            (_, Some(path), _) | (_, _, Some(path)) => file_stem(Path::new(path)),
            (Some(id), None, None) => suhina_device_registry::info(id).map_or_else(
                || session.plugins.env().and_then(|e| e.catalog.find(id).map(|p| p.name.clone())).unwrap_or_default(),
                |i| i.name.to_string(),
            ),
            (None, None, None) => numbered_name(&session.project, kind),
        },
    };
    let name = if name.trim().is_empty() { numbered_name(&session.project, kind) } else { name };
    let track = insert_track(&mut session.project, kind, &name, after)?;
    let mut added = Value::Null;
    if device.is_some() || preset.is_some() || sd.is_some() {
        let mut device_params = json!({ "track": track.0 });
        if let Some(path) = preset {
            device_params["preset"] = json!(path);
            if let Some(voice) = params.get("voice") {
                device_params["voice"] = voice.clone();
            }
        }
        if let Some(path) = sd {
            device_params["path"] = json!(path);
        }
        added = match (device, sd) {
            (_, Some(_)) => crate::devices::add_sd(session, &device_params)?,
            (Some(id), None) if suhina_device_registry::info(id).is_some() => {
                device_params["id"] = json!(id);
                crate::devices::add(session, &device_params)?
            }
            (Some(id), None) if preset.is_none() => {
                device_params["plugin"] = json!(id);
                crate::devices::add_plugin(session, &device_params)?
            }
            _ => crate::devices::add(session, &device_params)?,
        };
    }
    Ok(
        json!({ "track": track.0, "name": name, "kind": match kind { TrackKind::Audio => "audio", TrackKind::Midi => "midi" }, "device": added }),
    )
}

/// A file's name without its extension.
fn file_stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// One clip to place, made from a file.
pub(crate) struct Source {
    pub(crate) kind: TrackKind,
    pub(crate) name: String,
    /// Content length in beats (the clip's length too).
    pub(crate) length: f64,
    pub(crate) lane: LaneData,
    /// Whether the clip loops in a slot.
    pub(crate) loops: bool,
}

/// `clip.addFiles`: samples and MIDI files as clips on a track, or on new tracks.
pub(crate) fn add_files(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let paths: Vec<String> = params
        .get("paths")
        .and_then(Value::as_array)
        .ok_or("paths must be a list of file paths")?
        .iter()
        .map(|v| v.as_str().map(str::to_string).ok_or("paths must be a list of file paths"))
        .collect::<Result<_, _>>()?;
    if paths.is_empty() {
        return Err("paths is empty".into());
    }
    let start = match params.get("start") {
        None | Some(Value::Null) => None,
        Some(_) => Some(number(params, "start")?),
    };
    let scene = match params.get("scene") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .and_then(|n| n.checked_sub(1))
                .ok_or("scene must be a whole number from 1")?,
        ),
    };
    match (start, scene) {
        (Some(_), Some(_)) | (None, None) => {
            return Err("give either start (a beat in the arrangement) or scene (a row of clip slots)".into());
        }
        (Some(s), None) if !(0.0..=suhina_engine::MAX_START_BEAT).contains(&s) => {
            return Err(format!("start must be from 0 to {}", suhina_engine::MAX_START_BEAT).into());
        }
        (None, Some(s)) if s >= MAX_SCENES => return Err(format!("scene must be at most {MAX_SCENES}").into()),
        _ => {}
    }
    let separate = flag(params, "separate")?;
    let stack = flag(params, "stack")?;
    let stretch = match text(params, "warp")? {
        None => StretchMode::Rhythmic,
        Some(mode) => {
            stretch_mode(mode).ok_or("warp must be repitch, rhythmic, tonal, texture, smooth or high-quality")?
        }
    };
    let target = match params.get("track") {
        None | Some(Value::Null) => None,
        Some(_) => Some(track_at(&session.project, params, "track")?),
    };
    // A Stems file may be dropped on a group (its tracks go into it); other files need a track
    // that holds clips, or go on new tracks when the target is a group.
    let stems_target = target;
    let (target, target_kind) = match target {
        Some(t) => {
            let track = session.project.track(t).ok_or("the track is missing")?;
            match TrackKind::of(track) {
                Some(kind) => (Some(t), Some(kind)),
                None if is_group(&session.project, t) => (None, None),
                None => return Err(format!("track {:?} holds no clips (a return)", track.name).into()),
            }
        }
        None => (None, None),
    };

    // Read every file; the ones that cannot be read are skipped with a reason.
    let tempo = session.project.transport.tempo_bpm;
    let bar = bar_beats(&session.project);
    let mut sources: Vec<Source> = Vec::new();
    let mut skipped: Vec<Value> = Vec::new();
    let mut stems_paths: Vec<String> = Vec::new();
    for path in paths.iter().take(MAX_FILES) {
        if kind_of(Path::new(path)) == Some(FileKind::Stems) {
            stems_paths.push(path.clone());
            continue;
        }
        match read_sources(session, Path::new(path), tempo, bar, stretch) {
            Ok(mut list) => sources.append(&mut list),
            Err(reason) => skipped.push(json!({ "path": path, "reason": reason })),
        }
    }
    for path in paths.iter().skip(MAX_FILES) {
        skipped.push(json!({ "path": path, "reason": format!("more than {MAX_FILES} files in one go") }));
    }
    if sources.is_empty() && stems_paths.is_empty() {
        let reasons: Vec<String> = skipped
            .iter()
            .map(|s| format!("{}: {}", s["path"].as_str().unwrap_or(""), s["reason"].as_str().unwrap_or("")))
            .collect();
        return Err(format!("nothing could be placed ({})", reasons.join("; ")).into());
    }

    // Which track each clip goes on.
    let p = &mut session.project;
    let mut new_tracks: Vec<TrackId> = Vec::new();
    let mut groups: Vec<(TrackId, Vec<Source>)> = Vec::new();
    let mut last = target;
    let (fits, rest): (Vec<Source>, Vec<Source>) =
        sources.into_iter().partition(|s| target_kind.is_some_and(|k| k == s.kind));
    if let Some(target) = target
        && !fits.is_empty()
    {
        if separate {
            let mut iter = fits.into_iter();
            if let Some(first) = iter.next() {
                groups.push((target, vec![first]));
            }
            for source in iter {
                let id = insert_track(p, source.kind, &source.name, last)?;
                new_tracks.push(id);
                last = Some(id);
                groups.push((id, vec![source]));
            }
        } else {
            groups.push((target, fits));
        }
    }
    // The rest go on new tracks: one per kind, or one each.
    if separate {
        for source in rest {
            let id = insert_track(p, source.kind, &source.name, last)?;
            new_tracks.push(id);
            if target.is_some() {
                last = Some(id);
            }
            groups.push((id, vec![source]));
        }
    } else {
        let (audio, midi): (Vec<Source>, Vec<Source>) = rest.into_iter().partition(|s| s.kind == TrackKind::Audio);
        for (kind, of_kind) in [(TrackKind::Audio, audio), (TrackKind::Midi, midi)] {
            if of_kind.is_empty() {
                continue;
            }
            let name = match of_kind.as_slice() {
                [one] => one.name.clone(),
                _ => numbered_name(p, kind),
            };
            let id = insert_track(p, kind, &name, last)?;
            new_tracks.push(id);
            if target.is_some() {
                last = Some(id);
            }
            groups.push((id, of_kind));
        }
    }

    // Place them.
    let mut placed: Vec<Value> = Vec::new();
    for (track, list) in groups {
        let mut at = start.unwrap_or(0.0);
        for (i, source) in list.into_iter().enumerate() {
            let row = scene.map(|first| first + i);
            if row.is_some_and(|r| r >= MAX_SCENES) {
                skipped.push(json!({ "path": source.name, "reason": format!("past scene {MAX_SCENES}") }));
                continue;
            }
            let length = source.length;
            placed.push(place_clip(p, track, source, row, at, stack)?);
            at += length;
        }
    }

    // Stems files: a group of tracks each.
    let mut stem_groups: Vec<u32> = Vec::new();
    for path in stems_paths {
        match place_stems(session, &path, stems_target, start, scene, stretch, stack) {
            Ok(done) => {
                new_tracks.extend(done.tracks.iter().copied());
                stem_groups.push(done.group.0);
                placed.extend(done.clips);
            }
            Err(reason) => skipped.push(json!({ "path": path, "reason": reason.0 })),
        }
    }
    if placed.is_empty() {
        let reasons: Vec<String> = skipped
            .iter()
            .map(|s| format!("{}: {}", s["path"].as_str().unwrap_or(""), s["reason"].as_str().unwrap_or("")))
            .collect();
        return Err(format!("nothing could be placed ({})", reasons.join("; ")).into());
    }
    let tracks: Vec<u32> = new_tracks.iter().map(|t| t.0).collect();
    Ok(json!({ "clips": placed, "tracks": tracks, "groups": stem_groups, "skipped": skipped }))
}

/// The name of a clip made empty with `clip.create`, by kind.
/// The name an empty MIDI clip takes until it is renamed.
pub const EMPTY_CLIP_NAME: &str = "MIDI clip";

/// The name an empty audio clip takes until it is renamed: it has no file yet, so it plays
/// silence until a sample is dropped on its slot or lane.
pub const EMPTY_AUDIO_CLIP_NAME: &str = "Audio clip";

/// `clip.create`: an empty clip on a track that holds clips — MIDI on a notes track, audio with
/// no file yet on an audio track — in an empty clip slot or at a beat in the arrangement, one bar
/// long unless `length` says otherwise.
pub(crate) fn create_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &session.project;
    let track = track_at(p, params, "track")?;
    let target = p.track(track).ok_or("the track is missing")?;
    // The clip's kind follows the track, notes first: a track that takes both got a MIDI clip
    // before, so it still does; audio tracks are the ones that play audio and not notes.
    let kind = if target.content.notes {
        TrackKind::Midi
    } else if target.content.audio {
        TrackKind::Audio
    } else {
        return Err(format!("track {:?} holds no clips (a group or a return)", target.name).into());
    };
    let length = match params.get("length") {
        None | Some(Value::Null) => bar_beats(p),
        Some(_) => number(params, "length")?,
    };
    if !(length > 0.0 && length <= suhina_engine::MAX_START_BEAT) {
        return Err(format!("length must be more than 0 and at most {} beats", suhina_engine::MAX_START_BEAT).into());
    }
    let source = match kind {
        TrackKind::Midi => Source {
            kind: TrackKind::Midi,
            name: EMPTY_CLIP_NAME.into(),
            length,
            lane: LaneData::Notes(Vec::new()),
            loops: true,
        },
        TrackKind::Audio => Source {
            kind: TrackKind::Audio,
            name: EMPTY_AUDIO_CLIP_NAME.into(),
            length,
            // No file yet, so nothing to play: a sample dropped on the slot takes the clip's
            // place.
            lane: LaneData::Audio(AudioRegion {
                file: String::new(),
                absolute_file: None,
                sample_rate: None,
                channels: None,
                duration_seconds: None,
                warped: false,
                stretch: StretchMode::default(),
                warp_markers: Vec::new(),
                gain: 1.0,
                transpose: 0.0,
                stream: None,
            }),
            loops: true,
        },
    };
    let start = match params.get("start") {
        None | Some(Value::Null) => None,
        Some(_) => Some(number(params, "start")?),
    };
    let scene = match params.get("scene") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .and_then(|n| n.checked_sub(1))
                .ok_or("scene must be a whole number from 1")?,
        ),
    };
    let stack = flag(params, "stack")?;
    match (start, scene) {
        (Some(_), Some(_)) | (None, None) => {
            return Err("give either start (a beat in the arrangement) or scene (a row of clip slots)".into());
        }
        (Some(s), None) if !(0.0..=suhina_engine::MAX_START_BEAT).contains(&s) => {
            return Err(format!("start must be from 0 to {}", suhina_engine::MAX_START_BEAT).into());
        }
        (None, Some(row)) => {
            let scenes = p.launcher.scenes.len();
            let column = p.launcher.columns.iter().find(|c| c.track == track).ok_or("the track has no clip slots")?;
            match column.slots.get(row) {
                _ if row >= scenes => return Err(format!("scene must be from 1 to {scenes}").into()),
                Some(slot) if slot.clip.is_some() => {
                    return Err(format!("scene {} of track {:?} already has a clip", row + 1, target.name).into());
                }
                Some(_) => {}
                None => return Err("the slot is missing".into()),
            }
        }
        _ => {}
    }
    let at = start.unwrap_or(0.0);
    let mut done = place_clip(&mut session.project, track, source, scene, at, stack)?;
    // The result names the clip by its id; an arrangement clip also by where it is on the track.
    let id = done["clip"].as_u64().and_then(|n| u32::try_from(n).ok()).map(suhina_model::ClipId);
    if scene.is_none()
        && let Some(place) = id.and_then(|id| session.project.clip_place(id))
        && let suhina_model::ClipPlace::Lane { index, .. } = place
    {
        done["index"] = json!(index);
    }
    Ok(done)
}

/// Puts one source as a clip on `track`: in slot `row` of the Session, or at beat `at` in the
/// Arrangement (replacing what it covers unless `stack`).
pub(crate) fn place_clip(
    p: &mut Project,
    track: TrackId,
    source: Source,
    row: Option<usize>,
    at: f64,
    stack: bool,
) -> Result<Value, CommandError> {
    let id = ContentId(p.contents.iter().map(|c| c.id.0).max().unwrap_or(0).checked_add(1).ok_or("too many clips")?);
    let length = source.length;
    // A new MIDI clip takes the project's default scale (ADR 0001 › Amendment: clip scales).
    let scale = if matches!(source.lane, LaneData::Notes(_)) { p.default_scale } else { None };
    p.contents.push(ClipContent {
        id,
        name: source.name.clone(),
        length,
        lanes: vec![ContentLane { target: None, data: source.lane }],
        extensions: Default::default(),
        scale,
    });
    let clip_id = p.new_clip_id()?;
    let mut clip = Clip {
        id: clip_id,
        content: id,
        start: 0.0,
        length,
        offset: 0.0,
        loop_range: None,
        enabled: true,
        fade_in: 0.0,
        fade_out: 0.0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        follow_action: Default::default(),
        launch: Default::default(),
        color: None,
        extensions: Default::default(),
    };
    match row {
        Some(row) => {
            while p.launcher.scenes.len() <= row {
                let scene_id = p.new_scene_id()?;
                p.launcher.scenes.push(Scene { id: scene_id, ..Scene::default() });
                for column in &mut p.launcher.columns {
                    column.slots.push(Slot { clip: None, has_stop: true });
                }
            }
            if source.loops {
                clip.loop_range = Some(LoopRange { start: 0.0, end: length });
            }
            let column =
                p.launcher.columns.iter_mut().find(|c| c.track == track).ok_or("the track has no clip slots")?;
            let slot = column.slots.get_mut(row).ok_or("the slot is missing")?;
            slot.clip = Some(clip);
            Ok(json!({ "clip": clip_id.0, "track": track.0, "scene": row + 1, "name": source.name, "length": length }))
        }
        None => {
            clip.start = at;
            let lane = match p.arrangement.lanes.iter().position(|l| l.track == Some(track)) {
                Some(lane) => lane,
                None => {
                    let name = p.track(track).map(|t| t.name.clone()).unwrap_or_default();
                    let lane_id = p.new_lane_id()?;
                    p.arrangement.lanes.push(ArrangementLane {
                        id: lane_id,
                        name,
                        track: Some(track),
                        clips: Vec::new(),
                    });
                    p.arrangement.lanes.len() - 1
                }
            };
            let lane = p.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?;
            if !stack {
                make_room(&mut lane.clips, at, at + length);
            }
            lane.clips.push(clip);
            lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
            Ok(json!({ "clip": clip_id.0, "track": track.0, "start": at, "name": source.name, "length": length }))
        }
    }
}

fn stretch_mode(text: &str) -> Option<StretchMode> {
    Some(match text {
        "repitch" => StretchMode::Repitch,
        "rhythmic" | "beats" => StretchMode::Rhythmic,
        "tonal" => StretchMode::Tonal,
        "texture" => StretchMode::Texture,
        "smooth" => StretchMode::Smooth,
        "high-quality" | "pro" => StretchMode::HighQuality,
        _ => return None,
    })
}

fn bar_beats(p: &Project) -> f64 {
    let ts = p.transport.time_signature;
    if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) }
}

/// The clips one file gives: one for a sample, one per MIDI track for a MIDI file.
fn read_sources(
    session: &mut Session,
    path: &Path,
    tempo: f64,
    bar: f64,
    stretch: StretchMode,
) -> Result<Vec<Source>, String> {
    let absolute = absolute(path);
    if !absolute.is_file() {
        return Err("not found".into());
    }
    let name = file_stem(&absolute);
    match kind_of(&absolute) {
        Some(FileKind::Sample) => {
            let data = session.decoded_audio(&absolute)?;
            let duration = data.frames() as f64 / f64::from(data.sample_rate.max(1));
            if !(duration.is_finite() && duration > 0.0) {
                return Err("no audio".into());
            }
            let tempo_of_file = file_tempo(&absolute, &data);
            let key = absolute.display().to_string();
            let (warped, beats, markers) = match tempo_of_file {
                Some(t) => {
                    let beats = duration * t.bpm / 60.0;
                    (
                        true,
                        beats,
                        vec![
                            WarpMarker { time: 0.0, audio_time: 0.0 },
                            WarpMarker { time: beats, audio_time: duration },
                        ],
                    )
                }
                None => (false, duration * tempo / 60.0, Vec::new()),
            };
            if !(beats.is_finite() && beats > 0.0) {
                return Err("its length in beats is not usable".into());
            }
            let region = AudioRegion {
                file: key.clone(),
                absolute_file: Some(key),
                sample_rate: Some(data.sample_rate),
                channels: Some(data.channels),
                duration_seconds: Some(duration),
                warped,
                stretch: if warped { stretch } else { StretchMode::default() },
                warp_markers: markers,
                gain: 1.0,
                transpose: 0.0,
                stream: None,
            };
            Ok(vec![Source {
                kind: TrackKind::Audio,
                name,
                length: beats,
                lane: LaneData::Audio(region),
                // A slot loops a sample over the whole file whether or not its tempo is known
                // (`docs/spec/browser.md` › Warping); `clip.setLoop` turns it off.
                loops: true,
            }])
        }
        Some(FileKind::Midi) => {
            let midi = suhina_media::midi::read_file(&absolute)?;
            if midi.tracks.is_empty() {
                return Err("no notes".into());
            }
            Ok(midi
                .tracks
                .into_iter()
                .map(|track| {
                    let end = track.end();
                    let length = ((end / bar).ceil().max(1.0)) * bar;
                    let clip_name = match &track.name {
                        Some(n) => format!("{name} – {n}"),
                        None => name.clone(),
                    };
                    let notes: Vec<Note> = track.notes;
                    Source { kind: TrackKind::Midi, name: clip_name, length, lane: LaneData::Notes(notes), loops: true }
                })
                .collect())
        }
        Some(FileKind::Stems) => Err("a Stems file becomes a group of tracks".into()),
        Some(FileKind::Preset) => Err("a preset goes on a track as a device".into()),
        Some(FileKind::Project) => Err("a project is opened, not placed".into()),
        Some(FileKind::Effect) => Err("an .sd AudioEffect goes on a track as a device".into()),
        None => Err("not a sample or a MIDI file".into()),
    }
}

/// A sample's tempo: its metadata or name, else detected (`docs/spec/browser.md` › Tempo).
pub fn file_tempo(path: &Path, data: &AudioData) -> Option<Tempo> {
    let from_file = suhina_media::info::sample_info(path).ok().and_then(|i| i.tempo);
    from_file.or_else(|| suhina_media::tempo::detect(data).map(|bpm| Tempo { bpm, source: TempoSource::Detected }))
}

/// `path` made absolute against the working folder, without touching the disk.
pub(crate) fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Whether `track` is a group track.
fn is_group(p: &Project, track: TrackId) -> bool {
    p.tracks
        .iter()
        .find(|t| t.id == track)
        .and_then(|t| t.channel)
        .and_then(|c| p.channel(c))
        .is_some_and(|c| c.role == ChannelRole::Submix)
}

/// What one Stems file became.
struct StemsPlaced {
    group: TrackId,
    /// The group and its tracks.
    tracks: Vec<TrackId>,
    clips: Vec<Value>,
}

/// Places a Stems file (`docs/spec/stems-format.md`): a group named after the file holding a
/// muted track for the master and one track per stem (named and coloured from the file), each
/// with a clip of its audio track at `start` or in `scene`; the file's mastering compressor and
/// limiter on the group, which sums the stems before them (ADR 0004 › 10). The group goes
/// after `target`, or into it when it is a group, or at the end of the regular tracks. The file
/// is read and decoded before the project changes.
fn place_stems(
    session: &mut Session,
    path: &str,
    target: Option<TrackId>,
    start: Option<f64>,
    scene: Option<usize>,
    stretch: StretchMode,
    stack: bool,
) -> Result<StemsPlaced, CommandError> {
    let absolute = absolute(Path::new(path));
    if !absolute.is_file() {
        return Err("not found".into());
    }
    let info = suhina_media::stems::read_info(&absolute)?;
    let streams = session.decoded_streams(&absolute, info.tracks)?;
    let master = streams.first().ok_or("no master track")?;
    let name = file_stem(&absolute);
    let name = name.strip_suffix(".stem").map(str::to_string).unwrap_or(name);
    let key = absolute.display().to_string();
    // One tempo for all, from the master: the stems stay locked together.
    let tempo = session.project.transport.tempo_bpm;
    let file_bpm = file_tempo(&absolute, master).map(|t| t.bpm).filter(|b| b.is_finite() && *b > 0.0);
    let mut sources = Vec::with_capacity(streams.len());
    for (n, data) in streams.iter().enumerate() {
        let duration = data.frames() as f64 / f64::from(data.sample_rate.max(1));
        let (warped, beats, markers) = match file_bpm {
            Some(bpm) => {
                let beats = duration * bpm / 60.0;
                (
                    true,
                    beats,
                    vec![WarpMarker { time: 0.0, audio_time: 0.0 }, WarpMarker { time: beats, audio_time: duration }],
                )
            }
            None => (false, duration * tempo / 60.0, Vec::new()),
        };
        if !(beats.is_finite() && beats > 0.0) {
            return Err("its length in beats is not usable".into());
        }
        let stem = n.checked_sub(1).and_then(|i| info.stems.get(i));
        let region = AudioRegion {
            file: key.clone(),
            absolute_file: Some(key.clone()),
            sample_rate: Some(data.sample_rate),
            channels: Some(data.channels),
            duration_seconds: Some(duration),
            warped,
            stretch: if warped { stretch } else { StretchMode::default() },
            warp_markers: markers,
            gain: 1.0,
            transpose: 0.0,
            stream: Some(u16::try_from(n).map_err(|_| "too many tracks")?),
        };
        let clip_name = match stem {
            Some(s) => format!("{name} – {}", s.name),
            None => format!("{name} – Master"),
        };
        let track_name = stem.map_or_else(|| "Master".to_string(), |s| s.name.clone());
        let color = stem.and_then(|s| s.color).map(|[r, g, b]| suhina_model::Rgb(r, g, b));
        sources.push((
            track_name,
            color,
            Source {
                kind: TrackKind::Audio,
                name: clip_name,
                length: beats,
                lane: LaneData::Audio(region),
                loops: true,
            },
        ));
    }

    // The tracks, after the target or at the end of a target group.
    let p = &mut session.project;
    let into = target.filter(|&t| is_group(p, t));
    let mut after = match into {
        Some(group) => Some(p.tracks.iter().rev().find(|t| within(p, t.id, group)).map_or(group, |t| t.id)),
        None => target,
    };
    let mut tracks = Vec::with_capacity(sources.len());
    for (track_name, color, _) in &sources {
        let id = insert_track(p, TrackKind::Audio, track_name, after)?;
        if let Some(group) = into {
            let group_channel = p.track(group).and_then(|t| t.channel);
            let channel = p.track_mut(id).and_then(|t| {
                t.parent = Some(group);
                t.channel
            });
            if let Some(c) = p.channels.iter_mut().find(|c| Some(c.id) == channel) {
                c.output = group_channel;
            }
        }
        if let Some(track) = p.track_mut(id) {
            track.color = *color;
        }
        tracks.push(id);
        after = Some(id);
    }
    // The master is there to compare with and to play whole; muted, so nothing sounds twice.
    if let Some(channel) = tracks.first().and_then(|&m| p.track(m)).and_then(|t| t.channel)
        && let Some(c) = p.channel_mut(channel)
    {
        c.mute = true;
    }
    let grouped = crate::commands::group_tracks(
        session,
        &json!({ "tracks": tracks.iter().map(|t| t.0).collect::<Vec<_>>(), "name": name }),
    )?;
    let group = TrackId(grouped["track"].as_u64().and_then(|n| u32::try_from(n).ok()).ok_or("the group was not made")?);
    let p = &mut session.project;
    let group_channel = p.track(group).and_then(|t| t.channel);
    if let Some(c) = p.channels.iter_mut().find(|c| Some(c.id) == group_channel) {
        for setting in suhina_media::stems::mastering_devices(&info) {
            let Some(device) = suhina_device_registry::info(setting.id) else { continue };
            c.devices.push(suhina_model::Device {
                name: device.name.to_string(),
                kind: suhina_model::DeviceKind::Builtin(setting.id.to_string()),
                enabled: setting.enabled,
                state: None,
                params: setting.params.iter().map(|(id, v)| suhina_model::DeviceParam::new(*id, *v)).collect(),
                automation: Vec::new(),
                extensions: Default::default(),
            });
        }
    }

    let mut clips = Vec::with_capacity(sources.len());
    for (&track, (_, _, source)) in tracks.iter().zip(sources) {
        clips.push(place_clip(p, track, source, scene, start.unwrap_or(0.0), stack)?);
    }
    let mut all = vec![group];
    all.extend(tracks);
    Ok(StemsPlaced { group, tracks: all, clips })
}
