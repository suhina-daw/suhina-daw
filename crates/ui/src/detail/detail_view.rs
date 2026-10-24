// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The detail area: the selected track's device chain, and beside it the selected clip's
//! properties (`docs/spec/ui-session-view.md` › Detail area). The chain is laid out in a row under the view
//! or stacked in a column beside it (a setting). It shows the project as it is; changing devices
//! comes with the device commands. A plug-in device shows where its plug-in stands (loading,
//! running, missing, crashed) and a button that opens its editor or restarts it, through the
//! `device.*` commands (ADR 0006). A plug-in's editor opened inside the panel (the setting)
//! takes the room under that line, and where it lands in the window is reported each frame
//! ([`EditorPlace`]) for the host process to put it there.

use crate::kit::surface::Backgrounds as _;
use egui::{Align2, Id, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::json;
use suhina_device_sdk::builtin::{COMPACT_HEIGHT, COMPACT_WIDTH};
use suhina_device_sdk::{ControlShape, LabelPosition, ParameterControl};
use suhina_engine::model::{ClipId, DeviceKind, LaneData, Note, Project, StretchMode, TrackId};
use suhina_session::{Placement, PluginReport, PluginStatus};

use crate::interaction::grid::gain_to_db;
use crate::interaction::menus;
use crate::kit::motion;
use crate::kit::theme::{self, PAD, ROW};
use crate::kit::widgets::{self, Action};

fn device_display_name(device: &suhina_engine::model::Device) -> String {
    let Some(file) =
        device.sample_file().and_then(|path| std::path::Path::new(path).file_name()).and_then(|s| s.to_str())
    else {
        return device.name.clone();
    };
    let stem = std::path::Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or(file);
    if device.name.to_lowercase().contains(&stem.to_lowercase()) {
        device.name.clone()
    } else {
        format!("{} \u{b7} {file}", device.name)
    }
}

/// Width of one device in a row, at least.
pub const DEVICE_WIDTH: f32 = 220.0;
/// Default width of the column the stacked chain takes: a compact panel 260 px wide and its
/// padding (ADR 0021 › Amendment: device panels, 5).
pub const STACK_WIDTH: f32 = COMPACT_WIDTH + PAD * 2.0;
const PARAM_ROW: f32 = 16.0;
const NODE_ROW: f32 = 22.0;
/// Width of the clip's properties beside the chain, and of a property's label.
const CLIP_WIDTH: f32 = 196.0;
const LABEL_WIDTH: f32 = 52.0;
/// The Clip / Devices switch in the clip column's title row: one part's width, and its height.
const SWITCH_PART: f32 = 44.0;
const SWITCH_HEIGHT: f32 = 15.0;
const ON_SWITCH: f32 = 9.0;
/// A plug-in device's button (open its editor, restart it).
const BUTTON_HEIGHT: f32 = 18.0;
const BUTTON_WIDTH: f32 = 96.0;
/// Where an embedded editor starts under a plug-in device's top: its header, state line and
/// button.
const EDITOR_TOP: f32 = ROW + PAD + PARAM_ROW + PAD / 2.0 + BUTTON_HEIGHT + PAD / 2.0;

/// Where the panel shows a plug-in's embedded editor this frame, for its host to put it there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorPlace {
    pub track: TrackId,
    pub device: usize,
    pub placement: Placement,
    /// The room the panel has for the editor, in pixels, where it is asked to fit (an ARA
    /// editor in the clip's place); `None` where it keeps its own size.
    pub fit: Option<(u32, u32)>,
}

/// The height a device row needs to show an embedded editor `height` pixels tall under a
/// plug-in's line and button, at `pixels_per_point`.
pub fn row_height_with_editor(height: u32, pixels_per_point: f32) -> f32 {
    EDITOR_TOP + height as f32 / pixels_per_point.max(0.01) + PAD
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceBox {
    /// Its index in the list it is in: the track's devices, or a group's branch.
    pub index: usize,
    /// Where it is on the track, as the device commands take it.
    pub at: DeviceAt,
    /// For a device group: its settings and branches.
    pub group: Option<GroupBox>,
    pub name: String,
    /// What it is: a built-in effect or instrument, a plug-in format, or one Suhina cannot run.
    pub kind: String,
    /// What a theme selects its panel by, after `#device-`: a built-in device's id
    /// (`#device-compressor`), `sd` for a device file, `plugin` for a plug-in.
    pub theme_id: String,
    pub enabled: bool,
    /// Parameter ids and values as text.
    pub params: Vec<(String, String)>,
    /// Built-in parameter values, by id (in the parameter's unit, so a panel's knobs take them).
    pub values: Vec<(String, f64)>,
    /// The id of a device that runs a file ([docs/spec/nam-amp-and-cab.md]); the file is the
    /// state's path.
    pub file_device: Option<&'static str>,
    /// The file's path, when the device runs one.
    pub file: Option<String>,
    pub graph: Option<GraphBox>,
    /// For a plug-in: where it stands, while the project plays.
    pub plugin: Option<PluginBox>,
}

/// Where a device is on its track: its index, or its path through device groups
/// (`docs/spec/device-groups.md` › Commands).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct DeviceAt(pub Vec<usize>);

impl DeviceAt {
    /// The `device` parameter of a device command: the index of a device on the track, or the
    /// path `"2/1/0"` of one inside a group.
    pub fn value(&self) -> serde_json::Value {
        match self.0.as_slice() {
            [index] => json!(index),
            path => json!(path.iter().map(ToString::to_string).collect::<Vec<_>>().join("/")),
        }
    }

    /// Whether it is on the track itself, not inside a group.
    pub fn top(&self) -> bool {
        self.0.len() == 1
    }
}

/// A device group in the detail area: its mix and gain and its branches.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupBox {
    pub mix: f64,
    pub gain_db: f64,
    pub branches: Vec<BranchBox>,
}

/// One branch of a device group.
#[derive(Debug, Clone, PartialEq)]
pub struct BranchBox {
    pub name: String,
    pub volume_db: f64,
    pub pan: f64,
    pub mute: bool,
    pub solo: bool,
    pub devices: Vec<DeviceBox>,
}

/// What a device panel draws from a parameter list: an `.sd` device's graph, a built-in's
/// panel or a running plug-in's generated list.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphBox {
    /// The compact panel's canvas; `None` draws the parameters as a generated list.
    pub canvas: Option<std::sync::Arc<suhina_device_sdk::ui::DeviceUi>>,
    /// The device's larger panel, opened from its header over the main area, if it has one.
    pub expanded: Option<std::sync::Arc<suhina_device_sdk::ui::DeviceUi>>,
    /// The visible nodes form one chain, so the list marks each step with an arrow.
    pub linear: bool,
    /// The names of an `.sd` graph's visible nodes, in signal order; empty for the others.
    pub nodes: Vec<String>,
    /// Every parameter, in its device's order.
    pub parameters: Vec<GraphParameter>,
}

/// One parameter as a device panel shows and edits it.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphParameter {
    /// How its device writes the value it was listed with (a plug-in's "-3.2 dB"), or empty.
    pub text: String,
    /// What a double-click puts it back to.
    pub default: f64,
    /// How it looks: label, size, colours, step, decimals, a choice's labels.
    pub ui: suhina_device_sdk::ParameterUi,
    /// The control that edits it.
    pub control: ParameterControl,
    /// Written after its value; empty for none.
    pub unit: String,
    /// The id `device.setParam` names it by.
    pub id: String,
    /// Its label in the panel.
    pub name: String,
    /// Its lowest value.
    pub min: f64,
    /// Its highest value.
    pub max: f64,
    /// Its value now, within `min..=max`.
    pub value: f64,
}

impl GraphParameter {
    /// `parameter` as a panel shows it, at `value`.
    pub(crate) fn new(parameter: &suhina_device_sdk::Parameter, value: f64) -> Self {
        Self {
            text: String::new(),
            default: parameter.default,
            ui: parameter.ui.clone(),
            control: parameter.control,
            unit: parameter.unit.clone(),
            id: parameter.id.clone(),
            name: parameter.name.clone(),
            min: parameter.min,
            max: parameter.max,
            value,
        }
    }
}

/// `parameters` at the values `device` keeps for them (kept in range), or at their defaults.
fn graph_parameters(
    parameters: &[suhina_device_sdk::Parameter],
    device: &suhina_engine::model::Device,
) -> Vec<GraphParameter> {
    parameters
        .iter()
        .map(|p| {
            GraphParameter::new(p, device.params.iter().find(|v| v.id == p.id).map_or(p.default, |v| p.clamp(v.value)))
        })
        .collect()
}

/// Where a plug-in device's plug-in stands, as its panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginBox {
    pub state: PluginState,
    /// One line about it: the plug-in that runs, or why it does not.
    pub line: String,
    pub editor_open: bool,
    /// Its editor is open inside the panel: the size of it, in pixels.
    pub embedded: Option<(u32, u32)>,
    /// It is bound to its track's audio clips (ARA), so its editor edits them.
    pub ara: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    /// The project is not playing, so its plug-ins are not running.
    Stopped,
    Loading,
    Running,
    Missing,
    Crashed,
}

impl PluginBox {
    fn from_report(report: Option<PluginReport>) -> Self {
        let Some(report) = report else {
            return Self {
                state: PluginState::Stopped,
                line: "Not running".into(),
                editor_open: false,
                embedded: None,
                ara: false,
            };
        };
        let name = report.plugin.as_ref().map(|(name, vendor, _)| format!("{name} by {vendor}"));
        let (state, line) = match &report.status {
            PluginStatus::Loading => (PluginState::Loading, format!("Loading {}", name.unwrap_or_default())),
            PluginStatus::Running => {
                let mut line = match (&report.state_refused, report.latency) {
                    (Some(_), _) => "Running with its own settings".to_string(),
                    (None, 0) => name.unwrap_or_else(|| "Running".into()),
                    (None, latency) => format!("{} · {latency} frames", name.unwrap_or_else(|| "Running".into())),
                };
                // Blocks it did not answer in time: the chain went on without it for those.
                if report.late_blocks > 0 && report.blocks > 0 {
                    let percent = (u64::from(report.late_blocks) * 100 / u64::from(report.blocks)).max(1);
                    line.push_str(&format!(" · {percent}% of blocks late"));
                }
                (PluginState::Running, line)
            }
            PluginStatus::Missing => (PluginState::Missing, "Not installed; kept with its settings".into()),
            PluginStatus::Crashed(why) => (PluginState::Crashed, format!("Crashed: {why}")),
            PluginStatus::Failed(why) => (PluginState::Crashed, format!("Could not start: {why}")),
        };
        Self { state, line, editor_open: report.editor_open, embedded: report.editor_embedded, ara: report.ara }
    }

    /// The size the embedded editor takes in the panel, in points.
    fn editor_size(&self, pixels_per_point: f32) -> Option<Vec2> {
        let (width, height) = self.embedded?;
        let scale = pixels_per_point.max(0.01);
        Some(Vec2::new(width as f32 / scale, height as f32 / scale))
    }

    /// What its button does: its label and the command it runs.
    fn button(&self) -> Option<(&'static str, &'static str)> {
        match self.state {
            PluginState::Running if self.editor_open => Some(("Close editor", "device.closeEditor")),
            PluginState::Running => Some(("Open editor", "device.openEditor")),
            PluginState::Crashed => Some(("Restart", "device.restart")),
            PluginState::Stopped | PluginState::Loading | PluginState::Missing => None,
        }
    }
}

/// A selected clip: on the arrangement (its track and its id, which stays when it moves), or in
/// a launcher slot (its track and scene, from 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipRef {
    Arrangement(TrackId, ClipId),
    Slot(TrackId, usize),
}

impl ClipRef {
    pub fn track(self) -> TrackId {
        match self {
            Self::Arrangement(track, _) | Self::Slot(track, _) => track,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClipInfo {
    pub name: String,
    pub track: String,
    /// Where it is: bar.beat where an arrangement clip starts, or the number of a slot's scene
    /// (`scene` tells which); and its length in bars and beats.
    pub start: String,
    pub scene: bool,
    pub length: String,
    pub looping: bool,
    /// The loop's start and end in content beats, when it loops.
    pub loop_range: Option<(f64, f64)>,
    pub audio: bool,
    pub gain_db: Option<f64>,
    /// An audio clip's warp switch, warp mode and transpose in semitones (`clip.setAudio`).
    pub warped: bool,
    pub stretch: StretchMode,
    pub transpose: f64,
    /// The clip the column's controls edit; none where nothing can be edited.
    pub at: Option<ClipRef>,
    /// Fade-in and fade-out in beats (`clip.setFades`).
    pub fades: (f64, f64),
    pub notes: usize,
    /// What the piano roll has selected in this clip (› Clip column in
    /// `docs/spec/ui-clip-editor.md`).
    pub selection: SelectionInfo,
    /// The Clip / Devices switch at the end of the title row, and which part is on: only for a
    /// clip with an editor (› Where it opens in `docs/spec/ui-clip-editor.md`).
    pub switch: Option<Showing>,
}

/// What the detail area shows for a clip that has an editor: the editor, or its track's devices.
/// Window state, not part of the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Showing {
    #[default]
    Clip,
    Devices,
}

/// What the detail area's frame picked: the Clip / Devices switch's value, and a device panel
/// clicked (to select it).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Picked {
    pub showing: Option<Showing>,
    /// A device panel was clicked: the track and the device's index, to select.
    pub clicked: Option<(TrackId, usize)>,
    /// How tall the device chain's panels are, in points, when the area shows only the chain:
    /// the detail area is kept no taller.
    pub content_height: Option<u32>,
}

/// What the piano roll has selected in the clip the detail area shows: how many notes, how many
/// of them are muted, the span in bars and beats, and the range of keys and velocities (1 to 127).
/// Empty when nothing is selected, so the column shows no rows for it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SelectionInfo {
    pub selected: usize,
    pub muted: usize,
    pub from: Option<String>,
    pub to: Option<String>,
    pub low_key: Option<u8>,
    pub high_key: Option<u8>,
    pub low_velocity: Option<u8>,
    pub high_velocity: Option<u8>,
}

impl SelectionInfo {
    /// The selection `selected` (indices into `notes`) described as the clip column shows it.
    /// `bar_beats` is how long a bar is, as [`build_with`] reads it from the time signature.
    pub fn of(notes: &[Note], selected: &std::collections::BTreeSet<usize>, bar_beats: f64) -> Self {
        let mut info = Self { selected: selected.len(), ..Self::default() };
        let mut low_key: Option<u8> = None;
        let mut high_key: Option<u8> = None;
        let mut low_velocity: Option<u8> = None;
        let mut high_velocity: Option<u8> = None;
        let mut start: Option<f64> = None;
        let mut end: Option<f64> = None;
        for index in selected {
            let Some(note) = notes.get(*index) else { continue };
            if note.muted {
                info.muted += 1;
            }
            low_key = Some(low_key.map_or(note.key, |key| key.min(note.key)));
            high_key = Some(high_key.map_or(note.key, |key| key.max(note.key)));
            let velocity = suhina_session::midi_velocity(note.velocity);
            low_velocity = Some(low_velocity.map_or(velocity, |v| v.min(velocity)));
            high_velocity = Some(high_velocity.map_or(velocity, |v| v.max(velocity)));
            start = Some(start.map_or(note.time, |time: f64| time.min(note.time)));
            let note_end = note.time + note.duration;
            end = Some(end.map_or(note_end, |time: f64| time.max(note_end)));
        }
        info.from = start.map(|beats| bars_beats(beats, bar_beats, 1));
        info.to = end.map(|beats| bars_beats(beats, bar_beats, 1));
        info.low_key = low_key;
        info.high_key = high_key;
        info.low_velocity = low_velocity;
        info.high_velocity = high_velocity;
        info
    }
}

/// How long a bar is in the project's time signature, in beats, as the clip column counts bars
/// and beats.
pub fn bar_beats(project: &Project) -> f64 {
    let ts = project.transport.time_signature;
    if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) }
}

/// A position as the column writes it: `bar.beat`, counted from one for a position and from zero
/// for a length.
pub fn bars_beats(beats: f64, bar_beats: f64, from_one: u64) -> String {
    if !(beats.is_finite() && bar_beats.is_finite()) || bar_beats <= 0.0 {
        return "-".into();
    }
    let beats = beats.max(0.0);
    let bar = (beats / bar_beats).floor() as u64 + from_one;
    let beat = (beats % bar_beats).floor() as u64 + from_one;
    format!("{bar}.{beat}")
}

/// A track's device chain.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    pub track: TrackId,
    pub name: String,
    pub devices: Vec<DeviceBox>,
    /// The NAM panels' folders' files and a file's names and notes
    /// ([docs/spec/nam-amp-and-cab.md] › Folders).
    pub nam: crate::detail::nam_panel::Lists,
}

/// What the detail area shows: a selected clip comes with its track's chain, since that chain
/// is what the clip plays through. The clip's properties are boxed: they are much larger than the
/// chain or nothing, and the detail is one value moved around each frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Detail {
    Devices(Chain),
    Clip(Box<ClipInfo>, Chain),
    Nothing,
}

impl Detail {
    /// While the transport runs, the panels show what automation plays at `beat`: every enabled
    /// lane of a device's parameter sets the value its knob, slider and text show (#706).
    pub fn follow_automation(&mut self, project: &Project, beat: f64) {
        let chain = match self {
            Self::Devices(chain) | Self::Clip(_, chain) => chain,
            Self::Nothing => return,
        };
        let Some(channel) = project
            .tracks
            .iter()
            .find(|t| t.id == chain.track)
            .and_then(|t| t.channel)
            .and_then(|c| project.channel(c))
        else {
            return;
        };
        fn follow(boxes: &mut [DeviceBox], devices: &[suhina_engine::model::Device], beat: f64) {
            for device_box in boxes {
                if let Some(group) = device_box.group.as_mut() {
                    for branch in &mut group.branches {
                        follow(&mut branch.devices, devices, beat);
                    }
                }
                let Some(device) = suhina_engine::model::device_at(devices, &device_box.at.0) else { continue };
                // A built-in parameter's description, for its kind and range.
                let info = match &device.kind {
                    DeviceKind::Builtin(id) => suhina_device_registry::info(id),
                    _ => None,
                };
                for lane in device.automation.iter().filter(|l| l.enabled && !l.points.is_empty()) {
                    let spec = info.and_then(|info| info.params.iter().find(|p| p.id == lane.param));
                    // On/off and choice parameters hold each point's value until the next, as the
                    // engine plays them and the lane draws them.
                    let stepped = match spec {
                        Some(spec) => matches!(
                            spec.kind,
                            suhina_device_api::ParamKind::Toggle | suhina_device_api::ParamKind::Choice(_)
                        ),
                        None => device_box
                            .graph
                            .as_ref()
                            .and_then(|g| g.parameters.iter().find(|p| p.id == lane.param))
                            .is_some_and(|p| p.control.is_stepped()),
                    };
                    let Some(value) = lane.value_at(beat, stepped) else { continue };
                    let value = spec.map_or(value, |spec| spec.clamp(value));
                    for (id, v) in &mut device_box.values {
                        if *id == lane.param {
                            *v = value;
                        }
                    }
                    for (id, text) in &mut device_box.params {
                        if *id == lane.param {
                            *text = format_value(value);
                        }
                    }
                    if let Some(graph) = device_box.graph.as_mut() {
                        for parameter in graph.parameters.iter_mut().filter(|p| p.id == lane.param) {
                            let value = value.clamp(parameter.min, parameter.max);
                            // A plug-in's text is for the value it listed, not the one played.
                            if value != parameter.value {
                                parameter.text.clear();
                            }
                            parameter.value = value;
                        }
                    }
                }
            }
        }
        follow(&mut chain.devices, &channel.devices, beat);
    }
}

/// The detail for a selected clip (it wins, with its own track's chain) or track.
pub fn build(project: &Project, track: Option<TrackId>, clip: Option<ClipRef>) -> Detail {
    build_with(project, track, clip, &mut |_, _| None, crate::detail::nam_panel::Lists::default())
}

/// [`build`], with each plug-in device's state from `plugins` (track, device path).
pub fn build_with(
    project: &Project,
    track: Option<TrackId>,
    clip: Option<ClipRef>,
    plugins: &mut dyn FnMut(TrackId, &[usize]) -> Option<PluginReport>,
    nam: crate::detail::nam_panel::Lists,
) -> Detail {
    let bar = bar_beats(project);
    let track_name = |id: TrackId| project.track(id).map(|t| t.name.clone()).unwrap_or_default();
    let found = clip.and_then(|clip| match clip {
        ClipRef::Arrangement(id, clip) => project
            .arrangement
            .lanes
            .iter()
            .find(|l| l.track == Some(id))
            .and_then(|l| l.clips.iter().find(|c| c.id == clip))
            .map(|placed| (id, placed, None)),
        ClipRef::Slot(id, scene) => project
            .launcher
            .columns
            .iter()
            .find(|c| c.track == id)
            .and_then(|c| c.slots.get(scene))
            .and_then(|s| s.clip.as_ref())
            .map(|placed| (id, placed, Some(scene))),
    });
    if let Some((id, placed, scene)) = found {
        let content = project.content(placed.content);
        let audio = content.and_then(|c| {
            c.lanes.iter().find_map(|l| match &l.data {
                LaneData::Audio(region) => Some(region),
                LaneData::Notes(_) => None,
            })
        });
        let notes = content.map_or(0, |c| {
            c.lanes
                .iter()
                .map(|l| match &l.data {
                    LaneData::Notes(notes) => notes.len(),
                    LaneData::Audio(_) => 0,
                })
                .sum()
        });
        return Detail::Clip(
            Box::new(ClipInfo {
                name: content.map(|c| c.name.clone()).unwrap_or_default(),
                track: track_name(id),
                start: scene.map_or_else(|| bars_beats(placed.start, bar, 1), |s| (s + 1).to_string()),
                scene: scene.is_some(),
                length: bars_beats(placed.length, bar, 0),
                looping: placed.loop_range.is_some(),
                loop_range: placed.loop_range.map(|r| (r.start, r.end)),
                audio: audio.is_some(),
                gain_db: audio.map(|region| gain_to_db(region.gain)),
                warped: audio.is_some_and(|region| region.warped),
                stretch: audio.map(|region| region.stretch).unwrap_or_default(),
                transpose: audio.map_or(0.0, |region| region.transpose),
                at: Some(match scene {
                    Some(scene) => ClipRef::Slot(id, scene),
                    None => ClipRef::Arrangement(id, placed.id),
                }),
                fades: (placed.fade_in, placed.fade_out),
                notes,
                selection: SelectionInfo::default(),
                switch: None,
            }),
            chain(project, id, plugins, nam.clone()),
        );
    }
    track.map_or(Detail::Nothing, |id| Detail::Devices(chain(project, id, plugins, nam.clone())))
}

/// A track's devices as the detail area shows them.
fn chain(
    project: &Project,
    id: TrackId,
    plugins: &mut dyn FnMut(TrackId, &[usize]) -> Option<PluginReport>,
    nam: crate::detail::nam_panel::Lists,
) -> Chain {
    let track_name = project.track(id).map(|t| t.name.clone()).unwrap_or_default();
    let channel = project.tracks.iter().find(|t| t.id == id).and_then(|t| t.channel).and_then(|c| project.channel(c));
    let devices = channel
        .map(|c| c.devices.iter().enumerate().map(|(index, d)| device_box(d, &[index], id, plugins)).collect())
        .unwrap_or_default();
    Chain { track: id, name: track_name, devices, nam }
}

/// One device as the detail area shows it, at `path` on track `id`; a group with its branches.
fn device_box(
    d: &suhina_engine::model::Device,
    path: &[usize],
    id: TrackId,
    plugins: &mut dyn FnMut(TrackId, &[usize]) -> Option<PluginReport>,
) -> DeviceBox {
    let report = matches!(d.kind, DeviceKind::Clap { .. } | DeviceKind::Vst3 { .. } | DeviceKind::Vst2 { .. })
        .then(|| plugins(id, path));
    DeviceBox {
        index: path.last().copied().unwrap_or(0),
        at: DeviceAt(path.to_vec()),
        group: d.as_group().map(|g| group_box(g, d, path, id, plugins)),
        name: device_display_name(d),
        theme_id: match &d.kind {
            DeviceKind::Builtin(id) => id.to_ascii_lowercase(),
            DeviceKind::Sd => "sd".into(),
            DeviceKind::Clap { .. } | DeviceKind::Vst3 { .. } | DeviceKind::Vst2 { .. } => "plugin".into(),
            _ => "unsupported".into(),
        },
        kind: match &d.kind {
            DeviceKind::Builtin(_) => "Suhina".into(),
            DeviceKind::Sd => d
                .state
                .as_deref()
                .and_then(|state| std::str::from_utf8(state).ok())
                .and_then(|text| suhina_device_sdk::DeviceGraph::from_sd(text).ok())
                .map(|graph| {
                    match graph.role {
                        suhina_device_sdk::DeviceRole::Effect => "AudioEffect",
                        suhina_device_sdk::DeviceRole::Instrument => "Instrument",
                        suhina_device_sdk::DeviceRole::MidiEffect => "MIDI Effect",
                    }
                    .to_owned()
                })
                .unwrap_or_else(|| "AudioEffect".into()),
            DeviceKind::Clap { .. } => "CLAP".into(),
            DeviceKind::Vst3 { .. } => "VST3".into(),
            DeviceKind::Vst2 { .. } => "VST2".into(),
            DeviceKind::Foreign { format, .. } => format!("Not supported ({format})"),
            DeviceKind::Group(_) => "Group".into(),
        },
        enabled: d.enabled,
        params: d.params.iter().map(|p| (p.id.clone(), format_value(p.value))).collect(),
        values: if let DeviceKind::Builtin(id) = &d.kind {
            let info = suhina_device_registry::info(id);
            info.map(|info| {
                info.params
                    .iter()
                    .map(|p| {
                        (
                            p.id.to_owned(),
                            d.params
                                .iter()
                                .find(|stored| stored.id == p.id)
                                .map_or(p.default, |stored| p.clamp(stored.value)),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
        } else {
            Vec::new()
        },
        file_device: match &d.kind {
            DeviceKind::Builtin(id) if suhina_device_registry::is_file_device(id) => {
                suhina_device_registry::info(id).map(|info| info.id)
            }
            _ => None,
        },
        file: d
            .state
            .as_deref()
            .and_then(|state| std::str::from_utf8(state).ok())
            .filter(|path| !path.is_empty())
            .map(str::to_owned),
        graph: match (&d.kind, d.state.as_deref()) {
            // A built-in's panel, loaded by its id from this Suhina (ADR 0021 › Amendment: device
            // panels): its parameters from the device, its layout's canvas or the generated list.
            (DeviceKind::Builtin(id), _) => suhina_device_registry::panel(id).map(|panel| GraphBox {
                canvas: panel.ui.clone(),
                expanded: panel.expanded.clone(),
                linear: false,
                nodes: vec![],
                parameters: graph_parameters(&panel.parameters, d),
            }),
            (DeviceKind::Sd, Some(state)) => crate::detail::device_canvas::graph(state).map(|cached| {
                let graph = &cached.graph;
                GraphBox {
                    canvas: cached.canvas.clone(),
                    expanded: cached.expanded.clone(),
                    linear: graph.connections.len() == graph.nodes.len().saturating_sub(1)
                        && graph
                            .nodes
                            .iter()
                            .all(|node| graph.connections.iter().filter(|e| e.from == node.id).count() <= 1)
                        && graph.connections.iter().all(|e| e.from_port == "out" && e.to_port == "in"),
                    nodes: graph
                        .ordered_nodes()
                        .iter()
                        .filter(|node| node.visible)
                        .map(|node| node.kind.info().name.into())
                        .collect(),
                    parameters: graph_parameters(&graph.parameters, d),
                }
            }),
            // A plug-in's generated list, from the parameters it listed (#679).
            _ => report
                .as_ref()
                .and_then(Option::as_ref)
                .filter(|r| r.status == PluginStatus::Running)
                .and_then(|r| r.params.as_deref())
                .map(|list| plugin_graph(d, list)),
        },
        plugin: report.map(PluginBox::from_report),
    }
}

/// A plug-in's parameters as its generated list draws them: every one it shows and lets be
/// changed, in its order, with the value the project keeps for it or the one it listed, and its
/// own text for that value. A switch is a toggle; a stepped parameter moves in its steps.
fn plugin_graph(d: &suhina_engine::model::Device, list: &[suhina_plugins::ParamInfo]) -> GraphBox {
    let parameters = list
        .iter()
        .filter(|p| !p.hidden && !p.read_only && p.min < p.max)
        .take(suhina_device_sdk::MAX_PARAMETERS)
        .map(|p| {
            let id = p.id.to_string();
            let stored = d.params.iter().find(|v| suhina_engine::param_id(&v.id) == Some(p.id)).map(|v| v.value);
            let toggle = p.steps == 1 && p.min == 0.0 && p.max == 1.0;
            let mut ui = suhina_device_sdk::ParameterUi { group: p.group.clone(), decimals: 2, ..Default::default() };
            if p.steps > 1 {
                ui.step = (p.max - p.min) / f64::from(p.steps);
            }
            GraphParameter {
                text: if stored.is_none_or(|v| v == p.value) { p.text.clone() } else { String::new() },
                default: p.default.clamp(p.min, p.max),
                ui,
                control: if toggle { ParameterControl::Toggle } else { ParameterControl::Slider },
                unit: String::new(),
                name: if p.name.is_empty() { id.clone() } else { p.name.clone() },
                id,
                min: p.min,
                max: p.max,
                value: stored.unwrap_or(p.value).clamp(p.min, p.max),
            }
        })
        .collect();
    GraphBox { canvas: None, expanded: None, linear: false, nodes: vec![], parameters }
}

/// A device group's settings and its branches with their devices.
fn group_box(
    group: &suhina_engine::model::DeviceGroup,
    device: &suhina_engine::model::Device,
    path: &[usize],
    id: TrackId,
    plugins: &mut dyn FnMut(TrackId, &[usize]) -> Option<PluginReport>,
) -> GroupBox {
    let values = suhina_engine::group_param_values(&device.params);
    GroupBox {
        mix: values.first().copied().unwrap_or(1.0),
        gain_db: values.get(1).copied().unwrap_or(0.0),
        branches: group
            .branches
            .iter()
            .enumerate()
            .map(|(b, branch)| BranchBox {
                name: branch.name.clone(),
                volume_db: gain_to_db(branch.volume),
                pan: branch.pan,
                mute: branch.mute,
                solo: branch.solo,
                devices: branch
                    .devices
                    .iter()
                    .enumerate()
                    .map(|(i, inner)| {
                        let mut at = path.to_vec();
                        at.extend([b, i]);
                        device_box(inner, &at, id, plugins)
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn format_value(value: f64) -> String {
    if !value.is_finite() {
        return "-".into();
    }
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The detail laid out in a row: devices side by side, or the clip's properties. A plug-in's
/// button adds its command to `actions`; where its embedded editor lands goes to `places`.
/// Returns the part the clip's Clip / Devices switch was clicked to, if it was.
pub fn show_row(
    ui: &mut Ui,
    detail: &Detail,
    selected: &[(TrackId, usize)],
    actions: &mut Vec<Action>,
    places: &mut Vec<EditorPlace>,
) -> Picked {
    let _style_class = crate::kit::style_class::enter("detail");
    let mut styled_ui = crate::kit::style_class::ui(ui);
    let ui = &mut *styled_ui;
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.fill_background(area, 0.0, theme::window_background());
    crate::kit::native_css::background(&painter, area);
    match detail {
        Detail::Nothing => hint(&painter, area, "Select a track or a clip to see it here"),
        Detail::Devices(chain) => return chain_row(ui, &painter, area, chain, selected, actions, places),
        Detail::Clip(info, chain) => {
            let (column, picked) = clip_properties(ui, &painter, area, info, false, actions);
            clip_for_readers(ui, column, info);
            let rest = Rect::from_min_max(Pos2::new(column.right() + 1.0, area.top()), area.max);
            // The chain is drawn beside the clip's properties: a click on one of its panels
            // selects the device as it does with the chain alone.
            let chain_picked = if rest.width() > 0.0 {
                chain_row(ui, &painter, rest, chain, selected, actions, places)
            } else {
                Picked::default()
            };
            return Picked { showing: picked, clicked: chain_picked.clicked, content_height: None };
        }
    }
    Picked::default()
}

/// A chain's devices side by side in `area`; one with an embedded editor is as wide as it. Each
/// panel is as tall as what it shows; a chain wider than the area scrolls sideways with the
/// wheel's sideways turn (Shift and the wheel) or the bar along its bottom.
fn chain_row(
    ui: &mut Ui,
    painter: &egui::Painter,
    area: Rect,
    chain: &Chain,
    selected: &[(TrackId, usize)],
    actions: &mut Vec<Action>,
    places: &mut Vec<EditorPlace>,
) -> Picked {
    if chain.devices.is_empty() {
        hint(painter, area, &format!("No devices on {} yet: drop audio effects here", chain.name));
        return Picked::default();
    }
    let scale = ui.ctx().pixels_per_point();
    let mut picked = Picked::default();
    // A group's panel, then the devices of its shown branch, in the row as the chain's own.
    let mut entries = Vec::new();
    flattened(ui, chain.track, &chain.devices, &mut entries);
    // Each panel's width and height first, so the scroll knows how wide the chain is.
    let boxes: Vec<(f32, f32)> = entries
        .iter()
        .map(|device| {
            if let Some(group) = &device.group {
                return (GROUP_WIDTH, group_height(group));
            }
            // An editor embedded in the panel is placed for devices on the track itself.
            let editor = device.plugin.as_ref().and_then(|p| p.editor_size(scale)).filter(|_| device.at.top());
            // A NAM panel asks for the mock's wider box (nam spec › The panels); the rest keep
            // the plain device width.
            let authored_base =
                if device.file_device.is_some() { crate::detail::nam_panel::NAM_WIDTH } else { DEVICE_WIDTH };
            // A NAM panel draws its own controls: the stock graph every built-in has (#600) does
            // not size it, or its controls are crowded over each other.
            let graph = device.graph.as_ref().filter(|_| device.file_device.is_none());
            let authored_width = graph.map_or(authored_base, |graph| {
                graph.canvas.as_ref().map_or_else(
                    || {
                        // The generated list is a compact panel's width, wider only where an
                        // `.sd` file places its controls further out.
                        parameter_layout(&graph.parameters, COMPACT_WIDTH, &[])
                            .iter()
                            .map(|cell| cell.right() + PAD * 2.0)
                            .fold(STACK_WIDTH, f32::max)
                    },
                    |c| c.viewport_size.map_or(DEVICE_WIDTH.max(c.size[0] + PAD * 2.0), |v| v[0] + PAD * 2.0),
                )
            });
            let width = editor.map_or(authored_width, |e| authored_width.max(e.x + PAD * 2.0));
            // `.device { width }` or `#device-<id> { width }` widens the panel (up to 1600 points).
            let width = {
                let _style = crate::kit::style_class::enter_named("device", &format!("device-{}", device.theme_id));
                crate::kit::style_class::computed()
                    .and_then(|c| c.width)
                    .map_or(width, |themed| width.max(themed.clamp(120.0, 1600.0)))
            };
            let height = sdk_viewport(device)
                .map_or_else(|| panel_height(device, width - PAD * 2.0, editor), |v| ROW + PAD * 2.0 + v[1]);
            (width, height)
        })
        .collect();
    let total = boxes.iter().map(|(w, _)| w + 1.0).sum::<f32>();
    let scroll_id = Id::new(("chain-scroll", chain.track.0));
    let most = (total - area.width()).max(0.0);
    let mut offset = ui.data(|d| d.get_temp::<f32>(scroll_id)).unwrap_or(0.0).clamp(0.0, most);
    if most > 0.0 && ui.rect_contains_pointer(area) {
        offset = (offset - ui.input(|i| i.smooth_scroll_delta.x)).clamp(0.0, most);
    }
    // The bar along the bottom while the chain is wider than the area.
    let bar = (most > 0.0).then(|| Rect::from_min_max(Pos2::new(area.left(), area.bottom() - 6.0), area.max));
    if let Some(bar) = bar {
        let response = ui.interact(bar, scroll_id.with("bar"), Sense::drag());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Device chain scroll bar"));
        if response.dragged() {
            offset = (offset + response.drag_delta().x * total / area.width()).clamp(0.0, most);
        }
    }
    ui.data_mut(|d| d.insert_temp(scroll_id, offset));
    // Panels past the area's edges take no clicks there.
    let outer_clip = ui.clip_rect();
    ui.set_clip_rect(area.intersect(outer_clip));
    let mut x = area.left() - offset;
    let mut tallest: f32 = 0.0;
    for (device, (width, height)) in entries.iter().zip(boxes) {
        let editor = device.plugin.as_ref().and_then(|p| p.editor_size(scale)).filter(|_| device.at.top());
        // A panel with its editor embedded takes the area's whole height, and the editor is
        // fitted to it (#689).
        let shown_height = if editor.is_some() { area.height() } else { height.min(area.height()) };
        let rect = Rect::from_min_size(Pos2::new(x, area.top()), Vec2::new(width, shown_height));
        x = rect.right() + 1.0;
        tallest = tallest.max(height);
        if rect.right() < area.left() {
            continue;
        }
        if rect.left() > area.right() {
            break;
        }
        // Picks are of the track's own devices (Ctrl+G groups them, Delete removes them).
        let lit = device.at.top() && selected.contains(&(chain.track, device.index));
        let clicked = match &device.group {
            Some(group) => {
                group_panel(ui, &painter.with_clip_rect(area), rect, chain.track, device, group, lit, actions)
            }
            None => {
                device_panel(ui, &painter.with_clip_rect(area), rect, chain.track, device, lit, actions, &chain.nam)
            }
        };
        inside_group_mark(&painter.with_clip_rect(area), rect, &device.at);
        // Keep a device an earlier iteration clicked: the later panels were not clicked.
        if clicked && device.at.top() {
            picked.clicked = Some((chain.track, device.index));
        }
        plugin_button(ui, &painter.with_clip_rect(area), rect, chain.track, device, actions);
        if let Some(editor) = editor {
            let origin = Pos2::new(rect.left() + PAD, rect.top() + EDITOR_TOP);
            places.push(editor_place(ui, area, rect, editor, chain.track, device, origin));
        }
    }
    ui.set_clip_rect(outer_clip);
    if let Some(bar) = bar {
        painter.fill_background(bar, 0.0, theme::control_background());
        let thumb_width = (area.width() / total * bar.width()).max(24.0);
        let thumb_left = bar.left() + offset / most * (bar.width() - thumb_width);
        let thumb =
            Rect::from_min_size(Pos2::new(thumb_left, bar.top() + 1.0), Vec2::new(thumb_width, bar.height() - 2.0));
        painter.rect_filled(thumb, 0.0, theme::text_hint());
    }
    // Right of the last panel, what a drop does.
    if x < area.right() - 80.0 {
        let rest = Rect::from_min_max(
            Pos2::new(x, area.top()),
            Pos2::new(area.right(), (area.top() + tallest).min(area.bottom())),
        );
        hint(painter, rest, "Drop audio effects here");
    }
    picked.content_height = Some((tallest + if bar.is_some() { 8.0 } else { 0.0 }).ceil() as u32);
    picked
}

/// How tall a device's panel is in a row or a stack: its header, then its parameters, its
/// plug-in rows, its embedded editor, or a NAM panel's own height.
fn panel_height(device: &DeviceBox, width: f32, editor: Option<Vec2>) -> f32 {
    let rows =
        if device.plugin.is_some() { 2.0 + BUTTON_HEIGHT / PARAM_ROW } else { device.params.len().max(1) as f32 };
    match editor {
        Some(editor) => EDITOR_TOP + editor.y + PAD,
        // A NAM panel's own height (nam spec › The panels); the rest take their rows.
        None if device.file_device.is_some() => crate::detail::nam_panel::NAM_HEIGHT,
        // A plug-in's rows, then its generated list, within the compact panel's height.
        None if device.plugin.is_some() => {
            let rows = ROW + PAD * 2.0 + rows * PARAM_ROW;
            device.graph.as_ref().map_or(rows, |graph| {
                rows.max((EDITOR_TOP + graph_body(graph, width) + PAD).min(ROW + PAD * 2.0 + COMPACT_HEIGHT))
            })
        }
        None => ROW + PAD * 2.0 + device.graph.as_ref().map_or(rows * PARAM_ROW, |graph| graph_body(graph, width)),
    }
}

/// How tall a device's parameters, nodes and canvas are. A list without a canvas is at most a
/// compact panel's height and scrolls past it (ADR 0021 › Amendment: device panels, 5).
fn graph_body(graph: &GraphBox, width: f32) -> f32 {
    let cells = parameter_layout(
        &graph.parameters,
        width,
        graph.canvas.as_ref().map_or(&[], |c| c.hidden_parameters.as_slice()),
    )
    .iter()
    .map(|r| r.bottom())
    .fold(0.0f32, f32::max);
    let height = cells + graph.nodes.len() as f32 * NODE_ROW;
    match &graph.canvas {
        Some(canvas) => height + canvas.size[1],
        None => height.min(COMPACT_HEIGHT),
    }
}

fn sdk_viewport(device: &DeviceBox) -> Option<[f32; 2]> {
    device.graph.as_ref()?.canvas.as_ref()?.viewport_size
}

/// The detail stacked in a column: devices one under another. Returns the part the clip's
/// Clip / Devices switch was clicked to, if it was.
pub fn show_stack(
    ui: &mut Ui,
    detail: &Detail,
    selected: &[(TrackId, usize)],
    actions: &mut Vec<Action>,
    places: &mut Vec<EditorPlace>,
) -> Picked {
    let _style_class = crate::kit::style_class::enter("detail");
    let mut styled_ui = crate::kit::style_class::ui(ui);
    let ui = &mut *styled_ui;
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.fill_background(area, 0.0, theme::window_background());
    crate::kit::native_css::background(&painter, area);
    match detail {
        Detail::Nothing => hint(&painter, area, "Select a track or a clip"),
        Detail::Devices(chain) => return chain_stack(ui, &painter, area, chain, selected, actions, places),
        Detail::Clip(info, chain) => {
            let (column, picked) = clip_properties(ui, &painter, area, info, true, actions);
            clip_for_readers(ui, column, info);
            let rest = Rect::from_min_max(Pos2::new(area.left(), column.bottom() + 1.0), area.max);
            // The chain is drawn under the clip's properties: its panels pick as they do with
            // the chain alone.
            let chain_picked = if rest.height() > 0.0 {
                chain_stack(ui, &painter, rest, chain, selected, actions, places)
            } else {
                Picked::default()
            };
            return Picked { showing: picked, clicked: chain_picked.clicked, content_height: None };
        }
    }
    Picked::default()
}

/// The clip's properties column for a clip whose editor fills the rest of the detail area (the
/// piano roll): on the left in a row, on top when `stacked`. Returns the rest. The quantise
/// controls and the MIDI tools live in the column beside this one
/// (› Other note tools in `docs/spec/ui-clip-editor.md`). With it, the part the Clip / Devices
/// switch was clicked to, if it was.
pub fn show_clip_column(ui: &mut Ui, info: &ClipInfo, stacked: bool, actions: &mut Vec<Action>) -> (Rect, Picked) {
    let _style_class = crate::kit::style_class::enter("clip-editor");
    let mut styled_ui = crate::kit::style_class::ui(ui);
    let ui = &mut *styled_ui;
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.fill_background(area, 0.0, theme::window_background());
    crate::kit::native_css::background(&painter, area);
    let (column, picked) = clip_properties(ui, &painter, area, info, stacked, actions);
    clip_for_readers(ui, column, info);
    let rest = if stacked {
        Rect::from_min_max(Pos2::new(area.left(), column.bottom() + 1.0), area.max)
    } else {
        Rect::from_min_max(Pos2::new(column.right() + 1.0, area.top()), area.max)
    };
    (rest, Picked { showing: picked, ..Picked::default() })
}

/// A chain's devices one under another in `area`; one with an embedded editor is as tall as
/// it needs (the editor is cut off at the column's right edge when it is wider).
fn chain_stack(
    ui: &mut Ui,
    painter: &egui::Painter,
    area: Rect,
    chain: &Chain,
    selected: &[(TrackId, usize)],
    actions: &mut Vec<Action>,
    places: &mut Vec<EditorPlace>,
) -> Picked {
    if chain.devices.is_empty() {
        hint(painter, area, &format!("No devices on {} yet", chain.name));
        return Picked::default();
    }
    let scale = ui.ctx().pixels_per_point();
    let mut picked = Picked::default();
    let mut y = area.top();
    let mut entries = Vec::new();
    flattened(ui, chain.track, &chain.devices, &mut entries);
    for device in entries {
        if let Some(group) = &device.group {
            let rect = Rect::from_min_size(Pos2::new(area.left(), y), Vec2::new(area.width(), group_height(group)));
            y = rect.bottom() + 1.0;
            if rect.top() > area.bottom() {
                break;
            }
            let lit = device.at.top() && selected.contains(&(chain.track, device.index));
            let clicked =
                group_panel(ui, &painter.with_clip_rect(area), rect, chain.track, device, group, lit, actions);
            inside_group_mark(&painter.with_clip_rect(area), rect, &device.at);
            if clicked && device.at.top() {
                picked.clicked = Some((chain.track, device.index));
            }
            continue;
        }
        let editor = device.plugin.as_ref().and_then(|p| p.editor_size(scale)).filter(|_| device.at.top());
        let height = panel_height(device, area.width() - PAD * 2.0, editor);
        let height = sdk_viewport(device).map_or(height, |v| ROW + PAD * 2.0 + v[1]);
        let right = sdk_viewport(device).map_or(area.right(), |v| (area.left() + v[0] + PAD * 2.0).min(area.right()));
        let rect = Rect::from_min_max(Pos2::new(area.left(), y), Pos2::new(right, y + height));
        y = rect.bottom() + 1.0;
        if rect.top() > area.bottom() {
            break;
        }
        let lit = device.at.top() && selected.contains(&(chain.track, device.index));
        let clicked =
            device_panel(ui, &painter.with_clip_rect(area), rect, chain.track, device, lit, actions, &chain.nam);
        inside_group_mark(&painter.with_clip_rect(area), rect, &device.at);
        // Keep a device an earlier iteration clicked: the later panels were not clicked.
        if clicked && device.at.top() {
            picked.clicked = Some((chain.track, device.index));
        }
        plugin_button(ui, &painter.with_clip_rect(area), rect, chain.track, device, actions);
        if let Some(editor) = editor {
            let origin = Pos2::new(rect.left() + PAD, rect.top() + EDITOR_TOP);
            places.push(editor_place(ui, area, rect, editor, chain.track, device, origin));
        }
    }
    picked
}

/// Where a plug-in's embedded editor of `editor` points goes under its line and button in the
/// device's `rect`, as the host takes it: in the window's own pixels, with the part of the
/// panel's `area` (and the device's box) that shows it; what falls outside is cut off.
fn editor_place(
    ui: &Ui,
    area: Rect,
    rect: Rect,
    editor: Vec2,
    track: TrackId,
    device: &DeviceBox,
    origin: Pos2,
) -> EditorPlace {
    let scale = ui.ctx().pixels_per_point();
    let shown = Rect::from_min_size(origin, editor).intersect(rect).intersect(area);
    let pixel = |value: f32| (value * scale).round() as i32;
    let span = |from: f32, to: f32| u32::try_from(pixel(to).saturating_sub(pixel(from))).unwrap_or(0);
    let placement = if shown.is_negative() || shown.width() <= 0.0 || shown.height() <= 0.0 {
        Placement::HIDDEN
    } else {
        Placement {
            x: pixel(origin.x),
            y: pixel(origin.y),
            clip_x: pixel(shown.left()),
            clip_y: pixel(shown.top()),
            clip_width: span(shown.left(), shown.right()),
            clip_height: span(shown.top(), shown.bottom()),
        }
    };
    // The editor takes the panel's room under its header, when it can be resized (#689).
    let room = Vec2::new(rect.right() - PAD - origin.x, rect.bottom() - PAD - origin.y);
    let fit = (room.x >= 1.0 && room.y >= 1.0).then(|| (span(0.0, room.x), span(0.0, room.y)));
    EditorPlace { track, device: device.index, placement, fit }
}

/// A plug-in device's button under its state line: opens or closes its editor while it runs,
/// restarts it after a crash. A click adds the command to `actions`.
fn plugin_button(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    track: TrackId,
    device: &DeviceBox,
    actions: &mut Vec<Action>,
) {
    let Some((label, command)) = device.plugin.as_ref().and_then(PluginBox::button) else { return };
    let top = rect.top() + ROW + PAD + PARAM_ROW + PAD / 2.0;
    let button = Rect::from_min_size(Pos2::new(rect.left() + PAD, top), Vec2::new(BUTTON_WIDTH, BUTTON_HEIGHT));
    if button.bottom() > rect.bottom() || !painter.clip_rect().intersects(button) {
        return;
    }
    let response = widgets::button_area(
        ui,
        button,
        egui::Id::new(("plugin-button", track.0, &device.at)),
        format!("{label} {}", device.name),
    );
    let hover = widgets::hover(ui, &response);
    let fill = crate::kit::motion::mix(theme::control_background(), theme::control_hover(), hover);
    painter.fill_background(button, 0.0, fill);
    painter.rect_stroke(button, 0.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
    widgets::text_in(
        painter,
        button,
        Align2::CENTER_CENTER,
        label,
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_primary(),
    );
    if response.clicked() {
        actions.push(Action::new(command, json!({ "track": track.0, "device": device.at.value() })));
    }
}

/// The Clip part of an audio clip on a track whose plug-in edits clips (ARA, ADR 0028): the
/// plug-in's editor in `area`, where a MIDI clip has its piano roll. Embedded, it lands there
/// (`places`); in a window of its own, or closed, the area says so with a button to open or
/// close it. `device` is the plug-in's index in `chain`.
pub fn show_ara_editor(
    ui: &mut Ui,
    area: Rect,
    chain: &Chain,
    device: usize,
    actions: &mut Vec<Action>,
    places: &mut Vec<EditorPlace>,
) {
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.fill_background(area, 0.0, theme::window_background());
    let Some(device) = chain.devices.iter().find(|d| d.index == device) else { return };
    let Some(plugin) = &device.plugin else { return };
    let response = ui.interact(area, egui::Id::new(("ara-editor", chain.track.0)), Sense::hover());
    let label = format!("Clip editor {}", device.name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label));
    let track = chain.track;
    let device_params = json!({ "track": track.0, "device": device.index });
    // Embedded: a row with Float above the editor, which takes it out to a window of its own.
    if let Some(size) = plugin.editor_size(ui.ctx().pixels_per_point()) {
        let float = Rect::from_min_size(area.min + Vec2::splat(PAD), Vec2::new(BUTTON_WIDTH, BUTTON_HEIGHT));
        let params = json!({ "track": track.0, "device": device.index, "float": true });
        ara_button(
            ui,
            &painter,
            float,
            (track, "float"),
            "Float",
            &device.name,
            Action::new("view.araFloat", params),
            actions,
        );
        let rect = Rect::from_min_size(Pos2::new(area.left() + PAD, float.bottom() + PAD), size);
        let mut place = editor_place(ui, area, rect.expand(PAD), size, track, device, rect.min);
        // The editor is asked to take the room under Float, as a piano roll fills it.
        let room = Rect::from_min_max(rect.min, area.max - Vec2::splat(PAD));
        let scale = ui.ctx().pixels_per_point();
        let pixels = |points: f32| (points * scale).max(0.0).round() as u32;
        place.fit = Some((pixels(room.width()), pixels(room.height())));
        places.push(place);
        return;
    }
    let mut buttons = Vec::new();
    let text = match plugin.state {
        PluginState::Running if plugin.editor_open => {
            let params = json!({ "track": track.0, "device": device.index, "float": false });
            buttons.push(("Dock", Action::new("view.araFloat", params)));
            buttons.push(("Close editor", Action::new("device.closeEditor", device_params)));
            format!("{} is open in a window of its own", device.name)
        }
        PluginState::Running => {
            buttons.push(("Open editor", Action::new("device.openEditor", device_params)));
            format!("Open {} to edit this clip", device.name)
        }
        _ => format!("{}: {}", device.name, plugin.line),
    };
    let centre = area.center();
    painter.text(
        centre - Vec2::new(0.0, BUTTON_HEIGHT),
        Align2::CENTER_CENTER,
        text,
        theme::ui_font(theme::TEXT),
        theme::text_hint(),
    );
    let count = buttons.len() as f32;
    let width = count * BUTTON_WIDTH + (count - 1.0).max(0.0) * PAD;
    for (index, (label, action)) in buttons.into_iter().enumerate() {
        let left = centre.x - width / 2.0 + index as f32 * (BUTTON_WIDTH + PAD);
        let rect = Rect::from_min_size(Pos2::new(left, centre.y + PAD / 2.0), Vec2::new(BUTTON_WIDTH, BUTTON_HEIGHT));
        ara_button(ui, &painter, rect, (track, label), label, &device.name, action, actions);
    }
}

/// A button of the ARA editor's area: `label`, named with the plug-in for screen readers; a
/// click adds `action`.
#[allow(clippy::too_many_arguments, reason = "one button's place, names and action")]
fn ara_button(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    id: (TrackId, &str),
    label: &str,
    plugin: &str,
    action: Action,
    actions: &mut Vec<Action>,
) {
    let response =
        widgets::button_area(ui, rect, egui::Id::new(("ara-button", id.0.0, id.1)), format!("{label} {plugin}"));
    let hover = widgets::hover(ui, &response);
    painter.fill_background(
        rect,
        0.0,
        crate::kit::motion::mix(theme::control_background(), theme::control_hover(), hover),
    );
    painter.rect_stroke(rect, 0.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
    widgets::text_in(
        painter,
        rect,
        Align2::CENTER_CENTER,
        label,
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_primary(),
    );
    if response.clicked() {
        actions.push(action);
    }
}

/// Names the clip's properties for screen readers and tests: "Clip Walk, scene 2".
fn clip_for_readers(ui: &Ui, rect: Rect, info: &ClipInfo) {
    let response = ui.interact(rect, egui::Id::new("clip-properties"), Sense::hover());
    let label = if info.scene {
        format!("Clip {}, scene {}", info.name, info.start)
    } else {
        format!("Clip {}, start {}", info.name, info.start)
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
}

fn hint(painter: &egui::Painter, area: Rect, text: &str) {
    painter.text(area.center(), Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT), theme::text_hint());
}

/// One device: a header with its on switch and name, then its parameters, one per row. A NAM
/// Amp Modeler or NAM Cab Loader draws its own panel instead
/// ([docs/spec/nam-amp-and-cab.md] › The panels).
#[allow(clippy::too_many_arguments)]
fn device_panel(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    track: TrackId,
    device: &DeviceBox,
    selected: bool,
    actions: &mut Vec<Action>,
    nam: &crate::detail::nam_panel::Lists,
) -> bool {
    // The panel answers a right-click with its menu (the device's remove); its label is the
    // screen-reader and test name the chain drew for it before. A click selects the device, so
    // the Delete key takes it off the track.
    let panel = ui
        .interact(rect, Id::new(("device-panel", track.0, &device.at)), Sense::click_and_drag())
        .on_hover_text("Right-click for the device's menu; drag onto another track to move it there, Ctrl to copy it");
    if panel.drag_started_by(egui::PointerButton::Primary) && device.at.top() {
        crate::interaction::drop::start_device_carry(ui.ctx(), track, device.index);
    }
    panel.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Device {}", device.name)));
    let clicked = panel.clicked();
    panel.context_menu(|ui| {
        menus::device_menu(ui, track, &device.at, device.group.as_ref().map(|g| g.branches.len()), actions)
    });
    // The selection outline, over everything the panel draws — on every path out of here.
    let outline = |painter: &egui::Painter| {
        if selected {
            painter.rect_stroke(rect, 1.0, theme::hairline(theme::selection()), egui::StrokeKind::Inside);
        }
    };
    // `.device` styles every device's panel and `#device-compressor` (a built-in device's id)
    // one kind of them; `.device-header` its header row. Everything inside (`.device-control`,
    // the generated parameter list's widgets) is in that scope.
    let _device_style = crate::kit::style_class::enter_named("device", &format!("device-{}", device.theme_id));
    crate::kit::native_css::mark(crate::kit::native_css::HostState::Selected, selected);
    crate::kit::native_css::mark(crate::kit::native_css::HostState::Muted, !device.enabled);
    let mut styled_ui = crate::kit::style_class::ui(ui);
    let ui = &mut *styled_ui;
    crate::kit::native_css::surface(painter, rect, 0.0, theme::panel_background());
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW));
    {
        let _header = crate::kit::style_class::enter("device-header");
        crate::kit::native_css::surface(painter, header, 0.0, theme::control_background());
    }
    let switch = Rect::from_min_size(
        Pos2::new(header.left() + PAD, header.center().y - ON_SWITCH / 2.0),
        Vec2::splat(ON_SWITCH),
    );
    // The switch bypasses the device: off, the sound passes through untouched. It says which
    // way a click goes, and answers for itself to screen readers and tests.
    let bypass = ui.interact(switch, Id::new(("device-bypass", track.0, &device.at)), Sense::click()).on_hover_text(
        if device.enabled { format!("Bypass {}", device.name) } else { format!("Enable {}", device.name) },
    );
    bypass.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            if device.enabled { format!("Bypass {}", device.name) } else { format!("Enable {}", device.name) },
        )
    });
    if bypass.clicked() {
        actions.push(Action::new(
            "device.setEnabled",
            json!({ "track": track.0, "device": device.at.value(), "enabled": !device.enabled }),
        ));
    }
    if device.enabled {
        painter.rect_filled(switch, 0.0, theme::text_primary());
    } else {
        painter.rect_stroke(switch, 0.0, theme::hairline(theme::text_hint()), egui::StrokeKind::Inside);
    }
    let name_color = if device.enabled { theme::text_primary() } else { theme::text_hint() };
    // The X at the header's right end removes the device, as Remove device in its menu does.
    let close = Rect::from_center_size(Pos2::new(header.right() - PAD - 6.0, header.center().y), Vec2::splat(12.0));
    let remove = ui
        .interact(close, Id::new(("device-remove", track.0, &device.at)), Sense::click())
        .on_hover_text(format!("Remove {}", device.name));
    remove.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Remove {}", device.name)));
    if remove.clicked() {
        actions.push(Action::new("device.remove", json!({ "track": track.0, "device": device.at.value() })));
    }
    let cross = if remove.hovered() { theme::text_primary() } else { theme::text_hint() };
    let arm = 3.5;
    painter.line_segment(
        [close.center() + Vec2::new(-arm, -arm), close.center() + Vec2::new(arm, arm)],
        egui::Stroke::new(1.2, cross),
    );
    painter.line_segment(
        [close.center() + Vec2::new(-arm, arm), close.center() + Vec2::new(arm, -arm)],
        egui::Stroke::new(1.2, cross),
    );
    let mut kind_right = close.left() - 4.0;
    // A device with an expanded panel has a button for it beside its X (ADR 0021 › Amendment:
    // device panels, 6); a second click closes it again.
    if device.graph.as_ref().is_some_and(|g| g.expanded.is_some()) {
        let button = Rect::from_center_size(Pos2::new(kind_right - 6.0, header.center().y), Vec2::splat(12.0));
        kind_right = button.left() - 4.0;
        let open = expanded(ui.ctx()).is_some_and(|(t, at)| t == track && at == device.at);
        let label =
            if open { format!("Close {}'s expanded panel", device.name) } else { format!("Expand {}", device.name) };
        let response = widgets::button_area(ui, button, Id::new(("device-expand", track.0, &device.at)), &label)
            .on_hover_text(&label);
        if response.clicked() {
            if open {
                close_expanded(ui.ctx());
            } else {
                open_expanded(ui.ctx(), track, device.at.clone());
            }
        }
        let ink = if response.hovered() || open { theme::text_primary() } else { theme::text_hint() };
        // Two corners pointing out: the panel grows.
        let r = button.shrink(2.0);
        for (corner, x, y) in [(r.right_top(), -1.0, 1.0), (r.left_bottom(), 1.0, -1.0)] {
            painter.line_segment([corner, corner + Vec2::new(4.0 * x, 0.0)], egui::Stroke::new(1.2, ink));
            painter.line_segment([corner, corner + Vec2::new(0.0, 4.0 * y)], egui::Stroke::new(1.2, ink));
        }
    }
    let kind_width = widgets::text_width(ui, &device.kind, theme::ui_font(theme::TEXT_TINY));
    let name = Rect::from_min_max(
        Pos2::new(switch.right() + PAD, header.top()),
        Pos2::new((kind_right - PAD - kind_width).max(switch.right() + PAD), header.bottom()),
    );
    widgets::text_in(painter, name, Align2::LEFT_CENTER, &device.name, theme::ui_font(theme::TEXT), name_color);
    widgets::text_in(
        painter,
        Rect::from_min_max(Pos2::new(kind_right - kind_width, header.top()), Pos2::new(kind_right, header.bottom())),
        Align2::RIGHT_CENTER,
        &device.kind,
        theme::ui_font(theme::TEXT_TINY),
        theme::text_hint(),
    );

    let body = Rect::from_min_max(
        Pos2::new(rect.left() + PAD, header.bottom() + PAD),
        Pos2::new(rect.right() - PAD, rect.bottom() - PAD),
    );
    match device.file_device {
        Some("nam-amp-modeler") => {
            crate::detail::nam_panel::show_amp(ui, body, track, device, nam, actions);
            outline(painter);
            return clicked;
        }
        Some("nam-cab-loader") => {
            crate::detail::nam_panel::show_cab(ui, body, track, device, nam, actions);
            outline(painter);
            return clicked;
        }
        _ => {}
    }
    if let Some(plugin) = &device.plugin {
        let color = match plugin.state {
            PluginState::Crashed => theme::recording(),
            PluginState::Running => theme::text_secondary(),
            PluginState::Stopped | PluginState::Loading | PluginState::Missing => theme::text_hint(),
        };
        widgets::text_in(
            painter,
            Rect::from_min_size(body.min, Vec2::new(body.width(), PARAM_ROW)),
            Align2::LEFT_CENTER,
            &plugin.line,
            theme::ui_font(theme::TEXT_SMALL),
            color,
        );
        // Its generated list under its button, while its editor is not in the panel (#679).
        if plugin.embedded.is_none()
            && let Some(graph) = &device.graph
        {
            let list = Rect::from_min_max(Pos2::new(body.left(), rect.top() + EDITOR_TOP), body.max);
            if list.height() >= PARAM_ROW {
                graph_controls(ui, list, graph, track, &device.at, actions);
            }
        }
        outline(painter);
        return clicked;
    }
    if device.params.is_empty() && device.graph.is_none() {
        let text = if device.kind.starts_with("Not supported") {
            "Kept so the chain keeps its shape"
        } else {
            "No parameters"
        };
        widgets::text_in(
            painter,
            Rect::from_min_size(body.min, Vec2::new(body.width(), PARAM_ROW)),
            Align2::LEFT_CENTER,
            text,
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
        outline(painter);
        return clicked;
    }
    if let Some(graph) = &device.graph {
        graph_controls(ui, body, graph, track, &device.at, actions);
        outline(painter);
        return clicked;
    }
    for (index, (id, value)) in device.params.iter().enumerate() {
        let row = Rect::from_min_size(
            Pos2::new(body.left(), body.top() + index as f32 * PARAM_ROW),
            Vec2::new(body.width(), PARAM_ROW),
        );
        if row.bottom() > body.bottom() + 0.5 {
            break;
        }
        widgets::text_in(
            painter,
            row,
            Align2::LEFT_CENTER,
            id,
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_secondary(),
        );
        widgets::text_in(
            painter,
            row,
            Align2::RIGHT_CENTER,
            value,
            theme::mono_font(theme::TEXT_SMALL),
            theme::text_value(),
        );
    }
    outline(painter);
    clicked
}

/// Where the open expanded panel's device is, kept in the window's memory while the app runs.
fn expanded_id() -> Id {
    Id::new("device-expanded")
}

/// The device whose expanded panel is open, if one is.
pub fn expanded(ctx: &egui::Context) -> Option<(TrackId, DeviceAt)> {
    ctx.data(|d| d.get_temp::<(TrackId, DeviceAt)>(expanded_id()))
}

/// Opens the expanded panel of the device at `at` on `track`, closing any other.
fn open_expanded(ctx: &egui::Context, track: TrackId, at: DeviceAt) {
    ctx.data_mut(|d| d.insert_temp(expanded_id(), (track, at)));
}

/// Closes the expanded panel.
pub fn close_expanded(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<(TrackId, DeviceAt)>(expanded_id()));
}

/// The open expanded panel's device in what the detail area shows, with its track. A panel
/// whose device is no longer shown (another track selected, the device removed) closes.
pub fn expanded_device(ctx: &egui::Context, detail: &Detail) -> Option<(TrackId, DeviceBox)> {
    /// The device at `at` among `devices` or inside their groups' branches.
    fn find<'a>(devices: &'a [DeviceBox], at: &DeviceAt) -> Option<&'a DeviceBox> {
        devices.iter().find_map(|device| {
            if &device.at == at {
                return Some(device);
            }
            device.group.as_ref()?.branches.iter().find_map(|branch| find(&branch.devices, at))
        })
    }
    let (track, at) = expanded(ctx)?;
    let chain = match detail {
        Detail::Devices(chain) | Detail::Clip(_, chain) => Some(chain),
        Detail::Nothing => None,
    };
    let found = chain
        .filter(|chain| chain.track == track)
        .and_then(|chain| find(&chain.devices, &at))
        .filter(|device| device.graph.as_ref().is_some_and(|g| g.expanded.is_some()));
    if found.is_none() {
        close_expanded(ctx);
    }
    found.map(|device| (track, device.clone()))
}

/// Draws `device`'s expanded panel over `area` (the main area), on top of the views: a title
/// row with its name and a close button, then its canvas, scrolled when the area is smaller.
/// Its controls change the same parameters as the compact panel, through `device.setParam`.
/// The close button and Escape close it.
pub fn show_expanded(ui: &mut Ui, area: Rect, track: TrackId, device: &DeviceBox, actions: &mut Vec<Action>) {
    let Some((graph, canvas)) = device.graph.as_ref().and_then(|g| Some((g, g.expanded.as_ref()?))) else {
        return;
    };
    egui::Area::new(Id::new("device-expanded-area")).order(egui::Order::Foreground).fixed_pos(area.min).show(
        ui.ctx(),
        |ui| {
            // The whole main area takes the pointer, so the views under it get no clicks.
            let (rect, _) = ui.allocate_exact_size(area.size(), Sense::click());
            let painter = ui.painter_at(rect);
            painter.fill_background(rect, 0.0, theme::panel_background());
            let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW + PAD));
            painter.fill_background(header, 0.0, theme::control_background());
            widgets::text_in(
                &painter,
                header.shrink2(Vec2::new(PAD * 2.0, 0.0)),
                Align2::LEFT_CENTER,
                &device.name,
                theme::ui_font(theme::TEXT),
                theme::text_primary(),
            );
            let close =
                Rect::from_min_size(Pos2::new(header.right() - 28.0, header.top() + 3.0), Vec2::new(24.0, 20.0));
            let response = widgets::button_area(ui, close, Id::new("device-expanded-close"), "Close expanded panel")
                .on_hover_text("Close the expanded panel (Esc)");
            let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
            painter.fill_background(close, 0.0, fill);
            for (from, to) in
                [(Vec2::new(-4.0, -4.0), Vec2::new(4.0, 4.0)), (Vec2::new(-4.0, 4.0), Vec2::new(4.0, -4.0))]
            {
                painter.line_segment(
                    [close.center() + from, close.center() + to],
                    egui::Stroke::new(1.3, theme::text_primary()),
                );
            }
            let escape = !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::Escape));
            if response.clicked() || escape {
                close_expanded(ui.ctx());
            }
            let body =
                Rect::from_min_max(Pos2::new(rect.left() + PAD, header.bottom() + PAD), rect.max - Vec2::splat(PAD));
            let size = Vec2::new(canvas.size[0], canvas.size[1]);
            ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
                egui::ScrollArea::both().auto_shrink([false, false]).id_salt("device-expanded-scroll").show(ui, |ui| {
                    // Centred while it fits, from the top left when it scrolls.
                    let room = ui.available_size();
                    let offset = Vec2::new(((room.x - size.x) / 2.0).max(0.0), ((room.y - size.y) / 2.0).max(0.0));
                    let (space, _) = ui.allocate_exact_size(size + offset, Sense::hover());
                    let canvas_rect = Rect::from_min_size(space.min + offset, size);
                    crate::detail::device_canvas::show(
                        ui,
                        canvas_rect,
                        canvas,
                        &graph.parameters,
                        track,
                        &device.at,
                        actions,
                    );
                });
            });
        },
    );
}

/// A graph's parameter list, its node list and its canvas in `body`, scrolled when they do not
/// fit: a `.sd` device's, a built-in's (its layout or the generated list) and a plug-in's
/// generated list. A list without a canvas is a compact panel's width wherever it is, and a
/// narrower `body` scrolls it (ADR 0021 › Amendment: device panels, 5); stock controls under a
/// canvas take `body`'s width. Every change is one `device.setParam`.
fn graph_controls(ui: &mut Ui, body: Rect, graph: &GraphBox, track: TrackId, at: &DeviceAt, actions: &mut Vec<Action>) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        egui::ScrollArea::both().auto_shrink([false, false]).id_salt(("sd-controls", track.0, at)).show(ui, |ui| {
            let hidden = graph.canvas.as_ref().map_or(&[][..], |c| c.hidden_parameters.as_slice());
            let width = if graph.canvas.is_some() { body.width() } else { COMPACT_WIDTH };
            let cells = parameter_layout(&graph.parameters, width, hidden);
            let custom_size = graph.canvas.as_ref().map_or(Vec2::ZERO, |c| Vec2::new(c.size[0], c.size[1]));
            let nodes_height = graph.nodes.len() as f32 * NODE_ROW;
            let extent = cells.iter().fold(Vec2::new(body.width().max(custom_size.x), 0.0), |size, rect| {
                Vec2::new(size.x.max(rect.right()), size.y.max(rect.bottom()))
            });
            let (_, body) = ui.allocate_space(Vec2::new(extent.x, extent.y + nodes_height + custom_size.y));
            let painter = &ui.painter_at(body.intersect(ui.clip_rect()));
            node_rows(painter, body, graph);
            if let Some(canvas) = &graph.canvas {
                let rect = Rect::from_min_size(body.min + Vec2::new(0.0, nodes_height), custom_size);
                crate::detail::device_canvas::show(ui, rect, canvas, &graph.parameters, track, at, actions);
            }
            let origin = Vec2::new(body.left(), body.top() + nodes_height + custom_size.y);
            for (parameter, cell) in graph.parameters.iter().filter(|p| !hidden.contains(&p.id)).zip(&cells) {
                parameter_cell(ui, painter, cell.translate(origin), parameter, track, at, actions);
            }
        });
    });
}

/// A graph's visible nodes from the top of `body`, one row each, with an arrow between them
/// when they form a single chain.
fn node_rows(painter: &egui::Painter, body: Rect, graph: &GraphBox) {
    let mut row_top = body.top();
    for (index, node) in graph.nodes.iter().enumerate() {
        let row = Rect::from_min_size(Pos2::new(body.left(), row_top), Vec2::new(body.width(), NODE_ROW));
        if row.bottom() > body.bottom() + 0.5 {
            break;
        }
        let node_rect = Rect::from_min_size(row.min, Vec2::new((row.width() - PAD) * 0.78, NODE_ROW - PAD));
        painter.fill_background(node_rect, 1.0, theme::control_background());
        widgets::text_in(
            painter,
            node_rect.shrink(PAD / 2.0),
            Align2::CENTER_CENTER,
            node,
            theme::ui_font(theme::TEXT_TINY),
            theme::text_secondary(),
        );
        if graph.linear && index + 1 < graph.nodes.len() {
            widgets::text_in(
                painter,
                Rect::from_min_size(Pos2::new(node_rect.right(), row.top()), Vec2::new(PAD, NODE_ROW)),
                Align2::CENTER_CENTER,
                "›",
                theme::ui_font(theme::TEXT),
                theme::text_hint(),
            );
        }
        row_top += NODE_ROW;
    }
}

/// An `.sd` colour.
fn rgb([r, g, b]: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}

/// One parameter in its cell `row`: its panel colour, its label, its control and its value, as
/// its `ui` fields say. A change is one `device.setParam`.
fn parameter_cell(
    ui: &mut Ui,
    painter: &egui::Painter,
    row: Rect,
    parameter: &GraphParameter,
    track: TrackId,
    at: &DeviceAt,
    actions: &mut Vec<Action>,
) {
    let style = &parameter.ui;
    let family = if style.monospace { egui::FontFamily::Monospace } else { egui::FontFamily::Proportional };
    let font = egui::FontId::new(style.font_size, family);
    let foreground = style.text_color.map_or_else(theme::text_secondary, rgb);
    let radius = match style.shape {
        ControlShape::Square => 0.0,
        ControlShape::Pill => 128.0,
        ControlShape::Rounded => style.corner_radius,
    };
    if let Some(color) = style.panel_color {
        painter.rect_filled(row, radius, rgb(color));
    }
    let row = row.shrink(4.0);
    let name_rect = match style.label_position {
        LabelPosition::Left => Rect::from_min_size(row.min, Vec2::new(row.width() * 0.34, row.height())),
        LabelPosition::Top => Rect::from_min_size(row.min, Vec2::new(row.width(), style.font_size + 4.0)),
        LabelPosition::Hidden => Rect::from_min_size(row.min, Vec2::ZERO),
    };
    if style.label_position != LabelPosition::Hidden {
        let align =
            if style.label_position == LabelPosition::Top { Align2::CENTER_CENTER } else { Align2::LEFT_CENTER };
        let name = if style.group.is_empty() {
            parameter.name.clone()
        } else {
            format!("{} / {}", style.group, parameter.name)
        };
        widgets::text_in(painter, name_rect, align, &name, font.clone(), foreground);
    }
    let value_height =
        if style.show_value && style.label_position == LabelPosition::Top { style.font_size + 4.0 } else { 0.0 };
    let value_width = if style.show_value && value_height == 0.0 { (row.width() * 0.24).min(80.0) } else { 0.0 };
    let control_min = match style.label_position {
        LabelPosition::Left => Pos2::new(name_rect.right(), row.top()),
        LabelPosition::Top => Pos2::new(row.left(), name_rect.bottom()),
        LabelPosition::Hidden => row.min,
    };
    let slider_rect =
        Rect::from_min_max(control_min, Pos2::new(row.right() - value_width, row.bottom() - value_height));
    let mut value = parameter.value;
    let response = (parameter.min < parameter.max).then(|| {
        let builder = egui::UiBuilder::new()
            .max_rect(slider_rect)
            .layout(egui::Layout::centered_and_justified(egui::Direction::TopDown));
        ui.scope_builder(builder, |ui| {
            ui.style_mut().override_font_id = Some(font.clone());
            let accent = style.accent.map(rgb);
            let visuals = ui.visuals_mut();
            visuals.override_text_color = Some(foreground);
            if let Some(accent) = accent {
                visuals.selection.bg_fill = accent;
                visuals.widgets.active.bg_fill = accent;
                visuals.widgets.hovered.bg_stroke.color = accent;
                visuals.widgets.active.fg_stroke.color = accent;
            }
            for widget in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active] {
                widget.corner_radius = egui::CornerRadius::same(radius as u8);
                if let Some(background) = style.background {
                    widget.bg_fill = rgb(background);
                    widget.weak_bg_fill = widget.bg_fill;
                }
                if let Some(accent) = accent {
                    widget.fg_stroke.color = accent;
                }
            }
            ui.set_clip_rect(ui.clip_rect().intersect(slider_rect));
            ui.spacing_mut().slider_width = if parameter.control == ParameterControl::VerticalSlider {
                slider_rect.height()
            } else {
                slider_rect.width()
            };
            ui.push_id((&parameter.id, at, track.0), |ui| {
                let previous = value;
                let mut response = parameter_widget(ui, parameter, slider_rect, accent, &mut value);
                if response.double_clicked() {
                    value = parameter.default;
                }
                if value != previous {
                    response.mark_changed();
                }
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, &parameter.name));
                response.on_hover_text(&style.tooltip)
            })
            .inner
        })
        .inner
    });
    if style.show_value {
        let (rect, align) = if value_height > 0.0 {
            (Rect::from_min_max(Pos2::new(row.left(), slider_rect.bottom()), row.max), Align2::CENTER_CENTER)
        } else {
            (Rect::from_min_max(Pos2::new(slider_rect.right(), control_min.y), row.max), Align2::RIGHT_CENTER)
        };
        let color = style.text_color.map_or_else(theme::text_value, rgb);
        widgets::text_in(painter, rect, align, &value_text(parameter, value), font, color);
    }
    if let Some(response) = response.filter(|r| r.changed() || r.drag_stopped()) {
        let continuous = (response.drag_started() || response.dragged()) && !response.drag_stopped();
        actions.push(Action::new(
            "device.setParam",
            json!({ "track": track.0, "device": at.value(), "param": parameter.id, "value": value, "continuous": continuous }),
        ));
    }
}

/// The text beside a parameter's control at `value`: a choice's label, the device's own text
/// for the value it listed, or the number with its unit.
fn value_text(parameter: &GraphParameter, value: f64) -> String {
    if matches!(parameter.control, ParameterControl::Choice | ParameterControl::Radio) {
        parameter.ui.options.get(value as usize).cloned().unwrap_or_default()
    } else if !parameter.text.is_empty() && value == parameter.value {
        parameter.text.clone()
    } else {
        format!("{:.*} {}", parameter.ui.decimals, value, parameter.unit).trim().to_owned()
    }
}

/// The control `parameter.control` names, in `rect`, editing `value`.
fn parameter_widget(
    ui: &mut Ui,
    parameter: &GraphParameter,
    rect: Rect,
    accent: Option<egui::Color32>,
    value: &mut f64,
) -> egui::Response {
    let style = &parameter.ui;
    match parameter.control {
        ParameterControl::Toggle => {
            let mut checked = *value >= 0.5;
            let response = ui.checkbox(&mut checked, "");
            *value = f64::from(checked);
            response
        }
        ParameterControl::Button => {
            let response = ui.selectable_label(*value >= 0.5, &parameter.name);
            if response.clicked() {
                *value = if *value >= 0.5 { 0.0 } else { 1.0 };
            }
            response
        }
        ParameterControl::Choice => {
            widgets::combo_box(
                ui,
                egui::ComboBox::from_id_salt("choice")
                    .selected_text(style.options.get(*value as usize).map_or("", String::as_str))
                    .width(rect.width()),
                |ui| {
                    for (i, label) in style.options.iter().enumerate() {
                        ui.selectable_value(&mut *value, i as f64, label);
                    }
                },
            )
            .response
        }
        ParameterControl::Radio => {
            ui.vertical_centered(|ui| {
                for (i, label) in style.options.iter().enumerate() {
                    ui.selectable_value(&mut *value, i as f64, label);
                }
            })
            .response
        }
        ParameterControl::Number => widgets::number_input(
            ui,
            None,
            egui::DragValue::new(value)
                .range(parameter.min..=parameter.max)
                .speed(if style.step > 0.0 {
                    style.step
                } else {
                    (parameter.max - parameter.min) / widgets::PARAMETER_DRAG_PIXELS
                })
                .max_decimals(style.decimals),
        ),
        ParameterControl::Knob | ParameterControl::LogKnob => knob(ui, parameter, rect, accent, value),
        // `.slider { --suhina-appearance: knob }` draws a slider as a knob.
        ParameterControl::Slider | ParameterControl::LogSlider | ParameterControl::Integer if slider_as_knob() => {
            knob(ui, parameter, rect, accent, value)
        }
        ParameterControl::VerticalSlider => {
            let rect = Rect::from_center_size(rect.center(), Vec2::new(ui.spacing().interact_size.y, rect.height()));
            let mut slider_ui =
                ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Center)));
            slider_ui.add(
                egui::Slider::new(value, parameter.min..=parameter.max)
                    .vertical()
                    .show_value(false)
                    .trailing_fill(style.accent.is_some())
                    .max_decimals(style.decimals)
                    .step_by(style.step),
            )
        }
        control if themed_slider() => themed_slider_widget(ui, parameter, rect, control, value),
        control => ui.add(
            egui::Slider::new(value, parameter.min..=parameter.max)
                .show_value(false)
                .trailing_fill(style.accent.is_some())
                .max_decimals(style.decimals)
                .logarithmic(control == ParameterControl::LogSlider)
                .step_by(if control == ParameterControl::Integer { 1.0 } else { style.step }),
        ),
    }
}

/// Whether the theme draws parameter sliders as knobs (`.slider { --suhina-appearance: knob }`).
pub(crate) fn slider_as_knob() -> bool {
    let _style = crate::kit::style_class::enter("slider");
    crate::kit::style_class::knob_preference(false)
}

/// Whether the theme gives sliders parts of their own: `.slider-rail`, `.slider-fill` or
/// `.slider-handle` with a background, or `slider.rail` / `slider.fill` / `slider.handle` artwork.
pub(crate) fn themed_slider() -> bool {
    let _style = crate::kit::style_class::enter("slider");
    ["slider-rail", "slider-fill", "slider-handle"].into_iter().any(|class| {
        let _part = crate::kit::style_class::enter(class);
        crate::kit::style_class::computed().is_some_and(|c| c.authored.background || c.has_gradient())
    }) || ["slider.rail", "slider.fill", "slider.handle"].into_iter().any(crate::kit::skin::has_own)
}

/// A parameter slider drawn from the theme's parts: the rail (`.slider-rail`, `height` its
/// thickness), the fill up to the value (`.slider-fill`) and the handle (`.slider-handle`,
/// `width`), each also as artwork. Dragged sideways, back to its default on a double click.
fn themed_slider_widget(
    ui: &mut Ui,
    parameter: &GraphParameter,
    rect: Rect,
    control: ParameterControl,
    value: &mut f64,
) -> egui::Response {
    use crate::kit::skin::{self, State};
    let (min, max) = (parameter.min, parameter.max);
    let logarithmic = control == ParameterControl::LogSlider && min > 0.0 && max > min;
    let height = ui.spacing().interact_size.y.min(rect.height()).max(8.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(rect.width(), height), Sense::click_and_drag());
    let _style = crate::kit::style_class::enter_response("slider", &response);
    let to_position = |v: f64| {
        if logarithmic { (v / min).ln() / (max / min).ln() } else { (v - min) / (max - min).max(f64::EPSILON) }
    };
    let handle_width = {
        let _handle = crate::kit::style_class::enter("slider-handle");
        crate::kit::style_class::computed().and_then(|c| c.width).unwrap_or(8.0).clamp(2.0, rect.width().max(2.0))
    };
    let travel = (rect.width() - handle_width).max(1.0);
    if let Some(pointer) = response.interact_pointer_pos().filter(|_| response.dragged() || response.clicked()) {
        let position = f64::from(((pointer.x - rect.left() - handle_width / 2.0) / travel).clamp(0.0, 1.0));
        let mut next = if logarithmic { min * (max / min).powf(position) } else { min + position * (max - min) };
        let step = if control == ParameterControl::Integer { 1.0 } else { parameter.ui.step };
        if step > 0.0 {
            next = (min + ((next - min) / step).round() * step).clamp(min, max);
        }
        *value = next;
    }
    let position = to_position(*value).clamp(0.0, 1.0) as f32;
    let state = State::response(&response, false);
    let painter = ui.painter_at(rect);
    let rail = {
        let _rail = crate::kit::style_class::enter("slider-rail");
        let thickness = crate::kit::native_css::thickness(4.0).clamp(1.0, rect.height());
        let rail = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), thickness));
        if !(skin::has_own("slider.rail") && skin::paint(&painter, rail, "slider.rail", state, theme::text_hint(), 0.0))
        {
            crate::kit::native_css::surface(&painter, rail, thickness / 2.0, theme::control_background());
        }
        rail
    };
    let at = rect.left() + handle_width / 2.0 + travel * position;
    {
        let _fill = crate::kit::style_class::enter("slider-fill");
        let fill = Rect::from_min_max(rail.min, Pos2::new(at, rail.bottom()));
        if fill.width() >= 1.0
            && !(skin::has_own("slider.fill")
                && skin::paint(&painter, fill, "slider.fill", state, theme::accent(), 0.0))
        {
            crate::kit::native_css::surface(&painter, fill, rail.height() / 2.0, theme::control_selected());
        }
    }
    {
        let _handle = crate::kit::style_class::enter_response("slider-handle", &response);
        let handle = Rect::from_center_size(Pos2::new(at, rect.center().y), Vec2::new(handle_width, rect.height()));
        if !(skin::has_own("slider.handle")
            && skin::paint(&painter, handle, "slider.handle", state, theme::text_primary(), 0.0))
        {
            crate::kit::native_css::surface(&painter, handle, 2.0, theme::text_primary());
        }
    }
    response
}

/// A knob in `rect`, logarithmic for a `LogKnob`: dragged up and down (finer with Shift),
/// stepped by the arrow keys once clicked, back to its default on a double click.
fn knob(
    ui: &mut Ui,
    parameter: &GraphParameter,
    rect: Rect,
    accent: Option<egui::Color32>,
    value: &mut f64,
) -> egui::Response {
    let style = &parameter.ui;
    let (min, max) = (parameter.min, parameter.max);
    let logarithmic = parameter.control == ParameterControl::LogKnob;
    let size = rect.width().min(rect.height()).max(8.0);
    let (rect, mut response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
    if response.clicked() {
        response.request_focus();
    }
    if response.double_clicked() {
        *value = parameter.default;
    }
    if response.has_focus() {
        let direction = ui.input(|i| {
            i32::from(i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowRight))
                - i32::from(i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowLeft))
        });
        let step = if style.step > 0.0 { style.step } else { (max - min) * 0.01 };
        *value = (*value + f64::from(direction) * step).clamp(min, max);
    }
    let mut position = if logarithmic { (*value / min).ln() / (max / min).ln() } else { (*value - min) / (max - min) };
    if response.dragged() {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let delta = f64::from(response.drag_delta().y) / widgets::PARAMETER_DRAG_PIXELS * fine;
        position = (position - delta).clamp(0.0, 1.0);
        *value = if logarithmic { min * (max / min).powf(position) } else { min + position * (max - min) };
        response.mark_changed();
    }
    let center = rect.center();
    let radius = size * 9.0 / 22.0;
    let background = style.background.map_or_else(theme::control_background, rgb);
    let border = egui::Stroke::new(1.0, accent.unwrap_or_else(theme::text_hint));
    let painter = ui.painter();
    if style.shape == ControlShape::Square {
        let square = Rect::from_center_size(center, Vec2::splat(radius * 2.0));
        painter.fill_background(square, 0.0, background);
        painter.rect_stroke(square, 0.0, border, egui::StrokeKind::Inside);
    } else {
        painter.circle_filled(center, radius, background);
        painter.circle_stroke(center, radius, border);
    }
    let angle = (-135.0 + 270.0 * position as f32).to_radians();
    let tip = center + Vec2::new(angle.sin(), -angle.cos()) * (radius - 1.0);
    painter.line_segment([center, tip], egui::Stroke::new(1.5, accent.unwrap_or_else(theme::text_primary)));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, &parameter.name));
    response
}

/// What a row of the clip column edits, when it edits something (#699).
#[derive(Clone, Copy, PartialEq)]
enum ClipEdit {
    None,
    Loop,
    LoopStart,
    LoopEnd,
    Gain,
    Warp,
    Mode,
    Transpose,
}

/// The warp modes in the order a click on the Mode row steps through them: the name
/// `clip.setAudio` takes, and the one shown.
const WARP_MODES: [(StretchMode, &str, &str); 6] = [
    (StretchMode::Repitch, "repitch", "Re-pitch"),
    (StretchMode::Rhythmic, "rhythmic", "Rhythmic"),
    (StretchMode::Tonal, "tonal", "Tonal"),
    (StretchMode::Texture, "texture", "Texture"),
    (StretchMode::Smooth, "smooth", "Smooth"),
    (StretchMode::HighQuality, "high-quality", "High quality"),
];

/// `params` with the clip at `at` named in them, for a `clip.*` command.
fn on_clip(at: ClipRef, params: serde_json::Value) -> serde_json::Value {
    let mut named = match at {
        ClipRef::Arrangement(_, clip) => json!({ "clip": clip.0 }),
        ClipRef::Slot(track, scene) => json!({ "track": track.0, "scene": scene + 1 }),
    };
    if let (Some(map), serde_json::Value::Object(more)) = (named.as_object_mut(), params) {
        map.extend(more);
    }
    named
}

/// A beat count as the loop rows show it.
fn beats_text(beats: f64) -> String {
    let rounded = (beats * 100.0).round() / 100.0;
    format!("{rounded} beats")
}

/// The selected clip: its name and where it is, how long, and what it plays; the loop, gain and
/// warp rows edit it (› Clip column in `docs/spec/ui-clip-editor.md`). In a row it takes a column
/// of [`CLIP_WIDTH`] at the left of `area`, stacked the rows it needs at the top; returns the
/// space it took.
fn clip_properties(
    ui: &Ui,
    painter: &egui::Painter,
    area: Rect,
    info: &ClipInfo,
    stacked: bool,
    actions: &mut Vec<Action>,
) -> (Rect, Option<Showing>) {
    let mut rows = vec![
        ("Track", info.track.clone(), ClipEdit::None),
        (if info.scene { "Scene" } else { "Start" }, info.start.clone(), ClipEdit::None),
        ("Length", info.length.clone(), ClipEdit::None),
        ("Loop", if info.looping { "On".into() } else { "Off".into() }, ClipEdit::Loop),
    ];
    // A MIDI clip's loop is dragged on the piano roll's ruler; an audio clip has no editor yet,
    // so its loop is set here.
    if let (Some((start, end)), true) = (info.loop_range, info.audio) {
        rows.push(("Loop start", beats_text(start), ClipEdit::LoopStart));
        rows.push(("Loop end", beats_text(end), ClipEdit::LoopEnd));
    }
    if info.audio {
        // A slot's clip has no gain command of its own yet: its gain is shown, not edited.
        let gain_edit = if matches!(info.at, Some(ClipRef::Arrangement(..))) { ClipEdit::Gain } else { ClipEdit::None };
        rows.push((
            "Gain",
            info.gain_db.map_or_else(|| "-".into(), |db| format!("{} dB", widgets::format_db(db))),
            gain_edit,
        ));
        rows.push(("Warp", if info.warped { "On".into() } else { "Off".into() }, ClipEdit::Warp));
        let mode = WARP_MODES.iter().find(|(m, _, _)| *m == info.stretch).map_or("Smooth", |(_, _, shown)| shown);
        rows.push(("Mode", mode.into(), ClipEdit::Mode));
        rows.push(("Transpose", transpose_text(info.transpose), ClipEdit::Transpose));
        let beats = |b: f64| if b > 0.0 { format!("{} beats", (b * 100.0).round() / 100.0) } else { "-".into() };
        rows.push(("Fade in", beats(info.fades.0), ClipEdit::None));
        rows.push(("Fade out", beats(info.fades.1), ClipEdit::None));
    } else {
        // With notes selected the row says how many of the clip's notes they are, and how many of
        // them are muted (› Clip column in `docs/spec/ui-clip-editor.md`).
        let selection = &info.selection;
        let notes = if selection.selected == 0 {
            info.notes.to_string()
        } else if selection.muted == 0 {
            format!("{} of {}", selection.selected, info.notes)
        } else {
            format!("{} of {}, {} muted", selection.selected, info.notes, selection.muted)
        };
        rows.push(("Notes", notes, ClipEdit::None));
        if let Some(from) = &selection.from {
            rows.push(("From", from.clone(), ClipEdit::None));
        }
        if let Some(to) = &selection.to {
            rows.push(("To", to.clone(), ClipEdit::None));
        }
        if let (Some(low), Some(high)) = (selection.low_key, selection.high_key) {
            let one = low == high;
            let low_name = crate::detail::piano_roll::key_name(low);
            rows.push((
                "Pitch",
                if one { low_name } else { format!("{low_name}–{}", crate::detail::piano_roll::key_name(high)) },
                ClipEdit::None,
            ));
        }
        if let (Some(low), Some(high)) = (selection.low_velocity, selection.high_velocity) {
            rows.push((
                "Velocity",
                if low == high { low.to_string() } else { format!("{low}–{high}") },
                ClipEdit::None,
            ));
        }
    }
    let column = if stacked {
        let height = ROW + PAD * 2.0 + rows.len() as f32 * PARAM_ROW;
        Rect::from_min_size(area.min, Vec2::new(area.width(), height))
    } else {
        // `.clip-info { width }` in the theme (120 to 480 points).
        let width = {
            let _style = crate::kit::style_class::enter("clip-info");
            crate::kit::style_class::computed().and_then(|c| c.width).map_or(CLIP_WIDTH, |w| w.clamp(120.0, 480.0))
        };
        Rect::from_min_max(area.min, Pos2::new(area.left() + width, area.bottom()))
    }
    .intersect(area);
    // `.clip-info` styles the clip's column: its background and its text.
    let _style = crate::kit::style_class::enter("clip-info");
    crate::kit::native_css::surface(painter, column, 0.0, theme::panel_background());
    // Rows that do not fit under the column's bottom are left out rather than cut in half, the
    // title last; text never wraps (› Clip column in `docs/spec/ui-clip-editor.md`).
    let title = Rect::from_min_size(column.min, Vec2::new(column.width(), ROW));
    if title.bottom() > column.bottom() + 0.5 {
        return (column, None);
    }
    // The Clip / Devices switch ends the title row; the name keeps clear of it.
    let (name, picked) = match info.switch {
        Some(showing) => {
            let width = (SWITCH_PART * 2.0 + 1.0).min(title.width() - PAD * 2.0);
            let at = Rect::from_center_size(
                Pos2::new(title.right() - PAD - width / 2.0, title.center().y),
                Vec2::new(width, SWITCH_HEIGHT),
            );
            let picked = switch(ui, painter, at, showing);
            (Rect::from_min_max(title.min, Pos2::new(at.left() - PAD, title.bottom())), picked)
        }
        None => (title, None),
    };
    widgets::text_in(
        painter,
        name.shrink2(Vec2::new(PAD, 0.0)),
        Align2::LEFT_CENTER,
        &info.name,
        theme::ui_font(theme::TEXT),
        theme::text_primary(),
    );
    let mut y = title.bottom() + PAD;
    for (label, value, edit) in rows {
        let row = Rect::from_min_max(Pos2::new(column.left() + PAD, y), Pos2::new(column.right() - PAD, y + PARAM_ROW));
        y += PARAM_ROW;
        if row.bottom() > column.bottom() + 0.5 {
            break;
        }
        // The value keeps clear of the label, so a long track name is shortened, not drawn over it.
        let value_rect = Rect::from_min_max(Pos2::new(row.left() + LABEL_WIDTH, row.top()), row.max);
        if let (Some(at), true) = (info.at, edit != ClipEdit::None) {
            clip_edit_row(ui, painter, row, value_rect, (label, &value), edit, at, info, actions);
            continue;
        }
        // The row is named for a reader and a test ("Notes: 1 of 4"), as the piano roll's notes
        // are, so what the column says can be found by name.
        let response = ui.interact(row, egui::Id::new(("clip-property", label)), Sense::hover());
        let named = format!("{label}: {value}");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &named));
        widgets::text_in(
            painter,
            row,
            Align2::LEFT_CENTER,
            label,
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_secondary(),
        );
        widgets::text_in(
            painter,
            value_rect,
            Align2::RIGHT_CENTER,
            &value,
            theme::mono_font(theme::TEXT_SMALL),
            theme::text_value(),
        );
    }
    (column, picked)
}

/// Semitones as the Transpose row shows them.
fn transpose_text(semitones: f64) -> String {
    let rounded = (semitones * 100.0).round() / 100.0;
    if rounded > 0.0 { format!("+{rounded} st") } else { format!("{rounded} st") }
}

/// A row of the clip column that edits the clip: a click switches the loop or warp, or steps
/// the warp mode; the loop range, gain and transpose are dragged sideways (one command when the
/// drag ends, so a drag is one undo step) and a double-click puts gain and transpose back to 0.
#[allow(clippy::too_many_arguments)]
fn clip_edit_row(
    ui: &Ui,
    painter: &egui::Painter,
    row: Rect,
    value_rect: Rect,
    (label, value): (&str, &str),
    edit: ClipEdit,
    at: ClipRef,
    info: &ClipInfo,
    actions: &mut Vec<Action>,
) {
    widgets::text_in(
        painter,
        row,
        Align2::LEFT_CENTER,
        label,
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_secondary(),
    );
    let id = Id::new(("clip-edit", label));
    let draw_value = |response: &egui::Response, text: &str| {
        let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, response));
        painter.fill_background(value_rect, 2.0, fill);
        widgets::text_in(
            painter,
            value_rect.shrink2(Vec2::new(PAD / 2.0, 0.0)),
            Align2::RIGHT_CENTER,
            text,
            theme::mono_font(theme::TEXT_SMALL),
            theme::text_value(),
        );
    };
    if matches!(edit, ClipEdit::Loop | ClipEdit::Warp | ClipEdit::Mode) {
        let response = widgets::button_area(ui, value_rect, id, format!("{label}: {value}"));
        draw_value(&response, value);
        if response.clicked() {
            actions.push(match edit {
                ClipEdit::Loop => Action::new("clip.setLoop", on_clip(at, json!({ "loop": !info.looping }))),
                ClipEdit::Warp => Action::new("clip.setAudio", on_clip(at, json!({ "warp": !info.warped }))),
                _ => {
                    let index = WARP_MODES.iter().position(|(m, _, _)| *m == info.stretch).unwrap_or(4);
                    let next = WARP_MODES[(index + 1) % WARP_MODES.len()].1;
                    Action::new("clip.setAudio", on_clip(at, json!({ "mode": next })))
                }
            });
        }
        return;
    }
    // Dragged: shown from a draft while the drag lasts. The loop's ends keep a quarter beat
    // apart and move by quarter beats, the transpose by semitones, the gain by tenths of a dB.
    let (start, end) = info.loop_range.unwrap_or((0.0, 4.0));
    let (min, max) = loop_bounds(edit, start, end);
    let (stored, per_point, min, max, reset) = match edit {
        ClipEdit::LoopStart => (start, 0.0625, min, max, None),
        ClipEdit::LoopEnd => (end, 0.0625, min, max, None),
        ClipEdit::Gain => {
            (info.gain_db.filter(|db| db.is_finite()).unwrap_or(-70.0).max(-70.0), 0.1, -70.0, 24.0, Some(0.0))
        }
        _ => (info.transpose, 0.1, -48.0, 48.0, Some(0.0)),
    };
    // Rounding can step past a bound (a start of 3.85 rounds to 4.0 below an end of 4.1): kept
    // inside after it too.
    let snap = |v: f64| {
        match edit {
            ClipEdit::LoopStart | ClipEdit::LoopEnd => (v * 4.0).round() / 4.0,
            ClipEdit::Transpose => v.round(),
            _ => (v * 10.0).round() / 10.0,
        }
        .clamp(min, max)
    };
    let draft = ui.data(|d| d.get_temp::<f64>(id));
    let text = match draft.map(snap) {
        None => value.to_string(),
        Some(shown) => match edit {
            ClipEdit::Gain => format!("{} dB", widgets::format_db(shown)),
            ClipEdit::Transpose => transpose_text(shown),
            _ => beats_text(shown),
        },
    };
    let hint = if reset.is_some() { "Drag sideways; double-click resets" } else { "Drag sideways" };
    let response = ui.interact(value_rect, id.with("drag"), Sense::click_and_drag()).on_hover_text(hint);
    let named = format!("{label}: {text}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, &named));
    draw_value(&response, &text);
    let moved = |from: f64| snap((from + f64::from(response.drag_delta().x) * per_point).clamp(min, max));
    let set = if response.double_clicked() {
        ui.data_mut(|d| d.remove::<f64>(id));
        reset
    } else if response.drag_stopped() {
        ui.data_mut(|d| d.remove::<f64>(id));
        Some(moved(draft.unwrap_or(stored)))
    } else {
        if response.dragged() {
            let next = (draft.unwrap_or(stored) + f64::from(response.drag_delta().x) * per_point).clamp(min, max);
            ui.data_mut(|d| d.insert_temp(id, next));
        }
        None
    };
    let Some(value) = set.filter(|v| (v - stored).abs() > 1e-9) else { return };
    actions.push(match edit {
        ClipEdit::LoopStart => Action::new("clip.setLoop", on_clip(at, json!({ "start": value }))),
        ClipEdit::LoopEnd => Action::new("clip.setLoop", on_clip(at, json!({ "end": value }))),
        ClipEdit::Gain => {
            let db = if value <= -70.0 { json!("-inf") } else { json!(value) };
            Action::new("clip.setGain", on_clip(at, json!({ "db": db })))
        }
        _ => Action::new("clip.setAudio", on_clip(at, json!({ "transpose": value }))),
    });
}

/// The Clip / Devices switch in `rect`: two equal parts 1 px apart, the one on light
/// (`docs/design/components.md` › Clip properties). Returns the part clicked, when it was off.
fn switch(ui: &Ui, painter: &egui::Painter, rect: Rect, showing: Showing) -> Option<Showing> {
    let part = (rect.width() - 1.0) / 2.0;
    let mut picked = None;
    for (index, (text, value)) in [("Clip", Showing::Clip), ("Devices", Showing::Devices)].into_iter().enumerate() {
        let cell = Rect::from_min_size(
            Pos2::new(rect.left() + index as f32 * (part + 1.0), rect.top()),
            Vec2::new(part, rect.height()),
        );
        let response = widgets::button_area(ui, cell, egui::Id::new(("detail-switch", index)), format!("Show {text}"));
        if response.clicked() && value != showing {
            picked = Some(value);
        }
        // `.editor-tab` (`:checked` for the one shown), `#tab-clip` and `#tab-devices`.
        let _style = crate::kit::style_class::enter_named_response(
            "editor-tab",
            &format!("tab-{}", text.to_ascii_lowercase()),
            &response,
        );
        crate::kit::native_css::mark(crate::kit::native_css::HostState::Checked, value == showing);
        let on = motion::toward(ui.ctx(), response.id.with("on"), value == showing, motion::VALUE);
        let idle = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        crate::kit::native_css::surface(painter, cell, 0.0, motion::mix(idle, theme::control_selected(), on));
        let color =
            crate::kit::style_class::foreground(motion::mix(theme::text_secondary(), theme::text_inverse(), on));
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_SMALL), color);
    }
    picked
}

fn parameter_height(parameter: &GraphParameter) -> f32 {
    if let Some([_, height]) = parameter.ui.size {
        return height;
    }
    let extra = if parameter.ui.label_position == LabelPosition::Top { parameter.ui.font_size + 4.0 } else { 0.0 };
    extra
        + if parameter.ui.show_value && parameter.ui.label_position == LabelPosition::Top {
            parameter.ui.font_size + 4.0
        } else {
            0.0
        }
        + match parameter.control {
            ParameterControl::VerticalSlider => 80.0,
            ParameterControl::Knob | ParameterControl::LogKnob => 36.0,
            ParameterControl::Radio => parameter.ui.options.len().max(1) as f32 * (parameter.ui.font_size + 16.0),
            _ => 24.0,
        }
}

fn parameter_layout(parameters: &[GraphParameter], width: f32, hidden: &[String]) -> Vec<Rect> {
    let mut bottom = 0.0f32;
    parameters
        .iter()
        .filter(|p| !hidden.contains(&p.id))
        .map(|parameter| {
            let [x, y] = parameter.ui.position.unwrap_or([0.0, bottom]);
            let [w, h] = parameter.ui.size.unwrap_or([width, parameter_height(parameter)]);
            let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h));
            bottom = bottom.max(rect.bottom());
            rect
        })
        .collect()
}

#[cfg(test)]
#[path = "../../../device-sdk/examples/ui_controls.rs"]
mod styled_gallery;

#[cfg(test)]
#[path = "../../../device-sdk/examples/orbit_filter.rs"]
mod custom_panel;

/// Width of a device group's panel in a row, and of its Mix and Gain in the header.
const GROUP_WIDTH: f32 = 260.0;
const MIX_BOX: f32 = 62.0;
const GAIN_BOX: f32 = 66.0;
/// Height of one branch's row in a group's panel, and of its Mix and Gain rows.
const BRANCH_ROW: f32 = 20.0;
/// Width of a branch's M and S buttons, and of its volume and pan values.
const BRANCH_BUTTON: f32 = 16.0;
const BRANCH_VALUE: f32 = 44.0;

/// The branch of the group at `at` whose devices the chain shows, kept per group while the app
/// runs (the first one until another is clicked).
fn shown_branch(ui: &Ui, track: TrackId, at: &DeviceAt, branches: usize) -> usize {
    let id = Id::new(("group-branch", track.0, at));
    ui.data(|d| d.get_temp::<usize>(id)).unwrap_or(0).min(branches.saturating_sub(1))
}

/// The devices of `devices` as the chain shows them, in order: each device, and after a group
/// the devices of its shown branch, nested groups the same way.
fn flattened<'a>(ui: &Ui, track: TrackId, devices: &'a [DeviceBox], out: &mut Vec<&'a DeviceBox>) {
    for device in devices {
        out.push(device);
        if let Some(group) = &device.group
            && let Some(branch) = group.branches.get(shown_branch(ui, track, &device.at, group.branches.len()))
        {
            flattened(ui, track, &branch.devices, out);
        }
    }
}

/// How tall a group's panel is: its header (with Mix and Gain), a row per branch and the row
/// that adds one.
fn group_height(group: &GroupBox) -> f32 {
    ROW + PAD + (group.branches.len() + 1) as f32 * BRANCH_ROW + PAD
}

/// A value dragged sideways in a group's panel: the change a drag this frame makes, in
/// `per_point` units per point, and whether the drag is still on. A double-click asks for the
/// default.
struct Dragged {
    delta: f64,
    dragging: bool,
    ended: bool,
    reset: bool,
}

fn value_row(ui: &Ui, painter: &egui::Painter, rect: Rect, id: Id, label: &str, text: &str, per_point: f64) -> Dragged {
    let response = ui.interact(rect, id, Sense::click_and_drag()).on_hover_text("Drag sideways; double-click resets");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, format!("{label} {text}")));
    let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
    painter.fill_background(rect, 0.0, fill);
    let inner = rect.shrink2(Vec2::new(PAD / 2.0, 0.0));
    if !label.is_empty() {
        widgets::text_in(
            painter,
            inner,
            Align2::LEFT_CENTER,
            label,
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_hint(),
        );
    }
    let align = if label.is_empty() { Align2::CENTER_CENTER } else { Align2::RIGHT_CENTER };
    widgets::text_in(painter, inner, align, text, theme::ui_font(theme::TEXT_SMALL), theme::text_value());
    Dragged {
        delta: f64::from(response.drag_delta().x) * per_point,
        dragging: response.dragged() && !response.drag_stopped(),
        ended: response.drag_stopped(),
        reset: response.double_clicked(),
    }
}

/// A branch value (volume or pan) dragged: shown from a draft while the drag lasts and set with
/// one command when it ends, so a drag is one undo step.
fn branch_value(ui: &Ui, id: Id, stored: f64, dragged: &Dragged, min: f64, max: f64) -> (f64, Option<f64>) {
    let draft = ui.data(|d| d.get_temp::<f64>(id));
    if dragged.reset {
        ui.data_mut(|d| d.remove::<f64>(id));
        // Both reset to 0: 0 dB, or the centre.
        return (stored, Some(0.0f64.clamp(min, max)));
    }
    if dragged.dragging {
        let next = (draft.unwrap_or(stored) + dragged.delta).clamp(min, max);
        ui.data_mut(|d| d.insert_temp(id, next));
        return (next, None);
    }
    if dragged.ended {
        ui.data_mut(|d| d.remove::<f64>(id));
        let value = (draft.unwrap_or(stored) + dragged.delta).clamp(min, max);
        return (value, (value != stored).then_some(value));
    }
    (draft.unwrap_or(stored), None)
}

/// Where a loop's start or end may be dragged: the start from 0 to a quarter beat before the end,
/// the end from a quarter beat after the start. Never crossed (`max` at least `min`), so a loop
/// shorter than that (from a command or an import) does not make `f64::clamp` panic.
fn loop_bounds(edit: ClipEdit, start: f64, end: f64) -> (f64, f64) {
    let (min, max) = match edit {
        ClipEdit::LoopStart => (0.0, (end - 0.25).min(120_000.0)),
        // Never past the longest loop, even for a start within a quarter beat of it.
        _ => ((start + 0.25).min(120_000.0), 120_000.0),
    };
    let min = if min.is_finite() { min } else { 0.0 };
    (min, if max.is_finite() { max.max(min) } else { min })
}

fn pan_text(pan: f64) -> String {
    let amount = (pan.abs() * 50.0).round();
    if amount < 1.0 {
        "C".into()
    } else if pan < 0.0 {
        format!("{amount}L")
    } else {
        format!("{amount}R")
    }
}

fn db_text(db: f64) -> String {
    if db.is_finite() { format!("{db:.1} dB") } else { "-inf dB".into() }
}

/// A device group's panel: its header (switch, name, Mix, Gain, remove), one row per branch
/// (its name, which shows its devices after the panel, its volume and pan, M and S) and a row
/// that adds a branch (`docs/spec/device-groups.md` › The device panel). Returns whether the
/// panel was clicked.
#[allow(clippy::too_many_arguments)]
fn group_panel(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    track: TrackId,
    device: &DeviceBox,
    group: &GroupBox,
    selected: bool,
    actions: &mut Vec<Action>,
) -> bool {
    let at = device.at.value();
    let panel = ui
        .interact(rect, Id::new(("device-panel", track.0, &device.at)), Sense::click_and_drag())
        .on_hover_text("Right-click for the group's menu; drag onto another track to move it there, Ctrl to copy it");
    if panel.drag_started_by(egui::PointerButton::Primary) && device.at.top() {
        crate::interaction::drop::start_device_carry(ui.ctx(), track, device.index);
    }
    panel.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Device {}", device.name)));
    let clicked = panel.clicked();
    panel.context_menu(|ui| menus::device_menu(ui, track, &device.at, Some(group.branches.len()), actions));
    painter.fill_background(rect, 0.0, theme::panel_background());
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW));
    painter.fill_background(header, 0.0, theme::control_background());
    let switch = Rect::from_min_size(
        Pos2::new(header.left() + PAD, header.center().y - ON_SWITCH / 2.0),
        Vec2::splat(ON_SWITCH),
    );
    let label = if device.enabled { format!("Bypass {}", device.name) } else { format!("Enable {}", device.name) };
    let bypass = widgets::button_area(ui, switch, Id::new(("device-bypass", track.0, &device.at)), &label);
    if bypass.clicked() {
        actions.push(Action::new(
            "device.setEnabled",
            json!({ "track": track.0, "device": at, "enabled": !device.enabled }),
        ));
    }
    if device.enabled {
        painter.rect_filled(switch, 0.0, theme::text_primary());
    } else {
        painter.rect_stroke(switch, 0.0, theme::hairline(theme::text_hint()), egui::StrokeKind::Inside);
    }
    let close = Rect::from_center_size(Pos2::new(header.right() - PAD - 6.0, header.center().y), Vec2::splat(12.0));
    let remove = widgets::button_area(
        ui,
        close,
        Id::new(("device-remove", track.0, &device.at)),
        format!("Remove {}", device.name),
    );
    if remove.clicked() {
        actions.push(Action::new("device.remove", json!({ "track": track.0, "device": at })));
    }
    let cross = if remove.hovered() { theme::text_primary() } else { theme::text_hint() };
    for (from, to) in [(Vec2::new(-3.5, -3.5), Vec2::new(3.5, 3.5)), (Vec2::new(-3.5, 3.5), Vec2::new(3.5, -3.5))] {
        painter.line_segment([close.center() + from, close.center() + to], egui::Stroke::new(1.2, cross));
    }
    // Mix and Gain sit in the header, each dragged sideways; the engine smooths what they send.
    let gain_row = Rect::from_min_max(
        Pos2::new(close.left() - 4.0 - GAIN_BOX, header.top() + 2.0),
        Pos2::new(close.left() - 4.0, header.bottom() - 2.0),
    );
    let mix_row = Rect::from_min_max(
        Pos2::new(gain_row.left() - 2.0 - MIX_BOX, gain_row.top()),
        Pos2::new(gain_row.left() - 2.0, gain_row.bottom()),
    );
    let name_color = if device.enabled { theme::text_primary() } else { theme::text_hint() };
    let name = Rect::from_min_max(
        Pos2::new(switch.right() + PAD, header.top()),
        Pos2::new((mix_row.left() - PAD).max(switch.right() + PAD), header.bottom()),
    );
    widgets::text_in(painter, name, Align2::LEFT_CENTER, &device.name, theme::ui_font(theme::TEXT), name_color);
    let mix = value_row(
        ui,
        painter,
        mix_row,
        Id::new(("group-mix", track.0, &device.at)),
        "Mix",
        &format!("{:.0}%", group.mix * 100.0),
        0.005,
    );
    let mix_value = if mix.reset {
        Some(1.0)
    } else {
        (mix.delta != 0.0 || mix.ended).then(|| (group.mix + mix.delta).clamp(0.0, 1.0))
    };
    if let Some(value) = mix_value {
        actions.push(Action::new(
            "device.setParam",
            json!({ "track": track.0, "device": at, "param": "mix", "value": value, "continuous": mix.dragging }),
        ));
    }
    let silent = group.gain_db <= suhina_engine::SILENT_DB;
    let gain = value_row(
        ui,
        painter,
        gain_row,
        Id::new(("group-gain", track.0, &device.at)),
        "Gain",
        &if silent { "-inf".into() } else { format!("{:.1}", group.gain_db) },
        0.1,
    );
    let gain_value = if gain.reset {
        Some(0.0)
    } else {
        (gain.delta != 0.0 || gain.ended).then(|| (group.gain_db + gain.delta).clamp(suhina_engine::SILENT_DB, 12.0))
    };
    if let Some(value) = gain_value {
        actions.push(Action::new(
            "device.setParam",
            json!({ "track": track.0, "device": at, "param": "gain", "value": value, "continuous": gain.dragging }),
        ));
    }
    let left = rect.left() + PAD;
    let width = rect.width() - PAD * 2.0;
    let mut y = header.bottom() + PAD;

    let shown = shown_branch(ui, track, &device.at, group.branches.len());
    for (index, branch) in group.branches.iter().enumerate() {
        let row = Rect::from_min_size(Pos2::new(left, y), Vec2::new(width, BRANCH_ROW - 2.0));
        y += BRANCH_ROW;
        let solo = Rect::from_min_size(
            Pos2::new(row.right() - BRANCH_BUTTON, row.top()),
            Vec2::new(BRANCH_BUTTON, row.height()),
        );
        let mute = solo.translate(Vec2::new(-BRANCH_BUTTON - 2.0, 0.0));
        let pan = Rect::from_min_size(Pos2::new(mute.left() - 2.0 - 32.0, row.top()), Vec2::new(32.0, row.height()));
        let volume = Rect::from_min_size(
            Pos2::new(pan.left() - 2.0 - BRANCH_VALUE, row.top()),
            Vec2::new(BRANCH_VALUE, row.height()),
        );
        let name = Rect::from_min_max(row.min, Pos2::new(volume.left() - 2.0, row.bottom()));
        let pick = widgets::button_area(
            ui,
            name,
            Id::new(("group-branch-name", track.0, &device.at, index)),
            format!("Show branch {}", branch.name),
        );
        if pick.clicked() {
            ui.data_mut(|d| d.insert_temp(Id::new(("group-branch", track.0, &device.at)), index));
        }
        let lit = index == shown;
        let fill = if lit {
            theme::control_active()
        } else {
            motion::mix(theme::window_background(), theme::control_hover(), widgets::hover(ui, &pick))
        };
        painter.fill_background(name, 0.0, fill);
        let color = if branch.mute { theme::text_hint() } else { theme::text_secondary() };
        widgets::text_in(
            painter,
            name.shrink2(Vec2::new(4.0, 0.0)),
            Align2::LEFT_CENTER,
            &branch.name,
            theme::ui_font(theme::TEXT_SMALL),
            color,
        );

        let volume_id = Id::new(("branch-volume", track.0, &device.at, index));
        let shown_db = ui.data(|d| d.get_temp::<f64>(volume_id)).unwrap_or(branch.volume_db);
        let dragged = value_row(ui, painter, volume, volume_id.with("drag"), "", &db_text(shown_db), 0.1);
        let stored = if branch.volume_db.is_finite() { branch.volume_db } else { -70.0 };
        let (_, set) = branch_value(ui, volume_id, stored, &dragged, -70.0, 6.0);
        if let Some(db) = set {
            actions.push(Action::new(
                "branch.setVolume",
                json!({ "track": track.0, "device": at, "branch": index, "db": if db <= -70.0 { json!("-inf") } else { json!(db) } }),
            ));
        }
        let pan_id = Id::new(("branch-pan", track.0, &device.at, index));
        let shown_pan = ui.data(|d| d.get_temp::<f64>(pan_id)).unwrap_or(branch.pan);
        let dragged = value_row(ui, painter, pan, pan_id.with("drag"), "", &pan_text(shown_pan), 0.01);
        let (_, set) = branch_value(ui, pan_id, branch.pan, &dragged, -1.0, 1.0);
        if let Some(value) = set {
            actions.push(Action::new(
                "branch.setPan",
                json!({ "track": track.0, "device": at, "branch": index, "pan": value }),
            ));
        }

        let response = widgets::button_area(
            ui,
            mute,
            Id::new(("branch-mute", track.0, &device.at, index)),
            if branch.mute { format!("Unmute branch {}", branch.name) } else { format!("Mute branch {}", branch.name) },
        );
        let (fill, text) = widgets::toggle_colors(ui, &response, branch.mute);
        painter.fill_background(mute, 0.0, fill);
        widgets::text_in(painter, mute, Align2::CENTER_CENTER, "M", theme::ui_font(theme::TEXT_SMALL), text);
        if response.clicked() {
            actions.push(Action::new(
                "branch.setMute",
                json!({ "track": track.0, "device": at, "branch": index, "mute": !branch.mute }),
            ));
        }
        let response = widgets::button_area(
            ui,
            solo,
            Id::new(("branch-solo", track.0, &device.at, index)),
            if branch.solo {
                format!("End solo on branch {}", branch.name)
            } else {
                format!("Solo branch {}", branch.name)
            },
        );
        let lit = motion::toward(ui.ctx(), response.id.with("on"), branch.solo, motion::VALUE);
        let off = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.fill_background(solo, 0.0, motion::mix(off, theme::accent(), lit));
        let text = motion::mix(theme::text_secondary(), theme::text_inverse(), lit);
        widgets::text_in(painter, solo, Align2::CENTER_CENTER, "S", theme::ui_font(theme::TEXT_SMALL), text);
        if response.clicked() {
            actions.push(Action::new(
                "branch.setSolo",
                json!({ "track": track.0, "device": at, "branch": index, "solo": !branch.solo }),
            ));
        }
    }
    let add = Rect::from_min_size(Pos2::new(left, y), Vec2::new(width, BRANCH_ROW - 2.0));
    let response = widgets::button_area(
        ui,
        add,
        Id::new(("branch-add", track.0, &device.at)),
        format!("Add a branch to {}", device.name),
    );
    let fill = motion::mix(theme::window_background(), theme::control_hover(), widgets::hover(ui, &response));
    painter.fill_background(add, 0.0, fill);
    widgets::text_in(
        painter,
        add,
        Align2::CENTER_CENTER,
        "+ Branch",
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_hint(),
    );
    if response.clicked() {
        actions.push(Action::new("branch.add", json!({ "track": track.0, "device": at })));
    }
    if selected {
        painter.rect_stroke(rect, 1.0, theme::hairline(theme::selection()), egui::StrokeKind::Inside);
    }
    clicked
}

/// The line over a device inside a group, so the devices after a group's panel read as its
/// shown branch: one step lower for each group it is in.
fn inside_group_mark(painter: &egui::Painter, rect: Rect, at: &DeviceAt) {
    let depth = at.0.len() / 2;
    if depth == 0 {
        return;
    }
    for level in 0..depth {
        let y = rect.top() + level as f32 * 3.0 + 1.0;
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            egui::Stroke::new(2.0, theme::accent()),
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_loop_shorter_than_a_quarter_beat_gives_bounds_that_do_not_cross() {
        use super::{ClipEdit, loop_bounds};
        // A loop ending at beat 0.1 (from `clip.setLoop` or an import): the start's bounds were
        // 0 to -0.15, and the first drag panicked in `f64::clamp`.
        let (min, max) = loop_bounds(ClipEdit::LoopStart, 0.0, 0.1);
        assert!(min <= max, "{min} > {max}");
        assert_eq!(0.3f64.clamp(min, max), 0.0);
        let (min, max) = loop_bounds(ClipEdit::LoopEnd, 119_999.9, 120_000.0);
        assert!(min <= max, "{min} > {max}");
        assert!(max <= 120_000.0 && min <= 120_000.0, "never past the longest loop: {min} to {max}");
        // A loop ending past the limit (an import) gives a start no further than it either.
        assert_eq!(loop_bounds(ClipEdit::LoopStart, 0.0, 130_000.0), (0.0, 120_000.0));
        assert_eq!(loop_bounds(ClipEdit::LoopStart, 0.0, 4.0), (0.0, 3.75));
        assert_eq!(loop_bounds(ClipEdit::LoopEnd, 1.0, 4.0), (1.25, 120_000.0));
    }

    #[test]
    fn sample_name_is_in_header_once() {
        let mut device =
            suhina_engine::model::Device::new("Sampler", suhina_engine::model::DeviceKind::Builtin("sampler".into()));
        assert_eq!(super::device_display_name(&device), "Sampler");
        device.set_sample_file("samples/Piano.wav");
        assert_eq!(super::device_display_name(&device), "Sampler \u{b7} Piano.wav");
        device.name = "Piano".into();
        assert_eq!(super::device_display_name(&device), "Piano");
    }

    use super::*;

    #[test]
    fn top_label_places_value_below_control_and_reserves_height() {
        let mut parameter = GraphParameter {
            text: String::new(),
            default: 1.0,
            control: suhina_device_sdk::ParameterControl::Knob,
            unit: String::new(),
            id: "gain".into(),
            name: "Gain".into(),
            min: 0.0,
            max: 2.0,
            value: 1.0,
            ui: suhina_device_sdk::ParameterUi::default(),
        };
        parameter.ui.label_position = suhina_device_sdk::LabelPosition::Top;
        let with_value = parameter_height(&parameter);
        parameter.ui.show_value = false;
        assert!(with_value > parameter_height(&parameter));
    }
    use crate::demo::demo_project;
    use suhina_engine::model::{Device, DeviceParam};

    #[test]
    fn a_selected_clip_shows_its_properties() {
        let project = demo_project();
        let beat_b = project.arrangement.lanes[0].clips[1].id;
        assert_eq!(project.arrangement.lanes[0].track, Some(TrackId(1)));
        let detail = build(&project, Some(TrackId(1)), Some(ClipRef::Arrangement(TrackId(1), beat_b)));
        let Detail::Clip(info, chain) = detail else { panic!("expected the clip") };
        assert_eq!(chain.track, TrackId(1));
        assert_eq!((info.name.as_str(), info.start.as_str(), info.length.as_str()), ("Beat B", "17.1", "12.0"));
        assert!(info.looping && !info.audio && !info.scene);
        assert_eq!(info.notes, 4);
    }

    /// The clip column's loop, warp and mode rows ask for the clip's commands (#699).
    #[test]
    fn the_clip_column_edits_the_clip() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        let project = demo_project();
        let beat_b = project.arrangement.lanes[0].clips[1].id;
        let Detail::Clip(mut info, chain) =
            build(&project, Some(TrackId(1)), Some(ClipRef::Arrangement(TrackId(1), beat_b)))
        else {
            panic!("expected the clip")
        };
        assert_eq!(info.at, Some(ClipRef::Arrangement(TrackId(1), beat_b)));
        // Shown as an audio clip, to reach the warp rows.
        info.audio = true;
        info.gain_db = Some(0.0);
        let detail = Detail::Clip(info, chain);
        for (label, command, key, value) in [
            ("Loop: On", "clip.setLoop", "loop", json!(false)),
            ("Warp: Off", "clip.setAudio", "warp", json!(true)),
            ("Mode: Smooth", "clip.setAudio", "mode", json!("high-quality")),
        ] {
            let mut harness = Harness::builder().with_size(egui::vec2(700.0, 400.0)).build_ui_state(
                |ui, actions: &mut Vec<Action>| {
                    show_row(ui, &detail, &[], actions, &mut Vec::new());
                },
                Vec::new(),
            );
            harness.get_by_label(label).click();
            harness.run();
            let actions = harness.state();
            assert_eq!(actions.len(), 1, "{label}");
            assert_eq!(actions[0].id, command);
            assert_eq!(actions[0].params["clip"], json!(beat_b.0));
            assert_eq!(actions[0].params[key], value, "{label}");
        }
        // A slot's clip is named by its track and its scene, from 1.
        assert_eq!(
            on_clip(ClipRef::Slot(TrackId(2), 1), json!({ "loop": true })),
            json!({ "track": 2, "scene": 2, "loop": true })
        );
    }

    /// While the transport runs, a panel shows what its automation plays: a toggle holds each
    /// point's value, a continuous parameter moves between them (#706).
    #[test]
    fn panels_follow_automation_as_the_engine_plays_it() {
        use suhina_engine::model::{AutomationLane, AutomationPoint};
        let mut project = demo_project();
        let mut utility = Device::new("Utility", DeviceKind::Builtin("utility".into()));
        for param in ["mono", "balance"] {
            let mut lane = AutomationLane::new(param);
            lane.points = vec![AutomationPoint::new(0.0, 1.0), AutomationPoint::new(4.0, -1.0)];
            utility.automation.push(lane);
        }
        project.channels[0].devices.push(utility);
        let mut detail = build(&project, Some(TrackId(1)), None);
        detail.follow_automation(&project, 1.0);
        let Detail::Devices(chain) = &detail else { panic!("expected the chain") };
        let value = |id: &str| chain.devices.last().and_then(|d| d.values.iter().find(|(p, _)| p == id)).map(|v| v.1);
        assert_eq!(value("mono"), Some(1.0), "a toggle holds its point's value");
        assert_eq!(value("balance"), Some(0.5), "a continuous one is a quarter of the way down");
        detail.follow_automation(&project, 5.0);
        let Detail::Devices(chain) = &detail else { panic!("expected the chain") };
        let value = |id: &str| chain.devices.last().and_then(|d| d.values.iter().find(|(p, _)| p == id)).map(|v| v.1);
        assert_eq!((value("mono"), value("balance")), (Some(0.0), Some(-1.0)), "the last point, kept in range");
    }

    #[test]
    fn a_selected_slot_clip_shows_its_scene_loop_and_fades() {
        let mut project = demo_project();
        let column = project.launcher.columns.iter_mut().find(|c| c.track == TrackId(2)).expect("Bass has slots");
        let clip = column.slots[1].clip.as_mut().expect("Bass has a clip in scene 2");
        clip.fade_in = 1.5;
        clip.fade_out = 0.25;
        let content = clip.content;
        let name = project.content(content).map(|c| c.name.clone()).unwrap_or_default();
        let Detail::Clip(info, chain) = build(&project, None, Some(ClipRef::Slot(TrackId(2), 1))) else {
            panic!("expected the slot's clip")
        };
        assert_eq!(chain.track, TrackId(2));
        assert_eq!((info.name, info.start.as_str(), info.scene), (name, "2", true));
        assert_eq!(info.fades, (1.5, 0.25));
        // An empty slot, a scene past the last, or a track without slots shows the track, or nothing.
        let Detail::Devices(chain) = build(&project, Some(TrackId(3)), Some(ClipRef::Slot(TrackId(3), 0))) else {
            panic!("an empty slot shows the track")
        };
        assert_eq!(chain.track, TrackId(3));
        assert_eq!(build(&project, None, Some(ClipRef::Slot(TrackId(2), 99))), Detail::Nothing);
        assert_eq!(build(&project, None, Some(ClipRef::Slot(TrackId(10), 0))), Detail::Nothing);
    }

    #[test]
    fn a_selected_track_shows_its_devices() {
        let mut project = demo_project();
        project.channels[0].devices.push(Device {
            enabled: false,
            params: vec![DeviceParam::new("gain", -3.5)],
            ..Device::new("Utility", DeviceKind::Builtin("utility".into()))
        });
        let Detail::Devices(chain) = build(&project, Some(TrackId(1)), None) else { panic!() };
        assert_eq!(chain.devices.len(), 1);
        assert_eq!(chain.devices[0].params, [("gain".to_string(), "-3.5".to_string())]);
        assert!(!chain.devices[0].enabled);
        // A clip on the track shows the same chain beside its properties.
        let first = project.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(1))).unwrap().clips[0].id;
        let Detail::Clip(_, with_clip) = build(&project, None, Some(ClipRef::Arrangement(TrackId(1), first))) else {
            panic!()
        };
        assert_eq!(with_clip, chain);
        assert_eq!(build(&project, None, None), Detail::Nothing);
    }

    #[test]
    fn a_built_ins_generated_list_fits_the_compact_panel() {
        let mut project = demo_project();
        project.channels[0].devices = vec![
            Device::new("Utility", DeviceKind::Builtin("utility".into())),
            Device::new("Pitch", DeviceKind::Builtin("pitch".into())),
        ];
        let Detail::Devices(chain) = build(&project, Some(TrackId(1)), None) else { panic!() };
        for device in &chain.devices {
            let graph = device.graph.as_ref().unwrap_or_else(|| panic!("{} has a panel", device.name));
            assert!(graph.canvas.is_none() && graph.nodes.is_empty(), "the generated list, no graph");
            let height = panel_height(device, COMPACT_WIDTH, None);
            assert!(height <= ROW + PAD * 2.0 + COMPACT_HEIGHT, "{}: {height}", device.name);
        }
        // Utility's thirteen parameters scroll inside the standard height.
        assert_eq!(panel_height(&chain.devices[0], COMPACT_WIDTH, None), ROW + PAD * 2.0 + COMPACT_HEIGHT);
        assert_eq!(STACK_WIDTH, 272.0);
    }

    #[test]
    fn a_stacked_generated_list_keeps_its_width_in_any_column() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        let mut project = demo_project();
        project.channels[0].devices = vec![Device::new("Utility", DeviceKind::Builtin("utility".into()))];
        let Detail::Devices(chain) = build(&project, Some(TrackId(1)), None) else { panic!() };
        let detail = Detail::Devices(chain);
        // A narrow column, the default one and a wide one: Gain's slider is in the same place
        // each time, its row 260 px wide, scrolled past the narrow column's edge, not squeezed.
        let gain = |column: f32| {
            let mut harness = Harness::builder().with_size(egui::vec2(column, 220.0)).build_ui(|ui| {
                show_stack(ui, &detail, &[], &mut Vec::new(), &mut Vec::new());
            });
            harness.run();
            harness.get_by_label("Gain").rect()
        };
        let default = gain(STACK_WIDTH);
        assert!(default.right() <= PAD + COMPACT_WIDTH && default.right() > STACK_WIDTH / 2.0, "{default:?}");
        assert_eq!(gain(200.0), default, "a narrow column scrolls the list");
        assert_eq!(gain(600.0), default, "a wide column leaves it 260 px wide");
    }

    #[test]
    fn an_audio_effect_shows_its_label_and_only_visible_nodes() {
        let mut graph = suhina_device_sdk::DeviceGraph::new(
            "org.example.gain",
            "Example Gain",
            suhina_device_sdk::DeviceRole::Effect,
        );
        graph.parameters.push(suhina_device_sdk::Parameter::new("gain", "Gain", (0.0, 2.0), 1.0));
        graph.nodes = vec![
            suhina_device_sdk::Node::audio_input("in"),
            suhina_device_sdk::Node::gain("gain", "gain"),
            suhina_device_sdk::Node::audio_output("out"),
        ];
        graph.nodes[0].visible = false;
        graph.nodes[2].visible = false;
        graph.connections =
            vec![suhina_device_sdk::Connection::new("in", "gain"), suhina_device_sdk::Connection::new("gain", "out")];
        let mut project = demo_project();
        project.channels[0].devices.push(Device {
            state: graph.to_sd().ok().map(String::into_bytes),
            ..Device::new("Example Gain", DeviceKind::Sd)
        });
        let Detail::Devices(chain) = build(&project, Some(TrackId(1)), None) else { panic!() };
        assert_eq!(chain.devices[0].kind, "AudioEffect");
        assert_eq!(
            chain.devices[0].graph.as_ref().map(|graph| graph.nodes.as_slice()),
            Some(["Gain".into()].as_slice())
        );
        assert_eq!(chain.devices[0].graph.as_ref().map(|graph| graph.parameters.len()), Some(1));
    }

    #[test]
    fn every_sdk_control_renders_in_a_scrollable_device_panel() {
        use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, Parameter, ParameterControl::*};
        for control in
            [Slider, LogSlider, Integer, Toggle, Knob, LogKnob, Number, VerticalSlider, Button, Choice, Radio]
        {
            let mut graph = DeviceGraph::new("org.test.ui", "UI", DeviceRole::Effect);
            let mut p = Parameter::new("gain", "Gain", (0.0, 2.0), 1.0);
            p.control = control;
            if matches!(control, LogSlider | LogKnob) {
                p.min = 0.01;
            }
            if matches!(control, Toggle | Button) {
                p.max = 1.0;
            }
            if matches!(control, Choice | Radio) {
                p.ui.options = vec!["Off".into(), "Unity".into(), "Boost".into()];
            }
            p.ui.size = Some([240.0, 140.0]);
            p.ui.position = Some([180.0, 40.0]);
            p.ui.font_size = 16.0;
            p.ui.monospace = true;
            p.ui.shape = suhina_device_sdk::ControlShape::Square;
            p.ui.label_position = suhina_device_sdk::LabelPosition::Top;
            if matches!(control, Knob | LogKnob) {
                p.ui.size = Some([40.0, 40.0]);
                p.ui.font_size = 12.0;
                p.ui.label_position = suhina_device_sdk::LabelPosition::Hidden;
                p.ui.show_value = false;
            }
            graph.parameters.push(p);
            graph.nodes = vec![Node::audio_input("in"), Node::gain("gain", "gain"), Node::audio_output("out")];
            graph.connections = vec![Connection::new("in", "gain"), Connection::new("gain", "out")];
            let mut project = demo_project();
            project.channels[0]
                .devices
                .push(Device { state: Some(graph.to_sd().unwrap().into_bytes()), ..Device::new("UI", DeviceKind::Sd) });
            let detail = build(&project, Some(TrackId(1)), None);
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                show_stack(ui, &detail, &[], &mut Vec::new(), &mut Vec::new());
            });
            assert!(!output.shapes.is_empty(), "{control:?}");
            output.textures_delta.clear();
        }
    }

    #[test]
    fn authored_control_positions_and_auto_flow_have_the_expected_extent() {
        use suhina_device_sdk::{ParameterControl, ParameterUi};
        let mut p = GraphParameter {
            text: String::new(),
            default: 1.0,
            control: ParameterControl::Knob,
            unit: String::new(),
            id: "gain".into(),
            name: "Gain".into(),
            min: 0.0,
            max: 2.0,
            value: 1.0,
            ui: ParameterUi::default(),
        };
        p.ui.size = Some([240.0, 120.0]);
        p.ui.position = Some([300.0, 10.0]);
        let mut next = p.clone();
        next.ui.position = None;
        next.ui.size = Some([200.0, 60.0]);
        let cells = parameter_layout(&[p, next], 320.0, &[]);
        assert_eq!(cells[0].min, Pos2::new(300.0, 10.0));
        assert_eq!(cells[0].max, Pos2::new(540.0, 130.0));
        assert_eq!(cells[1].min, Pos2::new(0.0, 130.0));
        assert_eq!(cells[1].max, Pos2::new(200.0, 190.0));
    }

    #[test]
    #[ignore = "needs a GPU adapter; run with --ignored"]
    fn render_the_authored_control_gallery() {
        let graph = styled_gallery::build().unwrap();
        for parameter in &graph.parameters {
            assert_eq!(parameter.ui.position.map(|position| position[1]), Some(0.0));
            assert_eq!(parameter.ui.label_position, suhina_device_sdk::LabelPosition::Top);
        }
        let mut project = demo_project();
        project.channels[0].devices.push(Device {
            state: Some(graph.to_sd().unwrap().into_bytes()),
            ..Device::new(graph.name.clone(), DeviceKind::Sd)
        });
        let detail = build(&project, Some(TrackId(1)), None);
        let mut harness =
            egui_kittest::Harness::builder().with_size((1180.0, 180.0)).with_pixels_per_point(1.0).wgpu().build_ui(
                |ui| {
                    show_row(ui, &detail, &[], &mut Vec::new(), &mut Vec::new());
                },
            );
        theme::install(&harness.ctx);
        harness.run();
        let image = harness.render().unwrap();
        let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
        std::fs::create_dir_all(&output).unwrap();
        image.save(output.join("sdk-controls.png")).unwrap();
    }

    #[test]
    #[ignore = "needs a GPU adapter; run with --ignored"]
    fn render_sdk_example_with_stock_controls() {
        let graph = custom_panel::build().unwrap();
        let mut project = demo_project();
        project.channels[0].devices.push(Device {
            state: Some(graph.to_sd().unwrap().into_bytes()),
            ..Device::new(graph.name.clone(), DeviceKind::Sd)
        });
        let detail = build(&project, Some(TrackId(1)), None);
        let mut harness =
            egui_kittest::Harness::builder().with_size((660.0, 340.0)).with_pixels_per_point(1.0).wgpu().build_ui(
                |ui| {
                    show_stack(ui, &detail, &[], &mut Vec::new(), &mut Vec::new());
                },
            );
        theme::install(&harness.ctx);
        harness.run();
        let image = harness.render().unwrap();
        let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
        std::fs::create_dir_all(&output).unwrap();
        image.save(output.join("sdk-example-controls.png")).unwrap();
    }

    #[test]
    #[ignore = "needs a GPU adapter; run with --ignored"]
    fn render_sdk_example_in_a_narrow_panel() {
        let graph = custom_panel::build().unwrap();
        let mut project = demo_project();
        project.channels[0].devices.push(Device {
            state: Some(graph.to_sd().unwrap().into_bytes()),
            ..Device::new(graph.name.clone(), DeviceKind::Sd)
        });
        let detail = build(&project, Some(TrackId(1)), None);
        if let Detail::Devices(chain) = &detail {
            assert_eq!(sdk_viewport(&chain.devices[0]), None);
        } else {
            panic!("expected device chain");
        }
        let mut harness =
            egui_kittest::Harness::builder().with_size((660.0, 340.0)).with_pixels_per_point(1.0).wgpu().build_ui(
                |ui| {
                    show_stack(ui, &detail, &[], &mut Vec::new(), &mut Vec::new());
                },
            );
        theme::install(&harness.ctx);
        harness.run();
        let image = harness.render().unwrap();
        let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
        std::fs::create_dir_all(&output).unwrap();
        image.save(output.join("sdk-example-narrow.png")).unwrap();
    }

    fn report(status: PluginStatus, editor_open: bool) -> PluginReport {
        PluginReport {
            status,
            plugin: Some(("Verb".into(), "Vendor".into(), "same")),
            instrument: false,
            latency: 0,
            editor_open,
            editor_embedded: None,
            blocks: 0,
            late_blocks: 0,
            longest_us: 0,
            no_time: 0,
            state_refused: None,
            ara: false,
            params: None,
        }
    }

    fn param(id: u32, name: &str, steps: u32, value: f64) -> suhina_plugins::ParamInfo {
        suhina_plugins::ParamInfo {
            id,
            name: name.into(),
            group: String::new(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            value,
            text: format!("{name} at {value}"),
            steps,
            automatable: true,
            read_only: false,
            hidden: false,
        }
    }

    /// A running plug-in that listed a mix, a bypass switch, a hidden and a read-only one.
    fn listing(status: PluginStatus) -> PluginReport {
        let hidden = suhina_plugins::ParamInfo { hidden: true, ..param(3, "Secret", 0, 0.0) };
        let meter = suhina_plugins::ParamInfo { read_only: true, ..param(4, "Meter", 0, 0.0) };
        let list = vec![param(1, "Mix", 0, 0.5), param(2, "Bypass", 1, 0.0), hidden, meter];
        PluginReport { params: Some(list.into()), ..report(status, false) }
    }

    #[test]
    fn a_running_plug_in_shows_the_parameters_it_lists_as_a_generated_list() {
        let mut project = with_plugin();
        // A value the project keeps wins over the one the plug-in listed, without its text.
        project.channels[0].devices[0].params.push(suhina_engine::model::DeviceParam::new("1", 0.25));
        let graph = |status: PluginStatus| {
            let Detail::Devices(chain) = build_with(
                &project,
                Some(TrackId(1)),
                None,
                &mut |_, _| Some(listing(status.clone())),
                crate::detail::nam_panel::Lists::default(),
            ) else {
                panic!()
            };
            chain.devices[0].graph.clone()
        };
        let running = graph(PluginStatus::Running).unwrap();
        let shown: Vec<_> = running.parameters.iter().map(|p| (p.id.as_str(), p.name.as_str(), p.control)).collect();
        assert_eq!(
            shown,
            [
                ("1", "Mix", suhina_device_sdk::ParameterControl::Slider),
                ("2", "Bypass", suhina_device_sdk::ParameterControl::Toggle)
            ],
            "hidden and read-only ones are left out"
        );
        assert_eq!((running.parameters[0].value, running.parameters[0].text.as_str()), (0.25, ""));
        assert_eq!(running.parameters[1].text, "Bypass at 0");
        assert!(running.canvas.is_none() && running.expanded.is_none());
        // A plug-in that does not run has no controls to send to.
        assert_eq!(graph(PluginStatus::Crashed("gone".into())), None);
    }

    /// While automation plays, a plug-in's list shows the automated value, kept in range, and
    /// drops the text the plug-in gave for the value it listed.
    #[test]
    fn automation_replaces_a_plug_ins_listed_text_with_the_value_it_plays() {
        use suhina_engine::model::{AutomationLane, AutomationPoint};
        let mut project = with_plugin();
        let point = |beat, value| AutomationPoint { beat, value, curve: 0.0 };
        project.channels[0].devices[0].automation = vec![
            AutomationLane { param: "1".into(), points: vec![point(0.0, 0.75), point(8.0, 0.75)], enabled: true },
            AutomationLane { param: "2".into(), points: vec![point(0.0, 3.0), point(8.0, 3.0)], enabled: true },
        ];
        let mut detail = build_with(
            &project,
            Some(TrackId(1)),
            None,
            &mut |_, _| Some(listing(PluginStatus::Running)),
            crate::detail::nam_panel::Lists::default(),
        );
        detail.follow_automation(&project, 4.0);
        let Detail::Devices(chain) = detail else { panic!() };
        let graph = chain.devices[0].graph.as_ref().unwrap();
        let shown: Vec<_> = graph.parameters.iter().map(|p| (p.value, p.text.as_str())).collect();
        assert_eq!(shown, [(0.75, ""), (1.0, "")]);
        assert_eq!(value_text(&graph.parameters[0], 0.75), "0.75");
    }

    #[test]
    fn a_plug_ins_generated_list_sets_its_parameters_by_id() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        let project = with_plugin();
        let Detail::Devices(chain) = build_with(
            &project,
            Some(TrackId(1)),
            None,
            &mut |_, _| Some(listing(PluginStatus::Running)),
            crate::detail::nam_panel::Lists::default(),
        ) else {
            panic!()
        };
        let detail = Detail::Devices(chain);
        for stacked in [false, true] {
            let mut harness = Harness::builder().with_size(egui::vec2(600.0, 220.0)).build_ui_state(
                |ui, actions: &mut Vec<Action>| {
                    let mut places = Vec::new();
                    if stacked {
                        show_stack(ui, &detail, &[], actions, &mut places);
                    } else {
                        show_row(ui, &detail, &[], actions, &mut places);
                    }
                },
                Vec::new(),
            );
            harness.get_by_label("Bypass").click();
            harness.run();
            let actions = harness.state();
            let set: Vec<_> = actions.iter().filter(|a| a.id == "device.setParam").collect();
            assert_eq!(set.len(), 1, "stacked {stacked}: {actions:?}");
            assert_eq!((&set[0].params["param"], &set[0].params["value"]), (&json!("2"), &json!(1.0)));
            assert!(harness.query_by_label("Open editor Verb").is_some(), "the editor button stays");
        }
    }

    fn with_plugin() -> Project {
        let mut project = demo_project();
        project.channels[0]
            .devices
            .push(Device { state: Some(vec![1, 2]), ..Device::new("Verb", DeviceKind::Vst3 { id: "ABC".into() }) });
        project
    }

    #[test]
    fn a_plug_in_shows_where_it_stands_and_what_its_button_does() {
        let project = with_plugin();
        let shown = |status: Option<PluginStatus>, open: bool| {
            let Detail::Devices(chain) = build_with(
                &project,
                Some(TrackId(1)),
                None,
                &mut |_, _| status.clone().map(|s| report(s, open)),
                crate::detail::nam_panel::Lists::default(),
            ) else {
                panic!()
            };
            chain.devices[0].plugin.clone().unwrap()
        };
        let running = shown(Some(PluginStatus::Running), false);
        assert_eq!((running.state, running.line.as_str()), (PluginState::Running, "Verb by Vendor"));
        assert_eq!(running.button(), Some(("Open editor", "device.openEditor")));
        assert_eq!(shown(Some(PluginStatus::Running), true).button(), Some(("Close editor", "device.closeEditor")));
        let crashed = shown(Some(PluginStatus::Crashed("its host process exited".into())), false);
        assert_eq!(crashed.button(), Some(("Restart", "device.restart")));
        assert!(crashed.line.contains("exited"));
        let missing = shown(Some(PluginStatus::Missing), false);
        assert_eq!((missing.state, missing.button()), (PluginState::Missing, None));
        assert_eq!(shown(Some(PluginStatus::Loading), false).button(), None);
        // Not playing: nothing runs, nothing to press.
        let stopped = shown(None, false);
        assert_eq!((stopped.state, stopped.button()), (PluginState::Stopped, None));
        // A built-in effect or instrument has no plug-in part.
        let Detail::Devices(chain) = build(&demo_project(), Some(TrackId(1)), None) else { panic!() };
        assert!(chain.devices.iter().all(|d| d.plugin.is_none()));
    }

    #[test]
    fn a_clips_ara_editor_lands_in_the_clip_view_or_offers_to_open() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        let project = with_plugin();
        let chain_with = |report: PluginReport| {
            let Detail::Devices(chain) = build_with(
                &project,
                Some(TrackId(1)),
                None,
                &mut |_, _| Some(report.clone()),
                crate::detail::nam_panel::Lists::default(),
            ) else {
                panic!()
            };
            chain
        };
        let bound = PluginReport { ara: true, ..report(PluginStatus::Running, false) };
        assert!(chain_with(bound.clone()).devices[0].plugin.as_ref().unwrap().ara);
        // Embedded: it lands at the area's corner, the whole of it showing.
        let embedded =
            chain_with(PluginReport { editor_embedded: Some((300, 200)), ..report(PluginStatus::Running, true) });
        let mut harness = Harness::builder().with_size(egui::vec2(600.0, 300.0)).build_ui_state(
            |ui, (places, actions): &mut (Vec<EditorPlace>, Vec<Action>)| {
                places.clear();
                let area = ui.available_rect_before_wrap();
                show_ara_editor(ui, area, &embedded, 0, actions, places);
            },
            (Vec::new(), Vec::new()),
        );
        harness.run();
        let place = harness.state().0[0];
        assert_eq!((place.track, place.device), (TrackId(1), 0));
        assert_eq!((place.placement.clip_width, place.placement.clip_height), (300, 200));
        // Float above it takes it out to a window of its own.
        harness.get_by_label("Float Verb").click();
        harness.run();
        let float = &harness.state().1[0];
        assert_eq!(
            (float.id, float.params.clone()),
            ("view.araFloat", json!({ "track": 1, "device": 0, "float": true }))
        );
        // Closed: the area offers to open it, and the button opens that plug-in's editor.
        let closed = chain_with(bound);
        let mut harness = Harness::builder().with_size(egui::vec2(600.0, 300.0)).build_ui_state(
            |ui, actions: &mut Vec<Action>| {
                let area = ui.available_rect_before_wrap();
                show_ara_editor(ui, area, &closed, 0, actions, &mut Vec::new());
            },
            Vec::new(),
        );
        harness.run();
        harness.get_by_label("Open editor Verb").click();
        harness.run();
        assert_eq!(harness.state()[0].id, "device.openEditor");
        assert_eq!(harness.state()[0].params, json!({ "track": 1, "device": 0 }));
    }

    #[test]
    fn an_embedded_editor_takes_its_room_and_says_where_it_lands() {
        use egui_kittest::Harness;
        let project = with_plugin();
        let embedded = PluginReport { editor_embedded: Some((300, 200)), ..report(PluginStatus::Running, true) };
        let Detail::Devices(chain) = build_with(
            &project,
            Some(TrackId(1)),
            None,
            &mut |_, _| Some(embedded.clone()),
            crate::detail::nam_panel::Lists::default(),
        ) else {
            panic!()
        };
        assert_eq!(chain.devices[0].plugin.as_ref().unwrap().embedded, Some((300, 200)));
        let detail = Detail::Devices(chain);
        for stacked in [false, true] {
            let mut harness = Harness::builder().with_size(egui::vec2(600.0, 300.0)).build_ui_state(
                |ui, places: &mut Vec<EditorPlace>| {
                    places.clear();
                    let mut actions = Vec::new();
                    if stacked {
                        show_stack(ui, &detail, &[], &mut actions, places);
                    } else {
                        show_row(ui, &detail, &[], &mut actions, places);
                    }
                },
                Vec::new(),
            );
            harness.run();
            let places = harness.state();
            assert_eq!(places.len(), 1, "stacked {stacked}");
            let place = places[0];
            assert_eq!((place.track, place.device), (TrackId(1), 0));
            // Under the plug-in's line and button (past the harness's own margin), the whole
            // editor showing (at 1 pixel a point).
            let p = place.placement;
            let margin = p.y - EDITOR_TOP as i32;
            assert!(margin >= 0, "stacked {stacked}: {p:?}");
            assert_eq!((p.x, p.clip_x, p.clip_y), (PAD as i32 + margin, p.x, p.y), "stacked {stacked}");
            assert_eq!((p.clip_width, p.clip_height), (300, 200), "stacked {stacked}");
        }
        // A panel too short for it shows what fits; one with no room hides it.
        let margin = {
            let mut probe = Harness::builder().with_size(egui::vec2(600.0, 300.0)).build_ui_state(
                |ui, places: &mut Vec<EditorPlace>| {
                    places.clear();
                    show_row(ui, &detail, &[], &mut Vec::new(), places);
                },
                Vec::new(),
            );
            probe.run();
            probe.state()[0].placement.y as f32 - EDITOR_TOP
        };
        let mut short =
            Harness::builder().with_size(egui::vec2(600.0, EDITOR_TOP + 50.0 + margin * 2.0)).build_ui_state(
                |ui, places: &mut Vec<EditorPlace>| {
                    places.clear();
                    show_row(ui, &detail, &[], &mut Vec::new(), places);
                },
                Vec::new(),
            );
        short.run();
        assert_eq!((short.state()[0].placement.clip_width, short.state()[0].placement.clip_height), (300, 50));
        let mut none = Harness::builder().with_size(egui::vec2(600.0, EDITOR_TOP - 1.0 + margin * 2.0)).build_ui_state(
            |ui, places: &mut Vec<EditorPlace>| {
                places.clear();
                show_row(ui, &detail, &[], &mut Vec::new(), places);
            },
            Vec::new(),
        );
        none.run();
        assert_eq!(none.state()[0].placement, Placement::HIDDEN);
    }

    #[test]
    fn the_editor_button_asks_for_the_device_command() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;
        let project = with_plugin();
        let Detail::Devices(chain) = build_with(
            &project,
            Some(TrackId(1)),
            None,
            &mut |_, _| Some(report(PluginStatus::Running, false)),
            crate::detail::nam_panel::Lists::default(),
        ) else {
            panic!()
        };
        let detail = Detail::Devices(chain);
        for stacked in [false, true] {
            let mut harness = Harness::builder().with_size(egui::vec2(600.0, 200.0)).build_ui_state(
                |ui, actions: &mut Vec<Action>| {
                    let mut places = Vec::new();
                    if stacked {
                        show_stack(ui, &detail, &[], actions, &mut places);
                    } else {
                        show_row(ui, &detail, &[], actions, &mut places);
                    }
                    assert!(places.is_empty());
                },
                Vec::new(),
            );
            harness.get_by_label("Open editor Verb").click();
            harness.run();
            let actions = harness.state();
            assert_eq!(actions.len(), 1, "stacked {stacked}");
            assert_eq!(actions[0].id, "device.openEditor");
            assert_eq!(actions[0].params, json!({ "track": 1, "device": 0 }));
        }
    }
}
