// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Arrangement's toolbar (#991): its tools as icon buttons, in the piano roll's look, shown
//! when Settings › Arrangement › Toolbar is on. The tool keys (1 to 6) choose them while the
//! Arrangement has the focus.

use egui::{Id, Rect, Stroke, Ui, Vec2};

use super::{ArrangementTool, ArrangementViewState};
use crate::detail::piano_roll::IconButton;
use crate::kit::theme;
use crate::kit::widgets;

/// The toolbar's height, in points.
pub(super) const TOOLBAR: f32 = 24.0;
/// An icon button's side and the gap between two.
const ICON: f32 = 18.0;
const GAP: f32 = 3.0;

/// The action ids of the tool keys, in the tools' order: the keys their tooltips show.
pub const TOOL_ACTIONS: [&str; 6] = [
    "arrangement.tool1",
    "arrangement.tool2",
    "arrangement.tool3",
    "arrangement.tool4",
    "arrangement.tool5",
    "arrangement.tool6",
];

/// How wide the toolbar's grid menu is.
const GRID_MENU: f32 = 96.0;

/// Draws the toolbar in `rect`: a button per tool, the chosen one lit, a click choosing it; at
/// its right end the grid menu.
pub(super) fn toolbar(ui: &mut Ui, rect: Rect, state: &mut ArrangementViewState) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::panel_background());
    painter.hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline(theme::border()));
    let mut x = rect.left() + 6.0;
    for (index, (tool, name)) in ArrangementTool::ALL.into_iter().enumerate() {
        let area = Rect::from_min_size(egui::pos2(x, rect.center().y - ICON / 2.0), Vec2::splat(ICON));
        x = area.right() + GAP;
        let hint = match state.tool_keys.get(index).and_then(Option::as_ref) {
            Some(key) => format!("{name} ({key})"),
            None => name.to_string(),
        };
        let label = format!("Arrangement tool {name}");
        let response = widgets::button_area(ui, area, Id::new(("arrangement-tool", name)), &label).on_hover_text(&hint);
        let chosen = state.tool == tool;
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, chosen, &label));
        let fill = if chosen { theme::control_active() } else { theme::control_background() };
        painter.rect_filled(
            area,
            0.0,
            crate::kit::motion::mix(fill, theme::control_hover(), widgets::hover(ui, &response)),
        );
        let ink = if chosen { theme::text_primary() } else { theme::text_secondary() };
        match tool {
            ArrangementTool::Select => IconButton::Select.draw(&painter, area, ink, 1.0),
            ArrangementTool::Split => IconButton::Slice.draw(&painter, area, ink, 1.0),
            ArrangementTool::Erase => IconButton::Erase.draw(&painter, area, ink, 1.0),
            ArrangementTool::Draw => IconButton::Draw.draw(&painter, area, ink, 1.0),
            ArrangementTool::Mute => IconButton::Mute.draw(&painter, area, ink, 1.0),
            ArrangementTool::Range => range_icon(&painter, area, ink),
        }
        if response.clicked() {
            state.tool = tool;
        }
    }
    // The grid menu, as the piano roll's: adaptive or fixed, triplets, snap (Ctrl+1 to Ctrl+4).
    let menu = Rect::from_min_max(
        egui::pos2(rect.right() - GRID_MENU - 6.0, rect.top() + 2.0),
        egui::pos2(rect.right() - 6.0, rect.bottom() - 2.0),
    );
    if menu.left() > x + GAP {
        let grid = state.grid;
        let keys = state.grid_keys.clone();
        let shown = grid.label();
        let mono = theme::mono_font(theme::TEXT_SMALL);
        ui.scope_builder(egui::UiBuilder::new().max_rect(menu), |ui| {
            let text = egui::RichText::new(format!("{shown}   ")).font(mono).color(theme::text_value());
            let response = ui.menu_button(text, |ui| {
                if let Some(picked) = crate::detail::piano_roll::grid_entries(ui, grid, &keys, &|_| true) {
                    state.grid = picked;
                    ui.close();
                }
            });
            crate::detail::piano_roll::menu_arrow(ui, response.response.rect, theme::text_value());
            let label = format!("Arrangement grid {shown}");
            response.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
            response.response.on_hover_text(
                "The Arrangement's grid: snapping, adaptive or fixed lines, triplets (Ctrl+1 to Ctrl+4)",
            );
        });
    }
}

/// The Range tool's icon: a text cursor between two brackets, a span of time.
fn range_icon(painter: &egui::Painter, rect: Rect, color: egui::Color32) {
    let c = rect.center();
    let stroke = Stroke::new(1.1, color);
    let line = |a: (f32, f32), b: (f32, f32)| painter.line_segment([c + Vec2::from(a), c + Vec2::from(b)], stroke);
    line((0.0, -5.0), (0.0, 5.0));
    line((-1.5, -5.0), (1.5, -5.0));
    line((-1.5, 5.0), (1.5, 5.0));
    line((-6.0, -4.0), (-6.0, 4.0));
    line((-6.0, -4.0), (-4.5, -4.0));
    line((-6.0, 4.0), (-4.5, 4.0));
    line((6.0, -4.0), (6.0, 4.0));
    line((6.0, -4.0), (4.5, -4.0));
    line((6.0, 4.0), (4.5, 4.0));
}
