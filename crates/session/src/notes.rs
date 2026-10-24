// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Editing the notes of a MIDI clip: the `note.*` commands the piano roll, the CLI and MCP run
//! (`docs/spec/ui-clip-editor.md` › Commands).
//!
//! A clip is named by `track` and `index` (an arrangement clip) or `scene` (a launcher slot); its
//! notes by their index in the content's note lane. Edits keep the notes' order, so the indices
//! of notes an edit did not delete stay valid for the next edit.

use serde_json::{Map, Value, json};
use suhina_engine::{MAX_START_BEAT, Quantization};
use suhina_model::{Clip, ClipContent, ContentId, ContentLane, LaneData, Note, Project, TrackId};

use crate::commands::{parse_quantization, track_id};
use crate::{CommandError, Session};

/// The most notes one command may name or add.
pub const MAX_NOTES_PER_COMMAND: usize = 10_000;
/// The most notes a clip may hold.
pub const MAX_NOTES_PER_CLIP: usize = 100_000;
/// The latest start and the longest length of a note, in beats: the engine's furthest start.
pub(crate) const MAX_BEAT: f64 = MAX_START_BEAT;
/// The velocity a note gets when `note.add` gives none, and the release velocity of every new
/// note (the MIDI default, 64).
const DEFAULT_VELOCITY: f64 = 100.0;
const RELEASE_VELOCITY: f32 = 64.0 / 127.0;
/// Beats this close below 0 are 0: a move by the exact distance to the start lands on it.
const EPSILON: f64 = 1e-9;

/// Where a clip is: an arrangement lane and the clip's index in it, or a launcher column and
/// the slot's index in it.
#[derive(Clone, Copy)]
pub(crate) enum ClipAt {
    Arrangement(usize, usize),
    Slot(usize, usize),
}

/// The clip a command names: where it is, its content and the track it plays on.
pub(crate) fn locate(p: &Project, params: &Value) -> Result<(ClipAt, ContentId, TrackId), CommandError> {
    let track = track_id(p, params)?;
    let whole = |key: &str| -> Result<Option<u64>, CommandError> {
        match params.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v.as_u64().map(Some).ok_or_else(|| format!("{key} must be a whole number").into()),
        }
    };
    let (at, clip) = match (whole("index")?, whole("scene")?) {
        (Some(_), Some(_)) => return Err("give index or scene, not both".into()),
        (None, None) => return Err("give index (an arrangement clip) or scene (a launcher clip)".into()),
        (Some(index), None) => {
            let (l, lane) = p
                .arrangement
                .lanes
                .iter()
                .enumerate()
                .find(|(_, l)| l.track == Some(track))
                .ok_or("the track has no arrangement lane")?;
            let missing =
                || format!("clip index {index} does not exist; the track has {} arrangement clips", lane.clips.len());
            let i = usize::try_from(index).map_err(|_| missing())?;
            (ClipAt::Arrangement(l, i), lane.clips.get(i).ok_or_else(missing)?)
        }
        (None, Some(scene)) => {
            let (c, column) = p
                .launcher
                .columns
                .iter()
                .enumerate()
                .find(|(_, c)| c.track == track)
                .ok_or("the track has no clip slots")?;
            let missing = || format!("scene {scene} does not exist; the track has {} slots", column.slots.len());
            let s = usize::try_from(scene).ok().and_then(|s| s.checked_sub(1)).ok_or_else(missing)?;
            let slot = column.slots.get(s).ok_or_else(missing)?;
            let clip = slot.clip.as_ref().ok_or_else(|| format!("the slot of scene {scene} is empty"))?;
            (ClipAt::Slot(c, s), clip)
        }
    };
    Ok((at, clip.content, track))
}

/// The clip at `at`, to change.
pub(crate) fn clip_mut(p: &mut Project, at: ClipAt) -> Option<&mut Clip> {
    match at {
        ClipAt::Arrangement(lane, index) => p.arrangement.lanes.get_mut(lane)?.clips.get_mut(index),
        ClipAt::Slot(column, slot) => p.launcher.columns.get_mut(column)?.slots.get_mut(slot)?.clip.as_mut(),
    }
}

/// How many clips, on the arrangement and in the launcher, play `content`.
fn placements(p: &Project, content: ContentId) -> usize {
    let arranged = p.arrangement.lanes.iter().flat_map(|l| &l.clips).filter(|c| c.content == content).count();
    let launched = p
        .launcher
        .columns
        .iter()
        .flat_map(|c| &c.slots)
        .filter(|s| s.clip.as_ref().is_some_and(|c| c.content == content))
        .count();
    arranged.saturating_add(launched)
}

/// Runs `edit` on the notes of the clip `params` names. When other clips also play the clip's
/// content (a duplicate) and the edit changes it, the clip gets its own copy of the content
/// with the edit, and the other clips keep the content as it was (copy on write, ADR 0001). A
/// shared pattern, a content with lanes for named tracks, stays shared: its placements are one
/// pattern on purpose. The copy is part of the same command, so one undo takes back both.
pub(crate) fn edit_notes(
    p: &mut Project,
    params: &Value,
    edit: impl FnOnce(&mut Vec<Note>) -> Result<Value, CommandError>,
) -> Result<Value, CommandError> {
    copy_on_write(p, params, |p| edit(notes_mut(p, params)?))
}

/// Runs `edit`, which changes the content of the clip `params` names, with the copy on write
/// [`edit_notes`] describes: a content other clips also play is copied for this clip when the edit
/// changes it, and a shared pattern stays shared.
pub(crate) fn copy_on_write(
    p: &mut Project,
    params: &Value,
    edit: impl FnOnce(&mut Project) -> Result<Value, CommandError>,
) -> Result<Value, CommandError> {
    let (at, id, _) = locate(p, params)?;
    let original =
        p.content(id).filter(|c| c.lanes.iter().all(|l| l.target.is_none()) && placements(p, id) > 1).cloned();
    let value = edit(p)?;
    let Some(original) = original else { return Ok(value) };
    let new_id = p
        .contents
        .iter()
        .map(|c| c.id.0)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .map(ContentId)
        .ok_or("the project has too many clip contents")?;
    let content = p.contents.iter_mut().find(|c| c.id == id).ok_or("the clip's content is missing")?;
    if *content == original {
        return Ok(value);
    }
    let edited = std::mem::replace(content, original);
    p.contents.push(ClipContent { id: new_id, ..edited });
    clip_mut(p, at).ok_or("the clip is missing")?.content = new_id;
    Ok(value)
}

/// Whether a content lane plays on `track`: its own lane, or a shared pattern's lane for it.
fn plays_on(lane: &ContentLane, track: TrackId) -> bool {
    lane.target.is_none_or(|t| t == track)
}

pub(crate) fn check_midi(p: &Project, content: ContentId) -> Result<(), CommandError> {
    let content = p.content(content).ok_or("the clip's content is missing")?;
    if content.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))) {
        return Err("an audio clip has no notes; give a MIDI clip".into());
    }
    Ok(())
}

/// The notes of the clip `params` names (none for an empty MIDI clip).
pub(crate) fn notes_of<'a>(p: &'a Project, params: &Value) -> Result<&'a [Note], CommandError> {
    let (_, content, track) = locate(p, params)?;
    check_midi(p, content)?;
    let content = p.content(content).ok_or("the clip's content is missing")?;
    Ok(content
        .lanes
        .iter()
        .filter(|l| plays_on(l, track))
        .find_map(|l| match &l.data {
            LaneData::Notes(notes) => Some(notes.as_slice()),
            LaneData::Audio(_) => None,
        })
        .unwrap_or(&[]))
}

/// The notes of the clip `params` names, to change; an empty MIDI clip gets a note lane.
fn notes_mut<'a>(p: &'a mut Project, params: &Value) -> Result<&'a mut Vec<Note>, CommandError> {
    let (_, id, track) = locate(p, params)?;
    check_midi(p, id)?;
    let content = p.contents.iter_mut().find(|c| c.id == id).ok_or("the clip's content is missing")?;
    let has_lane = content.lanes.iter().any(|l| plays_on(l, track) && matches!(l.data, LaneData::Notes(_)));
    if !has_lane {
        if !content.lanes.is_empty() {
            return Err("the clip is a pattern with no notes for this track".into());
        }
        content.lanes.push(ContentLane { target: None, data: LaneData::Notes(Vec::new()) });
    }
    content
        .lanes
        .iter_mut()
        .filter(|l| plays_on(l, track))
        .find_map(|l| match &mut l.data {
            LaneData::Notes(notes) => Some(notes),
            LaneData::Audio(_) => None,
        })
        .ok_or_else(|| "the clip has no notes".into())
}

/// The note indices in `params.notes`, each once, in rising order; every one must exist.
pub(crate) fn indices(params: &Value, count: usize) -> Result<Vec<usize>, CommandError> {
    let list = params.get("notes").and_then(Value::as_array).ok_or("notes must be a list of note indices")?;
    if list.is_empty() {
        return Err("notes is empty; give the indices of the notes, from note.list".into());
    }
    if list.len() > MAX_NOTES_PER_COMMAND {
        return Err(format!("at most {MAX_NOTES_PER_COMMAND} notes in one command, got {}", list.len()).into());
    }
    let mut out = Vec::with_capacity(list.len());
    for value in list {
        let index = value.as_u64().ok_or("each note must be a whole number, an index from note.list")?;
        let index = usize::try_from(index)
            .ok()
            .filter(|i| *i < count)
            .ok_or_else(|| format!("note {index} does not exist; the clip has {count} notes"))?;
        out.push(index);
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

/// An optional finite number.
pub(crate) fn optional_number(params: &Value, key: &str) -> Result<Option<f64>, CommandError> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            v.as_f64().filter(|n| n.is_finite()).map(Some).ok_or_else(|| format!("{key} must be a number").into())
        }
    }
}

/// MIDI velocity 1 … 127 to the model's 0 … 1.
pub(crate) fn velocity_of(midi: f64) -> f32 {
    (midi.clamp(1.0, 127.0) / 127.0) as f32
}

/// The model's 0 … 1 velocity as MIDI 1 … 127.
pub fn midi_velocity(velocity: f32) -> u8 {
    if velocity.is_finite() { (f64::from(velocity) * 127.0).round().clamp(1.0, 127.0) as u8 } else { 1 }
}

fn note_json(index: usize, note: &Note) -> Value {
    json!({
        "index": index,
        "time": note.time,
        "length": note.duration,
        "key": note.key,
        "velocity": midi_velocity(note.velocity),
        "muted": note.muted,
    })
}

pub(crate) fn list(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let notes = notes_of(&session.project, params)?;
    Ok(Value::Array(notes.iter().enumerate().map(|(i, n)| note_json(i, n)).collect()))
}

/// One note `note.add` reads: `{time, length, key, velocity?}`.
fn new_note(value: &Value, at: usize) -> Result<Note, CommandError> {
    let object: &Map<String, Value> = value.as_object().ok_or_else(|| format!("note {at} must be an object"))?;
    if let Some(unknown) = object.keys().find(|k| !matches!(k.as_str(), "time" | "length" | "key" | "velocity")) {
        return Err(format!("note {at}: unknown field {unknown:?}; expected time, length, key, velocity").into());
    }
    let number = |key: &str| object.get(key).and_then(Value::as_f64).filter(|n| n.is_finite());
    let time = number("time").ok_or_else(|| format!("note {at}: time must be a number of beats"))?;
    if !(0.0..=MAX_BEAT).contains(&time) {
        return Err(format!("note {at}: time must be 0 to {MAX_BEAT} beats, got {time}").into());
    }
    let length = number("length").ok_or_else(|| format!("note {at}: length must be a number of beats"))?;
    if !(length > 0.0 && length <= MAX_BEAT) {
        return Err(format!("note {at}: length must be above 0 and at most {MAX_BEAT} beats, got {length}").into());
    }
    let key = object
        .get("key")
        .and_then(Value::as_u64)
        .and_then(|k| u8::try_from(k).ok())
        .filter(|k| *k <= 127)
        .ok_or_else(|| format!("note {at}: key must be a whole number from 0 to 127"))?;
    let velocity = match object.get("velocity") {
        None | Some(Value::Null) => DEFAULT_VELOCITY,
        Some(v) => v
            .as_f64()
            .filter(|v| (1.0..=127.0).contains(v))
            .ok_or_else(|| format!("note {at}: velocity must be 1 to 127"))?,
    };
    Ok(Note {
        time,
        duration: length,
        key,
        velocity: velocity_of(velocity),
        release_velocity: RELEASE_VELOCITY,
        channel: 0,
        muted: false,
    })
}

pub(crate) fn add(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let list = params.get("notes").and_then(Value::as_array).ok_or("notes must be a list of {time, length, key}")?;
    if list.is_empty() {
        return Err("notes is empty; give at least one {time, length, key}".into());
    }
    if list.len() > MAX_NOTES_PER_COMMAND {
        return Err(format!("at most {MAX_NOTES_PER_COMMAND} notes in one command, got {}", list.len()).into());
    }
    let added = list.iter().enumerate().map(|(i, v)| new_note(v, i)).collect::<Result<Vec<_>, _>>()?;
    append(session, params, added)
}

pub(crate) fn delete(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    edit_notes(&mut session.project, params, |notes| {
        let gone = indices(params, notes.len())?;
        // Removed from the back, so the indices still to remove stay where they are.
        for &index in gone.iter().rev() {
            notes.remove(index);
        }
        Ok(json!({ "deleted": gone, "count": notes.len() }))
    })
}

pub(crate) fn move_notes(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let beats = optional_number(params, "beats")?.unwrap_or(0.0);
    let semitones = match params.get("semitones") {
        None | Some(Value::Null) => 0,
        Some(v) => {
            v.as_i64().filter(|s| (-127..=127).contains(s)).ok_or("semitones must be a whole number, -127 to 127")?
        }
    };
    if params.get("beats").is_none() && params.get("semitones").is_none() {
        return Err("give beats (in time) or semitones (in pitch)".into());
    }
    if beats.abs() > MAX_BEAT {
        return Err(format!("beats must be within ±{MAX_BEAT}").into());
    }
    edit_notes(&mut session.project, params, |notes| {
        let moved = indices(params, notes.len())?;
        let mut changed = Vec::with_capacity(moved.len());
        for &index in &moved {
            let note = notes.get(index).ok_or("a note is missing")?;
            let mut time = note.time + beats;
            if (-EPSILON..0.0).contains(&time) {
                time = 0.0;
            }
            if !(0.0..=MAX_BEAT).contains(&time) {
                return Err(format!("note {index} would start at beat {time}, outside 0 to {MAX_BEAT}").into());
            }
            let key = u8::try_from(i64::from(note.key) + semitones)
                .ok()
                .filter(|k| *k <= 127)
                .ok_or_else(|| format!("note {index} would leave the keys 0 to 127"))?;
            changed.push((index, Note { time, key, ..*note }));
        }
        for (index, note) in &changed {
            if let Some(slot) = notes.get_mut(*index) {
                *slot = *note;
            }
        }
        // A moved note landing on another, the same key from the same start, takes its place.
        let replace = params.get("replace").and_then(Value::as_bool) == Some(true);
        let covered: Vec<usize> = if replace {
            (0..notes.len())
                .filter(|i| !moved.contains(i))
                .filter(|i| {
                    changed.iter().any(|(_, m)| m.key == notes[*i].key && (m.time - notes[*i].time).abs() < EPSILON)
                })
                .filter(|_| beats.abs() >= EPSILON || semitones != 0)
                .collect()
        } else {
            Vec::new()
        };
        for index in covered.iter().rev() {
            notes.remove(*index);
        }
        // The moved notes' indices after the covered ones went.
        let now: Vec<usize> = moved.iter().map(|i| i - covered.iter().filter(|c| *c < i).count()).collect();
        Ok(json!({ "notes": now, "replaced": covered }))
    })
}

pub(crate) fn resize(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (length, by, start_by) =
        (optional_number(params, "length")?, optional_number(params, "by")?, optional_number(params, "startBy")?);
    if [length.is_some(), by.is_some(), start_by.is_some()].iter().filter(|given| **given).count() != 1 {
        return Err(
            "give length (beats), by (beats to add) or startBy (beats to move the starts by), one of them".into()
        );
    }
    if start_by.is_some_and(|b| b.abs() > MAX_BEAT) {
        return Err(format!("startBy must be within ±{MAX_BEAT}").into());
    }
    edit_notes(&mut session.project, params, |notes| {
        let resized = indices(params, notes.len())?;
        let mut changes = Vec::with_capacity(resized.len());
        for &index in &resized {
            let note = notes.get(index).ok_or("a note is missing")?;
            // `startBy` moves the start and keeps the end: the length changes the other way.
            let mut time = note.time + start_by.unwrap_or(0.0);
            if (-EPSILON..0.0).contains(&time) {
                time = 0.0;
            }
            if !(0.0..=MAX_BEAT).contains(&time) {
                return Err(format!("note {index} would start at beat {time}, outside 0 to {MAX_BEAT}").into());
            }
            let new = match (length, start_by) {
                (Some(length), _) => length,
                (None, Some(_)) => note.time + note.duration - time,
                (None, None) => note.duration + by.unwrap_or(0.0),
            };
            if !(new > 0.0 && new <= MAX_BEAT) {
                return Err(format!(
                    "note {index} would be {new} beats long; a length is above 0 and at most {MAX_BEAT}"
                )
                .into());
            }
            changes.push((index, time, new));
        }
        for (index, time, new) in changes {
            if let Some(note) = notes.get_mut(index) {
                note.time = time;
                note.duration = new;
            }
        }
        Ok(json!({ "notes": resized }))
    })
}

pub(crate) fn set_velocity(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (velocity, by) = (optional_number(params, "velocity")?, optional_number(params, "by")?);
    let each = velocity_list(params)?;
    match (velocity, by, &each) {
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) | (_, Some(_), Some(_)) | (None, None, None) => {
            return Err("give velocity (1 to 127), by (to add) or velocities (one per note), one of them".into());
        }
        (Some(v), None, None) if !(1.0..=127.0).contains(&v) => {
            return Err(format!("velocity must be 1 to 127, got {v}").into());
        }
        (None, Some(b), None) if b.abs() > 127.0 => return Err(format!("by must be within ±127, got {b}").into()),
        _ => {}
    }
    edit_notes(&mut session.project, params, |notes| {
        let changed = indices(params, notes.len())?;
        if let Some(each) = &each {
            // One value per entry of `notes`, in the order given; a repeated index takes its last.
            let given = params.get("notes").and_then(Value::as_array).map_or(0, Vec::len);
            if each.len() != given {
                return Err(format!(
                    "velocities must have one value per note: {given} notes, {} velocities",
                    each.len()
                )
                .into());
            }
            let named = params.get("notes").and_then(Value::as_array).ok_or("notes must be a list of note indices")?;
            for (value, midi) in named.iter().zip(each) {
                let note = value.as_u64().and_then(|i| usize::try_from(i).ok()).and_then(|i| notes.get_mut(i));
                if let Some(note) = note {
                    note.velocity = velocity_of(midi.round());
                }
            }
            return Ok(json!({ "notes": changed }));
        }
        for &index in &changed {
            if let Some(note) = notes.get_mut(index) {
                let midi = velocity.unwrap_or_else(|| f64::from(midi_velocity(note.velocity)) + by.unwrap_or(0.0));
                note.velocity = velocity_of(midi.round());
            }
        }
        Ok(json!({ "notes": changed }))
    })
}

/// `params.velocities`: one velocity, 1 to 127, per entry of `notes`; `None` when left out.
fn velocity_list(params: &Value) -> Result<Option<Vec<f64>>, CommandError> {
    let list = match params.get("velocities") {
        None | Some(Value::Null) => return Ok(None),
        Some(v) => v.as_array().ok_or("velocities must be a list of velocities, 1 to 127, one per note")?,
    };
    if list.len() > MAX_NOTES_PER_COMMAND {
        return Err(format!("at most {MAX_NOTES_PER_COMMAND} velocities in one command, got {}", list.len()).into());
    }
    list.iter()
        .map(|v| {
            v.as_f64()
                .filter(|v| (1.0..=127.0).contains(v))
                .ok_or_else(|| CommandError::from("each of velocities must be a number from 1 to 127"))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

/// The quantise grid `params.grid` names, in beats: a value as `launcher.setQuantization` takes it,
/// with bars as long as the project's time signature makes them.
fn quantize_step(p: &Project, params: &Value) -> Result<f64, CommandError> {
    let text = params.get("grid").and_then(Value::as_str).ok_or("grid must be a text such as \"1/16\" or \"1/8T\"")?;
    let step = match parse_quantization(text)? {
        Quantization::None => return Err("grid none has no lines to quantise to; give a value such as \"1/16\"".into()),
        Quantization::Bars(bars) => {
            let ts = p.transport.time_signature;
            let bar = if ts.denominator == 0 {
                4.0
            } else {
                f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator)
            };
            f64::from(bars) * bar
        }
        Quantization::Beats(beats) => beats,
    };
    if step.is_finite() && step > 0.0 { Ok(step) } else { Err(format!("grid {text:?} has no step").into()) }
}

/// The grid line nearest to `beat`; `beat` itself when it is already on one, so a note on the grid
/// keeps its exact value.
fn nearest_line(beat: f64, step: f64) -> f64 {
    let line = (beat / step).round() * step;
    if (line - beat).abs() <= EPSILON { beat } else { line }
}

/// `from` moved towards `to` by `amount` (0 … 1); `to` itself at 1.
fn towards(from: f64, to: f64, amount: f64) -> f64 {
    if amount >= 1.0 { to } else { from + (to - from) * amount }
}

pub(crate) fn quantize(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let step = quantize_step(&session.project, params)?;
    let amount = optional_number(params, "amount")?.unwrap_or(100.0);
    if !(0.0..=100.0).contains(&amount) {
        return Err(format!("amount must be 0 to 100 percent, got {amount}").into());
    }
    let amount = amount / 100.0;
    let ends = match params.get("ends") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or("ends must be true or false")?,
    };
    let all = matches!(params.get("notes"), None | Some(Value::Null));
    // Every note of an empty clip is none: nothing to change, and no note lane to add.
    if all && notes_of(&session.project, params)?.is_empty() {
        return Ok(json!({ "notes": [], "moved": 0 }));
    }
    // Through `edit_notes`, so quantising a duplicate gives it its own content (#352).
    edit_notes(&mut session.project, params, |notes| {
        let chosen = if all { (0..notes.len()).collect() } else { indices(params, notes.len())? };
        let mut changed = Vec::with_capacity(chosen.len());
        for &index in &chosen {
            let note = notes.get(index).ok_or("a note is missing")?;
            let start = nearest_line(note.time, step);
            let time = towards(note.time, start, amount).max(0.0);
            if time > MAX_BEAT {
                return Err(format!("note {index} would start at beat {time}, after {MAX_BEAT}").into());
            }
            let duration = if ends {
                let end = note.time + note.duration;
                let mut line = nearest_line(end, step);
                if line <= start + EPSILON {
                    // On or before its quantised start: the end goes to the first line after it.
                    line = ((start / step + EPSILON).floor() + 1.0) * step;
                }
                towards(end, line, amount) - time
            } else {
                note.duration
            };
            if !(duration > 0.0 && duration <= MAX_BEAT) {
                return Err(format!("note {index} would be {duration} beats long").into());
            }
            if time != note.time || duration != note.duration {
                changed.push((index, time, duration));
            }
        }
        let moved = changed.len();
        for (index, time, duration) in changed {
            if let Some(note) = notes.get_mut(index) {
                note.time = time;
                note.duration = duration;
            }
        }
        Ok(json!({ "notes": chosen, "moved": moved }))
    })
}

/// Notes copied for `note.paste`: their times from the earliest start (the first is at 0), where
/// that start was, and how long they span (from the earliest start to the latest end).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NoteClipboard {
    notes: Vec<Note>,
    from: f64,
    span: f64,
}

impl NoteClipboard {
    pub(crate) fn len(&self) -> usize {
        self.notes.len()
    }
}

/// The notes `chosen` names, with their times from the earliest start.
fn clipboard_of(notes: &[Note], chosen: &[usize]) -> Result<NoteClipboard, CommandError> {
    let picked: Vec<Note> =
        chosen.iter().map(|&i| notes.get(i).copied().ok_or("a note is missing")).collect::<Result<_, _>>()?;
    let from = picked.iter().map(|n| n.time).fold(f64::INFINITY, f64::min);
    let end = picked.iter().map(|n| n.time + n.duration).fold(f64::NEG_INFINITY, f64::max);
    if !(from.is_finite() && end.is_finite()) {
        return Err("no notes to copy".into());
    }
    let notes = picked.into_iter().map(|n| Note { time: n.time - from, ..n }).collect();
    Ok(NoteClipboard { notes, from, span: end - from })
}

/// The clipboard's notes placed from `time`, each checked against the limits.
fn placed(clipboard: &NoteClipboard, time: f64) -> Result<Vec<Note>, CommandError> {
    clipboard
        .notes
        .iter()
        .map(|n| {
            let at = time + n.time;
            if (0.0..=MAX_BEAT).contains(&at) {
                Ok(Note { time: at, ..*n })
            } else {
                Err(format!("a note would start at beat {at}, outside 0 to {MAX_BEAT}").into())
            }
        })
        .collect()
}

/// Appends `added` to the clip `params` names and says where they went.
/// Through `edit_notes`, so adding to a duplicate gives it its own content (#352).
fn append(session: &mut Session, params: &Value, added: Vec<Note>) -> Result<Value, CommandError> {
    edit_notes(&mut session.project, params, |notes| {
        let total = notes.len().saturating_add(added.len());
        if total > MAX_NOTES_PER_CLIP {
            return Err(format!("a clip holds at most {MAX_NOTES_PER_CLIP} notes; this would make {total}").into());
        }
        let first = notes.len();
        notes.extend(added);
        Ok(json!({ "added": (first..notes.len()).collect::<Vec<_>>(), "count": notes.len() }))
    })
}

pub(crate) fn copy(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let notes = notes_of(&session.project, params)?;
    let chosen = indices(params, notes.len())?;
    let clipboard = clipboard_of(notes, &chosen)?;
    let result = json!({ "copied": clipboard.len(), "span": clipboard.span });
    session.clipboards.notes = Some(clipboard);
    Ok(result)
}

pub(crate) fn cut(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let notes = notes_of(&session.project, params)?;
    let chosen = indices(params, notes.len())?;
    let clipboard = clipboard_of(notes, &chosen)?;
    let result = delete(session, params)?;
    session.clipboards.notes = Some(clipboard);
    Ok(result)
}

pub(crate) fn paste(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let clipboard = session.clipboards.notes.as_ref().ok_or("no notes have been copied; run note.copy first")?;
    let time = match optional_number(params, "time")? {
        Some(time) if !(0.0..=MAX_BEAT).contains(&time) => {
            return Err(format!("time must be 0 to {MAX_BEAT} beats, got {time}").into());
        }
        Some(time) => time,
        None => clipboard.from + clipboard.span,
    };
    let added = placed(clipboard, time)?;
    append(session, params, added)
}

pub(crate) fn duplicate(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let beats = optional_number(params, "beats")?;
    let semitones = match params.get("semitones") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_i64().filter(|s| (-127..=127).contains(s)).ok_or("semitones must be a whole number, -127 to 127")?,
        ),
    };
    if beats.is_some_and(|b| b.abs() > MAX_BEAT) {
        return Err(format!("beats must be within ±{MAX_BEAT}").into());
    }
    let notes = notes_of(&session.project, params)?;
    let chosen = indices(params, notes.len())?;
    let clipboard = clipboard_of(notes, &chosen)?;
    if beats.is_none() && semitones.is_none() {
        let added = placed(&clipboard, clipboard.from + clipboard.span)?;
        return append(session, params, added);
    }
    // Copies `beats` and `semitones` away from the originals: a copy made by dragging.
    let (beats, semitones) = (beats.unwrap_or(0.0), semitones.unwrap_or(0));
    let mut time = clipboard.from + beats;
    if (-EPSILON..0.0).contains(&time) {
        time = 0.0;
    }
    let mut added = placed(&clipboard, time)?;
    for note in &mut added {
        note.key = u8::try_from(i64::from(note.key) + semitones)
            .ok()
            .filter(|k| *k <= 127)
            .ok_or("a copy would leave the keys 0 to 127")?;
    }
    append(session, params, added)
}

/// Whether the note `note` spans the beat `at`: it starts before and ends after it, so a note
/// that starts or ends exactly at `at` is already cut there.
fn spans(note: &Note, at: f64) -> bool {
    note.time < at - EPSILON && note.time + note.duration > at + EPSILON
}

/// `note.split`: splits each named note that spans the beat `at` into two at `at`, the first
/// keeping its start and the second its end. One command, one undo step; the halves are appended,
/// so the indices of the notes the command did not split stay valid.
pub(crate) fn split(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let at = optional_number(params, "at")?.ok_or("give at, the beat to split the notes at")?;
    if !(0.0..=MAX_BEAT).contains(&at) {
        return Err(format!("at must be 0 to {MAX_BEAT} beats, got {at}").into());
    }
    edit_notes(&mut session.project, params, |notes| {
        let chosen = indices(params, notes.len())?;
        // The first halves are made from the notes as they are; the second halves are appended
        // once every one of them is known, so an index always means the same note.
        let mut firsts: Vec<(usize, f64)> = Vec::new();
        let mut seconds = Vec::new();
        for &index in &chosen {
            let note = notes.get(index).ok_or("a note is missing")?;
            if !spans(note, at) {
                continue;
            }
            firsts.push((index, at - note.time));
            seconds.push(Note { time: at, duration: note.time + note.duration - at, ..*note });
        }
        let total = notes.len().saturating_add(seconds.len());
        if total > MAX_NOTES_PER_CLIP {
            return Err(format!("a clip holds at most {MAX_NOTES_PER_CLIP} notes; this would make {total}").into());
        }
        for (index, duration) in firsts {
            if let Some(note) = notes.get_mut(index) {
                note.duration = duration;
            }
        }
        let first_new = notes.len();
        notes.extend(seconds);
        let made: Vec<usize> = (first_new..notes.len()).collect();
        Ok(json!({ "notes": made, "count": made.len() }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn velocities_round_trip_through_midi_values() {
        for midi in 1..=127u8 {
            assert_eq!(midi_velocity(velocity_of(f64::from(midi))), midi);
        }
        assert_eq!(midi_velocity(f32::NAN), 1);
        assert_eq!(midi_velocity(0.0), 1);
        assert_eq!(midi_velocity(2.0), 127);
    }
}
