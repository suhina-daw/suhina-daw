// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Automation lanes in the Arrangement view (#420, `docs/spec/automation.md` › In the
//! Arrangement view): under a track, one lane per chosen parameter (its volume, pan and mute,
//! each parameter of its built-in effects and instruments), its envelope drawn over the
//! timeline and its points edited with the pointer. Every edit is an `automation.*` command;
//! which lanes are shown is the view's own state, like a lane's height.

use std::collections::{HashMap, HashSet};

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use serde_json::{Value, json};
use suhina_device_api::{ParamInfo, ParamKind, Unit};
use suhina_engine::model::automation::{MUTE, PAN, VOLUME};
use suhina_engine::model::{AutomationLane, AutomationPoint, ClipId, DeviceKind, Project, Rgb, TrackId};

use crate::interaction::timeline_grid::TimelineGrid;
use crate::kit::theme::{self, PAD};
use crate::kit::widgets::{self, Action};

/// Height of one automation lane.
pub const LANE: f32 = 40.0;
/// Height of the line under a lane.
const LINE: f32 = 1.0;
/// Space kept above and below the envelope inside a lane.
const INSET: f32 = 4.0;
/// The radius of the dot drawn for a point.
const POINT: f32 = 3.5;
/// The square around a point that takes the pointer.
const POINT_HIT: f32 = 11.0;
/// How near the envelope a drag has to start to bend a segment, and a click has to be to add a
/// point on it.
const SEGMENT_HIT: f32 = 6.0;
/// How much a point of vertical drag bends a segment's curve.
const CURVE_PER_POINT: f64 = 0.01;
/// How strongly a lane without points draws its parameter's value.
const LEVEL_ALPHA: f32 = 0.4;
/// Most points one segment of an envelope is drawn with.
const SEGMENT_POINTS: usize = 128;
/// Height of one row of a lane's header.
const HEADER_ROW: f32 = 15.0;
/// Width of the header's on/off toggle.
const TOGGLE: f32 = 30.0;
/// Side of the header's small square buttons (**+**, **×**).
const SMALL_BUTTON: f32 = 16.0;
/// Width of the lane header's parameter menu: its rows and search field fill it, no wider.
const MENU_WIDTH: f32 = 240.0;
/// Space the parameter menu leaves between itself and the window's edge.
const MENU_EDGE: f32 = 8.0;
/// The bottom of the volume's travel, as the mixer's fields show it.
const VOLUME_FLOOR: f64 = -70.0;

/// A lane: the device it belongs to (`None` for the track's mixer) and the parameter's id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LaneKey {
    pub device: Option<usize>,
    pub param: String,
}

impl LaneKey {
    /// The lane of the track's own mixer parameter `param` (volume, pan or mute).
    pub fn mixer(param: &str) -> Self {
        Self { device: None, param: param.into() }
    }

    /// The parameters that name this lane in an `automation.*` command.
    fn params(&self, track: TrackId) -> Value {
        let mut params = json!({ "track": track.0, "param": self.param });
        if let Some(device) = self.device {
            params["device"] = json!(device);
        }
        params
    }
}

/// How a parameter's values map onto a lane's height and onto the commands.
#[derive(Debug, Clone, PartialEq)]
pub enum Scale {
    /// A linear gain in the lane, decibels in commands; placed like the mixer's volume field.
    Volume,
    /// -1 (left) to 1 (right).
    Pan,
    /// 0 or 1, a step.
    Mute,
    /// A built-in device parameter, placed as its knob turns.
    Device(ParamInfo),
    /// A data-only device parameter, placed linearly or logarithmically as its control requires.
    Sd(Box<suhina_device_sdk::Parameter>),
}

impl Scale {
    /// Where `value` (as the lane keeps it) sits on the lane, 0 at the bottom to 1 at the top.
    pub fn position(&self, value: f64) -> f64 {
        let position = match self {
            Self::Volume => volume_position(gain_to_db(value)),
            Self::Pan => (value + 1.0) / 2.0,
            Self::Mute => value,
            Self::Device(info) => info.to_normalized(value),
            Self::Sd(parameter) => sd_position(parameter, value),
        };
        if position.is_finite() { position.clamp(0.0, 1.0) } else { 0.0 }
    }

    /// The value at `position` (0 … 1) as the lane keeps it.
    pub fn value_at(&self, position: f64) -> f64 {
        let position = if position.is_finite() { position.clamp(0.0, 1.0) } else { 0.0 };
        match self {
            Self::Volume => db_at(position).map_or(0.0, |db| 10f64.powf(db / 20.0)),
            Self::Pan => (position * 2.0 - 1.0).clamp(-1.0, 1.0),
            Self::Mute => position.round(),
            Self::Device(info) => info.from_normalized(position),
            Self::Sd(parameter) => sd_value(parameter, position),
        }
    }

    /// A value as the parameter's set command takes it: decibels (or "-inf") for the volume.
    pub fn command(&self, value: f64) -> Value {
        match self {
            Self::Volume => match self.number(value) {
                db if value <= 0.0 || db <= VOLUME_FLOOR => json!("-inf"),
                db => json!(db),
            },
            _ => json!(self.number(value)),
        }
    }

    /// A value as `automation.addPoint` takes it, unrounded, so a point added on the envelope
    /// keeps the envelope's value exactly.
    fn exact(&self, value: f64) -> Value {
        match self {
            Self::Volume if value <= 0.0 => json!("-inf"),
            Self::Volume => json!(gain_to_db(value).min(6.0)),
            _ => json!(value),
        }
    }

    /// Whether `automation.addPoint` keeps `value` as it is: a whole-number parameter (an integer,
    /// a toggle, a choice) rounds it, which would move a point spliced onto a slope.
    fn keeps(&self, value: f64) -> bool {
        match self {
            Self::Device(info) => info.clamp(value) == value,
            Self::Sd(parameter) => parameter.clamp(value) == value,
            Self::Volume | Self::Pan | Self::Mute => true,
        }
    }

    /// A value in the unit of the set command, as a number: the volume's silence is its floor.
    fn number(&self, value: f64) -> f64 {
        match self {
            Self::Volume => round(gain_to_db(value).max(VOLUME_FLOOR), 100.0),
            Self::Pan => round(value, 1000.0),
            Self::Mute => value.round(),
            Self::Device(info) => info.clamp(value),
            Self::Sd(parameter) => parameter.clamp(value),
        }
    }

    /// `value` moved by `delta` in the unit of the set command, as `automation.movePoints`
    /// moves it: kept in range, and a silent volume stays silent.
    fn shift(&self, value: f64, delta: f64) -> f64 {
        match self {
            Self::Volume if value <= 0.0 => 0.0,
            Self::Volume => 10f64.powf((gain_to_db(value) + delta).min(6.0) / 20.0),
            Self::Pan => (value + delta).clamp(-1.0, 1.0),
            Self::Mute => (value + delta).clamp(0.0, 1.0).round(),
            Self::Device(info) => info.clamp(value + delta),
            Self::Sd(parameter) => parameter.clamp(value + delta),
        }
    }

    /// Whether each point's value holds until the next (on/off and choices).
    pub fn stepped(&self) -> bool {
        match self {
            Self::Mute => true,
            Self::Device(info) => matches!(info.kind, ParamKind::Toggle | ParamKind::Choice(_)),
            Self::Sd(parameter) => parameter.control.is_stepped(),
            Self::Volume | Self::Pan => false,
        }
    }

    /// A value as the lane's header shows it.
    pub fn text(&self, value: f64) -> String {
        match self {
            Self::Volume => format!("{} dB", widgets::format_db(gain_to_db(value))),
            Self::Pan => widgets::format_pan(value),
            Self::Mute => if value >= 0.5 { "Muted" } else { "Sounding" }.into(),
            Self::Device(info) => match info.kind {
                ParamKind::Toggle => if value >= 0.5 { "On" } else { "Off" }.into(),
                ParamKind::Choice(options) => {
                    options.get(info.clamp(value) as usize).map_or_else(String::new, |o| (*o).to_string())
                }
                ParamKind::Integer => {
                    let value = info.clamp(value);
                    match info.unit {
                        Unit::Semitones => format!("{value:+} st"),
                        Unit::ScaleDegrees => format!("{value:+} sd"),
                        Unit::Key => crate::detail::piano_roll::key_name(value.clamp(0.0, 127.0) as u8),
                        _ => format!("{value}"),
                    }
                }
                ParamKind::Continuous(_) => match info.unit {
                    Unit::Decibels => format!("{value:.1} dB"),
                    Unit::Hertz => format!("{value:.0} Hz"),
                    Unit::Seconds => format!("{:.0} ms", value * 1000.0),
                    Unit::Percent => format!("{:.0} %", value * 100.0),
                    Unit::Balance => widgets::format_pan(value),
                    Unit::Octaves => format!("{value:.2} oct"),
                    Unit::Semitones => format!("{value:+.2} st"),
                    Unit::ScaleDegrees => format!("{value:+.2} sd"),
                    Unit::Key => crate::detail::piano_roll::key_name(value.round().clamp(0.0, 127.0) as u8),
                    Unit::None => format!("{value:.2}"),
                },
            },
            Self::Sd(parameter) => match parameter.control {
                suhina_device_sdk::ParameterControl::Toggle | suhina_device_sdk::ParameterControl::Button => {
                    if value >= 0.5 { "On" } else { "Off" }.into()
                }
                suhina_device_sdk::ParameterControl::Choice | suhina_device_sdk::ParameterControl::Radio => {
                    // A loaded point outside the graph's range names its nearest option.
                    let value = parameter.clamp(value);
                    let index = (value - parameter.min).max(0.0) as usize;
                    parameter.ui.options.get(index).cloned().unwrap_or_else(|| format!("{value}"))
                }
                suhina_device_sdk::ParameterControl::Integer => format!("{}", value.round()),
                _ if !parameter.unit.is_empty() => format!("{value:.2} {}", parameter.unit),
                _ => format!("{value:.2}"),
            },
        }
    }
}

/// Whether a log control places `parameter` logarithmically (only over a positive range).
fn sd_log(parameter: &suhina_device_sdk::Parameter) -> bool {
    matches!(
        parameter.control,
        suhina_device_sdk::ParameterControl::LogSlider | suhina_device_sdk::ParameterControl::LogKnob
    ) && parameter.min > 0.0
}

/// Where `value` sits on a lane of a data-only device's `parameter`, 0 … 1: linear, or
/// logarithmic for a log control; the middle for a parameter with no range.
fn sd_position(parameter: &suhina_device_sdk::Parameter, value: f64) -> f64 {
    let (min, max) = (parameter.min, parameter.max);
    let value = parameter.clamp(value);
    if min >= max {
        0.5
    } else if sd_log(parameter) {
        (value.ln() - min.ln()) / (max.ln() - min.ln())
    } else {
        (value - min) / (max - min)
    }
}

/// The value of a data-only device's `parameter` at `position` (0 … 1) on its lane: the
/// inverse of [`sd_position`], held in the parameter's range.
fn sd_value(parameter: &suhina_device_sdk::Parameter, position: f64) -> f64 {
    let (min, max) = (parameter.min, parameter.max);
    let position = position.clamp(0.0, 1.0);
    let value = if min >= max {
        min
    } else if sd_log(parameter) {
        (min.ln() + position * (max.ln() - min.ln())).exp()
    } else {
        min + position * (max - min)
    };
    parameter.clamp(value)
}

/// `value` rounded to the nearest `1 / per`: `round(x, 1000.0)` keeps three decimals.
fn round(value: f64, per: f64) -> f64 {
    (value * per).round() / per
}

/// A linear gain in decibels; minus infinity for silence (a gain of 0 or less).
fn gain_to_db(gain: f64) -> f64 {
    if gain > 0.0 { 20.0 * gain.log10() } else { f64::NEG_INFINITY }
}

/// Where a volume in decibels sits on a lane, 0 … 1: as the mixer's volume field places it
/// (`arrangement_view::volume_position`, 0 dB at 85 %), in full precision so a point dragged
/// keeps its value.
fn volume_position(db: f64) -> f64 {
    if !db.is_finite() || db <= VOLUME_FLOOR {
        0.0
    } else if db <= 0.0 {
        let x = (db - VOLUME_FLOOR) / -VOLUME_FLOOR;
        0.85 * x * x
    } else {
        0.85 + 0.15 * (db / 6.0).min(1.0)
    }
}

/// The volume in decibels at `position` on a lane: the inverse of the volume field's placing
/// (0 dB at 85 %); `None` for silence at the bottom.
fn db_at(position: f64) -> Option<f64> {
    if position <= 0.0 {
        None
    } else if position <= 0.85 {
        Some(VOLUME_FLOOR * (1.0 - (position / 0.85).sqrt()))
    } else {
        Some(((position - 0.85) / 0.15 * 6.0).min(6.0))
    }
}

/// A parameter a track's lanes can show: what it is called, how its values map, and its lane's
/// points and whether it plays (no points: it has no automation yet).
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub key: LaneKey,
    pub name: String,
    /// The device's name, for a device's parameter: the chooser groups by it.
    pub device_name: Option<String>,
    /// The part of its device it belongs to ("Op 1", "LFO"): the chooser gathers a group's
    /// parameters under its name.
    pub group: Option<String>,
    pub scale: Scale,
    pub points: Vec<AutomationPoint>,
    pub enabled: bool,
    /// The parameter's value set on the mixer or the device: where a lane without points draws
    /// its faint line.
    pub current: f64,
}

/// Every track's parameters that lanes can show, built from the project.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Params {
    tracks: HashMap<TrackId, Vec<Param>>,
}

impl Params {
    /// Every track's parameters from `project`: its mixer's volume, pan and mute, then each
    /// built-in and `.sd` device's parameters in device order, with their lanes' points.
    pub fn build(project: &Project) -> Self {
        let mut tracks = HashMap::new();
        for track in &project.tracks {
            let Some(channel) = track.channel.and_then(|id| project.channel(id)) else {
                continue;
            };
            let lane_of = |lanes: &[AutomationLane], id: &str| lanes.iter().find(|l| l.param == id).cloned();
            let mut params: Vec<Param> = [
                (VOLUME, "Volume", Scale::Volume, channel.volume),
                (PAN, "Pan", Scale::Pan, channel.pan),
                (MUTE, "Mute", Scale::Mute, if channel.mute { 1.0 } else { 0.0 }),
            ]
            .into_iter()
            .map(|(id, name, scale, current)| {
                let lane = lane_of(&channel.automation, id);
                Param {
                    key: LaneKey::mixer(id),
                    name: name.into(),
                    device_name: None,
                    group: None,
                    scale,
                    enabled: lane.as_ref().is_none_or(|l| l.enabled),
                    points: lane.map(|l| l.points).unwrap_or_default(),
                    current,
                }
            })
            .collect();
            for (index, device) in channel.devices.iter().enumerate() {
                let set = |id: &str| device.params.iter().find(|p| p.id == id).map(|p| p.value);
                match &device.kind {
                    DeviceKind::Builtin(id) => {
                        let Some(info) = suhina_device_registry::info(id) else { continue };
                        for spec in info.params {
                            let lane = lane_of(&device.automation, spec.id);
                            params.push(Param {
                                key: LaneKey { device: Some(index), param: spec.id.into() },
                                name: spec.name.into(),
                                device_name: Some(device.name.clone()),
                                group: (!spec.group.is_empty()).then(|| spec.group.to_string()),
                                scale: Scale::Device(*spec),
                                enabled: lane.as_ref().is_none_or(|l| l.enabled),
                                points: lane.map(|l| l.points).unwrap_or_default(),
                                current: set(spec.id).unwrap_or(spec.default),
                            });
                        }
                    }
                    DeviceKind::Sd => {
                        let Some(graph) = device
                            .state
                            .as_deref()
                            .and_then(|bytes| std::str::from_utf8(bytes).ok())
                            .and_then(|text| suhina_device_sdk::DeviceGraph::from_sd(text).ok())
                        else {
                            continue;
                        };
                        for spec in &graph.parameters {
                            let lane = lane_of(&device.automation, &spec.id);
                            params.push(Param {
                                key: LaneKey { device: Some(index), param: spec.id.clone() },
                                name: spec.name.clone(),
                                device_name: Some(device.name.clone()),
                                group: (!spec.ui.group.is_empty()).then(|| spec.ui.group.clone()),
                                scale: Scale::Sd(Box::new(spec.clone())),
                                enabled: lane.as_ref().is_none_or(|l| l.enabled),
                                points: lane.map(|l| l.points).unwrap_or_default(),
                                current: set(&spec.id).unwrap_or(spec.default),
                            });
                        }
                    }
                    _ => {}
                }
            }
            tracks.insert(track.id, params);
        }
        Self { tracks }
    }

    /// `track`'s parameters, in the order the chooser lists them; none for a track without a
    /// channel.
    pub fn of(&self, track: TrackId) -> &[Param] {
        self.tracks.get(&track).map_or(&[], Vec::as_slice)
    }

    /// `track`'s parameter that `key` names, if the track still has it.
    fn param(&self, track: TrackId, key: &LaneKey) -> Option<&Param> {
        self.of(track).iter().find(|p| &p.key == key)
    }
}

impl Param {
    /// The lane's name in its header and labels: "Volume", "Utility: Gain".
    pub fn label(&self) -> String {
        match &self.device_name {
            Some(device) => format!("{device}: {}", self.name),
            None => self.name.clone(),
        }
    }

    /// The name under its group's heading, without the group's name in front ("Rate 1" for
    /// "Op 1 Rate 1" in "Op 1"); the whole name when it does not start with the group's or is
    /// nothing more.
    pub fn short_name(&self) -> &str {
        self.group
            .as_deref()
            .and_then(|group| self.name.strip_prefix(group))
            .and_then(|rest| rest.strip_prefix(' '))
            .filter(|rest| !rest.trim().is_empty())
            .unwrap_or(&self.name)
    }

    /// What a search looks in: the device, the group and the name, lower case.
    fn search_text(&self) -> String {
        [self.device_name.as_deref(), self.group.as_deref(), Some(self.name.as_str())]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    }
}

/// Points selected on one lane.
#[derive(Debug, Clone, PartialEq)]
struct Selection {
    track: TrackId,
    key: LaneKey,
    points: Vec<usize>,
}

/// A point being dragged (with the rest of the selection when it is part of one): where the
/// pointer took hold of it and where it is now, in beats and lane positions before snapping.
#[derive(Debug, Clone, PartialEq)]
struct PointDrag {
    track: TrackId,
    key: LaneKey,
    point: usize,
    together: Vec<usize>,
    grab: (f64, f64),
    now: (f64, f64),
}

/// A segment's curve being bent: the segment's first point and its curve so far.
#[derive(Debug, Clone, PartialEq)]
struct CurveDrag {
    track: TrackId,
    key: LaneKey,
    point: usize,
    curve: f64,
}

/// A box drawn over a lane to select the points in it, from where it started to the pointer.
#[derive(Debug, Clone, PartialEq)]
struct BoxDrag {
    track: TrackId,
    key: LaneKey,
    from: Pos2,
    to: Pos2,
}

/// What the lanes keep between frames: which tracks show lanes and which, the selected points
/// and a drag in progress.
#[derive(Debug, Default)]
pub struct LanesState {
    open: HashSet<TrackId>,
    shown: HashMap<TrackId, Vec<LaneKey>>,
    selection: Option<Selection>,
    point_drag: Option<PointDrag>,
    curve_drag: Option<CurveDrag>,
    box_drag: Option<BoxDrag>,
    /// The lanes whose parameter was chosen in the parameter menu's "Automated" section: the
    /// menu opens there again for them, and on the parameter's own row for the rest.
    from_automated: HashSet<(TrackId, LaneKey)>,
    /// The clip selected in the view last frame: selecting a clip ends a selection of points.
    clip: Option<(TrackId, ClipId)>,
}

impl LanesState {
    /// Whether `track` shows its automation lanes.
    pub fn is_open(&self, track: TrackId) -> bool {
        self.open.contains(&track)
    }

    /// The lanes `track` shows, in order, those `params` still has.
    pub fn shown(&self, track: TrackId, params: &Params) -> Vec<LaneKey> {
        if !self.is_open(track) {
            return Vec::new();
        }
        let keys = self.shown.get(&track).map_or(&[][..], Vec::as_slice);
        keys.iter().filter(|k| params.param(track, k).is_some()).cloned().collect()
    }

    /// How much taller `track`'s row is for its lanes.
    pub fn height(&self, track: TrackId, params: &Params) -> f32 {
        self.shown(track, params).len() as f32 * (LANE + LINE)
    }

    /// Shows or hides `track`'s lanes. Shown the first time, they are the lanes that have
    /// points, or the volume's when none has.
    pub fn toggle(&mut self, track: TrackId, params: &Params) {
        if !self.open.remove(&track) {
            self.open.insert(track);
            let shown = self.shown.entry(track).or_default();
            shown.retain(|k| params.param(track, k).is_some());
            if shown.is_empty() {
                shown.extend(params.of(track).iter().filter(|p| !p.points.is_empty()).map(|p| p.key.clone()));
            }
            if shown.is_empty() {
                shown.push(LaneKey::mixer(VOLUME));
            }
        }
        if self.selection.as_ref().is_some_and(|s| s.track == track) {
            self.selection = None;
        }
    }

    /// Shows every one of `tracks`' lanes, or hides them all when they all show already
    /// (`view.automation`, A).
    pub fn toggle_all(&mut self, tracks: &[TrackId], params: &Params) {
        let all_open = !tracks.is_empty() && tracks.iter().all(|t| self.is_open(*t));
        for &track in tracks {
            if self.is_open(track) == all_open {
                self.toggle(track, params);
            }
        }
    }

    /// Shows `key` in `track`'s lanes (opening them), after the lanes already shown.
    pub fn show_lane(&mut self, track: TrackId, key: LaneKey) {
        self.open.insert(track);
        let shown = self.shown.entry(track).or_default();
        if !shown.contains(&key) {
            shown.push(key);
        }
    }

    /// The selected points: their track, lane and indices.
    pub fn selected_points(&self) -> Option<(TrackId, &LaneKey, &[usize])> {
        self.selection.as_ref().map(|s| (s.track, &s.key, s.points.as_slice()))
    }

    /// The command that deletes the selected points, ending the selection; `None` without one.
    pub fn delete_selected(&mut self) -> Option<Action> {
        let selection = self.selection.take().filter(|s| !s.points.is_empty())?;
        let mut params = selection.key.params(selection.track);
        params["points"] = json!(selection.points);
        Some(Action::new("automation.deletePoints", params))
    }

    /// Called once a frame before the lanes are drawn: a clip selected since the last frame ends
    /// the selection of points.
    pub fn follow_clip(&mut self, clip: Option<(TrackId, ClipId)>) {
        if clip.is_some() && clip != self.clip {
            self.selection = None;
        }
        self.clip = clip;
    }

    /// Whether `point` of `track`'s lane `key` is selected.
    fn is_selected(&self, track: TrackId, key: &LaneKey, point: usize) -> bool {
        self.selection.as_ref().is_some_and(|s| s.track == track && &s.key == key && s.points.contains(&point))
    }

    /// Selects `point` of `track`'s lane `key`: alone, or with `add` (Shift or Ctrl) added to the
    /// selection on that lane or taken out of it.
    fn select(&mut self, track: TrackId, key: &LaneKey, point: usize, add: bool) {
        match self.selection.as_mut().filter(|s| add && s.track == track && &s.key == key) {
            Some(selection) => match selection.points.iter().position(|p| *p == point) {
                Some(at) => {
                    selection.points.remove(at);
                }
                None => selection.points.push(point),
            },
            None => self.selection = Some(Selection { track, key: key.clone(), points: vec![point] }),
        }
    }
}

/// Where the lanes sit and how beats map to x: what the Arrangement view knows of its timeline.
#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    /// The timeline's x range and the header column's.
    pub time: egui::Rangef,
    pub headers: egui::Rangef,
    /// The lanes' visible area: nothing is drawn or hit outside it.
    pub visible: Rect,
    pub scroll_x: f32,
    pub beat_width: f32,
    /// The arrangement's grid at this zoom: points snap to the lines it draws.
    pub grid: TimelineGrid,
}

impl Geometry {
    /// Where `beat` is on screen, after the timeline's scroll.
    fn x_of(&self, beat: f64) -> f32 {
        self.time.min - self.scroll_x + beat as f32 * self.beat_width
    }

    /// The beat at `x` on screen: the inverse of [`Geometry::x_of`].
    fn beat_of(&self, x: f32) -> f64 {
        f64::from((x - self.time.min + self.scroll_x) / self.beat_width)
    }

    /// `beat` on the nearest grid line, or free (to a thousandth) with `free` or a grid that does
    /// not snap; never before the start.
    fn snap(&self, beat: f64, free: bool) -> f64 {
        let beat = if free || !self.grid.snaps { round(beat, 1000.0) } else { self.grid.place(beat, false, false) };
        if beat.is_finite() { beat.max(0.0) } else { 0.0 }
    }
}

/// The envelope's area inside a lane.
fn plot(lane: Rect) -> Rect {
    lane.shrink2(Vec2::new(0.0, INSET))
}

/// The height in `plot` of a lane `position` (0 at the bottom, 1 at the top).
fn y_of(plot: Rect, position: f64) -> f32 {
    plot.bottom() - position as f32 * plot.height()
}

/// The lane position (0 … 1) at height `y` in `plot`: the inverse of [`y_of`], held in the
/// lane.
fn position_of(plot: Rect, y: f32) -> f64 {
    if plot.height() > 0.0 { f64::from((plot.bottom() - y) / plot.height()).clamp(0.0, 1.0) } else { 0.0 }
}

/// A track colour from the project as egui draws it.
fn color(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.0, rgb.1, rgb.2)
}

/// The small envelope button in a track header that shows and hides its lanes.
pub fn header_button(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    track: TrackId,
    name: &str,
    state: &mut LanesState,
    params: &Params,
) {
    let open = state.is_open(track);
    let label = if open { format!("Hide automation of {name}") } else { format!("Show automation of {name}") };
    let response = widgets::button_area(ui, rect, Id::new(("automation-toggle", track.0)), label);
    if response.clicked() {
        state.toggle(track, params);
    }
    let hover = widgets::hover(ui, &response);
    let off = crate::kit::motion::mix(theme::control_background(), theme::control_hover(), hover);
    painter.rect_filled(rect, 0.0, if open { theme::control_active() } else { off });
    // A small envelope: up, along, down.
    let glyph = rect.shrink2(Vec2::new(4.0, 4.0));
    let line = vec![
        glyph.left_bottom(),
        Pos2::new(glyph.left() + glyph.width() * 0.35, glyph.top()),
        Pos2::new(glyph.left() + glyph.width() * 0.65, glyph.top()),
        glyph.right_bottom(),
    ];
    let ink = if open { theme::text_primary() } else { theme::text_secondary() };
    painter.add(Shape::line(line, Stroke::new(1.0, ink)));
}

/// What one track's lanes need from the view.
pub struct TrackLanes<'a> {
    pub track: TrackId,
    pub name: &'a str,
    pub color: Rgb,
    /// The top of the first lane, just under the track's own lane.
    pub top: f32,
}

/// Draws `track`'s lanes from `top` down and turns gestures on them into actions.
pub fn show(
    ui: &mut Ui,
    painter: &egui::Painter,
    geometry: &Geometry,
    lanes: &TrackLanes<'_>,
    params: &Params,
    state: &mut LanesState,
    actions: &mut Vec<Action>,
) {
    let mut y = lanes.top;
    for key in state.shown(lanes.track, params) {
        let Some(param) = params.param(lanes.track, &key) else { continue };
        let time = Rect::from_x_y_ranges(geometry.time, egui::Rangef::new(y, y + LANE));
        let header = Rect::from_x_y_ranges(geometry.headers, time.y_range());
        y += LANE + LINE;
        let full = Rect::from_x_y_ranges(geometry.visible.x_range(), time.y_range());
        if full.bottom() < geometry.visible.top() || full.top() > geometry.visible.bottom() {
            continue;
        }
        lane(ui, painter, geometry, lanes, param, time, state, actions);
        lane_header(ui, painter, geometry, lanes, param, header, state, params, actions);
        painter.hline(geometry.visible.x_range(), time.bottom() + 0.5, theme::hairline(theme::border()));
    }
}

/// The points a lane shows: the project's, or where a drag in progress puts them.
fn shown_points(
    param: &Param,
    track: TrackId,
    geometry: &Geometry,
    free: bool,
    state: &LanesState,
) -> Vec<AutomationPoint> {
    let mut points = param.points.clone();
    if let Some(drag) = state.point_drag.as_ref().filter(|d| d.track == track && d.key == param.key) {
        let (beats, delta) = drag_delta(drag, param, geometry, free);
        for &index in &drag.together {
            if let Some(point) = points.get_mut(index) {
                point.beat += beats;
                point.value = moved_value(param, drag, point.value, delta);
            }
        }
        points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
    }
    if let Some(drag) = state.curve_drag.as_ref().filter(|d| d.track == track && d.key == param.key)
        && let Some(point) = points.get_mut(drag.point)
    {
        point.curve = drag.curve;
    }
    points
}

/// How far a point drag moves its points: beats (the grabbed point onto the grid, unless
/// `free`, and none before beat 0) and the value change in the set command's unit, worked out
/// from the grabbed point.
fn drag_delta(drag: &PointDrag, param: &Param, geometry: &Geometry, free: bool) -> (f64, f64) {
    let Some(grabbed) = param.points.get(drag.point) else { return (0.0, 0.0) };
    let target = geometry.snap(grabbed.beat + drag.now.0 - drag.grab.0, free);
    let earliest =
        drag.together.iter().filter_map(|i| param.points.get(*i)).map(|p| p.beat).fold(grabbed.beat, f64::min);
    let beats = (target - grabbed.beat).max(-earliest);
    let position = param.scale.position(grabbed.value) + drag.now.1 - drag.grab.1;
    let new = param.scale.value_at(position);
    let delta = param.scale.number(new) - param.scale.number(grabbed.value);
    (beats, delta)
}

/// Where a point drag puts a value: one point lands where the pointer is; points moved
/// together shift by the same amount.
fn moved_value(param: &Param, drag: &PointDrag, value: f64, delta: f64) -> f64 {
    if drag.together.len() == 1 {
        let start = param.points.get(drag.point).map_or(value, |p| p.value);
        param.scale.value_at(param.scale.position(start) + drag.now.1 - drag.grab.1)
    } else {
        param.scale.shift(value, delta)
    }
}

/// The envelope's line across the visible part of a lane, in points of `plot`.
fn envelope(points: &[AutomationPoint], scale: &Scale, plot: Rect, geometry: &Geometry) -> Vec<Pos2> {
    let (Some(first), Some(last)) = (points.first(), points.last()) else { return Vec::new() };
    let at = |beat: f64, value: f64| Pos2::new(geometry.x_of(beat), y_of(plot, scale.position(value)));
    let mut line = vec![Pos2::new(plot.left(), at(0.0, first.value).y), at(first.beat, first.value)];
    for pair in points.windows(2) {
        let [a, b] = pair else { continue };
        let (xa, xb) = (geometry.x_of(a.beat), geometry.x_of(b.beat));
        if xb < plot.left() || xa > plot.right() || xb <= xa {
            line.push(at(b.beat, if scale.stepped() { a.value } else { b.value }));
            line.push(at(b.beat, b.value));
            continue;
        }
        if scale.stepped() {
            line.push(at(b.beat, a.value));
        } else {
            // Sampled even when straight: a straight segment of the volume is straight in gain,
            // which is not straight on the lane.
            let steps = (((xb - xa) / 2.0).ceil() as usize).clamp(2, SEGMENT_POINTS);
            for step in 1..steps {
                let t = step as f64 / steps as f64;
                let shaped = suhina_engine::model::automation::shape(t, a.curve);
                let value = a.value + (b.value - a.value) * shaped;
                line.push(at(a.beat + (b.beat - a.beat) * t, value));
            }
        }
        line.push(at(b.beat, b.value));
    }
    line.push(Pos2::new(plot.right(), at(last.beat, last.value).y));
    line
}

/// The points a click on a lane is measured against: the lane's own, or for a lane without
/// points one at the parameter's value, its faint line.
fn level_points(param: &Param) -> Vec<AutomationPoint> {
    if param.points.is_empty() { vec![AutomationPoint::new(0.0, param.current)] } else { param.points.clone() }
}

/// How far `pos` is from the nearest part of `line`; infinite for no line.
fn distance_to(line: &[Pos2], pos: Pos2) -> f32 {
    line.windows(2)
        .filter_map(|pair| match pair {
            [a, b] => {
                let along = *b - *a;
                let length = along.length_sq();
                let t = if length > 0.0 { ((pos - *a).dot(along) / length).clamp(0.0, 1.0) } else { 0.0 };
                Some(pos.distance(*a + along * t))
            }
            _ => None,
        })
        .fold(f32::INFINITY, f32::min)
}

/// A point at `beat` that leaves the envelope of `points` as it is: the envelope's value there,
/// and the curve of the segment it starts. The segment before it keeps its curve, which is exact
/// (a power curve cut short is the same power curve); the one after it gets the curve through
/// the old segment's middle, exact for a straight segment. `None` for no points.
fn splice(points: &[AutomationPoint], beat: f64, stepped: bool) -> Option<(f64, f64)> {
    use suhina_engine::model::automation::{shape, value_at};
    let value = value_at(points, beat, stepped)?;
    let index = points.partition_point(|p| p.beat <= beat).checked_sub(1);
    let segment = index.and_then(|i| Some((points.get(i)?, points.get(i + 1)?)));
    let Some((a, b)) = segment.filter(|(a, b)| !stepped && a.curve != 0.0 && b.beat > a.beat) else {
        return Some((value, 0.0));
    };
    let t = (beat - a.beat) / (b.beat - a.beat);
    let start = shape(t, a.curve);
    let middle = (shape(t + (1.0 - t) / 2.0, a.curve) - start) / (1.0 - start);
    // `shape(0.5, c)` is 0.5 to the power of 4^-c.
    let curve = -(middle.ln() / 0.5f64.ln()).ln() / 4f64.ln();
    let curve =
        if middle > 0.0 && middle < 1.0 && curve.is_finite() { round(curve.clamp(-1.0, 1.0), 1000.0) } else { 0.0 };
    Some((value, curve))
}

/// The segment of `points` under `pos`, near enough to its line to bend it: the index of its
/// first point. Steps have no curve.
fn segment_at(points: &[AutomationPoint], scale: &Scale, plot: Rect, geometry: &Geometry, pos: Pos2) -> Option<usize> {
    if scale.stepped() {
        return None;
    }
    let beat = geometry.beat_of(pos.x);
    let index = points.partition_point(|p| p.beat <= beat).checked_sub(1)?;
    let (a, b) = (points.get(index)?, points.get(index + 1)?);
    if b.beat <= a.beat || a.value == b.value {
        return None;
    }
    let value = suhina_engine::model::automation::value_at(points, beat, false)?;
    ((y_of(plot, scale.position(value)) - pos.y).abs() <= SEGMENT_HIT).then_some(index)
}

/// Draws one lane of `param` in `time` (its envelope, points and drag in progress) and turns
/// clicks and drags on it into `automation.*` actions.
#[allow(clippy::too_many_arguments)]
fn lane(
    ui: &mut Ui,
    painter: &egui::Painter,
    geometry: &Geometry,
    lanes: &TrackLanes<'_>,
    param: &Param,
    time: Rect,
    state: &mut LanesState,
    actions: &mut Vec<Action>,
) {
    let track = lanes.track;
    let key = &param.key;
    let visible = time.intersect(geometry.visible);
    let plot = plot(time);
    let free = ui.input(|i| i.modifiers.alt);
    let id = Id::new(("automation-lane", track.0, key.device, key.param.as_str()));
    let label = param.label();

    // The lane itself: a click adds a point (on the envelope when near it, so the envelope keeps
    // its shape; where the pointer is otherwise), a drag near the line bends its segment, any
    // other drag draws a box that selects the points in it.
    let body = ui.interact(visible, id, Sense::click_and_drag());
    body.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("{label} automation on {}", lanes.name))
    });
    if body.clicked()
        && let Some(pos) = body.interact_pointer_pos()
    {
        let mut params = key.params(track);
        let beat = geometry.snap(geometry.beat_of(pos.x), free);
        params["beat"] = json!(beat);
        let level = level_points(param);
        let near = distance_to(&envelope(&level, &param.scale, plot, geometry), pos) <= SEGMENT_HIT;
        // On a whole-number lane a value between two numbers cannot be kept: the point goes where
        // the pointer is, as off the line.
        match splice(&level, beat, param.scale.stepped()).filter(|(value, _)| near && param.scale.keeps(*value)) {
            Some((value, curve)) => {
                params["value"] = param.scale.exact(value);
                if curve != 0.0 {
                    params["curve"] = json!(curve);
                }
            }
            None => params["value"] = param.scale.command(param.scale.value_at(position_of(plot, pos.y))),
        }
        actions.push(Action::new("automation.addPoint", params));
        state.selection = None;
    }
    // A drag starts once the pointer has moved a little: it is measured from where it was pressed.
    let pressed_at = ui.input(|i| i.pointer.press_origin());
    if body.drag_started_by(egui::PointerButton::Primary)
        && let Some(pos) = pressed_at.or_else(|| body.interact_pointer_pos())
    {
        match segment_at(&param.points, &param.scale, plot, geometry, pos) {
            Some(point) => {
                let curve = param.points.get(point).map_or(0.0, |p| p.curve);
                state.curve_drag = Some(CurveDrag { track, key: key.clone(), point, curve });
            }
            None => state.box_drag = Some(BoxDrag { track, key: key.clone(), from: pos, to: pos }),
        }
    }
    if body.dragged_by(egui::PointerButton::Primary) {
        let dy = body.drag_delta().y;
        if let Some(drag) = state.curve_drag.as_mut().filter(|d| d.track == track && &d.key == key) {
            // Up bends the line up: a rising segment moves fast first, a falling one late.
            let rising = match (param.points.get(drag.point), param.points.get(drag.point + 1)) {
                (Some(a), Some(b)) => param.scale.position(b.value) >= param.scale.position(a.value),
                _ => true,
            };
            let sign = if rising { 1.0 } else { -1.0 };
            drag.curve = (drag.curve - f64::from(dy) * CURVE_PER_POINT * sign).clamp(-1.0, 1.0);
        }
        if let Some(drag) = state.box_drag.as_mut().filter(|d| d.track == track && &d.key == key)
            && let Some(pos) = body.interact_pointer_pos()
        {
            drag.to = pos;
        }
    }
    if body.drag_stopped_by(egui::PointerButton::Primary) {
        if let Some(drag) = state.curve_drag.take_if(|d| d.track == track && &d.key == key) {
            let before = param.points.get(drag.point).map_or(0.0, |p| p.curve);
            if (drag.curve - before).abs() > 1e-6 {
                let mut params = key.params(track);
                params["point"] = json!(drag.point);
                params["curve"] = json!(round(drag.curve, 1000.0));
                actions.push(Action::new("automation.movePoint", params));
            }
        }
        if let Some(drag) = state.box_drag.take_if(|d| d.track == track && &d.key == key) {
            let area = Rect::from_two_pos(drag.from, drag.to);
            let points: Vec<usize> = param
                .points
                .iter()
                .enumerate()
                .filter(|(_, p)| {
                    area.contains(Pos2::new(geometry.x_of(p.beat), y_of(plot, param.scale.position(p.value))))
                })
                .map(|(i, _)| i)
                .collect();
            state.selection = (!points.is_empty()).then(|| Selection { track, key: key.clone(), points });
        }
    }

    // The envelope: in the track colour while it plays, dimmed while a manual change has turned
    // it off.
    let lane_painter = painter.with_clip_rect(visible);
    let points = shown_points(param, track, geometry, free, state);
    let ink = if param.enabled { color(lanes.color) } else { theme::text_hint() };
    let line = envelope(&points, &param.scale, plot, geometry);
    if line.len() > 1 {
        lane_painter.add(Shape::line(line, Stroke::new(1.5, ink)));
    } else if points.is_empty() {
        // No automation yet: a faint line at the parameter's value, where a click adds a point.
        let y = y_of(plot, param.scale.position(param.current));
        lane_painter.hline(plot.x_range(), y, Stroke::new(1.0, ink.gamma_multiply(LEVEL_ALPHA)));
    }
    if let Some(drag) = state.box_drag.as_ref().filter(|d| d.track == track && &d.key == key) {
        let area = Rect::from_two_pos(drag.from, drag.to);
        lane_painter.rect_filled(area, 0.0, theme::accent().gamma_multiply(0.15));
        lane_painter.rect_stroke(area, 0.0, theme::hairline(theme::accent()), StrokeKind::Inside);
    }

    // The points: a click selects (Shift or Ctrl adds), a drag moves (the selection with it), a
    // double-click or the menu deletes.
    let dragging = state.point_drag.as_ref().filter(|d| d.track == track && &d.key == key).map(|d| d.point);
    for (index, point) in param.points.iter().enumerate() {
        // Drawn where a drag puts it.
        let shown = match dragging {
            Some(_) => {
                let drag = state.point_drag.as_ref();
                let moved = drag.filter(|d| d.together.contains(&index)).map(|d| {
                    let (beats, delta) = drag_delta(d, param, geometry, free);
                    (point.beat + beats, moved_value(param, d, point.value, delta))
                });
                moved.unwrap_or((point.beat, point.value))
            }
            None => (point.beat, point.value),
        };
        let center = Pos2::new(geometry.x_of(shown.0), y_of(plot, param.scale.position(shown.1)));
        if !visible.expand(POINT_HIT).contains(center) {
            continue;
        }
        let hit = Rect::from_center_size(center, Vec2::splat(POINT_HIT)).intersect(visible);
        if hit.width() <= 0.0 || hit.height() <= 0.0 {
            continue;
        }
        let point_id = id.with(("point", index));
        let response = ui.interact(hit, point_id, Sense::click_and_drag());
        let text = param.scale.text(point.value);
        response.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("{label} point {} on {}", index + 1, lanes.name),
            );
            info.current_text_value = Some(text.clone());
            info
        });
        let response =
            response.on_hover_text(format!("{} at beat {}", param.scale.text(point.value), round(point.beat, 1000.0)));
        let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        if response.double_clicked() {
            let mut params = key.params(track);
            params["point"] = json!(index);
            actions.push(Action::new("automation.deletePoint", params));
            state.selection = None;
        } else if response.clicked() {
            state.select(track, key, index, add);
        }
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(pos) = pressed_at.or_else(|| response.interact_pointer_pos())
        {
            if !state.is_selected(track, key, index) {
                state.select(track, key, index, add);
            }
            let together = state
                .selection
                .as_ref()
                .filter(|s| s.track == track && &s.key == key)
                .map_or_else(|| vec![index], |s| s.points.clone());
            let grab = (geometry.beat_of(pos.x), position_of(plot, pos.y));
            state.point_drag = Some(PointDrag { track, key: key.clone(), point: index, together, grab, now: grab });
        }
        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
            && let Some(drag) =
                state.point_drag.as_mut().filter(|d| d.track == track && &d.key == key && d.point == index)
        {
            drag.now = (geometry.beat_of(pos.x), position_of(plot, pos.y));
        }
        if response.drag_stopped_by(egui::PointerButton::Primary)
            && let Some(drag) = state.point_drag.take_if(|d| d.track == track && &d.key == key && d.point == index)
        {
            if let Some(action) = drop_points(&drag, param, geometry, free) {
                actions.push(action);
            }
            // The moved points keep no selection: their indices change with the edit.
            if drag.together.len() > 1 {
                state.selection = None;
            }
        }
        let selected = state.is_selected(track, key, index);
        response.context_menu(|ui| {
            let several = state.selection.as_ref().filter(|s| selected && s.points.len() > 1);
            if let Some(selection) = several {
                if ui.button(format!("Delete {} points", selection.points.len())).clicked() {
                    if let Some(action) = state.delete_selected() {
                        actions.push(action);
                    }
                    ui.close();
                }
            } else if ui.button("Delete point").clicked() {
                let mut params = key.params(track);
                params["point"] = json!(index);
                actions.push(Action::new("automation.deletePoint", params));
                state.selection = None;
                ui.close();
            }
        });
        let hover = widgets::hover(ui, &response);
        let radius = POINT + hover;
        let fill = if selected { theme::text_primary() } else { ink };
        lane_painter.circle_filled(center, radius, fill);
        lane_painter.circle_stroke(center, radius, Stroke::new(1.0, theme::window_background()));
    }
}

/// The command a finished point drag asks for: `automation.movePoint` for one point,
/// `automation.movePoints` for several; nothing when they did not move.
fn drop_points(drag: &PointDrag, param: &Param, geometry: &Geometry, free: bool) -> Option<Action> {
    let point = param.points.get(drag.point)?;
    let (beats, delta) = drag_delta(drag, param, geometry, free);
    let mut params = drag.key.params(drag.track);
    if drag.together.len() > 1 {
        if beats == 0.0 && delta == 0.0 {
            return None;
        }
        params["points"] = json!(drag.together);
        params["beats"] = json!(round(beats, 1000.0));
        params["value"] = json!(round(delta, 1000.0));
        return Some(Action::new("automation.movePoints", params));
    }
    let value = moved_value(param, drag, point.value, delta);
    let moved_value = param.scale.number(value) != param.scale.number(point.value);
    if beats == 0.0 && !moved_value {
        return None;
    }
    params["point"] = json!(drag.point);
    params["beat"] = json!(round(point.beat + beats, 1000.0));
    params["value"] = param.scale.command(value);
    Some(Action::new("automation.movePoint", params))
}

/// The words of a search, lower case: a parameter matches when its device, group and name have
/// all of them between them.
fn query_words(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

/// Whether `param` matches a search (`words` from [`query_words`]; none matches everything).
fn matches_query(param: &Param, words: &[String]) -> bool {
    let text = param.search_text();
    words.iter().all(|w| text.contains(w.as_str()))
}

/// One device's part of the parameter menu, or the track mixer's: its parameters without a
/// group, then each group in the order its first parameter comes.
#[derive(Debug)]
struct Section<'a> {
    device: Option<usize>,
    name: Option<&'a str>,
    loose: Vec<&'a Param>,
    groups: Vec<(&'a str, Vec<&'a Param>)>,
}

impl<'a> Section<'a> {
    /// Its parameters in the order the menu lists them.
    fn params(&self) -> impl Iterator<Item = &'a Param> + '_ {
        self.loose.iter().copied().chain(self.groups.iter().flat_map(|(_, params)| params.iter().copied()))
    }
}

/// The parameter menu's sections: the `choices` that match a search (`words`), by device in the
/// order they come, each with its groups.
fn sections<'a>(choices: &'a [Param], words: &[String]) -> Vec<Section<'a>> {
    let mut sections: Vec<Section<'a>> = Vec::new();
    for choice in choices.iter().filter(|c| matches_query(c, words)) {
        if sections.last().is_none_or(|s| s.device != choice.key.device) {
            sections.push(Section {
                device: choice.key.device,
                name: choice.device_name.as_deref(),
                loose: Vec::new(),
                groups: Vec::new(),
            });
        }
        let Some(section) = sections.last_mut() else { continue };
        match choice.group.as_deref() {
            None => section.loose.push(choice),
            Some(group) => match section.groups.iter_mut().find(|(g, _)| *g == group) {
                Some((_, params)) => params.push(choice),
                None => section.groups.push((group, vec![choice])),
            },
        }
    }
    sections
}

/// What the parameter menu keeps while it is open: the search, whether it has been shown yet,
/// the devices opened (by index) and the groups opened (by device and name). A device keeps its
/// groups open while it is closed.
#[derive(Debug, Clone, Default)]
struct MenuState {
    query: String,
    shown: bool,
    devices: HashSet<usize>,
    open: HashSet<(Option<usize>, String)>,
}

/// A device's or a group's heading in the parameter menu: an arrow, its name, and how many
/// parameters it has (" •" when one of them has points). A click opens or closes it
/// (`toggles`), except while a search shows every heading open.
fn heading_row(ui: &mut Ui, name: &str, count: usize, marked: bool, open: bool, toggles: bool) -> egui::Response {
    let height = ui.spacing().interact_size.y;
    let sense = if toggles { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), sense);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if toggles && response.hovered() {
            painter.rect_filled(rect, 2.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }
        widgets::chevron(painter, Pos2::new(rect.left() + 6.0, rect.center().y), open, theme::text_hint());
        let font = egui::TextStyle::Button.resolve(ui.style());
        let text = Pos2::new(rect.left() + 14.0, rect.center().y);
        painter.text(text, Align2::LEFT_CENTER, name, font.clone(), ui.visuals().text_color());
        let count = format!("{count}{}", if marked { " •" } else { "" });
        let end = Pos2::new(rect.right() - 4.0, rect.center().y);
        painter.text(end, Align2::RIGHT_CENTER, count, font, theme::text_hint());
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, true, open, name));
    response
}

/// A parameter's row in the menu: its name (short under a group's heading) and " •" when it has
/// points; selected for the lane's own, greyed out when it is not `free`. Its accessible label is
/// the whole "Device: Name". Answers whether it was clicked.
fn parameter_row(ui: &mut Ui, choice: &Param, current: &LaneKey, free: bool, scroll: bool) -> bool {
    let mark = if choice.points.is_empty() { "" } else { " •" };
    let selected = choice.key == *current;
    let button = egui::Button::selectable(selected, format!("{}{mark}", choice.short_name()));
    let response = ui.add_enabled(free, button);
    let label = choice.label();
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, free, selected, &label));
    if scroll && selected {
        response.scroll_to_me(Some(egui::Align::Center));
    }
    response.clicked()
}

/// A parameter's copy in the menu's "Automated" section: its whole name ("Utility: Gain"), as it
/// stands outside its device's heading, selected for the lane's own (scrolled to on `scroll`)
/// and greyed out when it is not `free`. Its accessible label ends in " (automated)", apart from
/// its row in its device. Answers whether it was clicked.
fn automated_row(ui: &mut Ui, choice: &Param, current: &LaneKey, free: bool, scroll: bool) -> bool {
    let selected = choice.key == *current;
    let label = choice.label();
    let response = ui.add_enabled(free, egui::Button::selectable(selected, &label));
    let label = format!("{label} (automated)");
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, free, selected, &label));
    if scroll && selected {
        response.scroll_to_me(Some(egui::Align::Center));
    }
    response.clicked()
}

/// The lane header's parameter menu: a search field and, under it, the track's parameters in a
/// list that scrolls when it is longer than the window (the FM synth has over a hundred): those
/// with points first under "Automated", a shortcut that a search narrows like the rest and that
/// does not fold, then the mixer's, then each device under a heading and, inside a device, by
/// group. Devices and
/// groups start closed except those holding the lane's own parameter (`current`), which is
/// scrolled into view, and a track's only device; a search shows the devices and groups with
/// matches open, and Enter takes the first match. A parameter another lane shows (`shown`) is
/// greyed out. The menu fits in `room`, the height on its side of the dropdown, so it grows as
/// headings open without moving. The lane's own parameter is scrolled to where it was chosen:
/// its copy under "Automated" when it was chosen there (`from_automated`) and still has points,
/// its own row otherwise; its device and group open either way. Answers the one chosen, and
/// whether it was chosen under "Automated".
fn parameter_menu(
    ui: &mut Ui,
    filter: Id,
    choices: &[Param],
    current: &LaneKey,
    shown: &[LaneKey],
    from_automated: bool,
    room: f32,
) -> Option<(LaneKey, bool)> {
    ui.set_width(MENU_WIDTH);
    let mut menu: MenuState = ui.data(|d| d.get_temp(filter)).unwrap_or_default();
    let first_frame = !menu.shown;
    if first_frame {
        if let Some(device) = current.device {
            menu.devices.insert(device);
        }
        if let Some(group) = choices.iter().find(|c| c.key == *current).and_then(|c| c.group.clone()) {
            menu.open.insert((current.device, group));
        }
        let devices: HashSet<usize> = choices.iter().filter_map(|c| c.key.device).collect();
        if let [only] = devices.into_iter().collect::<Vec<_>>()[..] {
            menu.devices.insert(only);
        }
    }
    let field_id = filter.with("field");
    // Enter ends the field's focus, so whether it had it is read before the field is shown.
    let had_focus = ui.memory(|m| m.has_focus(field_id));
    let field = ui.add(
        egui::TextEdit::singleline(&mut menu.query)
            .id(field_id)
            .hint_text("Search parameters")
            .font(theme::ui_font(theme::TEXT_SMALL))
            .desired_width(f32::INFINITY),
    );
    field.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search parameters"));
    if first_frame {
        field.request_focus();
    }
    let enter = had_focus && ui.input(|i| i.key_pressed(egui::Key::Enter));

    let words = query_words(&menu.query);
    let searching = !words.is_empty();
    let sections = sections(choices, &words);
    let automated: Vec<&Param> = sections.iter().flat_map(Section::params).filter(|c| !c.points.is_empty()).collect();
    let free = |choice: &Param| !shown.contains(&choice.key) || choice.key == *current;
    // Where the menu opens: on the copy under "Automated" when the lane's parameter was chosen
    // there, on its own row otherwise.
    let to_top = from_automated && automated.iter().any(|c| c.key == *current);
    let mut chosen = None;
    // What is left of the room under the search field and the menu's margins.
    let used = ui.min_rect().height() + ui.spacing().menu_margin.sum().y + ui.spacing().item_spacing.y + MENU_EDGE;
    let height = (ui.ctx().content_rect().height() * 0.6).min(room - used).max(80.0);
    // The list's room is its own most, not what the menu had the frame before: after a heading
    // opens, the menu grows to fit it rather than the list scrolling in the old height.
    ui.allocate_ui(Vec2::new(ui.available_width(), height), |ui| {
        egui::ScrollArea::vertical().max_height(height).auto_shrink([true, true]).show(ui, |ui| {
            if sections.is_empty() {
                ui.label(egui::RichText::new("No parameter matches").color(theme::text_hint()));
            }
            // Every parameter with points, again at the top: their devices may be closed.
            if !automated.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new("Automated").color(theme::text_hint()));
                for choice in &automated {
                    if automated_row(ui, choice, current, free(choice), first_frame && to_top) {
                        chosen = Some((choice.key.clone(), true));
                    }
                }
            }
            for section in &sections {
                ui.separator();
                // The mixer's few parameters have no heading.
                let Some(device) = section.device else {
                    for choice in &section.loose {
                        if parameter_row(ui, choice, current, free(choice), first_frame && !to_top) {
                            chosen = Some((choice.key.clone(), false));
                        }
                    }
                    continue;
                };
                let name = section.name.unwrap_or("Device");
                let marked = section.params().any(|p| !p.points.is_empty());
                let open = searching || menu.devices.contains(&device);
                if heading_row(ui, name, section.params().count(), marked, open, !searching).clicked()
                    && !menu.devices.remove(&device)
                {
                    menu.devices.insert(device);
                }
                if !(searching || menu.devices.contains(&device)) {
                    continue;
                }
                ui.indent(("device", device), |ui| {
                    for choice in &section.loose {
                        if parameter_row(ui, choice, current, free(choice), first_frame && !to_top) {
                            chosen = Some((choice.key.clone(), false));
                        }
                    }
                    for (group, params) in &section.groups {
                        let key = (section.device, (*group).to_string());
                        let marked = params.iter().any(|p| !p.points.is_empty());
                        let open = searching || menu.open.contains(&key);
                        if heading_row(ui, group, params.len(), marked, open, !searching).clicked()
                            && !menu.open.remove(&key)
                        {
                            menu.open.insert(key.clone());
                        }
                        if searching || menu.open.contains(&key) {
                            ui.indent(&key, |ui| {
                                for choice in params {
                                    if parameter_row(ui, choice, current, free(choice), first_frame && !to_top) {
                                        chosen = Some((choice.key.clone(), false));
                                    }
                                }
                            });
                        }
                    }
                });
            }
        });
    });
    if chosen.is_none() && enter {
        let top = automated.iter().find(|c| free(c)).map(|c| (c.key.clone(), true));
        let rest = || sections.iter().flat_map(Section::params).find(|c| free(c)).map(|c| (c.key.clone(), false));
        chosen = top.or_else(rest);
    }
    menu.shown = true;
    ui.data_mut(|d| d.insert_temp(filter, menu));
    if chosen.is_some() {
        ui.close();
    }
    chosen
}

/// A lane's header, under the track's: which parameter (a menu to choose another), whether its
/// automation plays (a manual change turns it off; the toggle turns it on again), a button that
/// adds a lane and one that hides this one.
#[allow(clippy::too_many_arguments)]
fn lane_header(
    ui: &mut Ui,
    painter: &egui::Painter,
    geometry: &Geometry,
    lanes: &TrackLanes<'_>,
    param: &Param,
    rect: Rect,
    state: &mut LanesState,
    params: &Params,
    actions: &mut Vec<Action>,
) {
    let clip = rect.intersect(geometry.visible);
    if clip.width() <= 0.0 || clip.height() <= 0.0 {
        return;
    }
    let painter = painter.with_clip_rect(clip);
    painter.rect_filled(rect, 0.0, theme::panel_background());
    let track = lanes.track;
    let id = Id::new(("automation-header", track.0, param.key.device, param.key.param.as_str()));
    let inner = rect.shrink2(Vec2::new(PAD, 5.0));
    let row = Rect::from_min_size(inner.min, Vec2::new(inner.width(), HEADER_ROW));
    let hide =
        Rect::from_min_size(Pos2::new(row.right() - SMALL_BUTTON, row.top()), Vec2::new(SMALL_BUTTON, HEADER_ROW));
    let add = Rect::from_min_size(
        Pos2::new(hide.left() - 2.0 - SMALL_BUTTON, row.top()),
        Vec2::new(SMALL_BUTTON, HEADER_ROW),
    );
    let has_lane = !param.points.is_empty();
    let toggle = Rect::from_min_size(Pos2::new(add.left() - 4.0 - TOGGLE, row.top()), Vec2::new(TOGGLE, HEADER_ROW));
    let chooser = Rect::from_min_max(row.min, Pos2::new(toggle.left() - 4.0, row.bottom()));
    let label = param.label();

    // The parameter: a dropdown of every parameter the track has, by device.
    let response = widgets::button_area(
        ui,
        chooser.intersect(clip),
        id.with("param"),
        format!("Parameter of {label} lane on {}", lanes.name),
    );
    widgets::dropdown(ui, &painter, chooser, &response, &label);
    let shown = state.shown(track, params);
    let filter = id.with("filter");
    // Under the dropdown, or over it when there is more room there; on that side only, so the
    // menu never jumps to another as it grows.
    let window = ui.ctx().content_rect();
    let below = window.bottom() - response.rect.bottom();
    let above = response.rect.top() - window.top();
    let (align, room) =
        if below >= above { (egui::RectAlign::BOTTOM_START, below) } else { (egui::RectAlign::TOP_START, above) };
    // Only a click outside closes it: the search field and the headings take clicks, and
    // choosing a parameter closes it itself. Outside is where the menu was when the click came,
    // as a heading the click closes shrinks the menu in the same frame, maybe from under it.
    let placed = filter.with("rect");
    let last: Option<Rect> = ui.data(|d| d.get_temp(placed));
    let outside = ui.input(|i| {
        i.pointer.any_click()
            && i.pointer
                .interact_pos()
                .is_some_and(|p| last.is_some_and(|r| !r.contains(p)) && !response.rect.contains(p))
    });
    let menu = egui::Popup::menu(&response)
        .align(align)
        .align_alternatives(&[])
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks);
    let this_lane = (track, param.key.clone());
    let from_automated = state.from_automated.contains(&this_lane);
    let choices = params.of(track);
    match menu.show(|ui| parameter_menu(ui, filter, choices, &param.key, &shown, from_automated, room)) {
        Some(menu) => {
            // A menu over the dropdown is placed by its size the frame before: when that changed
            // (a heading, a search), the frame is drawn again rather than shown in the old place.
            let rect = menu.response.rect;
            if last.is_some_and(|r| r.size() != rect.size()) {
                ui.ctx().request_discard("the parameter menu changed size");
            }
            ui.data_mut(|d| d.insert_temp(placed, rect));
            if outside {
                egui::Popup::close_id(ui.ctx(), egui::Popup::default_response_id(&response));
            }
            if let Some((choice, top)) = menu.inner {
                // The lane's ids follow its parameter, so the menu's state is dropped under the
                // old ones now rather than when a header with them is next drawn closed: a lane
                // switched back opens its menu afresh even in that header's first frame.
                forget_menu(ui, filter, placed);
                state.from_automated.remove(&this_lane);
                if top {
                    state.from_automated.insert((track, choice.clone()));
                }
                if let Some(keys) = state.shown.get_mut(&track)
                    && let Some(slot) = keys.iter_mut().find(|k| **k == param.key)
                {
                    *slot = choice;
                }
                state.selection = None;
            }
        }
        // Closed: the next opening starts with an empty search, its groups closed, and no place.
        None => forget_menu(ui, filter, placed),
    }

    // On or off: off after a manual change; a click turns it on again (or off).
    if has_lane {
        let text = if param.enabled { "On" } else { "Off" };
        let what = if param.enabled { "Turn off" } else { "Turn on again" };
        let response = widgets::button_area(
            ui,
            toggle.intersect(clip),
            id.with("enabled"),
            format!("{what} {label} automation on {}", lanes.name),
        );
        if response.clicked() {
            let mut params = param.key.params(track);
            params["enabled"] = json!(!param.enabled);
            actions.push(Action::new("automation.setEnabled", params));
        }
        let response = if param.enabled {
            response
        } else {
            response.on_hover_text("A manual change turned this automation off; click to follow it again")
        };
        let (fill, ink) = widgets::toggle_colors(ui, &response, param.enabled);
        painter.rect_filled(toggle, 0.0, fill);
        if !param.enabled {
            painter.rect_stroke(toggle, 0.0, theme::hairline(theme::accent()), StrokeKind::Inside);
        }
        widgets::text_in(&painter, toggle, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_SMALL), ink);
    }

    // Add a lane: the next parameter not shown yet.
    let next = params.of(track).iter().find(|p| !shown.contains(&p.key)).map(|p| p.key.clone());
    if let Some(next) = next {
        let response = widgets::button_area(
            ui,
            add.intersect(clip),
            id.with("add"),
            format!("Add an automation lane to {}", lanes.name),
        );
        if response.clicked() {
            state.show_lane(track, next);
        }
        small_button(ui, &painter, add, &response, "+");
    }
    let response =
        widgets::button_area(ui, hide.intersect(clip), id.with("hide"), format!("Hide {label} lane on {}", lanes.name));
    if response.clicked() {
        // A hidden lane draws no header to drop its menu's state when the menu closes.
        forget_menu(ui, filter, placed);
        state.from_automated.remove(&this_lane);
        if let Some(keys) = state.shown.get_mut(&track) {
            keys.retain(|k| *k != param.key);
            if keys.is_empty() {
                state.open.remove(&track);
            }
        }
        if state.selection.as_ref().is_some_and(|s| s.track == track && s.key == param.key) {
            state.selection = None;
        }
    }
    small_button(ui, &painter, hide, &response, "×");

    // Second row: the value at the playhead's side of things, the lane's first point, or a hint.
    let second = Rect::from_min_size(Pos2::new(inner.left(), row.bottom() + 3.0), Vec2::new(inner.width(), HEADER_ROW));
    let (text, ink) = match (param.points.first(), param.enabled) {
        (None, _) => ("Click the lane to add a point".to_string(), theme::text_hint()),
        (Some(_), false) => ("Off: set by hand".to_string(), theme::text_secondary()),
        (Some(_), true) => (format!("{} points", param.points.len()), theme::text_hint()),
    };
    widgets::text_in(&painter, second, Align2::LEFT_CENTER, &text, theme::ui_font(theme::TEXT_SMALL), ink);
}

/// Drops what a lane's parameter menu keeps while it is open (its [`MenuState`] under `filter`,
/// its place under `placed`), so the next opening starts afresh.
fn forget_menu(ui: &Ui, filter: Id, placed: Id) {
    ui.data_mut(|d| {
        d.remove_temp::<MenuState>(filter);
        d.remove::<Rect>(placed);
    });
}

/// Draws one of a lane header's small buttons: its background, lighter under the pointer, and
/// `text` in its middle.
fn small_button(ui: &Ui, painter: &egui::Painter, rect: Rect, response: &egui::Response, text: &str) {
    let fill =
        crate::kit::motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, response));
    painter.rect_filled(rect, 0.0, fill);
    widgets::text_in(painter, rect, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT), theme::text_secondary());
}

#[cfg(test)]
#[path = "automation_lanes_tests.rs"]
mod tests;
