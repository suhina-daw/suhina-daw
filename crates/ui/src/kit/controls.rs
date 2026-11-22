// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Controls laid out in a `Ui`'s flow, for a tool or a panel that is a column of rows (the track
//! color window). Each is drawn by the kit from the elements the rest of the app uses, so a theme
//! that styles those styles these too, with nothing of their own to learn: a push button is a
//! `button`, a tick box a `button[kind="check"]` with a `label[kind="check"]`, a row of choices
//! `button[kind="option"]` parts, an expandable section's row a `button[kind="entry"]`, a slider a
//! `slider` (its rail, fill and handle), a heading a `label[kind="caption"]`, a note a
//! `label[kind="hint"]` and a line between groups a `separator[kind="row"]`. A view with a fixed
//! geometry paints the same faces from [`super::widgets`] into rectangles of its own.

use std::ops::RangeInclusive;

use egui::{Align2, Color32, FontId, Key, Modifiers, Pos2, Rect, Response, Role, Sense, Ui, Vec2, WidgetInfo};

use crate::kit::element::{BUTTON, LABEL, row_line};
use crate::kit::{theme, widgets};

/// How tall a control's row is.
pub const HEIGHT: f32 = theme::ROW;
/// How tall a line of words is: a heading, a note.
pub const LINE: f32 = 16.0;
/// How far a button's words sit from its edges.
const INSET: f32 = 8.0;
/// The room a slider gives its name on the left and its value on the right.
const SLIDER_NAME: f32 = 44.0;
const SLIDER_VALUE: f32 = 48.0;

fn font() -> FontId {
    theme::ui_font(theme::TEXT_SMALL)
}

/// Words that head a group of controls and take no click: `label[kind="caption"]`'s `color`.
pub fn heading(ui: &mut Ui, text: &str) -> Response {
    let ink = LABEL.attribute("kind", "caption").look().color();
    line(ui, text, ink)
}

/// A dim note that takes no click: `label[kind="hint"]`'s `color`.
pub fn note(ui: &mut Ui, text: &str) -> Response {
    let ink = LABEL.attribute("kind", "hint").look().color();
    words(ui, text, ink)
}

/// Words that take no click, in `label`'s `color`, as wide as they need.
pub fn text(ui: &mut Ui, text: &str) -> Response {
    words(ui, text, LABEL.look().color())
}

/// Words that take no click, in `ink` (an element's: `label[kind="error"]`'s for an error), as wide
/// as they need. The response, for a tooltip.
pub fn words(ui: &mut Ui, text: &str, ink: Color32) -> Response {
    let width = widgets::text_width(ui, text, font());
    // Beside controls, as tall as they are, its words centered on theirs; on a line of its own, a line.
    let height = if ui.layout().is_horizontal() { HEIGHT } else { LINE };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, text));
    widgets::text_in(ui.painter(), rect, Align2::LEFT_CENTER, text, font(), ink);
    response
}

/// One line of words across the whole row, in `ink`.
fn line(ui: &mut Ui, text: &str, ink: Color32) -> Response {
    let width = widgets::text_width(ui, text, font()).max(ui.available_width());
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, LINE), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, text));
    widgets::text_in(ui.painter(), rect, Align2::LEFT_CENTER, text, font(), ink);
    response
}

/// A line between groups of controls, across the row: `separator[kind="row"]`'s `color`.
pub fn separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 7.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, theme::hairline(row_line()));
}

/// A push button with `text` on it, named `text` for readers and tests, as wide as its words.
pub fn button(ui: &mut Ui, text: &str) -> Response {
    let width = widgets::text_width(ui, text, font()) + 2.0 * INSET;
    let (_, rect) = ui.allocate_space(Vec2::new(width, HEIGHT));
    let id = ui.scope_id().with(("kit-button", text));
    widgets::button_with_id(ui, ui.painter(), rect, id, text, text, false)
}

/// A tick box with `text` beside it ([`widgets::check_box`]). The response; a click asks for the
/// other value.
pub fn check_box(ui: &mut Ui, text: &str, on: bool) -> Response {
    let width = 17.0 + widgets::text_width(ui, text, font()) + 4.0;
    let (_, rect) = ui.allocate_space(Vec2::new(width, HEIGHT));
    let id = ui.scope_id().with(("kit-check", text));
    widgets::check_box(ui, ui.painter(), rect, id, text, on)
}

/// A row of choices, one `button[kind="option"]` part for each of `options` (its words and its
/// value), `[checked]` on `current`'s, each named "`name`: words". The value clicked, if it is not
/// `current`.
pub fn options<T: Copy + PartialEq>(ui: &mut Ui, name: &str, options: &[(&str, T)], current: T) -> Option<T> {
    let widths: Vec<f32> = options
        .iter()
        .map(|(text, _)| (widgets::text_width(ui, text, font()) + 2.0 * INSET).max(40.0).ceil())
        .collect();
    let width = widths.iter().sum::<f32>() + widths.len().saturating_sub(1) as f32;
    let (_, rect) = ui.allocate_space(Vec2::new(width, HEIGHT));
    let scope = ui.scope_id();
    let id = |index: usize| scope.with(("kit-options", name, index));
    widgets::option_parts(ui, ui.painter(), rect, &widths, id, name, options, current)
}

/// The row that opens and closes a section, across the row: an arrow (right while closed, down
/// while open) and `text`, a `button[kind="entry"]` with the pointer's fill under the pointer and
/// while it has the keys. Named `text`, selected while open; a click asks to open or close it.
pub fn disclosure(ui: &mut Ui, text: &str, open: bool) -> Response {
    let (_, rect) = ui.allocate_space(Vec2::new(ui.available_width(), HEIGHT));
    let response = ui.interact(rect, ui.scope_id().with(("kit-disclosure", text)), Sense::click());
    response.widget_info(|| WidgetInfo::selected(Role::Button, ui.is_enabled(), open, text));
    let entry = BUTTON.attribute("kind", "entry");
    if response.hovered() || response.has_focus() {
        let fill = entry.clone().pointer(true, false, false).look().background();
        ui.painter().rect_filled(rect, 2.0, fill);
    }
    let ink = entry.response(&response).look().color();
    widgets::chevron(ui.painter(), Pos2::new(rect.left() + 6.0, rect.center().y), open, ink);
    let words = Rect::from_min_max(Pos2::new(rect.left() + 16.0, rect.top()), rect.max);
    widgets::text_in(ui.painter(), words, Align2::LEFT_CENTER, text, font(), ink);
    response
}

/// A small square button with an arrow pointing left (`left`) or right, a `button[kind="entry"]`
/// with the pointer's fill under the pointer and while it has the keys, as tall as a line of
/// words. Named `name` for readers and tests; the response, clicked when pressed.
pub fn arrow_button(ui: &mut Ui, name: &str, left: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(LINE), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), name));
    let entry = BUTTON.attribute("kind", "entry");
    if response.hovered() || response.has_focus() {
        let fill = entry.clone().pointer(true, false, false).look().background();
        ui.painter().rect_filled(rect, 2.0, fill);
    }
    let ink = entry.response(&response).look().color();
    let c = rect.center();
    let x = if left { 1.5 } else { -1.5 };
    let points = vec![c + Vec2::new(x, -3.0), c + Vec2::new(-x, 0.0), c + Vec2::new(x, 3.0)];
    ui.painter().add(egui::Shape::line(points, egui::Stroke::new(1.1, ink)));
    response
}

/// A horizontal slider across the row for `value` in `range`: `name` on the left in
/// `label[kind="caption"]`'s `color`, the [`widgets::slider_face`] and `shown(value)` on the right
/// in `label`'s. A click puts the handle there and takes the keys, a drag moves it, and while it
/// has the keys the left and right arrows move it by `step`. The response is `changed()` when
/// `value` changed, `dragged()` while the pointer moves it; it is named `name` with its value.
pub fn slider(
    ui: &mut Ui,
    name: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    step: f32,
    shown: impl Fn(f32) -> String,
) -> Response {
    let (response, track, share) = slide(ui, name, value, range, step, shown, None);
    widgets::slider_face(ui, ui.painter(), track, &response, share, false);
    response
}

/// A slider as [`slider`] whose rail is a band of color: `stops` evenly spaced from left to
/// right, under a [`widgets::band_face`] handle filled with `fill`.
#[allow(clippy::too_many_arguments)]
pub fn band(
    ui: &mut Ui,
    name: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    step: f32,
    shown: impl Fn(f32) -> String,
    stops: &[Color32],
    fill: Color32,
) -> Response {
    let (response, track, share) = slide(ui, name, value, range, step, shown, None);
    widgets::band_face(ui, ui.painter(), track, &response, share, stops, fill);
    response
}

/// A band as [`band`] cut into one equal block of color for each of `colors`, with lines
/// between them: `index` picks a block, a click or a drag takes the block under the pointer (the
/// handle jumps from block to block, at the middle of its block), and the arrows move one block.
/// The response is `changed()` when `index` changed.
pub fn stepped_band(
    ui: &mut Ui,
    name: &str,
    index: &mut usize,
    shown: impl Fn(usize) -> String,
    colors: &[Color32],
    fill: Color32,
) -> Response {
    let count = colors.len().max(1);
    let mut value = (*index).min(count - 1) as f32;
    let (mut response, track, share) =
        slide(ui, name, &mut value, 0.0..=(count - 1) as f32, 1.0, |v| shown(v as usize), Some(count));
    let next = value as usize;
    if next != *index {
        *index = next;
        response.mark_changed();
    }
    widgets::stepped_band_face(ui, ui.painter(), track, &response, share, colors, fill);
    response
}

/// The row, the pointer and the keys of [`slider`], [`band`] and [`stepped_band`], with the
/// words drawn: the response, the track the face is drawn in and the handle's share of it. With
/// `cells`, the track is that many equal blocks for the whole numbers of `range` (from 0): the
/// pointer takes the block it is over, and the handle's share is the middle of its block.
fn slide(
    ui: &mut Ui,
    name: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    step: f32,
    shown: impl Fn(f32) -> String,
    cells: Option<usize>,
) -> (Response, Rect, f32) {
    let (low, high) = (*range.start(), *range.end());
    let (_, row) = ui.allocate_space(Vec2::new(ui.available_width(), HEIGHT));
    let names = Rect::from_min_size(row.min, Vec2::new(SLIDER_NAME, row.height()));
    let values = Rect::from_min_max(Pos2::new(row.right() - SLIDER_VALUE, row.top()), row.max);
    // The rail sits centered in the row: the same margin on both sides, the wider of the two.
    let margin = SLIDER_NAME.max(SLIDER_VALUE + 6.0);
    let rail =
        Rect::from_min_max(Pos2::new(row.left() + margin, row.top()), Pos2::new(row.right() - margin, row.bottom()));
    let mut response = ui
        .interact(rail, ui.scope_id().with(("kit-slider", name)), Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    let track = rail.shrink2(Vec2::new(4.0, 0.0));
    let span = high - low;
    let mut next = *value;
    if (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
        && track.width() > 0.0
    {
        let at = ((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0);
        next = match cells {
            Some(cells) => (at * cells as f32).floor(),
            None => low + at * span,
        };
        response.request_focus();
    }
    if response.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(response.id, egui::EventFilter { horizontal_arrows: true, ..Default::default() });
        });
        let (left, right) = ui.input_mut(|i| {
            (
                i.count_and_consume_key(Modifiers::NONE, Key::ArrowLeft),
                i.count_and_consume_key(Modifiers::NONE, Key::ArrowRight),
            )
        });
        // Every press of a frame counts (a slow frame holds several).
        next += (right as f32 - left as f32) * step;
    }
    let next = if low <= high { next.clamp(low, high) } else { *value };
    if next != *value && next.is_finite() {
        *value = next;
        response.mark_changed();
    }
    let share = match cells {
        Some(cells) => (*value + 0.5) / cells.max(1) as f32,
        None if span > 0.0 => (*value - low) / span,
        None => 0.0,
    };
    response.widget_info(|| WidgetInfo::slider(ui.is_enabled(), f64::from(*value), name));
    let caption = LABEL.attribute("kind", "caption").look().color();
    widgets::text_in(ui.painter(), names, Align2::LEFT_CENTER, name, font(), caption);
    widgets::text_in(ui.painter(), values, Align2::RIGHT_CENTER, &shown(*value), font(), LABEL.look().color());
    (response, track, share)
}

#[cfg(test)]
#[path = "controls_tests.rs"]
mod tests;
