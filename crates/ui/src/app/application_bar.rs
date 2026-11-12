// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Application identity and optional project information in independently positioned groups.

use crate::kit::{theme, widgets};
use crate::settings::view_settings::ViewSettings;
use egui::{Align2, Rect, Sense, Ui};

/// The bar's contents in `ui`'s rect, its content box: `style_class` is the bar's element, which
/// the window entered and painted.
pub(super) fn show(
    ui: &mut Ui,
    style_class: &crate::kit::element::Entered,
    settings: ViewSettings,
    project: &str,
    timing: &str,
) {
    use crate::kit::native_css::row::{self, Bar, Item};
    let mut styled_ui = style_class.ui(ui);
    let ui = &mut *styled_ui;
    let whole = ui.max_rect();
    let custom_controls = cfg!(target_os = "windows");
    // The bar's items in one row (#1372): its start, its centre and its end, each a group. The
    // settings say which group the app's name and the project's information go in.
    let (mut start, mut centre) = (Vec::new(), Vec::new());
    if settings.app_name_centered {
        centre.push(("app-name", "Suhina"));
    } else {
        start.push(("app-name", "Suhina"));
    }
    let info = if settings.bar_project_centered { &mut centre } else { &mut start };
    if settings.bar_project_name && !project.is_empty() {
        info.push(("project-name", project));
    }
    if settings.bar_project_timing && !timing.is_empty() {
        info.push(("project-timing", timing));
    }
    let font = theme::ui_font(theme::TEXT);
    let mut items = vec![
        Item::named("group", "app-bar-start"),
        Item::named("group", "app-bar-centre"),
        Item::named("group", "app-bar-end"),
    ];
    // After the first of a group, an item's text starts with a dot that sets it apart.
    let mut texts = Vec::new();
    for (group, list) in [(0, &start), (1, &centre)] {
        for (index, (id, text)) in list.iter().enumerate() {
            let text = if index == 0 { (*text).to_owned() } else { format!("· {text}") };
            items.push(Item::named("bar-item", id).width(widgets::text_width(ui, &text, font.clone())).in_group(group));
            texts.push((items.len() - 1, *id, text));
        }
    }
    let buttons = [
        ("window-minimize", "Minimize", egui::ViewportCommand::Minimized(true)),
        (
            "window-maximize",
            "Maximize",
            egui::ViewportCommand::Maximized(!ui.input(|input| input.viewport().maximized.unwrap_or(false))),
        ),
        ("window-close", "Close", egui::ViewportCommand::Close),
    ];
    let first_button = items.len();
    if custom_controls {
        items.extend(buttons.iter().map(|(id, ..)| Item::named("bar-item", id).width(CONTROL_WIDTH).in_group(2)));
    }
    let laid = row::row(&Bar::new("application-bar", whole.size()), &items).unwrap_or_default();
    // The window's buttons are always there: a sheet that hides one leaves it at the bar's end.
    let control = |index: usize| {
        laid.at(first_button + index, whole.min).unwrap_or_else(|| {
            let left = whole.right() - (3 - index.min(3)) as f32 * CONTROL_WIDTH;
            Rect::from_min_max(egui::pos2(left, whole.top()), egui::pos2(left + CONTROL_WIDTH, whole.bottom()))
        })
    };
    let controls_left = if custom_controls { control(0).left().min(whole.right()) } else { whole.right() };
    let body = Rect::from_min_max(whole.min, egui::pos2(controls_left.max(whole.left()), whole.bottom()));
    let drag = ui.interact(body, ui.scope_id().with("application-bar-drag"), Sense::click_and_drag());
    drag.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, "Suhina application bar"));
    if custom_controls && drag.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if custom_controls && drag.double_clicked() {
        let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    for (index, id, text) in texts {
        if let Some(rect) = laid.at(index, whole.min) {
            let item = crate::kit::element::BAR_ITEM.id(id).enter();
            let content = item.paint(ui.painter(), rect).content;
            group(ui, content, text, item.color_or(theme::text_primary()));
        }
    }
    if !custom_controls {
        return;
    }
    for (index, (id, label, command)) in buttons.into_iter().enumerate() {
        let rect = control(index);
        let glyph = ["−", "□", "×"].get(index).copied().unwrap_or("");
        let _item = crate::kit::element::BAR_ITEM.id(id).enter();
        let response = ui.put(rect, egui::Button::new(glyph).frame(false)).on_hover_text(label);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Window {label}")));
        if response.clicked() {
            ui.ctx().send_viewport_cmd(command);
        }
    }
}

/// How wide each of the window's buttons is, in points.
const CONTROL_WIDTH: f32 = 32.0;

/// `text` in `rect`, on one line, cut short with an ellipsis when it does not fit.
fn group(ui: &Ui, rect: Rect, text: String, color: egui::Color32) {
    let mut job = egui::text::LayoutJob::simple(text, theme::ui_font(theme::TEXT), color, rect.width().max(0.0));
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(job);
    let position = Align2::LEFT_CENTER.align_size_within_rect(galley.size(), rect).min;
    widgets::paint_text(&ui.painter().with_clip_rect(rect), position, galley, color);
}

pub(super) fn resize_edges(ui: &Ui) {
    if !cfg!(target_os = "windows") || ui.input(|input| input.viewport().maximized.unwrap_or(false)) {
        return;
    }
    let Some(position) = ui.input(|input| input.pointer.hover_pos()) else { return };
    let rect = ui.max_rect();
    if !rect.contains(position) {
        return;
    }
    use egui::{CursorIcon as C, ResizeDirection as D};
    let left = position.x < rect.left() + 5.0;
    let right = position.x > rect.right() - 5.0;
    let top = position.y < rect.top() + 5.0;
    let bottom = position.y > rect.bottom() - 5.0;
    let edge = match (left, right, top, bottom) {
        (true, _, true, _) => Some((D::NorthWest, C::ResizeNwSe)),
        (_, true, true, _) => Some((D::NorthEast, C::ResizeNeSw)),
        (true, _, _, true) => Some((D::SouthWest, C::ResizeNeSw)),
        (_, true, _, true) => Some((D::SouthEast, C::ResizeNwSe)),
        (true, _, _, _) => Some((D::West, C::ResizeHorizontal)),
        (_, true, _, _) => Some((D::East, C::ResizeHorizontal)),
        (_, _, true, _) => Some((D::North, C::ResizeVertical)),
        (_, _, _, true) => Some((D::South, C::ResizeVertical)),
        _ => None,
    };
    if let Some((direction, cursor)) = edge {
        ui.ctx().set_cursor_icon(cursor);
        if ui.input(|input| input.pointer.primary_pressed()) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn native_decorations_do_not_duplicate_window_buttons() {
        let mut harness = Harness::builder().allow_missing_glyphs().with_size((640.0, 28.0)).build_ui(|ui| {
            let bar = crate::kit::element::APPLICATION_BAR.enter();
            show(ui, &bar, ViewSettings::default(), "Project", "120 BPM");
        });
        harness.run();
        for label in ["Window Minimize", "Window Maximize", "Window Close"] {
            assert_eq!(harness.query_by_label(label).is_some(), cfg!(target_os = "windows"));
        }
    }
}
