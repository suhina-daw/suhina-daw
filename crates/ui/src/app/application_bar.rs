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
    // The window's buttons, where Suhina draws them (Windows), keep their own room at the right
    // end: they are the system's, like the title bar, and no sheet moves or hides them.
    let custom_controls = cfg!(target_os = "windows");
    let controls_width = if custom_controls { 3.0 * CONTROL_WIDTH } else { 0.0 };
    let controls =
        Rect::from_min_max(egui::pos2((whole.right() - controls_width).max(whole.left()), whole.top()), whole.max);
    let body = Rect::from_min_max(whole.min, egui::pos2(controls.left(), whole.bottom()));
    let drag = ui.interact(body, ui.scope_id().with("application-bar-drag"), Sense::click_and_drag());
    drag.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, "Suhina application bar"));
    if custom_controls && drag.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if custom_controls && drag.double_clicked() {
        let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    // The bar's items in one row (#1372) across the rest: its start, its centre and its end, each
    // a group. The settings say which group the app's name and the project's information go in.
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
    let laid = row::row(&Bar::of(style_class, body.size()), &items).unwrap_or_default();
    // Each group's items are painted in its scope, so a rule through the group reaches them and
    // a group that hides its overflow cuts them.
    for (group_index, item) in items.iter().enumerate().take(3) {
        let (Some(id), Some(rect)) = (item.id.as_deref(), laid.at(group_index, body.min)) else { continue };
        let scope = crate::kit::element::GROUP.id(id).attribute("kind", id).enter();
        scope.paint(ui.painter(), rect);
        for (index, id, text) in texts.iter().filter(|(index, ..)| items[*index].parent == Some(group_index)) {
            let Some(rect) = laid.at(*index, body.min) else { continue };
            let item = crate::kit::element::BAR_ITEM.id(id).attribute("kind", id).enter();
            let content = item.paint(ui.painter(), rect).content;
            group(ui, content, text.clone(), item.own_color_or(theme::text_primary()));
        }
    }
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
        let rect = Rect::from_min_size(
            controls.min + egui::vec2(index as f32 * CONTROL_WIDTH, 0.0),
            egui::vec2(CONTROL_WIDTH, controls.height()),
        );
        let glyph = ["−", "□", "×"].get(index).copied().unwrap_or("");
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
        let mut harness = Harness::builder().allow_missing_glyphs().with_size((640.0, 28.0)).build_ui(|ui| {
            let bar = crate::kit::element::APPLICATION_BAR.enter();
            show(ui, &bar, ViewSettings::default(), "Project", "120 BPM");
        });
        harness.run();
        for label in ["Window Minimize", "Window Maximize", "Window Close"] {
            assert_eq!(harness.query_by_label(label).is_some(), cfg!(target_os = "windows"));
        }
    }

    /// The bar's items lie in one CSS row (#1372): its padding moves them, `display: none` takes
    /// one away, and the window's buttons stay whatever the sheet says.
    #[test]
    fn the_bar_s_items_follow_the_sheet() {
        let painted = |css: &str| {
            crate::kit::native_css::set_user_sheet(Some(css.into()));
            let mut harness = Harness::builder().allow_missing_glyphs().with_size((640.0, 28.0)).build_ui(|ui| {
                let _frame = crate::kit::native_css::frame(ui);
                let bar = crate::kit::element::APPLICATION_BAR.enter();
                let settings = ViewSettings {
                    app_name_centered: false,
                    bar_project_centered: false,
                    bar_project_name: true,
                    bar_project_timing: true,
                    ..ViewSettings::default()
                };
                show(ui, &bar, settings, "Project", "120 BPM");
            });
            harness.run();
            harness.run();
            let texts: Vec<_> = harness
                .output()
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some((text.galley.text().to_owned(), text.pos.x)),
                    _ => None,
                })
                .collect();
            let close = harness.query_by_label("Window Close").is_some();
            crate::kit::native_css::set_user_sheet(None);
            (texts, close)
        };
        let x = |texts: &[(String, f32)], text: &str| texts.iter().find(|(t, _)| t.contains(text)).map(|(_, x)| *x);
        let (plain, _) = painted("");
        let (padded, _) = painted("application-bar { padding-left: 30px; }");
        assert_eq!(x(&padded, "Suhina").zip(x(&plain, "Suhina")).map(|(a, b)| a - b), Some(22.0));
        let (spaced, _) = painted("application-bar group { gap: 14px; }");
        // Each item after the first of a group: the project's name, then its timing.
        let gap = |texts: &[(String, f32)]| x(texts, "120 BPM").zip(x(texts, "Project")).map(|(a, b)| a - b);
        assert_eq!(gap(&spaced).zip(gap(&plain)).map(|(a, b)| a - b), Some(10.0));
        let (hidden, close) = painted("#project-name { display: none; } bar-item { display: none; }");
        assert!(x(&plain, "Project").is_some() && x(&hidden, "Project").is_none());
        assert_eq!(close, cfg!(target_os = "windows"), "the window's buttons are not the sheet's");
    }

    /// The colour the bar's text containing `text` is painted in, under `css`.
    fn ink_of(css: &str, text: &str) -> Option<egui::Color32> {
        crate::kit::native_css::set_user_sheet(Some(css.into()));
        let mut harness = Harness::builder().allow_missing_glyphs().with_size((640.0, 28.0)).build_ui(|ui| {
            let _frame = crate::kit::native_css::frame(ui);
            let bar = crate::kit::element::APPLICATION_BAR.enter();
            let settings = ViewSettings { app_name_centered: false, ..ViewSettings::default() };
            show(ui, &bar, settings, "Project", "120 BPM");
        });
        harness.run();
        harness.run();
        let ink = harness.output().shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(shape) if shape.galley.text().contains(text) => {
                shape.galley.job.sections.first().map(|section| section.format.color)
            }
            _ => None,
        });
        crate::kit::native_css::set_user_sheet(None);
        ink
    }

    /// The items are painted in their group's scope: a rule through the group reaches them, and an
    /// item's own `color` wins even when a rule gives the bar the same one.
    #[test]
    fn a_rule_through_a_group_reaches_its_items() {
        let red = egui::Color32::from_rgb(255, 0, 0);
        let plain = ink_of("", "Suhina").unwrap();
        assert_ne!(plain, red);
        assert_eq!(ink_of("#app-bar-start > bar-item { color: #ff0000; }", "Suhina"), Some(red));
        assert_eq!(ink_of("application-bar, #app-name { color: #ff0000; }", "Suhina"), Some(red));
    }
}
