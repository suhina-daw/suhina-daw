// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rows of a right-click or drop-down menu, drawn by the kit so a theme styles them as it
//! does the clip panel's lists: an entry is a `button[kind="entry"]` (`[checked]` while it is
//! the one chosen, with the pointer's fill under the pointer), a line between groups a
//! `separator[kind="menu"]`, a heading a `label[kind="caption"]` and a note a
//! `label[kind="hint"]`. They go inside a menu egui opens (`Response::context_menu`,
//! `egui::Popup::menu`); a submenu opens with egui's own [`egui::containers::menu::SubMenu`]
//! from the kit's row, so the keyboard and the pointer move between them as in any menu.

use egui::{Align2, Pos2, Rect, Response, Role, Sense, Ui, Vec2, WidgetInfo};

use crate::kit::element::{BUTTON, Checked, LABEL, SEPARATOR};
use crate::kit::{theme, widgets};

/// How far a row's words sit from its edges.
const INSET: f32 = 8.0;
/// The room at a row's right end for a submenu's arrow.
const ARROW: f32 = 14.0;
/// How tall a line between groups is.
const LINE: f32 = 7.0;

/// An entry of a menu, `text` on it, `checked` while it is the one chosen: the response, which
/// the caller reads (`clicked()`, then `ui.close()`).
pub fn item(ui: &mut Ui, text: &str, checked: bool) -> Response {
    let response = row(ui, text, checked, 0.0);
    response.widget_info(|| WidgetInfo::selected(Role::Button, true, checked, text));
    response
}

/// An entry that opens a submenu of `content` beside it, its arrow at its right end; `marked`
/// draws a dot before the arrow, for a submenu that holds the one chosen. The row's response.
pub fn submenu(ui: &mut Ui, text: &str, marked: bool, content: impl FnOnce(&mut Ui)) -> Response {
    let response = row(ui, text, false, ARROW);
    response.widget_info(|| WidgetInfo::labeled(Role::MenuItem, true, text));
    let ink = BUTTON.attribute("kind", "entry").response(&response).look().color();
    let arrow = Pos2::new(response.rect.right() - INSET, response.rect.center().y);
    let painter = ui.painter();
    // Pointing right: the submenu opens beside the row.
    widgets::chevron(painter, arrow, false, ink);
    if marked {
        painter.circle_filled(Pos2::new(arrow.x - 10.0, arrow.y), 2.0, ink);
    }
    egui::containers::menu::SubMenu::new().show(ui, &response, content);
    response
}

/// Words that head a group of entries and take no click.
pub fn heading(ui: &mut Ui, text: &str) {
    let ink = LABEL.attribute("kind", "caption").look().color();
    words(ui, text, ink);
}

/// A note at the end of a menu, dim, that takes no click.
pub fn hint(ui: &mut Ui, text: &str) {
    let ink = LABEL.attribute("kind", "hint").look().color();
    words(ui, text, ink);
}

/// A line between groups of entries.
pub fn separator(ui: &mut Ui) {
    let (line, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), LINE), Sense::hover());
    let ink = SEPARATOR.attribute("kind", "menu").look().color();
    ui.painter().hline(line.x_range(), line.center().y, theme::hairline(ink));
}

/// A row of a menu as wide as the menu or its words (the menu grows to the widest), `extra`
/// more at its right end: lit with the pointer's fill under the pointer, with the chosen one's
/// fill while `checked`, its words in the entry's `color` (on the chosen fill, the colour that
/// reads on it unless the theme gives one).
fn row(ui: &mut Ui, text: &str, checked: bool, extra: f32) -> Response {
    let font = theme::ui_font(theme::TEXT);
    let width = widgets::text_width(ui, text, font.clone()) + 2.0 * INSET + extra;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width.max(ui.available_width()), theme::ROW), Sense::click());
    let entry = BUTTON.attribute("kind", "entry");
    let look = entry.response(&response).attr(Checked, checked).look();
    let fill = if checked {
        look.background()
    } else if response.hovered() || response.has_focus() {
        BUTTON.attribute("kind", "entry").pointer(true, false, false).look().background()
    } else {
        egui::Color32::TRANSPARENT
    };
    if fill != egui::Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 2.0, fill);
    }
    let ink = if checked { look.own_color_or(theme::text_on(look.background())) } else { look.color() };
    let words = Rect::from_min_max(rect.min + Vec2::new(INSET, 0.0), rect.max - Vec2::new(INSET + extra, 0.0));
    widgets::text_in(ui.painter(), words, Align2::LEFT_CENTER, text, font, ink);
    response
}

/// One line of `text` in `ink` that takes no click, as wide as it needs.
fn words(ui: &mut Ui, text: &str, ink: egui::Color32) {
    let font = theme::ui_font(theme::TEXT);
    let width = widgets::text_width(ui, text, font.clone()) + 2.0 * INSET;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width.max(ui.available_width()), theme::ROW), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, text));
    widgets::text_in(ui.painter(), rect.shrink2(Vec2::new(INSET, 0.0)), Align2::LEFT_CENTER, text, font, ink);
}

#[cfg(test)]
#[path = "menu_tests.rs"]
mod tests;
