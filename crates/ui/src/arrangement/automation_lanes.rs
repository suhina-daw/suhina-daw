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
const SMALL_BUTTON: f32 = 18.0;
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

/// The parameter a track's top lane shows when its lanes are first opened (Settings ›
/// Arrangement › Automation › Top lane shows, #1322): none, the track's volume or its pan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TopLane {
    None,
    #[default]
    Volume,
    Pan,
}

impl TopLane {
    /// The lane it names, `None` for none.
    pub fn key(self) -> Option<LaneKey> {
        match self {
            Self::None => None,
            Self::Volume => Some(LaneKey::mixer(VOLUME)),
            Self::Pan => Some(LaneKey::mixer(PAN)),
        }
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
    /// A hosted plug-in's parameter, evenly over its own range (#1205).
    Plugin(Box<PluginScale>),
}

/// A hosted plug-in's parameter as it listed it (#1205): its own range (a VST3 plug-in's
/// normalized 0 … 1, a CLAP plug-in's plain values), its steps, and the plug-in's own text for
/// the values it was asked about.
#[derive(Debug, Clone, Default)]
pub struct PluginScale {
    /// The parameter's id, which the plug-in's texts are kept by.
    pub id: u32,
    pub min: f64,
    pub max: f64,
    /// Steps of a stepped parameter (a switch has 1), 0 for a continuous one.
    pub steps: u32,
    /// The value the plug-in listed and its text for it.
    pub listed: Option<(f64, String)>,
    /// The plug-in's texts for values it was asked about, as the session keeps them.
    pub texts: std::sync::Arc<suhina_session::ParamTexts>,
    /// Values a text was wanted for and not known yet, for the app to ask the plug-in about.
    pub asked: std::sync::Arc<std::sync::Mutex<Vec<f64>>>,
}

impl PartialEq for PluginScale {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.min == other.min
            && self.max == other.max
            && self.steps == other.steps
            && self.listed == other.listed
            && std::sync::Arc::ptr_eq(&self.texts, &other.texts)
    }
}

/// Most values one parameter's lane asks a plug-in's text for between two frames.
const MAX_ASKED: usize = 64;

impl PluginScale {
    /// The range low to high, whichever way the plug-in gave it; 0 … 1 when it is not a range.
    fn range(&self) -> (f64, f64) {
        let (low, high) = (self.min.min(self.max), self.min.max(self.max));
        if low.is_finite() && high.is_finite() { (low, high) } else { (0.0, 1.0) }
    }

    /// `value` kept in the range and, for a stepped parameter, on its nearest step.
    pub fn snap(&self, value: f64) -> f64 {
        let (low, high) = self.range();
        let value = if value.is_finite() { value.clamp(low, high) } else { low };
        let span = high - low;
        if self.steps == 0 || span <= 0.0 {
            return value;
        }
        let step = span / f64::from(self.steps);
        (low + ((value - low) / step).round() * step).clamp(low, high)
    }

    /// Where `value` sits on the lane, 0 at the bottom of the range to 1 at its top.
    fn position(&self, value: f64) -> f64 {
        let (low, high) = self.range();
        if high - low <= 0.0 { 0.0 } else { (self.snap(value) - low) / (high - low) }
    }

    /// The value at `position` (0 … 1), on a step for a stepped parameter.
    fn value_at(&self, position: f64) -> f64 {
        let (low, high) = self.range();
        self.snap(low + position * (high - low))
    }

    /// The plug-in's own text for `value` when it gave one; else the number.
    pub fn text(&self, value: f64) -> String {
        let value = self.snap(value);
        match self.known_text(value) {
            Some(text) => text,
            None => {
                let (low, high) = self.range();
                if self.steps > 0 && high - low >= f64::from(self.steps) {
                    format!("{}", value.round())
                } else {
                    format!("{value:.3}")
                }
            }
        }
    }

    /// The text the plug-in gave for `value`, if it gave one. A value it was never asked about
    /// is noted for the app to ask it, so the next frames show its text.
    pub fn known_text(&self, value: f64) -> Option<String> {
        let (low, high) = self.range();
        let near = ((high - low) * 1e-6).max(1e-12);
        if let Some((_, text)) =
            self.listed.as_ref().filter(|(at, text)| (at - value).abs() <= near && !text.trim().is_empty())
        {
            return Some(text.clone());
        }
        let known = self.texts.lock().ok()?.get(&(self.id, value.to_bits())).cloned();
        match known {
            Some(text) => Some(text).filter(|text| !text.trim().is_empty()),
            None => {
                if let Ok(mut asked) = self.asked.lock()
                    && asked.len() < MAX_ASKED
                    && !asked.iter().any(|a| a.to_bits() == value.to_bits())
                {
                    asked.push(value);
                }
                None
            }
        }
    }
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
            Self::Plugin(plugin) => plugin.position(value),
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
            Self::Plugin(plugin) => plugin.value_at(position),
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

    /// `value` as [`Self::command`] sets it, back in the lane's unit: the pan rounded to a
    /// thousandth, the volume to a hundredth of a decibel (silent at its floor), so text shown
    /// before a command is the text the point has after it.
    fn committed(&self, value: f64) -> f64 {
        match self {
            Self::Volume => match self.number(value) {
                db if value <= 0.0 || db <= VOLUME_FLOOR => 0.0,
                db => 10f64.powf(db / 20.0),
            },
            _ => self.number(value),
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
            Self::Plugin(plugin) => {
                let (low, high) = plugin.range();
                (high - low) / if plugin.steps > 0 { f64::from(plugin.steps) } else { 100.0 }
            }
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
            Self::Plugin(plugin) => plugin.snap(value),
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
            Self::Plugin(plugin) => plugin.snap(value),
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
            Self::Plugin(plugin) => plugin.snap(value + delta),
        }
    }

    /// Whether each point's value holds until the next (on/off and choices).
    pub fn stepped(&self) -> bool {
        match self {
            Self::Mute => true,
            Self::Device(info) => matches!(info.kind, ParamKind::Toggle | ParamKind::Choice(_)),
            Self::Sd(parameter) => parameter.control.is_stepped(),
            Self::Plugin(plugin) => plugin.steps > 0,
            Self::Volume | Self::Pan => false,
        }
    }

    /// A value as the lane's header shows it.
    pub fn text(&self, value: f64) -> String {
        match self {
            Self::Volume => format!("{} dB", widgets::format_db(gain_to_db(value))),
            Self::Pan => widgets::format_pan(value),
            Self::Mute => if value >= 0.5 { "Muted" } else { "Sounding" }.into(),
            Self::Device(info) => match &info.kind {
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
                        Unit::Key => crate::interaction::key_name::key_name(value.clamp(0.0, 127.0) as u8),
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
                    Unit::Key => crate::interaction::key_name::key_name(value.round().clamp(0.0, 127.0) as u8),
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
            Self::Plugin(plugin) => plugin.text(value),
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
    /// Each laid-out track's lane layout from the project, its devices by their place (#1322).
    layouts: HashMap<TrackId, Layout>,
    /// The plug-ins' parameter lists they were built with (#1205), so lists that have not
    /// changed are not built again.
    plugins: PluginLists,
}

/// What the hosted plug-ins on the tracks listed (#1205): each one's parameters as it last listed
/// them and its own texts for values, by its track and its place in the track's chain.
#[derive(Debug, Clone, Default)]
pub struct PluginLists {
    pub lists: HashMap<(TrackId, usize), std::sync::Arc<[suhina_plugins::ParamInfo]>>,
    /// Each plug-in's own texts for values it was asked about, as the session keeps them.
    pub texts: HashMap<(TrackId, usize), std::sync::Arc<suhina_session::ParamTexts>>,
    /// Parameters moved in a plug-in's own editor since the last frame and the value each has
    /// now (track, device, parameter id, value): a lane's faint line follows them at once,
    /// not only when the plug-in lists its parameters again a couple of seconds later.
    pub moved: Vec<(TrackId, usize, u32, f64)>,
}

impl PartialEq for PluginLists {
    /// The same lists, the very ones the session handed over: comparing thousands of parameters
    /// every frame would cost more than building them again.
    fn eq(&self, other: &Self) -> bool {
        fn same<T: ?Sized>(
            a: &HashMap<(TrackId, usize), std::sync::Arc<T>>,
            b: &HashMap<(TrackId, usize), std::sync::Arc<T>>,
        ) -> bool {
            a.len() == b.len() && a.iter().all(|(key, x)| b.get(key).is_some_and(|y| std::sync::Arc::ptr_eq(x, y)))
        }
        // `moved` is this frame's news, not what the parameters were built with.
        same(&self.lists, &other.lists) && same(&self.texts, &other.texts)
    }
}

impl Params {
    /// Every track's parameters from `project`: its mixer's volume, pan and mute, then each
    /// built-in and `.sd` device's parameters in device order, with their lanes' points.
    pub fn build(project: &Project) -> Self {
        Self::build_with(project, PluginLists::default())
    }

    /// Builds the parameters again with `plugins` when they are not the lists the parameters
    /// were built with (#1205).
    pub fn take_plugins(&mut self, project: &Project, mut plugins: PluginLists) {
        let moved = std::mem::take(&mut plugins.moved);
        if self.plugins != plugins {
            *self = Self::build_with(project, plugins);
        }
        for (track, device, id, value) in moved {
            let key = LaneKey { device: Some(device), param: id.to_string() };
            // A move by hand also turns the parameter's lane off: the header says so at once.
            let enabled = project
                .channel_of_track(track)
                .and_then(|c| c.devices.get(device))
                .and_then(|d| d.automation.iter().find(|l| l.param == key.param))
                .map(|l| l.enabled);
            if let Some(param) = self.tracks.get_mut(&track).and_then(|params| params.iter_mut().find(|p| p.key == key))
                && value.is_finite()
            {
                param.current = value;
                if let Some(enabled) = enabled {
                    param.enabled = enabled;
                }
            }
        }
    }

    /// [`Params::build`], with each hosted plug-in's parameters from `plugins` after the
    /// built-in and `.sd` devices' (#1205).
    pub fn build_with(project: &Project, plugins: PluginLists) -> Self {
        let mut tracks = HashMap::new();
        let mut layouts = HashMap::new();
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
                    points: lane.map(|l| l.points.into_values()).unwrap_or_default(),
                    current,
                }
            })
            .collect();
            for (index, device) in channel.devices.iter().enumerate() {
                let set = |id: &str| device.params.iter().find(|p| p.id == id).map(|p| p.value);
                match &device.kind {
                    DeviceKind::Builtin(id) => {
                        let Some(info) = suhina_device_registry::info(id) else { continue };
                        for spec in info.params.iter() {
                            let lane = lane_of(&device.automation, &spec.id);
                            params.push(Param {
                                key: LaneKey { device: Some(index), param: spec.id.to_string() },
                                name: spec.name.to_string(),
                                device_name: Some(device.name.clone()),
                                group: (!spec.group.is_empty()).then(|| spec.group.to_string()),
                                scale: Scale::Device(spec.clone()),
                                enabled: lane.as_ref().is_none_or(|l| l.enabled),
                                points: lane.map(|l| l.points.into_values()).unwrap_or_default(),
                                current: set(&spec.id).unwrap_or(spec.default),
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
                                points: lane.map(|l| l.points.into_values()).unwrap_or_default(),
                                current: set(&spec.id).unwrap_or(spec.default),
                            });
                        }
                    }
                    DeviceKind::Clap { .. } | DeviceKind::Vst3 { .. } | DeviceKind::Vst2 { .. } => {
                        params.extend(plugin_params(track, index, device, &plugins));
                    }
                    _ => {}
                }
            }
            tracks.insert(track, params);
            // The project's layout of the channel's lanes, devices by id turned into places.
            if let Some(kept) = project.lane_layouts.iter().find(|l| l.channel == channel.id) {
                let key = |lane: &suhina_engine::model::LaneRef| -> Option<LaneKey> {
                    let device = match lane.device {
                        None => None,
                        Some(id) => Some(channel.devices.iter().position(|d| d.id == id)?),
                    };
                    Some(LaneKey { device, param: lane.param.clone() })
                };
                let groups = kept
                    .groups
                    .iter()
                    .map(|g| LaneGroup {
                        name: g.name.clone(),
                        keys: g.lanes.iter().filter_map(key).collect(),
                        folded: g.folded,
                        device: g.device.and_then(|id| channel.devices.iter().position(|d| d.id == id)),
                    })
                    .collect();
                layouts.insert(track, Layout { lanes: kept.lanes.iter().filter_map(key).collect(), groups });
            }
        }
        Self { tracks, layouts, plugins }
    }

    /// `track`'s lane layout: the project's, or, for a track never laid out, its lanes that have
    /// points, under the top lane (#1322).
    pub fn layout(&self, track: TrackId) -> Layout {
        match self.layouts.get(&track) {
            Some(layout) => layout.clone(),
            None => Layout {
                lanes: self.of(track).iter().filter(|p| !p.points.is_empty()).map(|p| p.key.clone()).collect(),
                groups: Vec::new(),
            },
        }
    }

    /// The values of hosted plug-ins' parameters whose text the lanes wanted since the last call
    /// and did not know (#1205), by track, device and parameter id, for the app to ask for.
    pub fn asked_texts(&self) -> Vec<(TrackId, usize, u32, Vec<f64>)> {
        let mut asked = Vec::new();
        for (track, params) in &self.tracks {
            for param in params {
                let (Scale::Plugin(plugin), Some(device)) = (&param.scale, param.key.device) else { continue };
                let values = plugin.asked.lock().map(|mut a| std::mem::take(&mut *a)).unwrap_or_default();
                if !values.is_empty() {
                    asked.push((*track, device, plugin.id, values));
                }
            }
        }
        asked
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

/// A hosted plug-in's parameters as lanes can show them (#1205): those it lists as automatable
/// and lets be seen and changed, in its order, grouped by its own groups; then each lane of its
/// whose parameter it does not list (it is not running, is not installed, or no longer has it),
/// kept with its points and named as missing.
fn plugin_params(
    track: TrackId,
    index: usize,
    device: &suhina_engine::model::Device,
    plugins: &PluginLists,
) -> Vec<Param> {
    let lane_of = |id: &str| device.automation.iter().find(|l| l.param == id);
    let set = |id: u32| device.params.iter().find(|p| p.id.parse::<u32>().ok() == Some(id)).map(|p| p.value);
    let texts = plugins.texts.get(&(track, index)).cloned().unwrap_or_default();
    let mut params = Vec::new();
    let listed = plugins.lists.get(&(track, index)).map_or(&[][..], |list| &list[..]);
    for info in listed.iter().filter(|p| p.automatable && !p.hidden && !p.read_only) {
        let id = info.id.to_string();
        let lane = lane_of(&id);
        params.push(Param {
            key: LaneKey { device: Some(index), param: id },
            name: if info.name.trim().is_empty() { format!("Parameter {}", info.id) } else { info.name.clone() },
            device_name: Some(device.name.clone()),
            group: (!info.group.trim().is_empty()).then(|| info.group.clone()),
            scale: Scale::Plugin(Box::new(PluginScale {
                id: info.id,
                min: info.min,
                max: info.max,
                steps: info.steps,
                listed: (!info.text.is_empty()).then(|| (info.value, info.text.clone())),
                texts: texts.clone(),
                asked: Default::default(),
            })),
            enabled: lane.is_none_or(|l| l.enabled),
            points: lane.map(|l| l.points.to_vec()).unwrap_or_default(),
            current: set(info.id).unwrap_or(info.value),
        });
    }
    // A lane the list does not name stays, with its points, so nothing is lost while the
    // plug-in is away; its range is what its points and the project's value reach, at least 0 … 1.
    for lane in &device.automation {
        if params.iter().any(|p| p.key.param == lane.param) {
            continue;
        }
        let Ok(id) = lane.param.parse::<u32>() else { continue };
        let values = lane.points.iter().map(|p| p.value).chain(set(id)).filter(|v| v.is_finite());
        let (min, max) = values.fold((0.0f64, 1.0f64), |(low, high), v| (low.min(v), high.max(v)));
        params.push(Param {
            key: LaneKey { device: Some(index), param: lane.param.clone() },
            name: format!("Parameter {id} (not available)"),
            device_name: Some(device.name.clone()),
            group: None,
            scale: Scale::Plugin(Box::new(PluginScale {
                id,
                min,
                max,
                texts: texts.clone(),
                ..PluginScale::default()
            })),
            enabled: lane.enabled,
            points: lane.points.to_vec(),
            current: set(id).unwrap_or(min),
        });
    }
    params
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

/// A lane whose points or curves are being dragged, or a range being painted on it, or either let
/// go of this frame: its track, its lane and its points where the drag puts them, as the lane draws
/// them. The parameter's controls show what this line plays at the playhead while the drag lasts,
/// not only once the command is in, or at `at` (#811).
#[derive(Debug, Clone, PartialEq)]
pub struct Dragged {
    pub track: TrackId,
    pub key: LaneKey,
    pub points: Vec<AutomationPoint>,
    /// The beat the controls show the line at instead of the playhead: the moving edge of a range
    /// being painted, then its start until the play start marker has moved there.
    pub at: Option<f64>,
}

/// Height of a group's heading row among a track's lanes (#1322).
const GROUP_ROW: f32 = 20.0;

/// How far in a dragged lane's drop line starts when the lane would land in a group (#1322).
const GROUP_INDENT: f32 = 16.0;

/// Some of a track's mounted lanes gathered under a heading of their own (#1322): its name, its
/// lanes (they show one after another, in the track's order), whether they are folded away, and
/// the device it was made for, by its place on the track's chain.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneGroup {
    pub name: String,
    pub keys: Vec<LaneKey>,
    pub folded: bool,
    pub device: Option<usize>,
}

/// A row of a track's lanes as they are drawn: a lane, the heading of a group (its index in the
/// track's groups), or the top lane with no parameter chosen.
#[derive(Debug, Clone, PartialEq)]
pub enum LaneRow {
    Lane(LaneKey),
    Group(usize),
    Empty,
}

/// How a track's lanes are laid out (#1322), as the project keeps it (`automation.setLayout`):
/// the lanes mounted under the top lane, top to bottom, and their groups. Every change is worked
/// out on a copy and sent whole as one command, so it is an undo step and is saved.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    pub lanes: Vec<LaneKey>,
    pub groups: Vec<LaneGroup>,
}

impl Layout {
    /// Moves `keys` (in the layout's order) to `place`. Moved as their group's, they stay in it;
    /// moved alone, they leave their group and join the one `place` puts them in, if any.
    pub fn move_lanes(&mut self, keys: &[LaneKey], place: &Place, as_group: bool) {
        let moved: Vec<LaneKey> = self.lanes.iter().filter(|k| keys.contains(k)).cloned().collect();
        if moved.is_empty() {
            return;
        }
        // The lane they go before, the group they join, worked out before anything moves.
        let (before, join) = match place {
            Place::Before { key, .. } if moved.contains(key) => return,
            Place::Before { key, join } => {
                (Some(key.clone()), join.then(|| self.groups.iter().position(|g| g.keys.contains(key))).flatten())
            }
            // Before a group's heading: before its first lane that stays, outside the group; when
            // all of its lanes move, before the next lane that stays.
            Place::BeforeGroup(index) => {
                let Some(group) = self.groups.get(*index) else { return };
                let first = self.lanes.iter().position(|k| group.keys.contains(k)).unwrap_or(self.lanes.len());
                (self.lanes.iter().skip(first).find(|k| !moved.contains(k)).cloned(), None)
            }
            Place::Into(index) => (None, Some(*index).filter(|i| *i < self.groups.len())),
            Place::End => (None, None),
        };
        self.lanes.retain(|k| !moved.contains(k));
        let at = match (place, join.and_then(|g| self.groups.get(g))) {
            // Into a group: after its last lane.
            (Place::Into(_), Some(group)) => {
                self.lanes.iter().rposition(|k| group.keys.contains(k)).map_or(self.lanes.len(), |i| i + 1)
            }
            _ => before.and_then(|b| self.lanes.iter().position(|k| *k == b)).unwrap_or(self.lanes.len()),
        };
        for (offset, key) in moved.iter().enumerate() {
            self.lanes.insert((at + offset).min(self.lanes.len()), key.clone());
        }
        if !as_group {
            for group in &mut self.groups {
                group.keys.retain(|k| !moved.contains(k));
            }
            if let Some(group) = join.and_then(|g| self.groups.get_mut(g)) {
                // In the group's order: where they were put among its lanes.
                group.keys.extend(moved.iter().cloned());
            }
        }
        self.tidy();
    }

    /// Gathers `keys` under a new group named "Group N", the first number free, in the place of
    /// the first of them; they leave the groups they were in. Answers its index.
    pub fn group(&mut self, keys: &[LaneKey]) -> Option<usize> {
        let members: Vec<LaneKey> = self.lanes.iter().filter(|k| keys.contains(k)).cloned().collect();
        if members.is_empty() {
            return None;
        }
        for group in &mut self.groups {
            group.keys.retain(|k| !members.contains(k));
        }
        let number = (1..).find(|n| !self.groups.iter().any(|g| g.name == format!("Group {n}"))).unwrap_or(1);
        let name = format!("Group {number}");
        self.groups.push(LaneGroup { name: name.clone(), keys: members, folded: false, device: None });
        self.tidy();
        self.groups.iter().position(|g| g.name == name)
    }

    /// Takes the groups `indices` away, their lanes staying where they are.
    pub fn ungroup(&mut self, indices: &[usize]) {
        let mut index = 0;
        self.groups.retain(|_| {
            let keep = !indices.contains(&index);
            index += 1;
            keep
        });
    }

    /// Takes the lanes `keys` and the groups `indices` (with their lanes) out of the layout.
    pub fn remove(&mut self, keys: &[LaneKey], indices: &[usize]) {
        let mut gone: Vec<LaneKey> = keys.to_vec();
        for index in indices {
            if let Some(group) = self.groups.get(*index) {
                gone.extend(group.keys.iter().cloned());
            }
        }
        self.lanes.retain(|k| !gone.contains(k));
        self.ungroup(indices);
        self.tidy();
    }

    /// Mounts `key` right under the top lane (#1322). With `group` (Settings › Arrangement ›
    /// Automation › Group plug-in lanes), a device's lane goes into its device's group, made with
    /// the device's name when it has none, a number after it when another device has the name.
    pub fn mount(&mut self, key: &LaneKey, group: Option<&str>) {
        if self.lanes.contains(key) {
            return;
        }
        self.lanes.insert(0, key.clone());
        if let (Some(name), Some(device)) = (group, key.device) {
            match self.groups.iter_mut().find(|g| g.device == Some(device)) {
                Some(existing) => existing.keys.push(key.clone()),
                None => {
                    let mut unique = name.to_string();
                    let mut n = 2;
                    while self.groups.iter().any(|g| g.name == unique) {
                        unique = format!("{name} {n}");
                        n += 1;
                    }
                    self.groups.insert(
                        0,
                        LaneGroup { name: unique, keys: vec![key.clone()], folded: false, device: Some(device) },
                    );
                }
            }
        }
        self.tidy();
    }

    /// Keeps each group's lanes together in the place of the first of them, and drops groups
    /// left with no lane.
    pub fn tidy(&mut self) {
        let lanes = &mut self.lanes;
        for group in &mut self.groups {
            group.keys.retain(|k| lanes.contains(k));
        }
        self.groups.retain(|g| !g.keys.is_empty());
        for group in &self.groups {
            let Some(first) = lanes.iter().position(|k| group.keys.contains(k)) else { continue };
            let members: Vec<LaneKey> = lanes.iter().filter(|k| group.keys.contains(k)).cloned().collect();
            lanes.retain(|k| !group.keys.contains(k));
            let at = first.min(lanes.len());
            for (offset, key) in members.into_iter().enumerate() {
                lanes.insert((at + offset).min(lanes.len()), key);
            }
        }
    }

    /// The command that sets this layout as `track`'s.
    pub fn action(&self, track: TrackId) -> Action {
        let lane = |k: &LaneKey| match k.device {
            Some(device) => json!({ "device": device, "param": k.param }),
            None => json!({ "param": k.param }),
        };
        let groups: Vec<Value> = self
            .groups
            .iter()
            .map(|g| {
                let mut group =
                    json!({ "name": g.name, "lanes": g.keys.iter().map(lane).collect::<Vec<_>>(), "folded": g.folded });
                if let Some(device) = g.device {
                    group["device"] = json!(device);
                }
                group
            })
            .collect();
        Action::new(
            "automation.setLayout",
            json!({ "track": track.0, "lanes": self.lanes.iter().map(lane).collect::<Vec<_>>(), "groups": groups }),
        )
    }
}

/// Where dragged lanes go among a track's lanes (#1322).
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    /// Before this lane; `join` puts them in its group too.
    Before { key: LaneKey, join: bool },
    /// Before this group's heading, outside the group.
    BeforeGroup(usize),
    /// At the end of this (folded) group.
    Into(usize),
    /// After the last lane, in no group.
    End,
}

/// Lane headers being dragged to another place among their track's lanes (#1322): the track,
/// the lanes moved (in their order), whether they move as their group, and where the pointer is.
#[derive(Debug, Clone, PartialEq)]
struct HeaderDrag {
    track: TrackId,
    keys: Vec<LaneKey>,
    group: bool,
    pointer_y: f32,
}

/// How near the top or bottom of a lane's plot a point has to be to take the parameter's maximum
/// or minimum exactly (#811): one point of the plot is about 3 % of a default lane, and a point
/// a pixel short of the top looks the same as one on it.
const END_SNAP: f32 = 2.0;

/// `position` (0 … 1 on the lane), at 1 or 0 when it lies within [`END_SNAP`] of the plot's top
/// or bottom.
fn at_ends(plot: Rect, position: f64) -> f64 {
    let edge = if plot.height() > 0.0 { f64::from(END_SNAP / plot.height()) } else { 0.0 };
    if position >= 1.0 - edge {
        1.0
    } else if position <= edge {
        0.0
    } else {
        position
    }
}

/// Where a point drag is now from `pos`: the dragged point, which sat at `start` on the lane,
/// goes to the lane's end when it comes within [`END_SNAP`] of it ([`at_ends`]). With Shift,
/// only along the axis the pointer has moved further on, and up or down at a quarter of the
/// pointer's speed for a finer value, without the ends.
fn dragged_to(
    grab: (f64, f64),
    beat: f64,
    position: f64,
    start: f64,
    plot: Rect,
    geometry: &Geometry,
    shift: bool,
) -> (f64, f64) {
    if !shift {
        return (beat, grab.1 + at_ends(plot, start + position - grab.1) - start);
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
    track: TrackId,
    key: LaneKey,
    shown: f64,
    /// What the points are scaled around, a place 0 to 1 on the lane, from the line as it was
    /// before the scaling began.
    pivot: f64,
    /// A step of this scaling is in the history already: the next one replaces it, starting from
    /// the line as it was (`continue` with `replace`), so going back to 100 % puts it back whole.
    started: bool,
    /// When the slider was last hovered or dragged (the window's clock), for the dashed line at
    /// the pivot on the lane.
    shown_at: f64,
}

/// The middle Scale works around: half-way between the lowest and highest of the points from
/// `start` to `end`, as places on the lane.
fn middle_of(points: &[AutomationPoint], scale: &Scale, start: f64, end: f64) -> f64 {
    let places = points.iter().filter(|p| p.beat >= start && p.beat <= end).map(|p| scale.position(p.value));
    let (low, high) = places.fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
    if low <= high { (low + high) / 2.0 } else { 0.5 }
}

/// The Scale slider and what it scales around, for what `of` names on `track`'s `key` lane, the
/// points' `middle` given: while it is dragged or when it is let go of, the factor from the line as
/// it was (100 % for something new) and whether the step before is replaced.
fn stretch_slider(
    ui: &mut Ui,
    state: &mut LanesState,
    (track, key): (TrackId, &LaneKey),
    of: String,
    middle: f64,
) -> Option<(f64, bool)> {
    let kept = state.stretch.clone().filter(|s| s.of == of);
    let mut percent = kept.as_ref().map_or(1.0, |s| s.shown) * 100.0;
    let response = ui
        .add(egui::Slider::new(&mut percent, 0.0..=200.0).text("Scale").suffix(" %").fixed_decimals(0))
        .on_hover_text(
            "Pushes the points away from the dashed line (over 100 %) or pulls them toward it (under 100 %), \
             from the line as it was: back at 100 % it is as before. Their timing stays.",
        );
    // Around: the points' middle, or the bottom of the lane (a share of each value).
    let mut bottom = state.scale_bottom;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Around").color(theme::text_hint()).size(theme::TEXT_SMALL));
        ui.selectable_value(&mut bottom, false, "Middle")
            .on_hover_text("Around half-way between the lowest and highest point, so the shape swings more or less");
        ui.selectable_value(&mut bottom, true, "Bottom")
            .on_hover_text("Around the bottom of the lane: 50 % halves every value");
    });
    let changed_around = bottom != state.scale_bottom;
    state.scale_bottom = bottom;
    let pivot = if bottom { 0.0 } else { kept.as_ref().filter(|_| !changed_around).map_or(middle, |s| s.pivot) };
    let now = ui.input(|i| i.time);
    let shown_at =
        if response.hovered() || response.dragged() { now } else { kept.as_ref().map_or(0.0, |s| s.shown_at) };
    let started = kept.as_ref().is_some_and(|s| s.started);
    let shown = percent / 100.0;
    state.stretch = Some(Stretch { of, track, key: key.clone(), shown, pivot, started, shown_at });
    let due = live(ui, state, &response) || (changed_around && started);
    if !due {
        return None;
    }
    if let Some(stretch) = state.stretch.as_mut() {
        stretch.started = true;
    }
    Some((shown.clamp(0.0, 4.0), started))
}

/// The Smooth slider's amount kept for one range.
#[derive(Debug, Clone, PartialEq)]
struct Smoothed {
    of: String,
    amount: f64,
}

/// The Simplify slider kept for one range: how far it is, and the lane's points as they were
/// before it began, drawn faintly while the range's menu is open so the new line can be compared.
#[derive(Debug, Clone, PartialEq)]
struct Simplified {
    of: String,
    track: TrackId,
    key: LaneKey,
    amount: f64,
    original: Vec<AutomationPoint>,
    /// The points inside the range before, for the count the menu shows.
    before: usize,
}

/// The shapes' Rate: how long one cycle lasts, as a label and in beats (bars from the meter's
/// bar). 1/4 is a quarter note, one beat in 4/4.
const RATES: [(&str, Rate); 8] = [
    ("4 bars", Rate::Bars(4.0)),
    ("2 bars", Rate::Bars(2.0)),
    ("1 bar", Rate::Bars(1.0)),
    ("1/2", Rate::Beats(2.0)),
    ("1/4", Rate::Beats(1.0)),
    ("1/8", Rate::Beats(0.5)),
    ("1/16", Rate::Beats(0.25)),
    ("1/32", Rate::Beats(0.125)),
];
/// The Rate a shape takes until another is chosen: 1/4.
const DEFAULT_RATE: usize = 4;

/// One cycle's length: in bars of the meter, or in beats (quarter notes).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Rate {
    Bars(f64),
    Beats(f64),
}

impl Rate {
    fn beats(self, bar_beats: f64) -> f64 {
        match self {
            Self::Bars(bars) => bars * bar_beats,
            Self::Beats(beats) => beats,
        }
    }
}

/// How far from the old line Simplify at `amount` (0 to 1) lets the new one go, as a share of the
/// lane's height: nothing at 0, 4 % of the lane at 1, finer near 0 where small steps matter.
fn simplify_tolerance(amount: f64) -> f64 {
    let amount = amount.clamp(0.0, 1.0);
    0.04 * amount * amount
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
    /// The other lanes of the track the range was painted over, top to bottom (#1322): the
    /// edit keys act on them too, in one step.
    pub more: Vec<LaneKey>,
}

/// A range a duplicate moved onto its copy (#1322): where it was and where it went, so its undo
/// brings the range back and a redo takes it on again.
#[derive(Debug, Clone)]
struct RangeStep {
    before: LaneRange,
    after: LaneRange,
}

/// What the lanes keep between frames: which tracks show lanes and which, the selected points
/// and a drag in progress.
#[derive(Debug, Default)]
pub struct LanesState {
    open: HashSet<TrackId>,
    /// The lanes dragged to a height of their own; the rest are [`LANE`] high. Kept while a
    /// track's lanes are hidden, so they come back as tall as they were (#818).
    heights: HashMap<(TrackId, LaneKey), f32>,
    /// Each track's top lane: the parameter it shows (the one being worked on, #1322), `None`
    /// when it was cleared or its parameter was mounted; a track missing here shows
    /// [`Self::top_lane`]'s parameter when it is not mounted.
    active: HashMap<TrackId, Option<LaneKey>>,
    /// What a track's top lane shows before anything is chosen in it (Settings › Arrangement ›
    /// Automation › Top lane shows).
    pub top_lane: TopLane,
    selection: Option<Selection>,
    point_drag: Option<PointDrag>,
    curve_drag: Option<CurveDrag>,
    box_drag: Option<BoxDrag>,
    /// The time range painted last on a lane, which the edit keys act on.
    pub range: Option<LaneRange>,
    /// A duplicate's move of the range, until its command has run ([`LanesState::follow_history`]).
    range_moved: Option<(LaneRange, LaneRange)>,
    /// A lane made taller past the bottom of the lanes' area it scrolls in: that area, and how far
    /// its edge went past, for the view to scroll by on its next frame so the edge stays in view.
    follow_resize: Option<(Rect, f32)>,
    /// The range's moves an undo can take back, and those a redo can make again, newest last.
    /// One for each of the session's undo steps, oldest first, and each of its redo steps, newest
    /// last: the range's move a step made, if it made one.
    range_undo: Vec<Option<RangeStep>>,
    range_redo: Vec<Option<RangeStep>>,
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
    /// The range menu's Depth (0 to 1; `None` for the whole travel) and Rate (an index into
    /// [`RATES`]; `None` for 1/4), the window's choices.
    shape_depth: Option<f64>,
    shape_rate: Option<usize>,
    /// Scale as the menu shows it for what it scales (a range or picked points, by a key): the
    /// value shown and the value the points are at now, from where the scaling began.
    stretch: Option<Stretch>,
    /// Scale works around the bottom of the lane, not the points' middle (the window's choice).
    scale_bottom: bool,
    /// Points moved together, picked again once the move is in: their lane, the beats they went
    /// to, and how many frames are left to wait for the edit.
    reselect: Option<(TrackId, LaneKey, Vec<f64>, u8)>,
    /// A point drag let go of this frame: still drawn where it went, as its command runs only
    /// after the frame (else the line flashes back to where the drag began for a frame).
    dropped: Option<PointDrag>,
    /// Smooth as the range menu shows it, and the smoothing it put in last.
    smooth: Option<Smoothed>,
    /// Simplify as the range menu shows it, with the line from before it.
    simplify: Option<Simplified>,
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
    /// The lane a drag held when the view was last drawn, with its points where the drag put
    /// them, for the headers and panels drawn before the lanes in the next frame.
    pub dragged: Option<Dragged>,
    /// The same, as the lanes draw it this frame.
    drawn_drag: Option<Dragged>,
    /// The lanes picked by their headers (a click, Ctrl+click, Shift+click), on one track (#1322).
    pub picked: Vec<(TrackId, LaneKey)>,
    /// The lanes mounted on each track never laid out, as they were when its lanes were opened.
    implied: HashMap<TrackId, Layout>,
    /// The group headings picked (a click, Ctrl+click), by track and index (#1322).
    pub picked_groups: Vec<(TrackId, usize)>,
    /// Each track's layout as it was when its groups were picked: an index stands for the same
    /// group only while the layout stays as it was ([`LanesState::keep_picks`]).
    picked_layouts: Vec<(TrackId, Layout)>,
    /// A lane mounted from a device goes into the device's group (Settings › Arrangement ›
    /// Automation › Group plug-in lanes, #1322).
    pub group_plugins: bool,
    /// Lane headers being dragged to another place (#1322).
    header_drag: Option<HeaderDrag>,
    /// A group heading whose name is being typed: its track, its index and the text.
    renaming: Option<(TrackId, usize, String)>,
    /// The lanes a range being painted reaches over, on its track, top to bottom (#1322).
    covered: Vec<LaneKey>,
    /// The lanes of the track being drawn that the range (painted or being painted) is on, top to
    /// bottom: it is drawn as one area over them, with no gaps between the lanes.
    range_lanes: Vec<LaneKey>,
    /// Where a range being painted began, once it was dragged out of its track's lanes: the
    /// Arrangement view goes on with it as a time selection ([`LanesState::take_time_from`]).
    time_from: Option<Pos2>,
}

impl LanesState {
    /// Takes the lane a drag holds as the lanes drew it this frame: `None` without a drag, or
    /// on a lane that is off.
    pub fn take_drawn_drag(&mut self) -> Option<Dragged> {
        self.drawn_drag.take()
    }

    /// Whether `track` shows its automation lanes.
    pub fn is_open(&self, track: TrackId) -> bool {
        self.open.contains(&track)
    }

    /// `track`'s lane layout: the project's; for a track never laid out, the lanes that had
    /// points when its lanes were opened, so drawing on the top lane does not move it (#1322).
    pub fn layout(&self, track: TrackId, params: &Params) -> Layout {
        if params.layouts.contains_key(&track) {
            return params.layout(track);
        }
        self.implied.get(&track).cloned().unwrap_or_else(|| params.layout(track))
    }

    /// The lanes mounted on `track` that `params` still has, top to bottom (#1322).
    pub fn mounted(&self, track: TrackId, params: &Params) -> Vec<LaneKey> {
        self.layout(track, params).lanes.into_iter().filter(|k| params.param(track, k).is_some()).collect()
    }

    /// The parameter `track`'s top lane shows (#1322): the one chosen or moved last while it is
    /// not mounted; for a track whose top lane was never chosen, cleared or mounted,
    /// [`Self::top_lane`]'s parameter when it is not mounted; else `None`, the empty top lane.
    pub fn active(&self, track: TrackId, params: &Params) -> Option<LaneKey> {
        let mounted = self.mounted(track, params);
        let shows = |key: &LaneKey| params.param(track, key).is_some() && !mounted.contains(key);
        match self.active.get(&track) {
            Some(Some(key)) if shows(key) => Some(key.clone()),
            Some(_) => None,
            None => self.top_lane.key().filter(shows),
        }
    }

    /// The lanes `track` shows, top to bottom: its top lane's parameter, then its mounted lanes.
    pub fn shown(&self, track: TrackId, params: &Params) -> Vec<LaneKey> {
        if !self.is_open(track) {
            return Vec::new();
        }
        let mut shown: Vec<LaneKey> = self.active(track, params).into_iter().collect();
        shown.extend(self.mounted(track, params));
        shown
    }

    /// How much taller `track`'s row is for its lanes.
    pub fn height(&self, track: TrackId, params: &Params) -> f32 {
        self.rows(track, params).iter().map(|row| self.row_height(track, row) + LINE).sum()
    }

    /// The height of one of `track`'s rows of lanes.
    pub(crate) fn row_height(&self, track: TrackId, row: &LaneRow) -> f32 {
        match row {
            LaneRow::Lane(key) => self.lane_height(track, key),
            LaneRow::Group(_) => GROUP_ROW,
            LaneRow::Empty => LANE,
        }
    }

    /// `track`'s rows of lanes from the top, as they are drawn (#1322): the top lane (its
    /// parameter, or the empty row), then each mounted lane, a group's heading before its lanes,
    /// which are left out while it is folded.
    pub fn rows(&self, track: TrackId, params: &Params) -> Vec<LaneRow> {
        if !self.is_open(track) {
            return Vec::new();
        }
        let mut rows = vec![self.active(track, params).map_or(LaneRow::Empty, LaneRow::Lane)];
        let layout = self.layout(track, params);
        let mounted = self.mounted(track, params);
        let mut headed: Vec<usize> = Vec::new();
        for key in &mounted {
            match layout.groups.iter().position(|g| g.keys.contains(key)) {
                Some(index) => {
                    if headed.contains(&index) {
                        continue;
                    }
                    headed.push(index);
                    rows.push(LaneRow::Group(index));
                    if let Some(group) = layout.groups.get(index).filter(|g| !g.folded) {
                        rows.extend(mounted.iter().filter(|k| group.keys.contains(k)).cloned().map(LaneRow::Lane));
                    }
                }
                None => rows.push(LaneRow::Lane(key.clone())),
            }
        }
        rows
    }

    /// Picks `track`'s mounted lane `key` by its header (#1322): alone with a plain click, added
    /// or taken away with Ctrl (Cmd on macOS), and every mounted lane from the one picked last to
    /// it with Shift. Picks of another track are let go of.
    pub fn pick(&mut self, track: TrackId, key: &LaneKey, modifiers: egui::Modifiers, params: &Params) {
        self.picked.retain(|(t, _)| *t == track);
        self.picked_groups.retain(|(t, _)| *t == track);
        let this = (track, key.clone());
        if modifiers.command {
            match self.picked.iter().position(|p| *p == this) {
                Some(at) => {
                    self.picked.remove(at);
                }
                None => self.picked.push(this),
            }
        } else if modifiers.shift
            && let Some((_, anchor)) = self.picked.last().cloned()
        {
            let mounted = self.mounted(track, params);
            let (Some(a), Some(b)) = (mounted.iter().position(|k| *k == anchor), mounted.iter().position(|k| k == key))
            else {
                self.picked = vec![this];
                return;
            };
            for k in mounted.get(a.min(b)..=a.max(b)).unwrap_or_default() {
                let pick = (track, k.clone());
                if !self.picked.contains(&pick) {
                    self.picked.push(pick);
                }
            }
        } else {
            self.picked = vec![this];
            self.picked_groups.clear();
        }
    }

    /// Picks `track`'s group heading `index` (#1322), its layout `layout` as it is now: alone, or
    /// added or taken away with Ctrl.
    pub fn pick_group(&mut self, track: TrackId, index: usize, modifiers: egui::Modifiers, layout: &Layout) {
        self.picked.retain(|(t, _)| *t == track);
        self.picked_groups.retain(|(t, _)| *t == track);
        self.picked_layouts = vec![(track, layout.clone())];
        let this = (track, index);
        if modifiers.command || modifiers.shift {
            match self.picked_groups.iter().position(|p| *p == this) {
                Some(at) => {
                    self.picked_groups.remove(at);
                }
                None => self.picked_groups.push(this),
            }
        } else {
            self.picked_groups = vec![this];
            self.picked.clear();
        }
    }

    /// Lets go of `track`'s picked group headings when its layout `now` is not the one they were
    /// picked in (an undo, a redo, an agent's command): their indices may stand for other groups
    /// now, and Delete or Ungroup would act on those (#1322). A group folded or opened is the same.
    pub fn keep_picks(&mut self, track: TrackId, now: &Layout) {
        let same = |a: &Layout| {
            a.groups.len() == now.groups.len()
                && a.groups.iter().zip(&now.groups).all(|(x, y)| x.name == y.name && x.keys == y.keys)
        };
        let changed = self.picked_layouts.iter().any(|(t, layout)| *t == track && !same(layout));
        if changed {
            self.picked_groups.retain(|(t, _)| *t != track);
            self.picked_layouts.retain(|(t, _)| *t != track);
        }
    }

    /// Whether `track`'s lane `key` is picked.
    pub fn is_picked(&self, track: TrackId, key: &LaneKey) -> bool {
        self.picked.iter().any(|(t, k)| *t == track && k == key)
    }

    /// Whether anything is picked by its header: lanes or group headings.
    pub fn has_picks(&self) -> bool {
        !self.picked.is_empty() || !self.picked_groups.is_empty()
    }

    /// The mounted lanes a drag of `key`'s header moves: the picked lanes of the track when it is
    /// one of them, in the track's order; else the lane alone.
    fn dragged_with(&self, track: TrackId, key: &LaneKey, params: &Params) -> Vec<LaneKey> {
        if !self.is_picked(track, key) {
            return vec![key.clone()];
        }
        self.mounted(track, params).into_iter().filter(|k| self.is_picked(track, k)).collect()
    }

    /// The command that takes the picked lanes and group headings (with their lanes) out of
    /// their track's layout (Delete, #1322); `None` with nothing picked.
    pub fn delete_picked(&mut self, params: &Params) -> Option<Action> {
        let track = self.picked.first().map(|(t, _)| *t).or_else(|| self.picked_groups.first().map(|(t, _)| *t))?;
        let keys: Vec<LaneKey> = self.picked.iter().filter(|(t, _)| *t == track).map(|(_, k)| k.clone()).collect();
        let groups: Vec<usize> = self.picked_groups.iter().filter(|(t, _)| *t == track).map(|(_, g)| *g).collect();
        let mut layout = self.layout(track, params);
        layout.remove(&keys, &groups);
        self.picked.clear();
        self.picked_groups.clear();
        Some(layout.action(track))
    }

    /// Mounts `track`'s top lane's parameter under it (the top lane's **+**, #1322): the top lane
    /// is then empty until a parameter is chosen in it or a control is moved. With
    /// [`Self::group_plugins`], a hosted plug-in's lane goes into its device's group.
    pub fn mount(&mut self, track: TrackId, key: &LaneKey, params: &Params) -> Action {
        let mut layout = self.layout(track, params);
        let device_name = params
            .param(track, key)
            .filter(|p| self.group_plugins && matches!(p.scale, Scale::Plugin(_)))
            .and_then(|p| p.device_name.clone());
        layout.mount(key, device_name.as_deref());
        self.active.insert(track, None);
        self.last_lane = Some((track, key.clone()));
        layout.action(track)
    }

    /// Clears `track`'s top lane (its **×**, #1322): it stays, empty, until a parameter is
    /// chosen in it or a control is moved.
    pub fn clear_active(&mut self, track: TrackId) {
        self.active.insert(track, None);
        if self.range.as_ref().is_some_and(|r| r.track == track) {
            self.range = None;
        }
    }

    /// Shows `key` in `track`'s top lane (chosen in its dropdown), keeping the lane's height.
    pub fn set_active(&mut self, track: TrackId, key: LaneKey, params: &Params) {
        if let Some(old) = self.active(track, params) {
            self.carry_height(track, &old, &key);
        }
        self.active.insert(track, Some(key));
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

    /// Every shown lane of every open track `factor` times as tall, within the lanes' limits, as
    /// Alt and the wheel make the tracks (#957).
    pub fn zoom_all(&mut self, factor: f32, params: &Params) {
        let shown: Vec<(TrackId, LaneKey)> = self
            .open
            .iter()
            .flat_map(|track| self.shown(*track, params).into_iter().map(move |k| (*track, k)))
            .collect();
        for (track, key) in shown {
            let height = self.lane_height(track, &key) * factor;
            self.set_lane_height(track, &key, height);
        }
    }

    /// Shows or hides `track`'s lanes: the top lane, then the lanes mounted under it (#1322).
    /// Opened, a track never laid out keeps the lanes that have points now as its mounted ones,
    /// and the top lane its parameter, until the layout is changed.
    pub fn toggle(&mut self, track: TrackId, params: &Params) {
        if !self.open.remove(&track) {
            self.open.insert(track);
            if !params.layouts.contains_key(&track) {
                self.implied.insert(track, params.layout(track));
            }
            if !self.active.contains_key(&track) {
                let active = self.active(track, params);
                self.active.insert(track, active);
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

    /// A parameter of `track` moved by hand (a fader, a knob, a plug-in's own editor) shows in the
    /// track's top lane, ready to be drawn on, when its lanes are shown (#1205, #1322); one that is
    /// mounted is edited in its own lane, and the top lane stays as it is. It becomes the lane
    /// clicked last, which Ctrl+V pastes onto. Closed lanes stay closed.
    pub fn focus(&mut self, track: TrackId, key: LaneKey, params: &Params) {
        if !self.is_open(track) {
            return;
        }
        if !self.mounted(track, params).contains(&key)
            && let Some(old) = self.active(track, params).filter(|old| *old != key)
        {
            self.carry_height(track, &old, &key);
            if self.selection.as_ref().is_some_and(|s| s.track == track && s.key == old) {
                self.selection = None;
            }
            if self.range.as_ref().is_some_and(|r| r.track == track && r.key == old) {
                self.range = None;
            }
            self.from_automated.remove(&(track, old));
            self.active.insert(track, Some(key.clone()));
        } else if !self.mounted(track, params).contains(&key) {
            self.active.insert(track, Some(key.clone()));
        }
        self.last_lane = Some((track, key));
    }

    /// A lane of `track` that showed `from` and shows `to` now keeps its height (#1205): the
    /// height goes with the lane, not with the parameter it happens to show.
    fn carry_height(&mut self, track: TrackId, from: &LaneKey, to: &LaneKey) {
        let height = self.lane_height(track, from);
        self.set_lane_height(track, to, height);
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
            self.picked.clear();
            self.picked_groups.clear();
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
        // The other lanes it was painted over, in the same step (#1322).
        if !range.more.is_empty() {
            let lanes: Vec<Value> = range
                .more
                .iter()
                .map(|k| match k.device {
                    Some(device) => json!({ "device": device, "param": k.param }),
                    None => json!({ "param": k.param }),
                })
                .collect();
            params["lanes"] = json!(lanes);
        }
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
                let moved = LaneRange { start: range.end, end: range.end + length, ..range.clone() };
                self.range = Some(moved.clone());
                self.range_moved = Some((range, moved));
                vec![action("automation.duplicate", &params)]
            }
            "paste" => {
                // Copied lanes go back on their own lanes; this lane says the track and the start.
                if let Some(map) = params.as_object_mut() {
                    map.remove("end");
                    map.remove("lanes");
                }
                self.selection = None;
                vec![action("automation.paste", &params)]
            }
            _ => Vec::new(),
        }
    }

    /// A command ran, the session's undo steps `before` and `after` it (how many an undo can go
    /// back, how many were ever made, [`suhina_session::Session::undo_steps`]): the lanes keep a
    /// step beside each of the session's, a duplicate's move of the range in its own. An undo of
    /// that step puts the range back where it was, a redo onto the copy again (#1322), as long as
    /// the range is still where the step left it.
    pub fn follow_history(&mut self, command: &str, before: (usize, u64), after: (usize, u64)) {
        let moved = self.range_moved.take();
        // Steps of another history (another project's) or made without the lanes knowing: none of
        // them moved the range.
        if self.range_undo.len() != before.0 {
            self.range_undo = vec![None; before.0];
            self.range_redo.clear();
        }
        match command {
            "edit.undo" if after.0 < before.0 => {
                let step = self.range_undo.pop().flatten();
                if let Some(step) = &step
                    && self.range.as_ref() == Some(&step.after)
                {
                    self.range = Some(step.before.clone());
                }
                self.range_redo.push(step);
            }
            "edit.redo" if after.0 > before.0 => {
                let step = self.range_redo.pop().flatten();
                if let Some(step) = &step
                    && self.range.as_ref() == Some(&step.before)
                {
                    self.range = Some(step.after.clone());
                }
                self.range_undo.push(step);
            }
            _ if after.1 != before.1 => {
                // A new step: the oldest went if the depth did not grow (the limit).
                self.range_redo.clear();
                while !self.range_undo.is_empty() && self.range_undo.len() >= after.0 {
                    self.range_undo.remove(0);
                }
                let step = moved
                    .filter(|_| command == "automation.duplicate")
                    .map(|(before, after)| RangeStep { before, after });
                self.range_undo.push(step);
            }
            _ => {}
        }
        if self.range_undo.len() != after.0 {
            self.range_undo = vec![None; after.0];
            self.range_redo.clear();
        }
    }

    /// Where a range painted on a lane began, when it was dragged out of its track's lanes this
    /// frame: the view goes on with the drag as a time selection over the tracks (#1322).
    pub fn take_time_from(&mut self) -> Option<Pos2> {
        self.time_from.take()
    }

    /// How far the view that scrolls `area` moves down so a lane made taller past its bottom stays
    /// in sight (`0` without one); asked once a frame.
    pub fn take_follow_resize(&mut self, area: Rect) -> f32 {
        match self.follow_resize.take() {
            Some((asked, past)) if asked == area => past,
            _ => 0.0,
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
        self.axis().x_of(beat)
    }

    /// The beat at `x` on screen: the inverse of [`Geometry::x_of`].
    fn beat_of(&self, x: f32) -> f64 {
        self.axis().beat_at(x)
    }

    fn axis(&self) -> crate::kit::time_axis::TimeViewport {
        crate::kit::time_axis::TimeViewport::scrolled(self.time.min, self.scroll_x, self.beat_width)
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

/// The lane position at height `y` in `plot`, not held in the lane: 1.2 is a fifth of the plot
/// above its top. A point drag measures the pointer with it, so a point taken hold of off its
/// centre still reaches the lane's ends when the pointer goes past them (#811).
fn place_of(plot: Rect, y: f32) -> f64 {
    if plot.height() > 0.0 { f64::from((plot.bottom() - y) / plot.height()) } else { 0.0 }
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
    let response = widgets::button_area(ui, rect, Id::unique(("automation-toggle", track.0)), label);
    if response.clicked() {
        state.toggle(track, params);
    }
    // `#control-automation`, `[checked]` while the lanes show, and `automation` artwork.
    let el = crate::kit::element::BUTTON
        .id("automation")
        .response(&response)
        .attr(crate::kit::element::Checked, open)
        .enter();
    let hover = widgets::hover(ui, &response);
    let off = crate::kit::motion::mix(theme::control_background(), theme::control_hover(), hover);
    widgets::track_button_face(ui, painter, rect, &el, &response, if open { theme::control_active() } else { off });
    let ink = el.color_or(if open { theme::text_primary() } else { theme::text_secondary() });
    if crate::kit::skin::paint_as(
        &el,
        painter,
        rect,
        "automation",
        crate::kit::skin::State::response(&response, open),
        ink,
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
    // Group headings picked in another layout than the track's now are let go of.
    let layout = state.layout(lanes.track, params);
    state.keep_picks(lanes.track, &layout);
    // A range painted from one of the track's lanes reaches the lanes the pointer went over,
    // up or down (#1322): worked out before the lanes are drawn, so each shows it this frame.
    // Dragged out of the track's lanes, onto its clips or another track, the range becomes a time
    // selection over the tracks, as one painted on the clips would be (#1322).
    if let Some(drag) = state.box_drag.clone().filter(|d| !d.select && d.track == lanes.track) {
        let bottom = lanes.top + state.height(lanes.track, params);
        if drag.to.y < lanes.top || drag.to.y > bottom {
            state.box_drag = None;
            state.covered.clear();
            state.range = None;
            state.time_from = Some(drag.from);
        }
    }
    if let Some(drag) = state.box_drag.clone().filter(|d| !d.select && d.track == lanes.track) {
        let (low, high) = (drag.from.y.min(drag.to.y), drag.from.y.max(drag.to.y));
        let mut top = lanes.top;
        let mut covered = Vec::new();
        for row in state.rows(lanes.track, params) {
            let height = state.row_height(lanes.track, &row);
            if let LaneRow::Lane(key) = row
                && top <= high
                && top + height >= low
            {
                covered.push(key);
            }
            top += height + LINE;
        }
        state.covered = covered;
    }
    let painting = state.box_drag.as_ref().filter(|d| !d.select && d.track == lanes.track);
    let on = |key: &LaneKey| match painting {
        Some(drag) => &drag.key == key || state.covered.contains(key),
        None => state.range.as_ref().is_some_and(|r| r.track == lanes.track && (&r.key == key || r.more.contains(key))),
    };
    let range_lanes: Vec<LaneKey> = state
        .rows(lanes.track, params)
        .into_iter()
        .filter_map(|row| match row {
            LaneRow::Lane(key) if on(&key) => Some(key),
            _ => None,
        })
        .collect();
    state.range_lanes = range_lanes;
    let mut y = lanes.top;
    // Where each row went, for a drag of headers to land between them.
    let mut placed_rows: Vec<(LaneRow, f32, f32)> = Vec::new();
    for row in state.rows(lanes.track, params) {
        let height = state.row_height(lanes.track, &row);
        let time = Rect::from_x_y_ranges(geometry.time, egui::Rangef::new(y, y + height));
        let header = Rect::from_x_y_ranges(geometry.headers, time.y_range());
        y += height + LINE;
        placed_rows.push((row.clone(), time.top(), time.bottom()));
        let full = Rect::from_x_y_ranges(geometry.visible.x_range(), time.y_range());
        if full.bottom() < geometry.visible.top() || full.top() > geometry.visible.bottom() {
            continue;
        }
        match row {
            LaneRow::Group(index) => {
                group_heading(ui, painter, geometry, lanes, index, header, time, state, params, actions);
                painter.hline(geometry.visible.x_range(), time.bottom() + 0.5, theme::hairline(theme::border()));
            }
            LaneRow::Empty => {
                empty_lane(ui, painter, geometry, lanes, header, time, state, params);
                painter.hline(geometry.visible.x_range(), time.bottom() + 0.5, theme::hairline(theme::border()));
            }
            LaneRow::Lane(key) => {
                let Some(param) = params.param(lanes.track, &key) else { continue };
                lane(ui, painter, geometry, lanes, param, time, state, actions);
                lane_header(ui, painter, geometry, lanes, param, header, state, params, actions);
                painter.hline(geometry.visible.x_range(), time.bottom() + 0.5, theme::hairline(theme::border()));
                resize_edge(ui, geometry, lanes, &key, full, state);
            }
        }
    }
    drop_headers(ui, painter, geometry, lanes.track, &placed_rows, state, params, actions);
}

/// The top lane with no parameter (#1322): a dropdown to choose one, and a hint over the lane.
#[allow(clippy::too_many_arguments)]
fn empty_lane(
    ui: &mut Ui,
    painter: &egui::Painter,
    geometry: &Geometry,
    lanes: &TrackLanes<'_>,
    header: Rect,
    time: Rect,
    state: &mut LanesState,
    params: &Params,
) {
    let track = lanes.track;
    let visible_time = time.intersect(geometry.visible);
    if visible_time.width() > 0.0 && visible_time.height() > 0.0 {
        widgets::text_in(
            &painter.with_clip_rect(visible_time),
            visible_time,
            Align2::CENTER_CENTER,
            "Choose a parameter, or move a control, to show it here",
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
    }
    let clip = header.intersect(geometry.visible);
    if clip.width() <= 0.0 || clip.height() <= 0.0 {
        return;
    }
    let painter = painter.with_clip_rect(clip);
    painter.rect_filled(header, 0.0, crate::kit::style_class::surface_or(theme::panel_background()));
    let inner = header.shrink2(Vec2::new(PAD, 5.0));
    let row = Rect::from_min_size(inner.min, Vec2::new(inner.width(), HEADER_ROW));
    let id = Id::unique(("automation-empty", track.0));
    let response = widgets::button_area(
        ui,
        row.intersect(clip),
        id.with("param"),
        format!("Choose a parameter on {}", lanes.name),
    );
    widgets::dropdown(ui, &painter, row, &response, "Choose a parameter");
    if let Some(choice) = parameter_dropdown(ui, &response, id, track, None, state, params) {
        state.set_active(track, choice, params);
    }
}

/// Where a drag of lane headers on `track` lands, drawn as a line, and the move once the button
/// comes up (#1322): before the row whose middle is under the pointer, or after the last.
#[allow(clippy::too_many_arguments)]
fn drop_headers(
    ui: &Ui,
    painter: &egui::Painter,
    geometry: &Geometry,
    track: TrackId,
    placed_rows: &[(LaneRow, f32, f32)],
    state: &mut LanesState,
    params: &Params,
    actions: &mut Vec<Action>,
) {
    let Some(drag) = state.header_drag.clone().filter(|d| d.track == track) else { return };
    let layout = state.layout(track, params);
    let across =
        egui::Rangef::new(geometry.headers.min.min(geometry.time.min), geometry.headers.max.max(geometry.time.max));
    // Over a folded group's heading, lanes go into the group: the heading lights up, as a place
    // something is dropped into does (#1322).
    let into = (!drag.group)
        .then(|| {
            placed_rows.iter().find_map(|(row, top, bottom)| match row {
                LaneRow::Group(index)
                    if layout.groups.get(*index).is_some_and(|g| g.folded)
                        && (*top..=*bottom).contains(&drag.pointer_y) =>
                {
                    Some((*index, *top, *bottom))
                }
                _ => None,
            })
        })
        .flatten();
    // Else before the row whose middle is under the pointer, under the top lane at least: before
    // a group's heading is outside the group, before one of its lanes is in it.
    let landing = placed_rows.iter().skip(1).find(|(_, top, bottom)| drag.pointer_y < (top + bottom) / 2.0);
    let place = match (into, landing) {
        (Some((index, _, _)), _) => Place::Into(index),
        (None, Some((LaneRow::Group(index), _, _))) => Place::BeforeGroup(*index),
        (None, Some((LaneRow::Lane(key), _, _))) => {
            Place::Before { key: key.clone(), join: layout.groups.iter().any(|g| g.keys.contains(key)) }
        }
        (None, _) => Place::End,
    };
    match into {
        Some((_, top, bottom)) => {
            let area = Rect::from_x_y_ranges(across, egui::Rangef::new(top, bottom));
            painter.rect_filled(area, 0.0, theme::accent().gamma_multiply(0.2));
            painter.rect_stroke(area, 0.0, Stroke::new(2.0, theme::accent()), StrokeKind::Inside);
        }
        None => {
            let line_y = match landing {
                Some((_, top, _)) => *top - LINE / 2.0,
                None => placed_rows.last().map_or(geometry.visible.top(), |(_, _, bottom)| *bottom + LINE / 2.0),
            };
            // A line that lands in a group starts further in, under the group's name, so where
            // the lanes go (in the group or out of it) shows before they are let go of.
            let joins = !drag.group && matches!(place, Place::Before { join: true, .. });
            let line = if joins { egui::Rangef::new(across.min + GROUP_INDENT, across.max) } else { across };
            painter.hline(line, line_y, Stroke::new(2.0, theme::accent()));
        }
    }
    if ui.input(|i| i.pointer.any_down()) {
        ui.ctx().request_repaint();
        return;
    }
    state.header_drag = None;
    let mut moved = layout.clone();
    // The top lane's parameter dragged down is mounted where it lands.
    let mut mounting = false;
    for key in &drag.keys {
        if !moved.lanes.contains(key) {
            moved.lanes.push(key.clone());
            mounting = true;
        }
    }
    moved.move_lanes(&drag.keys, &place, drag.group);
    if mounting {
        state.active.insert(track, None);
    }
    if moved != layout {
        actions.push(moved.action(track));
    }
}

/// A group's heading among `lanes`' rows (#1322): its fold arrow, its name (a double-click types
/// a new one) and how many lanes it holds; a click picks it (Ctrl adds), a drag moves its lanes,
/// its menu ungroups the picked groups. Each change is a command on the track's layout.
#[allow(clippy::too_many_arguments)]
fn group_heading(
    ui: &mut Ui,
    painter: &egui::Painter,
    geometry: &Geometry,
    lanes: &TrackLanes<'_>,
    index: usize,
    header: Rect,
    time: Rect,
    state: &mut LanesState,
    params: &Params,
    actions: &mut Vec<Action>,
) {
    let track = lanes.track;
    let layout = state.layout(track, params);
    let Some(group) = layout.groups.get(index).cloned() else { return };
    let clip = header.intersect(geometry.visible);
    if clip.width() <= 0.0 || clip.height() <= 0.0 {
        return;
    }
    let painter = painter.with_clip_rect(geometry.visible);
    painter.rect_filled(header, 0.0, theme::control_background());
    if state.picked_groups.contains(&(track, index)) {
        painter.rect_filled(header, 0.0, theme::accent().gamma_multiply(0.25));
    }
    painter.rect_filled(time.intersect(geometry.visible), 0.0, theme::accent().gamma_multiply(0.06));
    let id = Id::unique(("automation-group", track.0, index));
    let inner = header.shrink2(Vec2::new(PAD, 2.0));
    let arrow = Rect::from_min_size(inner.min, Vec2::new(14.0, inner.height()));
    let name = Rect::from_min_max(Pos2::new(arrow.right() + 4.0, inner.top()), inner.max);
    let background =
        ui.interact(clip, id.with("background"), Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::Grab);
    if background.clicked() {
        let modifiers = ui.input(|i| i.modifiers);
        state.pick_group(track, index, modifiers, &layout);
    }
    if background.drag_started_by(egui::PointerButton::Primary) {
        let keys: Vec<LaneKey> = layout.lanes.iter().filter(|k| group.keys.contains(k)).cloned().collect();
        state.header_drag = Some(HeaderDrag { track, keys, group: true, pointer_y: header.center().y });
    }
    if let Some(pos) = background.interact_pointer_pos()
        && let Some(drag) = state.header_drag.as_mut().filter(|d| d.track == track && d.group)
    {
        drag.pointer_y = pos.y;
    }
    if background.double_clicked() {
        state.renaming = Some((track, index, group.name.clone()));
    }
    background.context_menu(|ui| {
        // The picked groups, this one among them; or this one alone.
        let mut picked: Vec<usize> = state.picked_groups.iter().filter(|(t, _)| *t == track).map(|(_, g)| *g).collect();
        if !picked.contains(&index) {
            picked = vec![index];
        }
        if ui.button("Rename").clicked() {
            state.renaming = Some((track, index, group.name.clone()));
            ui.close();
        }
        let ungroup = if picked.len() > 1 { format!("Ungroup {} groups", picked.len()) } else { "Ungroup".into() };
        if ui.button(ungroup).clicked() {
            let mut changed = layout.clone();
            changed.ungroup(&picked);
            actions.push(changed.action(track));
            state.picked_groups.clear();
            ui.close();
        }
        let delete = if picked.len() > 1 { format!("Delete {} groups", picked.len()) } else { "Delete group".into() };
        if ui.button(delete).clicked() {
            let mut changed = layout.clone();
            changed.remove(&[], &picked);
            actions.push(changed.action(track));
            state.picked_groups.clear();
            ui.close();
        }
    });
    let fold = widgets::button_area(
        ui,
        arrow.intersect(clip),
        id.with("fold"),
        format!("{} {} on {}", if group.folded { "Unfold" } else { "Fold" }, group.name, lanes.name),
    );
    if fold.clicked() {
        let mut changed = layout.clone();
        if let Some(g) = changed.groups.get_mut(index) {
            g.folded = !g.folded;
        }
        actions.push(changed.action(track));
    }
    widgets::text_in(
        &painter,
        arrow,
        Align2::CENTER_CENTER,
        if group.folded { "▸" } else { "▾" },
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_secondary(),
    );
    match state.renaming.clone().filter(|(t, i, _)| *t == track && *i == index) {
        Some((_, _, mut text)) => {
            let edit = ui.put(
                name,
                egui::TextEdit::singleline(&mut text)
                    .font(theme::ui_font(theme::TEXT_SMALL))
                    .desired_width(name.width()),
            );
            edit.request_focus();
            let done = edit.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter));
            let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if cancel {
                state.renaming = None;
            } else if done {
                let text = text.trim().to_string();
                if !text.is_empty() && text != group.name {
                    let mut changed = layout.clone();
                    if let Some(g) = changed.groups.get_mut(index) {
                        g.name = text;
                    }
                    actions.push(changed.action(track));
                }
                state.renaming = None;
            } else {
                state.renaming = Some((track, index, text));
            }
        }
        None => {
            let count = group.keys.len();
            let text = format!("{}  ·  {count} {}", group.name, if count == 1 { "lane" } else { "lanes" });
            widgets::text_in(
                &painter,
                name,
                Align2::LEFT_CENTER,
                &text,
                theme::ui_font(theme::TEXT_SMALL),
                theme::text_primary(),
            );
        }
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
    let id = Id::unique(("automation-resize", lanes.track.0, key.device, key.param.as_str()));
    let response = ui.interact(edge, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeVertical);
    if response.double_clicked() {
        state.set_lane_height(lanes.track, key, LANE);
    } else if response.dragged_by(egui::PointerButton::Primary) {
        let before = state.lane_height(lanes.track, key);
        state.set_lane_height(lanes.track, key, before + response.drag_delta().y);
        // Past the bottom of the area, the view follows the edge: the whole lane stays in sight,
        // and the edge stays under the pointer to be dragged back up.
        let bottom = full.bottom() + state.lane_height(lanes.track, key) - before + LINE + 2.0;
        let past = bottom - geometry.visible.bottom();
        if past > 0.0 {
            state.follow_resize = Some((geometry.visible, past));
        }
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
    let held = state.point_drag.as_ref().or(state.dropped.as_ref());
    if let Some(drag) = held.filter(|d| d.track == track && d.key == param.key) {
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

/// What the point a drag holds will be when it is let go of: "50R at beat 25.25".
fn dragged_text(param: &Param, drag: &PointDrag, geometry: &Geometry, free: bool) -> Option<String> {
    let point = param.points.get(drag.point)?;
    let (beats, delta) = drag_delta(drag, param, geometry, free);
    let value = moved_value(param, drag, point.value, delta);
    Some(format!("{} at beat {}", param.scale.text(param.scale.committed(value)), round(point.beat + beats, 1000.0)))
}

/// Where the point a drag holds sat on the lane before the drag.
fn grabbed_at(param: &Param, drag: &PointDrag) -> f64 {
    param.points.get(drag.point).map_or(drag.grab.1, |p| param.scale.position(p.value))
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
    let id = Id::unique(("automation-lane", track.0, key.device, key.param.as_str()));
    let label = param.label();

    // The lane itself: a click adds a point (on the envelope when near it, so the envelope keeps
    // its shape; where the pointer is otherwise), a drag near the line bends its segment, any
    // other drag draws a box that selects the points in it.
    let body = ui.interact(visible, id, Sense::click_and_drag());
    body.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Unknown, true, format!("{label} automation on {}", lanes.name))
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
        let grab = (geometry.beat_of(pos.x), place_of(plot, pos.y));
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
            // where the pointer is, as off the line. Next to the top or bottom it takes the end.
            if !near {
                params["value"] = param.scale.command(param.scale.value_at(at_ends(plot, position_of(plot, pos.y))));
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
        // A click on a lane lets go of the headers picked, so Delete acts on the lane again.
        state.picked.clear();
        state.picked_groups.clear();
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
                let grab = (geometry.beat_of(pos.x), place_of(plot, pos.y));
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
        drag.now = dragged_to(
            drag.grab,
            geometry.beat_of(pos.x),
            place_of(plot, pos.y),
            grabbed_at(param, drag),
            plot,
            geometry,
            shift,
        );
    }
    if body.dragged_by(egui::PointerButton::Primary)
        && let Some(pos) = body.interact_pointer_pos()
        && let Some(drag) = state.point_drag.as_mut().filter(|d| d.segment && d.track == track && &d.key == key)
    {
        let position = place_of(plot, pos.y);
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
    state.dropped.take_if(|d| d.track == track && &d.key == key);
    if body.drag_stopped_by(egui::PointerButton::Primary) || ui.input(|i| i.pointer.primary_released()) {
        if let Some(drag) = state.point_drag.take_if(|d| d.body && d.track == track && &d.key == key)
            && let Some(action) = drop_points(&drag, param, geometry, free)
        {
            actions.push(action);
            state.dropped = Some(drag);
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
            // Painted down or up over other lanes of the track, the range is on all of them (#1322).
            let more: Vec<LaneKey> = state.covered.iter().filter(|k| *k != key).cloned().collect();
            state.covered.clear();
            state.range = (end - start > 1e-6).then(|| LaneRange { track, key: key.clone(), start, end, more });
            // A range painted moves the play start marker to its start, as a time selection does
            // (#937): the parameter's controls show the value there, and Play starts there (#811).
            if state.range.is_some() {
                let beats = start.min(suhina_engine::MAX_START_BEAT);
                actions.push(Action::new("transport.setStart", json!({ "beats": beats })));
                // Until the marker is there, after this frame, the controls show the line there.
                if param.enabled {
                    let points = param.points.clone();
                    state.drawn_drag = Some(Dragged { track, key: key.clone(), points, at: Some(beats) });
                }
            }
        }
    }
    // The lane's menu: shapes inserted into the painted range, or into the bar under the pointer.
    if body.secondary_clicked()
        && let Some(pos) = body.interact_pointer_pos()
    {
        state.menu_at = Some((track, key.clone(), geometry.beat_of(pos.x).max(0.0)));
        state.menu_line = near_line(pos).then(|| geometry.snap(geometry.beat_of(pos.x), free));
    }
    // A click inside (Middle, Bottom, a slider) keeps the menu open; its commands close it.
    egui::Popup::context_menu(&body).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
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
                let lane = (&param.points[..], &level[..], &param.scale);
                range_menu(ui, state, track, key, (start, end), center, geometry.grid.bar_beats, lane, actions);
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
                picked_menu(ui, state, (track, key, param), actions);
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
        drag.now = dragged_to(
            drag.grab,
            geometry.beat_of(pos.x),
            place_of(plot, pos.y),
            grabbed_at(param, drag),
            plot,
            geometry,
            shift,
        );
    }
    // While a point is dragged, its value and beat show beside the pointer, as its tooltip does
    // when it is let go of, so 47R can be told from 50R before the button comes up (#811).
    if let Some(text) = state
        .point_drag
        .as_ref()
        .filter(|d| d.track == track && &d.key == key && ui.input(|i| i.pointer.primary_down()))
        .and_then(|drag| dragged_text(param, drag, geometry, free))
    {
        egui::Tooltip::always_open(
            ui.ctx().clone(),
            ui.layer_id(),
            id.with("dragged-value"),
            egui::PopupAnchor::Pointer,
        )
        .gap(12.0)
        .show(|ui| ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend)));
    }
    // Points moved together are picked again where they went, once the move is in.
    if let Some((_, _, beats, frames)) = state.reselect.as_mut().filter(|(t, k, _, _)| *t == track && k == key) {
        let mut found: Vec<usize> = Vec::new();
        for beat in beats.iter() {
            let next =
                param.points.iter().enumerate().position(|(i, p)| (p.beat - beat).abs() < 1e-3 && !found.contains(&i));
            if let Some(i) = next {
                found.push(i);
            }
        }
        if found.len() == beats.len() {
            found.sort_unstable();
            state.selection = Some(Selection { track, key: key.clone(), points: found });
            state.reselect = None;
        } else if *frames == 0 {
            state.reselect = None;
        } else {
            *frames -= 1;
            ui.ctx().request_repaint();
        }
    }
    let points = shown_points(param, track, geometry, free, state);
    // A lane being dragged: the controls of its parameter follow the line as it is drawn (#811).
    let held = state.point_drag.as_ref().or(state.dropped.as_ref()).is_some_and(|d| d.track == track && &d.key == key)
        || state.curve_drag.as_ref().is_some_and(|d| d.track == track && &d.key == key);
    // A range being painted: they show the line where its moving edge is, under the pointer.
    let painting = state
        .box_drag
        .as_ref()
        .filter(|d| !d.select && d.track == track && &d.key == key)
        .map(|d| geometry.snap(geometry.beat_of(d.to.x), free).max(0.0));
    if (held || painting.is_some()) && param.enabled {
        let at = if held { None } else { painting };
        state.drawn_drag = Some(Dragged { track, key: key.clone(), points: points.clone(), at });
    }
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
    let now = ui.input(|i| i.time);
    if let Some(stretch) =
        state.stretch.as_ref().filter(|s| s.track == track && &s.key == key && now - s.shown_at < 0.15)
    {
        let y = y_of(plot, stretch.pivot);
        let dashes = Shape::dashed_line(
            &[Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(1.0, ink.gamma_multiply(0.8)),
            4.0,
            4.0,
        );
        lane_painter.extend(dashes);
        ui.ctx().request_repaint();
    }
    if let Some(simplified) = state.simplify.as_ref().filter(|s| s.track == track && &s.key == key && s.amount > 0.0) {
        let before = envelope(&simplified.original, &param.scale, plot, geometry);
        if before.len() > 1 {
            lane_painter.add(Shape::line(before, Stroke::new(1.0, ink.gamma_multiply(LEVEL_ALPHA))));
        }
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
    // A range painted from another lane over this one is on this one too (#1322).
    let painting = state
        .box_drag
        .as_ref()
        .filter(|d| !d.select && d.track == track && (&d.key == key || state.covered.contains(key)));
    let painted = match painting {
        Some(drag) => Some((
            geometry.x_of(geometry.snap(geometry.beat_of(drag.from.x), free)),
            geometry.x_of(geometry.snap(geometry.beat_of(drag.to.x), free)),
        )),
        None => state
            .range
            .as_ref()
            .filter(|r| r.track == track && (&r.key == key || r.more.contains(key)))
            .map(|r| (geometry.x_of(r.start), geometry.x_of(r.end))),
    };
    if let Some((a, b)) = painted {
        // One area over every lane it is on: a lane with another below it reaches over the line
        // between them, and only the area's outer edges are drawn.
        let at = state.range_lanes.iter().position(|k| k == key);
        let first = at.is_none_or(|i| i == 0);
        let last = at.is_none_or(|i| i + 1 == state.range_lanes.len());
        let mut area = Rect::from_x_y_ranges(egui::Rangef::new(a.min(b), a.max(b)), time.y_range());
        if !last {
            area.max.y += LINE;
        }
        lane_painter.rect_filled(area, 0.0, theme::accent().gamma_multiply(0.15));
        let stroke = theme::hairline(theme::accent());
        let inner = area.shrink(0.5);
        lane_painter.line_segment([inner.left_top(), Pos2::new(inner.left(), area.bottom())], stroke);
        lane_painter.line_segment([inner.right_top(), Pos2::new(inner.right(), area.bottom())], stroke);
        if first {
            lane_painter.line_segment([inner.left_top(), inner.right_top()], stroke);
        }
        if last {
            lane_painter.line_segment([inner.left_bottom(), inner.right_bottom()], stroke);
        }
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
                    egui::Role::Slider,
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
            lane_painter.circle_filled(
                center,
                CURVE_HANDLE + hover,
                crate::kit::style_class::surface_or(theme::window_background()),
            );
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
                egui::Role::Button,
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
            let grab = (geometry.beat_of(pos.x), place_of(plot, pos.y));
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
            drag.now = dragged_to(
                drag.grab,
                geometry.beat_of(pos.x),
                place_of(plot, pos.y),
                grabbed_at(param, drag),
                plot,
                geometry,
                shift,
            );
        }
        if response.drag_stopped_by(egui::PointerButton::Primary)
            && let Some(drag) = state.point_drag.take_if(|d| d.track == track && &d.key == key && d.point == index)
        {
            if let Some(action) = drop_points(&drag, param, geometry, free) {
                actions.push(action);
            }
            // The moved points stay picked: their indices change with the edit, so they are found
            // again by the beats they went to once it is in.
            if drag.together.len() > 1 {
                let (beats, _) = drag_delta(&drag, param, geometry, free);
                let to: Vec<f64> = drag
                    .together
                    .iter()
                    .filter_map(|&i| param.points.get(i))
                    .map(|p| round((p.beat + beats).max(0.0), 1000.0))
                    .collect();
                state.selection = None;
                state.reselect = Some((track, key.clone(), to, 10));
            }
        }
        let selected = state.is_selected(track, key, index);
        egui::Popup::context_menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
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
            picked_menu(ui, state, (track, key, param), actions);
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
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Point value"));
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
        lane_painter.circle_stroke(
            center,
            radius,
            Stroke::new(1.0, crate::kit::style_class::surface_or(theme::window_background())),
        );
    }
}

/// The menu of a painted range: how many bars it holds; the shapes, with Depth and Rate for them
/// (a change puts the shape put in last in again, live while a slider is dragged); and for
/// whatever points the range holds, Scale, Smooth (rounds corners and jumps) and Simplify (fewer
/// points, the old line drawn faintly to compare).
#[allow(clippy::too_many_arguments)]
fn range_menu(
    ui: &mut Ui,
    state: &mut LanesState,
    track: TrackId,
    key: &LaneKey,
    (start, end): (f64, f64),
    center: f64,
    bar_beats: f64,
    (points, level, scale): (&[AutomationPoint], &[AutomationPoint], &Scale),
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
    // The range's span loops, over every lane it reaches, as Ctrl+L does.
    if ui.add(egui::Button::new("Loop selection").shortcut_text(crate::settings::keymap::shown("Ctrl+L"))).clicked() {
        actions.push(Action::new("transport.setLoop", json!({ "start": start, "end": end, "enabled": true })));
        ui.close();
    }
    ui.separator();
    // The lane and the range, the params every command of this menu starts from.
    let ranged = || {
        let mut params = key.params(track);
        params["start"] = json!(start);
        params["end"] = json!(end);
        params
    };
    let put = |state: &mut LanesState, shape: &'static str, actions: &mut Vec<Action>| {
        let rate = RATES.get(state.shape_rate.unwrap_or(DEFAULT_RATE)).map_or(1.0, |(_, r)| r.beats(bar));
        let mut params = ranged();
        params["shape"] = json!(shape);
        params["cycles"] = json!((((end - start) / rate.max(1e-6)).round() as usize).clamp(1, 512));
        params["depth"] = json!(state.shape_depth.unwrap_or(1.0));
        // Put in again in the same range, the shape swings around the place the first one did,
        // and replaces it when nothing else changed since (one gesture of a command whose calls
        // replace each other), so the range's edges come from the line as it was, not from the
        // shape before.
        let again =
            state.last_shape.clone().filter(|l| l.track == track && &l.key == key && l.start == start && l.end == end);
        let center = again.as_ref().map_or(center, |l| l.center);
        params["center"] = json!(center.clamp(0.0, 1.0));
        range_gesture(&mut params, actions, "shape", again.is_some());
        actions.push(Action::new("automation.insertShape", params));
        state.selection = None;
        state.smooth = None;
        state.simplify = None;
        state.stretch = None;
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
    let mut rate = state.shape_rate.unwrap_or(DEFAULT_RATE);
    let rate_slider = ui
        .add(
            egui::Slider::new(&mut rate, 0..=RATES.len() - 1)
                .text("Rate")
                .custom_formatter(|v, _| RATES.get(v as usize).map_or_else(String::new, |(l, _)| (*l).to_owned()))
                .custom_parser(|text| {
                    RATES.iter().position(|(l, _)| l.eq_ignore_ascii_case(text.trim())).map(|i| i as f64)
                }),
        )
        .on_hover_text(
            "How long one cycle of a repeating shape lasts: 1 bar is once a bar, 1/4 once a quarter note \
             (a beat in 4/4), 1/16 four times a beat. Changing it puts the shape in again.",
        );
    if rate_slider.changed() {
        state.shape_rate = Some(rate);
    }
    let depth_due = live(ui, state, &depth_slider);
    let rate_due = live(ui, state, &rate_slider);
    let inserted = state
        .last_shape
        .as_ref()
        .filter(|l| l.track == track && &l.key == key && l.start == start && l.end == end)
        .map(|l| l.shape);
    match inserted {
        Some(shape) if depth_due || rate_due => put(state, shape, actions),
        Some(_) => {}
        // Nothing of this menu's in the range to change: the two set the next shape.
        None => {
            ui.label(
                egui::RichText::new("Depth and Rate apply to the next shape inserted here")
                    .color(theme::text_hint())
                    .size(theme::TEXT_SMALL),
            );
        }
    }
    ui.separator();
    ui.label(egui::RichText::new("Points in the range").color(theme::text_hint()).size(theme::TEXT_SMALL));
    let of = format!("{}/{key:?}/range {start} {end}", track.0);
    let middle = middle_of(points, scale, start, end);
    if let Some((factor, again)) = stretch_slider(ui, state, (track, key), of.clone(), middle) {
        let mut params = ranged();
        params["factor"] = json!(factor);
        params["around"] = json!(if state.scale_bottom { "bottom" } else { "middle" });
        range_gesture(&mut params, actions, "scale", again);
        actions.push(Action::new("automation.scaleRange", params));
        // The shape is scaled now: Depth and Rate would put the unscaled one back, and Smooth and
        // Simplify start again from the scaled line.
        state.last_shape = None;
        state.simplify = None;
        state.smooth = None;
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
    state.smooth = Some(Smoothed { of: of.clone(), amount });
    if live(ui, state, &smooth_slider) {
        // Each amount replaces the one before in this range's step (one gesture), so it rounds
        // the line as it was; a line it does not change (a sine's) stays as it is.
        let mut params = ranged();
        params["amount"] = json!(amount.clamp(0.0, 1.0));
        range_gesture(&mut params, actions, "smooth", kept.is_some());
        actions.push(Action::new("automation.smooth", params));
        state.selection = None;
        state.last_shape = None;
        state.simplify = None;
        state.stretch = None;
    }
    // Simplify: fewer points, the rest bent to keep the line's shape, as far from it as the slider
    // lets them; the line from before is drawn faintly while the menu is open.
    let inside = |points: &[AutomationPoint]| points.iter().filter(|p| p.beat >= start && p.beat <= end).count();
    let kept = state.simplify.clone().filter(|s| s.of == of);
    let mut amount = kept.as_ref().map_or(0.0, |s| s.amount) * 100.0;
    let now = inside(points);
    let before = kept.as_ref().map_or(now, |s| s.before);
    let simplify_slider = ui
        .add(egui::Slider::new(&mut amount, 0.0..=100.0).text("Simplify").suffix(" %").fixed_decimals(0))
        .on_hover_text(
            "Removes the points the line does not need, bending the rest to keep its shape: further right \
             lets the new line stray further from the old one, which shows faintly while this menu is open.",
        );
    ui.label(
        egui::RichText::new(format!("{before} \u{2192} {now} points"))
            .color(theme::text_hint())
            .size(theme::TEXT_SMALL),
    );
    let amount = amount / 100.0;
    let original = kept.as_ref().map_or_else(|| level.to_vec(), |s| s.original.clone());
    state.simplify = Some(Simplified { of, track, key: key.clone(), amount, original, before });
    if live(ui, state, &simplify_slider) {
        // Each amount replaces the one before in this range's step, so it thins the line as it was.
        let mut params = ranged();
        params["tolerance"] = json!(simplify_tolerance(amount));
        range_gesture(&mut params, actions, "simplify", kept.is_some());
        actions.push(Action::new("automation.simplify", params));
        state.selection = None;
        state.smooth = None;
        state.last_shape = None;
        state.stretch = None;
    }
}

/// The picked points' part of a lane's menus, when points of this lane are picked: how many, and
/// Scale for them (`automation.scaleRange` with them, live while it is dragged).
fn picked_menu(
    ui: &mut Ui,
    state: &mut LanesState,
    (track, key, param): (TrackId, &LaneKey, &Param),
    actions: &mut Vec<Action>,
) {
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
    let beats = points.iter().filter_map(|&i| param.points.get(i)).map(|p| p.beat);
    let (first, last) = beats.fold((f64::MAX, f64::MIN), |(lo, hi), b| (lo.min(b), hi.max(b)));
    let middle = middle_of(&param.points, &param.scale, first, last);
    let of = format!("{}/{key:?}/points {points:?}", track.0);
    if let Some((factor, again)) = stretch_slider(ui, state, (track, key), of, middle) {
        let mut params = key.params(track);
        params["points"] = json!(points);
        params["factor"] = json!(factor);
        params["around"] = json!(if state.scale_bottom { "bottom" } else { "middle" });
        range_gesture(&mut params, actions, "scale", again);
        actions.push(Action::new("automation.scaleRange", params));
    }
}

/// Puts a range tool's call in its gesture (#1146): `again` joins the open one, so the call
/// replaces the last; otherwise the gesture is ended first and this call starts another. A range
/// gesture stays open when the mouse is let go (its id starts with [`RANGE_GESTURE`]): the next
/// click on the same tool joins it, and any other edit ends it.
fn range_gesture(params: &mut serde_json::Value, actions: &mut Vec<Action>, tool: &str, again: bool) {
    let gesture = format!("{RANGE_GESTURE}{tool}");
    if !again {
        actions.push(Action::new("history.endGesture", json!({ "gesture": gesture })));
    }
    params["gesture"] = json!(gesture);
}

/// The start of a range tool's gesture id, which the window does not end on mouse up.
pub const RANGE_GESTURE: &str = "range:";

/// The grid entries at the end of the lanes' menus: the Arrangement's grid, as its toolbar and
/// its lanes' menus offer it.
fn grid_menu(ui: &mut Ui, state: &mut LanesState, _actions: &mut Vec<Action>) {
    ui.separator();
    let keys = state.grid_keys.clone();
    if let Some(picked) = crate::interaction::grid_menu::grid_entries(ui, state.grid, &keys, &|_| true) {
        state.grid_picked = Some(picked);
        ui.close();
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
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                let response = response.on_hover_text(&label);
                let painter = ui.painter();
                let fill = if chosen == Some(name) {
                    theme::control_active()
                } else {
                    crate::kit::motion::mix(
                        crate::kit::style_class::surface_or(theme::window_background()),
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
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::DisclosureTriangle, true, open, name));
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
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, free, selected, &label));
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
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, free, selected, &label));
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
    field.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Search parameters"));
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

/// The parameter menu under `response` (the top lane's dropdown, #1322): every parameter the
/// track has, by device, those another lane shows greyed out. Answers the parameter chosen.
fn parameter_dropdown(
    ui: &mut Ui,
    response: &egui::Response,
    id: Id,
    track: TrackId,
    current: Option<&LaneKey>,
    state: &mut LanesState,
    params: &Params,
) -> Option<LaneKey> {
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
    let menu = egui::Popup::menu(response)
        // The theme's popup fill: see-through where the theme is, with the window blurred under it.
        .frame(egui::Frame::popup(ui.style()))
        .align(align)
        .align_alternatives(&[])
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks);
    let none = LaneKey::mixer("");
    let current = current.unwrap_or(&none).clone();
    let this_lane = (track, current.clone());
    let from_automated = state.from_automated.contains(&this_lane);
    let choices = params.of(track);
    match menu.show(|ui| parameter_menu(ui, filter, choices, &current, &shown, from_automated, room)) {
        Some(menu) => {
            // A menu over the dropdown is placed by its size the frame before: when that changed
            // (a heading, a search), the frame is drawn again rather than shown in the old place.
            let rect = menu.response.rect;
            if last.is_some_and(|r| r.size() != rect.size()) {
                ui.ctx().request_discard("the parameter menu changed size");
            }
            ui.data_mut(|d| d.insert_temp(placed, rect));
            if outside {
                egui::Popup::close_id(ui.ctx(), egui::Popup::default_response_id(response));
            }
            let (choice, top) = menu.inner?;
            // The lane's ids follow its parameter, so the menu's state is dropped now.
            forget_menu(ui, filter, placed);
            state.from_automated.remove(&this_lane);
            if top {
                state.from_automated.insert((track, choice.clone()));
            }
            state.selection = None;
            Some(choice)
        }
        // Closed: the next opening starts with an empty search, its groups closed, and no place.
        None => {
            forget_menu(ui, filter, placed);
            None
        }
    }
}

/// A lane's header, under the track's (#1322). The top lane's: a dropdown to choose its
/// parameter, **+** that mounts it under the top lane and **×** that clears it (the top lane
/// stays). A mounted lane's: its parameter's name, which does not change, and **×** that
/// unmounts it (the picked lanes with it). Both: whether its automation plays (a manual change
/// turns it off; the toggle turns it on again). A mounted header's free space picks the lane
/// (Ctrl adds, Shift picks a run) and, dragged, moves it; the top lane's, dragged, mounts it.
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
    painter.rect_filled(rect, 0.0, crate::kit::style_class::surface_or(theme::panel_background()));
    let track = lanes.track;
    let layout = state.layout(track, params);
    let top = !layout.lanes.contains(&param.key);
    if state.is_picked(track, &param.key) {
        painter.rect_filled(rect, 0.0, theme::accent().gamma_multiply(0.18));
    }
    let id = Id::unique(("automation-header", track.0, param.key.device, param.key.param.as_str()));
    let background = ui.interact(clip, id.with("background"), Sense::click_and_drag());
    if background.clicked() && !top {
        let modifiers = ui.input(|i| i.modifiers);
        state.pick(track, &param.key, modifiers, params);
    }
    if background.drag_started_by(egui::PointerButton::Primary) {
        let keys = if top { vec![param.key.clone()] } else { state.dragged_with(track, &param.key, params) };
        state.header_drag = Some(HeaderDrag { track, keys, group: false, pointer_y: rect.center().y });
    }
    if let Some(pos) = background.interact_pointer_pos()
        && let Some(drag) = state.header_drag.as_mut().filter(|d| d.track == track && !d.group)
    {
        drag.pointer_y = pos.y;
    }
    let grouped = layout.groups.iter().position(|g| g.keys.contains(&param.key));
    background.context_menu(|ui| {
        let tall = state.heights.contains_key(&(track, param.key.clone()));
        if ui.add_enabled(tall, egui::Button::new("Reset height")).clicked() {
            state.set_lane_height(track, &param.key, LANE);
            ui.close();
        }
        if top {
            return;
        }
        ui.separator();
        // The picked lanes, or this one alone when it is not picked.
        let keys = state.dragged_with(track, &param.key, params);
        let label = if keys.len() > 1 { format!("Group {} lanes", keys.len()) } else { "Group lane".into() };
        if ui.button(label).clicked() {
            let mut changed = layout.clone();
            changed.group(&keys);
            actions.push(changed.action(track));
            state.picked.clear();
            ui.close();
        }
        if let Some(index) = grouped
            && ui.button("Ungroup").clicked()
        {
            let mut changed = layout.clone();
            changed.ungroup(&[index]);
            actions.push(changed.action(track));
            ui.close();
        }
    });
    let inner = rect.shrink2(Vec2::new(PAD, 5.0));
    let row = Rect::from_min_size(inner.min, Vec2::new(inner.width(), HEADER_ROW));
    // The pin at the left, before the parameter; the top lane's x on its own at the right.
    let pin = Rect::from_min_size(row.min, Vec2::new(SMALL_BUTTON, HEADER_ROW));
    let clear =
        Rect::from_min_size(Pos2::new(row.right() - SMALL_BUTTON, row.top()), Vec2::new(SMALL_BUTTON, HEADER_ROW));
    let has_lane = !param.points.is_empty();
    let toggle_right = if top { clear.left() - 4.0 } else { row.right() };
    let toggle = Rect::from_min_size(Pos2::new(toggle_right - TOGGLE, row.top()), Vec2::new(TOGGLE, HEADER_ROW));
    let chooser_right = if has_lane { toggle.left() - 4.0 } else { toggle_right };
    let chooser = Rect::from_min_max(Pos2::new(pin.right() + 4.0, row.top()), Pos2::new(chooser_right, row.bottom()));
    let label = param.label();

    if top {
        // The top lane's parameter: a dropdown of every parameter the track has, by device.
        let response = widgets::button_area(
            ui,
            chooser.intersect(clip),
            id.with("param"),
            format!("Parameter of {label} lane on {}", lanes.name),
        );
        widgets::dropdown(ui, &painter, chooser, &response, &label);
        if let Some(choice) = parameter_dropdown(ui, &response, id, track, Some(&param.key), state, params) {
            state.set_active(track, choice, params);
        }
    } else {
        // A mounted lane keeps its parameter: its name only.
        widgets::text_in(
            &painter,
            chooser,
            Align2::LEFT_CENTER,
            &label,
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_primary(),
        );
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

    // The pin: on the top lane an outline that pins its parameter as a lane of its own; on a
    // mounted lane filled, in the accent colour, and a click unpins it with the picked lanes.
    if top {
        let response = widgets::button_area(
            ui,
            pin.intersect(clip),
            id.with("add"),
            format!("Mount {label} lane on {}", lanes.name),
        )
        .on_hover_text("Pin this parameter as a lane of its own below");
        if response.clicked() {
            actions.push(state.mount(track, &param.key, params));
        }
        small_button(ui, &painter, pin, &response, Glyph::Pin);
        // The top lane's x empties it.
        let response = widgets::button_area(
            ui,
            clear.intersect(clip),
            id.with("hide"),
            format!("Clear the top lane on {}", lanes.name),
        )
        .on_hover_text("Clear the top lane");
        if response.clicked() {
            state.from_automated.remove(&(track, param.key.clone()));
            state.clear_active(track);
        }
        small_button(ui, &painter, clear, &response, Glyph::Cross);
    } else {
        let count = state.dragged_with(track, &param.key, params).len();
        let hint = if count > 1 { format!("Unpin these {count} lanes") } else { "Unpin this lane".to_string() };
        let response = widgets::button_area(
            ui,
            pin.intersect(clip),
            id.with("hide"),
            format!("Hide {label} lane on {}", lanes.name),
        )
        .on_hover_text(hint);
        if response.clicked() {
            state.from_automated.remove(&(track, param.key.clone()));
            let hidden = state.dragged_with(track, &param.key, params);
            let mut changed = layout.clone();
            changed.remove(&hidden, &[]);
            actions.push(changed.action(track));
            state.picked.retain(|(t, k)| !(*t == track && hidden.contains(k)));
            if state.selection.as_ref().is_some_and(|s| s.track == track && hidden.contains(&s.key)) {
                state.selection = None;
            }
        }
        small_button(ui, &painter, pin, &response, Glyph::Pinned);
    }

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

/// The mark on one of a lane header's small buttons (#1322): a pin's outline (pin it), a filled
/// pin (pinned; a click unpins) or a cross (clear).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Glyph {
    Pin,
    Pinned,
    Cross,
}

/// A pin's head and shoulders, drawn in a 16-point square: the top, down to the shoulders, out
/// to the base and back; its needle goes from the base's middle down.
const PIN_HEAD: [(f32, f32); 6] = [(6.0, 2.5), (10.0, 2.5), (9.4, 6.5), (11.5, 8.7), (4.5, 8.7), (6.6, 6.5)];
const PIN_NEEDLE: [(f32, f32); 2] = [(8.0, 8.7), (8.0, 13.5)];

/// Draws one of a lane header's small buttons: no box, a faint background under the pointer,
/// and `glyph` in its middle, drawn with lines so it is the same in every font; dim until the
/// pointer is on it, a filled pin in the accent colour.
fn small_button(ui: &Ui, painter: &egui::Painter, rect: Rect, response: &egui::Response, glyph: Glyph) {
    let hover = widgets::hover(ui, response);
    if hover > 0.0 {
        painter.rect_filled(rect, 3.0, theme::control_hover().gamma_multiply(hover));
    }
    let ink = match glyph {
        Glyph::Pinned => theme::automation_pin(),
        _ => crate::kit::motion::mix(theme::text_hint(), theme::text_primary(), hover),
    };
    let stroke = Stroke::new(1.25, ink);
    let side = (rect.width().min(rect.height()) - 1.0).max(4.0);
    let origin = rect.center() - Vec2::splat(side / 2.0);
    let at = |(x, y): (f32, f32)| origin + Vec2::new(x, y) * (side / 16.0);
    match glyph {
        Glyph::Pin | Glyph::Pinned => {
            let head: Vec<Pos2> = PIN_HEAD.iter().copied().map(at).collect();
            if glyph == Glyph::Pinned {
                // The head is not convex: filled as its body and its base.
                let body = [0, 1, 2, 5].iter().filter_map(|i| head.get(*i).copied()).collect();
                let base = [5, 2, 3, 4].iter().filter_map(|i| head.get(*i).copied()).collect();
                painter.add(Shape::convex_polygon(body, ink, Stroke::NONE));
                painter.add(Shape::convex_polygon(base, ink, Stroke::NONE));
            }
            painter.add(Shape::closed_line(head, stroke));
            painter.line_segment([at(PIN_NEEDLE[0]), at(PIN_NEEDLE[1])], stroke);
        }
        Glyph::Cross => {
            let arm = side * 0.22;
            let center = rect.center();
            painter.line_segment([center + Vec2::new(-arm, -arm), center + Vec2::new(arm, arm)], stroke);
            painter.line_segment([center + Vec2::new(-arm, arm), center + Vec2::new(arm, -arm)], stroke);
        }
    }
}

#[cfg(test)]
#[path = "automation_lanes_tests.rs"]
mod tests;
