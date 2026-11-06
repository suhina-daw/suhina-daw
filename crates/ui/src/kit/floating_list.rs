// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A keyboard-driven list floating in the middle of the window, as the project switcher
//! (Ctrl+Tab) shows it. Up and Down move the lit row and go no further while
//! it is open (#681); a click on a row chooses it.

use egui::{Context, Id, Rect, RichText, Ui, Vec2};

use crate::kit::theme;

/// One row: the text shown (it may have a second line), a short tag drawn at its right edge
/// (a plug-in's format), and the name assistive tools and tests read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub text: String,
    pub tag: Option<String>,
    pub label: String,
}

/// What the list did this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    /// The row clicked, if any.
    pub clicked: Option<usize>,
    /// Where the list is on screen, to tell a click outside it.
    pub rect: Rect,
}

/// How the list looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub width: f32,
    pub row_height: f32,
    /// How many rows show at once; the rest scroll.
    pub visible_rows: usize,
    /// Whether the list keeps the height of `visible_rows` with fewer rows, so the panel does
    /// not change size as a search narrows; otherwise it shrinks to its rows.
    pub fixed_height: bool,
}

/// Takes the presses of Down and Up out of this frame's input, so nothing behind the list moves
/// with them, and returns how many there were of each.
pub fn take_arrow_moves(ctx: &Context) -> (usize, usize) {
    ctx.input_mut(|i| {
        let mut moves = (0usize, 0usize);
        i.events.retain(|event| match event {
            egui::Event::Key { key: egui::Key::ArrowDown, pressed: true, .. } => {
                moves.0 += 1;
                false
            }
            egui::Event::Key { key: egui::Key::ArrowUp, pressed: true, .. } => {
                moves.1 += 1;
                false
            }
            _ => true,
        });
        moves
    })
}

/// The row `down` presses of Down and `up` of Up lead to from `picked`, wrapping around a list
/// of `count` rows; 0 when the list is empty.
pub fn step(picked: usize, count: usize, down: usize, up: usize) -> usize {
    if count == 0 {
        return 0;
    }
    (picked % count + down % count + count - up % count) % count
}

/// Draws the list in the middle of the window, above everything else: `heading` first (a label,
/// a text field), then the rows, `picked` lit, then `footer` (buttons that filter the rows).
/// `follow` scrolls the lit row into view (after a move).
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &Context,
    id: Id,
    look: Look,
    heading: impl FnOnce(&mut Ui),
    footer: impl FnOnce(&mut Ui),
    rows: &[Row],
    picked: Option<usize>,
    follow: bool,
) -> Shown {
    let mut clicked = None;
    let shown = egui::Area::new(id)
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(look.width);
                ui.set_max_width(look.width);
                heading(ui);
                let spacing = ui.spacing().item_spacing.y;
                let height = (look.row_height + spacing) * look.visible_rows.max(1) as f32;
                egui::ScrollArea::vertical()
                    .id_salt(id.with("rows"))
                    .max_height(height)
                    .auto_shrink([false, !look.fixed_height])
                    .show(ui, |ui| {
                        for (place, row) in rows.iter().enumerate() {
                            let lit = picked == Some(place);
                            let text = RichText::new(&row.text).color(if lit {
                                theme::text_primary()
                            } else {
                                theme::text_secondary()
                            });
                            let response = ui.add(
                                egui::Button::selectable(lit, text).min_size(Vec2::new(look.width, look.row_height)),
                            );
                            let label = row.label.clone();
                            response.widget_info(|| {
                                egui::WidgetInfo::selected(egui::Role::Button, true, lit, label.clone())
                            });
                            if let Some(tag) = &row.tag {
                                ui.painter().text(
                                    response.rect.right_center() - Vec2::new(10.0, 0.0),
                                    egui::Align2::RIGHT_CENTER,
                                    tag,
                                    egui::FontId::proportional(theme::TEXT_SMALL),
                                    theme::text_hint(),
                                );
                            }
                            if lit && follow {
                                response.scroll_to_me(None);
                            }
                            if response.clicked() {
                                clicked = Some(place);
                            }
                        }
                    });
                footer(ui);
            });
        });
    Shown { clicked, rect: shown.response.rect }
}

#[cfg(test)]
#[path = "floating_list_tests.rs"]
mod tests;
