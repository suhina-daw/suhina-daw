// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The dialog shown while an opened project's plug-ins start (ADR 0006): how many are ready,
//! which are still loading, which could not start. The project plays meanwhile (a plug-in that
//! has not started passes the sound through or is silent), so the dialog can be sent to the
//! background; it closes by itself once nothing is loading.

use egui::{Align2, Color32, Id, Key, Pos2, Rect, Sense, Ui, Vec2};
use suhina_session::PluginProgress;

use crate::kit::dialog;
use crate::kit::theme::{self, PAD};
use crate::kit::widgets;

/// Width of the dialog.
pub const WIDTH: f32 = 344.0;
/// Names of loading plug-ins listed before "and N more".
const LISTED: usize = 6;
const LINE: f32 = 16.0;
const BAR: f32 = 4.0;
const BUTTON: f32 = 150.0;

/// Draws the dialog over `area` for `project`, above everything else, and returns whether the
/// user sent it to the background (its button, or Escape).
pub fn show(ui: &Ui, area: Rect, project: &str, progress: &PluginProgress) -> bool {
    show_status(ui, area, project, Some(progress))
}

/// A retained project's audio is being prepared; no plug-ins are being loaded.
pub fn show_preparing(ui: &Ui, area: Rect, project: &str) -> bool {
    show_status(ui, area, project, None)
}

fn show_status(ui: &Ui, area: Rect, project: &str, progress: Option<&PluginProgress>) -> bool {
    let style_class = dialog::enter("loading");
    crate::kit::floating::area(ui.ctx(), Id::unique("plugin-loading"), crate::kit::floating::Kind::Own)
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            // The sheet's generic `button` and `text-input` rules reach the dialog's own widgets.
            let mut styled_ui = style_class.ui(ui);
            let ui = &mut *styled_ui;
            ui.set_min_size(area.size());
            draw(ui, &style_class, area, project, progress)
        })
        .inner
}

fn draw(
    ui: &Ui,
    dialog: &crate::kit::element::Entered,
    area: Rect,
    project: &str,
    progress: Option<&PluginProgress>,
) -> bool {
    // Dimmed behind, and the clicks there go nowhere while the dialog is up.
    let behind = ui.interact(area, Id::unique("plugin-loading-behind"), Sense::click_and_drag());
    let _swallowed = behind.clicked();
    let painter = ui.painter_at(area);
    painter.rect_filled(
        area,
        0.0,
        crate::kit::style_class::opaque_surface_or(crate::kit::element::app_fill("window")).gamma_multiply(0.6),
    );

    let lines = progress.map_or(2, |p| {
        1 + p.loading.len().min(LISTED) + usize::from(p.loading.len() > LISTED) + p.failed.len().min(LISTED)
    });
    let height = BAR + PAD + lines as f32 * LINE + PAD + dialog::BUTTON_HEIGHT;
    let outer = Rect::from_center_size(area.center(), dialog::size(dialog, area, Vec2::new(WIDTH, height), true));
    let what = if progress.is_some() { "Loading plug-ins" } else { "Preparing audio" };
    let title = format!("{what} · {project}");
    let fill = crate::kit::style_class::own_background_or(crate::kit::element::app_fill("panel"));
    let (painted, title_row) = dialog::paint(&painter, dialog, outer, fill, Some(&title));
    drop(title_row);
    let inner = painted.content;
    let bar = Rect::from_min_size(inner.min, Vec2::new(inner.width(), BAR));
    // `progress` (its background and radius) and `progress-fill` (its background) style the bar.
    if let Some(progress) = progress {
        let progress_style = crate::kit::element::PROGRESS.enter();
        let radius = progress_style.radius_or(0.0);
        painter.rect_filled(bar, radius, progress_style.background());
        let share = if progress.total == 0 { 1.0 } else { progress.ready as f32 / progress.total as f32 };
        let fill = crate::kit::element::PROGRESS_FILL.enter().background();
        painter.rect_filled(
            Rect::from_min_size(bar.min, Vec2::new(bar.width() * share.clamp(0.0, 1.0), BAR)),
            radius,
            fill,
        );
        drop(progress_style);
    }

    let mut y = bar.bottom() + PAD;
    let mut line = |label: &str, color: Color32| {
        let row = Rect::from_min_size(Pos2::new(inner.left(), y), Vec2::new(inner.width(), LINE));
        widgets::text_in(&painter, row, Align2::LEFT_CENTER, label, theme::ui_font(theme::TEXT_SMALL), color);
        name_for_readers(ui, row, label);
        y += LINE;
    };
    if let Some(progress) = progress {
        let mut summary = format!("{} of {} ready", progress.ready, progress.total);
        if progress.missing > 0 {
            summary.push_str(&format!(", {} not installed", progress.missing));
        }
        line(&summary, crate::kit::element::caption_ink());
        let hint = crate::kit::element::hint_ink();
        for name in progress.loading.iter().take(LISTED) {
            line(&format!("Loading {name}"), hint);
        }
        if progress.loading.len() > LISTED {
            line(&format!("and {} more", progress.loading.len() - LISTED), hint);
        }
        let error = crate::kit::element::error_ink();
        for name in progress.failed.iter().take(LISTED) {
            line(&format!("Could not start {name}"), error);
        }
    } else {
        line("Preparing audio for playback…", crate::kit::element::caption_ink());
        line("Please wait. Your plug-ins stay loaded.", crate::kit::element::hint_ink());
    }

    let hide = dialog::Action::new("Continue in the background", "Continue in the background").wide(BUTTON);
    dialog::actions(ui, &painter, inner, &[hide]).is_some() || ui.input(|i| i.key_pressed(Key::Escape))
}

/// Names a line of the dialog for screen readers and tests; two copies of a plug-in give two
/// lines of the same text, told apart by where they are.
fn name_for_readers(ui: &Ui, rect: Rect, label: &str) {
    let id = Id::unique(("plugin-loading-line", rect.top().to_bits(), label));
    let response = ui.interact(rect, id, Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, label));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn retained_audio_preparation_is_visible_and_can_continue_in_the_background() {
        let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(800.0, 500.0)).build_ui_state(
            |ui, hidden: &mut bool| {
                if !*hidden && show_preparing(ui, ui.max_rect(), "Song") {
                    *hidden = true;
                }
            },
            false,
        );
        harness.get_by_label("Preparing audio for playback…");
        harness.get_by_label("Please wait. Your plug-ins stay loaded.");
        harness.get_by_label("Continue in the background").click();
        harness.run();
        assert!(*harness.state());
        assert!(harness.query_by_label("Preparing audio for playback…").is_none());
    }

    #[test]
    fn it_shows_what_is_ready_and_loading_and_goes_to_the_background() {
        let progress = PluginProgress {
            total: 9,
            ready: 3,
            loading: (1..=7).map(|i| format!("Synth {i}")).collect(),
            missing: 2,
            failed: vec!["Verb".into()],
        };
        let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(800.0, 500.0)).build_ui_state(
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
