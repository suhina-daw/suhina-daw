// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The empty space after the last track, in the Session view and the Arrangement view alike.

use egui::{Align2, Id, Pos2, Rect, Ui, Vec2};
use serde_json::json;

use crate::kit::motion;
use crate::kit::theme::{self, PAD, ROW};
use crate::kit::widgets::{self, Action};

/// The empty space's right-click menu on `space`, the response the view has for it: Insert
/// audio track, Insert MIDI track, Insert return track.
pub(crate) fn empty_space_menu(space: &egui::Response, actions: &mut Vec<Action>) {
    space.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Empty space"));
    space.context_menu(|ui| crate::interaction::menus::insert_items(ui, None, actions));
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
        let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.rect_filled(rect, 0.0, fill);
        widgets::text_in(painter, rect, Align2::CENTER_CENTER, label, font.clone(), theme::text_secondary());
        if response.clicked() {
            if kind == "return" {
                actions.push(Action::new("return.add", json!({})));
            } else {
                actions.push(Action::new("track.add", json!({ "kind": kind })));
            }
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
