// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The question asked when samples are dropped on a folder in the browser (#943): move them
//! there or copy them, with Remember my choice, which sets Settings › Browser › Samples dropped
//! on a folder so the question is not asked again.

use egui::{Align2, Id, Key, Pos2, Rect, Ui, Vec2};

use super::project_dialog::{BUTTON, CloseChoice, LINE, SAVE_WIDTH, panel};
use crate::kit::theme::{self, PAD, ROW};
use crate::kit::widgets::{self, button};

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
    egui::Area::new(Id::new("folder-drop-prompt"))
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            ui.set_min_size(area.size());
            draw(ui, area, count, folder, remember)
        })
        .inner
}

fn draw(ui: &mut Ui, area: Rect, count: usize, folder: &str, remember: &mut bool) -> Option<FolderDropChoice> {
    let mut closed: Option<FolderDropChoice> = None;
    let size = Vec2::new(SAVE_WIDTH, ROW + 2.0 * LINE + BUTTON.y + 3.0 * PAD);
    let panel = panel(ui, area, size, "Move or copy", &mut closed);
    let painter = ui.painter_at(area);
    let samples = if count == 1 { "the sample".to_string() } else { format!("the {count} samples") };
    let line = Rect::from_min_size(
        Pos2::new(panel.left() + PAD, panel.top() + ROW + PAD),
        Vec2::new(panel.width() - 2.0 * PAD, LINE),
    );
    let message = format!("Move or copy {samples} to {folder}?");
    let font = theme::ui_font(theme::TEXT);
    let fitted = widgets::fitted(&painter, &message, font, theme::text_primary(), line.width());
    painter.galley(Pos2::new(line.left(), line.center().y - fitted.size().y / 2.0), fitted, theme::text_primary());
    // Remember my choice: a box and its label, both one control.
    let check = Rect::from_min_size(Pos2::new(line.left(), line.bottom()), Vec2::new(line.width() * 0.6, LINE));
    let response = widgets::button_area(ui, check, Id::new("folder-drop-remember"), "Remember my choice");
    let mark = Rect::from_center_size(Pos2::new(check.left() + 6.0, check.center().y), Vec2::splat(11.0));
    let stroke = if response.hovered() { theme::border_strong() } else { theme::border() };
    painter.rect_stroke(mark, 0.0, theme::hairline(stroke), egui::StrokeKind::Inside);
    if *remember {
        painter.rect_filled(mark.shrink(2.5), 0.0, theme::text_value());
    }
    let label = Rect::from_min_max(Pos2::new(mark.right() + 6.0, check.top()), check.max);
    let small = theme::ui_font(theme::TEXT_SMALL);
    widgets::text_in(&painter, label, Align2::LEFT_CENTER, "Remember my choice", small, theme::text_secondary());
    if response.clicked() {
        *remember = !*remember;
    }
    let moved = Rect::from_min_size(Pos2::new(panel.right() - PAD - BUTTON.x, panel.bottom() - PAD - BUTTON.y), BUTTON);
    let copied = Rect::from_min_size(Pos2::new(moved.left() - 4.0 - BUTTON.x, moved.top()), BUTTON);
    let cancel = Rect::from_min_size(Pos2::new(copied.left() - 4.0 - BUTTON.x, moved.top()), BUTTON);
    if button(ui, &painter, moved, "Move", "Move the samples into the folder")
        || ui.input(|i| i.key_pressed(Key::Enter))
    {
        return Some(FolderDropChoice::Move);
    }
    if button(ui, &painter, copied, "Copy", "Copy the samples into the folder, leaving them where they are") {
        return Some(FolderDropChoice::Copy);
    }
    if button(ui, &painter, cancel, "Cancel", "Leave the samples where they are")
        || ui.input(|i| i.key_pressed(Key::Escape))
    {
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
        let mut harness = Harness::builder().with_size(egui::vec2(800.0, 500.0)).build_ui(|ui| {
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
