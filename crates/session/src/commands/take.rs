// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `take.*` commands: the take lanes a recording into the arrangement keeps under a track,
//! and comping parts of them onto it (`docs/spec/recording.md` › Take lanes, #909).

use super::*;

const TAKE_LIST: Param = param("takes", ParamType::Integers, "the takes, their numbers on the track, from 1");
const AREA_START: Param = param("start", ParamType::Number, "where the area starts, in beats");
const AREA_END: Param = param("end", ParamType::Number, "where the area ends, in beats");

/// The least length a piece of a take is left with.
const MIN_PIECE: f64 = 1.0 / 64.0;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "take.comp",
        label: "Copy a recorded take, or the part of it from start to end, onto its track, over what is there (comping)",
        params: &[
            TRACK,
            param("take", ParamType::Integer, "the take's number on the track, from 1, as the take lanes show"),
            optional("start", ParamType::Number, "where the part starts, in beats (default: where the take starts)"),
            optional("end", ParamType::Number, "where the part ends, in beats (default: where the take ends)"),
            optional(
                "color",
                ParamType::Text,
                "the colour the part takes on the track, \"#rrggbb\" (the take lane's), so the track shows which take each part came from",
            ),
        ],
        kind: CommandKind::Edit,
        run: comp_take,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "take.setColor",
        label: "Set a take lane's colour and the colour of its parts on the track (Recolor lane), or one piece of the take's and its parts' (Recolor); null gives them the lane's or the palette's again",
        params: &[
            TRACK,
            param("take", ParamType::Integer, "the take's number on the track, from 1"),
            param("color", ParamType::Text, "\"#rrggbb\", or null"),
            optional(
                "piece",
                ParamType::Integer,
                "only this piece of the take, from 1 in time order (a split take has several), and its parts on the track",
            ),
        ],
        kind: CommandKind::Edit,
        run: set_take_color,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "take.swap",
        label: "Replace a comped part on a track with the same span of the take above or below, in turn round the takes that have something there",
        params: &[
            param("clip", ParamType::Integer, "the comped part's clip id, as project.clips lists it"),
            param("direction", ParamType::Text, "up or down"),
        ],
        kind: CommandKind::Edit,
        run: swap_take,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "take.moveCut",
        label: "Move the cut between a comped part and the part right after it: one gets longer, the other shorter, each playing its own take",
        params: &[
            param("clip", ParamType::Integer, "the clip id of the part before the cut"),
            param("to", ParamType::Number, "where the cut goes, in beats"),
        ],
        kind: CommandKind::Edit,
        run: move_cut,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "take.setCrossfade",
        label: "Set how long the crossfade is at the cut between a comped part and the part of another take right after it; without a length, the length in Settings",
        params: &[
            param("clip", ParamType::Integer, "the clip id of the part before the cut"),
            optional("length", ParamType::Number, "the whole crossfade, in beats"),
        ],
        kind: CommandKind::Edit,
        run: set_crossfade,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "take.solo",
        label: "Hear a take lane in place of its track's clips (its S button), or the track again with take 0; not saved and not an undo step",
        params: &[TRACK, param("take", ParamType::Integer, "the take's number on the track, from 1, or 0 for none")],
        kind: CommandKind::Transport,
        run: solo_take,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "take.promote",
        label: "Put a whole take on its track over what is there (its up arrow), its pieces as they are",
        params: &[TRACK, param("take", ParamType::Integer, "the take's number on the track, from 1")],
        kind: CommandKind::Edit,
        run: promote_take,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "take.deleteUnused",
        label: "Delete a track's take lanes that its comp does not use; the track's clips stay",
        params: &[TRACK],
        kind: CommandKind::Edit,
        run: delete_unused_takes,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "take.deletePieces",
        label: "Delete pieces of a take (a split take has several); a take left with none goes; the track's clips stay",
        params: &[
            TRACK,
            param("take", ParamType::Integer, "the take's number on the track, from 1"),
            param("pieces", ParamType::Integers, "the pieces, numbered from 1 in time order"),
        ],
        kind: CommandKind::Edit,
        run: delete_pieces,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "take.deleteRange",
        label: "Delete what some take lanes of a track hold between start and end (an area painted across them); a take left with nothing goes; the track's clips stay",
        params: &[TRACK, TAKE_LIST, AREA_START, AREA_END],
        kind: CommandKind::Edit,
        run: delete_range,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "take.cutRange",
        label: "Copy what some take lanes of a track hold between start and end for clip.paste and delete it, as one undo step",
        params: &[TRACK, TAKE_LIST, AREA_START, AREA_END],
        kind: CommandKind::Edit,
        run: cut_range,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "take.movePieces",
        label: "Move pieces of a track's takes along the timeline, each on its own take lane, over what they cover there; the parts already on the track stay as they are",
        params: &[
            TRACK,
            param("takes", ParamType::Integers, "the take of each piece, its number on the track, from 1"),
            param(
                "pieces",
                ParamType::Integers,
                "the pieces, from 1 in time order within their takes, one per entry of takes",
            ),
            param("by", ParamType::Number, "how far, in beats (the pieces stop at the start of the timeline)"),
        ],
        kind: CommandKind::Edit,
        run: move_pieces,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "take.trimPiece",
        label: "Move the edges of a piece of a take, bringing back the recording around it: it stops where its recording does and at the take's other pieces",
        params: &[
            TRACK,
            param("take", ParamType::Integer, "the take's number on the track, from 1"),
            param("piece", ParamType::Integer, "the piece, from 1 in time order"),
            optional("start", ParamType::Number, "where its left edge goes, in beats"),
            optional("end", ParamType::Number, "where its right edge goes, in beats"),
        ],
        kind: CommandKind::Edit,
        run: trim_piece,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "take.delete",
        label: "Delete a recorded take lane of a track; the track's clips stay",
        params: &[TRACK, param("take", ParamType::Integer, "the take's number on the track, from 1")],
        kind: CommandKind::Edit,
        run: delete_take,
        // Takes do not play: deleting one changes only what is shown.
        policy: Policy::UNHEARD,
    },
];

/// The index in `project.arrangement.takes` of take number `take` (from 1) of `track`.
fn take_index(p: &Project, track: TrackId, params: &Value) -> Result<usize, CommandError> {
    let number = params.get("take").and_then(Value::as_u64).ok_or("take must be a number from 1")?;
    index_of_take(p, track, number)
}

/// The index in `project.arrangement.takes` of take `number` of `track`, the number its lane
/// shows ([`Project::take_number`]).
fn index_of_take(p: &Project, track: TrackId, number: u64) -> Result<usize, CommandError> {
    let wanted = usize::try_from(number).ok().filter(|n| *n >= 1).ok_or("take must be a number from 1")?;
    (0..p.arrangement.takes.len())
        .find(|&i| p.arrangement.takes.get(i).is_some_and(|t| t.track == track) && p.take_number(i) == Some(wanted))
        .ok_or_else(|| format!("the track has no take {number}").into())
}

/// The indices in `project.arrangement.takes` of the takes `params` lists under `takes`, once each
/// and in order.
fn listed_takes(p: &Project, track: TrackId, params: &Value) -> Result<Vec<usize>, CommandError> {
    let list = params.get("takes").and_then(Value::as_array).ok_or("takes must be a list")?;
    let mut indices = Vec::new();
    for entry in list {
        let number = entry.as_u64().ok_or("each take is a number from 1")?;
        indices.push(index_of_take(p, track, number)?);
    }
    indices.sort_unstable();
    indices.dedup();
    Ok(indices)
}

/// The `start` and `end` of an area across take lanes, in beats.
fn area(params: &Value) -> Result<(f64, f64), CommandError> {
    let beat = |key: &str| -> Result<f64, CommandError> {
        params
            .get(key)
            .and_then(Value::as_f64)
            .filter(|b| b.is_finite() && (0.0..=MAX_START_BEAT).contains(b))
            .ok_or_else(|| format!("{key} must be a beat from 0 to {MAX_START_BEAT}").into())
    };
    let (start, end) = (beat("start")?, beat("end")?);
    if end <= start {
        return Err("end must be after start".into());
    }
    Ok((start, end))
}

/// `take.comp`: the take's clips, cut to `start` … `end`, laid on the track over what they
/// cover, as new clips (docs/spec/recording.md › Take lanes).
fn comp_take(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let fade = session.take_crossfade_beats();
    let p = &mut session.project;
    let track = track_id(p, params)?;
    let index = take_index(p, track, params)?;
    let take = p.arrangement.takes.get(index).ok_or("the take is missing")?;
    let first = take.clips.iter().map(|c| c.start).fold(f64::INFINITY, f64::min);
    let last = take.clips.iter().map(|c| c.start + c.length).fold(f64::NEG_INFINITY, f64::max);
    let name = take.name.clone();
    let bound = |key: &str, default: f64| -> Result<f64, CommandError> {
        match params.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(v) => v
                .as_f64()
                .filter(|b| b.is_finite() && (0.0..=MAX_START_BEAT).contains(b))
                .ok_or_else(|| format!("{key} must be a beat from 0 to {MAX_START_BEAT}").into()),
        }
    };
    let (from, to) = (bound("start", first)?, bound("end", last)?);
    if to.partial_cmp(&from) != Some(std::cmp::Ordering::Greater) {
        return Err("end must be after start".into());
    }
    let color = match params.get("color") {
        None | Some(Value::Null) => None,
        Some(_) => crate::clip_color::parse_color(params, "color")?,
    };
    let clips = comp_part(p, track, index, from, to, color, fade)?;
    Ok(json!({ "track": track.0, "take": name, "start": from, "end": to, "clips": clips }))
}

/// Lays the part of take `index` (in `arrangement.takes`) from `from` to `to` on `track`'s lane,
/// over what it covers, as new clips in `color`, or the take's own colour when it has one; a part
/// with neither shows its take's colour in the window. An audio part gets fades of `fade` beats,
/// so its edges never click and it crossfades that long with a part of another take it meets
/// (#1078). Returns the clips' ids.
fn comp_part(
    p: &mut Project,
    track: TrackId,
    index: usize,
    from: f64,
    to: f64,
    color: Option<suhina_model::Rgb>,
    fade: f64,
) -> Result<Vec<u32>, CommandError> {
    let take = p.arrangement.takes.get(index).ok_or("the take is missing")?.clone();
    let color = color.or(take.color);
    let mut pieces = Vec::new();
    for clip in &take.clips {
        let (start, end) = (clip.start.max(from), (clip.start + clip.length).min(to));
        if end - start < 1e-9 {
            continue;
        }
        let offset = content_at(clip, start - clip.start, 1.0);
        let mut piece = Clip { id: p.new_clip_id()?, start, length: end - start, offset, ..clip.clone() };
        let audio =
            p.content(piece.content).is_some_and(|c| c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))));
        piece.fade_in = if audio { fade } else { 0.0 };
        piece.fade_out = piece.fade_in;
        piece.fit_fades();
        if color.is_some() {
            piece.color = color;
        }
        pieces.push(piece);
    }
    if pieces.is_empty() {
        return Err("the take has nothing between start and end".into());
    }
    let lane = match p.arrangement.lanes.iter().position(|l| l.track == Some(track)) {
        Some(lane) => lane,
        None => {
            let name = p.track(track).map(|t| t.name.clone()).unwrap_or_default();
            let id = p.new_lane_id()?;
            p.arrangement.lanes.push(ArrangementLane { id, name, track: Some(track), clips: Vec::new() });
            p.arrangement.lanes.len() - 1
        }
    };
    let lane = p.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?;
    let clips: Vec<u32> = pieces.iter().map(|c| c.id.0).collect();
    for piece in pieces {
        make_room(&mut lane.clips, piece.start, piece.start + piece.length);
        lane.clips.push(piece);
    }
    lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    // The second parts of clips the pieces split get their ids (ADR 0025).
    p.assign_ids()?;
    Ok(clips)
}

/// `take.setColor {track, take, color, piece?}`: the take lane's colour and its parts' on the
/// track (Recolor lane, #989), its pieces' own colours given up; with `piece` (from 1, a split
/// take has several), that piece's colour and its parts' alone (Recolor). `null` gives them the
/// lane's or the palette's colour again.
fn set_take_color(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let track = track_id(p, params)?;
    let index = take_index(p, track, params)?;
    let color = crate::clip_color::parse_color(params, "color")?;
    let piece = match params.get("piece") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let n = v.as_u64().filter(|n| *n >= 1).ok_or("piece must be a number from 1")?;
            let n = usize::try_from(n - 1).map_err(|_| "piece is out of range")?;
            let pieces = p.arrangement.takes.get(index).map_or(0, |t| t.clips.len());
            if n >= pieces {
                return Err(format!("the take has {pieces} pieces").into());
            }
            Some(n)
        }
    };
    let parts: Vec<suhina_model::ClipId> = p
        .arrangement
        .lanes
        .iter()
        .filter(|l| l.track == Some(track))
        .flat_map(|l| &l.clips)
        .filter(|c| match piece {
            Some(n) => p.comp_piece(track, c) == Some((index, n)),
            None => p.comp_source(track, c) == Some(index),
        })
        .map(|c| c.id)
        .collect();
    if let Some(take) = p.arrangement.takes.get_mut(index) {
        match piece {
            Some(n) => {
                if let Some(clip) = take.clips.get_mut(n) {
                    clip.color = color;
                }
            }
            None => {
                take.color = color;
                for clip in &mut take.clips {
                    clip.color = None;
                }
            }
        }
    }
    for clip in p.arrangement.lanes.iter_mut().flat_map(|l| l.clips.iter_mut()).filter(|c| parts.contains(&c.id)) {
        clip.color = color;
    }
    Ok(json!({ "track": track.0, "parts": parts.len() }))
}

/// `take.swap {clip, direction}`: the comped part `clip` replaced by the same span of the take
/// above (`up`) or below (`down`) on its track, in turn round the takes that have something
/// there (#989).
fn swap_take(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let fade = session.take_crossfade_beats();
    let p = &mut session.project;
    let id = params.get("clip").and_then(Value::as_u64).ok_or("clip must be a clip id")?;
    let (track, clip) = p
        .arrangement
        .lanes
        .iter()
        .find_map(|l| Some((l.track?, l.clips.iter().find(|c| u64::from(c.id.0) == id)?.clone())))
        .ok_or_else(|| format!("clip {id} is not on the arrangement"))?;
    let up = match params.get("direction").and_then(Value::as_str) {
        Some("up") => true,
        Some("down") => false,
        _ => return Err("direction must be up or down".into()),
    };
    let source = p.comp_source(track, &clip).ok_or("the clip was not comped from a take")?;
    let (start, end) = (clip.start, clip.start + clip.length);
    let takes: Vec<usize> = (0..p.arrangement.takes.len())
        .filter(|&i| {
            p.arrangement.takes.get(i).is_some_and(|t| {
                t.track == track && t.clips.iter().any(|c| c.start < end - 1e-9 && start + 1e-9 < c.start + c.length)
            })
        })
        .collect();
    let at = takes.iter().position(|&i| i == source).ok_or("the clip's take is missing")?;
    let next = if up { (at + takes.len() - 1) % takes.len() } else { (at + 1) % takes.len() };
    let target = *takes.get(next).ok_or("no other take")?;
    if target == source {
        return Err("no other take has something there".into());
    }
    let lane = p.arrangement.lanes.iter_mut().find(|l| l.track == Some(track)).ok_or("the lane is missing")?;
    lane.clips.retain(|c| u64::from(c.id.0) != id);
    let clips = comp_part(p, track, target, start, end, None, fade)?;
    let name = p.arrangement.takes.get(target).map(|t| t.name.clone()).unwrap_or_default();
    Ok(json!({ "track": track.0, "take": name, "clips": clips }))
}

/// `take.moveCut {clip, to}`: the cut between comped part `clip` and the part right after it moves
/// to `to`: one gets longer, the other shorter, each playing its own take (#989).
fn move_cut(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let id = params.get("clip").and_then(Value::as_u64).ok_or("clip must be a clip id")?;
    let to = params.get("to").and_then(Value::as_f64).filter(|t| t.is_finite()).ok_or("to must be a beat")?;
    let p = &session.project;
    let (track, left, right) = p
        .arrangement
        .lanes
        .iter()
        .find_map(|l| {
            let left = l.clips.iter().find(|c| u64::from(c.id.0) == id)?;
            let cut = left.start + left.length;
            let right = l.clips.iter().find(|c| (c.start - cut).abs() < 1e-6 && c.id != left.id)?;
            Some((l.track?, left.clone(), right.clone()))
        })
        .ok_or("the clip has no part right after it")?;
    let cut = left.start + left.length;
    // Each part keeps at least a sixteenth of a beat.
    let to = to.clamp(left.start + 1.0 / 16.0, right.start + right.length - 1.0 / 16.0);
    let trim = |session: &mut Session, clip: &Clip, edge: &str, at: f64| -> Result<Value, CommandError> {
        let index = session
            .project
            .arrangement
            .lanes
            .iter()
            .find(|l| l.track == Some(track))
            .and_then(|l| l.clips.iter().position(|c| c.id == clip.id))
            .ok_or("the part is missing")?;
        crate::clip_edges::trim_clip(session, &json!({ "track": track.0, "index": index, edge: at, "stack": true }))
    };
    // The part that gets shorter first, so the other never covers it.
    let done = if to > cut {
        trim(session, &right, "start", to)?;
        let grown = trim(session, &left, "end", to)?;
        grown["end"].as_f64().unwrap_or(to)
    } else {
        trim(session, &left, "end", to)?;
        let grown = trim(session, &right, "start", to)?;
        grown["start"].as_f64().unwrap_or(to)
    };
    // The part that got shorter starts or ends where the other one got to.
    if (done - to).abs() > 1e-9 {
        if to > cut {
            trim(session, &right, "start", done)?;
        } else {
            trim(session, &left, "end", done)?;
        }
    }
    Ok(json!({ "track": track.0, "cut": done }))
}

/// `take.setCrossfade {clip, length?}`: the crossfade at the cut after part `clip` made `length`
/// beats long, as the two parts' fades there, up to the shorter part's length; without a length,
/// as long as Settings says.
fn set_crossfade(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let id = params.get("clip").and_then(Value::as_u64).ok_or("clip must be a clip id")?;
    let length = match params.get("length") {
        None | Some(Value::Null) => session.take_crossfade_beats(),
        Some(v) => {
            v.as_f64().filter(|l| l.is_finite() && *l > 0.0).ok_or("length must be a number of beats above 0")?
        }
    };
    let p = &mut session.project;
    let (lane, left, right) = p
        .arrangement
        .lanes
        .iter()
        .enumerate()
        .find_map(|(at, l)| {
            let left = l.clips.iter().position(|c| u64::from(c.id.0) == id)?;
            let clip = l.clips.get(left)?;
            let cut = clip.start + clip.length;
            let right = l.clips.iter().position(|c| (c.start - cut).abs() < 1e-6 && c.id != clip.id)?;
            let (track, other) = (l.track?, l.clips.get(right)?);
            let takes = (p.comp_source(track, clip)?, p.comp_source(track, other)?);
            (takes.0 != takes.1).then_some((at, left, right))
        })
        .ok_or("the clip has no part of another take right after it")?;
    let clips = &mut p.arrangement.lanes.get_mut(lane).ok_or("the lane is missing")?.clips;
    let shortest = clips.get(left).map_or(0.0, |c| c.length).min(clips.get(right).map_or(0.0, |c| c.length));
    let length = length.min(shortest);
    let equal = suhina_model::FadeCurve { shape: suhina_model::FadeShape::EqualPower, curve: 0.0 };
    if let Some(clip) = clips.get_mut(left) {
        clip.fade_out = length;
        clip.fade_out_curve = equal;
        clip.fit_fades();
    }
    if let Some(clip) = clips.get_mut(right) {
        clip.fade_in = length;
        clip.fade_in_curve = equal;
        clip.fit_fades();
    }
    Ok(json!({ "clip": id, "length": length }))
}

/// `take.solo {track, take}`: the take lane heard in place of its track's clips, or none with 0.
fn solo_take(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_id(&session.project, params)?;
    let none = params.get("take").and_then(Value::as_u64) == Some(0);
    session.take_solo = if none { None } else { Some((track, take_index(&session.project, track, params)?)) };
    session.update_player(None)?;
    Ok(json!({ "track": track.0, "take": session.take_solo().map_or(0, |(_, n)| n) }))
}

/// `take.promote {track, take}`: every piece of the take put on the track over what is there.
fn promote_take(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let fade = session.take_crossfade_beats();
    let p = &mut session.project;
    let track = track_id(p, params)?;
    let index = take_index(p, track, params)?;
    let spans: Vec<(f64, f64)> = p
        .arrangement
        .takes
        .get(index)
        .map(|t| t.clips.iter().map(|c| (c.start, c.start + c.length)).collect())
        .unwrap_or_default();
    let mut clips = Vec::new();
    for (from, to) in spans {
        clips.extend(comp_part(p, track, index, from, to, None, fade)?);
    }
    Ok(json!({ "track": track.0, "clips": clips }))
}

/// `take.deleteUnused {track}`: the take lanes the comp uses nothing of go.
fn delete_unused_takes(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let track = track_id(p, params)?;
    let unused: Vec<usize> = (0..p.arrangement.takes.len())
        .filter(|&i| p.arrangement.takes.get(i).is_some_and(|t| t.track == track) && p.comp_used(i).is_empty())
        .collect();
    for &index in unused.iter().rev() {
        p.arrangement.takes.remove(index);
    }
    session.take_solo = None;
    Ok(json!({ "track": track.0, "deleted": unused.len() }))
}

/// `take.deletePieces {track, take, pieces}`: the pieces go from the take, and the take goes too
/// when none is left (#989).
fn delete_pieces(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let track = track_id(p, params)?;
    let index = take_index(p, track, params)?;
    let list = params.get("pieces").and_then(Value::as_array).ok_or("pieces must be a list")?;
    let count = p.arrangement.takes.get(index).map_or(0, |t| t.clips.len());
    let mut doomed: Vec<usize> = Vec::new();
    for entry in list {
        let piece = entry
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .and_then(|n| n.checked_sub(1))
            .filter(|&n| n < count)
            .ok_or("each piece is a number from 1 up to the take's pieces")?;
        doomed.push(piece);
    }
    doomed.sort_unstable();
    doomed.dedup();
    let Some(take) = p.arrangement.takes.get_mut(index) else { return Err("no such take".into()) };
    for &piece in doomed.iter().rev() {
        take.clips.remove(piece);
    }
    let emptied = take.clips.is_empty();
    if emptied {
        p.arrangement.takes.remove(index);
        session.take_solo = None;
    }
    Ok(json!({ "track": track.0, "pieces": doomed.len(), "take_deleted": emptied }))
}

/// `take.delete`.
fn delete_take(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let track = track_id(p, params)?;
    let index = take_index(p, track, params)?;
    let take = p.arrangement.takes.remove(index);
    session.take_solo = None;
    Ok(json!({ "track": track.0, "deleted": take.name }))
}

/// `take.deleteRange {track, takes, start, end}`: what the takes hold in the area goes, pieces
/// crossing its edges keeping their parts outside it, and a take left with nothing goes too.
fn delete_range(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &session.project;
    let track = track_id(p, params)?;
    let takes = listed_takes(p, track, params)?;
    let (start, end) = area(params)?;
    delete_area(session, track, &takes, start, end)
}

/// `take.cutRange {track, takes, start, end}`: `take.deleteRange`, with what goes copied first.
fn cut_range(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &session.project;
    let track = track_id(p, params)?;
    let takes = listed_takes(p, track, params)?;
    let (start, end) = area(params)?;
    if crate::range::copy_take_span(session, track, &takes, start, end) == 0 {
        return Err("the takes have nothing in the area".into());
    }
    delete_area(session, track, &takes, start, end)
}

fn delete_area(
    session: &mut Session,
    track: TrackId,
    takes: &[usize],
    start: f64,
    end: f64,
) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let holds = |clip: &Clip| clip.start < end && start < clip.start + clip.length;
    let inside = takes.iter().filter_map(|&i| p.arrangement.takes.get(i)).any(|t| t.clips.iter().any(holds));
    if !inside {
        return Err("the takes have nothing in the area".into());
    }
    let mut emptied = 0usize;
    // The last first, so the indices of the others hold when an emptied take goes.
    for &index in takes.iter().rev() {
        let Some(take) = p.arrangement.takes.get_mut(index) else { continue };
        make_room(&mut take.clips, start, end);
        if take.clips.is_empty() {
            p.arrangement.takes.remove(index);
            emptied += 1;
        }
    }
    if emptied > 0 {
        session.take_solo = None;
    }
    Ok(json!({ "track": track.0, "takes": takes.len(), "takes_deleted": emptied }))
}

/// `take.movePieces {track, takes, pieces, by}`: the pieces moved by `by` beats, each on its own
/// take lane, over what they cover there. Their content goes along, so each plays the same; the
/// parts comped from them earlier stay where they are, and no longer count as their parts.
fn move_pieces(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let track = track_id(p, params)?;
    let list = |key: &str| params.get(key).and_then(Value::as_array).ok_or(format!("{key} must be a list"));
    let (takes, pieces) = (list("takes")?, list("pieces")?);
    if takes.len() != pieces.len() {
        return Err("takes and pieces must be as long as each other".into());
    }
    let by =
        params.get("by").and_then(Value::as_f64).filter(|b| b.is_finite()).ok_or("by must be a number of beats")?;
    let mut moved: Vec<(usize, usize)> = Vec::with_capacity(takes.len());
    for (take, piece) in takes.iter().zip(pieces) {
        let index = index_of_take(p, track, take.as_u64().ok_or("each take is a number from 1")?)?;
        let count = p.arrangement.takes.get(index).map_or(0, |t| t.clips.len());
        let piece = piece
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .and_then(|n| n.checked_sub(1))
            .filter(|&n| n < count)
            .ok_or("each piece is a number from 1 up to its take's pieces")?;
        moved.push((index, piece));
    }
    moved.sort_unstable();
    moved.dedup();
    let starts = || moved.iter().filter_map(|&(i, n)| p.arrangement.takes.get(i)?.clips.get(n)).map(|c| c.start);
    // They stop together at the timeline's ends, so they keep their places to each other.
    let by = by.max(-starts().fold(f64::INFINITY, f64::min)).min(MAX_START_BEAT - starts().fold(0.0, f64::max));
    let mut indices: Vec<usize> = moved.iter().map(|(index, _)| *index).collect();
    indices.dedup();
    for index in indices {
        let Some(take) = p.arrangement.takes.get_mut(index) else { continue };
        let mut going = Vec::new();
        // The last piece first, so the numbers of the others hold.
        for &(_, piece) in moved.iter().rev().filter(|(i, _)| *i == index) {
            going.push(take.clips.remove(piece));
        }
        for clip in &mut going {
            clip.start += by;
            make_room(&mut take.clips, clip.start, clip.start + clip.length);
        }
        take.clips.append(&mut going);
        take.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    }
    Ok(json!({ "track": track.0, "pieces": moved.len(), "by": by }))
}

/// `take.trimPiece {track, take, piece, start?, end?}`: the piece's edges moved, the recording
/// around it coming back as they go out. A pass is a window on the file its take was recorded
/// into, so an audio piece goes on up to the end of that file; a MIDI pass has a content of its
/// own, so a MIDI piece goes on up to the end of its pass. Edges stop at those limits and at the
/// take's other pieces. The content stays lined up with the beats, so the piece's parts on the
/// track stay its parts.
fn trim_piece(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let track = track_id(p, params)?;
    let index = take_index(p, track, params)?;
    let take = p.arrangement.takes.get(index).ok_or("the take is missing")?;
    let piece = params
        .get("piece")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .and_then(|n| n.checked_sub(1))
        .filter(|&n| n < take.clips.len())
        .ok_or("piece must be a number from 1 up to the take's pieces")?;
    let edge = |key: &str| -> Result<Option<f64>, CommandError> {
        match params.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => {
                let beat = v.as_f64().filter(|b| b.is_finite()).ok_or_else(|| format!("{key} must be a beat"))?;
                Ok(Some(beat.max(0.0)))
            }
        }
    };
    let (start, end) = (edge("start")?, edge("end")?);
    if start.is_none() && end.is_none() {
        return Err("give start, end, or both".into());
    }
    let clip = take.clips.get(piece).ok_or("the piece is missing")?;
    let old_end = clip.start + clip.length;
    // The pieces are in time order, so its neighbours are the ones next to it.
    let before = piece.checked_sub(1).and_then(|n| take.clips.get(n)).map_or(0.0, |c| c.start + c.length);
    let after = take.clips.get(piece + 1).map_or(f64::INFINITY, |c| c.start);
    let rate = crate::editing::content_rate(p, clip).max(1e-9);
    let content_length = p.content(clip.content).map_or(f64::INFINITY, |c| c.length);
    let earliest = before.max(clip.start - clip.offset / rate).min(clip.start);
    let new_start = start.unwrap_or(clip.start).max(earliest).min(old_end - MIN_PIECE).max(0.0);
    let offset = clip.offset + (new_start - clip.start) * rate;
    let latest = after.min(new_start + (content_length - offset) / rate).max(new_start + MIN_PIECE);
    let new_end = end.unwrap_or(old_end).min(latest).max(new_start + MIN_PIECE);
    let clip = p.arrangement.takes.get_mut(index).and_then(|t| t.clips.get_mut(piece)).ok_or("the piece is missing")?;
    clip.start = new_start;
    clip.length = new_end - new_start;
    clip.offset = offset;
    clip.fit_fades();
    Ok(json!({ "track": track.0, "start": new_start, "end": new_end }))
}
