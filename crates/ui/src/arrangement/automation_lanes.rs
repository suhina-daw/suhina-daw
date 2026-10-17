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

use crate::kit::theme::{self, PAD};
use crate::kit::widgets::{self, Action};

/// Height of one automation lane, and of the line under it.
pub const LANE: f32 = 40.0;
const LINE: f32 = 1.0;
/// Space kept above and below the envelope inside a lane.
const INSET: f32 = 4.0;
/// A point: the dot drawn, and the square that takes the pointer.
const POINT: f32 = 3.5;
const POINT_HIT: f32 = 11.0;
/// How near the envelope a drag has to start to bend a segment.
const SEGMENT_HIT: f32 = 6.0;
/// How much a point of vertical drag bends a segment's curve.
const CURVE_PER_POINT: f64 = 0.01;
/// Most points one segment of an envelope is drawn with.
const SEGMENT_POINTS: usize = 128;
/// Header parts: a row's height, the on/off toggle and the small buttons.
const HEADER_ROW: f32 = 15.0;
const TOGGLE: f32 = 30.0;
const SMALL_BUTTON: f32 = 16.0;
/// The bottom of the volume's travel, as the mixer's fields show it.
const VOLUME_FLOOR: f64 = -70.0;

/// A lane: the device it belongs to (`None` for the track's mixer) and the parameter's id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LaneKey {
    pub device: Option<usize>,
    pub param: String,
}

impl LaneKey {
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
    Sd { min: f64, max: f64, control: suhina_device_sdk::ParameterControl, unit: String, options: Vec<String> },
}

impl Scale {
    /// Where `value` (as the lane keeps it) sits on the lane, 0 at the bottom to 1 at the top.
    pub fn position(&self, value: f64) -> f64 {
        let position = match self {
            Self::Volume => volume_position(gain_to_db(value)),
            Self::Pan => (value + 1.0) / 2.0,
            Self::Mute => value,
            Self::Device(info) => info.to_normalized(value),
            Self::Sd { min, max, control, .. } => sd_position(value, *min, *max, *control),
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
            Self::Sd { min, max, control, .. } => sd_value(position, *min, *max, *control),
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

    /// A value in the unit of the set command, as a number: the volume's silence is its floor.
    fn number(&self, value: f64) -> f64 {
        match self {
            Self::Volume => round(gain_to_db(value).max(VOLUME_FLOOR), 100.0),
            Self::Pan => round(value, 1000.0),
            Self::Mute => value.round(),
            Self::Device(info) => info.clamp(value),
            Self::Sd { min, max, control, .. } => sd_clamp(value, *min, *max, *control),
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
            Self::Sd { min, max, control, .. } => sd_clamp(value + delta, *min, *max, *control),
        }
    }

    /// Whether each point's value holds until the next (on/off and choices).
    pub fn stepped(&self) -> bool {
        match self {
            Self::Mute => true,
            Self::Device(info) => matches!(info.kind, ParamKind::Toggle | ParamKind::Choice(_)),
            Self::Sd { control, .. } => control.is_stepped(),
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
                ParamKind::Integer => format!("{}", info.clamp(value)),
                ParamKind::Continuous(_) => match info.unit {
                    Unit::Decibels => format!("{value:.1} dB"),
                    Unit::Hertz => format!("{value:.0} Hz"),
                    Unit::Seconds => format!("{:.0} ms", value * 1000.0),
                    Unit::Percent => format!("{:.0} %", value * 100.0),
                    Unit::Balance => widgets::format_pan(value),
                    Unit::Octaves => format!("{value:.2} oct"),
                    Unit::None => format!("{value:.2}"),
                },
            },
            Self::Sd { min, control, unit, options, .. } => match control {
                suhina_device_sdk::ParameterControl::Toggle | suhina_device_sdk::ParameterControl::Button => {
                    if value >= 0.5 { "On" } else { "Off" }.into()
                }
                suhina_device_sdk::ParameterControl::Choice | suhina_device_sdk::ParameterControl::Radio => {
                    let index = (value.round() - *min).max(0.0) as usize;
                    options.get(index).cloned().unwrap_or_else(|| format!("{}", value.round()))
                }
                suhina_device_sdk::ParameterControl::Integer => format!("{}", value.round()),
                _ if !unit.is_empty() => format!("{value:.2} {unit}"),
                _ => format!("{value:.2}"),
            },
        }
    }
}

fn sd_clamp(value: f64, min: f64, max: f64, control: suhina_device_sdk::ParameterControl) -> f64 {
    let value = if value.is_finite() { value.clamp(min, max) } else { min };
    if control.is_stepped() { value.round().clamp(min, max) } else { value }
}

fn sd_position(value: f64, min: f64, max: f64, control: suhina_device_sdk::ParameterControl) -> f64 {
    let value = sd_clamp(value, min, max, control);
    if min >= max {
        0.5
    } else if matches!(
        control,
        suhina_device_sdk::ParameterControl::LogSlider | suhina_device_sdk::ParameterControl::LogKnob
    ) && min > 0.0
    {
        (value.ln() - min.ln()) / (max.ln() - min.ln())
    } else {
        (value - min) / (max - min)
    }
}

fn sd_value(position: f64, min: f64, max: f64, control: suhina_device_sdk::ParameterControl) -> f64 {
    let position = position.clamp(0.0, 1.0);
    let value = if min >= max {
        min
    } else if matches!(
        control,
        suhina_device_sdk::ParameterControl::LogSlider | suhina_device_sdk::ParameterControl::LogKnob
    ) && min > 0.0
    {
        (min.ln() + position * (max.ln() - min.ln())).exp()
    } else {
        min + position * (max - min)
    };
    sd_clamp(value, min, max, control)
}

fn round(value: f64, per: f64) -> f64 {
    (value * per).round() / per
}

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
    pub scale: Scale,
    pub points: Vec<AutomationPoint>,
    pub enabled: bool,
}

/// Every track's parameters that lanes can show, built from the project.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Params {
    tracks: HashMap<TrackId, Vec<Param>>,
}

impl Params {
    pub fn build(project: &Project) -> Self {
        let mut tracks = HashMap::new();
        for track in &project.tracks {
            let Some(channel) = track.channel.and_then(|id| project.channels.iter().find(|c| c.id == id)) else {
                continue;
            };
            let lane_of = |lanes: &[AutomationLane], id: &str| lanes.iter().find(|l| l.param == id).cloned();
            let mut params: Vec<Param> =
                [(VOLUME, "Volume", Scale::Volume), (PAN, "Pan", Scale::Pan), (MUTE, "Mute", Scale::Mute)]
                    .into_iter()
                    .map(|(id, name, scale)| {
                        let lane = lane_of(&channel.automation, id);
                        Param {
                            key: LaneKey::mixer(id),
                            name: name.into(),
                            device_name: None,
                            scale,
                            enabled: lane.as_ref().is_none_or(|l| l.enabled),
                            points: lane.map(|l| l.points).unwrap_or_default(),
                        }
                    })
                    .collect();
            for (index, device) in channel.devices.iter().enumerate() {
                match &device.kind {
                    DeviceKind::Builtin(id) => {
                        let Some(info) = suhina_device_registry::info(id) else { continue };
                        for spec in info.params {
                            let lane = lane_of(&device.automation, spec.id);
                            params.push(Param {
                                key: LaneKey { device: Some(index), param: spec.id.into() },
                                name: spec.name.into(),
                                device_name: Some(device.name.clone()),
                                scale: Scale::Device(*spec),
                                enabled: lane.as_ref().is_none_or(|l| l.enabled),
                                points: lane.map(|l| l.points).unwrap_or_default(),
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
                                scale: Scale::Sd {
                                    min: spec.min,
                                    max: spec.max,
                                    control: spec.control,
                                    unit: spec.unit.clone(),
                                    options: spec.ui.options.clone(),
                                },
                                enabled: lane.as_ref().is_none_or(|l| l.enabled),
                                points: lane.map(|l| l.points).unwrap_or_default(),
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

    pub fn of(&self, track: TrackId) -> &[Param] {
        self.tracks.get(&track).map_or(&[], Vec::as_slice)
    }

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

    fn is_selected(&self, track: TrackId, key: &LaneKey, point: usize) -> bool {
        self.selection.as_ref().is_some_and(|s| s.track == track && &s.key == key && s.points.contains(&point))
    }

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
    /// The clip grid at this zoom, in beats.
    pub grid: f64,
}

impl Geometry {
    fn x_of(&self, beat: f64) -> f32 {
        self.time.min - self.scroll_x + beat as f32 * self.beat_width
    }

    fn beat_of(&self, x: f32) -> f64 {
        f64::from((x - self.time.min + self.scroll_x) / self.beat_width)
    }

    /// `beat` on the grid, or free (to a thousandth) with `free`; never before the start.
    fn snap(&self, beat: f64, free: bool) -> f64 {
        let beat = if free || self.grid <= 0.0 { round(beat, 1000.0) } else { (beat / self.grid).round() * self.grid };
        if beat.is_finite() { beat.max(0.0) } else { 0.0 }
    }
}

/// The envelope's area inside a lane.
fn plot(lane: Rect) -> Rect {
    lane.shrink2(Vec2::new(0.0, INSET))
}

fn y_of(plot: Rect, position: f64) -> f32 {
    plot.bottom() - position as f32 * plot.height()
}

fn position_of(plot: Rect, y: f32) -> f64 {
    if plot.height() > 0.0 { f64::from((plot.bottom() - y) / plot.height()).clamp(0.0, 1.0) } else { 0.0 }
}

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

    // The lane itself: a click adds a point, a drag near the line bends its segment, any other
    // drag draws a box that selects the points in it.
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
        params["value"] = param.scale.command(param.scale.value_at(position_of(plot, pos.y)));
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
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(200.0);
        let mut device = None;
        for choice in params.of(track) {
            if choice.device_name != device {
                device.clone_from(&choice.device_name);
                ui.separator();
                if let Some(name) = &device {
                    ui.label(egui::RichText::new(name).color(theme::text_hint()));
                }
            }
            let taken = shown.contains(&choice.key) && choice.key != param.key;
            let mark = if choice.points.is_empty() { "" } else { " •" };
            let button = egui::Button::selectable(choice.key == param.key, format!("{}{mark}", choice.name));
            if ui.add_enabled(!taken, button).clicked() {
                if let Some(keys) = state.shown.get_mut(&track)
                    && let Some(slot) = keys.iter_mut().find(|k| **k == param.key)
                {
                    *slot = choice.key.clone();
                }
                state.selection = None;
                ui.close();
            }
        }
    });

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

fn small_button(ui: &Ui, painter: &egui::Painter, rect: Rect, response: &egui::Response, text: &str) {
    let fill =
        crate::kit::motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, response));
    painter.rect_filled(rect, 0.0, fill);
    widgets::text_in(painter, rect, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT), theme::text_secondary());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_positions_and_values_are_inverses() {
        let scale = Scale::Volume;
        for db in [-60.0, -24.0, -6.0, 0.0, 3.0, 6.0] {
            let gain = 10f64.powf(db / 20.0);
            let back = scale.value_at(scale.position(gain));
            assert!((gain_to_db(back) - db).abs() < 1e-6, "{db}");
        }
        assert_eq!(scale.value_at(0.0), 0.0);
        assert_eq!(scale.command(0.0), json!("-inf"));
        assert_eq!(scale.command(1.0), json!(0.0));
        assert!((scale.position(1.0) - 0.85).abs() < 1e-6);
        // The lane places a volume where the mixer's field does.
        for db in [-70.0, -30.0, -6.0, 0.0, 4.0] {
            let field = f64::from(crate::kit::widgets::volume_position(db));
            assert!((volume_position(db) - field).abs() < 1e-6, "{db}");
        }
    }

    #[test]
    fn pan_and_mute_map_across_the_lane() {
        assert_eq!(Scale::Pan.position(-1.0), 0.0);
        assert_eq!(Scale::Pan.value_at(0.75), 0.5);
        assert_eq!(Scale::Mute.value_at(0.7), 1.0);
        assert!(Scale::Mute.stepped());
        assert!(!Scale::Pan.stepped());
    }

    #[test]
    fn a_shift_keeps_values_in_range_and_silence_silent() {
        assert_eq!(Scale::Volume.shift(0.0, 6.0), 0.0);
        assert!((gain_to_db(Scale::Volume.shift(1.0, 10.0)) - 6.0).abs() < 1e-9);
        assert_eq!(Scale::Pan.shift(0.5, 1.0), 1.0);
        assert_eq!(Scale::Mute.shift(1.0, -0.4), 1.0);
    }

    #[test]
    fn the_first_open_shows_the_lanes_with_points_or_the_volume() {
        let mut project = crate::demo::demo_project();
        let track = project.tracks[0].id;
        let mut state = LanesState::default();
        let params = Params::build(&project);
        state.toggle(track, &params);
        assert_eq!(state.shown(track, &params), [LaneKey::mixer(VOLUME)]);
        state.toggle(track, &params);
        assert!(state.shown(track, &params).is_empty());

        let channel = project.tracks[1].channel;
        if let Some(channel) = project.channels.iter_mut().find(|c| Some(c.id) == channel) {
            let mut lane = AutomationLane::new(PAN);
            lane.insert(AutomationPoint::new(0.0, 0.5));
            channel.automation.push(lane);
        }
        let params = Params::build(&project);
        let track = project.tracks[1].id;
        state.toggle(track, &params);
        assert_eq!(state.shown(track, &params), [LaneKey::mixer(PAN)]);
        assert_eq!(state.height(track, &params), LANE + LINE);
    }

    #[test]
    fn a_built_in_devices_parameters_can_be_shown_with_its_lanes() {
        let mut project = crate::demo::demo_project();
        let channel = project.tracks[0].channel;
        let Some(channel) = project.channels.iter_mut().find(|c| Some(c.id) == channel) else { return };
        let mut lane = AutomationLane::new("gain");
        lane.insert(AutomationPoint::new(4.0, -6.0));
        channel.devices.push(suhina_engine::model::Device {
            name: "Utility".into(),
            kind: DeviceKind::Builtin("utility".into()),
            enabled: true,
            state: None,
            params: Vec::new(),
            automation: vec![lane],
            extensions: Default::default(),
        });
        let params = Params::build(&project);
        let track = project.tracks[0].id;
        let gain = params.param(track, &LaneKey { device: Some(0), param: "gain".into() });
        assert_eq!(gain.map(Param::label).as_deref(), Some("Utility: Gain"));
        assert_eq!(gain.map(|p| p.points.len()), Some(1));
        let mut state = LanesState::default();
        state.toggle(track, &params);
        assert_eq!(state.shown(track, &params), [LaneKey { device: Some(0), param: "gain".into() }]);
    }

    #[test]
    fn sd_parameters_are_listed_and_use_their_control_scale() {
        let mut project = crate::demo::demo_project();
        let track = project.tracks[0].id;
        let Some(channel) = project.tracks[0].channel.and_then(|id| project.channels.iter_mut().find(|c| c.id == id))
        else {
            return;
        };
        let mut graph =
            suhina_device_sdk::DeviceGraph::new("test.filter", "Test Filter", suhina_device_sdk::DeviceRole::Effect);
        let mut parameter = suhina_device_sdk::Parameter::new("frequency", "Frequency", (20.0, 20_000.0), 1_000.0);
        parameter.control = suhina_device_sdk::ParameterControl::LogKnob;
        parameter.unit = "Hz".into();
        graph.parameters.push(parameter);
        graph.nodes = vec![
            suhina_device_sdk::Node::audio_input("in"),
            suhina_device_sdk::Node::gain("gain", "frequency"),
            suhina_device_sdk::Node::audio_output("out"),
        ];
        graph.connections =
            vec![suhina_device_sdk::Connection::new("in", "gain"), suhina_device_sdk::Connection::new("gain", "out")];
        let mut lane = AutomationLane::new("frequency");
        lane.insert(AutomationPoint::new(4.0, 1_000.0));
        channel.devices.push(suhina_engine::model::Device {
            name: "Test Filter".into(),
            kind: DeviceKind::Sd,
            enabled: true,
            state: graph.to_sd().ok().map(String::into_bytes),
            params: Vec::new(),
            automation: vec![lane],
            extensions: Default::default(),
        });
        let params = Params::build(&project);
        let frequency = params.param(track, &LaneKey { device: Some(0), param: "frequency".into() });
        assert_eq!(frequency.map(Param::label).as_deref(), Some("Test Filter: Frequency"));
        let Some(Scale::Sd { .. }) = frequency.map(|p| &p.scale) else { panic!("expected .sd scale") };
        assert_eq!(frequency.map(|p| p.scale.text(1_000.0)).as_deref(), Some("1000.00 Hz"));
        let position = (1_000.0_f64.ln() - 20.0_f64.ln()) / (20_000.0_f64.ln() - 20.0_f64.ln());
        assert!((frequency.map_or(0.0, |p| p.scale.position(1_000.0)) - position).abs() < 1e-9);
    }
}
