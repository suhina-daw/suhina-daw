// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The dialog shown while an opened project's plug-ins start (ADR 0006): how many are ready,
//! which are still loading, which could not start. The project plays meanwhile (a plug-in that
//! has not started passes the sound through or is silent), so the dialog can be sent to the
//! background; it closes by itself once nothing is loading.

use crate::kit::surface::Backgrounds as _;
use egui::{Align2, Color32, Id, Key, Pos2, Rect, Sense, Ui, Vec2};
use suhina_session::PluginProgress;

use crate::kit::theme::{self, PAD, ROW};
use crate::kit::widgets;

/// Width of the dialog.
pub const WIDTH: f32 = 360.0;
/// Names of loading plug-ins listed before "and N more".
const LISTED: usize = 6;
const LINE: f32 = 16.0;
const BAR: f32 = 4.0;
const BUTTON: Vec2 = Vec2::new(150.0, 18.0);

/// Draws the dialog over `area` for `project`, above everything else, and returns whether the
/// user sent it to the background (its button, or Escape).
pub fn show(ui: &Ui, area: Rect, project: &str, progress: &PluginProgress) -> bool {
    let _style_class = crate::kit::style_class::enter("dialog");
    egui::Area::new(Id::new("plugin-loading"))
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(area.size());
            draw(ui, area, project, progress)
        })
        .inner
}

fn draw(ui: &Ui, area: Rect, project: &str, progress: &PluginProgress) -> bool {
    // Dimmed behind, and the clicks there go nowhere while the dialog is up.
    let behind = ui.interact(area, Id::new("plugin-loading-behind"), Sense::click_and_drag());
    let _swallowed = behind.clicked();
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background().gamma_multiply(0.6));

    let loading = progress.loading.len().min(LISTED) + usize::from(progress.loading.len() > LISTED);
    let lines = 1 + loading + progress.failed.len().min(LISTED);
    let height = ROW + PAD * 3.0 + BAR + lines as f32 * LINE + BUTTON.y + PAD;
    let rect = Rect::from_center_size(area.center(), Vec2::new(WIDTH.min(area.width() - 2.0 * PAD), height));
    crate::kit::liquid::backdrop(&painter, rect);
    painter.fill_background(rect, 0.0, theme::panel_background());
    painter.rect_stroke(rect, 0.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);

    let title = Rect::from_min_size(rect.min, Vec2::new(rect.width(), ROW));
    painter.fill_background(title, 0.0, theme::control_background());
    let text = |rect: Rect, align: Align2, text: &str, color: Color32| {
        widgets::text_in(&painter, rect, align, text, theme::ui_font(theme::TEXT), color);
    };
    text(
        title.shrink2(Vec2::new(PAD, 0.0)),
        Align2::LEFT_CENTER,
        &format!("Loading plug-ins · {project}"),
        theme::text_primary(),
    );

    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + PAD, title.bottom() + PAD),
        Pos2::new(rect.right() - PAD, rect.bottom() - PAD),
    );
    let bar = Rect::from_min_size(inner.min, Vec2::new(inner.width(), BAR));
    painter.fill_background(bar, 0.0, theme::control_background());
    let share = if progress.total == 0 { 1.0 } else { progress.ready as f32 / progress.total as f32 };
    painter.rect_filled(
        Rect::from_min_size(bar.min, Vec2::new(bar.width() * share.clamp(0.0, 1.0), BAR)),
        0.0,
        theme::accent(),
    );

    let mut y = bar.bottom() + PAD;
    let mut line = |label: &str, color: Color32| {
        let row = Rect::from_min_size(Pos2::new(inner.left(), y), Vec2::new(inner.width(), LINE));
        widgets::text_in(&painter, row, Align2::LEFT_CENTER, label, theme::ui_font(theme::TEXT_SMALL), color);
        name_for_readers(ui, row, label);
        y += LINE;
    };
    let mut summary = format!("{} of {} ready", progress.ready, progress.total);
    if progress.missing > 0 {
        summary.push_str(&format!(", {} not installed", progress.missing));
    }
    line(&summary, theme::text_secondary());
    for name in progress.loading.iter().take(LISTED) {
        line(&format!("Loading {name}"), theme::text_hint());
    }
    if progress.loading.len() > LISTED {
        line(&format!("and {} more", progress.loading.len() - LISTED), theme::text_hint());
    }
    for name in progress.failed.iter().take(LISTED) {
        line(&format!("Could not start {name}"), theme::recording());
    }

    let button = Rect::from_min_size(Pos2::new(inner.right() - BUTTON.x, inner.bottom() - BUTTON.y), BUTTON);
    let response = widgets::button_area(ui, button, Id::new("plugin-loading-hide"), "Continue in the background");
    let hover = widgets::hover(ui, &response);
    painter.fill_background(
        button,
        0.0,
        crate::kit::motion::mix(theme::control_background(), theme::control_hover(), hover),
    );
    painter.rect_stroke(button, 0.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
    widgets::text_in(
        &painter,
        button,
        Align2::CENTER_CENTER,
        "Continue in the background",
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_primary(),
    );
    response.clicked() || ui.input(|i| i.key_pressed(Key::Escape))
}

/// Names a line of the dialog for screen readers and tests.
fn name_for_readers(ui: &Ui, rect: Rect, label: &str) {
    let response = ui.interact(rect, Id::new(("plugin-loading-line", label)), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn it_shows_what_is_ready_and_loading_and_goes_to_the_background() {
        let progress = PluginProgress {
            total: 9,
            ready: 3,
            loading: (1..=7).map(|i| format!("Synth {i}")).collect(),
            missing: 2,
            failed: vec!["Verb".into()],
        };
        let mut harness = Harness::builder().with_size(egui::vec2(800.0, 500.0)).build_ui_state(
            |ui, hidden: &mut bool| {
                let area = ui.max_rect();
                if show(ui, area, "Song", &progress) {
                    *hidden = true;
                }
            },
            false,
        );
        harness.get_by_label("3 of 9 ready, 2 not installed");
        harness.get_by_label("Loading Synth 1");
        harness.get_by_label("and 1 more");
        harness.get_by_label("Could not start Verb");
        assert!(harness.query_by_label("Loading Synth 7").is_none(), "six are listed");
        assert!(!*harness.state());
        harness.get_by_label("Continue in the background").click();
        harness.run();
        assert!(*harness.state());
    }
}
