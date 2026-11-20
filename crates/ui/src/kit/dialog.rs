// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A dialog: one `dialog` element for all of them, told apart by its `name` attribute
//! (`dialog[name="export"]`, `dialog[name="color-picker"]`), so a theme styles every dialog with
//! one rule and one dialog with one more. Its box is the sheet's: the background, border, corners,
//! shadow and blur, and its `padding`, which is the room between its edge and what it holds. Its
//! title row (`dialog-title`) runs along its top inside the border, outside the padding; its
//! buttons stand in a `dialog-actions` row along the bottom of what it holds, laid out by the
//! sheet (`justify-content`, `gap`).

use egui::{Align2, Painter, Pos2, Rect, Ui, Vec2};

use super::element::{DIALOG, DIALOG_ACTIONS, DIALOG_TITLE, Entered};
use super::native_css::row::{self, Bar, Item};
use super::{theme, widgets};

/// How tall a dialog's title row is.
pub const TITLE: f32 = theme::ROW;
/// How tall a dialog's buttons are.
pub const BUTTON_HEIGHT: f32 = 18.0;
/// How wide a dialog's button is unless its words need more.
pub const BUTTON_WIDTH: f32 = 96.0;

/// Enter the dialog called `name`: `dialog[name="…"]`.
pub fn enter(name: &str) -> Entered {
    DIALOG.attribute("name", name).enter()
}

/// How big the dialog's box is for contents `content` big: its border and padding around them,
/// and its title row over them when it has one; no bigger than `area` less a margin.
pub fn size(dialog: &Entered, area: Rect, content: Vec2, titled: bool) -> Vec2 {
    let title = if titled { TITLE } else { 0.0 };
    super::floating::dialog_size(dialog, area, content + Vec2::new(0.0, title))
}

/// What a dialog's box is made of, once it is painted.
pub struct Painted {
    /// The whole box, its border included.
    pub outer: Rect,
    /// The title row, along the top inside the border: empty when the dialog has none.
    pub title: Rect,
    /// Where what the dialog holds goes: inside the border and the padding, under the title row.
    pub content: Rect,
}

/// Paints `dialog`'s box in `outer`: the window blurred under it (unless its `backdrop-filter` is
/// `none`), its background, and its border, then its title row when it has `title`.
/// The title row's element stays entered in what is returned for a Close button on it.
pub fn paint(painter: &Painter, dialog: &Entered, outer: Rect, title: Option<&str>) -> (Painted, Option<Entered>) {
    let inside = super::floating::dialog(painter, dialog, outer);
    let Some(title) = title else {
        return (Painted { outer, title: Rect::from_min_size(inside.min, Vec2::ZERO), content: inside }, None);
    };
    // The row spans the box inside its border, whatever the padding.
    let [top, right, _, left] =
        dialog.style().filter(|style| style.authored.border_width).map_or([0.0; 4], |style| style.border_sides);
    let row = Rect::from_min_size(
        Pos2::new(outer.left() + left, outer.top() + top),
        Vec2::new(outer.width() - left - right, TITLE),
    );
    let entered = title_row(painter, row, title);
    let content = Rect::from_min_max(Pos2::new(inside.left(), inside.top() + TITLE), inside.max);
    (Painted { outer, title: row, content }, Some(entered))
}

/// A dialog's title row in `row`, `dialog-title`: its fill and its words, `label[kind="title"]`.
/// The element stays entered for what its caller puts on the row (a Close button).
pub fn title_row(painter: &Painter, row: Rect, title: &str) -> Entered {
    let entered = DIALOG_TITLE.enter();
    painter.rect_filled(row, 0.0, entered.background());
    widgets::text_in(
        painter,
        row.shrink2(Vec2::new(theme::PAD, 0.0)),
        Align2::LEFT_CENTER,
        title,
        theme::ui_font(theme::TEXT),
        super::element::title_ink(),
    );
    entered
}

/// How wide the Close button at the right of a dialog's title row is.
const CLOSE_WIDTH: f32 = 52.0;

/// The Close button at the right of a dialog's title row `bar` ([`Painted::title`]), a flat
/// button answering to `id`, named "Close `title`". Drawn while the title row's element from
/// [`paint`] is still entered, so the theme styles it as part of that row. A click closes.
pub fn close_button(ui: &Ui, painter: &Painter, bar: Rect, id: egui::Id, title: &str) -> egui::Response {
    let rect =
        Rect::from_min_size(Pos2::new(bar.right() - CLOSE_WIDTH, bar.top()), Vec2::new(CLOSE_WIDTH, bar.height()));
    let response = widgets::button_area(ui, rect, id, format!("Close {title}"));
    widgets::flat_button(ui, painter, rect, &response, "Close", (theme::TEXT_SMALL, None));
    response
}

/// One button of a dialog's [`actions`].
#[derive(Clone, Copy, Debug)]
pub struct Action<'a> {
    /// The words on it.
    pub text: &'a str,
    /// What it is named for readers and tests, and says when the pointer rests on it.
    pub label: &'a str,
    /// How wide it is: [`BUTTON_WIDTH`] unless [`Action::wide`] gave it another.
    pub width: f32,
    /// A `flexible-space` instead of a button: the room the buttons leave, which puts the ones
    /// after it at the other end of the row.
    pub space: bool,
}

impl<'a> Action<'a> {
    /// A button with `text` on it, named `label`, [`BUTTON_WIDTH`] wide.
    pub fn new(text: &'a str, label: &'a str) -> Self {
        Self { text, label, width: BUTTON_WIDTH, space: false }
    }

    /// The room the buttons leave, between those before it and those after it.
    pub fn space() -> Self {
        Self { text: "", label: "", width: 0.0, space: true }
    }

    /// The button `width` wide.
    pub fn wide(self, width: f32) -> Self {
        Self { width, ..self }
    }
}

/// The dialog's buttons in a `dialog-actions` row along the bottom of `content`, the dialog's
/// content box, in the order given: the sheet places them (`justify-content`, `gap`; the
/// user-agent sheet's put them at the right, 8 points apart). The index of the one clicked; a
/// [`Action::space`] is never clicked. Where the sheet cannot lay the row out (a box with no room,
/// a sheet that fails), the buttons stand at its right, [`FALLBACK_GAP`] apart, so a dialog never
/// loses the only buttons that answer it; a sheet that hides the row (`display: none`) is obeyed.
pub fn actions(ui: &Ui, painter: &Painter, content: Rect, actions: &[Action<'_>]) -> Option<usize> {
    let height = BUTTON_HEIGHT;
    let bar = Rect::from_min_max(Pos2::new(content.left(), content.bottom() - height), content.max);
    let element = DIALOG_ACTIONS.enter();
    let items: Vec<Item> = actions
        .iter()
        .map(|action| {
            if action.space {
                Item::new("flexible-space")
            } else {
                Item::new("button").width(action.width).height(height)
            }
        })
        .collect();
    let laid = row::row(&Bar::of(&element, bar.size()), &items);
    let fallback = if laid.is_none() { at_the_end(bar, actions) } else { Vec::new() };
    let mut clicked = None;
    for (index, action) in actions.iter().enumerate() {
        let place = match &laid {
            Some(laid) => laid.at(index, bar.min),
            None => fallback.get(index).copied().flatten(),
        };
        let Some(rect) = place.filter(|_| !action.space) else { continue };
        if widgets::button(ui, painter, rect, action.text, action.label) && clicked.is_none() {
            clicked = Some(index);
        }
    }
    clicked
}

/// How far apart [`actions`] puts the buttons where the sheet lays out none: the user-agent
/// sheet's `gap`.
const FALLBACK_GAP: f32 = 8.0;

/// Where [`actions`] puts the buttons without the sheet: in their order at the right of `bar`,
/// [`FALLBACK_GAP`] apart; nowhere for a space, or in a box that is not finite.
fn at_the_end(bar: Rect, actions: &[Action<'_>]) -> Vec<Option<Rect>> {
    if !bar.is_finite() {
        return vec![None; actions.len()];
    }
    let buttons = actions.iter().filter(|action| !action.space);
    let width = buttons.clone().map(|action| action.width.max(0.0)).sum::<f32>()
        + FALLBACK_GAP * buttons.count().saturating_sub(1) as f32;
    let mut x = bar.right() - width;
    actions
        .iter()
        .map(|action| {
            if action.space {
                return None;
            }
            let rect = Rect::from_min_size(Pos2::new(x, bar.top()), Vec2::new(action.width.max(0.0), bar.height()));
            x = rect.right() + FALLBACK_GAP;
            Some(rect)
        })
        .collect()
}

#[cfg(test)]
#[path = "dialog_tests.rs"]
mod tests;
