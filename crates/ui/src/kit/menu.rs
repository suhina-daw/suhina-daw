// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The rows of a right-click or drop-down menu, drawn by the kit so a theme styles them as it
//! does the clip panel's lists: an entry is a `list-row[kind="menu"]` (`[selected]` while it is
//! the one chosen, `:hover` under the pointer), a choice among a row of them (a grid's widths) a
//! `button[kind="entry"]` drawn alike, a line between groups a
//! `separator[kind="menu"]`, a heading a `label[kind="caption"]` and a note a
//! `label[kind="hint"]`. They go inside a menu egui opens (`Response::context_menu`,
//! `egui::Popup::menu`); a submenu opens with egui's own [`egui::containers::menu::SubMenu`]
//! from the kit's row, so the keyboard and the pointer move between them as in any menu.

use egui::{Align2, Pos2, Rect, Response, Role, Sense, Ui, Vec2, WidgetInfo};

use crate::kit::element::{BUTTON, Checked, LABEL, LIST_ROW, SEPARATOR, Selected};
use crate::kit::{controls, theme, widgets};

/// How far a row's words sit from its edges.
const INSET: f32 = 8.0;
/// The room at a row's right end for a submenu's arrow.
const ARROW: f32 = 14.0;
/// How tall a line between groups is.
const LINE: f32 = 7.0;

/// An entry of a menu, `text` on it, `checked` while it is the one chosen: the response, which
/// the caller reads (`clicked()`, then `ui.close()`).
pub fn item(ui: &mut Ui, text: &str, checked: bool) -> Response {
    Entry::new(text).chosen(checked).show(ui)
}

/// An entry of a menu that does something (not a choice): `text` on it, the key that does the
/// same (`key`, as the keymap shows it) at its right end in the hint ink, grayed and taking no
/// click while `enabled` is false. The response, which the caller reads (`clicked()`, then
/// `ui.close()`).
pub fn entry(ui: &mut Ui, text: &str, key: Option<&str>, enabled: bool) -> Response {
    Entry::new(text).key(key).enabled(enabled).show(ui)
}

/// An entry that turns something on or off (a check box in a menu), checked while `on`: the
/// response, which the caller reads (`clicked()` turns it the other way).
pub fn toggle(ui: &mut Ui, text: &str, on: bool) -> Response {
    Entry::new(text).check(on).show(ui)
}

/// A compact entry of a context menu, as the piano roll's are: `text` in the small font, the key
/// that does the same (`key`, as the keymap labels it) at its right end in the hint ink, grayed
/// and taking no click while `enabled` is false. The response, which the caller reads
/// (`clicked()`, then `ui.close()`). The menu sizes to its widest entry, so it goes in a popup
/// with a justified layout.
pub fn action(ui: &mut Ui, text: &str, key: Option<&str>, enabled: bool) -> Response {
    Entry::new(text).key(key).enabled(enabled).compact().show(ui)
}

/// An entry that opens a submenu of `content` beside it, its arrow at its right end; `marked`
/// draws a dot before the arrow, for a submenu that holds the one chosen. The row's response.
pub fn submenu(ui: &mut Ui, text: &str, marked: bool, content: impl FnOnce(&mut Ui)) -> Response {
    Entry::new(text).marked(marked).submenu(ui, content)
}

/// What an entry is besides a row that takes a click.
#[derive(Clone, Copy, PartialEq)]
enum Mark {
    /// It does something.
    Action,
    /// One of a set of choices, chosen or not.
    Choice(bool),
    /// A check box, on or off.
    Check(bool),
}

/// An entry of a menu, set up step by step and drawn with [`Entry::show`] (or
/// [`Entry::submenu`], for one that opens a submenu): what [`item`], [`entry`], [`toggle`],
/// [`action`] and [`submenu`] draw, for the rows that need more than one of them says (a grayed
/// choice, a key beside a check box, a value at the right end, a name read out that differs from
/// the words shown).
#[derive(Clone, Copy)]
pub struct Entry<'a> {
    text: &'a str,
    name: Option<&'a str>,
    trailing: Option<(&'a str, bool)>,
    mark: Mark,
    enabled: bool,
    small: bool,
    marked: bool,
    keep_open: bool,
}

impl<'a> Entry<'a> {
    /// An entry with `text` on it that does something, in the menu's font, taking a click.
    pub fn new(text: &'a str) -> Self {
        Entry {
            text,
            name: None,
            trailing: None,
            mark: Mark::Action,
            enabled: true,
            small: false,
            marked: false,
            keep_open: false,
        }
    }

    /// The key that does the same (as the keymap shows it) at its right end in the hint ink, read
    /// out as its keyboard shortcut. `None` (or an empty key) shows none.
    pub fn key(mut self, key: Option<&'a str>) -> Self {
        self.trailing = key.filter(|k| !k.is_empty()).map(|k| (k, true));
        self
    }

    /// A value at its right end in the hint ink, in the menu's font: what a submenu holds now.
    pub fn value(mut self, value: &'a str) -> Self {
        self.trailing = Some((value, false));
        self
    }

    /// One of a set of choices, filled while `chosen`.
    pub fn chosen(mut self, chosen: bool) -> Self {
        self.mark = Mark::Choice(chosen);
        self
    }

    /// A check box, filled while `on`.
    pub fn check(mut self, on: bool) -> Self {
        self.mark = Mark::Check(on);
        self
    }

    /// Grayed, taking no click and opening no submenu, while `enabled` is false.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// In the small font and as tall as the menu's rows are set, as a compact context menu's.
    pub fn compact(mut self) -> Self {
        self.small = true;
        self
    }

    /// The name a screen reader (and a test) reads, when it says more than the words shown.
    pub fn named(mut self, name: &'a str) -> Self {
        self.name = Some(name);
        self
    }

    /// For a submenu: a dot before its arrow while `marked`, as for one that holds the choice.
    pub fn marked(mut self, marked: bool) -> Self {
        self.marked = marked;
        self
    }

    /// For a submenu: a click inside it (into a field it holds) leaves it open; its own picks
    /// close it.
    pub fn keep_open(mut self) -> Self {
        self.keep_open = true;
        self
    }

    /// Draws the entry: the response, which the caller reads (`clicked()`, then `ui.close()`).
    pub fn show(self, ui: &mut Ui) -> Response {
        let checked = matches!(self.mark, Mark::Choice(true) | Mark::Check(true));
        let response = row(ui, self.text, checked, 0.0, self.face(ui));
        let (name, enabled) = (self.name.unwrap_or(self.text), self.enabled);
        response.widget_info(|| match self.mark {
            Mark::Action => WidgetInfo::labeled(Role::Button, enabled, name),
            Mark::Choice(on) => WidgetInfo::selected(Role::Button, enabled, on, name),
            Mark::Check(on) => WidgetInfo::selected(Role::CheckBox, enabled, on, name),
        });
        // A screen reader says the key after the name, as it does for any menu entry.
        if let Some((key, true)) = self.trailing {
            ui.ctx().accesskit_node_builder(response.id, |node| node.set_keyboard_shortcut(key));
        }
        response
    }

    /// Draws the entry as one that opens a submenu of `content` beside it, its arrow at its right
    /// end. The row's response.
    pub fn submenu(self, ui: &mut Ui, content: impl FnOnce(&mut Ui)) -> Response {
        let response = row(ui, self.text, false, ARROW, self.face(ui));
        let (name, enabled, marked) = (self.name.unwrap_or(self.text), self.enabled, self.marked);
        // A marked submenu reads as selected (a chosen source behind it), as its dot shows.
        response.widget_info(|| {
            if marked {
                WidgetInfo::selected(Role::MenuItem, enabled, true, name)
            } else {
                WidgetInfo::labeled(Role::MenuItem, enabled, name)
            }
        });
        let ink = LIST_ROW.attribute("kind", "menu").response(&response).pointer(false, false, !enabled).look().color();
        let arrow = Pos2::new(response.rect.right() - INSET, response.rect.center().y);
        let painter = ui.painter();
        // Pointing right: the submenu opens beside the row.
        widgets::chevron(painter, arrow, false, ink);
        if self.marked {
            painter.circle_filled(Pos2::new(arrow.x - 10.0, arrow.y), 2.0, ink);
        }
        if enabled {
            let mut sub = egui::containers::menu::SubMenu::new();
            if self.keep_open {
                let config = egui::containers::menu::MenuConfig::new()
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
                sub = sub.config(config);
            }
            sub.show(ui, &response, content);
        }
        response
    }

    /// How wide the entry is drawn at least: its words, its key or value and its insets. A
    /// table of entries makes every cell this wide, so no row of it is wider than the menu egui
    /// measured.
    pub fn width(&self, ui: &Ui) -> f32 {
        natural_width(ui, self.text, 0.0, self.face(ui)).0
    }

    fn face(&self, ui: &Ui) -> Face<'a> {
        // A compact row is as tall as the menu's rows are set (lower than a full row).
        let height = if self.small { ui.spacing().interact_size.y.max(1.0) } else { theme::ROW };
        Face { small: self.small, height, trailing: self.trailing, enabled: self.enabled }
    }
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

/// A note that explains the entries above it, dim, wrapped to the menu's width (`width`, or the
/// room the menu has), taking no click.
pub fn note(ui: &mut Ui, text: &str, width: Option<f32>) {
    let ink = LABEL.attribute("kind", "hint").look().color();
    // While egui measures the menu, the room it offers: a note given no width wraps at most there.
    let width = width.unwrap_or_else(|| if ui.is_sizing_pass() { ui.available_width() } else { across(ui, 0.0) });
    let wrap = (width - 2.0 * INSET).max(1.0);
    let galley = ui.painter().layout(text.to_owned(), theme::ui_font(theme::TEXT_SMALL), ink, wrap);
    let size = Vec2::new(width, galley.size().y.max(theme::ROW));
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, text));
    let at = Pos2::new(rect.left() + INSET, rect.center().y - galley.size().y / 2.0);
    widgets::paint_text(ui.painter(), at, galley, ink);
}

/// A slider in a menu ([`controls::slider`]): `name` at its left, `shown(value)` at its right.
/// [`SLIDER`] wide while egui measures the menu, so it does not grow the menu to all the room
/// around it, and across the menu after. It moves in whole `step`s from the range's start. The
/// response, `changed()` when `value` changed.
pub fn slider(
    ui: &mut Ui,
    name: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    shown: impl Fn(f32) -> String,
) -> Response {
    let width = across(ui, SLIDER);
    ui.allocate_ui(Vec2::new(width, theme::ROW), |ui| controls::stepped_slider(ui, name, value, range, step, shown))
        .inner
}

/// The ink of the chosen entry's words, on its chosen fill: for what a caller draws on a chosen
/// row besides them (a waveform's picture).
pub fn chosen_ink(response: &Response) -> egui::Color32 {
    LIST_ROW.attribute("kind", "menu").response(response).attr(Selected, true).look().color()
}

/// How wide a slider in a menu is at least.
const SLIDER: f32 = 240.0;

/// A line between groups of entries.
pub fn separator(ui: &mut Ui) {
    let (line, _) = ui.allocate_exact_size(Vec2::new(across(ui, 0.0), LINE), Sense::hover());
    let ink = SEPARATOR.attribute("kind", "menu").look().color();
    ui.painter().hline(line.x_range(), line.center().y, theme::hairline(ink));
}

/// How wide a row `width` wide at least is drawn: across the menu, which is as wide as its widest
/// row. While egui measures the menu (its sizing pass) a row asks only for its own width, or the
/// menu would grow to all the room it is offered, as a justified layout does not.
fn across(ui: &Ui, width: f32) -> f32 {
    if ui.is_sizing_pass() { width } else { width.max(ui.available_width()) }
}

/// How a row is drawn besides its words: in the small font (a compact context menu's) or the
/// menu's, how tall, the key that does the same at its right end, and whether it takes a click.
#[derive(Clone, Copy)]
struct Face<'a> {
    small: bool,
    height: f32,
    /// What its right end shows: a key (`true`, in the mono font) or a value (in the menu's).
    trailing: Option<(&'a str, bool)>,
    enabled: bool,
}

/// The room between a row's words and its key.
const KEY_GAP: f32 = 16.0;

/// A row of a menu as wide as the menu or its words (the menu grows to the widest), `extra`
/// more at its right end, drawn by [`row_face`], `[selected]` while `checked`. `face` says its
/// font and height, and the key shown at its right end in the hint ink; a row that is not
/// enabled takes no click.
fn row(ui: &mut Ui, text: &str, checked: bool, extra: f32, face: Face<'_>) -> Response {
    let font = theme::ui_font(if face.small { theme::TEXT_SMALL } else { theme::TEXT });
    let key_font = key_font(face, &font);
    let (width, key_width) = natural_width(ui, text, extra, face);
    let sense = if face.enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(across(ui, width), face.height), sense);
    let hot = response.hovered() || response.has_focus();
    let ink = row_face(ui.painter(), rect, checked, hot, face.enabled);
    let hint = crate::kit::element::hint_ink();
    let inside = Rect::from_min_max(rect.min + Vec2::new(INSET, 0.0), rect.max - Vec2::new(INSET + extra, 0.0));
    if let Some((key, _)) = face.trailing {
        let key_ink = if face.enabled { hint } else { hint.gamma_multiply(0.6) };
        widgets::text_in(ui.painter(), inside, Align2::RIGHT_CENTER, key, key_font, key_ink);
    }
    let words = Rect::from_min_max(inside.min, inside.max - Vec2::new(key_width, 0.0));
    widgets::text_in(ui.painter(), words, Align2::LEFT_CENTER, text, font, ink);
    response
}

/// The face of a row of a menu or of a drop-down's list in `rect`: `list-row[kind="menu"]`,
/// `[selected]` while `selected` (the chosen one, highlighted), `:hover` while `hot` (under the
/// pointer, or the keys on it), `:disabled` while not `enabled`, its box as the sheet paints it.
/// Returns the ink of its words.
pub(crate) fn row_face(painter: &egui::Painter, rect: Rect, selected: bool, hot: bool, enabled: bool) -> egui::Color32 {
    let hot = enabled && hot;
    let el = LIST_ROW.attribute("kind", "menu").pointer(hot, false, !enabled).attr(Selected, selected).enter();
    el.paint_with(painter, rect, crate::kit::element::Native { radius: 2.0, fill: el.background() });
    el.color()
}

/// One of a row of choices in a menu (a grid's widths and values): `text` on a
/// `button[kind="entry"]`, a toggle drawn as the menu's rows are, `[checked]` while `on`, as wide
/// as the room it is given and as tall as a compact row. Named `name`, selected while `on`; one
/// not `enabled` takes no click. The response, clicked when pressed.
pub fn choice(ui: &mut Ui, text: &str, name: &str, on: bool, enabled: bool) -> Response {
    let height = ui.spacing().interact_size.y.max(1.0);
    let font = theme::ui_font(theme::TEXT_SMALL);
    let width = widgets::text_width(ui, text, font.clone()) + 2.0 * INSET;
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(across(ui, width), height), sense);
    response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, on, name));
    let el = BUTTON.attribute("kind", "entry").response(&response).attr(Checked, on);
    let el = if enabled { el } else { el.pointer(false, false, true) }.enter();
    let fill = el.hover_fade(widgets::hover(ui, &response), crate::kit::element::Look::background);
    // Drawn as a menu's rows are: its CSS box, no artwork.
    el.paint_with(ui.painter(), rect, crate::kit::element::Native { radius: 2.0, fill });
    let ink = el.color();
    drop(el);
    let words = Rect::from_min_max(rect.min + Vec2::new(INSET, 0.0), rect.max - Vec2::new(INSET, 0.0));
    widgets::text_in(ui.painter(), words, Align2::LEFT_CENTER, text, font, ink);
    response
}

/// The font of a row's key (the mono font) or value (the row's own).
fn key_font(face: Face<'_>, font: &egui::FontId) -> egui::FontId {
    match face.trailing {
        Some((_, true)) => theme::mono_font(theme::TEXT_SMALL),
        _ => font.clone(),
    }
}

/// How wide a row of `text` is at least, `extra` more at its right end, and how much of that its
/// key or value takes.
fn natural_width(ui: &Ui, text: &str, extra: f32, face: Face<'_>) -> (f32, f32) {
    let font = theme::ui_font(if face.small { theme::TEXT_SMALL } else { theme::TEXT });
    let key_font = key_font(face, &font);
    let key_width = face.trailing.map_or(0.0, |(key, _)| widgets::text_width(ui, key, key_font) + KEY_GAP);
    (widgets::text_width(ui, text, font) + key_width + 2.0 * INSET + extra, key_width)
}

/// One line of `text` in `ink` that takes no click, as wide as it needs.
fn words(ui: &mut Ui, text: &str, ink: egui::Color32) {
    let font = theme::ui_font(theme::TEXT);
    let width = widgets::text_width(ui, text, font.clone()) + 2.0 * INSET;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(across(ui, width), theme::ROW), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, text));
    widgets::text_in(ui.painter(), rect.shrink2(Vec2::new(INSET, 0.0)), Align2::LEFT_CENTER, text, font, ink);
}

#[cfg(test)]
#[path = "menu_tests.rs"]
mod tests;
