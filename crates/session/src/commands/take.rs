// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `take.*` commands: the take lanes a recording into the arrangement keeps under a track,
//! and comping parts of them onto it (`docs/spec/recording.md` › Take lanes, #909).

use super::*;

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
        id: "take.delete",
        label: "Delete a recorded take lane of a track; the track's clips stay",
        params: &[TRACK, param("take", ParamType::Integer, "the take's number on the track, from 1")],
        kind: CommandKind::Edit,
        run: delete_take,
        // Takes do not play: deleting one changes only what is shown.
        policy: Policy::UNHEARD,
    },
];

/// The number in a take lane's name, "Take 3" giving 3.
pub(crate) fn take_number(name: &str) -> Option<usize> {
    name.strip_prefix("Take ")?.trim().parse().ok().filter(|n| *n >= 1)
}

/// The index in `project.arrangement.takes` of take number `take` (from 1) of `track`.
fn take_index(p: &Project, track: TrackId, params: &Value) -> Result<usize, CommandError> {
    let number = Args::new(params)
        .value("take")
        .and_then(Value::as_u64)
        .filter(|n| *n >= 1)
        .ok_or("take must be a number from 1")?;
    let number = usize::try_from(number).map_err(|_| "take is out of range")?;
    // The number the lane shows ("Take 3" is 3), which stays the take's after others are
    // deleted; a take named otherwise is found by its place among the track's takes.
    let mine = || p.arrangement.takes.iter().enumerate().filter(|(_, t)| t.track == track);
    mine()
        .find(|(_, t)| take_number(&t.name) == Some(number))
        .or_else(|| mine().filter(|(_, t)| take_number(&t.name).is_none()).nth(number - 1))
        .map(|(i, _)| i)
        .ok_or_else(|| format!("the track has no take {number}").into())
}

/// `take.comp`: the take's clips, cut to `start` … `end`, laid on the track over what they
/// cover, as new clips (docs/spec/recording.md › Take lanes).
fn comp_take(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let track = track_id(p, params)?;
    let index = take_index(p, track, params)?;
    let take = p.arrangement.takes.get(index).ok_or("the take is missing")?;
    let first = take.clips.iter().map(|c| c.start).fold(f64::INFINITY, f64::min);
    let last = take.clips.iter().map(|c| c.start + c.length).fold(f64::NEG_INFINITY, f64::max);
    let name = take.name.clone();
    let bound = |key: &str, default: f64| -> Result<f64, CommandError> {
        match Args::new(params).value(key) {
            None => Ok(default),
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
    let color = match Args::new(params).value("color") {
        None => None,
        Some(_) => crate::clip_color::parse_color(params, "color")?,
    };
    let clips = comp_part(p, track, index, from, to, color)?;
    Ok(json!({ "track": track.0, "take": name, "start": from, "end": to, "clips": clips }))
}

/// Lays the part of take `index` (in `arrangement.takes`) from `from` to `to` on `track`'s lane,
/// over what it covers, as new clips in `color`, or the take's own colour when it has one; a part
/// with neither shows its take's colour in the window. Returns the clips' ids.
fn comp_part(
    p: &mut Project,
    track: TrackId,
    index: usize,
    from: f64,
    to: f64,
    color: Option<suhina_model::Rgb>,
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
        piece.fade_in = 0.0;
        piece.fade_out = 0.0;
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
    let piece = match Args::new(params).value("piece") {
        None => None,
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
    let p = &mut session.project;
    let id = Args::new(params).value("clip").and_then(Value::as_u64).ok_or("clip must be a clip id")?;
    let (track, clip) = p
        .arrangement
        .lanes
        .iter()
        .find_map(|l| Some((l.track?, l.clips.iter().find(|c| u64::from(c.id.0) == id)?.clone())))
        .ok_or_else(|| format!("clip {id} is not on the arrangement"))?;
    let up = match Args::new(params).raw_str("direction") {
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
    let clips = comp_part(p, track, target, start, end, None)?;
    let name = p.arrangement.takes.get(target).map(|t| t.name.clone()).unwrap_or_default();
    Ok(json!({ "track": track.0, "take": name, "clips": clips }))
}

/// `take.moveCut {clip, to}`: the cut between comped part `clip` and the part right after it moves
/// to `to`: one gets longer, the other shorter, each playing its own take (#989).
fn move_cut(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let id = Args::new(params).value("clip").and_then(Value::as_u64).ok_or("clip must be a clip id")?;
    let to =
        Args::new(params).value("to").and_then(Value::as_f64).filter(|t| t.is_finite()).ok_or("to must be a beat")?;
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

/// `take.solo {track, take}`: the take lane heard in place of its track's clips, or none with 0.
fn solo_take(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_id(&session.project, params)?;
    let none = Args::new(params).value("take").and_then(Value::as_u64) == Some(0);
    session.take_solo = if none { None } else { Some((track, take_index(&session.project, track, params)?)) };
    session.update_player(None)?;
    Ok(json!({ "track": track.0, "take": session.take_solo().map_or(0, |(_, n)| n) }))
}

/// `take.promote {track, take}`: every piece of the take put on the track over what is there.
fn promote_take(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
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
        clips.extend(comp_part(p, track, index, from, to, None)?);
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
    let list = Args::new(params).value("pieces").and_then(Value::as_array).ok_or("pieces must be a list")?;
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
