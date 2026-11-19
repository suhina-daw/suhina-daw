// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Conditional consent for dissolving a group that owns processing (#1569).

use egui::{Align2, Id, Key, Pos2, Rect, Ui, Vec2};

use super::project_dialog::{BUTTON, CloseChoice, LINE, panel};
use crate::kit::theme::{self, PAD, ROW};
use crate::kit::widgets::{self, button};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Choice {
    Remove,
    Cancel,
}

impl CloseChoice for Choice {
    fn close() -> Self {
        Self::Cancel
    }
}

pub(super) fn show(ui: &Ui, area: Rect, names: &[String], remember: &mut bool) -> Option<Choice> {
    let dialog = crate::kit::element::DIALOG.enter();
    crate::kit::floating::area(ui.ctx(), Id::unique("group-removal-prompt"), crate::kit::floating::Kind::Own)
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            let mut styled = dialog.ui(ui);
            let ui = &mut *styled;
            ui.set_min_size(area.size());
            let mut closed = None;
            let box_rect = panel(
                ui,
                &dialog,
                area,
                Vec2::new(480.0, ROW + 8.0 * LINE + BUTTON.y + 3.0 * PAD),
                "Remove group processing?",
                &mut closed,
            );
            let painter = ui.painter_at(area);
            let line = Rect::from_min_size(
                Pos2::new(box_rect.left() + PAD, box_rect.top() + ROW + PAD),
                Vec2::new(box_rect.width() - 2.0 * PAD, LINE),
            );
            let names_area = Rect::from_min_size(line.min, Vec2::new(line.width(), 4.0 * LINE));
            ui.scope_builder(egui::UiBuilder::new().max_rect(names_area), |ui| {
                ui.set_clip_rect(names_area.intersect(area));
                egui::ScrollArea::vertical()
                    .id_salt("group-removal-names")
                    .max_height(names_area.height())
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for name in names {
                            ui.add(
                                egui::Label::new(egui::RichText::new(name).font(theme::ui_font(theme::TEXT_SMALL)))
                                    .wrap(),
                            );
                        }
                    });
            });
            let line = line.translate(Vec2::new(0.0, names_area.height()));
            for (index, message) in [
                "Their effects, mixer settings and routing will be removed.",
                "Clips or takes on the groups themselves will also be removed.",
                "The whole edit can be undone in one step.",
            ]
            .iter()
            .enumerate()
            {
                let at = line.translate(Vec2::new(0.0, index as f32 * LINE));
                widgets::text_in(
                    &painter,
                    at,
                    Align2::LEFT_CENTER,
                    message,
                    theme::ui_font(theme::TEXT_SMALL),
                    crate::kit::element::name_ink(),
                );
            }
            let check = line.translate(Vec2::new(0.0, 3.0 * LINE));
            let response = widgets::button_area(ui, check, Id::unique("group-removal-remember"), "Remember my choice");
            let mark = Rect::from_center_size(Pos2::new(check.left() + 6.0, check.center().y), Vec2::splat(11.0));
            let el = crate::kit::element::BUTTON
                .attribute("kind", "check")
                .response(&response)
                .attr(crate::kit::element::Checked, *remember)
                .enter();
            painter.rect_stroke(mark, 0.0, theme::hairline(el.border()), egui::StrokeKind::Inside);
            if *remember {
                painter.rect_filled(mark.shrink(2.5), 0.0, el.background());
            }
            drop(el);
            widgets::text_in(
                &painter,
                Rect::from_min_max(Pos2::new(mark.right() + 6.0, check.top()), check.max),
                Align2::LEFT_CENTER,
                "Remember my choice",
                theme::ui_font(theme::TEXT_SMALL),
                crate::kit::element::caption_ink(),
            );
            if response.clicked() {
                *remember = !*remember;
            }
            let remove = Rect::from_min_size(
                Pos2::new(box_rect.right() - PAD - 132.0, box_rect.bottom() - PAD - BUTTON.y),
                Vec2::new(132.0, BUTTON.y),
            );
            let cancel = Rect::from_min_size(Pos2::new(remove.left() - PAD - BUTTON.x, remove.top()), BUTTON);
            if button(ui, &painter, remove, "Remove processing", "Remove the groups and their processing") {
                return Some(Choice::Remove);
            }
            if button(ui, &painter, cancel, "Cancel", "Keep the project unchanged")
                || ui.input(|i| i.key_pressed(Key::Escape))
            {
                return Some(Choice::Cancel);
            }
            closed
        })
        .inner
}
