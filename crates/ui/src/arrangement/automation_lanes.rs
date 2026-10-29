// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Automation lanes in the Arrangement view (#420, `docs/spec/automation.md` › In the
//! Arrangement view): under a track, one lane per chosen parameter (its volume, pan and mute,
//! each parameter of its built-in effects and instruments), its envelope drawn over the
//! timeline and its points edited with the pointer. Every edit is an `automation.*` command;
//! which lanes are shown is the view's own state, like a lane's height.

use crate::kit::surface::Backgrounds as _;
use std::collections::{HashMap, HashSet};

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use serde_json::{Value, json};
use suhina_device_api::{ParamInfo, ParamKind, Unit};
use suhina_engine::model::automation::{self, Curve, MUTE, PAN, Taper, VOLUME};
use suhina_engine::model::{AutomationLane, AutomationPoint, ClipId, DeviceKind, Project, Rgb, TrackId};

use crate::interaction::timeline_grid::TimelineGrid;
use crate::kit::theme::{self, PAD};
use crate::kit::widgets::{self, Action};

/// Height of an automation lane until it is given another (#818).
pub const LANE: f32 = 40.0;
/// The lowest and highest a lane's bottom edge drags it to.
const MIN_LANE: f32 = 24.0;
const MAX_LANE: f32 = LANE * 6.0;
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
/// The line's grip as a share of the lane's height, when that is more than [`SEGMENT_HIT`]: a
/// click this close above or below it takes hold of it (#916).
const SEGMENT_HIT_SHARE: f32 = 0.08;

/// How near the line in `plot` a click must be to take hold of it.
fn segment_hit(plot: Rect) -> f32 {
    SEGMENT_HIT.max(plot.height() * SEGMENT_HIT_SHARE)
}
/// How much a point of vertical drag bends a segment's curve.
/// The curve handle's radius, and how wide a segment must be on screen to show one.
const CURVE_HANDLE: f32 = 3.0;
const CURVE_HANDLE_MIN: f32 = 28.0;
/// Points of vertical drag per step of a curve drag (`automation::CurveDrag`): the mirrored
/// family's whole range, 128 steps, is 192 points.
const POINTS_PER_STEP: f32 = 1.5;
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
            Self::Volume => automation::volume_position(gain_to_db(value)),
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
            Self::Volume => automation::VolumeTaper.value(position),
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

    /// How far Up or Down moves a point, in the set command's unit.
    fn nudge_step(&self) -> f64 {
        match self {
            Self::Volume => 1.0,
            Self::Pan => 0.02,
            Self::Mute => 1.0,
            Self::Device(info) => match info.kind {
                ParamKind::Continuous(_) => (info.max - info.min) / 100.0,
                _ => 1.0,
            },
            Self::Sd(parameter) if parameter.control.is_stepped() => 1.0,
            Self::Sd(parameter) => (parameter.max - parameter.min) / 100.0,
        }
    }

    /// A value as Edit value's field starts with: the set command's number, `-inf` for a silent
    /// volume.
    fn typed_text(&self, value: f64) -> String {
        match self.command(value) {
            Value::String(text) => text,
            number => number.to_string(),
        }
    }

    /// A value typed into Edit value's field, as the set command takes it: a number in its unit
    /// (decibels for the volume, `-inf` for silence; a pan as -1 to 1 or as `25L`, `C`, `25R`; a
    /// percentage with `%`), units written after it left out. `None` for text that is not one.
    fn parse_typed(&self, text: &str) -> Option<Value> {
        let text = text.trim();
        let lower = text.to_lowercase();
        if matches!(self, Self::Volume) && (lower == "-inf" || lower == "inf" || lower == "-∞") {
            return Some(json!("-inf"));
        }
        if matches!(self, Self::Pan) {
            if lower == "c" {
                return Some(json!(0.0));
            }
            for (side, sign) in [('l', -1.0), ('r', 1.0)] {
                if let Some(amount) = lower.strip_suffix(side).and_then(|n| n.trim().parse::<f64>().ok()) {
                    return Some(json!((sign * amount / 50.0).clamp(-1.0, 1.0)));
                }
            }
        }
        let percent = lower.ends_with('%');
        let number: String = lower
            .chars()
            .take_while(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | ','))
            .map(|c| if c == ',' { '.' } else { c })
            .collect();
        let value: f64 = number.parse().ok().filter(|v: &f64| v.is_finite())?;
        let value = match self {
            Self::Device(info) if percent && info.unit == Unit::Percent => value / 100.0,
            Self::Device(info) if lower.ends_with("ms") && info.unit == Unit::Seconds => value / 1000.0,
            Self::Volume => value.min(6.0),
            Self::Pan => value.clamp(-1.0, 1.0),
            Self::Mute => value.round().clamp(0.0, 1.0),
            Self::Device(info) => info.clamp(value),
            Self::Sd(parameter) => parameter.clamp(value),
        };
        Some(json!(value))
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
    /// moves it: kept in range, and a silent volume moves from the lane's bottom (a level moved
    /// down to the bottom is silent).
    fn shift(&self, value: f64, delta: f64) -> f64 {
        match self {
            Self::Volume => {
                let db = (if value <= 0.0 { VOLUME_FLOOR } else { gain_to_db(value).max(VOLUME_FLOOR) }) + delta;
                if db <= VOLUME_FLOOR { 0.0 } else { 10f64.powf(db.min(6.0) / 20.0) }
            }
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
                // What the parameter plays: a lane between two numbers cuts the fraction off.
                ParamKind::Integer => {
                    let value = info.played(value);
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
/// Where `value` sits on a lane of a data-only device's `parameter`, 0 … 1.
fn sd_position(parameter: &suhina_device_sdk::Parameter, value: f64) -> f64 {
    parameter.lane_position(value)
}

/// The value of a data-only device's `parameter` at `position` (0 … 1) on its lane.
fn sd_value(parameter: &suhina_device_sdk::Parameter, position: f64) -> f64 {
    parameter.lane_value(position)
}

/// `value` rounded to the nearest `1 / per`: `round(x, 1000.0)` keeps three decimals.
fn round(value: f64, per: f64) -> f64 {
    (value * per).round() / per
}

/// A linear gain in decibels; minus infinity for silence (a gain of 0 or less).
fn gain_to_db(gain: f64) -> f64 {
    if gain > 0.0 { 20.0 * gain.log10() } else { f64::NEG_INFINITY }
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
        // Every track's channel, and Main's under [`TrackId::MAIN`] (#687).
        let channels = project
            .tracks
            .iter()
            .filter_map(|t| Some((t.id, t.channel.and_then(|id| project.channel(id))?)))
            .chain(project.main_channel().map(|main| (TrackId::MAIN, main)));
        for (track, channel) in channels {
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
            tracks.insert(track, params);
        }
        Self { tracks }
    }

    /// `track`'s parameters, in the order the chooser lists them; none for a track without a
    /// channel.
    pub fn of(&self, track: TrackId) -> &[Param] {
        self.tracks.get(&track).map_or(&[], Vec::as_slice)
    }

    /// `track`'s parameter that `key` names, if the track still has it.
    pub(crate) fn param(&self, track: TrackId, key: &LaneKey) -> Option<&Param> {
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
    /// A segment's two points dragged by their line: up and down only.
    segment: bool,
    /// Taken hold of from the lane, not the point itself (a point just made by a press on the
    /// line): the lane follows the pointer for it.
    body: bool,
}

/// Where a point drag is now from `pos`: with Shift, only along the axis the pointer has moved
/// further on, and up or down at a quarter of the pointer's speed for a finer value.
fn dragged_to(grab: (f64, f64), beat: f64, position: f64, plot: Rect, geometry: &Geometry, shift: bool) -> (f64, f64) {
    if !shift {
        return (beat, position);
    }
    let across = (beat - grab.0).abs() as f32 * geometry.beat_width;
    let along = (position - grab.1).abs() as f32 * plot.height();
    if across > along { (beat, grab.1) } else { (grab.0, grab.1 + (position - grab.1) * 0.25) }
}

/// A point a press on the line has just added, until the button comes up: its beat, to find it
/// once the command has run, and where the pointer took hold, so a drag goes on to move it.
#[derive(Debug, Clone, PartialEq)]
struct NewPoint {
    track: TrackId,
    key: LaneKey,
    beat: f64,
    grab: (f64, f64),
}

/// The shape put in a range last: where, which, the place on the lane it swings around (worked
/// out before it went in, so the sliders keep it), and the window's edit count once it was in.
#[derive(Debug, Clone, PartialEq)]
struct LastShape {
    track: TrackId,
    key: LaneKey,
    start: f64,
    end: f64,
    shape: &'static str,
    center: f64,
}

/// How often a slider being dragged puts its value in, in seconds: once a frame at 60 frames a
/// second, so the line follows the slider without steps (an edit takes well under a frame), and
/// no more often on a faster screen.
const LIVE_EVERY: f64 = 1.0 / 60.0;

/// Whether a menu slider's value goes in now: when it is let go of, clicked or typed into, and
/// while it is dragged, at most every [`LIVE_EVERY`] seconds.
fn live(ui: &Ui, state: &mut LanesState, response: &egui::Response) -> bool {
    let now = ui.input(|i| i.time);
    let due =
        response.drag_stopped() || (response.changed() && (!response.dragged() || now - state.live_at >= LIVE_EVERY));
    if due {
        state.live_at = now;
    }
    due
}

/// Scale kept for one range or set of picked points, so the slider stays where it was let go of
/// while they stay the same.
#[derive(Debug, Clone, PartialEq)]
struct Stretch {
    of: String,
    shown: f64,
    applied: f64,
    /// A step of this drag is in the history already, so the next one joins it (`continue`).
    joined: bool,
}

/// The Scale slider for what `of` names: where it stands (100 % for something new), and while it
/// is dragged or when it is let go of, the factor that takes the points from where they are to the
/// value shown, as a share of where the scaling began (so 150 % then 120 % ends at 120 %), and
/// whether that step joins the one before it in the history (one drag is one undo step).
fn stretch_slider(ui: &mut Ui, state: &mut LanesState, of: String) -> Option<(f64, bool)> {
    let kept = state.stretch.clone().filter(|s| s.of == of);
    let (shown, applied, joined) = kept.map_or((1.0, 1.0, false), |s| (s.shown, s.applied, s.joined));
    let mut percent = shown * 100.0;
    let response = ui
        .add(egui::Slider::new(&mut percent, 5.0..=200.0).text("Scale").suffix(" %").fixed_decimals(0))
        .on_hover_text(
            "Pushes the points away from their middle (over 100 %) or pulls them toward it (under 100 %). \
             Works on any points, drawn or inserted; their timing stays.",
        );
    let shown = percent / 100.0;
    let joined = joined && !response.drag_started();
    state.stretch = Some(Stretch { of, shown, applied, joined });
    let due = live(ui, state, &response);
    if !due || (shown - applied).abs() < 1e-6 || applied <= 0.0 {
        if response.drag_stopped()
            && let Some(stretch) = state.stretch.as_mut()
        {
            stretch.joined = false;
        }
        return None;
    }
    // The command takes 0 to 4 at a time; what it did is what the points are at now.
    let factor = (shown / applied).clamp(0.0, 4.0);
    let dragging = response.dragged() && !response.drag_stopped();
    if let Some(stretch) = state.stretch.as_mut() {
        stretch.applied = applied * factor;
        stretch.joined = dragging;
    }
    Some((factor, joined))
}

/// The Smooth slider's amount kept for one range.
#[derive(Debug, Clone, PartialEq)]
struct Smoothed {
    of: String,
    amount: f64,
}

/// A segment's curve being bent: the segment's first point, the drag along the control points'
/// paths, how far the pointer has moved and the curve that gives.
#[derive(Debug, Clone, PartialEq)]
struct CurveDrag {
    track: TrackId,
    key: LaneKey,
    point: usize,
    paths: automation::CurveDrag,
    moved: f32,
    curve: Curve,
}

/// A box drawn over a lane to select the points in it, from where it started to the pointer.
#[derive(Debug, Clone, PartialEq)]
struct BoxDrag {
    track: TrackId,
    key: LaneKey,
    from: Pos2,
    to: Pos2,
    /// Begun in the lane's lower half: a box that selects the points in it. From the upper half
    /// the drag paints a time range instead (#916).
    select: bool,
}

/// A time range painted on one lane: copy, paste, duplicate, delete and the shapes act on it.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneRange {
    pub track: TrackId,
    pub key: LaneKey,
    pub start: f64,
    pub end: f64,
}

/// What the lanes keep between frames: which tracks show lanes and which, the selected points
/// and a drag in progress.
#[derive(Debug, Default)]
pub struct LanesState {
    open: HashSet<TrackId>,
    /// The lanes dragged to a height of their own; the rest are [`LANE`] high. Kept while a
    /// track's lanes are hidden, so they come back as tall as they were (#818).
    heights: HashMap<(TrackId, LaneKey), f32>,
    shown: HashMap<TrackId, Vec<LaneKey>>,
    selection: Option<Selection>,
    point_drag: Option<PointDrag>,
    curve_drag: Option<CurveDrag>,
    box_drag: Option<BoxDrag>,
    /// The time range painted last on a lane, which the edit keys act on.
    pub range: Option<LaneRange>,
    /// A click anywhere on a lane adds a point (Settings › Arrangement › Automation); off, only a
    /// click on or near the line does, and a double-click adds one anywhere.
    pub click_anywhere: bool,
    /// The shape a point added from now on gives the segment that ends at it, chosen from a lane's
    /// menu away from a painted range; `None` for a straight line.
    pub new_shape: Option<&'static str>,
    /// Where a right-click on a lane was: its track, lane and beat, for the shapes its menu
    /// inserts.
    menu_at: Option<(TrackId, LaneKey, f64)>,
    /// The beat on the grid a right-click on the line was at, for the menu's Add value.
    menu_line: Option<f64>,
    /// Snap to grid is off in the lanes' menu: points and ranges go where the pointer is, as with
    /// Alt. The window's choice, not saved.
    pub snap_off: bool,
    /// The Arrangement's grid, which the menu shows; the view sets it before each frame.
    pub grid: crate::detail::piano_roll_grid::PianoGrid,
    /// A grid picked in the lanes' menu this frame, for the Arrangement to take.
    pub grid_picked: Option<crate::detail::piano_roll_grid::PianoGrid>,
    /// The grid keys' labels, as the menu shows them.
    pub grid_keys: [String; 4],
    /// A point whose value is being typed (its menu's Edit value): track, lane, point and text.
    editing: Option<(TrackId, LaneKey, usize, String)>,
    /// A point being made by a drag from the line.
    new_point: Option<NewPoint>,
    /// The lane clicked last, which Ctrl+V pastes copied automation onto at the play position;
    /// a clip or a track header clicked since lets go of it.
    pub last_lane: Option<(TrackId, LaneKey)>,
    /// The range menu's Intensity (0 to 1; `None` for the whole travel) and Rate (cycles a bar;
    /// `None` for one a beat), the window's choices.
    shape_depth: Option<f64>,
    shape_rate: Option<f64>,
    /// Scale as the menu shows it for what it scales (a range or picked points, by a key): the
    /// value shown and the value the points are at now, from where the scaling began.
    stretch: Option<Stretch>,
    /// Smooth as the range menu shows it, and the smoothing it put in last.
    smooth: Option<Smoothed>,
    /// When a dragged menu slider last put its value in (the window's clock, in seconds).
    live_at: f64,
    /// The shape put in a range last (track, lane, start, end, shape), which the menu's sliders
    /// put in again as they change.
    last_shape: Option<LastShape>,
    /// The lanes whose parameter was chosen in the parameter menu's "Automated" section: the
    /// menu opens there again for them, and on the parameter's own row for the rest.
    from_automated: HashSet<(TrackId, LaneKey)>,
    /// The clip selected in the view last frame: selecting a clip ends a selection of points.
    clip: Option<(TrackId, ClipId)>,
    /// Where every point of the lanes was drawn in the pass `placed_pass`, for a selection box
    /// begun on a track that reaches over its lanes.
    placed: Vec<(TrackId, LaneKey, usize, Pos2)>,
    placed_pass: u64,
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
        self.shown(track, params).iter().map(|key| self.lane_height(track, key) + LINE).sum()
    }

    /// The height of `track`'s lane `key`.
    pub fn lane_height(&self, track: TrackId, key: &LaneKey) -> f32 {
        self.heights.get(&(track, key.clone())).copied().unwrap_or(LANE)
    }

    /// Gives `track`'s lane `key` the height `height`, within the lanes' limits; [`LANE`] puts
    /// it back to the normal height.
    pub fn set_lane_height(&mut self, track: TrackId, key: &LaneKey, height: f32) {
        let height = height.clamp(MIN_LANE, MAX_LANE);
        if (height - LANE).abs() < f32::EPSILON {
            self.heights.remove(&(track, key.clone()));
        } else {
            self.heights.insert((track, key.clone()), height);
        }
    }

    /// Every shown lane of every track `factor` times as tall, within the lanes' limits, as Alt
    /// and the wheel make the tracks (#957).
    pub fn zoom_all(&mut self, factor: f32) {
        let shown: Vec<(TrackId, LaneKey)> = self
            .shown
            .iter()
            .filter(|(track, _)| self.open.contains(track))
            .flat_map(|(track, keys)| keys.iter().map(move |k| (*track, k.clone())))
            .collect();
        for (track, key) in shown {
            let height = self.lane_height(track, &key) * factor;
            self.set_lane_height(track, &key, height);
        }
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
    /// the selection of points and the painted range.
    pub fn follow_clip(&mut self, clip: Option<(TrackId, ClipId)>) {
        if clip.is_some() && clip != self.clip {
            self.selection = None;
            self.range = None;
            self.last_lane = None;
        }
        self.clip = clip;
    }

    /// The command an arrow key asks for on the selected points: Left and Right move them a grid
    /// step (`step` beats) earlier or later, Up and Down a small step of value (a decibel for the
    /// volume, a hundredth of the travel for most parameters, one for a whole-number one).
    pub fn nudge(&mut self, params: &Params, direction: &str, step: f64) -> Option<Action> {
        let selection = self.selection.clone().filter(|s| !s.points.is_empty())?;
        let param = params.param(selection.track, &selection.key)?;
        let mut command = selection.key.params(selection.track);
        command["points"] = json!(selection.points);
        match direction {
            "left" => command["beats"] = json!(-step),
            "right" => command["beats"] = json!(step),
            "up" => command["value"] = json!(param.scale.nudge_step()),
            "down" => command["value"] = json!(-param.scale.nudge_step()),
            _ => return None,
        }
        Some(Action::new("automation.movePoints", command))
    }

    /// Lets go of the selected points, the painted range and the lane clicked last: something
    /// else (a device, a track header) is what the edit keys act on now.
    pub fn let_go(&mut self) {
        self.selection = None;
        self.range = None;
        self.last_lane = None;
    }

    /// The commands an edit key asks for on the selected points, with no range painted: `copy`,
    /// `cut` (a copy, then the points deleted) and `paste` (the copied automation onto the lane
    /// clicked last, from `at`, the play position). Nothing when they do not apply.
    pub fn point_actions(&mut self, edit: &str, at: f64) -> Vec<Action> {
        match edit {
            "copy" | "cut" => {
                let Some(selection) = self.selection.clone().filter(|s| !s.points.is_empty()) else {
                    return Vec::new();
                };
                let mut params = selection.key.params(selection.track);
                params["points"] = json!(selection.points);
                self.last_lane = Some((selection.track, selection.key.clone()));
                let copy = Action::new("automation.copy", params.clone());
                if edit == "copy" {
                    return vec![copy];
                }
                self.selection = None;
                vec![copy, Action::new("automation.deletePoints", params)]
            }
            "paste" => {
                let Some((track, key)) = self.last_lane.clone() else { return Vec::new() };
                let mut params = key.params(track);
                params["start"] = json!(at.max(0.0));
                self.selection = None;
                vec![Action::new("automation.paste", params)]
            }
            _ => Vec::new(),
        }
    }

    /// Whether the edit keys act on automation points: points are selected, or (for a paste) a
    /// lane was clicked last.
    pub fn takes_point_keys(&self, edit: &str) -> bool {
        match edit {
            "copy" | "cut" => self.selection.as_ref().is_some_and(|s| !s.points.is_empty()),
            "paste" => self.last_lane.is_some(),
            _ => false,
        }
    }

    /// The command an edit key asks for on the painted range: `copy`, `cut` (a copy, then the
    /// range deleted), `paste` (from its start), `duplicate` (right after it, the range moving
    /// onto the copy) or `delete`; nothing without a range.
    pub fn range_actions(&mut self, edit: &str) -> Vec<Action> {
        let Some(range) = self.range.clone() else { return Vec::new() };
        let mut params = range.key.params(range.track);
        params["start"] = json!(range.start);
        params["end"] = json!(range.end);
        let action = |id: &'static str, params: &Value| Action::new(id, params.clone());
        match edit {
            "copy" => vec![action("automation.copy", &params)],
            "cut" => {
                self.selection = None;
                vec![action("automation.copy", &params), action("automation.deleteRange", &params)]
            }
            "delete" => {
                self.selection = None;
                vec![action("automation.deleteRange", &params)]
            }
            "duplicate" => {
                let length = range.end - range.start;
                self.selection = None;
                self.range = Some(LaneRange { start: range.end, end: range.end + length, ..range });
                vec![action("automation.duplicate", &params)]
            }
            "paste" => {
                if let Some(map) = params.as_object_mut() {
                    map.remove("end");
                }
                self.selection = None;
                vec![action("automation.paste", &params)]
            }
            _ => Vec::new(),
        }
    }

    /// Picks the points a selection box begun on a track covers: on the lane where it covers the
    /// most (a selection of points is on one lane), in place of the points picked before. With no
    /// point under it, the selection of points is left as it is.
    pub fn select_inside(&mut self, rect: Rect) {
        let mut by_lane: Vec<((TrackId, LaneKey), Vec<usize>)> = Vec::new();
        for (track, key, index, _) in self.placed.iter().filter(|(_, _, _, at)| rect.contains(*at)) {
            match by_lane.iter_mut().find(|(lane, _)| lane.0 == *track && &lane.1 == key) {
                Some((_, points)) => points.push(*index),
                None => by_lane.push(((*track, key.clone()), vec![*index])),
            }
        }
        if let Some(((track, key), mut points)) = by_lane.into_iter().max_by_key(|(_, points)| points.len()) {
            points.sort_unstable();
            points.dedup();
            self.selection = Some(Selection { track, key, points });
            self.range = None;
        }
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
    // `#control-automation`, `:checked` while the lanes show, and `automation` artwork.
    let _style = crate::kit::style_class::enter_named_response("button", "automation", &response);
    crate::kit::native_css::mark(crate::kit::native_css::HostState::Checked, open);
    let hover = widgets::hover(ui, &response);
    let off = crate::kit::motion::mix(theme::control_background(), theme::control_hover(), hover);
    widgets::track_button_face(ui, painter, rect, &response, if open { theme::control_active() } else { off });
    let ink = crate::kit::style_class::foreground(if open { theme::text_primary() } else { theme::text_secondary() });
    if crate::kit::skin::paint(
        painter,
        rect,
        "automation",
        crate::kit::skin::State::response(&response, open),
        ink,
        0.0,
    ) {
        return;
    }
    // A small envelope: up, along, down.
    let glyph = rect.shrink2(Vec2::new(4.0, 4.0));
    let line = vec![
        glyph.left_bottom(),
        Pos2::new(glyph.left() + glyph.width() * 0.35, glyph.top()),
        Pos2::new(glyph.left() + glyph.width() * 0.65, glyph.top()),
        glyph.right_bottom(),
    ];
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
        let height = state.lane_height(lanes.track, &key);
        let time = Rect::from_x_y_ranges(geometry.time, egui::Rangef::new(y, y + height));
        let header = Rect::from_x_y_ranges(geometry.headers, time.y_range());
        y += height + LINE;
        let full = Rect::from_x_y_ranges(geometry.visible.x_range(), time.y_range());
        if full.bottom() < geometry.visible.top() || full.top() > geometry.visible.bottom() {
            continue;
        }
        lane(ui, painter, geometry, lanes, param, time, state, actions);
        lane_header(ui, painter, geometry, lanes, param, header, state, params, actions);
        painter.hline(geometry.visible.x_range(), time.bottom() + 0.5, theme::hairline(theme::border()));
        resize_edge(ui, geometry, lanes, &key, full, state);
    }
}

/// The line under a lane: dragged, it makes the lane taller or shorter; a double-click puts it
/// back to the normal height (#818).
fn resize_edge(
    ui: &Ui,
    geometry: &Geometry,
    lanes: &TrackLanes<'_>,
    key: &LaneKey,
    full: Rect,
    state: &mut LanesState,
) {
    let edge = Rect::from_min_max(
        Pos2::new(full.left(), full.bottom() - 3.0),
        Pos2::new(full.right(), full.bottom() + LINE + 2.0),
    )
    .intersect(geometry.visible);
    if !edge.is_positive() {
        return;
    }
    let id = Id::new(("automation-resize", lanes.track.0, key.device, key.param.as_str()));
    let response = ui.interact(edge, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeVertical);
    if response.double_clicked() {
        state.set_lane_height(lanes.track, key, LANE);
    } else if response.dragged_by(egui::PointerButton::Primary) {
        let height = state.lane_height(lanes.track, key) + response.drag_delta().y;
        state.set_lane_height(lanes.track, key, height);
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
            // Evenly in the lane's position, as the engine plays it: a straight segment is a
            // straight line; a bent one is sampled along its curve.
            let (ya, yb) = (scale.position(a.value), scale.position(b.value));
            let steps =
                if a.curve.is_straight() { 1 } else { (((xb - xa) / 2.0).ceil() as usize).clamp(2, SEGMENT_POINTS) };
            for step in 1..steps {
                let t = step as f64 / steps as f64;
                let shaped = a.curve.shape(t);
                let x = geometry.x_of(a.beat + (b.beat - a.beat) * t);
                line.push(Pos2::new(x, y_of(plot, ya + (yb - ya) * shaped)));
            }
        }
        line.push(at(b.beat, b.value));
    }
    line.push(Pos2::new(plot.right(), at(last.beat, last.value).y));
    line
}

/// The points a click on a lane is measured against: the lane's own, or for a lane without
/// points one at the parameter's value, its faint line.
/// A lane moves evenly in its position, as the engine plays it.
impl automation::Taper for Scale {
    fn position(&self, value: f64) -> f64 {
        Scale::position(self, value)
    }
    fn value(&self, position: f64) -> f64 {
        // A whole-number parameter moves along the line between its points, through values
        // between its numbers, as the engine moves it (it plays them with the fraction cut off).
        match self {
            Self::Device(info) if matches!(info.kind, ParamKind::Integer) => {
                info.min + position.clamp(0.0, 1.0) * (info.max - info.min)
            }
            _ => self.value_at(position),
        }
    }
}

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
    let value = suhina_engine::model::automation::value_at(points, beat, false, scale)?;
    ((y_of(plot, scale.position(value)) - pos.y).abs() <= segment_hit(plot)).then_some(index)
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
    // Points added and dragged and a painted range go on the grid; Alt, or Snap to grid off in the
    // lane's menu, frees them (#916).
    let free = ui.input(|i| i.modifiers.alt) || state.snap_off;
    let id = Id::new(("automation-lane", track.0, key.device, key.param.as_str()));
    let label = param.label();

    // The lane itself: a click adds a point (on the envelope when near it, so the envelope keeps
    // its shape; where the pointer is otherwise), a drag near the line bends its segment, any
    // other drag draws a box that selects the points in it.
    let body = ui.interact(visible, id, Sense::click_and_drag());
    body.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("{label} automation on {}", lanes.name))
    });
    let level = level_points(param);
    let near_line = |pos: Pos2| distance_to(&envelope(&level, &param.scale, plot, geometry), pos) <= segment_hit(plot);
    // A click on or near the line adds a point there; away from it, only with the setting on, or a
    // double-click (#916). Otherwise a click there lets go of the range and the points.
    if body.is_pointer_button_down_on() {
        state.last_lane = Some((track, key.clone()));
    }
    // A press on the line adds the point there at once; a drag goes on to move it (#916).
    let (alt, shift) = ui.input(|i| (i.modifiers.alt, i.modifiers.shift));
    if body.is_pointer_button_down_on()
        && ui.input(|i| i.pointer.primary_pressed())
        && !alt
        && !shift
        && let Some(pos) = ui.input(|i| i.pointer.press_origin())
        && near_line(pos)
    {
        let beat = geometry.snap(geometry.beat_of(pos.x), free);
        let mut params = key.params(track);
        params["beat"] = json!(beat);
        // On the line the point goes on the envelope, and the command splits a bent segment so it
        // plays as before (on a whole-number lane it keeps a value between two numbers); a lane
        // without points takes the faint line's value.
        if param.points.is_empty() {
            let value =
                automation::value_at(&level, beat, param.scale.stepped(), &param.scale).unwrap_or(param.current);
            params["value"] = param.scale.exact(value);
        }
        if let Some(shape) = state.new_shape {
            params["shape"] = json!(shape);
        }
        actions.push(Action::new("automation.addPoint", params));
        let grab = (geometry.beat_of(pos.x), position_of(plot, pos.y));
        state.new_point = Some(NewPoint { track, key: key.clone(), beat, grab });
        state.selection = None;
        state.range = None;
    }
    // Once the press's point is on the lane, the drag holds it.
    if let Some(made) = state.new_point.as_ref().filter(|d| d.track == track && &d.key == key)
        && state.point_drag.is_none()
        && let Some(index) = param.points.iter().rposition(|p| (p.beat - made.beat).abs() < 1e-9)
    {
        state.point_drag = Some(PointDrag {
            track,
            key: key.clone(),
            point: index,
            together: vec![index],
            grab: made.grab,
            now: made.grab,
            segment: false,
            body: true,
        });
    }
    let clicked_at = body.interact_pointer_pos().filter(|_| body.clicked() || body.double_clicked());
    // A click whose press added a point adds nothing more.
    let pressed_point =
        clicked_at.is_some() && state.new_point.take_if(|d| d.track == track && &d.key == key).is_some();
    if pressed_point {
        state.point_drag.take_if(|d| d.body && d.track == track && &d.key == key);
    }
    if let Some(pos) = clicked_at.filter(|_| !pressed_point) {
        let near = near_line(pos);
        let adds = if body.double_clicked() { !state.click_anywhere && !near } else { near || state.click_anywhere };
        if adds {
            let mut params = key.params(track);
            let beat = geometry.snap(geometry.beat_of(pos.x), free);
            params["beat"] = json!(beat);
            // On a whole-number lane a value between two numbers cannot be kept: the point goes
            // where the pointer is, as off the line.
            if !near {
                params["value"] = param.scale.command(param.scale.value_at(position_of(plot, pos.y)));
            } else if param.points.is_empty() {
                let value =
                    automation::value_at(&level, beat, param.scale.stepped(), &param.scale).unwrap_or(param.current);
                params["value"] = param.scale.exact(value);
            }
            if let Some(shape) = state.new_shape {
                params["shape"] = json!(shape);
            }
            actions.push(Action::new("automation.addPoint", params));
        } else if !body.double_clicked() {
            // An empty spot: the play start marker moves there, as a click on a clip lane does.
            let beat = geometry.snap(geometry.beat_of(pos.x), free);
            actions.push(Action::new("transport.setStart", json!({ "beats": beat })));
        }
        state.selection = None;
        state.range = None;
        state.last_lane = Some((track, key.clone()));
    }
    // Over the line the pointer is a hand and the line's value there shows beside it, so where a
    // click takes hold of it is plain.
    let over_line = body.hover_pos().filter(|pos| {
        state.box_drag.is_none() && state.curve_drag.is_none() && state.point_drag.is_none() && near_line(*pos)
    });
    if over_line.is_none() && state.box_drag.is_none() && body.hover_pos().is_some_and(|pos| pos.y <= time.center().y) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    }
    if let Some(pos) = over_line {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        // Where a click puts the point: on the grid (at the pointer with Alt, or with Snap to grid
        // off), marked by a dot on the line.
        let beat = geometry.snap(geometry.beat_of(pos.x), free);
        if let Some(value) =
            suhina_engine::model::automation::value_at(&level, beat, param.scale.stepped(), &param.scale)
        {
            let y = y_of(plot, param.scale.position(value));
            let x = geometry.x_of(beat);
            let guide = painter.with_clip_rect(visible);
            guide.circle_filled(Pos2::new(x, y), POINT, color(lanes.color));
            egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), id.with("value"), egui::PopupAnchor::Pointer)
                .gap(12.0)
                .show(|ui| ui.add(egui::Label::new(param.scale.text(value)).wrap_mode(egui::TextWrapMode::Extend)));
        }
    }
    // A drag starts once the pointer has moved a little: it is measured from where it was pressed.
    let pressed_at = ui.input(|i| i.pointer.press_origin());
    if body.drag_started_by(egui::PointerButton::Primary)
        && let Some(pos) = pressed_at.or_else(|| body.interact_pointer_pos())
    {
        // From the line: a drag makes a point where the guide showed and moves it, added when the
        // button comes up; with Alt it bends the segment, with Shift it moves the segment's two
        // points up or down together (#916).
        let segment = segment_at(&param.points, &param.scale, plot, geometry, pos);
        match segment {
            // The press made a point: the drag moves it.
            _ if state.new_point.as_ref().is_some_and(|d| d.track == track && &d.key == key) => {}
            Some(point) if alt => {
                let curve = param.points.get(point).map_or(Curve::STRAIGHT, |p| p.curve);
                // A drag first holds a split half's control points in the unit square, so even one
                // shorter than a step changes a curve that lay outside it.
                let paths = automation::CurveDrag::new(&curve);
                let curve = paths.dragged(0);
                state.curve_drag = Some(CurveDrag { track, key: key.clone(), point, paths, moved: 0.0, curve });
            }
            Some(point) if shift => {
                let grab = (geometry.beat_of(pos.x), position_of(plot, pos.y));
                state.point_drag = Some(PointDrag {
                    track,
                    key: key.clone(),
                    point,
                    together: vec![point, point + 1],
                    grab,
                    now: grab,
                    segment: true,
                    body: true,
                });
                state.selection = None;
            }
            // Away from the line: from the lane's lower half a box selects points, from its upper
            // half the drag paints a time range, as on the clips' lanes.
            _ => {
                let select = pos.y > time.center().y;
                state.box_drag = Some(BoxDrag { track, key: key.clone(), from: pos, to: pos, select });
            }
        }
    }
    if body.dragged_by(egui::PointerButton::Primary)
        && let Some(pos) = body.interact_pointer_pos()
        && let Some(drag) =
            state.point_drag.as_mut().filter(|d| d.body && !d.segment && d.track == track && &d.key == key)
    {
        drag.now = dragged_to(drag.grab, geometry.beat_of(pos.x), position_of(plot, pos.y), plot, geometry, shift);
    }
    if body.dragged_by(egui::PointerButton::Primary)
        && let Some(pos) = body.interact_pointer_pos()
        && let Some(drag) = state.point_drag.as_mut().filter(|d| d.segment && d.track == track && &d.key == key)
    {
        let position = position_of(plot, pos.y);
        drag.now = (drag.grab.0, drag.grab.1 + (position - drag.grab.1) * 0.25);
    }
    if body.dragged_by(egui::PointerButton::Primary) {
        let dy = body.drag_delta().y;
        if let Some(drag) = state.curve_drag.as_mut().filter(|d| d.track == track && &d.key == key) {
            // Up bends the line up: on a rising segment that moves the curve earlier, on a falling
            // one later.
            let rising = match (param.points.get(drag.point), param.points.get(drag.point + 1)) {
                (Some(a), Some(b)) => param.scale.position(b.value) >= param.scale.position(a.value),
                _ => true,
            };
            drag.moved += if rising { dy } else { -dy };
            drag.curve = drag.paths.dragged((drag.moved / POINTS_PER_STEP).round() as i32);
        }
        if let Some(drag) = state.box_drag.as_mut().filter(|d| d.track == track && &d.key == key)
            && let Some(pos) = body.interact_pointer_pos()
        {
            drag.to = pos;
        }
    }
    // The button up ends a point held from the lane however it was held: a press held still
    // and let go is neither a click nor the end of a drag, and would leave the point held.
    if body.drag_stopped_by(egui::PointerButton::Primary) || ui.input(|i| i.pointer.primary_released()) {
        if let Some(drag) = state.point_drag.take_if(|d| d.body && d.track == track && &d.key == key)
            && let Some(action) = drop_points(&drag, param, geometry, free)
        {
            actions.push(action);
        }
        state.new_point.take_if(|d| d.track == track && &d.key == key);
    }
    if body.drag_stopped_by(egui::PointerButton::Primary) {
        // The drag paints a time range on the lane, on the grid (free with Alt): the edit keys and
        // the shapes act on it, and the points in it are selected (#916).
        if let Some(drag) = state.box_drag.take_if(|d| d.select && d.track == track && &d.key == key) {
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
            state.range = None;
        }
        if let Some(drag) = state.box_drag.take_if(|d| d.track == track && &d.key == key) {
            let (a, b) =
                (geometry.snap(geometry.beat_of(drag.from.x), free), geometry.snap(geometry.beat_of(drag.to.x), free));
            let (start, end) = (a.min(b), a.max(b));
            let points: Vec<usize> = param
                .points
                .iter()
                .enumerate()
                .filter(|(_, p)| p.beat >= start && p.beat <= end)
                .map(|(i, _)| i)
                .collect();
            state.selection = (!points.is_empty()).then(|| Selection { track, key: key.clone(), points });
            state.range = (end - start > 1e-6).then(|| LaneRange { track, key: key.clone(), start, end });
        }
    }
    // The lane's menu: shapes inserted into the painted range, or into the bar under the pointer.
    if body.secondary_clicked()
        && let Some(pos) = body.interact_pointer_pos()
    {
        state.menu_at = Some((track, key.clone(), geometry.beat_of(pos.x).max(0.0)));
        state.menu_line = near_line(pos).then(|| geometry.snap(geometry.beat_of(pos.x), free));
    }
    body.context_menu(|ui| {
        let Some(beat) = state.menu_at.as_ref().filter(|(t, k, _)| *t == track && k == key).map(|(_, _, b)| *b) else {
            return;
        };
        // On the line: Add value puts a point on it there, on the grid, keeping its shape.
        if let Some(at) = state.menu_line {
            if ui.button("Add value").clicked() {
                let mut params = key.params(track);
                params["beat"] = json!(at);
                // Without a value the command splits a bent segment so it plays as before; a lane
                // without points takes the faint line's value.
                if param.points.is_empty() {
                    params["value"] = param.scale.command(param.current);
                }
                if let Some(shape) = state.new_shape {
                    params["shape"] = json!(shape);
                }
                actions.push(Action::new("automation.addPoint", params));
                state.selection = None;
                ui.close();
            }
            ui.separator();
        }
        match state.range.as_ref().filter(|r| r.track == track && &r.key == key && (r.start..=r.end).contains(&beat)) {
            // Inside a painted range: the shape fills it, once a beat for a repeating one.
            Some(range) => {
                let (start, end) = (range.start, range.end);
                // Where a shape of less depth swings: the line's place at the range's start now.
                let center =
                    suhina_engine::model::automation::value_at(&level, start, param.scale.stepped(), &param.scale)
                        .map_or(0.5, |v| param.scale.position(v));
                range_menu(ui, state, track, key, (start, end), center, geometry.grid.bar_beats, actions);
            }
            // Elsewhere nothing is drawn: the shape is the one the next points' segments take,
            // and a click on the chosen one goes back to straight lines.
            None => {
                if let Some(shape) = shape_menu(ui, "Shape for new points", state.new_shape) {
                    state.new_shape = (state.new_shape != Some(shape)).then_some(shape);
                    ui.close();
                }
                if state.new_shape.is_some() && ui.button("Straight lines for new points").clicked() {
                    state.new_shape = None;
                    ui.close();
                }
                picked_menu(ui, state, track, key, actions);
            }
        }
        grid_menu(ui, state, actions);
    });

    // The envelope: in the track colour while it plays, dimmed while a manual change has turned
    // it off.
    let lane_painter = painter.with_clip_rect(visible);
    // A point being dragged is drawn where the pointer is now: its own response, read further
    // down, would only move it after this frame's line and dots, a frame behind the pointer.
    if let Some(drag) = state.point_drag.as_mut().filter(|d| !d.segment && d.track == track && &d.key == key)
        && let Some(pos) = ui.input(|i| i.pointer.primary_down().then(|| i.pointer.latest_pos()).flatten())
    {
        drag.now = dragged_to(drag.grab, geometry.beat_of(pos.x), position_of(plot, pos.y), plot, geometry, shift);
    }
    let points = shown_points(param, track, geometry, free, state);
    let ink = if param.enabled { color(lanes.color) } else { theme::text_hint() };
    let line = envelope(&points, &param.scale, plot, geometry);
    // A finished bend asks for its command only now, after the envelope above drew it: the
    // project has the new curve only once the command has run, after this frame, so dropping
    // the drag before drawing showed the old curve for one frame.
    if body.drag_stopped_by(egui::PointerButton::Primary)
        && let Some(drag) = state.curve_drag.take_if(|d| d.track == track && &d.key == key)
    {
        let before = param.points.get(drag.point).map_or(Curve::STRAIGHT, |p| p.curve);
        if drag.curve != before {
            let mut params = key.params(track);
            params["point"] = json!(drag.point);
            params["curve"] = json!(drag.curve.to_array());
            actions.push(Action::new("automation.movePoint", params));
        }
    }
    // A lane with points drawn without its line: the line is missing or has a place that is not
    // a number (a flash of the faint line or the empty lane, #916).
    let broken = line.iter().filter(|p| !p.x.is_finite() || !p.y.is_finite()).count();
    if !param.points.is_empty() && (line.len() < 2 || broken > 0) {
        suhina_plugins::trace!(
            "automation lane {} {}/{:?} drew no whole line: {} points, {} shown, line of {}, {broken} not numbers, drag {:?}",
            track.0,
            key.param,
            key.device,
            param.points.len(),
            points.len(),
            line.len(),
            state.point_drag.as_ref().map(|d| (d.point, d.together.len(), d.grab, d.now)),
        );
    }
    // What each lane drew, traced when it changes: a lane that flashes to the faint line (or to
    // nothing) and back shows in the log, with how many points it had.
    if suhina_plugins::trace::on() {
        let drew = if line.len() > 1 {
            "line"
        } else if points.is_empty() {
            "faint line"
        } else {
            "nothing"
        };
        suhina_plugins::trace::state(
            &format!("automation lane {} {}/{:?}", track.0, key.param, key.device),
            format!("{drew}, {} points", param.points.len()),
        );
    }
    if line.len() > 1 {
        lane_painter.add(Shape::line(line, Stroke::new(1.5, ink)));
    } else if points.is_empty() {
        // No automation yet: a faint line at the parameter's value, where a click adds a point.
        let y = y_of(plot, param.scale.position(param.current));
        lane_painter.hline(plot.x_range(), y, Stroke::new(1.0, ink.gamma_multiply(LEVEL_ALPHA)));
    }
    // The range being painted, and the one painted: the lane's whole height between them.
    if let Some(drag) = state.box_drag.as_ref().filter(|d| d.select && d.track == track && &d.key == key) {
        let area = Rect::from_two_pos(drag.from, drag.to);
        lane_painter.rect_filled(area, 0.0, theme::accent().gamma_multiply(0.15));
        lane_painter.rect_stroke(area, 0.0, theme::hairline(theme::accent()), StrokeKind::Inside);
    }
    let painted = match state.box_drag.as_ref().filter(|d| !d.select && d.track == track && &d.key == key) {
        Some(drag) => Some((
            geometry.x_of(geometry.snap(geometry.beat_of(drag.from.x), free)),
            geometry.x_of(geometry.snap(geometry.beat_of(drag.to.x), free)),
        )),
        None => state
            .range
            .as_ref()
            .filter(|r| r.track == track && &r.key == key)
            .map(|r| (geometry.x_of(r.start), geometry.x_of(r.end))),
    };
    if let Some((a, b)) = painted {
        let area = Rect::from_x_y_ranges(egui::Rangef::new(a.min(b), a.max(b)), time.y_range());
        lane_painter.rect_filled(area, 0.0, theme::accent().gamma_multiply(0.15));
        lane_painter.rect_stroke(area, 0.0, theme::hairline(theme::accent()), StrokeKind::Inside);
    }

    // A round handle half-way along each sloping segment wide enough: dragged up or down it bends
    // the segment as dragging its line does, a double-click straightens it (as a fade's curve
    // handle).
    if !param.scale.stepped() && state.point_drag.is_none() {
        for (index, pair) in points.windows(2).enumerate() {
            let (a, b) = (pair[0], pair[1]);
            let (xa, xb) = (geometry.x_of(a.beat), geometry.x_of(b.beat));
            if xb - xa < CURVE_HANDLE_MIN || a.value == b.value {
                continue;
            }
            let mid = (a.beat + b.beat) / 2.0;
            let Some(value) = suhina_engine::model::automation::value_at(&points, mid, false, &param.scale) else {
                continue;
            };
            let center = Pos2::new((xa + xb) / 2.0, y_of(plot, param.scale.position(value)));
            let hit = Rect::from_center_size(center, Vec2::splat(POINT_HIT)).intersect(visible);
            if hit.width() <= 0.0 || hit.height() <= 0.0 {
                continue;
            }
            let response = ui.interact(hit, id.with(("curve", index)), Sense::click_and_drag());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Slider,
                    true,
                    format!("{label} curve {} on {}", index + 1, lanes.name),
                )
            });
            let rising = param.scale.position(b.value) >= param.scale.position(a.value);
            if response.double_clicked() {
                let mut params = key.params(track);
                params["point"] = json!(index);
                params["curve"] = json!(0.0);
                actions.push(Action::new("automation.movePoint", params));
            } else if response.drag_started_by(egui::PointerButton::Primary) {
                let curve = param.points.get(index).map_or(Curve::STRAIGHT, |p| p.curve);
                let paths = suhina_engine::model::automation::CurveDrag::new(&curve);
                let curve = paths.dragged(0);
                state.curve_drag = Some(CurveDrag { track, key: key.clone(), point: index, paths, moved: 0.0, curve });
            }
            if response.dragged_by(egui::PointerButton::Primary)
                && let Some(drag) =
                    state.curve_drag.as_mut().filter(|d| d.track == track && &d.key == key && d.point == index)
            {
                let dy = response.drag_delta().y;
                drag.moved += if rising { dy } else { -dy };
                drag.curve = drag.paths.dragged((drag.moved / POINTS_PER_STEP).round() as i32);
            }
            if response.drag_stopped_by(egui::PointerButton::Primary)
                && let Some(drag) = state.curve_drag.take_if(|d| d.track == track && &d.key == key && d.point == index)
            {
                let before = param.points.get(index).map_or(Curve::STRAIGHT, |p| p.curve);
                if drag.curve != before {
                    let mut params = key.params(track);
                    params["point"] = json!(index);
                    params["curve"] = json!(drag.curve.to_array());
                    actions.push(Action::new("automation.movePoint", params));
                }
            }
            let hover = widgets::hover(ui, &response);
            lane_painter.circle_filled(center, CURVE_HANDLE + hover, theme::window_background());
            lane_painter.circle_stroke(center, CURVE_HANDLE + hover, Stroke::new(1.2, ink));
        }
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
        let pass = ui.ctx().cumulative_pass_nr();
        if state.placed_pass != pass {
            state.placed.clear();
            state.placed_pass = pass;
        }
        state.placed.push((track, key.clone(), index, center));
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
            state.range = None;
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
            state.point_drag = Some(PointDrag {
                track,
                key: key.clone(),
                point: index,
                together,
                grab,
                now: grab,
                segment: false,
                body: false,
            });
        }
        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
            && let Some(drag) =
                state.point_drag.as_mut().filter(|d| d.track == track && &d.key == key && d.point == index)
        {
            let shift = ui.input(|i| i.modifiers.shift);
            drag.now = dragged_to(drag.grab, geometry.beat_of(pos.x), position_of(plot, pos.y), plot, geometry, shift);
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
            // Edit value: a field beside the point takes the value typed, in the set command's unit.
            if ui.button("Edit value").clicked() {
                state.editing = Some((track, key.clone(), index, param.scale.typed_text(point.value)));
                ui.close();
            }
            ui.separator();
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
            // The segment from the point before to this one takes a shape, between their heights.
            if param.points.len() > 1 && !param.scale.stepped() {
                ui.separator();
                if let Some(shape) = shape_menu(ui, "Shape to this point", None) {
                    let mut params = key.params(track);
                    params["point"] = json!(index);
                    params["shape"] = json!(shape);
                    actions.push(Action::new("automation.insertShape", params));
                    state.selection = None;
                    ui.close();
                }
            }
            picked_menu(ui, state, track, key, actions);
            grid_menu(ui, state, actions);
        });
        // The field Edit value opened, beside the point: Enter (or a click elsewhere) sets the
        // value typed, Escape leaves it.
        if let Some((_, _, _, text)) =
            state.editing.as_mut().filter(|(t, k, p, _)| *t == track && k == key && *p == index)
        {
            let field = Rect::from_min_size(center + Vec2::new(6.0, -22.0), Vec2::new(64.0, 18.0));
            let edit_id = point_id.with("edit");
            let response = ui.put(
                field,
                egui::TextEdit::singleline(text)
                    .id(edit_id)
                    .font(theme::mono_font(theme::TEXT_SMALL))
                    .desired_width(60.0),
            );
            // It opens with the whole value selected, so typing replaces it.
            if !response.has_focus() && !response.lost_focus() {
                response.request_focus();
                if let Some(mut edit) = egui::TextEdit::load_state(ui.ctx(), edit_id) {
                    let all = egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(text.chars().count()),
                    );
                    edit.cursor.set_char_range(Some(all));
                    edit.store(ui.ctx(), edit_id);
                }
            }
            let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if response.lost_focus() || escape {
                let typed = text.clone();
                state.editing = None;
                if !escape && let Some(value) = param.scale.parse_typed(&typed) {
                    let mut params = key.params(track);
                    params["point"] = json!(index);
                    params["value"] = value;
                    actions.push(Action::new("automation.movePoint", params));
                }
            }
        }
        let hover = widgets::hover(ui, &response);
        let radius = POINT + hover;
        let fill = if selected { theme::text_primary() } else { ink };
        lane_painter.circle_filled(center, radius, fill);
        lane_painter.circle_stroke(center, radius, Stroke::new(1.0, theme::window_background()));
    }
}

/// The menu of a painted range: how many bars it holds; the shapes, with Depth and Cycles per bar
/// for them (a change puts the shape put in last in again, live while a slider is dragged); and
/// for whatever points the range holds, Scale, Smooth (rounds corners and jumps) and Fewer points.
#[allow(clippy::too_many_arguments)]
fn range_menu(
    ui: &mut Ui,
    state: &mut LanesState,
    track: TrackId,
    key: &LaneKey,
    (start, end): (f64, f64),
    center: f64,
    bar_beats: f64,
    actions: &mut Vec<Action>,
) {
    let bar = bar_beats.max(1e-3);
    let bars = (end - start) / bar;
    let shown = if (bars - bars.round()).abs() < 1e-6 { format!("{}", bars.round()) } else { format!("{bars:.2}") };
    ui.label(
        egui::RichText::new(format!("{shown} bar{} selected", if (bars - 1.0).abs() < 1e-6 { "" } else { "s" }))
            .color(theme::text_hint())
            .size(theme::TEXT_SMALL),
    );
    // The lane and the range, the params every command of this menu starts from.
    let ranged = || {
        let mut params = key.params(track);
        params["start"] = json!(start);
        params["end"] = json!(end);
        params
    };
    let put = |state: &mut LanesState, shape: &'static str, actions: &mut Vec<Action>| {
        let rate = state.shape_rate.unwrap_or(bar.round().max(1.0));
        let mut params = ranged();
        params["shape"] = json!(shape);
        params["cycles"] = json!(((rate * bars).round() as usize).clamp(1, 512));
        params["depth"] = json!(state.shape_depth.unwrap_or(1.0));
        // Put in again in the same range, the shape swings around the place the first one did,
        // and replaces it when nothing else changed since (`continue` with `replace`), so the
        // range's edges come from the line as it was, not from the shape before.
        let again =
            state.last_shape.clone().filter(|l| l.track == track && &l.key == key && l.start == start && l.end == end);
        let center = again.as_ref().map_or(center, |l| l.center);
        params["center"] = json!(center.clamp(0.0, 1.0));
        params["continue"] = json!(again.is_some());
        params["replace"] = json!(again.is_some());
        actions.push(Action::new("automation.insertShape", params));
        state.selection = None;
        state.smooth = None;
        state.last_shape = Some(LastShape { track, key: key.clone(), start, end, shape, center });
    };
    if let Some(shape) = shape_menu(ui, "Insert shape", None) {
        put(state, shape, actions);
        ui.close();
    }
    let mut depth = state.shape_depth.unwrap_or(1.0) * 100.0;
    let depth_slider =
        ui.add(egui::Slider::new(&mut depth, 0.0..=100.0).text("Depth").suffix(" %").fixed_decimals(0)).on_hover_text(
            "How far the inserted shape swings around its middle: 100 % from the bottom of the lane to its top, \
             50 % half of that. Changing it puts the shape in again.",
        );
    if depth_slider.changed() {
        state.shape_depth = Some(depth / 100.0);
    }
    let mut rate = state.shape_rate.unwrap_or(bar.round().max(1.0));
    let rate_slider = ui.add(egui::Slider::new(&mut rate, 1.0..=32.0).text("Cycles per bar").integer()).on_hover_text(
        "How many times the inserted shape repeats in each bar: 4 is once a beat in 4/4. \
             Changing it puts the shape in again.",
    );
    if rate_slider.changed() {
        state.shape_rate = Some(rate);
    }
    let depth_due = live(ui, state, &depth_slider);
    let rate_due = live(ui, state, &rate_slider);
    if (depth_due || rate_due)
        && let Some(shape) = state
            .last_shape
            .as_ref()
            .filter(|l| l.track == track && &l.key == key && l.start == start && l.end == end)
            .map(|l| l.shape)
    {
        put(state, shape, actions);
    }
    ui.separator();
    ui.label(egui::RichText::new("Points in the range").color(theme::text_hint()).size(theme::TEXT_SMALL));
    let of = format!("{}/{key:?}/range {start} {end}", track.0);
    if let Some((factor, joined)) = stretch_slider(ui, state, of.clone()) {
        let mut params = ranged();
        params["factor"] = json!(factor);
        params["continue"] = json!(joined);
        actions.push(Action::new("automation.scaleRange", params));
    }
    let kept = state.smooth.clone().filter(|s| s.of == of);
    let mut amount = kept.as_ref().map_or(0.0, |s| s.amount) * 100.0;
    let smooth_slider = ui
        .add(egui::Slider::new(&mut amount, 0.0..=100.0).text("Smooth").suffix(" %").fixed_decimals(0))
        .on_hover_text(
            "Rounds the corners and jumps in the range into curves: a square's steps ease from one value \
             to the next, a triangle's tips turn round. 0 % leaves them sharp.",
        );
    let amount = amount / 100.0;
    state.smooth = Some(Smoothed { of, amount });
    if live(ui, state, &smooth_slider) {
        // Each amount replaces the one before in this range's step (`continue` with `replace`),
        // so it rounds the line as it was; a line it does not change (a sine's) stays as it is.
        let mut params = ranged();
        params["amount"] = json!(amount.clamp(0.0, 1.0));
        params["continue"] = json!(kept.is_some());
        params["replace"] = json!(kept.is_some());
        actions.push(Action::new("automation.smooth", params));
        state.selection = None;
    }
    if ui
        .button("Fewer points")
        .on_hover_text("Removes the points the line does not need, bending the rest to keep its shape")
        .clicked()
    {
        actions.push(Action::new("automation.simplify", ranged()));
        state.selection = None;
        state.smooth = None;
        ui.close();
    }
}

/// The picked points' part of a lane's menus, when points of this lane are picked: how many, and
/// Scale for them (`automation.scaleRange` with them, live while it is dragged).
fn picked_menu(ui: &mut Ui, state: &mut LanesState, track: TrackId, key: &LaneKey, actions: &mut Vec<Action>) {
    let Some(points) = state
        .selection
        .as_ref()
        .filter(|s| s.track == track && &s.key == key && !s.points.is_empty())
        .map(|s| s.points.clone())
    else {
        return;
    };
    ui.separator();
    let count = points.len();
    ui.label(
        egui::RichText::new(format!("{count} point{} selected", if count == 1 { "" } else { "s" }))
            .color(theme::text_hint())
            .size(theme::TEXT_SMALL),
    );
    if let Some((factor, joined)) = stretch_slider(ui, state, format!("{}/{key:?}/points {points:?}", track.0)) {
        let mut params = key.params(track);
        params["points"] = json!(points);
        params["factor"] = json!(factor);
        params["continue"] = json!(joined);
        actions.push(Action::new("automation.scaleRange", params));
    }
}

/// The grid entries at the end of the lanes' menus: the Arrangement's grid, as its toolbar and
/// its lanes' menus offer it.
fn grid_menu(ui: &mut Ui, state: &mut LanesState, _actions: &mut Vec<Action>) {
    ui.separator();
    let keys = state.grid_keys.clone();
    if let Some(picked) = crate::detail::piano_roll::grid_entries(ui, state.grid, &keys, &|_| true) {
        state.grid_picked = Some(picked);
    }
}

/// A menu's shapes under `title`, drawn as small pictures in rows of five, `chosen` lit; the one
/// clicked, by name.
fn shape_menu(ui: &mut Ui, title: &str, chosen: Option<&str>) -> Option<&'static str> {
    ui.label(egui::RichText::new(title).color(theme::text_hint()).size(theme::TEXT_SMALL));
    let mut picked = None;
    for row in suhina_session::AUTOMATION_SHAPES.chunks(5) {
        ui.horizontal(|ui| {
            for &name in row {
                let (rect, response) = ui.allocate_exact_size(Vec2::new(30.0, 20.0), Sense::click());
                let label = format!("Insert {}", name.replace('_', " "));
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
                let response = response.on_hover_text(&label);
                let painter = ui.painter();
                let fill = if chosen == Some(name) {
                    theme::control_active()
                } else {
                    crate::kit::motion::mix(
                        theme::window_background(),
                        theme::control_hover(),
                        widgets::hover(ui, &response),
                    )
                };
                painter.rect_filled(rect, 2.0, fill);
                let line = shape_icon(name, rect.shrink2(Vec2::new(5.0, 4.0)));
                painter.add(Shape::line(line, Stroke::new(1.3, theme::text_secondary())));
                if response.clicked() {
                    picked = Some(name);
                }
            }
        });
    }
    picked
}

/// The line of shape `name` over one cycle in `rect`, its curves followed.
fn shape_icon(name: &str, rect: Rect) -> Vec<Pos2> {
    let Some((points, _)) = suhina_session::automation_shape(name) else { return Vec::new() };
    let at =
        |t: f64, v: f64| Pos2::new(rect.left() + t as f32 * rect.width(), rect.bottom() - v as f32 * rect.height());
    let mut line = Vec::new();
    for pair in points.windows(2) {
        let (&(t0, v0, curve), &(t1, v1, _)) = (&pair[0], &pair[1]);
        let curve = Curve::bend(curve);
        let steps = if curve.is_straight() { 1 } else { 8 };
        for i in 0..=steps {
            let f = f64::from(i) / f64::from(steps);
            let v = v0 + (v1 - v0) * curve.shape(f);
            line.push(at(t0 + (t1 - t0) * f, v));
        }
    }
    line
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
            painter.fill_background(rect, 2.0, ui.visuals().widgets.hovered.weak_bg_fill);
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
    let text = egui::RichText::new(format!("{}{mark}", choice.short_name()));
    let text = if selected { text.color(theme::text_on(ui.visuals().selection.bg_fill)) } else { text };
    let button = egui::Button::selectable(selected, text);
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
    let text = egui::RichText::new(&label);
    let text = if selected { text.color(theme::text_on(ui.visuals().selection.bg_fill)) } else { text };
    let response = ui.add_enabled(free, egui::Button::selectable(selected, text));
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
    painter.fill_background(rect, 0.0, theme::panel_background());
    let track = lanes.track;
    let id = Id::new(("automation-header", track.0, param.key.device, param.key.param.as_str()));
    // A right-click on the header's free space: the lane's height back to normal (#818).
    let background = ui.interact(clip, id.with("background"), Sense::click());
    background.context_menu(|ui| {
        let tall = state.heights.contains_key(&(track, param.key.clone()));
        if ui.add_enabled(tall, egui::Button::new("Reset height")).clicked() {
            state.set_lane_height(track, &param.key, LANE);
            ui.close();
        }
    });
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
        // The theme's popup fill: see-through where the theme is, with the window blurred under it.
        .frame(egui::Frame::popup(ui.style()))
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
        painter.fill_background(toggle, 0.0, fill);
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
    painter.fill_background(rect, 0.0, fill);
    widgets::text_in(painter, rect, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT), theme::text_secondary());
}

#[cfg(test)]
#[path = "automation_lanes_tests.rs"]
mod tests;
