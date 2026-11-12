// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Track colours: the palette every colour choice offers, defined once (the track's menu, the
//! picker under a track's colour area, a colour tag's menu), and the colour area itself, which a
//! click opens the palette from (`docs/design/components.md` › Colour palette). Picking a colour
//! is the `track.setColor` command, one undo step.

use egui::{Color32, Id, Key, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};
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

/// A colour picked in [`swatch_row`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pick {
    pub colour: Rgb,
    /// Whether a swatch was clicked. A colour taken from the field when the button was let go
    /// is not: the menu stays open for the next drag.
    pub swatch: bool,
}

impl Pick {
    /// Closes the menu the row is in after a swatch; a colour from the field keeps it open.
    pub fn close_menu(self, ui: &Ui) {
        if self.swatch {
            ui.close();
        }
    }
}

/// The palette's swatches in one row, `current` outlined: the swatch clicked, if any. Each
/// swatch is a button named "Colour #rrggbb".
fn swatch_strip(ui: &mut Ui, current: Option<Rgb>) -> Option<Rgb> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(SWATCH_GAP);
        for colour in swatches() {
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(SWATCH), Sense::click());
            let name = format!("Colour {}", hex(colour));
            let chosen = current == Some(colour);
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, chosen, &name));
            ui.painter().rect_filled(rect, 0.0, color32(colour));
            if chosen || response.hovered() {
                let width = if chosen { 1.5 } else { 1.0 };
                ui.painter().rect_stroke(rect, 0.0, Stroke::new(width, theme::text_primary()), StrokeKind::Inside);
            }
            if response.clicked() {
                picked = Some(colour);
            }
        }
    });
    picked
}

/// The palette as a row of swatches, `current` outlined, and after it "+" for any colour: a
/// colour field and hue strip as the theme editor's, the colour taken when the button is let
/// go. Gives the colour picked — a swatch's click and the field's drag's end alike, the field's
/// keeps a menu open ([#1254] review; the steps' menus with it, and the track's menu closes
/// only for a swatch, as main asks).
pub fn swatch_row(ui: &mut Ui, current: Option<Rgb>) -> Option<Pick> {
    let mut picked = swatch_strip(ui, current).map(|colour| Pick { colour, swatch: true });
    let any_id = ui.scope_id().with("any-colour");
    // A row that was not drawn in the pass before was closed with the palette around it (a
    // menu's or a colour area's): it opens as new, "Any colour" shut and its field holding
    // nothing of the last time, so no adjustment starts from an older colour than the one
    // it edits now ([#1254] review). The palettes' callers need no close of their own.
    let pass = ui.ctx().cumulative_pass_nr();
    let seen_id = any_id.with("seen");
    let shown_before = ui.data(|d| d.get_temp::<u64>(seen_id)).is_some_and(|seen| seen.saturating_add(1) >= pass);
    ui.data_mut(|d| d.insert_temp(seen_id, pass));
    if !shown_before {
        ui.data_mut(|d| forget_state(d, any_id));
    }
    let was_any_open = ui.data(|d| d.get_temp::<bool>(any_id)).unwrap_or(false);
    let mut any_open = was_any_open;
    // "+": any colour.
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(SWATCH), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, any_open, "Any colour"));
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
    if any_open {
        // A swatch clicked is the pick: the click's release would otherwise give the field's
        // pending colour too, and an unfinished adjustment won over the swatch ([#1254]
        // review).
        let field = stepped_color_field(ui, any_id.with("value"), current.map_or(Color32::GRAY, color32));
        picked = picked.or(field.map(|colour| Pick { colour, swatch: false }));
    }
    if let Some(picked) = picked {
        // The field follows the pick: the next adjustment starts from it ([#1254] review).
        ui.data_mut(|d| {
            d.insert_temp(any_id.with("value"), color32(picked.colour));
            d.remove::<bool>(any_id.with("value").with("moved"));
        });
    } else if !any_open {
        // Shut, the field keeps nothing of its own: it opens on `current`, whatever changed
        // the colour since a pick in a palette that stays shown ([#1254] review).
        ui.data_mut(|d| {
            d.remove::<Color32>(any_id.with("value"));
            d.remove::<bool>(any_id.with("value").with("moved"));
        });
    }
    // A closing "Any colour" leaves no stale colour behind, as the rows' field does
    // ([#1254] review).
    if was_any_open && !any_open {
        ui.data_mut(|d| {
            d.remove::<Color32>(any_id.with("value"));
            d.remove::<bool>(any_id.with("value").with("moved"));
        });
    }
    ui.data_mut(|d| d.insert_temp(any_id, any_open));
    picked
}

/// The colour field: `value` follows the field as it is dragged and is kept in `id` between
/// frames, and the colour is given when a drag in it ends — so one pick is one step, one undo
/// of what the caller records it as. Swatch_row's "+" and the browser's Select colour rows
/// share it.
pub fn stepped_color_field(ui: &mut Ui, id: Id, start: Color32) -> Option<Rgb> {
    let mut value = ui.data(|d| d.get_temp::<Color32>(id)).unwrap_or(start);
    let changed = egui::color_picker::color_picker_color32(ui, &mut value, egui::color_picker::Alpha::Opaque);
    ui.data_mut(|d| d.insert_temp(id, value));
    // Taken when the drag in the field ends, so one pick is one undo step — or when the
    // field's value is committed from the keyboard: Enter gives the typed colour whether or
    // not the pointer lets go (#1196).
    if (changed || ui.data(|d| d.get_temp::<bool>(id.with("moved"))).unwrap_or(false))
        && (ui.input(|i| i.pointer.any_released()) || ui.input(|i| i.key_pressed(Key::Enter)))
    {
        ui.data_mut(|d| d.remove::<bool>(id.with("moved")));
        Some(to_rgb(value))
    } else {
        if changed {
            ui.data_mut(|d| d.insert_temp(id.with("moved"), true));
        }
        None
    }
}

/// The palette's swatches and a `{label}` press for the theme editor's picker, opened right
/// here, where a picker of its own would end the menu it sits in as a click outside it does.
/// The press carries a square of the current colour, as the tag editor's colour button does,
/// and reads `{label}…` while the field is open. Gives the colour picked — a swatch clicked,
/// or the field's drag ended. `id` keeps one row's own picker state between frames.
pub fn select_colour_row(ui: &mut Ui, id: Id, current: Option<Rgb>, label: &str) -> Option<Rgb> {
    let mut picked = swatch_strip(ui, current);
    let was_open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let text = if open { format!("{label}…") } else { label.to_string() };
    // The row is its content's own width: a popup menu given the parent's whole room would
    // stretch to it and drag the menu out to that width.
    let width = widgets::text_width(ui, &text, theme::ui_font(theme::TEXT_SMALL)) + SWATCH + 6.0 + theme::PAD * 2.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, SWATCH + 8.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, text.clone()));
    let fill = widgets::hover(ui, &response);
    if fill > 0.0 {
        ui.painter().rect_filled(rect, 3.0, theme::panel_hover().gamma_multiply(fill));
    }
    let square =
        Rect::from_center_size(Pos2::new(rect.left() + 6.0 + SWATCH / 2.0, rect.center().y), Vec2::splat(SWATCH));
    ui.painter().rect_filled(square, 2.0, current.map_or(Color32::GRAY, color32));
    ui.painter().rect_stroke(square, 2.0, Stroke::new(0.5, theme::border()), StrokeKind::Inside);
    ui.painter().text(
        Pos2::new(square.right() + 5.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &text,
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_secondary(),
    );
    if response.clicked() {
        open = !open;
    }
    if open {
        // A swatch clicked is the pick, and its draft goes below: the click's release would
        // otherwise give the field's pending colour, and an adjustment never committed won
        // over the swatch ([#1254] review).
        let field = stepped_color_field(ui, id.with("value"), current.map_or(Color32::GRAY, color32));
        picked = picked.or(field);
    }
    if let Some(picked) = picked {
        // The field's own cache follows the pick, even while it is closed: the next custom
        // adjustment starts from what was picked, not from the older colour it still held
        // ([#1254] review, #1196).
        ui.data_mut(|d| {
            d.insert_temp(id.with("value"), color32(picked));
            d.remove::<bool>(id.with("value").with("moved"));
        });
    } else if !open {
        // Nothing is being adjusted: the field's cache stays with the colour of what it
        // edits, so an outer change (a command's, another menu's) is not lost when the
        // field reopens ([#1254] review). Being open holds the field's own draft, as a
        // text field holds a typed one.
        ui.data_mut(|d| d.insert_temp(id.with("value"), current.map_or(Color32::GRAY, color32)));
    }
    // A palette closing drops the field's own draft: the mid-adjust colour it held goes
    // with the palette ([#1254] review), and a later opening starts from the colour the
    // row edits now. A widget that runs no frame (a menu closed by another's open) skips
    // the sync above, so the close itself clears.
    if was_open && !open {
        ui.data_mut(|d| {
            d.remove::<Color32>(id.with("value"));
            d.remove::<bool>(id.with("value").with("moved"));
        });
    }
    ui.data_mut(|d| d.insert_temp(id, open));
    picked
}

/// Forgets a row's picker state — the items a menu's close drops with its drafts: the
/// field's open flag, its cache and the moved flag ([#1254] review). A caller that holds
/// `Context::data_mut` already puts the removals inside it; this waits for none.
pub fn forget_state(d: &mut egui::util::IdTypeMap, id: Id) {
    d.remove::<bool>(id);
    d.remove::<Color32>(id.with("value"));
    d.remove::<bool>(id.with("value").with("moved"));
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
    egui::Popup::menu(&response)
        .id(id.with("palette"))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            if let Some(pick) = swatch_row(ui, Some(current)) {
                if pick.colour != current {
                    actions.push(set_colour(track, pick.colour));
                }
                pick.close_menu(ui);
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

    /// A swatch clicked while the field holds an adjustment never committed is the pick: the
    /// click's release gave the pending colour instead ([#1254] review).
    #[test]
    fn a_clicked_swatch_wins_over_a_pending_custom_colour() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable as _;
        let id = Id::unique("test-select-colour");
        let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(400.0, 400.0)).build_ui_state(
            |ui, picked: &mut Vec<Rgb>| {
                if let Some(colour) = select_colour_row(ui, id, Some(Rgb(1, 2, 3)), "Select colour") {
                    picked.push(colour);
                }
            },
            Vec::new(),
        );
        harness.run();
        harness.get_by_label("Select colour").click();
        harness.run();
        // An adjustment moved in the field and never let go of.
        harness.ctx.data_mut(|d| {
            d.insert_temp(id.with("value"), Color32::from_rgb(9, 9, 9));
            d.insert_temp(id.with("value").with("moved"), true);
        });
        let swatch = swatches()[2];
        harness.get_by_label(&format!("Colour {}", hex(swatch))).click();
        harness.run();
        assert_eq!(harness.state().as_slice(), [swatch], "the swatch alone was picked");
    }

    /// A palette closed with its "Any colour" open and an adjustment pending opens again
    /// afresh: no older colour than the one it edits seeds a later pick ([#1254] review).
    #[test]
    fn a_palette_opened_again_starts_its_any_colour_afresh() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable as _;
        let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(400.0, 400.0)).build_ui_state(
            |ui, (shown, any_id): &mut (bool, Option<Id>)| {
                *any_id = Some(ui.scope_id().with("any-colour"));
                if *shown {
                    let _ = swatch_row(ui, Some(Rgb(10, 20, 30)));
                }
            },
            (true, None),
        );
        harness.run();
        harness.get_by_label("Any colour").click();
        harness.run();
        let any_id = harness.state().1.unwrap_or(Id::NULL);
        assert_eq!(harness.ctx.data(|d| d.get_temp::<bool>(any_id)), Some(true), "Any colour opened");
        harness.ctx.data_mut(|d| {
            d.insert_temp(any_id.with("value"), Color32::from_rgb(9, 9, 9));
            d.insert_temp(any_id.with("value").with("moved"), true);
        });
        // The palette closes (its row is not drawn) and opens again.
        harness.state_mut().0 = false;
        harness.run();
        harness.state_mut().0 = true;
        harness.run();
        assert_eq!(harness.ctx.data(|d| d.get_temp::<bool>(any_id)), Some(false), "Any colour shut");
        assert_eq!(harness.ctx.data(|d| d.get_temp::<Color32>(any_id.with("value"))), None, "no old colour kept");
        assert_eq!(harness.ctx.data(|d| d.get_temp::<bool>(any_id.with("value").with("moved"))), None);
    }

    /// A palette that stays shown after a swatch pick opens its Any colour on the colour it
    /// edits now, not on the swatch picked before an outer change ([#1254] review).
    #[test]
    fn a_shown_palettes_any_colour_opens_on_the_current_colour() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable as _;
        let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(400.0, 400.0)).build_ui_state(
            |ui, (current, any_id): &mut (Rgb, Option<Id>)| {
                *any_id = Some(ui.scope_id().with("any-colour"));
                if let Some(picked) = swatch_row(ui, Some(*current)) {
                    *current = picked.colour;
                }
            },
            (Rgb(10, 20, 30), None),
        );
        harness.run();
        let swatch = swatches()[1];
        harness.get_by_label(&format!("Colour {}", hex(swatch))).click();
        harness.run();
        assert_eq!(harness.state().0, swatch);
        // The colour changes elsewhere, the palette still shown; Any colour opens.
        let outer = Rgb(200, 100, 50);
        harness.state_mut().0 = outer;
        harness.run();
        harness.get_by_label("Any colour").click();
        harness.run();
        let any_id = harness.state().1.unwrap_or(Id::NULL);
        assert_eq!(harness.ctx.data(|d| d.get_temp::<Color32>(any_id.with("value"))), Some(color32(outer)));
    }

    #[test]
    fn a_muted_fill_is_dimmer_and_the_hex_is_what_the_command_reads() {
        let colour = Rgb(0xD0, 0x8F, 0x6E);
        assert_eq!(fill(colour, false), Color32::from_rgb(0xD0, 0x8F, 0x6E));
        assert_ne!(fill(colour, true), fill(colour, false));
        assert_eq!(hex(colour), "#d08f6e");
    }
}
