// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An arrangement clip's edges dragged (`docs/spec/arrangement-editing.md` › Clip edges): its
//! start and end moved, or its audio
//! stretched to the new length (Alt held on an edge). A clip that does not loop
//! ends at most where its audio does; a looping clip goes on as long as it is made, its loop
//! repeating; `loop` turns the loop on first (the loop handle at a clip's lower right).

use serde_json::{Value, json};
use suhina_model::{Clip, ContentId, LaneData, LoopRange, Project, WarpMarker};

use crate::commands::{Args, lane_and_index, make_room, new_content_id};
use crate::editing::content_rate;
use crate::{CommandError, Session};

/// The least length a clip is left with.
const MIN_LENGTH: f64 = 1.0 / 64.0;

/// An optional beat parameter, finite and at least 0.
fn edge(params: &Value, key: &str) -> Result<Option<f64>, CommandError> {
    match Args::new(params).value(key) {
        None => Ok(None),
        Some(v) => {
            let beat =
                v.as_f64().filter(|b| b.is_finite()).ok_or_else(|| format!("{key} must be a number of beats"))?;
            Ok(Some(beat.max(0.0)))
        }
    }
}

/// Whether the content plays audio.
fn is_audio(p: &Project, content: ContentId) -> bool {
    p.content(content).is_some_and(|c| c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))))
}

/// Puts `clip` back on lane `lane` (it was taken out), replacing what it covers unless `stack`;
/// answers with its index there.
fn put_back(p: &mut Project, lane: usize, clip: Clip, stack: bool) -> Result<usize, CommandError> {
    let stack = stack && !p.lane_takes_cover(lane, clip.start, clip.start + clip.length);
    let lane = p.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?;
    if !stack {
        make_room(&mut lane.clips, clip.start, clip.start + clip.length);
    }
    lane.clips.push(clip.clone());
    lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    Ok(lane.clips.iter().position(|c| *c == clip).unwrap_or_default())
}

/// `clip.trimMany {clips, edge, by, stack?}`: the same edge of every clip moved by `by` beats as
/// `clip.trim` moves it, each stopping at its own limits and at the next clip of the selection on
/// its lane, in one undo step (#956). Each clip is
/// found by its id when its turn comes, as an earlier one may have changed where the others are.
pub(crate) fn trim_clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let edge = match Args::new(params).raw_str("edge") {
        Some("start") => "start",
        Some("end") => "end",
        _ => return Err("edge must be start or end".into()),
    };
    let by = Args::new(params)
        .value("by")
        .and_then(Value::as_f64)
        .filter(|b| b.is_finite())
        .ok_or("by must be a number of beats")?;
    let stack = Args::new(params).value("stack").cloned().unwrap_or(Value::Bool(false));
    let p = &session.project;
    let ids: Vec<u32> = crate::commands::clip_list(p, params)?
        .into_iter()
        .filter_map(|(track, index)| {
            p.arrangement.lanes.iter().find(|l| l.track == Some(track)).and_then(|l| l.clips.get(index)).map(|c| c.id.0)
        })
        .collect();
    // Where each clip of the selection starts and ends before any moves, by id, on its lane: an
    // edge moved outward stops where the next clip of the same selection on that lane begins
    // (or the one before ends), so stretching the last of two clips side by side does not run
    // the first over it and cut it short.
    let before: Vec<(u32, usize, f64, f64)> = session
        .project
        .arrangement
        .lanes
        .iter()
        .enumerate()
        .flat_map(|(lane, l)| {
            l.clips.iter().filter(|c| ids.contains(&c.id.0)).map(move |c| (c.id.0, lane, c.start, c.start + c.length))
        })
        .collect();
    for id in &ids {
        let p = &session.project;
        let Some((lane, track, index, clip)) = p.arrangement.lanes.iter().enumerate().find_map(|(lane, l)| {
            let index = l.clips.iter().position(|c| c.id.0 == *id)?;
            Some((lane, l.track?, index, l.clips.get(index)?.clone()))
        }) else {
            continue;
        };
        let at = if edge == "start" { clip.start } else { clip.start + clip.length };
        let mut to = at + by;
        let others = before.iter().filter(|(other, l, ..)| *other != *id && *l == lane);
        if edge == "end" && by > 0.0 {
            let next = others
                .filter(|(.., start, _)| *start >= at - 1e-9)
                .map(|(.., start, _)| *start)
                .fold(f64::INFINITY, f64::min);
            to = to.min(next.max(at));
        } else if edge == "start" && by < 0.0 {
            let previous =
                others.filter(|(.., end)| *end <= at + 1e-9).map(|(.., end)| *end).fold(f64::NEG_INFINITY, f64::max);
            to = to.max(previous.min(at));
        }
        if (to - at).abs() < 1e-9 {
            continue;
        }
        trim_clip(session, &json!({ "track": track.0, "index": index, edge: to, "stack": stack }))?;
    }
    Ok(json!({ "clips": ids.len() }))
}

/// `clip.trim {track, index, start?, end?, loop?, stack?}`: moves the clip's left edge to
/// `start` and its right edge to `end` (arrangement beats). The left edge moves the content
/// with it, so what plays stays where it was; an audio clip's goes no earlier than its audio's
/// start, a MIDI clip's on before its notes, the content growing at the start (#832). A clip
/// that does not loop ends at most where its audio does (a MIDI clip's content grows with it);
/// a looping clip repeats its loop as far as it goes. The clip replaces what it now covers
/// unless stacked. Edges past the limits stop at them.
pub(crate) fn trim_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (start, end) = (edge(params, "start")?, edge(params, "end")?);
    let (turn_loop, stack) = (Args::new(params).flag("loop")?, Args::new(params).flag("stack")?);
    if start.is_none() && end.is_none() && !turn_loop {
        return Err("give start, end, or both".into());
    }
    let (lane, index) = lane_and_index(&session.project, params)?;
    let p = &mut session.project;
    let mut clip =
        p.arrangement.lanes.get(lane).and_then(|l| l.clips.get(index)).cloned().ok_or("the clip is missing")?;
    let rate = content_rate(p, &clip);
    let audio = is_audio(p, clip.content);
    let content_length = p.content(clip.content).map_or(f64::INFINITY, |c| c.length);
    if turn_loop && clip.loop_range.is_none() {
        let end = (clip.offset + clip.length * rate).min(content_length);
        clip.loop_range = Some(LoopRange { start: clip.offset, end });
    }
    let looped = clip.loop_range.is_some();
    let old_end = clip.start + clip.length;
    let (mut new_start, mut new_end) = (start.unwrap_or(clip.start), end.unwrap_or(old_end));
    // The left edge stops short of the right one: a clip never gets shorter than the least.
    if start.is_some() {
        new_start = new_start.min(new_end - MIN_LENGTH).max(0.0);
    }
    // The left edge: the content moves with it, no earlier than its start; a looping clip's goes
    // back into earlier passes of its loop, as its right edge goes on into later ones.
    let mut offset = clip.offset + (new_start - clip.start) * rate;
    match clip.loop_range.filter(|r| r.end > r.start) {
        // Past the content's start, or from inside the loop past the loop's start (an intro before
        // the loop is drawn out as it is), or past the loop's end into a later pass: kept inside
        // the loop, where it plays, as the engine starts a clip whose offset is past the loop's
        // end at the loop's start, which moved what played.
        Some(range) if offset < 0.0 || (clip.offset >= range.start && offset < range.start) || offset >= range.end => {
            offset = range.start + (offset - range.start).rem_euclid(range.end - range.start);
        }
        // A MIDI clip goes on before its notes (#832): its content grows at the start by as
        // much, its notes keeping where they play, so the clip starts with empty time. A content
        // other clips play too is copied first, so they are left as they are.
        _ if offset < 0.0 && !audio => {
            let grow = -offset;
            let shared = p.all_clips().filter(|c| c.content == clip.content).count() > 1;
            if shared {
                let mut copy = p.content(clip.content).cloned().ok_or("the clip's content is missing")?;
                copy.id = new_content_id(p)?;
                clip.content = copy.id;
                p.contents.push(copy);
            }
            if let Some(content) = p.contents.iter_mut().find(|c| c.id == clip.content) {
                content.length += grow;
                for lane in &mut content.lanes {
                    if let LaneData::Notes(notes) = &mut lane.data {
                        for note in notes {
                            note.time += grow;
                        }
                    }
                }
            }
            offset = 0.0;
        }
        _ if offset < 0.0 => {
            new_start -= offset / rate;
            offset = 0.0;
        }
        _ => {}
    }
    // The right edge: an unlooped audio clip ends where its audio does.
    if audio && !looped {
        new_end = new_end.min(new_start + (content_length - offset) / rate);
    }
    new_end = new_end.max(new_start + MIN_LENGTH);
    if !audio && !looped {
        // A MIDI clip made longer than its content takes the content with it.
        let needed = offset + (new_end - new_start) * rate;
        if let Some(content) = p.contents.iter_mut().find(|c| c.id == clip.content)
            && needed > content.length
        {
            content.length = needed;
        }
    }
    clip.start = new_start;
    clip.length = new_end - new_start;
    clip.offset = offset;
    clip.fit_fades();
    p.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?.clips.remove(index);
    let track = p.arrangement.lanes.get(lane).and_then(|l| l.track).map_or(0, |t| t.0);
    let index = put_back(p, lane, clip.clone(), stack)?;
    Ok(json!({ "track": track, "index": index, "start": clip.start, "end": clip.start + clip.length, "loop": looped }))
}

/// `clip.stretch {track, index, start?, end?, stack?}`: an audio clip's edges moved with its
/// audio stretched to fill the new length (time-stretched, so its pitch stays): the content's
/// warp markers are scaled, and a clip whose content others play gets a content of its own first.
pub(crate) fn stretch_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (start, end) = (edge(params, "start")?, edge(params, "end")?);
    let stack = Args::new(params).flag("stack")?;
    if start.is_none() && end.is_none() {
        return Err("give start, end, or both".into());
    }
    let (lane, index) = lane_and_index(&session.project, params)?;
    let p = &mut session.project;
    let mut clip =
        p.arrangement.lanes.get(lane).and_then(|l| l.clips.get(index)).cloned().ok_or("the clip is missing")?;
    if !is_audio(p, clip.content) {
        return Err("only an audio clip stretches; a MIDI clip's edges are moved with clip.trim".into());
    }
    let (new_start, new_end) = (start.unwrap_or(clip.start), end.unwrap_or(clip.start + clip.length));
    if new_end - new_start < MIN_LENGTH {
        return Err(format!("the clip would be shorter than {MIN_LENGTH} beats").into());
    }
    let factor = (new_end - new_start) / clip.length;
    // Played by another clip too: this one gets a copy, so the other keeps its speed.
    let shared = p.arrangement.lanes.iter().flat_map(|l| &l.clips).filter(|c| c.content == clip.content).count()
        + p.launcher
            .columns
            .iter()
            .flat_map(|c| &c.slots)
            .filter_map(|s| s.clip.as_ref())
            .filter(|c| c.content == clip.content)
            .count()
        > 1;
    if shared {
        let id = new_content_id(p)?;
        let content = p.content(clip.content).cloned().ok_or("the clip's content is missing")?;
        p.contents.push(suhina_model::ClipContent { id, ..content });
        clip.content = id;
    }
    let content = p.contents.iter_mut().find(|c| c.id == clip.content).ok_or("the clip's content is missing")?;
    let old_length = content.length;
    for lane in &mut content.lanes {
        if let LaneData::Audio(region) = &mut lane.data {
            if region.warp_markers.is_empty() {
                let seconds = region.duration_seconds.ok_or("the audio's length is not known, so it cannot stretch")?;
                region.warp_markers = vec![
                    WarpMarker { time: 0.0, audio_time: 0.0 },
                    WarpMarker { time: old_length, audio_time: seconds },
                ];
            }
            if !region.warped {
                region.warped = true;
                region.stretch = suhina_model::StretchMode::default();
            }
            for marker in &mut region.warp_markers {
                marker.time *= factor;
            }
        }
    }
    content.length = old_length * factor;
    clip.offset *= factor;
    clip.loop_range = clip.loop_range.map(|r| LoopRange { start: r.start * factor, end: r.end * factor });
    clip.fade_in *= factor;
    clip.fade_out *= factor;
    clip.start = new_start;
    clip.length = new_end - new_start;
    clip.fit_fades();
    p.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?.clips.remove(index);
    let track = p.arrangement.lanes.get(lane).and_then(|l| l.track).map_or(0, |t| t.0);
    let index = put_back(p, lane, clip.clone(), stack)?;
    Ok(
        json!({ "track": track, "index": index, "start": clip.start, "end": clip.start + clip.length, "factor": factor }),
    )
}
