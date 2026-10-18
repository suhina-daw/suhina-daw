// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Editing a time selection in the arrangement: a span of beats over some tracks, as the
//! selection rectangle makes it. The commands act on the material inside the span, not on whole
//! clips: a clip that crosses an edge is split there first, as Ableton Live and Studio One do with
//! a time selection (`docs/spec/arrangement-editing.md` › Time selection). Each is one undo step.

use serde_json::{Value, json};
use suhina_model::{ArrangementLane, Clip, ClipContent, Project, TrackId};

use crate::commands::{clip_list, make_room, new_content_id, number, track_at, track_list};
use crate::editing::{content_rate, split};
use crate::{CommandError, Session};

/// The span a command names: `tracks`, and `start` before `end`, both at least 0.
fn span(p: &Project, params: &Value) -> Result<(Vec<TrackId>, f64, f64), CommandError> {
    let tracks = track_list(p, params)?;
    let (start, end) = (number(params, "start")?, number(params, "end")?);
    if !(start >= 0.0 && end > start && end.is_finite()) {
        return Err(format!("start and end must be beats with 0 ≤ start < end, got {start} and {end}").into());
    }
    Ok((tracks, start, end))
}

/// The arrangement lane of `track`, made (with an id of its own) when it has none.
fn lane_of(p: &mut Project, track: TrackId) -> Result<usize, CommandError> {
    if let Some(lane) = p.arrangement.lanes.iter().position(|l| l.track == Some(track)) {
        return Ok(lane);
    }
    let name = p.tracks.iter().find(|t| t.id == track).map(|t| t.name.clone()).unwrap_or_default();
    let id = p.new_lane_id()?;
    p.arrangement.lanes.push(ArrangementLane { id, name, track: Some(track), clips: Vec::new() });
    Ok(p.arrangement.lanes.len() - 1)
}

/// Splits every clip of lane `lane` that crosses `at`; each second part is a new clip with an id
/// of its own (ADR 0025).
fn split_lane_at(p: &mut Project, lane: usize, at: f64) -> Result<(), CommandError> {
    let clips: Vec<Clip> = p.arrangement.lanes.get(lane).map(|l| l.clips.clone()).unwrap_or_default();
    let mut out = Vec::with_capacity(clips.len() + 1);
    for clip in clips {
        if clip.start + 1e-6 < at && at < clip.start + clip.length - 1e-6 {
            let rate = content_rate(p, &clip);
            let (first, mut second) = split(&clip, at, rate);
            second.id = p.new_clip_id()?;
            out.push(first);
            out.push(second);
        } else {
            out.push(clip);
        }
    }
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    if let Some(l) = p.arrangement.lanes.get_mut(lane) {
        l.clips = out;
    }
    Ok(())
}

/// The clips of the span, each cut to it, with their contents and tracks: what a copy holds.
fn pieces(p: &Project, tracks: &[TrackId], start: f64, end: f64) -> Vec<(TrackId, Clip, ClipContent)> {
    let mut out = Vec::new();
    for &track in tracks {
        let Some(lane) = p.arrangement.lanes.iter().find(|l| l.track == Some(track)) else { continue };
        for clip in &lane.clips {
            let (s, e) = (clip.start, clip.start + clip.length);
            if e <= start + 1e-9 || s >= end - 1e-9 {
                continue;
            }
            let Some(content) = p.content(clip.content).cloned() else { continue };
            let from = s.max(start);
            let rate = content_rate(p, clip);
            let mut piece = clip.clone();
            if from > s {
                piece = split(&piece, from, rate).1;
            }
            if e > end {
                piece = split(&piece, end, rate).0;
            }
            out.push((track, piece, content));
        }
    }
    out
}

/// Places copies of `pieces` (each on its own track), moved by `offset` beats, replacing what
/// they cover unless `stack`; each copy plays content of its own. Returns where they went.
fn place(
    p: &mut Project,
    pieces: Vec<(TrackId, Clip, ClipContent)>,
    offset: f64,
    stack: bool,
) -> Result<Vec<Value>, CommandError> {
    let mut placed = Vec::with_capacity(pieces.len());
    for (track, clip, content) in pieces {
        let id = new_content_id(p)?;
        p.contents.push(ClipContent { id, ..content });
        let at = clip.start + offset;
        let mut copy = Clip { id: p.new_clip_id()?, content: id, start: at, ..clip };
        copy.fit_fades();
        let lane = lane_of(p, track)?;
        let lane = p.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?;
        if !stack {
            make_room(&mut lane.clips, at, at + copy.length);
        }
        lane.clips.push(copy.clone());
        lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        let index = lane.clips.iter().position(|c| c.id == copy.id).unwrap_or_default();
        placed.push(json!({ "clip": copy.id.0, "track": track.0, "index": index, "start": at }));
    }
    Ok(placed)
}

/// The clips lying inside the span after a split, as `{clip, track, index}`.
fn inside(p: &Project, tracks: &[TrackId], start: f64, end: f64) -> Vec<Value> {
    let mut out = Vec::new();
    for &track in tracks {
        let Some(lane) = p.arrangement.lanes.iter().find(|l| l.track == Some(track)) else { continue };
        for (index, clip) in lane.clips.iter().enumerate() {
            if clip.start >= start - 1e-6 && clip.start + clip.length <= end + 1e-6 {
                out.push(json!({ "clip": clip.id.0, "track": track.0, "index": index }));
            }
        }
    }
    out
}

/// `range.split {tracks, start, end}`: the clips crossing either edge are cut there, so the span
/// is clips of its own; answers with the clips inside it.
pub(crate) fn split_range(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (tracks, start, end) = span(&session.project, params)?;
    let p = &mut session.project;
    for &track in &tracks {
        let Some(lane) = p.arrangement.lanes.iter().position(|l| l.track == Some(track)) else { continue };
        split_lane_at(p, lane, start)?;
        split_lane_at(p, lane, end)?;
    }
    Ok(json!({ "clips": inside(p, &tracks, start, end) }))
}

/// `range.delete {tracks, start, end}`: what plays inside the span goes; the clips crossing an
/// edge keep their parts outside it, and the gap stays.
pub(crate) fn delete_range(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (tracks, start, end) = span(&session.project, params)?;
    let p = &mut session.project;
    let mut removed = 0usize;
    for &track in &tracks {
        let Some(lane) = p.arrangement.lanes.iter_mut().find(|l| l.track == Some(track)) else { continue };
        let before = lane.clips.len();
        make_room(&mut lane.clips, start, end);
        removed += before.saturating_sub(lane.clips.len());
    }
    Ok(
        json!({ "start": start, "end": end, "tracks": tracks.iter().map(|t| t.0).collect::<Vec<_>>(), "removed": removed }),
    )
}

/// `range.copy {tracks, start, end}`: copies what plays inside the span, cut to it, for
/// `clip.paste`; a paste with no track puts each part back on its own track.
pub(crate) fn copy_range(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (tracks, start, end) = span(&session.project, params)?;
    let parts = pieces(&session.project, &tracks, start, end);
    if parts.is_empty() {
        return Err("nothing plays in the selection".into());
    }
    let count = parts.len();
    session.clipboard_tracks = parts.iter().map(|(track, _, _)| *track).collect();
    session.clipboard = parts.into_iter().map(|(_, clip, content)| (clip, content)).collect();
    Ok(json!({ "copied": count, "start": start, "end": end }))
}

/// `range.cut {tracks, start, end}`: `range.copy`, then `range.delete`, as one undo step.
pub(crate) fn cut_range(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let copied = copy_range(session, params)?;
    delete_range(session, params)?;
    Ok(copied)
}

/// `range.duplicate {tracks, start, end, stack?}`: a copy of the span goes right after it, on
/// the same tracks; answers with the copies.
pub(crate) fn duplicate_range(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (tracks, start, end) = span(&session.project, params)?;
    let stack = params.get("stack").and_then(Value::as_bool).unwrap_or(false);
    let parts = pieces(&session.project, &tracks, start, end);
    if parts.is_empty() {
        return Err("nothing plays in the selection".into());
    }
    let placed = place(&mut session.project, parts, end - start, stack)?;
    Ok(json!({ "clips": placed, "start": end, "end": end + (end - start) }))
}

/// `clip.paste {start, stack?}` with no track, after a copy that knows its tracks: each copied
/// clip goes back to the track it came from, the earliest at `start`.
pub(crate) fn paste_to_sources(session: &mut Session, start: f64, stack: bool) -> Result<Value, CommandError> {
    if session.clipboard_tracks.len() != session.clipboard.len() || session.clipboard.is_empty() {
        return Err("give a track: the copied clips do not know their tracks".into());
    }
    let earliest = session.clipboard.iter().map(|(clip, _)| clip.start).fold(f64::INFINITY, f64::min);
    let offset = if earliest.is_finite() { start - earliest } else { 0.0 };
    let parts: Vec<(TrackId, Clip, ClipContent)> = session
        .clipboard_tracks
        .iter()
        .zip(session.clipboard.iter())
        .map(|(track, (clip, content))| (*track, clip.clone(), content.clone()))
        .collect();
    let placed = place(&mut session.project, parts, offset, stack)?;
    Ok(json!({ "clips": placed }))
}

/// `clip.moveMany {clips, beats, to_track?, stack?}`: moves several arrangement clips by the
/// same number of beats at once, so none replaces another on the way; `to_track` takes them to
/// another track when they are all on one.
pub(crate) fn move_clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let chosen = clip_list(&session.project, params)?;
    let beats = number(params, "beats")?;
    let stack = params.get("stack").and_then(Value::as_bool).unwrap_or(false);
    let copy = params.get("copy").and_then(Value::as_bool).unwrap_or(false);
    let to = match params.get("to_track") {
        Some(_) => Some(track_at(&session.project, params, "to_track")?),
        None => None,
    };
    let keep = match params.get("color") {
        None | Some(Value::Null) => None,
        Some(_) => crate::clip_color::parse_color(params, "color")?,
    };
    if to.is_some() && chosen.iter().any(|(track, _)| *track != chosen[0].0) {
        return Err("to_track moves clips of one track; these are on several".into());
    }
    let p = &mut session.project;
    // Taken out first, the highest index of each lane first so the others keep theirs.
    let mut taken: Vec<(TrackId, Clip)> = Vec::with_capacity(chosen.len());
    let mut order = chosen.clone();
    order.sort_by_key(|c| std::cmp::Reverse(c.1));
    for (track, index) in order {
        let lane = p.arrangement.lanes.iter_mut().find(|l| l.track == Some(track)).ok_or("the track has no lane")?;
        // Copied, the clip stays where it is; its copy plays a content of its own.
        let clip =
            if copy { lane.clips.get(index).cloned().ok_or("the clip is missing")? } else { lane.clips.remove(index) };
        if clip.start + beats < -1e-9 {
            return Err(format!("a clip would start at beat {}, before the start", clip.start + beats).into());
        }
        let mut clip = clip;
        if to.is_some_and(|to| to != track) && clip.color.is_none() {
            clip.color = keep;
        }
        taken.push((to.unwrap_or(track), clip));
    }
    if let Some(to) = to {
        let target = p.tracks.iter().find(|t| t.id == to).ok_or("the target track is missing")?;
        for (_, clip) in &taken {
            let kinds = p.content(clip.content).map_or((false, false), |c| {
                (
                    c.lanes.iter().any(|l| matches!(l.data, suhina_model::LaneData::Notes(_))),
                    c.lanes.iter().any(|l| matches!(l.data, suhina_model::LaneData::Audio(_))),
                )
            });
            if (kinds.0 && !target.content.notes) || (kinds.1 && !target.content.audio) {
                return Err(format!("track {:?} cannot hold these clips", target.name).into());
            }
        }
    }
    if copy {
        for (_, clip) in &mut taken {
            let id = new_content_id(p)?;
            let content = p.content(clip.content).cloned().ok_or("the clip's content is missing")?;
            p.contents.push(ClipContent { id, ..content });
            clip.content = id;
            // A copy is a new clip (ADR 0025); a moved clip keeps its id.
            clip.id = p.new_clip_id()?;
        }
    }
    let mut placed = Vec::with_capacity(taken.len());
    for (track, mut clip) in taken.into_iter().rev() {
        clip.start = (clip.start + beats).max(0.0);
        let lane = lane_of(p, track)?;
        let lane = p.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?;
        if !stack {
            make_room(&mut lane.clips, clip.start, clip.start + clip.length);
        }
        lane.clips.push(clip.clone());
        lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        placed.push((track, clip));
    }
    let p = &session.project;
    let clips: Vec<Value> = placed
        .iter()
        .filter_map(|(track, clip)| {
            let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(*track))?;
            let index = lane.clips.iter().position(|c| c.id == clip.id)?;
            Some(json!({ "clip": clip.id.0, "track": track.0, "index": index, "start": clip.start }))
        })
        .collect();
    Ok(json!({ "clips": clips }))
}
