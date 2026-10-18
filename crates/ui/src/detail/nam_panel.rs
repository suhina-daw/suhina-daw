// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The NAM Amp Modeler's and NAM Cab Loader's panels
//! ([docs/spec/nam-amp-and-cab.md] › The panels): the name row's dropdown with the folder's
//! files and the steppers, the knobs and toggles in the middle, the file dropdown at the
//! bottom, the failure note. Suhina's own look — theme tokens, a straight grid, no Live-style
//! chrome — and everything a `device.*` command.

use std::collections::HashMap;

use egui::{Align2, Rect, Sense, Ui};
use suhina_device_api::{ParamInfo, Unit};
use suhina_device_nam_amp_modeler::INFO as AMP_INFO;
use suhina_device_nam_cab_loader::INFO as CAB_INFO;
use suhina_engine::model::TrackId;

use crate::detail::detail_view::DeviceBox;
use crate::kit::theme;
use crate::kit::widgets::{self, Action};

/// A pack ([docs/spec/nam-amp-and-cab.md] › Folders › Packs): a subfolder of the device's
/// folder, an amp or cab with several files. Files beside the packs form the
/// "(entered directly)" pack.
#[derive(Clone, Debug, PartialEq)]
pub struct Set {
    /// The pack's name: its folder's (or "(entered directly)" for the loose files).
    pub name: String,
    /// The pack's files: (path, shown name), name-sorted.
    pub files: Vec<(String, String)>,
}

/// The folders' packs and a device file's shown names and failure notes, provided each frame
/// by the app ([docs/spec/nam-amp-and-cab.md] › Folders).
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Lists {
    /// The models folder's packs.
    pub models: Vec<Set>,
    /// The IR folder's packs.
    pub irs: Vec<Set>,
    /// A file's shown name — the model's metadata name or the cab's pack's — by path.
    pub names: HashMap<String, String>,
    /// Why a device's file does not run, by path: what a load gave.
    pub notes: HashMap<String, String>,
}

/// The pack the files beside the packs make.
pub const LOOSE_PACK: &str = "(entered directly)";

/// The width a NAM panel asks for in the chain's row layout: the mock's two-column controls
/// with the knobs' labels and values beside each other.
pub const NAM_WIDTH: f32 = 440.0;

/// The height a NAM panel asks for in the chain, both layouts: the name row, the controls and
/// the file row at the mock's spacing, whatever the parameter count is.
pub const NAM_HEIGHT: f32 = 216.0;

const KNOB: f32 = 34.0;
const STEP: f32 = 16.0;
const ROW_HEIGHT: f32 = 22.0;
const PAD: f32 = 8.0;
const CELL: f32 = 62.0;

/// Is this the parameter a toggle shows lit when its value is on (`>= 0.5`)?
fn on_of(device: &DeviceBox, parameter: &ParamInfo) -> bool {
    value_of(device, parameter) >= 0.5
}

/// The parameter with id `id`, from a device's static info.
fn parameter_of(params: &'static [ParamInfo], id: &str) -> &'static ParamInfo {
    params.iter().find(|p| p.id == id).unwrap_or(&params[0])
}

/// The parameter with id `id`, from the amp's info.
fn amp_parameter(id: &str) -> &'static ParamInfo {
    parameter_of(AMP_INFO.params, id)
}

/// The parameter with id `id`, from the cab's info.
fn cab_parameter(id: &str) -> &'static ParamInfo {
    parameter_of(CAB_INFO.params, id)
}

/// The label's prefix a panel's controls take: the device's shown name, "NAM " off if it leads.
fn prefix_of(device: &DeviceBox) -> String {
    let name = device.name.as_str();
    name.strip_prefix("NAM ").unwrap_or(name).to_owned()
}

/// The value the device holds for `parameter` (the default when it holds none).
fn value_of(device: &DeviceBox, parameter: &ParamInfo) -> f64 {
    device.values.iter().find(|(id, _)| *id == parameter.id).map_or(parameter.default, |(_, value)| *value)
}

/// `device.setParam` for a panel change.
fn set_param(
    actions: &mut Vec<Action>,
    track: TrackId,
    device: usize,
    parameter: &ParamInfo,
    value: f64,
    continuous: bool,
) {
    actions.push(Action::new(
        "device.setParam",
        serde_json::json!({
            "track": track.0, "device": device, "param": parameter.id,
            "value": parameter.clamp(value), "continuous": continuous,
        }),
    ));
}

/// A parameter's value with its unit, as the value lines show it.
fn shown_value(parameter: &ParamInfo, value: f64) -> String {
    let value = if value.is_finite() { value } else { parameter.default };
    match parameter.unit {
        Unit::Decibels => format!("{value:+.1} dB"),
        Unit::Hertz => format!("{value:.0} Hz"),
        Unit::Seconds => format!("{:.2} ms", value * 1000.0),
        _ => format!("{value:.2}"),
    }
}

/// A knob in `cell`, its label over it, its value under it. A drag turns it (one
/// `device.setParam` with `continuous` on), a double-click returns to the default, the arrow
/// keys step while it has focus.
#[allow(clippy::too_many_arguments)]
fn knob(
    ui: &Ui,
    cell: Rect,
    track: TrackId,
    device_index: usize,
    prefix: &str,
    parameter: &'static ParamInfo,
    value: f64,
    actions: &mut Vec<Action>,
) -> egui::Response {
    let publisher = format!("{}: {}", prefix, parameter.name.to_lowercase());
    let response =
        ui.interact(cell, egui::Id::new(("nam-knob", track.0, device_index, parameter.id)), Sense::click_and_drag());
    if response.clicked() {
        response.request_focus();
    }
    let mut at = parameter.to_normalized(value);
    let mut change: Option<(f64, bool)> = None;
    if response.dragged() {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        at = (at - f64::from(response.drag_delta().y) * 0.005 * fine).clamp(0.0, 1.0);
        change = Some((parameter.from_normalized(at), true));
    } else if response.double_clicked() {
        change = Some((parameter.default, false));
    } else if response.has_focus() {
        let up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowRight));
        let down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowLeft));
        if up != down {
            let step = (parameter.max - parameter.min) * 0.01;
            let next = value + f64::from(up as i8) * step - f64::from(down as i8) * step;
            change = Some((parameter.clamp(next), false));
        }
    }
    let circle = (cell.height() - 28.0).clamp(20.0, KNOB);
    let circle_center = egui::pos2(cell.center().x, cell.top() + 12.0 + circle / 2.0);
    widgets::knob(ui.painter(), circle_center, circle, at as f32, theme::text_primary());
    widgets::text_in(
        ui.painter(),
        Rect::from_min_max(cell.left_top(), egui::pos2(cell.right(), cell.top() + 12.0)),
        Align2::CENTER_CENTER,
        parameter.name,
        theme::ui_font(theme::TEXT_TINY),
        theme::text_secondary(),
    );
    widgets::text_in(
        ui.painter(),
        Rect::from_min_max(egui::pos2(cell.left(), cell.bottom() - 14.0), cell.max),
        Align2::CENTER_CENTER,
        &shown_value(parameter, change.as_ref().map_or(value, |(v, _)| *v)),
        theme::mono_font(theme::TEXT_TINY),
        theme::text_value(),
    );
    if let Some((next, continuous)) = change {
        set_param(actions, track, device_index, parameter, next, continuous);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, &publisher));
    response
}

/// The Mid Freq's control under the amp's trio, as the mock draws it: the name over, a line
/// with a diamond handle at the value's place, the value under. A drag along the line sets it,
/// a click jumps, a double-click returns to the default, the arrows step while it has focus.
#[allow(clippy::too_many_arguments)]
fn mid_freq_line(
    ui: &Ui,
    under: Rect,
    track: TrackId,
    device_index: usize,
    prefix: &str,
    parameter: &'static ParamInfo,
    value: f64,
    actions: &mut Vec<Action>,
) -> egui::Response {
    let publisher = format!("{}: {}", prefix, parameter.name.to_lowercase());
    let painter = ui.painter();
    widgets::text_in(
        painter,
        Rect::from_min_max(under.left_top(), egui::pos2(under.right(), under.top() + 12.0)),
        Align2::CENTER_CENTER,
        parameter.name,
        theme::ui_font(theme::TEXT_TINY),
        theme::text_secondary(),
    );
    let width = 150.0_f32.min(under.width() - 16.0).max(60.0);
    let line = Rect::from_min_max(
        egui::pos2(under.center().x - width / 2.0, under.top() + 16.0),
        egui::pos2(under.center().x + width / 2.0, under.top() + 18.0),
    );
    painter.rect_filled(line, 1.0, theme::control_background());
    let response = ui.interact(
        Rect::from_min_max(
            egui::pos2(line.left() - 8.0, under.top() + 10.0),
            egui::pos2(line.right() + 8.0, under.bottom()),
        ),
        egui::Id::new(("nam-pill", track.0, device_index, parameter.id)),
        Sense::click_and_drag(),
    );
    if response.clicked() {
        response.request_focus();
    }
    let mut change: Option<(f64, bool)> = None;
    if response.dragged() || response.clicked() {
        if let Some(at) = response.interact_pointer_pos() {
            let normalized = ((at.x - line.left()) / line.width()).clamp(0.0, 1.0);
            change = Some((parameter.from_normalized(f64::from(normalized)), response.dragged()));
        }
    } else if response.double_clicked() {
        change = Some((parameter.default, false));
    } else if response.has_focus() {
        let up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowRight));
        let down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowLeft));
        if up != down {
            let step = (parameter.max - parameter.min) * 0.01;
            let next = value + f64::from(up as i8) * step - f64::from(down as i8) * step;
            change = Some((parameter.clamp(next), false));
        }
    }
    let at = change.as_ref().map_or(value, |(next, _)| *next);
    let at = parameter.to_normalized(at) as f32;
    let handle = egui::pos2(line.left() + at * line.width(), line.center().y);
    let half = 4.0;
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(handle.x, handle.y - half),
            egui::pos2(handle.x + half, handle.y),
            egui::pos2(handle.x, handle.y + half),
            egui::pos2(handle.x - half, handle.y),
        ],
        theme::text_value(),
        egui::Stroke::NONE,
    ));
    widgets::text_in(
        painter,
        Rect::from_min_max(egui::pos2(under.left(), line.bottom() + 1.0), egui::pos2(under.right(), under.bottom())),
        Align2::CENTER_CENTER,
        &shown_value(parameter, change.as_ref().map_or(value, |(v, _)| *v)),
        theme::mono_font(theme::TEXT_TINY),
        theme::text_value(),
    );
    if let Some((next, continuous)) = change {
        set_param(actions, track, device_index, parameter, next, continuous);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, &publisher));
    response
}

/// The Cab Loader's vertical Volume slider in `column`: a lane with a handle a drag moves,
/// the value under it.
#[allow(clippy::too_many_arguments)]
fn vslider(
    ui: &Ui,
    column: Rect,
    track: TrackId,
    device_index: usize,
    prefix: &str,
    parameter: &'static ParamInfo,
    value: f64,
    actions: &mut Vec<Action>,
) -> egui::Response {
    let publisher = format!("{}: {}", prefix, parameter.name.to_lowercase());
    let at = parameter.to_normalized(value) as f32;
    widgets::text_in(
        ui.painter(),
        Rect::from_min_max(column.left_top(), egui::pos2(column.right(), column.top() + 12.0)),
        Align2::CENTER_CENTER,
        parameter.name,
        theme::ui_font(theme::TEXT_TINY),
        theme::text_secondary(),
    );
    let lane = Rect::from_min_max(
        egui::pos2(column.center().x - 3.0, column.top() + 16.0),
        egui::pos2(column.center().x + 3.0, column.bottom() - 20.0),
    );
    ui.painter().rect_filled(lane, 1.5, theme::control_background());
    let handle_y = lane.bottom() - at * lane.height();
    ui.painter().rect_filled(
        Rect::from_min_max(
            egui::pos2(lane.center().x - 7.0, handle_y - 3.0),
            egui::pos2(lane.center().x + 7.0, handle_y + 3.0),
        ),
        1.5,
        theme::text_primary(),
    );
    let response = ui.interact(
        Rect::from_min_max(
            egui::pos2(lane.center().x - 12.0, lane.top()),
            egui::pos2(lane.center().x + 12.0, lane.bottom()),
        ),
        egui::Id::new(("nam-vslider", track.0, device_index, parameter.id)),
        Sense::click_and_drag(),
    );
    if response.dragged() {
        let relative = (1.0
            - (response.interact_pointer_pos().map_or(lane.top(), |p| p.y) - lane.top()) / lane.height())
        .clamp(0.0, 1.0);
        set_param(actions, track, device_index, parameter, parameter.from_normalized(f64::from(relative)), true);
    }
    widgets::text_in(
        ui.painter(),
        Rect::from_min_max(egui::pos2(column.left(), lane.bottom() + 2.0), egui::pos2(column.right(), column.bottom())),
        Align2::CENTER_CENTER,
        &shown_value(parameter, value),
        theme::mono_font(theme::TEXT_TINY),
        theme::text_value(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, &publisher));
    response
}

/// A lit-or-dark button (EQ, HQ, Phase, HP), one `device.setParam` per click.
#[allow(clippy::too_many_arguments)]
fn toggle(
    ui: &Ui,
    rect: Rect,
    track: TrackId,
    device_index: usize,
    prefix: &str,
    parameter: &'static ParamInfo,
    on: bool,
    actions: &mut Vec<Action>,
) -> egui::Response {
    let id = egui::Id::new(("nam-toggle", track.0, device_index, parameter.id));
    let response = ui.interact(rect, id, Sense::click());
    let (fill, text) = widgets::toggle_colors(ui, &response, on);
    ui.painter().rect_filled(rect, 3.0, fill);
    widgets::text_in(
        ui.painter(),
        rect,
        Align2::CENTER_CENTER,
        parameter.name,
        theme::ui_font(theme::TEXT_SMALL),
        text,
    );
    if response.clicked() {
        set_param(actions, track, device_index, parameter, f64::from(!on), false);
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{}: {}", prefix, parameter.name.to_lowercase()),
        )
    });
    response
}

/// A file's dropdown pill in `rect`, its shown text and the files under it. Returns the row's
/// response; `picked` set when the panel asks for a switch.
#[allow(clippy::too_many_arguments)]
fn file_row(
    ui: &mut Ui,
    rect: Rect,
    id_salt: egui::Id,
    files: &[(String, String)],
    current: &str,
    shown: &str,
    empty: &str,
    label: &str,
) -> (egui::Response, Option<String>) {
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let current_name = if current.is_empty() {
        empty.to_owned()
    } else if shown.is_empty() {
        std::path::Path::new(current).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
    } else {
        shown.to_owned()
    };
    let mut choice: Option<String> = None;
    let response = egui::ComboBox::from_id_salt(id_salt)
        .width(rect.width() - 52.0)
        .selected_text(egui::RichText::new(current_name).font(theme::ui_font(theme::TEXT_SMALL)))
        .show_ui(&mut row, |ui| {
            if files.is_empty() {
                let empty_label = ui
                    .selectable_label(false, egui::RichText::new(empty).font(theme::ui_font(theme::TEXT_TINY)).weak());
                empty_label.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{label}: {empty}"))
                });
            }
            for (path, name) in files {
                let mut on = path == current;
                let item = ui.selectable_value(
                    &mut on,
                    true,
                    egui::RichText::new(name).font(theme::ui_font(theme::TEXT_SMALL)),
                );
                if item.changed() {
                    choice = Some(path.clone());
                }
                item.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{label}: {name}"))
                });
            }
        })
        .response;
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, label));
    (response, choice)
}

/// Where `current` sits in `files`.
fn index_of(files: &[(String, String)], current: &str) -> Option<usize> {
    files.iter().position(|(path, _)| path == current)
}

/// Steps `current` through `files` by `by` (wrapping); a file that is not in the list steps
/// from the ends.
fn step_through(files: &[(String, String)], current: &str, by: i64) -> Option<String> {
    let len = files.len();
    if len == 0 {
        return None;
    }
    let index = match (index_of(files, current), by) {
        (Some(at), by) => (at as i64 + by + len as i64).rem_euclid(len as i64) as usize,
        (None, 1) => 0,
        (None, _) => len - 1,
    };
    files.get(index).map(|(path, _)| path.clone())
}

/// A stepper button drawn in `rect`: a triangle pointing `up` or `down`.
fn stepper(ui: &mut Ui, rect: Rect, id_salt: egui::Id, up: bool, label: String) -> egui::Response {
    let painter = ui.painter();
    let (fill, _) = widgets::toggle_colors(ui, &ui.interact(rect, egui::Id::new(id_salt), Sense::click()), false);
    let response = ui.interact(rect, egui::Id::new(id_salt), Sense::click());
    painter.rect_filled(rect, 2.0, if response.hovered() { theme::control_hover() } else { fill });
    let inner = rect.shrink(5.0);
    let center = inner.center();
    let dir = if up { -1.0 } else { 1.0 };
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(center.x, center.y + 2.5 * dir),
            egui::pos2(center.x - 3.5, center.y - 1.5 * dir),
            egui::pos2(center.x + 3.5, center.y - 1.5 * dir),
        ],
        theme::text_hint(),
        egui::Stroke::NONE,
    ));
    response.widget_info(move || egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone()));
    response
}

/// `rect` split into `cells` even columns.
fn split(rect: Rect, cells: usize) -> Vec<Rect> {
    let gap = 2.0;
    let each = ((rect.width() - gap * (cells.saturating_sub(1) as f32)) / cells.max(1) as f32).min(CELL + 24.0);
    (0..cells)
        .map(|c| {
            Rect::from_min_size(
                egui::pos2(rect.left() + c as f32 * (each + gap), rect.top()),
                egui::vec2(each, rect.height()),
            )
        })
        .collect()
}

/// The failure note a panel shows for its device's file: the load's message, or the missing
/// file's line, or nothing when everything is well.
fn note_of(device: &DeviceBox, lists: &Lists) -> Option<String> {
    let file = device.file.as_deref().unwrap_or("");
    if file.is_empty() {
        return None;
    }
    if let Some(note) = lists.notes.get(file) {
        return Some(note.clone());
    }
    (!lists.names.contains_key(file)).then(|| format!("Model missing: {file}"))
}

/// The pack the device's current file is in, by its parent folder: a pack's name, the loose
/// files' pack, or nothing when the file is not one of the folder's packs (a changed folder,
/// a missing file).
fn pack_of_current<'a>(current: &str, sets: &'a [Set]) -> Option<&'a Set> {
    let parent = std::path::Path::new(current).parent()?.file_name()?.to_string_lossy().into_owned();
    sets.iter().find(|pack| pack.name == parent)
}

/// The pack row's and the file row's picks and the steppers, for one panel in its `rect`'s
/// rows. `what` is the panel's word for its file ("model", "cab"), `empty` the rows' empty
/// text. Returns the file a click asked for, when one differs from the device's current.
#[allow(clippy::too_many_arguments)]
fn rows_and_picks(
    ui: &mut Ui,
    rect: Rect,
    top: f32,
    bottom: f32,
    track: TrackId,
    device: &DeviceBox,
    sets: &[Set],
    what: &str,
    empty: &str,
) -> Option<String> {
    let (wide, left) = (rect.width(), rect.left());
    let prefix = prefix_of(device);
    let current = device.file.as_deref().unwrap_or("");
    let salt = (track.0, device.index);
    let in_pack = pack_of_current(current, sets);
    let shown_pack = in_pack.map_or(String::new(), |pack| pack.name.clone());
    // The pack row: the current pack, the folder's packs under, the steppers beside.
    let pack_rect =
        Rect::from_min_max(egui::pos2(left, top), egui::pos2(left + wide - 2.0 * STEP - 12.0, top + ROW_HEIGHT));
    let packs: Vec<(String, String)> = sets.iter().map(|pack| (pack.name.clone(), pack.name.clone())).collect();
    let (_, pack_pick) = file_row(
        ui,
        pack_rect,
        egui::Id::new(("nam-pack", salt)),
        &packs,
        &shown_pack,
        &shown_pack,
        empty,
        &format!("{prefix}: pack"),
    );
    let up_rect = Rect::from_min_size(egui::pos2(left + wide - 2.0 * STEP - 10.0, top), egui::vec2(STEP, ROW_HEIGHT));
    let down_rect = Rect::from_min_size(egui::pos2(left + wide - STEP - 4.0, top), egui::vec2(STEP, ROW_HEIGHT));
    let up = stepper(ui, up_rect, egui::Id::new(("nam-step-up", salt)), true, format!("{prefix}: previous pack"));
    let down = stepper(ui, down_rect, egui::Id::new(("nam-step-down", salt)), false, format!("{prefix}: next pack"));
    // A pack's choice asks for its first file; a pack stepper asks for the pack it steps to's.
    let pack_step = if up.clicked() {
        step_through(&packs, &shown_pack, -1)
    } else if down.clicked() {
        step_through(&packs, &shown_pack, 1)
    } else {
        pack_pick.clone()
    };
    let mut ask = pack_step
        .as_ref()
        .and_then(|pack_name| sets.iter().find(|pack| &pack.name == pack_name))
        .and_then(|pack| pack.files.first().map(|(path, _)| path.clone()));
    // The file row, at the bottom: the current pack's files, the file's own name shown, its
    // own steppers ("…previous file", "…next file").
    let files = in_pack.map_or(&[][..], |pack| pack.files.as_slice());
    let file_rect =
        Rect::from_min_max(egui::pos2(left, bottom - ROW_HEIGHT), egui::pos2(left + wide - 2.0 * STEP - 12.0, bottom));
    let file_up = Rect::from_min_size(
        egui::pos2(left + wide - 2.0 * STEP - 10.0, bottom - ROW_HEIGHT),
        egui::vec2(STEP, ROW_HEIGHT),
    );
    let file_down =
        Rect::from_min_size(egui::pos2(left + wide - STEP - 4.0, bottom - ROW_HEIGHT), egui::vec2(STEP, ROW_HEIGHT));
    let file_shown =
        if current.is_empty() { String::new() } else { current.rsplit('/').next().unwrap_or(current).to_owned() };
    let (_, file_pick) = file_row(
        ui,
        file_rect,
        egui::Id::new(("nam-file", salt)),
        files,
        current,
        &file_shown,
        &format!("Choose a {what} file…"),
        &format!("{prefix}: {what} file"),
    );
    let file_up_clicked =
        stepper(ui, file_up, egui::Id::new(("nam-file-step-up", salt)), true, format!("{prefix}: previous file"))
            .clicked();
    let file_down_clicked =
        stepper(ui, file_down, egui::Id::new(("nam-file-step-down", salt)), false, format!("{prefix}: next file"))
            .clicked();
    ask = ask.or_else(|| file_up_clicked.then(|| step_through(files, current, -1)).flatten());
    ask = ask.or_else(|| file_down_clicked.then(|| step_through(files, current, 1)).flatten());
    ask = ask.or(file_pick);
    ask.filter(|candidate| Some(candidate.as_str()) != device.file.as_deref())
}

/// The rows a panel's controls sit between: under its name row, over its file row.
fn controls_rect(rect: Rect, top: f32, bottom: f32) -> Rect {
    Rect::from_min_max(
        egui::pos2(rect.left(), top + ROW_HEIGHT + 2.0),
        egui::pos2(rect.right(), bottom - ROW_HEIGHT - 2.0),
    )
}

/// The NAM Amp Modeler's panel, drawn in `rect` ([docs/spec/nam-amp-and-cab.md] › The panels):
/// Gain at the left, the EQ's trio with the Mid Freq's value and the EQ and HQ buttons under
/// it in the middle, Delay and Volume at the right.
pub(crate) fn show_amp(
    ui: &mut Ui,
    rect: Rect,
    track: TrackId,
    device: &DeviceBox,
    lists: &Lists,
    actions: &mut Vec<Action>,
) {
    let prefix = prefix_of(device);
    let top = rect.top();
    let bottom = rect.bottom();
    if let Some(pick) = rows_and_picks(ui, rect, top, bottom, track, device, &lists.models, "model", "No model") {
        actions.push(Action::new(
            "device.setNamFile",
            serde_json::json!({ "track": track.0, "device": device.index, "path": pick }),
        ));
    }
    let controls = controls_rect(rect, top, bottom);
    // The Gain and Volume columns at the sides; the middle holds the trio, the Mid Freq's
    // line and the EQ button.
    let rows_top = controls.top();
    let (left, right) = (60.0, 60.0);
    let gain_cell =
        Rect::from_min_max(egui::pos2(controls.left(), rows_top), egui::pos2(controls.left() + left, rows_top + 56.0));
    knob(
        ui,
        gain_cell.shrink(2.0),
        track,
        device.index,
        &prefix,
        amp_parameter("inputGain"),
        value_of(device, amp_parameter("inputGain")),
        actions,
    );
    let volume_cell = Rect::from_min_max(
        egui::pos2(controls.right() - right, rows_top),
        egui::pos2(controls.right(), rows_top + 56.0),
    );
    knob(
        ui,
        volume_cell.shrink(2.0),
        track,
        device.index,
        &prefix,
        amp_parameter("outputVolume"),
        value_of(device, amp_parameter("outputVolume")),
        actions,
    );
    let middle_rect = Rect::from_min_max(
        egui::pos2(controls.left() + left + 4.0, rows_top),
        egui::pos2(controls.right() - right - 4.0, controls.bottom()),
    );
    let trio = Rect::from_min_max(middle_rect.left_top(), egui::pos2(middle_rect.right(), middle_rect.top() + 56.0));
    let cells = split(trio, 3);
    for (cell, id) in cells.iter().zip(["eqBass", "eqMid", "eqTreble"]) {
        let parameter = amp_parameter(id);
        knob(ui, cell.shrink(2.0), track, device.index, &prefix, parameter, value_of(device, parameter), actions);
    }
    // Under the trio: the Mid Freq's line with its handle, then the EQ button under that. The
    // Delay knob and the HQ button share a narrow column at the trio's right.
    let mid_freq_rect = Rect::from_min_max(
        egui::pos2(middle_rect.left(), trio.bottom() + 2.0),
        egui::pos2(middle_rect.left() + middle_rect.width() - 78.0, trio.bottom() + 30.0),
    );
    mid_freq_line(
        ui,
        mid_freq_rect,
        track,
        device.index,
        &prefix,
        amp_parameter("eqMidFreq"),
        value_of(device, amp_parameter("eqMidFreq")),
        actions,
    );
    let eq_row = Rect::from_min_max(
        egui::pos2(
            middle_rect.left(),
            (mid_freq_rect.bottom() + 2.0).min((controls.bottom() - ROW_HEIGHT).max(mid_freq_rect.top() + ROW_HEIGHT)),
        ),
        egui::pos2(mid_freq_rect.right(), controls.bottom()),
    );
    toggle(
        ui,
        Rect::from_center_size(egui::pos2(eq_row.left() + 32.0, eq_row.center().y), egui::vec2(48.0, ROW_HEIGHT)),
        track,
        device.index,
        &prefix,
        amp_parameter("eqOn"),
        on_of(device, amp_parameter("eqOn")),
        actions,
    );
    // The narrow column at the middle's right: the Delay's knob, the HQ button under it.
    let aux = Rect::from_min_max(
        egui::pos2(mid_freq_rect.right() + 8.0, trio.top()),
        egui::pos2(middle_rect.right(), controls.bottom()),
    );
    knob(
        ui,
        Rect::from_min_max(aux.left_top(), egui::pos2(aux.right(), aux.top() + 56.0)).shrink(2.0),
        track,
        device.index,
        &prefix,
        amp_parameter("delay"),
        value_of(device, amp_parameter("delay")),
        actions,
    );
    let hq_cell = Rect::from_center_size(egui::pos2(aux.center().x, aux.top() + 70.0), egui::vec2(48.0, ROW_HEIGHT));
    toggle(ui, hq_cell, track, device.index, &prefix, amp_parameter("hq"), on_of(device, amp_parameter("hq")), actions);
    // The failure note, beside the file row's left, over the file row's line.
    if let Some(note) = note_of(device, lists) {
        widgets::text_in(
            ui.painter(),
            Rect::from_min_max(
                egui::pos2(rect.left(), bottom - ROW_HEIGHT - 14.0),
                egui::pos2(rect.right() - PAD, bottom - ROW_HEIGHT),
            ),
            Align2::LEFT_CENTER,
            &note,
            theme::ui_font(theme::TEXT_TINY),
            theme::text_hint(),
        );
    }
}

/// The NAM Cab Loader's panel, drawn in `rect` ([docs/spec/nam-amp-and-cab.md] › The panels):
/// Ø and HP at the middle's top, the cuts' knobs, the Mid Freq knob under them, Delay at the
/// left and the vertical Volume slider at the right.
pub(crate) fn show_cab(
    ui: &mut Ui,
    rect: Rect,
    track: TrackId,
    device: &DeviceBox,
    lists: &Lists,
    actions: &mut Vec<Action>,
) {
    let prefix = prefix_of(device);
    let top = rect.top();
    let bottom = rect.bottom();
    if let Some(pick) = rows_and_picks(ui, rect, top, bottom, track, device, &lists.irs, "cab", "No IR") {
        actions.push(Action::new(
            "device.setNamFile",
            serde_json::json!({ "track": track.0, "device": device.index, "path": pick }),
        ));
    }
    let controls = controls_rect(rect, top, bottom);
    // The Delay column at the left and the Volume slider at the right; the middle holds the
    // Ø and HP buttons, the cuts' knobs and the Mid Freq knob under them.
    let (left, right) = (64.0, 52.0);
    let delay_cell = Rect::from_min_max(
        egui::pos2(controls.left(), controls.top()),
        egui::pos2(controls.left() + left, controls.top() + 62.0),
    );
    knob(
        ui,
        delay_cell.shrink(2.0),
        track,
        device.index,
        &prefix,
        cab_parameter("delay"),
        value_of(device, cab_parameter("delay")),
        actions,
    );
    let volume_cell = Rect::from_min_max(
        egui::pos2(controls.right() - right, controls.top() + 12.0),
        egui::pos2(controls.right(), controls.bottom()),
    );
    vslider(
        ui,
        volume_cell,
        track,
        device.index,
        &prefix,
        cab_parameter("volume"),
        value_of(device, cab_parameter("volume")),
        actions,
    );
    let middle_rect = Rect::from_min_max(
        egui::pos2(controls.left() + left + 4.0, controls.top()),
        egui::pos2(controls.right() - right - 4.0, controls.bottom()),
    );
    // The Ø and HP buttons.
    let toggles =
        Rect::from_min_max(middle_rect.left_top(), egui::pos2(middle_rect.right(), middle_rect.top() + ROW_HEIGHT));
    let cells = split(toggles, 2);
    for (cell, id) in cells.iter().zip(["phase", "hpOn"]) {
        let parameter = cab_parameter(id);
        toggle(
            ui,
            Rect::from_center_size(
                cell.center().max(egui::pos2(cell.left() + 24.0, cell.center().y)),
                egui::vec2(48.0, ROW_HEIGHT),
            ),
            track,
            device.index,
            &prefix,
            parameter,
            on_of(device, parameter),
            actions,
        );
    }
    // The cuts' knobs, then the Mid Freq's knob under them.
    let cuts = Rect::from_min_max(
        egui::pos2(middle_rect.left(), toggles.bottom() + 2.0),
        egui::pos2(middle_rect.right(), toggles.bottom() + 46.0),
    );
    let cells = split(cuts, 3);
    for (cell, id) in cells.iter().zip(["cutBass", "cutMid", "cutTreble"]) {
        let parameter = cab_parameter(id);
        knob(ui, cell.shrink(2.0), track, device.index, &prefix, parameter, value_of(device, parameter), actions);
    }
    let mid_freq_rect = Rect::from_min_max(
        egui::pos2(middle_rect.left(), cuts.bottom() + 2.0),
        egui::pos2(middle_rect.right(), cuts.bottom() + 66.0).min(egui::pos2(middle_rect.right(), controls.bottom())),
    );
    let mid_freq_cell =
        Rect::from_center_size(mid_freq_rect.center(), egui::vec2(CELL + 20.0, mid_freq_rect.height().min(62.0)));
    knob(
        ui,
        mid_freq_cell,
        track,
        device.index,
        &prefix,
        cab_parameter("cutMidFreq"),
        value_of(device, cab_parameter("cutMidFreq")),
        actions,
    );
    // The failure note, beside the file row's left, over the file row's line.
    if let Some(note) = note_of(device, lists) {
        widgets::text_in(
            ui.painter(),
            Rect::from_min_max(
                egui::pos2(rect.left(), bottom - ROW_HEIGHT - 14.0),
                egui::pos2(rect.right() - PAD, bottom - ROW_HEIGHT),
            ),
            Align2::LEFT_CENTER,
            &note,
            theme::ui_font(theme::TEXT_TINY),
            theme::text_hint(),
        );
    }
}
