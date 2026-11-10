// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A time range of one automation lane (`docs/spec/automation.md` › Ranges): copied and pasted
//! (onto any lane, its values placed where they sat on their own), duplicated right after itself,
//! cleared, or filled with a shape. An edit replaces the points inside the range and keeps the
//! envelope outside it: points at the range's edges hold the values it had there.

use serde_json::{Value, json};
use suhina_engine::MAX_START_BEAT;
use suhina_model::automation::{Curve, MAX_POINTS, Taper, value_at};
use suhina_model::{AutomationPoint, Entry, IdList, PointId};

use suhina_model::TrackId;

use crate::automation::{Points, Target, target};
use crate::commands::{Args, track_id};
use crate::{CommandError, Session};

/// Copied points: their offsets from the range's start, their places on the lane (0 … 1) and
/// their curves, and how long the range was.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AutomationClip {
    pub(crate) length: f64,
    pub(crate) points: Vec<(f64, f64, Curve)>,
    /// Copied as picked points, not a range: pasted, they go in among the lane's points without
    /// edge points, replacing only those between the first and the last.
    pub(crate) picked: bool,
}

/// A lane's taper as the engine plays it, from the parameter `target` names: segments move
/// evenly in its position.
struct Lane<'a> {
    target: &'a Target,
    session: &'a Session,
}

impl Taper for Lane<'_> {
    fn position(&self, value: f64) -> f64 {
        self.target.position(self.session, value)
    }
    fn value(&self, position: f64) -> f64 {
        self.target.value_at_position(self.session, position)
    }
}

/// Values closer than this are one, for an edge point that would repeat its neighbour.
const SAME: f64 = 1e-9;

/// Most cycles a shape repeats in its range.
pub(crate) const MAX_CYCLES: usize = 512;

/// `start` and `end` from `params`: from beat 0, the end after the start.
fn range(params: &Value) -> Result<(f64, f64), CommandError> {
    let start = Args::new(params).f64("start")?;
    let end = Args::new(params).f64("end")?;
    if !(0.0..=MAX_START_BEAT).contains(&start) || !(0.0..=MAX_START_BEAT).contains(&end) {
        return Err(format!("start and end must be from 0 to {MAX_START_BEAT}").into());
    }
    if end <= start {
        return Err(format!("end must be after start, got {start} to {end}").into());
    }
    Ok((start, end))
}

/// The envelope of `points` from `start` to `end`: its value at each edge and the points
/// between. Empty when the lane has no points.
fn slice(points: &[AutomationPoint], start: f64, end: f64, stepped: bool, taper: &impl Taper) -> Vec<AutomationPoint> {
    let Some(first) = value_at(points, start, stepped, taper) else { return Vec::new() };
    let last = value_at(points, end, stepped, taper).unwrap_or(first);
    let curve_at = |beat: f64| {
        let index = points.partition_point(|p| p.beat <= beat);
        index.checked_sub(1).and_then(|i| points.get(i)).map_or(Curve::STRAIGHT, |p| p.curve)
    };
    let mut out = vec![AutomationPoint { beat: start, value: first, curve: curve_at(start) }];
    out.extend(points.iter().filter(|p| p.beat > start && p.beat < end).copied());
    out.push(AutomationPoint { beat: end, value: last, curve: Curve::STRAIGHT });
    out
}

/// The indices of `points` from `start` to `end`, both included.
fn span(points: &[AutomationPoint], start: f64, end: f64) -> std::ops::Range<usize> {
    points.partition_point(|p| p.beat < start)..points.partition_point(|p| p.beat <= end)
}

/// `points` with what was from `start` to `end` replaced by `inner` (beats in that range), the
/// values at the edges kept so the envelope outside does not change. The points outside keep
/// their ids; those of `inner` and the edges are new.
fn replace(
    points: &Points,
    start: f64,
    end: f64,
    inner: Vec<AutomationPoint>,
    stepped: bool,
    taper: &impl Taper,
) -> Points {
    let edges = value_at(points, start, stepped, taper).zip(value_at(points, end, stepped, taper));
    let after_curve = {
        let index = points.partition_point(|p| p.beat <= end);
        index.checked_sub(1).and_then(|i| points.get(i)).map_or(Curve::STRAIGHT, |p| p.curve)
    };
    let mut out: Vec<AutomationPoint> = Vec::with_capacity(inner.len() + 2);
    if let Some((before, _)) = edges
        && inner.first().is_none_or(|p| p.beat > start || (p.value - before).abs() > SAME)
    {
        out.push(AutomationPoint { beat: start, value: before, curve: Curve::STRAIGHT });
    }
    let last_inner = inner.last().copied();
    out.extend(inner);
    if let Some((_, after)) = edges
        && last_inner.is_none_or(|p| p.beat < end || (p.value - after).abs() > SAME)
    {
        out.push(AutomationPoint { beat: end, value: after, curve: after_curve });
    }
    let mut merged = points.clone();
    merged.splice(span(points, start, end), out.into_iter().map(Entry::new).collect());
    merged
}

/// Puts `points` on the lane `target` names, making the lane when it has none.
fn put(session: &mut Session, target: &Target, points: Points) -> Result<usize, CommandError> {
    if points.len() > MAX_POINTS {
        return Err(format!("a lane holds at most {MAX_POINTS} points").into());
    }
    let lanes = target.lanes(session)?;
    let count = points.len();
    match lanes.iter_mut().find(|l| target.matches(l)) {
        Some(lane) => lane.points = points,
        None => {
            let mut lane = suhina_model::AutomationLane::new(target.param.clone());
            lane.points = points;
            lanes.push(lane);
        }
    }
    Ok(count)
}

/// The points of the lane `target` names (none when it has no lane).
fn points_of(session: &mut Session, target: &Target) -> Result<Points, CommandError> {
    Ok(target.lanes(session)?.iter().find(|l| target.matches(l)).map(|l| l.points.clone()).unwrap_or_default())
}

/// `automation.copy {track, device?, param, start, end}`: the envelope of the range, for
/// `automation.paste`.
/// Most lanes one command names with `lanes` (#1322).
const MAX_LANES: usize = 256;

/// The params of each lane a command names (#1322): its own lane, then each of `lanes`
/// (`{device?, param}`) on the same track, with the rest of the params as given.
fn each_lane(params: &Value) -> Result<Vec<Value>, CommandError> {
    let mut own = params.clone();
    if let Some(map) = own.as_object_mut() {
        map.remove("lanes");
    }
    let mut all = vec![own.clone()];
    let Some(list) = Args::new(params).value("lanes").filter(|v| !v.is_null()) else { return Ok(all) };
    let list =
        list.as_array().ok_or("lanes must be a list of lanes: {\"device\": n, \"param\": id} or {\"param\": id}")?;
    if list.len() > MAX_LANES {
        return Err(format!("lanes names at most {MAX_LANES} lanes").into());
    }
    for lane in list {
        let param = lane.get("param").and_then(Value::as_str).ok_or("each of lanes needs a param")?;
        let mut one = own.clone();
        if let Some(map) = one.as_object_mut() {
            map.insert("param".into(), json!(param));
            match lane.get("device").filter(|d| !d.is_null()) {
                Some(device) => map.insert("device".into(), device.clone()),
                None => map.remove("device"),
            };
        }
        all.push(one);
    }
    Ok(all)
}

/// The lane `params` names, as a copied range keeps it: its device and its parameter.
fn lane_of(params: &Value) -> (Option<usize>, String) {
    let device = params.get("device").and_then(Value::as_u64).and_then(|d| usize::try_from(d).ok());
    (device, params.get("param").and_then(Value::as_str).unwrap_or_default().to_string())
}

/// One of several lanes copied together (#1322): its device by its place on the track's chain
/// and what it is (its kind and name), its parameter and its range.
#[derive(Debug, Clone)]
pub(crate) struct CopiedLane {
    device: Option<usize>,
    kind: Option<(suhina_model::DeviceKind, String)>,
    param: String,
    clip: AutomationClip,
}

/// What the device at `index` on `track`'s chain is: its kind and its name.
fn device_kind(session: &Session, track: TrackId, index: usize) -> Option<(suhina_model::DeviceKind, String)> {
    let device = session.project().channel_of_track(track)?.devices.get(index)?;
    Some((device.kind.clone(), device.name.clone()))
}

/// Where a lane copied from a device goes on `track`: a device of the same kind (a built-in one
/// by its id, a plug-in by its class, a `.sd` graph by its name), the one at the same place when
/// that is one, else the first; `None` when the track has none, so the lane is left out rather
/// than land on another device's parameter of the same id.
fn same_device(session: &Session, track: TrackId, lane: &CopiedLane) -> Option<Option<usize>> {
    let (Some(index), Some((kind, name))) = (lane.device, &lane.kind) else { return Some(lane.device) };
    let devices = &session.project().channel_of_track(track)?.devices;
    let alike = |d: &suhina_model::Device| {
        d.kind == *kind && (!matches!(kind, suhina_model::DeviceKind::Sd) || d.name == *name)
    };
    if devices.get(index).is_some_and(alike) {
        return Some(Some(index));
    }
    devices.iter().position(alike).map(Some)
}

/// Runs `one` on every lane `params` names (#1322), as one command: the lanes it could not act
/// on (no automation there) are left out, and it fails only when it could act on none.
fn on_each_lane(
    session: &mut Session,
    params: &Value,
    mut one: impl FnMut(&mut Session, &Value) -> Result<Value, CommandError>,
) -> Result<Value, CommandError> {
    let lanes = each_lane(params)?;
    if lanes.len() == 1 {
        return one(session, &lanes[0]);
    }
    let mut answers = Vec::new();
    let mut first_error = None;
    for lane in &lanes {
        match one(session, lane) {
            Ok(answer) => answers.push(answer),
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    match (answers.first().cloned(), first_error) {
        (Some(first), _) => {
            let mut answer = first;
            answer["lanes"] = json!(answers);
            Ok(answer)
        }
        (None, Some(error)) => Err(error),
        (None, None) => Err("no lane was named".into()),
    }
}

/// `automation.copy {track, device?, param, start, end, lanes?}` or `{…, points}`: a range of
/// one lane, or of several (`lanes`, #1322) for `automation.paste` to put back on each.
pub(crate) fn copy(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let lanes = each_lane(params)?;
    if lanes.len() == 1 {
        session.clipboards.automation_lanes.clear();
        return copy_one(session, params);
    }
    let mut copied = Vec::new();
    let mut answers = Vec::new();
    for lane in &lanes {
        if let Ok(answer) = copy_one(session, lane)
            && let Some(clip) = session.clipboards.automation.clone()
        {
            let (device, param) = lane_of(lane);
            let kind = match (device, track_id(&session.project, lane)) {
                (Some(index), Ok(track)) => device_kind(session, track, index),
                _ => None,
            };
            copied.push(CopiedLane { device, kind, param, clip });
            answers.push(answer);
        }
    }
    let Some(first) = copied.first().map(|lane| lane.clip.clone()) else {
        return Err("none of these lanes has automation in the range".into());
    };
    session.clipboards.automation = Some(first);
    session.clipboards.automation_lanes = copied;
    let mut answer = answers.first().cloned().unwrap_or_else(|| json!({}));
    answer["lanes"] = json!(answers);
    Ok(answer)
}

fn copy_one(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let target = target(session, params)?;
    // Picked points (`points`): copied as they are, from the first of them.
    if let Some(list) = Args::new(params).value("points").filter(|v| !v.is_null()) {
        let lane = points_of(session, &target)?;
        let mut picked: Vec<AutomationPoint> = Vec::new();
        for value in list.as_array().ok_or("points must be a list of points")? {
            let index = crate::automation::point_at(value, &lane)?;
            picked.extend(lane.get(index).copied());
        }
        picked.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        let (Some(first), Some(last)) = (picked.first().map(|p| p.beat), picked.last().map(|p| p.beat)) else {
            return Err("points must name at least one point".into());
        };
        let points: Vec<(f64, f64, Curve)> =
            picked.iter().map(|p| (p.beat - first, target.position(session, p.value), p.curve)).collect();
        let count = points.len();
        session.clipboards.automation = Some(AutomationClip { length: last - first, points, picked: true });
        return Ok(json!({ "param": target.param, "points": count, "length": last - first }));
    }
    let (start, end) = range(params)?;
    let points = points_of(session, &target)?;
    let stepped = target.stepped();
    let cut = slice(&points, start, end, stepped, &Lane { target: &target, session });
    if cut.is_empty() {
        return Err(format!("{} has no automation to copy", target.param).into());
    }
    let points: Vec<(f64, f64, Curve)> =
        cut.iter().map(|p| (p.beat - start, target.position(session, p.value), p.curve)).collect();
    let count = points.len();
    session.clipboards.automation = Some(AutomationClip { length: end - start, points, picked: false });
    Ok(json!({ "param": target.param, "points": count, "length": end - start }))
}

/// `automation.paste {track, device?, param, start}`: the copied range from `start`, over what
/// is there; on another parameter its values sit where they sat on their own lane.
pub(crate) fn paste(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    // Several lanes copied together go back on the same lanes of the track pasted to (#1322).
    let several = session.clipboards.automation_lanes.clone();
    if several.len() > 1 {
        let kept = session.clipboards.automation.clone();
        let track = track_id(&session.project, params)?;
        let mut answers = Vec::new();
        for lane in several {
            // Onto a device of the same kind on the track pasted to, or not at all.
            let Some(device) = same_device(session, track, &lane) else { continue };
            let CopiedLane { param, clip, .. } = lane;
            let mut one = params.clone();
            if let Some(map) = one.as_object_mut() {
                map.insert("param".into(), json!(param));
                match device {
                    Some(device) => map.insert("device".into(), json!(device)),
                    None => map.remove("device"),
                };
            }
            session.clipboards.automation = Some(clip);
            if let Ok(answer) = paste_one(session, &one) {
                answers.push(answer);
            }
        }
        session.clipboards.automation = kept;
        let Some(first) = answers.first().cloned() else {
            return Err("the track has none of the copied lanes' parameters".into());
        };
        let mut answer = first;
        answer["lanes"] = json!(answers);
        return Ok(answer);
    }
    paste_one(session, params)
}

fn paste_one(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let target = target(session, params)?;
    let start = Args::new(params).f64("start")?;
    if !(0.0..=MAX_START_BEAT).contains(&start) {
        return Err(format!("start must be from 0 to {MAX_START_BEAT}").into());
    }
    let clip =
        session.clipboards.automation.clone().ok_or("no automation has been copied; automation.copy copies a range")?;
    let end = start + clip.length;
    let inner: Vec<AutomationPoint> = clip
        .points
        .iter()
        .map(|&(offset, position, curve)| AutomationPoint {
            beat: start + offset,
            value: target.value_at_position(session, position),
            curve,
        })
        .collect();
    let points = points_of(session, &target)?;
    let merged = if clip.picked {
        // Picked points go in among the lane's, replacing those from the first to the last, each
        // after the lane's points at its beat; the lane's points outside keep their ids.
        let mut merged = points;
        merged.retain(|p| p.beat < start || p.beat > end);
        for point in inner {
            let at = merged.partition_point(|p| p.beat <= point.beat);
            merged.insert(at, point);
        }
        merged
    } else {
        replace(&points, start, end, inner, target.stepped(), &Lane { target: &target, session })
    };
    let count = put(session, &target, merged)?;
    Ok(json!({ "param": target.param, "start": start, "end": end, "points": count }))
}

/// `automation.duplicate {track, device?, param, start, end}`: a copy of the range right after
/// it; the copy's range is the answer, so the next duplicate goes on from there. The clipboard
/// stays as it was.
pub(crate) fn duplicate(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    on_each_lane(session, params, duplicate_one)
}

fn duplicate_one(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let target = target(session, params)?;
    let (start, end) = range(params)?;
    let length = end - start;
    if end + length > MAX_START_BEAT {
        return Err("the copy would end past the arrangement's last beat".into());
    }
    let points = points_of(session, &target)?;
    let stepped = target.stepped();
    let cut = slice(&points, start, end, stepped, &Lane { target: &target, session });
    if cut.is_empty() {
        return Err(format!("{} has no automation to duplicate", target.param).into());
    }
    let inner: Vec<AutomationPoint> = cut.into_iter().map(|p| AutomationPoint { beat: p.beat + length, ..p }).collect();
    let merged = replace(&points, end, end + length, inner, stepped, &Lane { target: &target, session });
    let count = put(session, &target, merged)?;
    Ok(json!({ "param": target.param, "start": end, "end": end + length, "points": count }))
}

/// One lane's envelope under a clip (`docs/spec/automation.md` › Automation under clips): the
/// lane, by its device on the track (`None` for the mixer's) and its parameter, and its points
/// from the clip's start, the values at the clip's two edges included.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Under {
    device: Option<usize>,
    param: String,
    points: Vec<AutomationPoint>,
}

/// The params that name a lane of `track`: its device (none for the mixer) and parameter.
fn lane_params(track: TrackId, device: Option<usize>, param: &str) -> Value {
    match device {
        Some(device) => json!({ "track": track.0, "device": device, "param": param }),
        None => json!({ "track": track.0, "param": param }),
    }
}

/// The envelope under a clip from `start` to `end` on `track`: every lane of the track's mixer
/// and devices with a point from `start` up to `end`, its points from `start`. A lane with no
/// point there is a straight line under the clip and is left out: it stays as it is.
pub(crate) fn under(session: &Session, track: TrackId, start: f64, end: f64) -> Vec<Under> {
    let Some(channel) = session.project.channel_of_track(track) else { return Vec::new() };
    let mixer = channel.automation.iter().map(|lane| (None, lane));
    let devices = channel.devices.iter().enumerate().flat_map(|(i, d)| d.automation.iter().map(move |l| (Some(i), l)));
    mixer
        .chain(devices)
        .filter(|(_, lane)| lane.points.iter().any(|p| p.beat >= start - SAME && p.beat < end - SAME))
        .filter_map(|(device, lane)| {
            let target = target(session, &lane_params(track, device, &lane.param)).ok()?;
            let cut = slice(&lane.points, start, end, target.stepped(), &Lane { target: &target, session });
            let points = cut.into_iter().map(|p| AutomationPoint { beat: p.beat - start, ..p }).collect();
            Some(Under { device, param: lane.param.clone(), points })
        })
        .collect()
}

/// Clears the lanes `lanes` names on `track` from `start` to `end`, the values at the edges
/// kept: what is left where a clip and its envelope moved away.
pub(crate) fn clear_under(
    session: &mut Session,
    track: TrackId,
    start: f64,
    end: f64,
    lanes: &[Under],
) -> Result<(), CommandError> {
    for lane in lanes {
        let Ok(target) = target(session, &lane_params(track, lane.device, &lane.param)) else { continue };
        let points = points_of(session, &target)?;
        let cleared = replace(&points, start, end, Vec::new(), target.stepped(), &Lane { target: &target, session });
        put(session, &target, cleared)?;
    }
    Ok(())
}

/// Lays `lanes` on `track` from `at`, over what is there, the values at the edges outside kept:
/// the envelope under a clip moved or pasted there. A device's lane goes only on the track the
/// envelope came from (`same`); the mixer's go on any track.
pub(crate) fn lay_under(
    session: &mut Session,
    track: TrackId,
    at: f64,
    lanes: &[Under],
    same: bool,
) -> Result<(), CommandError> {
    for lane in lanes.iter().filter(|l| same || l.device.is_none()) {
        let length = lane.points.last().map_or(0.0, |p| p.beat);
        if length <= SAME || at + length > MAX_START_BEAT {
            continue;
        }
        let Ok(target) = target(session, &lane_params(track, lane.device, &lane.param)) else { continue };
        let inner: Vec<AutomationPoint> =
            lane.points.iter().map(|p| AutomationPoint { beat: at + p.beat, ..*p }).collect();
        let points = points_of(session, &target)?;
        let laid = replace(&points, at, at + length, inner, target.stepped(), &Lane { target: &target, session });
        put(session, &target, laid)?;
    }
    Ok(())
}

/// The bend, -1 to 1, whose curve is at `share` of its rise `t` of the way along, or `None`
/// when no bend reaches it.
fn curve_through(t: f64, share: f64) -> Option<f64> {
    if !(t > 0.0 && t < 1.0 && share > 0.0 && share < 1.0) {
        return None;
    }
    let (mut lo, mut hi) = (-1.0f64, 1.0f64);
    if share < Curve::bend(lo).shape(t) || share > Curve::bend(hi).shape(t) {
        return None;
    }
    // A mirrored curve rises with its bend at every `t` inside the segment.
    for _ in 0..48 {
        let mid = 0.5 * (lo + hi);
        if Curve::bend(mid).shape(t) < share {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(0.5 * (lo + hi))
}

/// `automation.simplify {track, device?, param, start, end, tolerance?}`: the points inside the
/// range replaced by the fewest bent segments that stay within `tolerance` of the old line, curves
/// and all, as a share of the lane's height (default 2 %), keeping its jumps and sharp corners
/// ([`crate::automation_fit`]); so a sine stays a sine.
pub(crate) fn simplify(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let target = target(session, params)?;
    let (start, end) = range(params)?;
    let tolerance = share(params, "tolerance")?.unwrap_or(0.02);
    let points = points_of(session, &target)?;
    let inside: Vec<AutomationPoint> = points.iter().filter(|p| p.beat >= start && p.beat <= end).copied().collect();
    let unchanged = json!({ "param": target.param, "points": points.len(), "removed": 0 });
    if inside.len() < 3 || target.stepped() {
        return Ok(unchanged);
    }
    let lane = Lane { target: &target, session };
    let old: Vec<(f64, f64, Curve)> = inside.iter().map(|p| (p.beat, lane.position(p.value), p.curve)).collect();
    let Some(fitted) = crate::automation_fit::simplify(&old, tolerance) else { return Ok(unchanged) };
    let kept: Vec<AutomationPoint> =
        fitted.iter().map(|f| AutomationPoint { beat: f.beat, value: lane.value(f.place), curve: f.curve }).collect();
    // The points outside keep their ids; the fitted ones are new.
    let mut merged = points.clone();
    merged.splice(span(&points, start, end), kept.into_iter().map(Entry::new).collect());
    let removed = points.len().saturating_sub(merged.len());
    let count = put(session, &target, merged)?;
    Ok(json!({ "param": target.param, "points": count, "removed": removed }))
}

/// The share of the room on each side of a corner or jump that rounding takes at amount 1: under
/// a half, so the rounding of two neighbours never meets at one beat.
const ROOM: f64 = 0.45;

/// How far a corner may bend: the steepest curve a rounded corner or jump gets, at amount 1.
const ROUND_CURVE: f64 = 0.9;

/// Whether the line turns at the point between `a`, `p` and `b` (places on the lane at beats, the
/// time as a share of `span`): its slope changes by more than a twentieth of the lane a span.
fn turns(a: (f64, f64), p: (f64, f64), b: (f64, f64), span: f64) -> bool {
    let slope = |x: (f64, f64), y: (f64, f64)| (y.1 - x.1) / ((y.0 - x.0) / span);
    (slope(a, p) - slope(p, b)).abs() > 0.05
}

/// `automation.smooth {track, device?, param, start, end, amount}`: the corners and jumps inside
/// the range rounded, `amount` (0 to 1) taking up to [`ROOM`] of the room on each side. A jump
/// (two points at one beat) becomes an eased step: it leaves its first value slowly, passes the
/// middle at its beat and settles on its second; a corner between two straight segments gets a
/// point on each side, the line easing into the corner and out of it. Bent segments (a sine's)
/// are smooth already and are left as they are.
pub(crate) fn smooth(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let target = target(session, params)?;
    let (start, end) = range(params)?;
    let amount = share(params, "amount")?.ok_or("amount must be a number from 0 to 1")?;
    let mut list = points_of(session, &target)?;
    // The values to look at, and the points with their ids to put back: a point that stays (a
    // corner only bent) keeps its id, a point a rounding draws is new.
    let points = list.to_vec();
    let mut taken = list.take_range(0..list.len()).into_iter();
    if amount <= 0.0 || target.stepped() || points.len() < 2 {
        return Ok(json!({ "param": target.param, "points": points.len(), "rounded": 0 }));
    }
    let lane = Lane { target: &target, session };
    let place = |p: &AutomationPoint| (p.beat, lane.position(p.value));
    let at = |beat: f64| value_at(&points, beat, false, &lane);
    let bend = ROUND_CURVE * amount;
    let span = end - start;
    let inside = |beat: f64| beat > start + SAME && beat < end - SAME;
    let mut out: Vec<Entry<PointId, AutomationPoint>> = Vec::with_capacity(points.len() * 2);
    let mut rounded = 0;
    let mut i = 0;
    while let Some(&p) = points.get(i) {
        let before = i.checked_sub(1).and_then(|j| points.get(j)).map_or(start, |a| a.beat.max(start));
        // A jump: this point and the next at one beat.
        if let Some(&q) = points.get(i + 1).filter(|q| (q.beat - p.beat).abs() < SAME && inside(p.beat)) {
            let after = points.get(i + 2).map_or(end, |b| b.beat.min(end));
            let d = amount * ROOM * (p.beat - before).min(after - p.beat);
            if d > SAME {
                let (Some(from), Some(to)) = (at(p.beat - d), at(p.beat + d)) else { break };
                let middle = lane.value((lane.position(from) + lane.position(to)) / 2.0);
                // The jump's two points become three new ones.
                taken.next();
                taken.next();
                out.push(Entry::new(AutomationPoint { beat: p.beat - d, value: from, curve: Curve::bend(-bend) }));
                out.push(Entry::new(AutomationPoint { beat: p.beat, value: middle, curve: Curve::bend(bend) }));
                out.push(Entry::new(AutomationPoint { beat: p.beat + d, value: to, curve: q.curve }));
                rounded += 1;
                i += 2;
                continue;
            }
        }
        // A corner between two straight segments.
        if let (Some(&a), Some(&b)) = (i.checked_sub(1).and_then(|j| points.get(j)), points.get(i + 1))
            && inside(p.beat)
            && a.curve.is_straight()
            && p.curve.is_straight()
            && p.beat - a.beat > SAME
            && b.beat - p.beat > SAME
            && turns(place(&a), place(&p), place(&b), span)
        {
            let d = amount * ROOM * (p.beat - a.beat).min(b.beat - p.beat);
            if let (Some(into), Some(out_of)) = (at(p.beat - d), at(p.beat + d))
                && d > SAME
            {
                out.push(Entry::new(AutomationPoint { beat: p.beat - d, value: into, curve: Curve::bend(bend) }));
                // The corner itself stays, bent.
                if let Some(mut corner) = taken.next() {
                    corner.value.curve = Curve::bend(-bend);
                    out.push(corner);
                }
                out.push(Entry::new(AutomationPoint { beat: p.beat + d, value: out_of, curve: Curve::STRAIGHT }));
                rounded += 1;
                i += 1;
                continue;
            }
        }
        out.extend(taken.next());
        i += 1;
    }
    let count = put(session, &target, IdList::from_entries(out))?;
    Ok(json!({ "param": target.param, "points": count, "rounded": rounded }))
}

/// `automation.scaleRange {track, device?, param, start, end, factor}`: the points inside the
/// range stretched up and down by `factor` (0 to 4) around the middle of where they sit on the
/// lane, kept inside the travel.
pub(crate) fn scale_range(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let target = target(session, params)?;
    let factor = Args::new(params).f64("factor")?;
    if !(0.0..=4.0).contains(&factor) {
        return Err(format!("factor must be from 0 to 4, got {factor}").into());
    }
    let mut points = points_of(session, &target)?;
    // Picked points (`points`): only they move, around the middle of the line over their span, so
    // troughs picked alone deepen while the peaks between them stay.
    let picked: Option<Vec<usize>> = match Args::new(params).value("points").filter(|v| !v.is_null()) {
        None => None,
        Some(list) => {
            let mut out = Vec::new();
            for value in list.as_array().ok_or("points must be a list of points")? {
                out.push(crate::automation::point_at(value, &points)?);
            }
            Some(out)
        }
    };
    let (start, end) = match &picked {
        None => range(params)?,
        Some(list) => {
            let beats = list.iter().filter_map(|&i| points.get(i)).map(|p| p.beat);
            let (Some(first), Some(last)) = (beats.clone().reduce(f64::min), beats.reduce(f64::max)) else {
                return Err("points must name at least one point".into());
            };
            (first, last)
        }
    };
    let lane = Lane { target: &target, session };
    let places: Vec<f64> =
        points.iter().filter(|p| p.beat >= start && p.beat <= end).map(|p| lane.position(p.value)).collect();
    let (Some(low), Some(high)) = (places.iter().copied().reduce(f64::min), places.iter().copied().reduce(f64::max))
    else {
        return Err("the range has no points".into());
    };
    let middle = match Args::new(params).raw_str("around") {
        None | Some("middle") => (low + high) / 2.0,
        Some("bottom") => 0.0,
        Some(other) => return Err(format!("around is \"middle\" or \"bottom\", got {other:?}").into()),
    };
    for (index, point) in points.iter_mut().enumerate() {
        let moves = match &picked {
            Some(list) => list.contains(&index),
            None => point.beat >= start && point.beat <= end,
        };
        if moves {
            let place = (middle + (lane.position(point.value) - middle) * factor).clamp(0.0, 1.0);
            point.value = lane.value(place);
        }
    }
    let count = put(session, &target, points)?;
    Ok(json!({ "param": target.param, "points": count }))
}

/// `automation.deleteRange {track, device?, param, start, end}`: the points inside the range go;
/// the values at its edges stay, so the envelope outside is as it was.
pub(crate) fn delete_range(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    on_each_lane(session, params, delete_range_one)
}

fn delete_range_one(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let target = target(session, params)?;
    let (start, end) = range(params)?;
    let points = points_of(session, &target)?;
    if points.is_empty() {
        return Err(format!("{} has no automation", target.param).into());
    }
    let merged = replace(&points, start, end, Vec::new(), target.stepped(), &Lane { target: &target, session });
    let count = put(session, &target, merged)?;
    Ok(json!({ "param": target.param, "points": count }))
}

/// A point of a shape over one cycle: its time (0 … 1), its place on the lane (0 … 1) and the
/// curve of the segment after it.
pub type ShapePoint = (f64, f64, f64);

/// A shape `automation.insertShape` draws: its points over one cycle, and whether it repeats.
pub fn shape_points(shape: &str) -> Option<(Vec<ShapePoint>, bool)> {
    // A sine from four bent segments a cycle: each quarter leaves the middle fast and reaches a
    // peak or a trough slowly, the curves that pass through a sine's value half-way along.
    let away = curve_through(0.5, std::f64::consts::FRAC_1_SQRT_2).unwrap_or(0.0);
    let back = curve_through(0.5, 1.0 - std::f64::consts::FRAC_1_SQRT_2).unwrap_or(0.0);
    Some(match shape {
        "sine" => {
            (vec![(0.0, 0.5, away), (0.25, 1.0, back), (0.5, 0.5, away), (0.75, 0.0, back), (1.0, 0.5, 0.0)], true)
        }
        "triangle" => (vec![(0.0, 0.0, 0.0), (0.5, 1.0, 0.0), (1.0, 0.0, 0.0)], true),
        "saw_up" => (vec![(0.0, 0.0, 0.0), (1.0, 1.0, 0.0)], true),
        "saw_down" => (vec![(0.0, 1.0, 0.0), (1.0, 0.0, 0.0)], true),
        "square" => (vec![(0.0, 1.0, 0.0), (0.5, 1.0, 0.0), (0.5, 0.0, 0.0), (1.0, 0.0, 0.0)], true),
        "ramp_up" => (vec![(0.0, 0.0, 0.0), (1.0, 1.0, 0.0)], false),
        "ramp_down" => (vec![(0.0, 1.0, 0.0), (1.0, 0.0, 0.0)], false),
        "curve_up" => (vec![(0.0, 0.0, -0.7), (1.0, 1.0, 0.0)], false),
        "curve_down" => (vec![(0.0, 1.0, 0.7), (1.0, 0.0, 0.0)], false),
        "swell" => (vec![(0.0, 0.0, 0.6), (0.5, 1.0, -0.6), (1.0, 0.0, 0.0)], false),
        _ => return None,
    })
}

/// The shapes by name, in the order the menu shows them.
pub const SHAPES: [&str; 10] =
    ["sine", "triangle", "saw_up", "saw_down", "square", "ramp_up", "ramp_down", "curve_up", "curve_down", "swell"];

/// The cycles `params["cycles"]` asks for, from 1 to [`MAX_CYCLES`]; `None` when left out.
fn cycles(params: &Value) -> Result<Option<usize>, CommandError> {
    match Args::new(params).value("cycles") {
        None => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| (1..=MAX_CYCLES).contains(n))
            .map(Some)
            .ok_or_else(|| format!("cycles must be a whole number from 1 to {MAX_CYCLES}").into()),
    }
}

/// A share from 0 to 1 that `params[name]` gives, `None` when left out.
fn share(params: &Value, name: &str) -> Result<Option<f64>, CommandError> {
    match Args::new(params).value(name) {
        None => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|n| (0.0..=1.0).contains(n))
            .map(Some)
            .ok_or_else(|| format!("{name} must be a number from 0 to 1").into()),
    }
}

/// A repeating shape's cycles over `start` to `end` when none are asked for: one a beat.
fn cycles_per_beat(start: f64, end: f64) -> usize {
    let beats = (end - start).round();
    if beats.is_finite() && beats >= 1.0 { (beats as usize).min(MAX_CYCLES) } else { 1 }
}

/// The segment a shape on point `index` of `points` replaces: from the point before it to it (for
/// the first point, from it to the next), and the two points' places on the lane.
fn segment(
    session: &Session,
    target: &Target,
    points: &[AutomationPoint],
    index: usize,
) -> Result<(f64, f64, f64, f64), CommandError> {
    let (a, b) = if index == 0 { (0, 1) } else { (index - 1, index) };
    let (Some(a), Some(b)) = (points.get(a), points.get(b)) else {
        return Err("the point needs a neighbour: a lane of at least two points".into());
    };
    if b.beat - a.beat <= SAME {
        return Err("the two points are at one beat: there is no segment between them".into());
    }
    Ok((a.beat, b.beat, target.position(session, a.value), target.position(session, b.value)))
}

/// Fills `start` to `end` of the lane with shape `name` between lane places `low` and `high`, a
/// repeating shape `cycles` times; answers the lane's point count.
#[allow(clippy::too_many_arguments)]
fn fill(
    session: &mut Session,
    target: &Target,
    start: f64,
    end: f64,
    (low, high): (f64, f64),
    name: &str,
    cycles: usize,
) -> Result<usize, CommandError> {
    let (cycle, repeats) =
        shape_points(name).ok_or_else(|| format!("no shape {name:?}; the shapes are {}", SHAPES.join(", ")))?;
    let cycles = if repeats { cycles.clamp(1, MAX_CYCLES) } else { 1 };
    let span = (end - start) / cycles as f64;
    let mut inner: Vec<AutomationPoint> = Vec::new();
    for n in 0..cycles {
        for (i, &(t, position, curve)) in cycle.iter().enumerate() {
            // A cycle starts where the one before ended: its first point is that end, unless the
            // shape jumps there (a saw).
            let beat = start + (n as f64 + t) * span;
            let value = target.value_at_position(session, low + position * (high - low));
            if i == 0
                && let Some(last) = inner.last()
                && (last.beat - beat).abs() < SAME
                && (last.value - value).abs() < SAME
            {
                if let Some(last) = inner.last_mut() {
                    last.curve = Curve::bend(curve);
                }
                continue;
            }
            inner.push(AutomationPoint { beat, value, curve: Curve::bend(curve) });
        }
    }
    let points = points_of(session, target)?;
    let merged = replace(&points, start, end, inner, target.stepped(), &Lane { target, session });
    put(session, target, merged)
}

/// The segment shape `automation.addPoint {shape}` gives the segment that ends at the point it
/// added (from the point before it), between the two points' heights, a repeating shape once a
/// beat. Nothing for the lane's first point.
pub(crate) fn shape_new_segment(
    session: &mut Session,
    target: &Target,
    index: usize,
    name: &str,
) -> Result<(), CommandError> {
    if index == 0 {
        return Ok(());
    }
    let points = points_of(session, target)?;
    let Ok((start, end, a, b)) = segment(session, target, &points, index) else { return Ok(()) };
    fill(session, target, start, end, (a.min(b), a.max(b)), name, cycles_per_beat(start, end)).map(|_| ())
}

/// `automation.insertShape {track, device?, param, start, end, shape, cycles?}`: the range filled
/// with a shape over the parameter's whole range, a repeating one `cycles` times (default 1). With
/// `point` in place of the range: the segment from the point before it to it, between the two
/// points' heights (the whole range when they are level), a repeating shape once a beat unless
/// `cycles` says.
pub(crate) fn insert_shape(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let target = target(session, params)?;
    let name = Args::new(params).raw_str("shape").ok_or("shape must be a shape's name")?;
    if shape_points(name).is_none() {
        return Err(format!("no shape {name:?}; the shapes are {}", SHAPES.join(", ")).into());
    }
    let asked = cycles(params)?;
    let depth = share(params, "depth")?.unwrap_or(1.0);
    let (start, end, heights, cycles) = match Args::new(params).value("point") {
        None => {
            let (start, end) = range(params)?;
            // A shape less than the whole travel swings around the lane's value where the range
            // starts (or `center`), kept inside the travel.
            let center = match share(params, "center")? {
                Some(center) => center,
                None => {
                    let points = points_of(session, &target)?;
                    let lane = Lane { target: &target, session };
                    value_at(&points, start, target.stepped(), &lane).map_or(0.5, |v| target.position(session, v))
                }
            };
            let low = (center - depth / 2.0).clamp(0.0, 1.0 - depth);
            (start, end, (low, low + depth), asked.unwrap_or(1))
        }
        Some(v) => {
            let points = points_of(session, &target)?;
            let index = crate::automation::point_at(v, &points)?;
            let (start, end, a, b) = segment(session, &target, &points, index)?;
            let (low, high) = if (a - b).abs() < SAME { (0.0, 1.0) } else { (a.min(b), a.max(b)) };
            // Less than the whole depth: as much of the way between the two heights, around
            // their middle.
            let middle = (low + high) / 2.0;
            let half = (high - low) / 2.0 * depth;
            (start, end, (middle - half, middle + half), asked.unwrap_or_else(|| cycles_per_beat(start, end)))
        }
    };
    let count = fill(session, &target, start, end, heights, name, cycles)?;
    Ok(json!({ "param": target.param, "shape": name, "start": start, "end": end, "points": count }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use suhina_model::automation::Plain;

    fn p(beat: f64, value: f64) -> AutomationPoint {
        AutomationPoint { beat, value, curve: Curve::STRAIGHT }
    }

    #[test]
    fn a_replaced_range_keeps_the_envelope_outside_it() {
        let points: Points = vec![p(0.0, 0.0), p(8.0, 1.0)].into();
        let out = replace(&points, 2.0, 4.0, vec![p(3.0, 0.5)], false, &Plain);
        let beats: Vec<(f64, f64)> = out.iter().map(|p| (p.beat, p.value)).collect();
        assert_eq!(beats, [(0.0, 0.0), (2.0, 0.25), (3.0, 0.5), (4.0, 0.5), (8.0, 1.0)]);
        // The points outside the range keep their ids; the ones drawn in it are new.
        assert_eq!((out.id(0), out.id(4)), (points.id(0), points.id(1)));
        assert!(out.ids()[1..4].iter().all(|id| !points.ids().contains(id)));
        for beat in [1.0, 5.0, 7.0] {
            assert!(
                (value_at(&out, beat, false, &Plain).unwrap() - value_at(&points, beat, false, &Plain).unwrap()).abs()
                    < 1e-9
            );
        }
    }

    #[test]
    fn a_slice_has_the_values_at_its_edges() {
        let points = vec![p(0.0, 0.0), p(4.0, 1.0), p(8.0, 0.0)];
        let cut: Vec<(f64, f64)> = slice(&points, 2.0, 6.0, false, &Plain).iter().map(|p| (p.beat, p.value)).collect();
        assert_eq!(cut, [(2.0, 0.5), (4.0, 1.0), (6.0, 0.5)]);
        assert!(slice(&[], 0.0, 1.0, false, &Plain).is_empty());
    }

    #[test]
    fn every_shape_starts_and_ends_its_cycle_in_range() {
        for name in SHAPES {
            let (points, _) = shape_points(name).unwrap();
            assert!(points.first().is_some_and(|p| p.0 == 0.0), "{name}");
            assert!(points.last().is_some_and(|p| (p.0 - 1.0).abs() < 1e-12), "{name}");
            assert!(points.iter().all(|p| (0.0..=1.0).contains(&p.1)), "{name}");
        }
        assert!(shape_points("spiral").is_none());
    }
}
