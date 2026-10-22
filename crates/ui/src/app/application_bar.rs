// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Application identity and optional project information in independently positioned groups.

use crate::kit::surface::Backgrounds as _;
use crate::kit::{theme, widgets};
use crate::settings::view_settings::ViewSettings;
use egui::{Align2, Rect, Sense, Ui, Vec2};

pub(super) fn show(ui: &mut Ui, settings: ViewSettings, project: &str, timing: &str) {
    let _style_class = crate::kit::style_class::enter("application-bar");
    ui.painter().fill_background(ui.max_rect(), 0.0, theme::panel_background());
    crate::kit::native_css::background(ui.painter(), ui.max_rect());
    let mut styled_ui = crate::kit::style_class::ui(ui);
    let ui = &mut *styled_ui;
    let row = ui.available_rect_before_wrap();
    let custom_controls = cfg!(target_os = "windows");
    let controls_width = if custom_controls { 96.0 } else { 0.0 };
    let controls = Rect::from_min_max(egui::pos2((row.right() - controls_width).max(row.left()), row.top()), row.max);
    let body = Rect::from_min_max(row.min, egui::pos2(controls.left(), row.bottom()));
    let drag = ui.interact(body, ui.id().with("application-bar-drag"), Sense::click_and_drag());
    drag.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Suhina application bar"));
    if custom_controls && drag.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if custom_controls && drag.double_clicked() {
        let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    let mut left = Vec::new();
    let mut center = Vec::new();
    if settings.app_name_centered {
        center.push("Suhina");
    } else {
        left.push("Suhina");
    }
    let info = if settings.bar_project_centered { &mut center } else { &mut left };
    if settings.bar_project_name && !project.is_empty() {
        info.push(project);
    }
    if settings.bar_project_timing && !timing.is_empty() {
        info.push(timing);
    }
    // Non-overlapping cells keep long project names away from the window buttons.
    let left_rect =
        Rect::from_min_max(body.min + Vec2::new(8.0, 0.0), egui::pos2(row.left() + row.width() * 0.25, body.bottom()));
    let center_rect = Rect::from_min_max(
        egui::pos2(left_rect.right(), body.top()),
        egui::pos2((row.left() + row.width() * 0.75).min(body.right()), body.bottom()),
    );
    group(ui, left_rect, Align2::LEFT_CENTER, left.join(" · "));
    group(ui, center_rect, Align2::CENTER_CENTER, center.join(" · "));
    if !custom_controls {
        return;
    }
    for (index, (label, command)) in [
        ("Minimize", egui::ViewportCommand::Minimized(true)),
        ("Maximize", egui::ViewportCommand::Maximized(!ui.input(|input| input.viewport().maximized.unwrap_or(false)))),
        ("Close", egui::ViewportCommand::Close),
    ]
    .into_iter()
    .enumerate()
    {
        let rect =
            Rect::from_min_size(controls.min + Vec2::new(index as f32 * 32.0, 0.0), Vec2::new(32.0, row.height()));
        let glyph = ["−", "□", "×"].get(index).copied().unwrap_or("");
        let response = ui.put(rect, egui::Button::new(glyph).frame(false)).on_hover_text(label);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Window {label}")));
        if response.clicked() {
            ui.ctx().send_viewport_cmd(command);
        }
    }
}

fn group(ui: &Ui, rect: Rect, align: Align2, text: String) {
    let mut job =
        egui::text::LayoutJob::simple(text, theme::ui_font(theme::TEXT), theme::text_primary(), rect.width().max(0.0));
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(job);
    let position = align.align_size_within_rect(galley.size(), rect).min;
    widgets::paint_text(&ui.painter().with_clip_rect(rect), position, galley, theme::text_primary());
}

/// The Windows custom title bar keeps all eight native resize directions.
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
        let mut harness = Harness::builder().with_size((640.0, 28.0)).build_ui(|ui| {
            show(ui, ViewSettings::default(), "Project", "120 BPM");
        });
        harness.run();
        for label in ["Window Minimize", "Window Maximize", "Window Close"] {
            assert_eq!(harness.query_by_label(label).is_some(), cfg!(target_os = "windows"));
        }
    }
}
