// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Track colours: the palette every colour choice offers, defined once (the track's menu, the
//! picker under a track's colour area, a colour tag's menu), and the colour area itself, which a
//! click opens the palette from (`docs/design/components.md` › Colour palette). Picking a colour
//! is the `track.setColor` command, one undo step.

use egui::{Color32, Id, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{Rgb, TrackId};

use crate::interaction::grid::to_rgb;
use crate::kit::motion;
use crate::kit::theme;
use crate::kit::widgets::{self, Action};

/// A swatch's side, and the space between swatches.
pub const SWATCH: f32 = 14.0;
const SWATCH_GAP: f32 = 3.0;
/// How much of its colour a muted track's colour area keeps over `control.background`.
const MUTED: f32 = 0.55;

/// The palette: the theme's eight track colours (`track.1` to `track.8`; the first four are the
/// colours new tracks take in turn).
pub fn swatches() -> [Rgb; 8] {
    [
        theme::track_color_1(),
        theme::track_color_2(),
        theme::track_color_3(),
        theme::track_color_4(),
        theme::track_color_5(),
        theme::track_color_6(),
        theme::track_color_7(),
        theme::track_color_8(),
    ]
    .map(to_rgb)
}

/// `#rrggbb`, as `track.setColor` takes it.
pub fn hex(colour: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", colour.0, colour.1, colour.2)
}

pub fn color32(colour: Rgb) -> Color32 {
    Color32::from_rgb(colour.0, colour.1, colour.2)
}

/// What a track's colour area is filled with: the colour, dimmed while the track is off.
pub fn fill(colour: Rgb, muted: bool) -> Color32 {
    let colour = color32(colour);
    if muted { motion::mix(theme::control_background(), colour, MUTED) } else { colour }
}

/// Sets `track`'s colour.
pub fn set_colour(track: TrackId, colour: Rgb) -> Action {
    Action::new("track.setColor", json!({ "track": track.0, "color": hex(colour) }))
}

/// The palette as a row of swatches, `current` outlined, and after it "+" for any colour: a
/// colour field and hue strip as the theme editor's, the colour taken when the button is let
/// go. Gives the colour picked. Each swatch is a button named "Colour #rrggbb".
pub fn swatch_row(ui: &mut Ui, current: Option<Rgb>) -> Option<Rgb> {
    let mut picked = None;
    let any_id = ui.id().with("any-colour");
    let mut any_open = ui.data(|d| d.get_temp::<bool>(any_id)).unwrap_or(false);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(SWATCH_GAP);
        for colour in swatches() {
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(SWATCH), Sense::click());
            let name = format!("Colour {}", hex(colour));
            let chosen = current == Some(colour);
            response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, chosen, &name));
            ui.painter().rect_filled(rect, 0.0, color32(colour));
            if chosen || response.hovered() {
                let width = if chosen { 1.5 } else { 1.0 };
                ui.painter().rect_stroke(rect, 0.0, Stroke::new(width, theme::text_primary()), StrokeKind::Inside);
            }
            if response.clicked() {
                picked = Some(colour);
            }
        }
        // "+": any colour.
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(SWATCH), Sense::click());
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, any_open, "Any colour"));
        let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        ui.painter().rect_filled(rect, 0.0, fill);
        widgets::text_in(
            ui.painter(),
            rect,
            egui::Align2::CENTER_CENTER,
            "+",
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_primary(),
        );
        if response.clicked() {
            any_open = !any_open;
        }
    });
    if any_open {
        let value_id = any_id.with("value");
        let start = current.map_or(Color32::GRAY, color32);
        let mut value = ui.data(|d| d.get_temp::<Color32>(value_id)).unwrap_or(start);
        let changed = egui::color_picker::color_picker_color32(ui, &mut value, egui::color_picker::Alpha::Opaque);
        ui.data_mut(|d| d.insert_temp(value_id, value));
        // Taken when the drag in the field ends, so one pick is one undo step.
        if (changed || ui.data(|d| d.get_temp::<bool>(value_id.with("moved"))).unwrap_or(false))
            && ui.input(|i| i.pointer.any_released())
        {
            ui.data_mut(|d| d.remove::<bool>(value_id.with("moved")));
            picked = Some(to_rgb(value));
        } else if changed {
            ui.data_mut(|d| d.insert_temp(value_id.with("moved"), true));
        }
    }
    ui.data_mut(|d| d.insert_temp(any_id, any_open));
    picked
}

/// A track's colour area in `rect` (drawn by the caller): a click opens the palette under it, and
/// a swatch picked there sets the track's colour. The area is a button named "<track> colour".
pub fn area(
    ui: &Ui,
    rect: Rect,
    id: Id,
    track: TrackId,
    name: &str,
    current: Rgb,
    actions: &mut Vec<Action>,
) -> Response {
    let response = widgets::button_area(ui, rect, id, format!("{name} colour"));
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    egui::Popup::menu(&response).id(id.with("palette")).show(|ui| {
        if let Some(colour) = swatch_row(ui, Some(current)) {
            if colour != current {
                actions.push(set_colour(track, colour));
            }
            ui.close();
        }
    });
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_is_the_themes_track_colours_and_starts_with_the_default_ones() {
        let palette = swatches();
        assert_eq!(&palette[..4], &crate::interaction::grid::track_palette()[..]);
        let mut unique = palette.to_vec();
        unique.sort_by_key(|c| (c.0, c.1, c.2));
        unique.dedup();
        assert_eq!(unique.len(), palette.len(), "every swatch is its own colour");
    }

    #[test]
    fn a_muted_fill_is_dimmer_and_the_hex_is_what_the_command_reads() {
        let colour = Rgb(0xD0, 0x8F, 0x6E);
        assert_eq!(fill(colour, false), Color32::from_rgb(0xD0, 0x8F, 0x6E));
        assert_ne!(fill(colour, true), fill(colour, false));
        assert_eq!(hex(colour), "#d08f6e");
    }
}
