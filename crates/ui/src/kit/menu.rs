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
    let response = row(ui, text, checked, 0.0, ENTRY);
    response.widget_info(|| WidgetInfo::selected(Role::Button, true, checked, text));
    response
}

/// A compact entry of a context menu, as the piano roll's are: `text` in the small font, the key
/// that does the same (`key`, as the keymap labels it) at its right end in the hint ink, grayed
/// and taking no click while `enabled` is false. The response, which the caller reads
/// (`clicked()`, then `ui.close()`). The menu sizes to its widest entry, so it goes in a popup
/// with a justified layout.
pub fn action(ui: &mut Ui, text: &str, key: Option<&str>, enabled: bool) -> Response {
    // As tall as the menu's rows are set (a compact menu sets them lower than a row).
    let height = ui.spacing().interact_size.y.max(1.0);
    let face = Face { small: true, height, shortcut: key, enabled };
    let response = row(ui, text, false, 0.0, face);
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, text));
    response
}

/// An entry that opens a submenu of `content` beside it, its arrow at its right end; `marked`
/// draws a dot before the arrow, for a submenu that holds the one chosen. The row's response.
pub fn submenu(ui: &mut Ui, text: &str, marked: bool, content: impl FnOnce(&mut Ui)) -> Response {
    let response = row(ui, text, false, ARROW, ENTRY);
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

/// How a row is drawn besides its words: in the small font (a compact context menu's) or the
/// menu's, how tall, the key that does the same at its right end, and whether it takes a click.
#[derive(Clone, Copy)]
struct Face<'a> {
    small: bool,
    height: f32,
    shortcut: Option<&'a str>,
    enabled: bool,
}

/// An entry of a menu: the menu's font, a row tall, no key, clickable.
const ENTRY: Face<'static> = Face { small: false, height: theme::ROW, shortcut: None, enabled: true };

/// The room between a row's words and its key.
const KEY_GAP: f32 = 16.0;

/// A row of a menu as wide as the menu or its words (the menu grows to the widest), `extra`
/// more at its right end: lit with the pointer's fill under the pointer, with the chosen one's
/// fill while `checked`, its words in the entry's `color` (on the chosen fill, the color that
/// reads on it unless the theme gives one). `face` says its font and height, and the key shown
/// at its right end in the hint ink; a row that is not enabled takes no click and is drawn in
/// the entry's disabled color (the hint ink unless the theme gives one).
fn row(ui: &mut Ui, text: &str, checked: bool, extra: f32, face: Face<'_>) -> Response {
    let font = theme::ui_font(if face.small { theme::TEXT_SMALL } else { theme::TEXT });
    let key_font = theme::mono_font(theme::TEXT_SMALL);
    let key_width = face.shortcut.map_or(0.0, |key| widgets::text_width(ui, key, key_font.clone()) + KEY_GAP);
    let width = widgets::text_width(ui, text, font.clone()) + key_width + 2.0 * INSET + extra;
    let sense = if face.enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width.max(ui.available_width()), face.height), sense);
    let entry = BUTTON.attribute("kind", "entry");
    let look = entry.response(&response).attr(Checked, checked).look();
    let fill = if checked {
        look.background()
    } else if face.enabled && (response.hovered() || response.has_focus()) {
        BUTTON.attribute("kind", "entry").pointer(true, false, false).look().background()
    } else {
        egui::Color32::TRANSPARENT
    };
    if fill != egui::Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 2.0, fill);
    }
    let hint = crate::kit::element::hint_ink();
    let ink = if !face.enabled {
        let disabled = BUTTON.attribute("kind", "entry").pointer(false, false, true).look().color();
        if disabled == look.color() { hint } else { disabled }
    } else if checked {
        look.own_color_or(theme::text_on(look.background()))
    } else {
        look.color()
    };
    let inside = Rect::from_min_max(rect.min + Vec2::new(INSET, 0.0), rect.max - Vec2::new(INSET + extra, 0.0));
    if let Some(key) = face.shortcut {
        let key_ink = if face.enabled { hint } else { hint.gamma_multiply(0.6) };
        widgets::text_in(ui.painter(), inside, Align2::RIGHT_CENTER, key, key_font, key_ink);
    }
    let words = Rect::from_min_max(inside.min, inside.max - Vec2::new(key_width, 0.0));
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
