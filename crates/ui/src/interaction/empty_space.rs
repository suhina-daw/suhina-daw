// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The empty space after the last track, in the Launcher view and the Arrangement view alike.

use egui::{Align2, Id, Pos2, Rect, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::TrackId;

use crate::kit::motion;
use crate::kit::theme::{self, PAD, ROW};
use crate::kit::widgets::{self, Action};

/// What the empty space's menu needs from the view: the selected tracks, which Cut, Copy and
/// Duplicate act on, and whether tracks are copied, for Paste.
pub(crate) struct EmptyMenu<'a> {
    pub selection: &'a [TrackId],
    pub tracks_copied: bool,
}

/// The empty space's right-click menu on `space`, the response the view has for it: Insert
/// audio track, Insert MIDI track, Insert return track, then Cut, Copy and Duplicate for the
/// selected tracks and Paste after the last regular track (#947).
pub(crate) fn empty_space_menu(space: &egui::Response, menu: &EmptyMenu<'_>, actions: &mut Vec<Action>) {
    space.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Empty space"));
    space.context_menu(|ui| {
        crate::interaction::menus::insert_items(ui, None, actions);
        ui.separator();
        let tracks: Vec<u32> = menu.selection.iter().map(|t| t.0).collect();
        for (label, keys, command) in [
            ("Cut", "Ctrl+X", "track.cut"),
            ("Copy", "Ctrl+C", "track.copy"),
            ("Duplicate", "Ctrl+D", "track.duplicate"),
        ] {
            let button = egui::Button::new(label).shortcut_text(crate::settings::keymap::shown(keys));
            if ui.add_enabled(!tracks.is_empty(), button).clicked() {
                actions.push(Action::new(command, json!({ "tracks": tracks })));
                ui.close();
            }
        }
        let paste = egui::Button::new("Paste").shortcut_text(crate::settings::keymap::shown("Ctrl+V"));
        if ui.add_enabled(menu.tracks_copied, paste).clicked() {
            actions.push(Action::new("track.paste", json!({ "after": null })));
            ui.close();
        }
    });
}

/// How many tracks Insert multiple offers to add until the user types another number.
const INSERT_MULTIPLE: u32 = 4;

/// The menu a right-click on **+ MIDI track** or **+ Audio track** opens: Insert multiple, a
/// field for how many (1 to 64) and Insert, which adds them in one undo step (#947).
fn insert_multiple_menu(response: &egui::Response, kind: &str, actions: &mut Vec<Action>) {
    response.context_menu(|ui| {
        let id = Id::new(("insert-multiple", kind));
        let mut count: u32 = ui.data(|d| d.get_temp(id)).unwrap_or(INSERT_MULTIPLE);
        ui.label("Insert multiple");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut count).range(1..=64).speed(0.2));
            let what = if kind == "midi" { "MIDI tracks" } else { "audio tracks" };
            ui.label(what);
        });
        ui.data_mut(|d| d.insert_temp(id, count));
        if ui.button("Insert").clicked() {
            actions.push(Action::new("track.add", json!({ "kind": kind, "count": count })));
            ui.close();
        }
    });
}

/// The labels of the buttons that add a track, by the kind each adds.
const BUTTONS: [(&str, &str); 3] = [("midi", "+ MIDI track"), ("audio", "+ Audio track"), ("return", "+ Return track")];

/// The width the empty space needs for its buttons to show whole: a view that scrolls sideways
/// leaves this much after its last track, so the buttons never shrink however many tracks
/// there are.
pub(crate) fn empty_space_width(ui: &Ui) -> f32 {
    let font = theme::ui_font(theme::TEXT_SMALL);
    BUTTONS.iter().map(|(_, label)| widgets::text_width(ui, label, font.clone())).fold(0.0, f32::max) + PAD * 4.0
}

/// The buttons that add an empty track or a return track, at the top of the empty space after
/// the tracks; the space below takes drops (`docs/spec/browser.md` › Files).
pub(crate) fn empty_space(ui: &Ui, painter: &egui::Painter, area: Rect, actions: &mut Vec<Action>) {
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mut y = area.top() + PAD;
    for (kind, label) in BUTTONS {
        let width = (widgets::text_width(ui, label, font.clone()) + PAD * 2.0).min(area.width() - PAD * 2.0);
        let rect = Rect::from_min_size(Pos2::new(area.left() + PAD, y), Vec2::new(width, ROW));
        y = rect.bottom() + 2.0;
        if rect.bottom() > area.bottom() || width <= 0.0 {
            break;
        }
        let response = widgets::button_area(ui, rect, Id::new(("add-track", kind, area.min.x as i32)), label);
        // `.add-track` styles the three, `#add-midi-track`, `#add-audio-track` and
        // `#add-return-track` one of them; `button.add-track` is their artwork.
        let name = format!("add-{kind}-track");
        let _style = crate::kit::style_class::enter_named_response("add-track", &name, &response);
        let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        let state = crate::kit::skin::State::response(&response, false);
        if !crate::kit::skin::paint(painter, rect, "add-track", state, theme::text_secondary(), 0.0) {
            crate::kit::native_css::surface(painter, rect, 0.0, fill);
        }
        let font = crate::kit::style_class::computed()
            .filter(|c| c.authored.font_size)
            .map_or_else(|| font.clone(), |c| theme::ui_font(c.font_size));
        widgets::text_in(
            painter,
            rect,
            Align2::CENTER_CENTER,
            label,
            font,
            crate::kit::style_class::foreground(theme::text_secondary()),
        );
        if response.clicked() {
            if kind == "return" {
                actions.push(Action::new("return.add", json!({})));
            } else {
                actions.push(Action::new("track.add", json!({ "kind": kind })));
            }
        }
        if kind != "return" {
            insert_multiple_menu(&response, kind, actions);
        }
    }
    if area.height() > y - area.top() + ROW * 2.0 {
        let hint = Rect::from_min_max(Pos2::new(area.left(), y), area.max);
        widgets::text_in(
            painter,
            hint.shrink(PAD),
            Align2::CENTER_CENTER,
            "Drop files or devices to add a track",
            font,
            theme::text_hint(),
        );
    }
}
