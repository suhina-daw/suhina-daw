// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Automation commands (#395, `docs/spec/automation.md`): points on the lanes of a track's
//! mixer (`volume`, `pan`, `mute`) and of its devices' parameters, a lane switched off and on,
//! and the lanes listed. A parameter is named by `track`, `device` (left out for the mixer) and
//! `param`; values are in the unit the matching set command takes (`track.setVolume`'s
//! decibels for the volume, `device.setParam`'s for a device). Each edit is one undo step.

use serde_json::{Value, json};
use suhina_engine::{MAX_START_BEAT, param_id};
use suhina_model::automation::{MAX_POINTS, MAX_VOLUME, MUTE, VOLUME, mixer_range};
use suhina_model::{AutomationLane, AutomationPoint, DeviceKind, TrackId};

use crate::commands::{channel_param, number, to_db, track_id};
use crate::devices::{device_index, is_plugin, plugin_param_id, plugin_value, sd_graph};
use crate::{CommandError, Session};

/// Below this many decibels a volume point is silence, as for `track.setVolume`.
const SILENCE_DB: f64 = -150.0;

/// The parameter a command names, checked.
struct Target {
    track: TrackId,
    /// The device's index on the track; `None` for the track's mixer.
    device: Option<usize>,
    /// The parameter's id as lanes store it.
    param: String,
    kind: Kind,
}

#[derive(Clone)]
enum Kind {
    /// The mixer's volume: decibels in commands, a linear gain in the lane.
    Volume,
    /// The mixer's pan (-1 to 1) or mute (0 or 1).
    Plain,
    /// A built-in device parameter, limited and rounded as the device takes it.
    Builtin(&'static suhina_device_api::ParamInfo),
    /// A data-only device parameter's range and discrete behavior.
    Sd(Box<suhina_device_sdk::Parameter>),
    /// A plug-in parameter by its id.
    Plugin(u32),
}

fn target(session: &Session, params: &Value) -> Result<Target, CommandError> {
    let param = params.get("param").and_then(Value::as_str).ok_or("param must be a parameter id")?;
    let track = track_id(&session.project, params)?;
    let channel = channel_of(session, track)?;
    match params.get("device") {
        None | Some(Value::Null) => {
            if mixer_range(param).is_none() {
                return Err(format!(
                    "{param:?} is not a mixer parameter: volume, pan or mute (give device for a device's parameter)"
                )
                .into());
            }
            let kind = if param == VOLUME { Kind::Volume } else { Kind::Plain };
            Ok(Target { track, device: None, param: param.into(), kind })
        }
        Some(_) => {
            let index = device_index(channel.devices.len(), params)?;
            let device = channel.devices.get(index).ok_or("the device is missing")?;
            if is_plugin(&device.kind) {
                let id = plugin_param_id(param)?;
                return Ok(Target { track, device: Some(index), param: id.to_string(), kind: Kind::Plugin(id) });
            }
            let kind = match &device.kind {
                DeviceKind::Builtin(id) => {
                    let info = suhina_device_registry::info(id)
                        .ok_or_else(|| format!("{id:?} is not a device this version knows"))?;
                    let spec = info
                        .param_index(param)
                        .and_then(|i| info.params.get(i))
                        .ok_or_else(|| format!("{} has no parameter {param:?}; devices.list lists them", info.name))?;
                    Kind::Builtin(spec)
                }
                DeviceKind::Sd => {
                    let graph = sd_graph(device)?;
                    let spec = graph
                        .parameters
                        .into_iter()
                        .find(|p| p.id == param)
                        .ok_or_else(|| format!("{} has no parameter {param:?}", graph.name))?;
                    Kind::Sd(Box::new(spec))
                }
                _ => {
                    return Err("this device has no parameters Suhina can automate (it is from another program)".into());
                }
            };
            Ok(Target { track, device: Some(index), param: param.into(), kind })
        }
    }
}

fn channel_of(session: &Session, track: TrackId) -> Result<&suhina_model::Channel, CommandError> {
    let id = session.project.tracks.iter().find(|t| t.id == track).and_then(|t| t.channel);
    id.and_then(|id| session.project.channels.iter().find(|c| c.id == id))
        .ok_or_else(|| "the track has no mixer channel".into())
}

impl Target {
    /// `value` from a command as the lane keeps it.
    fn store(&self, session: &Session, value: f64) -> Result<f64, CommandError> {
        match &self.kind {
            Kind::Volume => {
                if value > 6.0 {
                    return Err(format!("a volume must be at most +6 dB, got {value}").into());
                }
                Ok(if value < SILENCE_DB { 0.0 } else { 10f64.powf(value / 20.0).min(MAX_VOLUME) })
            }
            Kind::Plain => {
                let (min, max) = mixer_range(&self.param).unwrap_or((value, value));
                if !(min..=max).contains(&value) {
                    return Err(format!("{} must be from {min} to {max}, got {value}", self.param).into());
                }
                Ok(if self.param == MUTE { value.round() } else { value })
            }
            Kind::Builtin(spec) => Ok(spec.clamp(value)),
            Kind::Sd(parameter) if value.is_finite() => Ok(parameter.clamp(value)),
            Kind::Sd(_) => Err("value must be finite".into()),
            Kind::Plugin(id) => plugin_value(session, self.track, &[self.device.unwrap_or(0)], *id, value),
        }
    }

    /// A lane's value as commands show it.
    fn show(&self, value: f64) -> Value {
        match &self.kind {
            Kind::Volume => to_db(value).map_or(json!("-inf"), |db| json!(db)),
            _ => json!(value),
        }
    }

    /// The lanes the parameter's lane is among.
    fn lanes<'a>(&self, session: &'a mut Session) -> Result<&'a mut Vec<AutomationLane>, CommandError> {
        let channel = channel_param(session, &json!({ "track": self.track.0 }))?;
        match self.device {
            None => Ok(&mut channel.automation),
            Some(index) => Ok(&mut channel.devices.get_mut(index).ok_or("the device is missing")?.automation),
        }
    }

    fn matches(&self, lane: &AutomationLane) -> bool {
        match &self.kind {
            Kind::Plugin(id) => param_id(&lane.param) == Some(*id),
            _ => lane.param == self.param,
        }
    }

    fn lane<'a>(&self, session: &'a mut Session) -> Result<&'a mut AutomationLane, CommandError> {
        let name = self.param.clone();
        self.lanes(session)?
            .iter_mut()
            .find(|l| self.matches(l))
            .ok_or_else(|| format!("{name} has no automation; automation.addPoint makes it").into())
    }
}

fn beat(params: &Value) -> Result<f64, CommandError> {
    let beat = number(params, "beat")?;
    if !(0.0..=MAX_START_BEAT).contains(&beat) {
        return Err(format!("beat must be from 0 to {MAX_START_BEAT}, got {beat}").into());
    }
    Ok(beat)
}

fn curve(params: &Value) -> Result<Option<f64>, CommandError> {
    match params.get("curve") {
        None | Some(Value::Null) => Ok(None),
        Some(_) => {
            let curve = number(params, "curve")?;
            if !(-1.0..=1.0).contains(&curve) {
                return Err(format!("curve must be from -1 to 1, got {curve}").into());
            }
            Ok(Some(curve))
        }
    }
}

/// A value from `params[name]`: a number, or for the volume also "-inf"; for the mute also
/// true or false.
fn value(params: &Value, name: &str) -> Result<f64, CommandError> {
    match params.get(name) {
        Some(Value::String(s)) if s == "-inf" => Ok(f64::NEG_INFINITY),
        Some(Value::Bool(b)) => Ok(if *b { 1.0 } else { 0.0 }),
        _ => number(params, name),
    }
}

fn point_index(params: &Value, count: usize) -> Result<usize, CommandError> {
    let index = params
        .get("point")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or("point must be a point index, from 0, as in automation.list")?;
    if index >= count {
        return Err(format!("the lane has no point {index}; it has {count}").into());
    }
    Ok(index)
}

pub(crate) fn add_point(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let target = target(session, params)?;
    let beat = beat(params)?;
    let value = target.store(session, value(params, "value")?)?;
    let curve = curve(params)?.unwrap_or(0.0);
    let lanes = target.lanes(session)?;
    let lane = match lanes.iter().position(|l| target.matches(l)) {
        Some(index) => lanes.get_mut(index).ok_or("the lane is missing")?,
        None => {
            lanes.push(AutomationLane::new(target.param.clone()));
            lanes.last_mut().ok_or("the lane is missing")?
        }
    };
    if lane.points.len() >= MAX_POINTS {
        return Err(format!("a lane holds at most {MAX_POINTS} points").into());
    }
    let index = lane.insert(AutomationPoint { beat, value, curve });
    Ok(json!({ "param": target.param, "point": index, "points": lane.points.len(), "enabled": lane.enabled }))
}

pub(crate) fn move_point(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let target = target(session, params)?;
    let new_beat = if params.get("beat").is_some_and(|v| !v.is_null()) { Some(beat(params)?) } else { None };
    let new_value = match params.get("value") {
        None | Some(Value::Null) => None,
        Some(_) => Some(target.store(session, value(params, "value")?)?),
    };
    let new_curve = curve(params)?;
    if new_beat.is_none() && new_value.is_none() && new_curve.is_none() {
        return Err("give a new beat, value or curve".into());
    }
    let lane = target.lane(session)?;
    let index = point_index(params, lane.points.len())?;
    let mut point = lane.points.remove(index);
    point.beat = new_beat.unwrap_or(point.beat);
    point.value = new_value.unwrap_or(point.value);
    point.curve = new_curve.unwrap_or(point.curve);
    let index = lane.insert(point);
    Ok(
        json!({ "param": target.param, "point": index, "beat": point.beat, "value": target.show(point.value), "curve": point.curve }),
    )
}

pub(crate) fn delete_point(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let target = target(session, params)?;
    let lane = target.lane(session)?;
    let index = point_index(params, lane.points.len())?;
    lane.points.remove(index);
    let left = lane.points.len();
    // A lane without points is gone.
    if left == 0 {
        target.lanes(session)?.retain(|l| !target.matches(l));
    }
    Ok(json!({ "param": target.param, "points": left }))
}

/// The point indices `params["points"]` names: each once, each on a lane of `count` points.
fn point_list(params: &Value, count: usize) -> Result<Vec<usize>, CommandError> {
    let list = params.get("points").and_then(Value::as_array).ok_or("points must be a list of point indices")?;
    if list.is_empty() {
        return Err("points must name at least one point".into());
    }
    let mut indices = Vec::with_capacity(list.len().min(count));
    for item in list {
        let index = item
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or("points must be point indices, from 0, as in automation.list")?;
        if index >= count {
            return Err(format!("the lane has no point {index}; it has {count}").into());
        }
        if indices.contains(&index) {
            return Err(format!("point {index} is named twice").into());
        }
        indices.push(index);
    }
    Ok(indices)
}

/// An optional number in `params[name]`.
fn optional_number(params: &Value, name: &str) -> Result<Option<f64>, CommandError> {
    match params.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => number(params, name).map(Some),
    }
}

impl Target {
    /// A lane value moved by `delta` in the unit of the parameter's set command, kept in range.
    fn shift(&self, session: &Session, value: f64, delta: f64) -> Result<f64, CommandError> {
        match &self.kind {
            // Silence stays silent; a level never goes past the fader's top.
            Kind::Volume => match to_db(value) {
                None => Ok(0.0),
                Some(db) => self.store(session, (db + delta).min(6.0)),
            },
            Kind::Plain => {
                let (min, max) = mixer_range(&self.param).unwrap_or((value, value));
                self.store(session, (value + delta).clamp(min, max))
            }
            Kind::Builtin(_) | Kind::Sd(_) | Kind::Plugin(_) => self.store(session, value + delta),
        }
    }
}

/// Moves several points of one lane together, by `beats` and by `value` (in the unit of the
/// parameter's set command), in one undo step. Values are kept in the parameter's range; beats
/// must stay inside the arrangement.
pub(crate) fn move_points(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let target = target(session, params)?;
    let beats = optional_number(params, "beats")?.unwrap_or(0.0);
    let delta = optional_number(params, "value")?.unwrap_or(0.0);
    let curve = curve(params)?;
    if beats == 0.0 && delta == 0.0 && curve.is_none() {
        return Err("give beats or value to move the points by, or a curve".into());
    }
    let mut points = target.lane(session)?.points.clone();
    let indices = point_list(params, points.len())?;
    let mut moved = Vec::with_capacity(indices.len());
    for &index in &indices {
        let mut point = *points.get(index).ok_or("the point is missing")?;
        point.beat += beats;
        if !(0.0..=MAX_START_BEAT).contains(&point.beat) {
            return Err(
                format!("point {index} would move to beat {}, outside 0 to {MAX_START_BEAT}", point.beat).into()
            );
        }
        if delta != 0.0 {
            point.value = target.shift(session, point.value, delta)?;
        }
        point.curve = curve.unwrap_or(point.curve);
        moved.push(point);
    }
    // The points left in place, then the moved ones in the order given, each after any point
    // already at its beat; `None` marks a point that stayed.
    let mut sorted = indices.clone();
    sorted.sort_unstable();
    for index in sorted.into_iter().rev() {
        points.remove(index);
    }
    let mut tagged: Vec<(AutomationPoint, Option<usize>)> = points.into_iter().map(|p| (p, None)).collect();
    for (order, point) in moved.into_iter().enumerate() {
        let at = tagged.partition_point(|(p, _)| p.beat <= point.beat);
        tagged.insert(at, (point, Some(order)));
    }
    let mut now = vec![0; indices.len()];
    for (index, (_, tag)) in tagged.iter().enumerate() {
        if let Some(slot) = tag.and_then(|order| now.get_mut(order)) {
            *slot = index;
        }
    }
    target.lane(session)?.points = tagged.into_iter().map(|(p, _)| p).collect();
    Ok(json!({ "param": target.param, "points": now }))
}

/// Deletes several points of one lane in one undo step; the lane goes with its last point.
pub(crate) fn delete_points(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let target = target(session, params)?;
    let lane = target.lane(session)?;
    let mut indices = point_list(params, lane.points.len())?;
    indices.sort_unstable();
    for index in indices.into_iter().rev() {
        lane.points.remove(index);
    }
    let left = lane.points.len();
    if left == 0 {
        target.lanes(session)?.retain(|l| !target.matches(l));
    }
    Ok(json!({ "param": target.param, "points": left }))
}

pub(crate) fn clear(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let has_param = params.get("param").is_some_and(|v| !v.is_null());
    if has_param {
        let target = target(session, params)?;
        let lanes = target.lanes(session)?;
        let before = lanes.len();
        lanes.retain(|l| !target.matches(l));
        return Ok(json!({ "cleared": before - lanes.len() }));
    }
    // Every lane of the mixer, or of the device.
    let track = track_id(&session.project, params)?;
    let device = match params.get("device") {
        None | Some(Value::Null) => None,
        Some(_) => Some(device_index(channel_of(session, track)?.devices.len(), params)?),
    };
    let channel = channel_param(session, params)?;
    let lanes = match device {
        None => &mut channel.automation,
        Some(index) => &mut channel.devices.get_mut(index).ok_or("the device is missing")?.automation,
    };
    let cleared = lanes.len();
    lanes.clear();
    Ok(json!({ "cleared": cleared }))
}

pub(crate) fn set_enabled(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let enabled = params.get("enabled").and_then(Value::as_bool).ok_or("enabled must be true or false")?;
    let target = target(session, params)?;
    let lane = target.lane(session)?;
    lane.enabled = enabled;
    Ok(json!({ "param": target.param, "enabled": enabled }))
}

/// Every lane of a track: its mixer's, then each device's.
pub(crate) fn list(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_id(&session.project, params)?;
    let channel = channel_of(session, track)?;
    let lanes = |lanes: &[AutomationLane], volume_db: bool| -> Vec<Value> {
        lanes
            .iter()
            .map(|lane| {
                let show = |v: f64| {
                    if volume_db && lane.param == VOLUME { to_db(v).map_or(json!("-inf"), |db| json!(db)) } else { json!(v) }
                };
                json!({
                    "param": lane.param,
                    "enabled": lane.enabled,
                    "points": lane.points.iter().map(|p| json!({ "beat": p.beat, "value": show(p.value), "curve": p.curve })).collect::<Vec<_>>(),
                })
            })
            .collect()
    };
    let devices: Vec<Value> = channel
        .devices
        .iter()
        .enumerate()
        .filter(|(_, d)| !d.automation.is_empty())
        .map(|(index, d)| json!({ "device": index, "name": d.name, "lanes": lanes(&d.automation, false) }))
        .collect();
    Ok(json!({ "mixer": lanes(&channel.automation, true), "devices": devices }))
}

/// A manual change to a parameter that follows a lane turns the lane off, so the value set by
/// hand holds until `automation.setEnabled` turns it on again. Returns whether it did.
pub(crate) fn override_lane(lanes: &mut [AutomationLane], matches: impl Fn(&AutomationLane) -> bool) -> bool {
    match lanes.iter_mut().find(|l| matches(l) && l.enabled && !l.points.is_empty()) {
        Some(lane) => {
            lane.enabled = false;
            true
        }
        None => false,
    }
}
