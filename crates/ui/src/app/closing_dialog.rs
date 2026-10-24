// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the window shows while it closes: the open projects are let go of, their plug-ins
//! unloaded, and the window waits a moment for them ([`suhina_session::Projects::let_go_all`]).
//! A plug-in busy with work of its own (an ARA plug-in finishing an analysis) can take longer;
//! the window then closes anyway and the plug-in's host ends once it is done.

use crate::kit::surface::Backgrounds as _;
use egui::{Align2, Id, Pos2, Rect, Sense, Ui, Vec2};

use crate::kit::theme::{self, PAD, ROW};
use crate::kit::widgets;

/// Width of the dialog.
const WIDTH: f32 = 320.0;
const LINE: f32 = 16.0;
/// Projects listed before "and N more".
const LISTED: usize = 6;

/// Draws the dialog over `area`, above everything else, for the projects still being let go of.
pub fn show(ui: &Ui, area: Rect, waiting: &[String]) {
    let _style_class = crate::kit::style_class::enter("dialog");
    egui::Area::new(Id::new("closing")).order(egui::Order::Foreground).fixed_pos(area.min).show(ui.ctx(), |ui| {
        // The sheet's generic `.button` and `.text-input` rules reach the dialog's own widgets.
        let mut styled_ui = crate::kit::style_class::ui(ui);
        let ui = &mut *styled_ui;
        ui.set_min_size(area.size());
        draw(ui, area, waiting);
    });
}

fn draw(ui: &Ui, area: Rect, waiting: &[String]) {
    // The window has nothing left to click while it closes.
    let behind = ui.interact(area, Id::new("closing-behind"), Sense::click_and_drag());
    let _swallowed = behind.clicked();
    let painter = ui.painter_at(area);
    painter.fill_background(area, 0.0, theme::window_background());

    let listed = waiting.len().min(LISTED) + usize::from(waiting.len() > LISTED);
    let height = ROW + PAD * 2.0 + (1 + listed) as f32 * LINE;
    let rect = Rect::from_center_size(area.center(), Vec2::new(WIDTH.min(area.width() - 2.0 * PAD), height));
    painter.fill_background(rect, 0.0, theme::panel_background());
    painter.rect_stroke(rect, 0.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
    let title = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW));
    painter.fill_background(title, 0.0, theme::control_background());
    widgets::text_in(
        &painter,
        title.shrink2(Vec2::new(PAD, 0.0)),
        Align2::LEFT_CENTER,
        "Closing Suhina",
        theme::ui_font(theme::TEXT),
        theme::text_primary(),
    );

    let mut y = title.bottom() + PAD;
    let mut line = |label: &str, color| {
        let row = Rect::from_min_size(Pos2::new(rect.left() + PAD, y), Vec2::new(rect.width() - 2.0 * PAD, LINE));
        widgets::text_in(&painter, row, Align2::LEFT_CENTER, label, theme::ui_font(theme::TEXT_SMALL), color);
        let response = ui.interact(row, Id::new(("closing-line", label)), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
        y += LINE;
    };
    line("Letting the plug-ins finish", theme::text_secondary());
    for name in waiting.iter().take(LISTED) {
        line(&format!("Closing {name}"), theme::text_hint());
    }
    if waiting.len() > LISTED {
        line(&format!("and {} more", waiting.len() - LISTED), theme::text_hint());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn it_names_the_projects_still_closing() {
        let waiting: Vec<String> = (1..=8).map(|i| format!("Song {i}")).collect();
        let harness = Harness::builder().with_size(egui::vec2(800.0, 500.0)).build_ui(|ui| {
            let area = ui.max_rect();
            show(ui, area, &waiting);
        });
        harness.get_by_label("Letting the plug-ins finish");
        harness.get_by_label("Closing Song 1");
        harness.get_by_label("and 2 more");
        assert!(harness.query_by_label("Closing Song 7").is_none(), "six are listed");
    }
}
