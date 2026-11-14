// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's clip edits (`docs/spec/ui-clip-editor.md` › Joining, fitting, cropping and
//! splitting): `note.join`, `note.fitToTime`, `clip.crop` and `clip.setOrigin`. `clip.split` with
//! several cuts is in [`crate::editing`].
//!
//! Each edits through [`copy_on_write`], so a clip that shares its content with another gets a
//! copy of it first (ADR 0001), and each call is one undo step. A clip that holds audio refuses
//! them: it has no notes.

use serde_json::{Value, json};
use suhina_model::{LoopRange, Note, Notes};

use crate::commands::Args;
use crate::notes::{
    ClipAt, MAX_BEAT, after_removal, clip_at, clip_mut, copy_on_write, covered_go, edit_notes, indices, locate,
    notes_mut, placements, replace_under, replaces, with_replaced,
};
use crate::{CommandError, Session};

/// Starts and ends this close are one.
const EPSILON: f64 = 1e-9;
/// The shortest note a crop leaves: a hundredth of a beat. A sliver shorter than this is dropped.
const MIN_LENGTH: f64 = 0.01;

/// `note.join`: the named notes on one key become one note, from the first's start to the last's end
/// (the first reaching over any gap, as legato does), with the first's velocity, release velocity,
/// channel and mute. Notes on different keys join per key; a key with one named note keeps it.
pub(crate) fn join(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    edit_notes(&mut session.project, params, |notes| {
        let named = indices(params, notes)?;
        let (kept, removed) = joined(notes, &named)?;
        for (index, end) in &kept {
            if let Some(note) = notes.get_mut(*index) {
                note.duration = end - note.time;
            }
        }
        let mut gone = removed.clone();
        gone.sort_unstable();
        gone.dedup();
        for &index in gone.iter().rev() {
            notes.remove(index);
        }
        let joined: Vec<usize> = kept.iter().map(|(index, _)| *index).collect();
        Ok(json!({
            "notes": after_removal(&joined, &gone),
            "removed": gone,
            "count": notes.len(),
        }))
    })
}

/// Each joined note's index with its new end, and the indices of the notes it took in.
type Joined = (Vec<(usize, f64)>, Vec<usize>);

/// What `note.join` does to `named` among `notes`: each joined note's index with its new end, and
/// the indices of the notes it took in. On each key the first named note reaches, as legato does,
/// over any gap, to the last one's end and takes in the others.
fn joined(notes: &Notes, named: &[usize]) -> Result<Joined, CommandError> {
    let mut by_key: std::collections::BTreeMap<u8, Vec<(f64, usize)>> = std::collections::BTreeMap::new();
    for &index in named {
        let note = notes.get(index).ok_or("a note is missing")?;
        by_key.entry(note.key).or_default().push((note.time, index));
    }
    let mut kept = Vec::new();
    let mut removed = Vec::new();
    for line in by_key.values_mut().filter(|line| line.len() > 1) {
        line.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let Some(&(_, first)) = line.first() else { continue };
        let end = line.iter().filter_map(|(_, i)| notes.get(*i)).map(|n| n.time + n.duration).fold(f64::MIN, f64::max);
        kept.push((first, end));
        removed.extend(line.iter().skip(1).map(|(_, i)| *i));
    }
    if kept.is_empty() {
        return Err("no notes to join: give at least two notes on one key".into());
    }
    if kept.iter().any(|(_, end)| !end.is_finite() || *end > MAX_BEAT) {
        return Err(format!("a joined note would end after beat {MAX_BEAT}").into());
    }
    Ok((kept, removed))
}

/// `note.fitToTime`: the named notes are stretched or squeezed, starts and lengths alike, so they
/// fill `start` … `end` exactly: the first start at `start`, the last end at `end`.
pub(crate) fn fit_to_time(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let args = Args::new(params);
    let start = args.opt_f64("start")?.ok_or("give start, where the notes begin, in content beats")?;
    let end = args.opt_f64("end")?.ok_or("give end, where the notes end, in content beats")?;
    if !(0.0..=MAX_BEAT).contains(&start) || !(0.0..=MAX_BEAT).contains(&end) || end <= start + EPSILON {
        return Err(
            format!("start and end must be within 0 to {MAX_BEAT}, end after start; got {start} and {end}").into()
        );
    }
    edit_notes(&mut session.project, params, |notes| {
        let named = indices(params, notes)?;
        let (mut from, mut to) = (f64::INFINITY, f64::NEG_INFINITY);
        for &index in &named {
            let note = notes.get(index).ok_or("a note is missing")?;
            from = from.min(note.time);
            to = to.max(note.time + note.duration);
        }
        // Not after the first start (or not a number): nothing to fit.
        if to.partial_cmp(&(from + EPSILON)) != Some(std::cmp::Ordering::Greater) {
            return Err("the notes have no length to fit".into());
        }
        let factor = (end - start) / (to - from);
        let mut fitted = Vec::with_capacity(named.len());
        for &index in &named {
            let note = notes.get(index).ok_or("a note is missing")?;
            let time = start + (note.time - from) * factor;
            let length = note.duration * factor;
            if !(time.is_finite() && length.is_finite() && length > 0.0) {
                return Err(format!("note {index} would not fit").into());
            }
            fitted.push((index, time.clamp(0.0, MAX_BEAT), length.min(MAX_BEAT)));
        }
        for (index, time, length) in fitted {
            if let Some(note) = notes.get_mut(index) {
                note.time = time;
                note.duration = length;
            }
        }
        let replaced = if replaces(params) { replace_under(notes, &named, covered_go(params)) } else { Vec::new() };
        Ok(with_replaced(params, json!({ "notes": after_removal(&named, &replaced), "factor": factor }), &replaced))
    })
}

/// Where `note` lies once cut to `start` … `end`: its new start and length, or `None` when it is
/// outside. A note wholly inside keeps its length, however short; a piece an edge cuts shorter
/// than [`MIN_LENGTH`] goes.
fn cropped(note: &Note, start: f64, end: f64) -> Option<(f64, f64)> {
    let note_end = note.time + note.duration;
    if note.time >= start - EPSILON && note_end <= end + EPSILON {
        return (note.time < end).then_some((note.time, note.duration));
    }
    let from = note.time.max(start);
    let to = note_end.min(end);
    (to - from >= MIN_LENGTH - EPSILON).then_some((from, to - from))
}

/// Fails when the clip `params` names plays a pattern of named-track lanes that another clip plays
/// too: such content is shared, not copied on write, and moving its notes for this clip's start
/// marker or loop would move them under the other clip as well.
fn check_unshared_pattern(p: &suhina_model::Project, params: &Value) -> Result<(), CommandError> {
    let (_, content, _) = locate(p, params)?;
    let pattern = p.content(content).is_some_and(|c| c.lanes.iter().any(|l| l.target.is_some()));
    if pattern && placements(p, content) > 1 {
        return Err("the clip plays a pattern another clip plays too; this edit would change that clip as well".into());
    }
    Ok(())
}

/// `clip.crop`: the clip keeps only `start` … `end` of its content: its start marker and length
/// become that stretch's, its loop is clipped to it (off where they do not meet), and the notes
/// outside it are deleted, those across an edge cut at it. An arrangement clip moves on the
/// arrangement with its start marker, so what is left plays where it played.
pub(crate) fn crop(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let args = Args::new(params);
    let start = args.opt_f64("start")?.ok_or("give start, the time selection's start, in content beats")?;
    let end = args.opt_f64("end")?.ok_or("give end, the time selection's end, in content beats")?;
    if !(0.0..=MAX_BEAT).contains(&start) || !(0.0..=MAX_BEAT).contains(&end) || end <= start + MIN_LENGTH {
        return Err(
            format!("start and end must be within 0 to {MAX_BEAT}, end after start; got {start} and {end}").into()
        );
    }
    check_unshared_pattern(&session.project, params)?;
    let (at, _, _) = locate(&session.project, params)?;
    let clip = clip_at(&session.project, at).ok_or("the clip is missing")?.clone();
    let played = match clip.loop_range {
        Some(range) => (clip.offset.min(range.start), range.end),
        None => (clip.offset, clip.offset + clip.length),
    };
    if end <= played.0 + EPSILON || start >= played.1 - EPSILON {
        return Err(format!(
            "the time selection, beats {start} to {end}, is outside the part of the clip that plays ({} to {})",
            played.0, played.1
        )
        .into());
    }
    // A looping clip goes on looping over what is left and keeps its end on the arrangement; one
    // that plays once is as long as the stretch.
    let (new_start, new_length) = match at {
        ClipAt::Arrangement(..) => {
            // Where the clip first plays content beat `start`: after the start marker on the first
            // pass, or, for a loop that starts before the marker, once the loop has come round.
            let into = match clip.loop_range {
                Some(range) if start < clip.offset - EPSILON && start >= range.start - EPSILON => {
                    (range.end - clip.offset) + (start - range.start)
                }
                _ => start - clip.offset,
            };
            let moved = clip.start + into;
            if moved < -EPSILON {
                return Err("the clip would start before the beginning of the song".into());
            }
            let moved = moved.max(0.0);
            let length = if clip.loop_range.is_some() { clip.start + clip.length - moved } else { end - start };
            if length.partial_cmp(&MIN_LENGTH) != Some(std::cmp::Ordering::Greater) {
                return Err("the cropped clip would have no length left".into());
            }
            check_neighbours(&session.project, at, moved, length)?;
            (moved, length)
        }
        ClipAt::Slot(..) => (clip.start, if clip.loop_range.is_some() { clip.length } else { end - start }),
    };
    copy_on_write(&mut session.project, params, |p| {
        let notes = notes_mut(p, params)?;
        let mut deleted = Vec::new();
        let mut cut = Vec::new();
        for index in (0..notes.len()).rev() {
            let Some(note) = notes.get(index).copied() else { continue };
            match cropped(&note, start, end) {
                None => {
                    notes.remove(index);
                    deleted.push(index);
                }
                Some((time, length)) => {
                    if (time - note.time).abs() > EPSILON || (length - note.duration).abs() > EPSILON {
                        if let Some(note) = notes.get_mut(index) {
                            note.time = time;
                            note.duration = length;
                        }
                        cut.push(index);
                    }
                }
            }
        }
        deleted.reverse();
        cut.sort_unstable();
        let cut = after_removal(&cut, &deleted);
        let clip = clip_mut(p, at).ok_or("the clip is missing")?;
        clip.start = new_start;
        clip.offset = start;
        clip.length = new_length;
        clip.loop_range = clip
            .loop_range
            .map(|range| LoopRange { start: range.start.max(start), end: range.end.min(end) })
            .filter(|range| range.end > range.start + EPSILON);
        clip.fit_fades();
        Ok(json!({
            "start": start,
            "end": end,
            "deleted": deleted,
            "cut": cut,
            "loop": clip.loop_range.is_some(),
        }))
    })
}

/// Fails when an arrangement clip moved to `start`, `length` beats long, would overlap another clip
/// on its lane.
fn check_neighbours(p: &suhina_model::Project, at: ClipAt, start: f64, length: f64) -> Result<(), CommandError> {
    let ClipAt::Arrangement(lane, index) = at else { return Ok(()) };
    let lane = p.arrangement.lanes.get(lane).ok_or("the clip is missing")?;
    let end = start + length;
    for (other, clip) in lane.clips.iter().enumerate() {
        if other != index && clip.start < end - 1e-6 && start < clip.start + clip.length - 1e-6 {
            return Err(format!("the cropped clip would overlap the clip at beat {} on its lane", clip.start).into());
        }
    }
    Ok(())
}

/// `clip.setOrigin`: content beat `beat` becomes the content's beat 0 (1.1.1 on the clip's own
/// ruler): every note, the start marker and the loop move back by it together, so the clip sounds
/// and sits in the song as before. Nothing of the clip may lie before the beat, as the content has
/// no beats before 0: no note, not the start marker, not the loop's start.
pub(crate) fn set_origin(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let beat = Args::new(params).opt_f64("beat")?.ok_or("give beat, the content beat that becomes the first")?;
    if !(0.0..=MAX_BEAT).contains(&beat) {
        return Err(format!("beat must be within 0 to {MAX_BEAT}, got {beat}").into());
    }
    check_unshared_pattern(&session.project, params)?;
    let (at, _, _) = locate(&session.project, params)?;
    let clip = clip_at(&session.project, at).ok_or("the clip is missing")?.clone();
    if beat == 0.0 {
        return Ok(json!({ "beat": 0.0, "offset": clip.offset }));
    }
    if clip.offset < beat - EPSILON {
        return Err(format!(
            "the clip starts playing at content beat {}, before beat {beat}: move its start marker to it or later first",
            clip.offset
        )
        .into());
    }
    if let Some(range) = clip.loop_range
        && range.start < beat - EPSILON
    {
        return Err(format!("the clip's loop starts at content beat {}, before beat {beat}", range.start).into());
    }
    copy_on_write(&mut session.project, params, |p| {
        let notes = notes_mut(p, params)?;
        if let Some(early) = notes.iter().map(|n| n.time).find(|t| *t < beat - EPSILON) {
            return Err(format!(
                "a note starts at content beat {early}, before beat {beat}; the content has no beats before its first"
            )
            .into());
        }
        for note in notes.iter_mut() {
            note.time = (note.time - beat).max(0.0);
        }
        let clip = clip_mut(p, at).ok_or("the clip is missing")?;
        clip.offset = (clip.offset - beat).max(0.0);
        clip.loop_range =
            clip.loop_range.map(|range| LoopRange { start: (range.start - beat).max(0.0), end: range.end - beat });
        Ok(json!({ "beat": beat, "offset": clip.offset, "loop": clip.loop_range.map(|r| json!([r.start, r.end])) }))
    })
}
