// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the window shows while it closes: the open projects are let go of, their plug-ins
//! unloaded, and the window waits a moment for them ([`suhina_session::Projects::let_go_all`]).
//! A plug-in busy with work of its own (an ARA plug-in finishing an analysis) can take longer;
//! the window then closes anyway and the plug-in's host ends once it is done.

use egui::{Align2, Id, Pos2, Rect, Sense, Ui, Vec2};

use crate::kit::theme;
use crate::kit::widgets;

/// Width of the dialog.
const WIDTH: f32 = 304.0;
const LINE: f32 = 16.0;
/// Projects listed before "and N more".
const LISTED: usize = 6;

/// Draws the dialog over `area`, above everything else, for the projects still being let go of.
pub fn show(ui: &Ui, area: Rect, waiting: &[String]) {
    let style_class = crate::kit::dialog::enter("closing");
    crate::kit::floating::area(ui.ctx(), Id::unique("closing"), crate::kit::floating::Kind::Own)
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            // The sheet's generic `button` and `text-input` rules reach the dialog's own widgets.
            let mut styled_ui = style_class.ui(ui);
            let ui = &mut *styled_ui;
            ui.set_min_size(area.size());
            draw(ui, &style_class, area, waiting);
        });
}

fn draw(ui: &Ui, dialog: &crate::kit::element::Entered, area: Rect, waiting: &[String]) {
    // The window has nothing left to click while it closes.
    let behind = ui.interact(area, Id::unique("closing-behind"), Sense::click_and_drag());
    let _swallowed = behind.clicked();
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, crate::kit::style_class::surface_or(crate::kit::element::app_fill("window")));

    let listed = waiting.len().min(LISTED) + usize::from(waiting.len() > LISTED);
    let height = (1 + listed) as f32 * LINE;
    let outer =
        Rect::from_center_size(area.center(), crate::kit::dialog::size(dialog, area, Vec2::new(WIDTH, height), true));
    let fill = crate::kit::style_class::surface_or(crate::kit::element::app_fill("panel"));
    let (painted, title) = crate::kit::dialog::paint(&painter, dialog, outer, fill, Some("Closing Suhina"));
    drop(title);
    let content = painted.content;

    let mut y = content.top();
    let mut line = |label: &str, color| {
        let row = Rect::from_min_size(Pos2::new(content.left(), y), Vec2::new(content.width(), LINE));
        widgets::text_in(&painter, row, Align2::LEFT_CENTER, label, theme::ui_font(theme::TEXT_SMALL), color);
        let response = ui.interact(row, Id::unique(("closing-line", row.top().to_bits(), label)), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, label));
        y += LINE;
    };
    line("Letting the plug-ins finish", crate::kit::element::caption_ink());
    let hint = crate::kit::element::hint_ink();
    for name in waiting.iter().take(LISTED) {
        line(&format!("Closing {name}"), hint);
    }
    if waiting.len() > LISTED {
        line(&format!("and {} more", waiting.len() - LISTED), hint);
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
        let harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(800.0, 500.0)).build_ui(|ui| {
            let area = ui.max_rect();
            show(ui, area, &waiting);
        });
        harness.get_by_label("Letting the plug-ins finish");
        harness.get_by_label("Closing Song 1");
        harness.get_by_label("and 2 more");
        assert!(harness.query_by_label("Closing Song 7").is_none(), "six are listed");
    }
}
