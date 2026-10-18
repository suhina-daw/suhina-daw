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

use egui::{Align2, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{DeviceKind, LaneData, Note, Project, TrackId};
use suhina_session::{Placement, PluginReport, PluginStatus};

use crate::interaction::grid::gain_to_db;
use crate::kit::motion;
use crate::kit::theme::{self, PAD, ROW};
use crate::kit::widgets::{self, Action};

/// Width of one device in a row, and of the column the stacked chain takes.
pub const DEVICE_WIDTH: f32 = 220.0;
pub const STACK_WIDTH: f32 = 260.0;
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
}

/// The height a device row needs to show an embedded editor `height` pixels tall under a
/// plug-in's line and button, at `pixels_per_point`.
pub fn row_height_with_editor(height: u32, pixels_per_point: f32) -> f32 {
    EDITOR_TOP + height as f32 / pixels_per_point.max(0.01) + PAD
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceBox {
    /// Its index among the track's devices, as the device commands take it.
    pub index: usize,
    pub name: String,
    /// What it is: a built-in effect or instrument, a plug-in format, or one Suhina cannot run.
    pub kind: String,
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

#[derive(Debug, Clone, PartialEq)]
pub struct GraphBox {
    pub canvas: Option<std::sync::Arc<suhina_device_sdk::ui::DeviceUi>>,
    pub linear: bool,
    pub nodes: Vec<String>,
    pub parameters: Vec<GraphParameter>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GraphParameter {
    pub default: f64,
    pub ui: suhina_device_sdk::ParameterUi,
    pub control: suhina_device_sdk::ParameterControl,
    pub unit: String,
    pub id: String,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub value: f64,
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
        Self { state, line, editor_open: report.editor_open, embedded: report.editor_embedded }
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

/// A selected clip: on the arrangement (its track and index there), or in a launcher slot (its
/// track and scene, from 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipRef {
    Arrangement(TrackId, usize),
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
    pub audio: bool,
    pub gain_db: Option<f64>,
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

/// The detail for a selected clip (it wins, with its own track's chain) or track.
pub fn build(project: &Project, track: Option<TrackId>, clip: Option<ClipRef>) -> Detail {
    build_with(project, track, clip, &mut |_, _| None, crate::detail::nam_panel::Lists::default())
}

/// [`build`], with each plug-in device's state from `plugins` (track, device index).
pub fn build_with(
    project: &Project,
    track: Option<TrackId>,
    clip: Option<ClipRef>,
    plugins: &mut dyn FnMut(TrackId, usize) -> Option<PluginReport>,
    nam: crate::detail::nam_panel::Lists,
) -> Detail {
    let bar = bar_beats(project);
    let track_name =
        |id: TrackId| project.tracks.iter().find(|t| t.id == id).map(|t| t.name.clone()).unwrap_or_default();
    let found = clip.and_then(|clip| match clip {
        ClipRef::Arrangement(id, index) => project
            .arrangement
            .lanes
            .iter()
            .find(|l| l.track == Some(id))
            .and_then(|l| l.clips.get(index))
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
                LaneData::Audio(region) => Some(region.gain),
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
                audio: audio.is_some(),
                gain_db: audio.map(gain_to_db),
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
    plugins: &mut dyn FnMut(TrackId, usize) -> Option<PluginReport>,
    nam: crate::detail::nam_panel::Lists,
) -> Chain {
    let track_name = project.tracks.iter().find(|t| t.id == id).map(|t| t.name.clone()).unwrap_or_default();
    let channel = project
        .tracks
        .iter()
        .find(|t| t.id == id)
        .and_then(|t| t.channel)
        .and_then(|c| project.channels.iter().find(|ch| ch.id == c));
    let devices = channel
        .map(|c| {
            c.devices
                .iter()
                .enumerate()
                .map(|(index, d)| DeviceBox {
                    index,
                    name: d.name.clone(),
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
                        (DeviceKind::Sd, Some(state)) => crate::detail::device_canvas::graph(state).map(|cached| {
                            let graph = &cached.graph;
                            GraphBox {
                                canvas: cached.canvas.clone(),
                                linear: graph.connections.len() == graph.nodes.len().saturating_sub(1)
                                    && graph.nodes.iter().all(|node| {
                                        graph.connections.iter().filter(|e| e.from == node.id).count() <= 1
                                    })
                                    && graph.connections.iter().all(|e| e.from_port == "out" && e.to_port == "in"),
                                nodes: graph
                                    .ordered_nodes()
                                    .iter()
                                    .filter(|node| node.visible)
                                    .map(|node| node.kind.info().name.into())
                                    .collect(),
                                parameters: graph
                                    .parameters
                                    .iter()
                                    .map(|parameter| GraphParameter {
                                        default: parameter.default,
                                        control: parameter.control,
                                        ui: parameter.ui.clone(),
                                        unit: parameter.unit.clone(),
                                        id: parameter.id.clone(),
                                        name: parameter.name.clone(),
                                        min: parameter.min,
                                        max: parameter.max,
                                        value: d
                                            .params
                                            .iter()
                                            .find(|value| value.id == parameter.id)
                                            .map_or(parameter.default, |value| parameter.clamp(value.value)),
                                    })
                                    .collect(),
                            }
                        }),
                        _ => None,
                    },
                    plugin: matches!(
                        d.kind,
                        DeviceKind::Clap { .. } | DeviceKind::Vst3 { .. } | DeviceKind::Vst2 { .. }
                    )
                    .then(|| PluginBox::from_report(plugins(id, index))),
                })
                .collect()
        })
        .unwrap_or_default();
    Chain { track: id, name: track_name, devices, nam }
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
    actions: &mut Vec<Action>,
    places: &mut Vec<EditorPlace>,
) -> Option<Showing> {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());
    match detail {
        Detail::Nothing => hint(&painter, area, "Select a track or a clip to see it here"),
        Detail::Devices(chain) => chain_row(ui, &painter, area, chain, actions, places),
        Detail::Clip(info, chain) => {
            let (column, picked) = clip_properties(ui, &painter, area, info, false);
            clip_for_readers(ui, column, info);
            let rest = Rect::from_min_max(Pos2::new(column.right() + 1.0, area.top()), area.max);
            if rest.width() > 0.0 {
                chain_row(ui, &painter, rest, chain, actions, places);
            }
            return picked;
        }
    }
    None
}

/// A chain's devices side by side in `area`; one with an embedded editor is as wide as it.
fn chain_row(
    ui: &mut Ui,
    painter: &egui::Painter,
    area: Rect,
    chain: &Chain,
    actions: &mut Vec<Action>,
    places: &mut Vec<EditorPlace>,
) {
    if chain.devices.is_empty() {
        hint(painter, area, &format!("No devices on {} yet", chain.name));
        return;
    }
    let scale = ui.ctx().pixels_per_point();
    let mut x = area.left();
    for device in &chain.devices {
        let editor = device.plugin.as_ref().and_then(|p| p.editor_size(scale));
        // A NAM panel asks for the mock's wider box (nam spec › The panels); the rest keep
        // the plain device width.
        let authored_base =
            if device.file_device.is_some() { crate::detail::nam_panel::NAM_WIDTH } else { DEVICE_WIDTH };
        let authored_width = device.graph.as_ref().map_or(authored_base, |graph| {
            graph.canvas.as_ref().map_or_else(
                || {
                    parameter_layout(&graph.parameters, DEVICE_WIDTH - PAD * 2.0, &[])
                        .iter()
                        .map(|cell| cell.right() + PAD * 2.0)
                        .fold(DEVICE_WIDTH, f32::max)
                },
                |c| c.viewport_size.map_or(DEVICE_WIDTH.max(c.size[0] + PAD * 2.0), |v| v[0] + PAD * 2.0),
            )
        });
        let width = editor.map_or(authored_width, |e| authored_width.max(e.x + PAD * 2.0));
        let bottom = sdk_viewport(device).map_or(
            if device.file_device.is_some() {
                // A NAM panel's own height, not the whole area's (nam spec › The panels).
                area.top() + crate::detail::nam_panel::NAM_HEIGHT
            } else {
                area.bottom()
            },
            |v| (area.top() + ROW + PAD * 2.0 + v[1]).min(area.bottom()),
        );
        let rect = Rect::from_min_max(Pos2::new(x, area.top()), Pos2::new(x + width, bottom));
        eprintln!("[nam-debug] row box {rect:?} area {area:?} bottom {bottom}");
        x = rect.right() + 1.0;
        if rect.left() > area.right() {
            break;
        }
        name_for_readers(ui, rect.intersect(area), device);
        device_panel(ui, &painter.with_clip_rect(area), rect, chain.track, device, actions, &chain.nam);
        plugin_button(ui, &painter.with_clip_rect(area), rect, chain.track, device, actions);
        if let Some(editor) = editor {
            places.push(editor_place(ui, area, rect, editor, chain.track, device));
        }
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
    actions: &mut Vec<Action>,
    places: &mut Vec<EditorPlace>,
) -> Option<Showing> {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());
    match detail {
        Detail::Nothing => hint(&painter, area, "Select a track or a clip"),
        Detail::Devices(chain) => chain_stack(ui, &painter, area, chain, actions, places),
        Detail::Clip(info, chain) => {
            let (column, picked) = clip_properties(ui, &painter, area, info, true);
            clip_for_readers(ui, column, info);
            let rest = Rect::from_min_max(Pos2::new(area.left(), column.bottom() + 1.0), area.max);
            if rest.height() > 0.0 {
                chain_stack(ui, &painter, rest, chain, actions, places);
            }
            return picked;
        }
    }
    None
}

/// The clip's properties column for a clip whose editor fills the rest of the detail area (the
/// piano roll): on the left in a row, on top when `stacked`. Returns the rest. The quantise
/// controls and the MIDI tools live in the column beside this one
/// (› Other note tools in `docs/spec/ui-clip-editor.md`). With it, the part the Clip / Devices
/// switch was clicked to, if it was.
pub fn show_clip_column(ui: &mut Ui, info: &ClipInfo, stacked: bool) -> (Rect, Option<Showing>) {
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());
    let (column, picked) = clip_properties(ui, &painter, area, info, stacked);
    clip_for_readers(ui, column, info);
    let rest = if stacked {
        Rect::from_min_max(Pos2::new(area.left(), column.bottom() + 1.0), area.max)
    } else {
        Rect::from_min_max(Pos2::new(column.right() + 1.0, area.top()), area.max)
    };
    (rest, picked)
}

/// A chain's devices one under another in `area`; one with an embedded editor is as tall as
/// it needs (the editor is cut off at the column's right edge when it is wider).
fn chain_stack(
    ui: &mut Ui,
    painter: &egui::Painter,
    area: Rect,
    chain: &Chain,
    actions: &mut Vec<Action>,
    places: &mut Vec<EditorPlace>,
) {
    if chain.devices.is_empty() {
        hint(painter, area, &format!("No devices on {} yet", chain.name));
        return;
    }
    let scale = ui.ctx().pixels_per_point();
    let mut y = area.top();
    for device in &chain.devices {
        let editor = device.plugin.as_ref().and_then(|p| p.editor_size(scale));
        let rows =
            if device.plugin.is_some() { 2.0 + BUTTON_HEIGHT / PARAM_ROW } else { device.params.len().max(1) as f32 };
        let height = match editor {
            Some(editor) => EDITOR_TOP + editor.y + PAD,
            // A NAM panel's own height (nam spec › The panels); the rest take their rows.
            None if device.file_device.is_some() => crate::detail::nam_panel::NAM_HEIGHT,
            None => {
                ROW + PAD * 2.0
                    + device.graph.as_ref().map_or(rows * PARAM_ROW, |graph| {
                        parameter_layout(
                            &graph.parameters,
                            area.width(),
                            graph.canvas.as_ref().map_or(&[], |c| c.hidden_parameters.as_slice()),
                        )
                        .iter()
                        .map(|r| r.bottom())
                        .fold(0.0f32, f32::max)
                    })
                    + device.graph.as_ref().map_or(0.0, |graph| {
                        graph.nodes.len() as f32 * NODE_ROW + graph.canvas.as_ref().map_or(0.0, |c| c.size[1])
                    })
            }
        };
        let height = sdk_viewport(device).map_or(height, |v| ROW + PAD * 2.0 + v[1]);
        let right = sdk_viewport(device).map_or(area.right(), |v| (area.left() + v[0] + PAD * 2.0).min(area.right()));
        let rect = Rect::from_min_max(Pos2::new(area.left(), y), Pos2::new(right, y + height));
        y = rect.bottom() + 1.0;
        if rect.top() > area.bottom() {
            break;
        }
        name_for_readers(ui, rect.intersect(area), device);
        device_panel(ui, &painter.with_clip_rect(area), rect, chain.track, device, actions, &chain.nam);
        plugin_button(ui, &painter.with_clip_rect(area), rect, chain.track, device, actions);
        if let Some(editor) = editor {
            places.push(editor_place(ui, area, rect, editor, chain.track, device));
        }
    }
}

/// Where a plug-in's embedded editor of `editor` points goes under its line and button in the
/// device's `rect`, as the host takes it: in the window's own pixels, with the part of the
/// panel's `area` (and the device's box) that shows it; what falls outside is cut off.
fn editor_place(ui: &Ui, area: Rect, rect: Rect, editor: Vec2, track: TrackId, device: &DeviceBox) -> EditorPlace {
    let scale = ui.ctx().pixels_per_point();
    let origin = Pos2::new(rect.left() + PAD, rect.top() + EDITOR_TOP);
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
    EditorPlace { track, device: device.index, placement }
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
        egui::Id::new(("plugin-button", track.0, device.index)),
        format!("{label} {}", device.name),
    );
    let hover = widgets::hover(ui, &response);
    let fill = crate::kit::motion::mix(theme::control_background(), theme::control_hover(), hover);
    painter.rect_filled(button, 0.0, fill);
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
        actions.push(Action::new(command, json!({ "track": track.0, "device": device.index })));
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

/// Names a device's panel for screen readers and tests.
fn name_for_readers(ui: &Ui, rect: Rect, device: &DeviceBox) {
    let response = ui.interact(rect, egui::Id::new(("device", rect.min.x as i32, rect.min.y as i32)), Sense::hover());
    let label = format!("Device {}", device.name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
}

fn hint(painter: &egui::Painter, area: Rect, text: &str) {
    painter.text(area.center(), Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT), theme::text_hint());
}

/// One device: a header with its on switch and name, then its parameters, one per row. A NAM
/// Amp Modeler or NAM Cab Loader draws its own panel instead
/// ([docs/spec/nam-amp-and-cab.md] › The panels).
fn device_panel(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    track: TrackId,
    device: &DeviceBox,
    actions: &mut Vec<Action>,
    nam: &crate::detail::nam_panel::Lists,
) {
    painter.rect_filled(rect, 0.0, theme::panel_background());
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW));
    painter.rect_filled(header, 0.0, theme::control_background());
    let switch = Rect::from_min_size(
        Pos2::new(header.left() + PAD, header.center().y - ON_SWITCH / 2.0),
        Vec2::splat(ON_SWITCH),
    );
    // The square is the bypass ([docs/spec/nam-amp-and-cab.md] › The panels): lit accent while
    // the device runs, an accent outline when it is bypassed and the sound passes it by.
    let bypass = ui.interact(switch, egui::Id::new(("device-switch", track.0, device.index)), Sense::click());
    if device.enabled {
        painter.rect_filled(switch, 0.0, theme::accent());
    } else {
        painter.rect_stroke(switch, 0.0, theme::hairline(theme::accent()), egui::StrokeKind::Inside);
    }
    if bypass.clicked() {
        actions.push(Action::new(
            "device.setEnabled",
            serde_json::json!({ "track": track.0, "device": device.index, "enabled": !device.enabled }),
        ));
    }
    bypass.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Bypass {}", device.name)));
    bypass.on_hover_text("Bypass: the sound passes this device by");
    let name_color = if device.enabled { theme::text_primary() } else { theme::text_hint() };
    let kind_width = widgets::text_width(ui, &device.kind, theme::ui_font(theme::TEXT_TINY));
    let name = Rect::from_min_max(
        Pos2::new(switch.right() + PAD, header.top()),
        Pos2::new((header.right() - PAD * 2.0 - kind_width).max(switch.right() + PAD), header.bottom()),
    );
    widgets::text_in(painter, name, Align2::LEFT_CENTER, &device.name, theme::ui_font(theme::TEXT), name_color);
    widgets::text_in(
        painter,
        Rect::from_min_max(
            Pos2::new(header.right() - PAD - kind_width, header.top()),
            Pos2::new(header.right() - PAD, header.bottom()),
        ),
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
            return;
        }
        Some("nam-cab-loader") => {
            crate::detail::nam_panel::show_cab(ui, body, track, device, nam, actions);
            return;
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
        return;
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
        return;
    }
    if let Some(graph) = &device.graph {
        ui.scope_builder(egui::UiBuilder::new().max_rect(body.intersect(ui.clip_rect())), |ui| {
            egui::ScrollArea::both().auto_shrink([false,false]).id_salt(("sd-controls",track.0,device.index)).show(ui, |ui| {
                let hidden = graph.canvas.as_ref().map_or(&[][..],|c|c.hidden_parameters.as_slice());
                let cells = parameter_layout(&graph.parameters,body.width(),hidden);
                let custom_size=graph.canvas.as_ref().map_or(Vec2::ZERO,|c|Vec2::new(c.size[0],c.size[1]));
                let nodes_height = graph.nodes.len() as f32 * NODE_ROW;
                let canvas = cells.iter().fold(Vec2::new(body.width().max(custom_size.x),0.0),|size,rect| Vec2::new(size.x.max(rect.right()),size.y.max(rect.bottom())));
                let (_, body) = ui.allocate_space(Vec2::new(canvas.x,canvas.y+nodes_height+custom_size.y));
                let painter = &ui.painter_at(body.intersect(ui.clip_rect()));
                let mut row_top = body.top();
        for (index, node) in graph.nodes.iter().enumerate() {
            let row = Rect::from_min_size(Pos2::new(body.left(), row_top), Vec2::new(body.width(), NODE_ROW));
            if row.bottom() > body.bottom() + 0.5 {
                break;
            }
            let node_rect = Rect::from_min_size(row.min, Vec2::new((row.width() - PAD) * 0.78, NODE_ROW - PAD));
            painter.rect_filled(node_rect, 1.0, theme::control_background());
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
        if let Some(canvas)=&graph.canvas {
            let rect=Rect::from_min_size(body.min+Vec2::new(0.0,nodes_height),custom_size);
            crate::detail::device_canvas::show(ui,rect,canvas,&graph.parameters,track,device.index,actions);
        }
        for (parameter, cell) in graph.parameters.iter().filter(|p|!hidden.contains(&p.id)).zip(&cells) {
            let row = cell.translate(Vec2::new(body.left(),body.top()+nodes_height+custom_size.y));
            let font = egui::FontId::new(parameter.ui.font_size,if parameter.ui.monospace { egui::FontFamily::Monospace } else { egui::FontFamily::Proportional });
            let foreground = parameter.ui.text_color.map_or_else(theme::text_secondary,|[r,g,b]| egui::Color32::from_rgb(r,g,b));
            let radius = match parameter.ui.shape {
                suhina_device_sdk::ControlShape::Square => 0.0,
                suhina_device_sdk::ControlShape::Pill => 128.0,
                suhina_device_sdk::ControlShape::Rounded => parameter.ui.corner_radius,
            };
            if let Some([r,g,b]) = parameter.ui.panel_color {
                painter.rect_filled(row,radius,egui::Color32::from_rgb(r,g,b));
            }
            let row = row.shrink(4.0);
            let label_height = parameter.ui.font_size+4.0;
            let name_rect = match parameter.ui.label_position {
                suhina_device_sdk::LabelPosition::Left => Rect::from_min_size(row.min,Vec2::new(row.width()*0.34,row.height())),
                suhina_device_sdk::LabelPosition::Top => Rect::from_min_size(row.min,Vec2::new(row.width(),label_height)),
                suhina_device_sdk::LabelPosition::Hidden => Rect::from_min_size(row.min,Vec2::ZERO),
            };
            if parameter.ui.label_position != suhina_device_sdk::LabelPosition::Hidden {
            widgets::text_in(
                painter,
                name_rect,
                if parameter.ui.label_position == suhina_device_sdk::LabelPosition::Top { Align2::CENTER_CENTER } else { Align2::LEFT_CENTER },
                &if parameter.ui.group.is_empty() {
                    parameter.name.clone()
                } else {
                    format!("{} / {}", parameter.ui.group, parameter.name)
                },
                font.clone(),
                foreground,
            );
            }
            let value_height = if parameter.ui.show_value && parameter.ui.label_position == suhina_device_sdk::LabelPosition::Top {
                parameter.ui.font_size + 4.0
            } else {
                0.0
            };
            let value_width = if parameter.ui.show_value && value_height == 0.0 { (row.width()*0.24).min(80.0) } else { 0.0 };
            let control_min = match parameter.ui.label_position {
                suhina_device_sdk::LabelPosition::Left => Pos2::new(name_rect.right(),row.top()),
                suhina_device_sdk::LabelPosition::Top => Pos2::new(row.left(),name_rect.bottom()),
                suhina_device_sdk::LabelPosition::Hidden => row.min,
            };
            let slider_rect = Rect::from_min_max(control_min,Pos2::new(row.right()-value_width,row.bottom()-value_height));
            let mut value = parameter.value;
            let response = (parameter.min < parameter.max).then(|| {
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(slider_rect)
                        .layout(egui::Layout::centered_and_justified(egui::Direction::TopDown)),
                    |ui| {
                        ui.style_mut().override_font_id = Some(font.clone());
                        let visuals = ui.visuals_mut();
                        visuals.override_text_color = Some(foreground);
                        let accent = parameter.ui.accent.map(|[r,g,b]| egui::Color32::from_rgb(r,g,b));
                        if let Some(accent) = accent {
                            visuals.selection.bg_fill = accent;
                            visuals.widgets.active.bg_fill = accent;
                            visuals.widgets.hovered.bg_stroke.color = accent;
                            visuals.widgets.active.fg_stroke.color = accent;
                        }
                        for style in [&mut visuals.widgets.inactive,&mut visuals.widgets.hovered,&mut visuals.widgets.active] {
                            style.corner_radius = egui::CornerRadius::same(radius as u8);
                            if let Some([r,g,b]) = parameter.ui.background { style.bg_fill = egui::Color32::from_rgb(r,g,b); style.weak_bg_fill = style.bg_fill; }
                            if let Some(accent) = accent { style.fg_stroke.color = accent; }
                        }
                        ui.set_clip_rect(ui.clip_rect().intersect(slider_rect));
                        ui.spacing_mut().slider_width = if parameter.control == suhina_device_sdk::ParameterControl::VerticalSlider { slider_rect.height() } else { slider_rect.width() };
                        ui.push_id((&parameter.id, device.index, track.0), |ui| {
                            let previous = value;
                            let mut response = match parameter.control {
                                suhina_device_sdk::ParameterControl::Toggle => {
                                    let mut checked = value >= 0.5;
                                    let response = ui.checkbox(&mut checked, "");
                                    value = f64::from(checked);
                                    response
                                }
                                suhina_device_sdk::ParameterControl::Button => {
                                    let response = ui.selectable_label(value >= 0.5, &parameter.name);
                                    if response.clicked() {
                                        value = if value >= 0.5 { 0.0 } else { 1.0 };
                                    }
                                    response
                                }
                                suhina_device_sdk::ParameterControl::Choice => {
                                    egui::ComboBox::from_id_salt("choice")
                                        .selected_text(
                                            parameter.ui.options.get(value as usize).map_or("", String::as_str),
                                        )
                                        .width(slider_rect.width())
                                        .show_ui(ui, |ui| {
                                            for (i, label) in parameter.ui.options.iter().enumerate() {
                                                ui.selectable_value(&mut value, i as f64, label);
                                            }
                                        })
                                        .response
                                }
                                suhina_device_sdk::ParameterControl::Radio => {
                                    ui.vertical_centered(|ui| {
                                        for (i, label) in parameter.ui.options.iter().enumerate() {
                                            ui.selectable_value(&mut value, i as f64, label);
                                        }
                                    })
                                    .response
                                }
                                suhina_device_sdk::ParameterControl::Number => ui.add(
                                    egui::DragValue::new(&mut value)
                                        .range(parameter.min..=parameter.max)
                                        .speed(if parameter.ui.step > 0.0 {
                                            parameter.ui.step
                                        } else {
                                            (parameter.max - parameter.min) / 200.0
                                        })
                                        .max_decimals(parameter.ui.decimals),
                                ),
                                suhina_device_sdk::ParameterControl::Knob
                                | suhina_device_sdk::ParameterControl::LogKnob => {
                                    let logarithmic = parameter.control == suhina_device_sdk::ParameterControl::LogKnob;
                                    let size = slider_rect.width().min(slider_rect.height()).max(8.0);
                                    let (rect, mut response) =
                                        ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click_and_drag());
                                    if response.clicked() { response.request_focus(); }
                                    if response.double_clicked() { value = parameter.default; }
                                    if response.has_focus() {
                                        let direction = ui.input(|i| {
                                            i32::from(i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowRight))
                                            - i32::from(i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowLeft))
                                        });
                                        let step = if parameter.ui.step > 0.0 { parameter.ui.step } else { (parameter.max-parameter.min)*0.01 };
                                        value = (value+direction as f64*step).clamp(parameter.min,parameter.max);
                                    }
                                    let mut position = if logarithmic {
                                        (value / parameter.min).ln() / (parameter.max / parameter.min).ln()
                                    } else {
                                        (value - parameter.min) / (parameter.max - parameter.min)
                                    };
                                    if response.dragged() {
                                        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
                                        position =
                                            (position - response.drag_delta().y as f64 * 0.005 * fine).clamp(0.0, 1.0);
                                        value = if logarithmic {
                                            parameter.min * (parameter.max / parameter.min).powf(position)
                                        } else {
                                            parameter.min + position * (parameter.max - parameter.min)
                                        };
                                        response.mark_changed();
                                    }
                                    let center=rect.center(); let knob_radius=size*9.0/22.0;
                                    let background=parameter.ui.background.map_or_else(theme::control_background,|[r,g,b]| egui::Color32::from_rgb(r,g,b));
                                    if parameter.ui.shape == suhina_device_sdk::ControlShape::Square {
                                        ui.painter().rect_filled(egui::Rect::from_center_size(center,Vec2::splat(knob_radius*2.0)),0.0,background);
                                    } else { ui.painter().circle_filled(center,knob_radius,background); }
                                    let border=egui::Stroke::new(1.0,accent.unwrap_or_else(theme::text_hint));
                                    if parameter.ui.shape == suhina_device_sdk::ControlShape::Square {
                                        ui.painter().rect_stroke(egui::Rect::from_center_size(center,Vec2::splat(knob_radius*2.0)),0.0,border,egui::StrokeKind::Inside);
                                    } else { ui.painter().circle_stroke(center,knob_radius,border); }
                                    let angle=(-135.0+270.0*position as f32).to_radians();
                                    let tip=center+Vec2::new(angle.sin(),-angle.cos())*(knob_radius-1.0);
                                    ui.painter().line_segment([center,tip],egui::Stroke::new(1.5,accent.unwrap_or_else(theme::text_primary)));
                                    response.widget_info(|| {
                                        egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, &parameter.name)
                                    });
                                    response
                                }
                                suhina_device_sdk::ParameterControl::VerticalSlider => {
                                    let rect = Rect::from_center_size(slider_rect.center(), Vec2::new(ui.spacing().interact_size.y, slider_rect.height()));
                                    let mut slider_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Center)));
                                    slider_ui.add(egui::Slider::new(&mut value, parameter.min..=parameter.max)
                                        .vertical().show_value(false)
                                        .trailing_fill(parameter.ui.accent.is_some())
                                        .max_decimals(parameter.ui.decimals)
                                        .step_by(parameter.ui.step))
                                }
                                control => ui.add(
                                    egui::Slider::new(&mut value, parameter.min..=parameter.max)
                                    .show_value(false)
                                    .trailing_fill(parameter.ui.accent.is_some())
                                    .max_decimals(parameter.ui.decimals)
                                    .logarithmic(control == suhina_device_sdk::ParameterControl::LogSlider)
                                    .step_by(
                                        if control == suhina_device_sdk::ParameterControl::Integer {
                                            1.0
                                        } else {
                                            parameter.ui.step
                                        },
                                    ),
                                ),
                            };
                            if value != previous {
                                response.mark_changed();
                            }
                            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue,true,&parameter.name));
                            response.on_hover_text(&parameter.ui.tooltip)
                        })
                        .inner
                    },
                )
                .inner
            });
            if parameter.ui.show_value { widgets::text_in(
                painter,
                if value_height > 0.0 {
                    Rect::from_min_max(Pos2::new(row.left(), slider_rect.bottom()), row.max)
                } else {
                    Rect::from_min_max(Pos2::new(slider_rect.right(), control_min.y), row.max)
                },
                if value_height > 0.0 { Align2::CENTER_CENTER } else { Align2::RIGHT_CENTER },
                if matches!(parameter.control,suhina_device_sdk::ParameterControl::Choice | suhina_device_sdk::ParameterControl::Radio) {
                    parameter.ui.options.get(value as usize).cloned().unwrap_or_default()
                } else { format!("{:.*} {}", parameter.ui.decimals, value, parameter.unit).trim().to_owned() }.as_str(),
                font.clone(),
                parameter.ui.text_color.map_or_else(theme::text_value,|[r,g,b]| egui::Color32::from_rgb(r,g,b)),
            ); }
            if response.as_ref().is_some_and(|response| response.changed() || response.drag_stopped()) {
                let continuous = response.as_ref().is_some_and(|response| {
                    (response.drag_started() || response.dragged()) && !response.drag_stopped()
                });
                actions.push(Action::new(
                    "device.setParam",
                    json!({ "track": track.0, "device": device.index, "param": parameter.id, "value": value, "continuous": continuous }),
                ));
            }
        }
            });
        });
        return;
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
}

/// The selected clip: its name and where it is, how long, and what it plays. In a row it takes
/// a column of [`CLIP_WIDTH`] at the left of `area`, stacked the rows it needs at the top;
/// returns the space it took.
fn clip_properties(
    ui: &Ui,
    painter: &egui::Painter,
    area: Rect,
    info: &ClipInfo,
    stacked: bool,
) -> (Rect, Option<Showing>) {
    let mut rows = vec![
        ("Track", info.track.clone()),
        (if info.scene { "Scene" } else { "Start" }, info.start.clone()),
        ("Length", info.length.clone()),
        ("Loop", if info.looping { "On".into() } else { "Off".into() }),
    ];
    if info.audio {
        rows.push(("Gain", info.gain_db.map_or_else(|| "-".into(), |db| format!("{} dB", widgets::format_db(db)))));
        let beats = |b: f64| if b > 0.0 { format!("{} beats", (b * 100.0).round() / 100.0) } else { "-".into() };
        rows.push(("Fade in", beats(info.fades.0)));
        rows.push(("Fade out", beats(info.fades.1)));
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
        rows.push(("Notes", notes));
        if let Some(from) = &selection.from {
            rows.push(("From", from.clone()));
        }
        if let Some(to) = &selection.to {
            rows.push(("To", to.clone()));
        }
        if let (Some(low), Some(high)) = (selection.low_key, selection.high_key) {
            let one = low == high;
            let low_name = crate::detail::piano_roll::key_name(low);
            rows.push((
                "Pitch",
                if one { low_name } else { format!("{low_name}–{}", crate::detail::piano_roll::key_name(high)) },
            ));
        }
        if let (Some(low), Some(high)) = (selection.low_velocity, selection.high_velocity) {
            rows.push(("Velocity", if low == high { low.to_string() } else { format!("{low}–{high}") }));
        }
    }
    let column = if stacked {
        let height = ROW + PAD * 2.0 + rows.len() as f32 * PARAM_ROW;
        Rect::from_min_size(area.min, Vec2::new(area.width(), height))
    } else {
        Rect::from_min_max(area.min, Pos2::new(area.left() + CLIP_WIDTH, area.bottom()))
    }
    .intersect(area);
    painter.rect_filled(column, 0.0, theme::panel_background());
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
    for (label, value) in rows {
        let row = Rect::from_min_max(Pos2::new(column.left() + PAD, y), Pos2::new(column.right() - PAD, y + PARAM_ROW));
        y += PARAM_ROW;
        if row.bottom() > column.bottom() + 0.5 {
            break;
        }
        // The row is named for a reader and a test ("Notes: 1 of 4"), as the piano roll's notes
        // are, so what the column says can be found by name.
        let response = ui.interact(row, egui::Id::new(("clip-property", label)), Sense::hover());
        let named = format!("{label}: {value}");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &named));
        // The value keeps clear of the label, so a long track name is shortened, not drawn over it.
        let value_rect = Rect::from_min_max(Pos2::new(row.left() + LABEL_WIDTH, row.top()), row.max);
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
        let on = motion::toward(ui.ctx(), response.id.with("on"), value == showing, motion::VALUE);
        let idle = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.rect_filled(cell, 0.0, motion::mix(idle, theme::text_value(), on));
        let color = motion::mix(theme::text_secondary(), theme::window_background(), on);
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_SMALL), color);
    }
    picked
}

fn parameter_height(parameter: &GraphParameter) -> f32 {
    if let Some([_, height]) = parameter.ui.size {
        return height;
    }
    let extra = if parameter.ui.label_position == suhina_device_sdk::LabelPosition::Top {
        parameter.ui.font_size + 4.0
    } else {
        0.0
    };
    extra
        + if parameter.ui.show_value && parameter.ui.label_position == suhina_device_sdk::LabelPosition::Top {
            parameter.ui.font_size + 4.0
        } else {
            0.0
        }
        + match parameter.control {
            suhina_device_sdk::ParameterControl::VerticalSlider => 80.0,
            suhina_device_sdk::ParameterControl::Knob | suhina_device_sdk::ParameterControl::LogKnob => 36.0,
            suhina_device_sdk::ParameterControl::Radio => {
                parameter.ui.options.len().max(1) as f32 * (parameter.ui.font_size + 16.0)
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_label_places_value_below_control_and_reserves_height() {
        let mut parameter = GraphParameter {
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
        let detail = build(&demo_project(), Some(TrackId(1)), Some(ClipRef::Arrangement(TrackId(1), 1)));
        let Detail::Clip(info, chain) = detail else { panic!("expected the clip") };
        assert_eq!(chain.track, TrackId(1));
        assert_eq!((info.name.as_str(), info.start.as_str(), info.length.as_str()), ("Beat B", "17.1", "12.0"));
        assert!(info.looping && !info.audio && !info.scene);
        assert_eq!(info.notes, 4);
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
        let Detail::Clip(_, with_clip) = build(&project, None, Some(ClipRef::Arrangement(TrackId(1), 0))) else {
            panic!()
        };
        assert_eq!(with_clip, chain);
        assert_eq!(build(&project, None, None), Detail::Nothing);
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
                show_stack(ui, &detail, &mut Vec::new(), &mut Vec::new());
            });
            assert!(!output.shapes.is_empty(), "{control:?}");
            output.textures_delta.clear();
        }
    }

    #[test]
    fn authored_control_positions_and_auto_flow_have_the_expected_extent() {
        use suhina_device_sdk::{ParameterControl, ParameterUi};
        let mut p = GraphParameter {
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
                    show_row(ui, &detail, &mut Vec::new(), &mut Vec::new());
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
                    show_stack(ui, &detail, &mut Vec::new(), &mut Vec::new());
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
                    show_stack(ui, &detail, &mut Vec::new(), &mut Vec::new());
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
                        show_stack(ui, &detail, &mut actions, places);
                    } else {
                        show_row(ui, &detail, &mut actions, places);
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
                    show_row(ui, &detail, &mut Vec::new(), places);
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
                    show_row(ui, &detail, &mut Vec::new(), places);
                },
                Vec::new(),
            );
        short.run();
        assert_eq!((short.state()[0].placement.clip_width, short.state()[0].placement.clip_height), (300, 50));
        let mut none = Harness::builder().with_size(egui::vec2(600.0, EDITOR_TOP - 1.0 + margin * 2.0)).build_ui_state(
            |ui, places: &mut Vec<EditorPlace>| {
                places.clear();
                show_row(ui, &detail, &mut Vec::new(), places);
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
                        show_stack(ui, &detail, actions, &mut places);
                    } else {
                        show_row(ui, &detail, actions, &mut places);
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
