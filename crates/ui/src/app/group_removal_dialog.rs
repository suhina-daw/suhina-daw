// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Conditional consent for dissolving a group that owns processing (#1569).

use egui::{Align2, Id, Key, Pos2, Rect, Ui, Vec2};

use super::project_dialog::{CloseChoice, LINE, panel};
use crate::kit::theme::{self, PAD};
use crate::kit::{dialog, widgets};

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
    let style_class = dialog::enter("group-removal");
    crate::kit::floating::area(ui.ctx(), Id::unique("group-removal-prompt"), crate::kit::floating::Kind::Own)
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            let mut styled = style_class.ui(ui);
            let ui = &mut *styled;
            ui.set_min_size(area.size());
            let mut closed = None;
            let content = panel(
                ui,
                &style_class,
                area,
                Vec2::new(464.0, 8.0 * LINE + PAD + dialog::BUTTON_HEIGHT),
                "Remove group processing?",
                &mut closed,
            );
            let painter = ui.painter_at(area);
            let line = Rect::from_min_size(content.min, Vec2::new(content.width(), LINE));
            let names_area = Rect::from_min_size(line.min, Vec2::new(line.width(), 4.0 * LINE));
            ui.scope_builder(egui::UiBuilder::new().max_rect(names_area), |ui| {
                ui.set_clip_rect(names_area.intersect(area));
                egui::ScrollArea::vertical()
                    .id_salt("group-removal-names")
                    .max_height(names_area.height())
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Each name is the kit's text in the dialog's ink, wrapped to the list's
                        // width so a long one is read whole, and styled by the theme as the lines
                        // below.
                        let ink = crate::kit::element::name_ink();
                        for name in names {
                            let width = ui.available_width();
                            let galley =
                                ui.painter().layout(name.clone(), theme::ui_font(theme::TEXT_SMALL), ink, width);
                            let height = galley.size().y.max(LINE);
                            let (rect, response) =
                                ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
                            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, name));
                            let at = Pos2::new(rect.left(), rect.center().y - galley.size().y / 2.0);
                            widgets::paint_text(ui.painter(), at, galley, ink);
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
            let response = widgets::check_box(
                ui,
                &painter,
                check,
                Id::unique("group-removal-remember"),
                "Remember my choice",
                *remember,
            );
            if response.clicked() {
                *remember = !*remember;
            }
            let clicked = dialog::actions(
                ui,
                &painter,
                content,
                &[
                    dialog::Action::new("Cancel", "Keep the project unchanged"),
                    dialog::Action::new("Remove processing", "Remove the groups and their processing").wide(132.0),
                ],
            );
            if clicked == Some(1) {
                return Some(Choice::Remove);
            }
            if clicked == Some(0) || ui.input(|i| i.key_pressed(Key::Escape)) {
                return Some(Choice::Cancel);
            }
            closed
        })
        .inner
}
