// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A divider: the strip between two parts that a drag moves, resizing one of them. The window's
//! splitters (`#browser-splitter`, `#detail-splitter`) and a mixer strip's `#mixer-divider` take
//! their input, their drag and their style here; each says what a double-click or a size past its
//! bounds means for it (#1370).

use super::element::{DIVIDER, Entered};
use egui::{CursorIcon, Id, Rect, Response, Sense, Ui};

/// A divider to show.
pub struct Divider<'a> {
    /// Its element's id, as a theme selects it (`#detail-splitter`).
    pub name: &'a str,
    /// The egui id of its input.
    pub key: Id,
    /// The strip it takes, drawn and taking the pointer there.
    pub strip: Rect,
    /// Whether it is dragged left and right (a strip standing up); otherwise up and down.
    pub across: bool,
    /// What screen readers and tests call it.
    pub label: &'a str,
    /// The size it resizes, as it is.
    pub size: f32,
    /// Whether the size grows as the pointer moves right or down; otherwise as it moves left or up.
    pub grows_forward: bool,
    /// The least and the most size a drag gives.
    pub bounds: (f32, f32),
}

/// What the divider did this frame.
pub struct Dragged {
    pub response: Response,
    /// The size while it is dragged, within its bounds: the size it had when the drag began and
    /// the pointer's way since, so the size never falls behind the pointer (adding each frame's
    /// movement to a size rounded to whole points would).
    pub size: Option<f32>,
    /// Its style, `divider` with its id and `:hover` and `:active`, for the caller to paint with.
    pub style: Entered,
}

/// Shows `divider`: its input, its cursor and its style.
pub fn show(ui: &Ui, divider: Divider<'_>) -> Dragged {
    let Divider { name, key, strip, across, label, size, grows_forward, bounds } = divider;
    let cursor = if across { CursorIcon::ResizeHorizontal } else { CursorIcon::ResizeVertical };
    let response = ui.interact(strip, key, Sense::click_and_drag()).on_hover_cursor(cursor);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, label));
    let start = key.with("drag-start");
    let along = |pointer: egui::Pos2| if across { pointer.x } else { pointer.y };
    // From where the button went down, not where the pointer is once the drag is told apart from
    // a click.
    if response.drag_started()
        && let Some(pointer) = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos())
    {
        ui.ctx().data_mut(|d| d.insert_temp(start, (size, along(pointer))));
    }
    let size = (response.dragged_by(egui::PointerButton::Primary))
        .then(|| ui.ctx().data(|d| d.get_temp::<(f32, f32)>(start)).zip(response.interact_pointer_pos()))
        .flatten()
        .map(|((from, at), pointer)| {
            let moved = along(pointer) - at;
            let (low, high) = (bounds.0.min(bounds.1), bounds.0.max(bounds.1));
            (if grows_forward { from + moved } else { from - moved }).clamp(low, high)
        })
        .filter(|size| size.is_finite());
    let style = DIVIDER.id(name).response(&response).enter();
    Dragged { response, size, style }
}

/// Makes the drag of the divider `key` go on from `size` where the pointer is now: a divider
/// that changed what it resizes in the middle of a drag (a closed area that opened whole).
pub fn restart(ui: &Ui, key: Id, size: f32, across: bool) {
    if let Some(pointer) = ui.input(|i| i.pointer.latest_pos()) {
        let at = if across { pointer.x } else { pointer.y };
        ui.ctx().data_mut(|d| d.insert_temp(key.with("drag-start"), (size, at)));
    }
}

#[cfg(test)]
#[path = "divider_tests.rs"]
mod tests;
