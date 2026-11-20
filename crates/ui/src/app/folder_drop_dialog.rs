// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The question asked when samples are dropped on a folder in the browser (#943): move them
//! there or copy them, with Remember my choice, which sets Settings › Browser › Samples dropped
//! on a folder so the question is not asked again.

use egui::{Id, Key, Pos2, Rect, Ui, Vec2};

use super::project_dialog::{CloseChoice, LINE, SAVE_WIDTH, panel};
use crate::kit::theme::{self, PAD};
use crate::kit::{dialog, widgets};

/// What the person answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderDropChoice {
    Move,
    Copy,
    Cancel,
}

impl CloseChoice for FolderDropChoice {
    fn close() -> Self {
        Self::Cancel
    }
}

/// Asks whether `count` samples go to `folder` moved or copied, over `area`; `remember` is the
/// Remember my choice box. Returns the answer, or `None` while it stays open.
pub fn show(ui: &Ui, area: Rect, count: usize, folder: &str, remember: &mut bool) -> Option<FolderDropChoice> {
    // A dialog: the sheet's `dialog` rules reach it, as the other dialogs.
    let style_class = dialog::enter("folder-drop");
    crate::kit::floating::area(ui.ctx(), Id::unique("folder-drop-prompt"), crate::kit::floating::Kind::Own)
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            let mut styled_ui = style_class.ui(ui);
            let ui = &mut *styled_ui;
            ui.set_min_size(area.size());
            draw(ui, &style_class, area, count, folder, remember)
        })
        .inner
}

fn draw(
    ui: &mut Ui,
    dialog: &crate::kit::element::Entered,
    area: Rect,
    count: usize,
    folder: &str,
    remember: &mut bool,
) -> Option<FolderDropChoice> {
    let mut closed: Option<FolderDropChoice> = None;
    let size = Vec2::new(SAVE_WIDTH, 2.0 * LINE + PAD + dialog::BUTTON_HEIGHT);
    let panel = panel(ui, dialog, area, size, "Move or copy", &mut closed);
    let painter = ui.painter_at(area);
    let samples = if count == 1 { "the sample".to_string() } else { format!("the {count} samples") };
    let line = Rect::from_min_size(Pos2::new(panel.left(), panel.top()), Vec2::new(panel.width(), LINE));
    let message = format!("Move or copy {samples} to {folder}?");
    let font = theme::ui_font(theme::TEXT);
    let ink = crate::kit::element::name_ink();
    let fitted = widgets::fitted(&painter, &message, font, ink, line.width());
    painter.galley(Pos2::new(line.left(), line.center().y - fitted.size().y / 2.0), fitted, ink);
    // Remember my choice: a box and its label, both one control.
    let check = Rect::from_min_size(Pos2::new(line.left(), line.bottom()), Vec2::new(line.width() * 0.6, LINE));
    let response =
        widgets::check_box(ui, &painter, check, Id::unique("folder-drop-remember"), "Remember my choice", *remember);
    if response.clicked() {
        *remember = !*remember;
    }
    let clicked = dialog::actions(
        ui,
        &painter,
        panel,
        &[
            dialog::Action::new("Cancel", "Leave the samples where they are"),
            dialog::Action::new("Copy", "Copy the samples into the folder, leaving them where they are"),
            dialog::Action::new("Move", "Move the samples into the folder"),
        ],
    );
    if clicked == Some(2) || ui.input(|i| i.key_pressed(Key::Enter)) {
        return Some(FolderDropChoice::Move);
    }
    if clicked == Some(1) {
        return Some(FolderDropChoice::Copy);
    }
    if clicked == Some(0) || ui.input(|i| i.key_pressed(Key::Escape)) {
        return Some(FolderDropChoice::Cancel);
    }
    closed
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn it_asks_move_or_copy_and_remembers_the_choice_when_ticked() {
        let mut remember = false;
        let mut answer = None;
        let mut harness =
            Harness::builder().allow_missing_glyphs().with_size(egui::vec2(800.0, 500.0)).build_ui(|ui| {
                let area = ui.max_rect();
                if let Some(choice) = show(ui, area, 2, "Drums", &mut remember) {
                    answer = Some(choice);
                }
            });
        harness.get_by_label("Remember my choice").click();
        harness.run();
        harness.get_by_label("Copy the samples into the folder, leaving them where they are").click();
        harness.run();
        drop(harness);
        assert!(remember, "the box was ticked");
        assert_eq!(answer, Some(FolderDropChoice::Copy));
    }
}
