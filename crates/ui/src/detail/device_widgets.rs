// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a device panel's [`super::device_canvas`] draws from device state beside its
//! parameters: the device's own on/off switch in its body, the file rows and the fault note.
//! The device-SDK names them for layouts (`Widget::DeviceSwitch`, `Widget::DeviceFiles`,
//! `Widget::DeviceNote`), a built-in's layout places them, and every change they make is
//! still a `device.*` command ([docs/spec/nam-amp-and-cab.md] › The panels).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{Align2, Pos2, Rect, Sense, Ui, Vec2};
use suhina_device_sdk::ui::FileRowPart;
use suhina_engine::model::TrackId;

use crate::detail::detail_view::DeviceAt;
use crate::kit::surface::Backgrounds as _;
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

/// The pack the files beside the packs make.
pub const LOOSE_PACK: &str = "(entered directly)";

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
    /// The library's root (`Documents/Suhina`), where a device's file state can be a name
    /// relative to it (`Sounds/NAM/…`, [ADR 0036] › it) — the rows read that state the way
    /// this machine's lists carry it (`state_as_listed`).
    pub library: Option<PathBuf>,
}

/// What a canvas's device widgets read beside its parameters: the device's own state and,
/// for a file-backing device, the folders' lists. What drew the panel hands it each frame;
/// a canvas whose device places no device widgets hands the default state instead (no file,
/// nothing listed).
#[derive(Clone)]
pub struct PanelState<'a> {
    /// The device's shown name, for the controls' accessible names ([`PanelState::label`]).
    pub name: String,
    /// Whether the device runs: the body's switch says it with the header's square, and a
    /// click asks for `device.setEnabled`.
    pub enabled: bool,
    /// The file the device runs, as the project holds it; empty when none.
    pub file: &'a str,
    /// The folders' packs, a file's shown names and its failure notes (nam spec › Folders),
    /// or none when the device takes no file and its file rows and note stay empty.
    pub lists: Option<&'a Lists>,
}

impl<'a> PanelState<'a> {
    /// The state a canvas whose device has no body device controls reads: no file, running,
    /// no lists — the file controls draw nothing.
    #[cfg(test)]
    pub fn none() -> Self {
        Self { name: String::new(), enabled: true, file: "", lists: None }
    }

    /// The device's shown name without the maker prefix: "NAM Amp Modeler" gives
    /// "Amp Modeler", the panels' label prefix.
    pub fn label(&self) -> String {
        self.name.strip_prefix("NAM ").unwrap_or(&self.name).to_owned()
    }
}

/// The words a panel's rows read by its `what` (nam spec › The panels): the pack row's empty
/// text and the file row's own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Words {
    /// What the pack row shows when the device has no file ("No model").
    pub pack: &'static str,
    /// What the file row shows when the device has no file ("Choose a model file…").
    pub file: &'static str,
}

/// The words for `what` ("model", "cab"); `None` makes `DeviceFiles` and `DeviceNote`
/// controls draw nothing, so their widgets can be placed only where a device takes files.
pub fn words(what: &str) -> Option<Words> {
    match what {
        "model" => Some(Words { pack: "No model", file: "Choose a model file…" }),
        "cab" => Some(Words { pack: "No IR", file: "Choose a cab file…" }),
        _ => None,
    }
}

/// The folder's packs `what` reads: the models folder's for "model", the IR folder's for
/// "cab".
fn packs_of<'a>(lists: &'a Lists, what: &str) -> &'a [Set] {
    match what {
        "model" => &lists.models,
        "cab" => &lists.irs,
        _ => &[],
    }
}

/// The pack that lists the device's current file: a pack's name (the subfolder's, or the
/// loose files' "(entered directly)"), or nothing when no pack lists it (a changed folder,
/// a missing file). Membership, not the parent's name: the folders' loose files form the
/// loose pack, and a file somewhere else that shares a pack's folder name lists nowhere.
/// The current state reads the way the lists carry its file (`state_as_listed`): a relative
/// one joins the library's root, a shipped key stands whole against the shipped pack's rows
/// and, beside the unpacked copy that scan pinned, reads its own file's name there. The
/// texts compare by `canonical_paths`: this platform's join and the scan may fold their
/// own separators in, and the texts must read the same file ([ADR 0036] › 4).
fn pack_of_current<'a>(current: &str, sets: &'a [Set]) -> Option<&'a Set> {
    if current.is_empty() {
        return None;
    }
    let current = canonical_paths(current);
    if let Some(pack) = sets.iter().find(|pack| pack.files.iter().any(|(path, _)| canonical_paths(path) == current)) {
        return Some(pack);
    }
    let (_, file) = current.strip_prefix("builtin:")?.split_once('/')?;
    let file_name = Path::new(file).file_name()?.to_string_lossy().into_owned();
    sets.iter().find(|pack| {
        pack.name == suhina_device_registry::SHIPPED_PACK
            && pack
                .files
                .iter()
                .any(|(path, _)| Path::new(path).file_name().is_some_and(|n| n.to_string_lossy() == file_name))
    })
}

/// The salt a device widget scopes its state and popups by: the device on the track, the
/// part fits one per panel.
fn salt(track: TrackId, at: &DeviceAt, part: FileRowPart) -> egui::Id {
    egui::Id::unique(("device-files", track.0, &at.0, part))
}

/// A shipped key beside the unpacked copy reads its disk row: the dropdown's own row and the
/// file steps match the disk path, not the key — a stock preset's Next file steps to the
/// successor, not the pack's first. Without the copy the pack's rows are the keys, and the
/// lookup finds the key's own.
fn listed_of_disk_row(listed: String, in_pack: Option<&Set>) -> String {
    let Some((_, file)) = listed.strip_prefix("builtin:").and_then(|rest| rest.split_once('/')) else {
        return listed;
    };
    let Some(pack) = in_pack else { return listed };
    let file_name = Path::new(file).file_name().map_or_else(|| file.to_owned(), |n| n.to_string_lossy().into_owned());
    pack.files
        .iter()
        .find(|(path, _)| Path::new(path).file_name().is_some_and(|n| n.to_string_lossy() == file_name))
        .map_or_else(|| listed, |(path, _)| path.clone())
}

/// Where `current` sits in `files`. The two texts read the same file by
/// `canonical_paths` — the rows' whole paths and the state's joined form fold what
/// separators their own platform made in ([ADR 0036] › 4).
fn index_of(files: &[(String, String)], current: &str) -> Option<usize> {
    let current = canonical_paths(current);
    files.iter().position(|(path, _)| canonical_paths(path) == current)
}

/// The device's file state as the lists carry its file: a shipped key stands whole (the
/// shipped pack's rows and the pack lookup read it), a name relative to the library joins
/// this machine's root — the state a project carries names the same file wherever it opens,
/// [ADR 0036] › it — an absolute one stands. The panel rows compare this form by
/// `canonical_paths`, both sides; the row's own reads (the shown name, the fault note) stay
/// on the state as the project holds it.
fn state_as_listed(state: &str, lists: &Lists) -> String {
    if state.starts_with("builtin:") || Path::new(state).is_absolute() {
        return state.to_owned();
    }
    lists.library.as_ref().map_or_else(|| state.to_owned(), |base| base.join(state).display().to_string())
}

/// The texts the lists' whole paths and a state's joined form compare by: path components
/// joined with `/` ([ADR 0036] › 4, the portable form). A Windows join folds backslashes
/// in, the mac's does not, and both must read the same file; the compare does not care
/// which separator either text was quoted with.
fn canonical_paths(text: &str) -> String {
    std::path::Path::new(text)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
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

/// The shown name of the device's current file: the session's ("Fender Super Reverb 1965")
/// or the file's name without its folder or extension ("Test amp"), whichever separator the
/// path uses.
fn file_shown(current: &str, lists: &Lists) -> String {
    match lists.names.get(current) {
        Some(shown) if !shown.is_empty() => shown.clone(),
        _ => std::path::Path::new(current)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

/// The failure note a `DeviceNote` control shows for its device's file: the load's message,
/// or the missing file's line, or nothing when everything is well.
fn note_of(state: &PanelState<'_>) -> Option<String> {
    let lists = state.lists?;
    let file = state.file;
    if file.is_empty() {
        return None;
    }
    if let Some(note) = lists.notes.get(file) {
        return Some(note.clone());
    }
    (!lists.names.contains_key(file)).then(|| format!("Model missing: {file}"))
}

/// `device.setNamFile` for the file the row asked for, when it differs from the device's
/// current.
fn set_file(actions: &mut Vec<Action>, track: TrackId, at: &DeviceAt, current: &str, ask: Option<String>) {
    if let Some(path) = ask.filter(|candidate| candidate != current) {
        actions.push(Action::new(
            "device.setNamFile",
            serde_json::json!({ "track": track.0, "device": at.value(), "path": path }),
        ));
    }
}

/// The device's on/off as a control in its body (nam spec › The panels): the header's
/// square, bigger, lit while the device runs; a click turns the whole device off or on
/// again, as the `0` key does.
pub(crate) fn switch(
    ui: &Ui,
    rect: Rect,
    state: &PanelState<'_>,
    track: TrackId,
    at: &DeviceAt,
    actions: &mut Vec<Action>,
) {
    let label = state.label();
    let id = egui::Id::unique(("device-switch", track.0, &at.0));
    let response = ui.interact(rect, id, Sense::click()).on_hover_text(if state.enabled {
        format!("Click to switch the device off; the {} keeps its settings", label)
    } else {
        format!("Click to enable the {} again", label)
    });
    let (fill, text) = widgets::toggle_colors(ui, &response, state.enabled);
    ui.painter().fill_background(rect, 3.0, fill);
    let word = if state.enabled { "On" } else { "Off" };
    widgets::text_in(ui.painter(), rect, Align2::CENTER_CENTER, word, theme::ui_font(theme::TEXT_SMALL), text);
    if response.clicked() {
        actions.push(Action::new(
            "device.setEnabled",
            serde_json::json!({ "track": track.0, "device": at.value(), "enabled": !state.enabled }),
        ));
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{}: device {}", label, word.to_lowercase()))
    });
}

/// A `DeviceFiles` control: the row its `part` places, at `rect`'s bounds. The dropdowns'
/// pills show the current pack and the file's own name (nam spec › Folders › Packs), the
/// steppers step through them wrapping. Choices and steps run `device.setNamFile`, one undo
/// step, only when the file they ask for differs from the device's current.
#[allow(clippy::too_many_arguments)] // the row's bounds, part, words, state and dispatch
pub(crate) fn files(
    ui: &mut Ui,
    rect: Rect,
    part: FileRowPart,
    what: &str,
    state: &PanelState<'_>,
    track: TrackId,
    at: &DeviceAt,
    actions: &mut Vec<Action>,
) {
    let Some(words) = words(what) else { return };
    let Some(lists) = state.lists else { return };
    let current = state.file;
    let label = state.label();
    // The salt scopes this row's popup and its two steppers to the device; the parts keep
    // one control each, so a panel's two dropdowns never share a salt.
    let salt = salt(track, at, part);
    let packs = packs_of(lists, what);
    // The state reads as the lists carry it, for every lookup between the rows and them.
    let listed = state_as_listed(current, lists);
    let in_pack = pack_of_current(&listed, packs);
    // A shipped key beside the unpacked copy reads its disk row: the dropdown's own row and
    // the file steps match the disk path, not the key.
    let listed = listed_of_disk_row(listed, in_pack);
    let shown_pack = in_pack.map_or(String::new(), |pack| pack.name.clone());
    let files: &[(String, String)] = in_pack.map_or(&[], |pack| pack.files.as_slice());
    let packs_rows = || packs.iter().map(|pack| (pack.name.clone(), pack.name.clone())).collect::<Vec<_>>();
    let first_of = |pack_name: &str| {
        packs
            .iter()
            .find(|pack| pack.name == pack_name)
            .and_then(|pack| pack.files.first().map(|(path, _)| path.clone()))
    };
    let ask: Option<String> = match part {
        FileRowPart::Pack => {
            let rows = packs_rows();
            let picked = dropdown(
                ui,
                rect,
                salt,
                &rows,
                &shown_pack,
                &shown_pack,
                words.pack,
                &format!("{label}: pack"),
                Some("folder"),
            );
            picked.as_deref().and_then(first_of)
        }
        FileRowPart::File => {
            let shown = file_shown(current, lists);
            dropdown(ui, rect, salt, files, &listed, &shown, words.file, &format!("{label}: {what} file"), None)
        }
        FileRowPart::PreviousPack | FileRowPart::NextPack => {
            let up = part == FileRowPart::PreviousPack;
            let step = if up { -1 } else { 1 };
            let label = if up { format!("{label}: previous pack") } else { format!("{label}: next pack") };
            let rows = packs_rows();
            let clicked = stepper(ui, rect, salt, up, label).clicked();
            clicked.then(|| step_through(&rows, &shown_pack, step)).flatten().and_then(|name| first_of(&name))
        }
        FileRowPart::PreviousFile | FileRowPart::NextFile => {
            let up = part == FileRowPart::PreviousFile;
            let step = if up { -1 } else { 1 };
            let label = if up { format!("{label}: previous file") } else { format!("{label}: next file") };
            let clicked = stepper(ui, rect, salt, up, label).clicked();
            clicked.then(|| step_through(files, &listed, step)).flatten()
        }
    };
    set_file(actions, track, at, current, ask);
}

/// A dropdown pill in `rect`, its shown text and the rows under it (`rows` lists each as
/// (path, label); the pack row's paths are the packs' names). The shown text falls back to
/// the path's own name; with no rows, the empty text alone. The pack row draws the design
/// system's folder artwork before its name (`icon`), so the two pickers read apart. Returns
/// the row's path a click asked for.
#[allow(clippy::too_many_arguments)] // the labels, current value, rows and artwork of one dropdown
fn dropdown(
    ui: &mut Ui,
    rect: Rect,
    id_salt: egui::Id,
    rows: &[(String, String)],
    current: &str,
    shown: &str,
    empty: &str,
    label: &str,
    icon: Option<&str>,
) -> Option<String> {
    let mut row = match icon {
        Some(role) => {
            // The folder's icon sits in the row's own margin: artwork a theme keeps
            // changeable (nam spec › The panels), the size the pill's text starts after.
            let icon_rect = Rect::from_min_size(Pos2::new(rect.left() + 8.0, rect.center().y - 7.0), Vec2::splat(14.0));
            crate::kit::skin::paint(
                ui.painter(),
                icon_rect,
                role,
                crate::kit::skin::State::Normal,
                theme::text_secondary(),
                0.0,
            );
            ui.new_child(egui::UiBuilder::new().max_rect(rect.with_min_x(rect.left() + 24.0)))
        }
        None => ui.new_child(egui::UiBuilder::new().max_rect(rect)),
    };
    let current_name = if current.is_empty() {
        empty.to_owned()
    } else if shown.is_empty() {
        std::path::Path::new(current).file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default()
    } else {
        shown.to_owned()
    };
    let mut choice: Option<String> = None;
    // Cut short to the row, never drawn under the steppers beside it: the icon's own margin,
    // when the row draws one, comes out of the same bounds.
    let width = rect.width() - 16.0 - if icon.is_some() { 24.0 } else { 0.0 };
    let response = crate::kit::widgets::combo_box(
        &mut row,
        egui::ComboBox::from_id_salt(id_salt)
            .width(width.max(24.0))
            .truncate()
            .selected_text(egui::RichText::new(current_name).font(theme::ui_font(theme::TEXT_SMALL))),
        |ui| {
            if rows.is_empty() {
                let empty_label = ui
                    .selectable_label(false, egui::RichText::new(empty).font(theme::ui_font(theme::TEXT_TINY)).weak());
                empty_label
                    .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, format!("{label}: {empty}")));
            }
            for (path, name) in rows {
                let mut on = canonical_paths(path) == canonical_paths(current);
                let item = ui.selectable_value(
                    &mut on,
                    true,
                    egui::RichText::new(name).font(theme::ui_font(theme::TEXT_SMALL)),
                );
                if item.changed() {
                    choice = Some(path.clone());
                }
                item.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, format!("{label}: {name}")));
            }
        },
    )
    .response;
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, label));
    choice
}

/// A stepper button drawn in `rect`: a triangle pointing up or down, answering by its label.
fn stepper(ui: &mut Ui, rect: Rect, id_salt: egui::Id, up: bool, label: String) -> egui::Response {
    let painter = ui.painter();
    let (fill, _) = widgets::toggle_colors(ui, &ui.interact(rect, egui::Id::unique(id_salt), Sense::click()), false);
    let response = ui.interact(rect, egui::Id::unique(id_salt), Sense::click());
    painter.fill_background(rect, 2.0, if response.hovered() { theme::control_hover() } else { fill });
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
    response.widget_info(move || egui::WidgetInfo::labeled(egui::Role::Button, true, label.clone()));
    response
}

/// The fault note a `DeviceNote` control shows: one line, plain text, over its `rect`;
/// nothing when the device's file runs well.
pub(crate) fn note(painter: &egui::Painter, rect: Rect, state: &PanelState<'_>) {
    if let Some(text) = note_of(state) {
        widgets::text_in(
            painter,
            rect,
            Align2::LEFT_CENTER,
            &text,
            theme::ui_font(theme::TEXT_TINY),
            theme::text_hint(),
        );
    }
}

#[cfg(test)]
#[path = "device_widgets_tests.rs"]
mod tests;
