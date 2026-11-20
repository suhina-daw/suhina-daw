// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The warning shown when samples moved or copied into a browser folder have a name a file there
//! already has (#943): keep both, the new one numbered (kick 2.wav), skip those samples, or stop.
//! Nothing in the folder is ever overwritten.

use egui::{Id, Key, Pos2, Rect, Ui, Vec2};

use super::project_dialog::{CloseChoice, LINE, SAVE_WIDTH, panel};
use crate::kit::theme::{self, PAD};
use crate::kit::{dialog, widgets};

/// What the person answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameClashChoice {
    KeepBoth,
    Skip,
    Cancel,
}

impl CloseChoice for NameClashChoice {
    fn close() -> Self {
        Self::Cancel
    }
}

/// Warns that `taken` of the `count` samples have a name already in `folder`, over `area`.
/// Returns the answer, or `None` while it stays open.
pub fn show(ui: &Ui, area: Rect, taken: &[String], count: usize, folder: &str) -> Option<NameClashChoice> {
    // A dialog: the sheet's `dialog` rules reach it, as the other dialogs.
    let style_class = dialog::enter("name-clash");
    crate::kit::floating::area(ui.ctx(), Id::unique("name-clash-prompt"), crate::kit::floating::Kind::Own)
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            let mut styled_ui = style_class.ui(ui);
            let ui = &mut *styled_ui;
            ui.set_min_size(area.size());
            draw(ui, &style_class, area, taken, count, folder)
        })
        .inner
}

fn draw(
    ui: &mut Ui,
    dialog: &crate::kit::element::Entered,
    area: Rect,
    taken: &[String],
    count: usize,
    folder: &str,
) -> Option<NameClashChoice> {
    let mut closed: Option<NameClashChoice> = None;
    let size = Vec2::new(SAVE_WIDTH, 2.0 * LINE + PAD + dialog::BUTTON_HEIGHT);
    let panel = panel(ui, dialog, area, size, "Name already taken", &mut closed);
    let painter = ui.painter_at(area);
    let line = Rect::from_min_size(Pos2::new(panel.left(), panel.top()), Vec2::new(panel.width(), LINE));
    let message = match taken {
        [one] => format!("{folder} already has a file named {one}"),
        _ => format!("{folder} already has files named as {} of the {count} samples", taken.len()),
    };
    let font = theme::ui_font(theme::TEXT);
    let ink = crate::kit::element::name_ink();
    let fitted = widgets::fitted(&painter, &message, font, ink, line.width());
    painter.galley(Pos2::new(line.left(), line.center().y - fitted.size().y / 2.0), fitted, ink);
    let hint = line.translate(Vec2::new(0.0, LINE));
    let small = theme::ui_font(theme::TEXT_SMALL);
    let note = "Keep both gives the new one a number; nothing is overwritten";
    let caption = crate::kit::element::caption_ink();
    let fitted = widgets::fitted(&painter, note, small, caption, hint.width());
    painter.galley(Pos2::new(hint.left(), hint.center().y - fitted.size().y / 2.0), fitted, caption);
    let clicked = dialog::actions(
        ui,
        &painter,
        panel,
        &[
            dialog::Action::new("Cancel", "Move or copy nothing"),
            dialog::Action::new("Skip", "Leave out the samples whose name is taken"),
            dialog::Action::new("Keep both", "Keep both files, the new one with a number added to its name"),
        ],
    );
    if clicked == Some(2) || ui.input(|i| i.key_pressed(Key::Enter)) {
        return Some(NameClashChoice::KeepBoth);
    }
    if clicked == Some(1) {
        return Some(NameClashChoice::Skip);
    }
    if clicked == Some(0) || ui.input(|i| i.key_pressed(Key::Escape)) {
        return Some(NameClashChoice::Cancel);
    }
    closed
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn it_warns_of_a_taken_name_and_answers_skip() {
        let mut answer = None;
        let taken = vec!["kick.wav".to_string()];
        let mut harness =
            Harness::builder().allow_missing_glyphs().with_size(egui::vec2(800.0, 500.0)).build_ui(|ui| {
                let area = ui.max_rect();
                if let Some(choice) = show(ui, area, &taken, 2, "Drums") {
                    answer = Some(choice);
                }
            });
        harness.get_by_label("Leave out the samples whose name is taken").click();
        harness.run();
        drop(harness);
        assert_eq!(answer, Some(NameClashChoice::Skip));
    }
}
