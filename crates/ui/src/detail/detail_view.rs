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

use egui::{Align2, Id, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::json;
use suhina_device_sdk::builtin::{COMPACT_HEIGHT, COMPACT_WIDTH};
use suhina_device_sdk::{ControlShape, LabelPosition, ParameterControl};
use suhina_engine::model::{ClipId, DeviceKind, LaneData, Note, Project, StretchMode, TrackId};
use suhina_session::{Placement, PluginReport, PluginStatus};

use crate::interaction::commands;
use crate::interaction::grid::gain_to_db;
use crate::interaction::menus;
use crate::kit::motion;
use crate::kit::theme::{self, PAD, ROW};
use crate::kit::widgets::{self, Action};

mod clip_properties;
mod expanded;
mod group_panel;
mod model;
mod parameters;

use self::clip_properties::*;
pub use self::expanded::*;
use self::group_panel::*;
pub use self::model::*;
use self::parameters::*;

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

/// The height a device row needs to show an embedded editor `height` pixels tall under a
/// plug-in's line and button, at `pixels_per_point`.
pub fn row_height_with_editor(height: u32, pixels_per_point: f32) -> f32 {
    EDITOR_TOP + height as f32 / pixels_per_point.max(0.01) + PAD
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
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());
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
        painter.rect_filled(bar, 0.0, theme::control_background());
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
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());
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
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());
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
    painter.rect_filled(area, 0.0, theme::window_background());
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
    painter.rect_filled(rect, 0.0, crate::kit::motion::mix(theme::control_background(), theme::control_hover(), hover));
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
    painter.rect_filled(rect, 0.0, theme::panel_background());
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW));
    painter.rect_filled(header, 0.0, theme::control_background());
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

#[cfg(test)]
#[path = "../../../device-sdk/examples/ui_controls.rs"]
mod styled_gallery;

#[cfg(test)]
#[path = "../../../device-sdk/examples/orbit_filter.rs"]
mod custom_panel;

#[cfg(test)]
#[path = "detail_view_tests.rs"]
mod tests;
