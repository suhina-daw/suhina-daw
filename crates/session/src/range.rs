// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Editing a time selection in the arrangement: a span of beats over some tracks, as the
//! selection rectangle makes it. The commands act on the material inside the span, not on whole
//! clips: a clip that crosses an edge is split there first, as is usual with
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
    let name = p.shown_track_name(track);
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
        out.extend(lane.clips.iter().filter_map(|c| cut_to_span(p, c, start, end)).map(|(c, cc)| (track, c, cc)));
    }
    out
}

/// `clip` cut to the span and its content, when it has something inside the span.
fn cut_to_span(p: &Project, clip: &Clip, start: f64, end: f64) -> Option<(Clip, ClipContent)> {
    let (s, e) = (clip.start, clip.start + clip.length);
    if e <= start + 1e-9 || s >= end - 1e-9 {
        return None;
    }
    let content = p.content(clip.content).cloned()?;
    let from = s.max(start);
    let rate = content_rate(p, clip);
    let mut piece = clip.clone();
    if from > s {
        piece = split(&piece, from, rate).1;
    }
    if e > end {
        piece = split(&piece, end, rate).0;
    }
    Some((piece, content))
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
        let stack = stack && !p.takes_cover(track, at, at + copy.length);
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
/// `range.insertTime {start, end}` (#713): empty time as long as the span, at its start.
pub(crate) fn insert_time(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let start = number(params, "start")?;
    let end = number(params, "end")?;
    if start < 0.0 || end <= start + 1e-6 || end > suhina_engine::MAX_START_BEAT {
        return Err(
            format!("start and end must be from 0 to {}, end after start", suhina_engine::MAX_START_BEAT).into()
        );
    }
    let length = end - start;
    let after = |beat: f64| beat >= start - 1e-9;
    let p = &mut session.project;
    let last = p
        .arrangement
        .lanes
        .iter()
        .flat_map(|l| &l.clips)
        .map(|c| c.start)
        .chain(p.arrangement.markers.iter().map(|m| m.beat))
        .fold(0.0, f64::max);
    // The rest that moves later too: tempo changes, automation points and the loop's end, which
    // `transport.setLoop` and the automation commands also keep within the song's last beat.
    let automation: Vec<f64> = p
        .channels
        .iter()
        .flat_map(|channel| {
            let devices = suhina_model::walk_devices(&channel.devices);
            let device_lanes = devices.into_iter().flat_map(|(_, device)| &device.automation);
            channel.automation.iter().chain(device_lanes).flat_map(|lane| lane.points.iter().map(|pt| pt.beat))
        })
        .collect();
    let loop_end = p.transport.loop_range.as_ref().filter(|r| after(r.start) || r.end > start + 1e-9).map(|r| r.end);
    let last = p
        .transport
        .tempo_changes
        .iter()
        .map(|change| change.beat)
        .chain(automation)
        .filter(|beat| after(*beat))
        .chain(loop_end)
        .fold(last, f64::max);
    if last + length > suhina_engine::MAX_START_BEAT {
        return Err(format!("the song would run past beat {}", suhina_engine::MAX_START_BEAT).into());
    }
    // Clips crossing the start are split there; what starts from it on moves later.
    for lane in 0..p.arrangement.lanes.len() {
        split_lane_at(p, lane, start)?;
    }
    for clip in p.arrangement.lanes.iter_mut().flat_map(|l| &mut l.clips) {
        if after(clip.start) {
            clip.start += length;
        }
    }
    for marker in &mut p.arrangement.markers {
        if after(marker.beat) {
            marker.beat += length;
        }
    }
    let shift = |lanes: &mut Vec<suhina_model::AutomationLane>| {
        for point in lanes.iter_mut().flat_map(|l| &mut l.points) {
            if after(point.beat) {
                point.beat += length;
            }
        }
    };
    for channel in &mut p.channels {
        shift(&mut channel.automation);
        let paths: Vec<Vec<usize>> =
            suhina_model::walk_devices(&channel.devices).into_iter().map(|(path, _)| path).collect();
        for path in paths {
            if let Some(device) = suhina_model::device_at_mut(&mut channel.devices, &path) {
                shift(&mut device.automation);
            }
        }
    }
    for change in &mut p.transport.tempo_changes {
        if after(change.beat) && change.beat > 0.0 {
            change.beat += length;
        }
    }
    // Time-signature changes are on bars: moved when the length is whole bars of the meter there.
    let position = p.transport.meter().position(start);
    let bars = length / position.bar_beats;
    if position.bar_beats > 0.0 && (bars - bars.round()).abs() < 1e-6 && bars >= 1.0 {
        let on_bar = (position.bar_start - start).abs() < 1e-6;
        let first = if on_bar { position.bar } else { position.bar + 1 };
        let by = bars.round() as u32;
        for change in &mut p.transport.time_signature_changes {
            if change.bar >= first {
                change.bar = change.bar.saturating_add(by);
            }
        }
    }
    if let Some(range) = p.transport.loop_range.as_mut() {
        if after(range.start) {
            range.start += length;
            range.end += length;
        } else if range.end > start + 1e-9 {
            range.end += length;
        }
    }
    Ok(json!({ "start": start, "length": length }))
}

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
    let count = copy_parts(session, parts);
    Ok(json!({ "copied": count, "start": start, "end": end }))
}

/// Puts `parts` on the clip clipboard, with the automation under each, as `clip.paste` takes them.
fn copy_parts(session: &mut Session, parts: Vec<(TrackId, Clip, ClipContent)>) -> usize {
    let count = parts.len();
    session.clipboards.under = parts
        .iter()
        .map(|(track, clip, _)| crate::automation_range::under(session, *track, clip.start, clip.start + clip.length))
        .collect();
    session.clipboards.tracks = parts.iter().map(|(track, _, _)| *track).collect();
    session.clipboards.clips = parts.into_iter().map(|(_, clip, content)| (clip, content)).collect();
    count
}

/// What the take lanes `takes` (indices in `arrangement.takes`) of `track` hold inside the span,
/// cut to it, copied for `clip.paste` as `range.copy` copies a track's clips; answers how many.
pub(crate) fn copy_take_span(session: &mut Session, track: TrackId, takes: &[usize], start: f64, end: f64) -> usize {
    let p = &session.project;
    let parts: Vec<_> = takes
        .iter()
        .filter_map(|&index| p.arrangement.takes.get(index))
        .flat_map(|take| &take.clips)
        .filter_map(|clip| cut_to_span(p, clip, start, end))
        .map(|(clip, content)| (track, clip, content))
        .collect();
    copy_parts(session, parts)
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
    // The automation under each part goes under its copy (#916).
    let carried: Vec<_> = parts
        .iter()
        .map(|(track, clip, _)| {
            let lanes = crate::automation_range::under(session, *track, clip.start, clip.start + clip.length);
            (*track, clip.start + (end - start), lanes)
        })
        .collect();
    let placed = place(&mut session.project, parts, end - start, stack)?;
    for (track, at, lanes) in &carried {
        crate::automation_range::lay_under(session, *track, *at, lanes, true)?;
    }
    Ok(json!({ "clips": placed, "start": end, "end": end + (end - start) }))
}

/// `clip.paste {start, stack?}` with no track, after a copy that knows its tracks: each copied
/// clip goes back to the track it came from, the earliest at `start`.
pub(crate) fn paste_to_sources(session: &mut Session, start: f64, stack: bool) -> Result<Value, CommandError> {
    if session.clipboards.tracks.len() != session.clipboards.clips.len() || session.clipboards.clips.is_empty() {
        return Err("give a track: the copied clips do not know their tracks".into());
    }
    let earliest = session.clipboards.clips.iter().map(|(clip, _)| clip.start).fold(f64::INFINITY, f64::min);
    let offset = if earliest.is_finite() { start - earliest } else { 0.0 };
    let parts: Vec<(TrackId, Clip, ClipContent)> = session
        .clipboards
        .tracks
        .iter()
        .zip(session.clipboards.clips.iter())
        .map(|(track, (clip, content))| (*track, clip.clone(), content.clone()))
        .collect();
    let starts: Vec<(TrackId, f64)> = parts.iter().map(|(track, clip, _)| (*track, clip.start + offset)).collect();
    let placed = place(&mut session.project, parts, offset, stack)?;
    // The automation that was under each clip, under its copy on the same track.
    let under = session.clipboards.under.clone();
    for ((track, at), lanes) in starts.iter().zip(&under) {
        crate::automation_range::lay_under(session, *track, *at, lanes, true)?;
    }
    Ok(json!({ "clips": placed }))
}

/// What edits a clip about to move (ADR 0033 step 6): the key and device of the plug-in on its
/// track, its content and its audio (the pool key); `None` for a clip no plug-in edits.
type Edited = Option<(u64, usize, suhina_model::ContentId, String)>;

/// `clip.moveMany {clips, beats, toTrack?, tracksBy?, toNewTrack?, stack?}`: moves several
/// arrangement clips by the same number of beats at once, so none replaces another on the way;
/// `toTrack` takes them to another track when they are all on one, `tracksBy` takes each that
/// many tracks down (up when negative) from its own, and `toNewTrack` to a new track after the
/// last, of the clips' kind, made in the same step.
pub(crate) fn move_clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let chosen = clip_list(&session.project, params)?;
    let beats = number(params, "beats")?;
    let stack = params.get("stack").and_then(Value::as_bool).unwrap_or(false);
    let copy = params.get("copy").and_then(Value::as_bool).unwrap_or(false);
    let by = match params.get("tracksBy") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_i64().ok_or("tracksBy must be a whole number of tracks")?),
    };
    let new_track = params.get("toNewTrack").and_then(Value::as_bool).unwrap_or(false);
    // A clip a plug-in edits (ADR 0033), going to another track: its edits go along, the
    // plug-in added there when it has none (`keep`), or it starts there without them (`new`).
    let keep_edits = match params.get("araEdits") {
        None | Some(Value::Null) => false,
        Some(v) => match v.as_str() {
            Some("keep") => true,
            Some("new") => false,
            _ => return Err("araEdits must be keep or new".into()),
        },
    };
    // Each chosen clip's plug-in that edits it (its key and device), content and audio (the
    // pool key), before it moves.
    let edited: Vec<Edited> = chosen
        .iter()
        .map(|(track, index)| {
            if !keep_edits {
                return None;
            }
            let device = session.ara_device(*track)?;
            let key = crate::plugin_key_at(&session.project, *track, device)?;
            let lane = session.project.arrangement.lanes.iter().find(|l| l.track == Some(*track))?;
            let clip = lane.clips.get(*index)?;
            let content = session.project.content(clip.content)?;
            let source = content.lanes.iter().find_map(|l| match &l.data {
                suhina_model::LaneData::Audio(region) => Some(region.pool_key().into_owned()),
                suhina_model::LaneData::Notes(_) => None,
            })?;
            Some((key, device, clip.content, source))
        })
        .collect();
    if [params.get("toTrack").is_some_and(|v| !v.is_null()), by.is_some(), new_track].iter().filter(|b| **b).count() > 1
    {
        return Err("give one of toTrack, tracksBy and toNewTrack".into());
    }
    // Each clip's track after the move, by its own: the tracks that hold clips, in the order the
    // tracks are shown (a track added after another has its lane at the end of the lanes).
    let holders: Vec<TrackId> = session
        .project
        .tracks
        .iter()
        .filter(|t| t.content.notes || t.content.audio)
        .filter(|t| session.project.arrangement.lanes.iter().any(|l| l.track == Some(t.id)))
        .map(|t| t.id)
        .collect();
    let moved_by = |track: TrackId| -> Result<TrackId, CommandError> {
        let Some(by) = by else { return Ok(track) };
        let at = holders.iter().position(|t| *t == track).ok_or("the clip's track holds no clips")?;
        let to = i64::try_from(at).unwrap_or(i64::MAX).saturating_add(by);
        usize::try_from(to)
            .ok()
            .and_then(|to| holders.get(to).copied())
            .ok_or_else(|| "there is no track that far".into())
    };
    for (track, _) in &chosen {
        moved_by(*track)?;
    }
    let to = if new_track {
        // The clips' kind: audio clips make an audio track, MIDI clips a MIDI track.
        let audio = chosen.iter().any(|(track, index)| {
            let lane = session.project.arrangement.lanes.iter().find(|l| l.track == Some(*track));
            let clip = lane.and_then(|l| l.clips.get(*index));
            clip.and_then(|c| session.project.content(c.content))
                .is_some_and(|c| c.lanes.iter().any(|l| matches!(l.data, suhina_model::LaneData::Audio(_))))
        });
        let added = crate::files::add_track(session, &json!({ "kind": if audio { "audio" } else { "midi" } }))?;
        let id = added["track"].as_u64().and_then(|n| u32::try_from(n).ok()).ok_or("the new track has no id")?;
        Some(TrackId(id))
    } else {
        match params.get("toTrack") {
            Some(v) if !v.is_null() => Some(track_at(&session.project, params, "toTrack")?),
            _ => None,
        }
    };
    let keep = match params.get("color") {
        None | Some(Value::Null) => None,
        Some(_) => crate::clip_color::parse_color(params, "color")?,
    };
    if to.is_some() && !new_track && chosen.iter().any(|(track, _)| *track != chosen[0].0) {
        return Err("toTrack moves clips of one track; these are on several".into());
    }
    // An empty track of the other kind takes the clips' kind (#945).
    if let Some(target) = to.filter(|t| !new_track && chosen.iter().all(|(track, _)| track != t))
        && let Some(content) = chosen.first().and_then(|(track, index)| {
            let lane = session.project.arrangement.lanes.iter().find(|l| l.track == Some(*track));
            lane.and_then(|l| l.clips.get(*index)).map(|c| c.content)
        })
    {
        crate::track_kind::fit_empty_for(session, target, content)?;
    }
    // The automation under each clip, taken along (#916): from where it was (left there for a
    // copy) to where the clip lands.
    let carried: Vec<(TrackId, f64, f64, Vec<crate::automation_range::Under>)> = chosen
        .iter()
        .filter_map(|(track, index)| {
            let lane = session.project.arrangement.lanes.iter().find(|l| l.track == Some(*track))?;
            let clip = lane.clips.get(*index)?;
            let end = clip.start + clip.length;
            Some((*track, clip.start, end, crate::automation_range::under(session, *track, clip.start, end)))
        })
        .collect();
    let p = &mut session.project;
    // Taken out first, the highest index of each lane first so the others keep theirs.
    let mut taken: Vec<(TrackId, Clip)> = Vec::with_capacity(chosen.len());
    // Where each taken clip came from and what edited it there, in the same order.
    let mut came: Vec<(TrackId, Edited)> = Vec::new();
    let mut order = chosen.clone();
    order.sort_by_key(|c| std::cmp::Reverse(c.1));
    for (track, index) in order {
        let at = chosen.iter().position(|c| *c == (track, index));
        came.push((track, at.and_then(|at| edited.get(at).cloned().flatten())));
        let lane = p.arrangement.lanes.iter_mut().find(|l| l.track == Some(track)).ok_or("the track has no lane")?;
        // Copied, the clip stays where it is; its copy plays a content of its own.
        let clip =
            if copy { lane.clips.get(index).cloned().ok_or("the clip is missing")? } else { lane.clips.remove(index) };
        if clip.start + beats < -1e-9 {
            return Err(format!("a clip would start at beat {}, before the start", clip.start + beats).into());
        }
        let mut clip = clip;
        let goes = to.unwrap_or(moved_by(track)?);
        if goes != track && clip.color.is_none() {
            clip.color = keep;
        }
        taken.push((goes, clip));
    }
    for (goes, clip) in &taken {
        {
            let target = p.track(*goes).ok_or("the target track is missing")?;
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
    // Parts of a comp take what their takes hold under them along (#1078).
    let mut take_spans: Vec<crate::take_carry::Span> = Vec::new();
    for ((goes, clip), (from, _)) in taken.iter().zip(&came) {
        take_spans.extend(crate::take_carry::spans_of_part(p, *from, *goes, clip));
    }
    if copy {
        for (_, clip) in &mut taken {
            let id = new_content_id(p)?;
            let content = p.content(clip.content).cloned().ok_or("the clip's content is missing")?;
            p.contents.push(ClipContent { id, ..content });
            for span in take_spans.iter_mut().filter(|s| s.is_of(clip)) {
                span.remap = Some((clip.content, id));
            }
            clip.content = id;
            // A copy is a new clip (ADR 0025); a moved clip keeps its id.
            clip.id = p.new_clip_id()?;
        }
    }
    let mut placed = Vec::with_capacity(taken.len());
    came.reverse();
    for (track, mut clip) in taken.into_iter().rev() {
        clip.start = (clip.start + beats).max(0.0);
        let stack = stack && !p.takes_cover(track, clip.start, clip.start + clip.length);
        let lane = lane_of(p, track)?;
        let lane = p.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?;
        if !stack {
            make_room(&mut lane.clips, clip.start, clip.start + clip.length);
        }
        lane.clips.push(clip.clone());
        lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        placed.push((track, clip));
    }
    if crate::take_carry::carry(p, &take_spans, beats, copy) {
        session.take_solo = None;
    }
    if !copy {
        for (track, start, end, lanes) in &carried {
            crate::automation_range::clear_under(session, *track, *start, *end, lanes)?;
        }
    }
    for (track, start, _, lanes) in &carried {
        let goes = to.unwrap_or(moved_by(*track)?);
        crate::automation_range::lay_under(session, goes, (start + beats).max(0.0), lanes, goes == *track)?;
    }
    // The edits of clips that left their plug-in's track go along (ADR 0033 step 6).
    let mut carried = 0;
    for ((track, clip), (from_track, origin)) in placed.iter().zip(&came) {
        let Some((key, device, content, source)) = origin else { continue };
        if *track == *from_track {
            continue;
        }
        add_editor_like(session, *from_track, *device, *track)?;
        session.plugins.carry(
            *key,
            *track,
            (format!("content-{}", content.0), format!("content-{}", clip.content.0)),
            source.clone(),
            format!("clip-{}", clip.id.0),
        );
        carried += 1;
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
    Ok(json!({ "clips": clips, "edits_carried": carried }))
}

/// Puts on `to`'s channel, first, the plug-in that edits clips on `from` (device `device`)
/// when `to` has no such plug-in yet: a clip moved there with its edits kept is edited there by
/// the same plug-in (ADR 0033 step 6). It starts without the other's state; the edits come by
/// the carry.
fn add_editor_like(session: &mut Session, from: TrackId, device: usize, to: TrackId) -> Result<(), CommandError> {
    let p = &mut session.project;
    let channel_of = |p: &suhina_model::Project, track: TrackId| p.track(track)?.channel;
    let source = channel_of(p, from)
        .and_then(|c| p.channel(c))
        .and_then(|c| c.devices.get(device))
        .map(|d| (d.name.clone(), d.kind.clone()))
        .ok_or("the plug-in that edits the clip is missing")?;
    let target = channel_of(p, to).ok_or("the track has no channel")?;
    let channel = p.channel_mut(target).ok_or("the track has no channel")?;
    if channel.devices.iter().any(|d| d.kind == source.1) {
        return Ok(());
    }
    channel.devices.insert(0, suhina_model::Device::new(source.0, source.1));
    Ok(())
}
