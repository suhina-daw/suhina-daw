// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Track colors: the palette every color choice offers, defined once (the track's menu, the
//! picker under a track's color area, a color tag's menu), and the color area itself, which a
//! click opens the palette from (`docs/design/components.md` › Color palette). Picking a color
//! is the `track.setColor` command, one undo step.

use egui::{Color32, Id, Key, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{Rgb, TrackId};

use crate::interaction::grid::to_rgb;
use crate::kit::motion;
use crate::kit::theme;
use crate::kit::widgets::{self, Action};

mod names;
pub(crate) mod panel;

/// A swatch's side, and the space between swatches.
pub const SWATCH: f32 = 14.0;
const SWATCH_GAP: f32 = 3.0;
/// How much of its color a muted track's color area keeps over `control.background`.
const MUTED: f32 = 0.55;

/// The palette: Suhina's 36 track colors, the same in every theme (a theme draws a track's
/// color but never chooses it, #1020).
pub fn swatches() -> [Rgb; 36] {
    suhina_session::colors::PALETTE
}

/// `#rrggbb`, as `track.setColor` takes it.
pub fn hex(color: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", color.0, color.1, color.2)
}

pub fn color32(color: Rgb) -> Color32 {
    Color32::from_rgb(color.0, color.1, color.2)
}

/// What a track's color area is filled with: the color, dimmed while the track is off.
pub fn fill(color: Rgb, muted: bool) -> Color32 {
    let color = color32(color);
    if muted { motion::mix(crate::kit::element::app_fill("control"), color, MUTED) } else { color }
}

/// Sets `track`'s color.
pub fn set_color(track: TrackId, color: Rgb) -> Action {
    Action::new("track.setColor", json!({ "track": track.0, "color": hex(color) }))
}

/// A color picked in [`swatch_row`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pick {
    pub color: Rgb,
    /// Whether a swatch was clicked. A color taken from the field when the button was let go
    /// is not: the menu stays open for the next drag.
    pub swatch: bool,
}

impl Pick {
    /// Closes the menu the row is in after a swatch; a color from the field keeps it open.
    pub fn close_menu(self, ui: &Ui) {
        if self.swatch {
            ui.close();
        }
    }
}

/// The palette's swatches in three rows, `current` outlined: the swatch clicked, if any. Each
/// swatch is a button named "Color #rrggbb".
pub(crate) fn swatch_strip(ui: &mut Ui, current: Option<Rgb>) -> Option<Rgb> {
    match swatch_grid(ui, &swatches(), current, "Color", SWATCH, None) {
        Some(GridPick::Color(color)) => Some(color),
        _ => None,
    }
}

/// The width of a grid of the palette's columns of swatches `side` points square.
fn grid_width(side: f32) -> f32 {
    let columns = suhina_session::colors::PALETTE_COLUMNS as f32;
    columns * side + (columns - 1.0) * SWATCH_GAP
}

/// The height of `rows` rows of swatches `side` points square.
fn grid_height(rows: usize, side: f32) -> f32 {
    let rows = rows.max(1) as f32;
    rows * side + (rows - 1.0) * SWATCH_GAP
}

/// The side of a menu's square swatches: as large as fills the menu's width, which its other
/// entries set, and [`SWATCH`] in the pass that measures them (a grid as wide as the room it is
/// given there would set the menu's width itself) or where the menu is narrower.
fn menu_side(ui: &Ui) -> f32 {
    if ui.is_sizing_pass() {
        return SWATCH;
    }
    let columns = suhina_session::colors::PALETTE_COLUMNS as f32;
    ((ui.available_width() - (columns - 1.0) * SWATCH_GAP) / columns).max(SWATCH)
}

/// `colors` as a grid of swatches `side` points square in the palette's columns, the same gap
/// between columns and rows, `current` outlined: the swatch clicked, if any. Each swatch is a
/// button named "`label` #rrggbb". With `more`, the cell after the last color is a "..." button
/// named `more`, whose click gives [`GridPick::More`].
fn swatch_grid(
    ui: &mut Ui,
    colors: &[Rgb],
    current: Option<Rgb>,
    label: &str,
    side: f32,
    more: Option<&str>,
) -> Option<GridPick> {
    let columns = suhina_session::colors::PALETTE_COLUMNS;
    let cells = colors.len() + usize::from(more.is_some());
    let rows = cells.div_ceil(columns);
    let (area, _) = ui.allocate_exact_size(Vec2::new(grid_width(side), grid_height(rows, side)), Sense::hover());
    let scope = ui.scope_id().with(("swatch-grid", label));
    let cell = |index: usize| {
        let (row, column) = (index / columns, index % columns);
        let min = area.min + Vec2::new(column as f32, row as f32) * (side + SWATCH_GAP);
        Rect::from_min_size(min, Vec2::splat(side))
    };
    let mut picked = None;
    let mut chosen_swatch = None;
    for (index, &color) in colors.iter().enumerate() {
        let rect = cell(index);
        let response = ui.interact(rect, scope.with(index), Sense::click());
        let name = format!("{label} {}", hex(color));
        let chosen = current == Some(color);
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), chosen, &name));
        ui.painter().rect_filled(rect, 0.0, color32(color));
        if chosen {
            chosen_swatch = Some((response.clone(), color));
        } else if response.hovered() || response.has_focus() {
            swatch_ring(ui.painter(), &response, color, false);
        }
        if response.clicked() {
            picked = Some(GridPick::Color(color));
        }
        swatch_tooltip(response, color);
    }
    if let Some(name) = more {
        let rect = cell(colors.len());
        let response = ui.interact(rect, scope.with("more"), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), name));
        let button = crate::kit::element::BUTTON.response(&response).enter();
        let fill = button.hover_fade(widgets::hover(ui, &response), crate::kit::element::Look::background);
        ui.painter().rect_filled(rect, 0.0, fill);
        let font = theme::ui_font(theme::TEXT_SMALL);
        widgets::text_in(ui.painter(), rect, egui::Align2::CENTER_CENTER, "...", font, button.color());
        drop(button);
        if response.clicked() {
            picked = Some(GridPick::More);
        }
        response.on_hover_text(name);
    }
    // The chosen swatch's outer outline overlaps the gap round it: drawn last, over its
    // neighbors'.
    if let Some((response, color)) = chosen_swatch {
        swatch_ring(ui.painter(), &response, color, true);
    }
    picked
}

/// What a click in [`swatch_grid`] chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GridPick {
    Color(Rgb),
    /// The "..." cell after the colors that fit.
    More,
}

/// Which swatches a color menu shows: the palette, or the saved colors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Palette,
    Saved,
}

/// Where the context keeps the saved colors for the menus (set by the app, copied when they change) and
/// which page the menus last showed.
fn saved_id() -> Id {
    Id::unique("color-menu-saved")
}
fn page_id() -> Id {
    Id::unique("color-menu-page")
}

/// Hands the menus the user's saved colors, in the order the color window shows them. The list is
/// copied only when it differs from the one the context holds, so a frame with no change copies
/// nothing.
pub fn set_menu_saved(ctx: &egui::Context, saved: &[Rgb]) {
    ctx.data_mut(|d| {
        let same = d.get_temp::<std::sync::Arc<Vec<Rgb>>>(saved_id()).is_some_and(|held| held.as_slice() == saved);
        if !same {
            d.insert_temp(saved_id(), std::sync::Arc::new(saved.to_vec()));
        }
    });
}

/// How many saved colors a menu's page shows: the palette's cells, the last one "..." when there
/// are more.
const MENU_SAVED: usize = 36;

/// A menu's swatches — the palette, or the saved colors — as a grid of square swatches as wide
/// as the menu, and under it the arrows that switch between them beside the page's name, then
/// More colors at the row's end. The saved colors take the palette's cells: past them, the last
/// is "..." (More saved colors), which opens the color window as More colors does. The page is
/// kept for the next menu. The swatch clicked, or [`GridPick::More`] for either way to the
/// window.
fn swatch_pages(ui: &mut Ui, current: Option<Rgb>, shortcut: &str) -> Option<GridPick> {
    let mut page = ui.data(|d| d.get_temp::<Page>(page_id())).unwrap_or_default();
    let side = menu_side(ui);
    let palette_rows = MENU_SAVED.div_ceil(suhina_session::colors::PALETTE_COLUMNS);
    let height = grid_height(palette_rows, side);
    let mut picked = match page {
        Page::Palette => swatch_grid(ui, &swatches(), current, "Color", side, None),
        Page::Saved => {
            let saved = ui.data(|d| d.get_temp::<std::sync::Arc<Vec<Rgb>>>(saved_id())).unwrap_or_default();
            if saved.is_empty() {
                let (rect, response) = ui.allocate_exact_size(Vec2::new(grid_width(side), height), Sense::hover());
                let text = "No saved colors";
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
                let ink = crate::kit::element::LABEL.attribute("kind", "hint").look().color();
                widgets::text_in(
                    ui.painter(),
                    rect,
                    egui::Align2::CENTER_CENTER,
                    text,
                    theme::ui_font(theme::TEXT_SMALL),
                    ink,
                );
                None
            } else {
                let top = ui.cursor().top();
                let overflow = saved.len() > MENU_SAVED;
                let shown = if overflow { &saved[..MENU_SAVED - 1] } else { &saved[..] };
                let more = overflow.then_some("More saved colors");
                let picked = swatch_grid(ui, shown, current, "Saved color", side, more);
                // As tall as the palette, so turning the page keeps the menu's size.
                let drawn = ui.cursor().top() - top - ui.spacing().item_spacing.y;
                if drawn < height {
                    ui.allocate_exact_size(Vec2::new(grid_width(side), height - drawn), Sense::hover());
                }
                picked
            }
        }
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let back = crate::kit::controls::arrow_button(ui, "Previous swatches", true).clicked();
        let next = crate::kit::controls::arrow_button(ui, "Next swatches", false).clicked();
        // Two pages: either arrow turns to the other.
        if back || next {
            page = if page == Page::Palette { Page::Saved } else { Page::Palette };
        }
        ui.add_space(3.0);
        crate::kit::controls::text(ui, if page == Page::Palette { "Palette" } else { "Saved colors" });
        // At the row's end, once the menu's width is measured: in that pass it follows the
        // name, so the row asks for no more room than it needs.
        let mut more = |ui: &mut Ui| {
            let button = crate::kit::controls::button(ui, "More colors");
            let button = if shortcut.is_empty() { button } else { button.on_hover_text(shortcut) };
            if button.clicked() {
                picked = Some(GridPick::More);
            }
        };
        if ui.is_sizing_pass() {
            ui.add_space(8.0);
            more(ui);
        } else {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), more);
        }
    });
    ui.data_mut(|d| d.insert_temp(page_id(), page));
    picked
}

/// The ring round the swatch `response` of `color`. Under the pointer, one point inside its edge
/// of `color-swatch:hover`'s `border-color`, or the ink that reads on the color where that does
/// not. The chosen one (`chosen`) has two: a point of `color-swatch[selected]`'s `border-color`
/// just outside it, which reads on the surface around it, and two points of black inside its edge.
pub(crate) fn swatch_ring(painter: &egui::Painter, response: &Response, color: Rgb, chosen: bool) {
    let rect = response.rect;
    let swatch = crate::kit::element::COLOR_SWATCH.response(response);
    if chosen {
        let outer = swatch.attr(crate::kit::element::Selected, true).look().border();
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, outer), StrokeKind::Outside);
        painter.rect_stroke(rect, 0.0, Stroke::new(2.0, Color32::BLACK), StrokeKind::Inside);
    } else {
        let edge = swatch.look().border();
        let fill = color32(color);
        let ring = if theme::text_contrast(edge, fill) >= 3.0 { edge } else { theme::text_on(fill) };
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, ring), StrokeKind::Inside);
    }
}

/// A swatch's tooltip: `color`'s nearest name and its exact hex.
pub(crate) fn swatch_tooltip(response: Response, color: Rgb) {
    response.on_hover_ui(|ui| {
        ui.label(format!("{} · {}", names::describe(color), hex(color)));
    });
}

/// What a swatch of `color` is called to a screen reader, after `label`: its nearest color
/// name and its exact hex, as `Palette blue #3a7bd5`.
pub fn swatch_name(label: &str, color: Rgb) -> String {
    format!("{label} {} {}", names::describe(color), hex(color))
}

/// Where the context keeps whether a swatch last got the keys from a click rather than Tab.
fn by_pointer_id() -> Id {
    Id::unique("color-swatch-focus-by-pointer")
}

/// Give a clicked swatch the keys, so the arrows go on from it, without its focus ring: the ring
/// shows only once Tab moves the keys (see [`focus_ring`]).
pub(crate) fn focus_by_pointer(response: &Response) {
    response.request_focus();
    response.ctx.data_mut(|d| d.insert_temp(by_pointer_id(), true));
}

/// The ring round the swatch with the keys in `rect`: two points of `color-swatch:focus`'s
/// `accent-color` (the accent where a theme gives none) a little outside it, so it reads apart
/// from the ring of the chosen swatch and of the one under the pointer. Drawn after the swatches
/// around it, which it overlaps, and only while the keys came by Tab: after a click (see
/// [`focus_by_pointer`]) it waits for the next Tab, as a browser's `:focus-visible` does.
pub(crate) fn focus_ring(ui: &Ui, rect: Rect) {
    if ui.data(|d| d.get_temp::<bool>(by_pointer_id())).unwrap_or(false) {
        return;
    }
    let painter = ui.painter();
    let swatch = crate::kit::element::COLOR_SWATCH.pointer(false, false, false).focused(true);
    let accent = swatch.look().accent_or(crate::kit::element::accent_ink());
    painter.rect_stroke(rect.expand(1.0), 0.0, Stroke::new(2.0, accent), StrokeKind::Outside);
}

/// A grid of swatches as one stop for Tab, `ids` in reading order in rows of `columns`: the
/// swatch with the keys, else `fallback` (the chosen one), else the first, takes Tab; the others
/// take only the pointer. While one has the keys the arrows move them to the next swatch that
/// way, and Home and End to the first and last of its row, and pick it, as a slider's arrows
/// move its value; Enter and Space press the one with the keys.
pub(crate) struct Roving<'a> {
    ids: &'a [Id],
    columns: usize,
    /// The place of the swatch with the keys.
    pub focused: Option<usize>,
    stop: usize,
}

impl<'a> Roving<'a> {
    pub fn new(ui: &Ui, ids: &'a [Id], columns: usize, fallback: Option<usize>) -> Self {
        // Tab moves the keys from here on, so the focus ring shows again (see [`focus_ring`]).
        if ui.input(|i| i.key_pressed(Key::Tab)) {
            ui.data_mut(|d| d.insert_temp(by_pointer_id(), false));
        }
        let focused = ui.memory(|m| m.focused()).and_then(|id| ids.iter().position(|i| *i == id));
        let stop = focused.or(fallback).filter(|&i| i < ids.len()).unwrap_or(0);
        Self { ids, columns: columns.max(1), focused, stop }
    }

    /// `base` for the swatch at `index`, focusable only for the one that takes Tab.
    pub fn sense(&self, index: usize, base: Sense) -> Sense {
        if index == self.stop { base } else { base.difference(Sense::FOCUSABLE) }
    }

    /// Move the keys with the arrows, Home and End; call it after the swatches are drawn. The
    /// place they moved to, which the caller picks.
    pub fn keys(&self, ui: &Ui) -> Option<usize> {
        let at = self.focused?;
        let &id = self.ids.get(at)?;
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                id,
                egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() },
            );
        });
        let none = egui::Modifiers::NONE;
        let [left, right, up, down, home, end] = ui.input_mut(|i| {
            [Key::ArrowLeft, Key::ArrowRight, Key::ArrowUp, Key::ArrowDown, Key::Home, Key::End]
                .map(|key| i.count_and_consume_key(none, key) as isize)
        });
        let last = self.ids.len().saturating_sub(1) as isize;
        let row_start = (at - at % self.columns) as isize;
        let mut next = at as isize + right - left + (down - up) * self.columns as isize;
        if home > 0 {
            next = row_start;
        }
        if end > 0 {
            next = row_start + self.columns as isize - 1;
        }
        // Up from the first row or down from the last stays in the column's end of the grid.
        if !(0..=last).contains(&next) && (up > 0 || down > 0) && left == 0 && right == 0 {
            next = at as isize;
        }
        let next = next.clamp(0, last) as usize;
        let &id = self.ids.get(next).filter(|_| next != at)?;
        ui.memory_mut(|m| m.request_focus(id));
        Some(next)
    }
}

/// A track menu's compact palette; + opens the persistent window through the app.
pub fn track_row(ui: &mut Ui, current: Rgb, tracks: &[TrackId], actions: &mut Vec<Action>) {
    let ids: Vec<u32> = tracks.iter().map(|t| t.0).collect();
    let shortcut = ui.data(|d| d.get_temp::<String>(Id::unique("color-picker-shortcut"))).unwrap_or_default();
    let more = Action::new("ui.colorPicker", json!({ "tracks": ids }));
    color_row(ui, Some(current), tracks.len() > 1, &shortcut, more, actions, |color| {
        Action::new("track.setColor", json!({ "tracks": ids, "color": hex(color) }))
    });
}

/// The same palette for arrangement clips, `current` their color where it is one: a swatch
/// colors them all as one undo step, and More colors opens the persistent window on them.
pub fn clip_row(ui: &mut Ui, current: Option<Rgb>, clips: &[suhina_engine::model::ClipId], actions: &mut Vec<Action>) {
    let ids: Vec<u32> = clips.iter().map(|c| c.0).collect();
    let list: Vec<serde_json::Value> = ids.iter().map(|id| json!({ "clip": id })).collect();
    let more = Action::new("ui.colorPicker", json!({ "clips": ids }));
    color_row(ui, current, clips.len() > 1 || current.is_none(), "", more, actions, |color| {
        Action::new("clip.setColorMany", json!({ "clips": list, "color": hex(color) }))
    });
}

/// The menu's swatches and More colors, `shortcut` in its tooltip. A swatch picked gives `set`'s
/// action (also for the current color when `always`, as several targets may differ) and More
/// colors (or a saved page's "...") gives `more`; either closes the menu.
fn color_row(
    ui: &mut Ui,
    current: Option<Rgb>,
    always: bool,
    shortcut: &str,
    more: Action,
    actions: &mut Vec<Action>,
    set: impl FnOnce(Rgb) -> Action,
) {
    match swatch_pages(ui, current, shortcut) {
        Some(GridPick::Color(color)) => {
            if always || Some(color) != current {
                actions.push(set(color));
            }
            ui.close();
        }
        Some(GridPick::More) => {
            actions.push(more);
            ui.close();
        }
        None => {}
    }
}

/// The palette as a row of swatches, `current` outlined, and after it "+" for any color: a
/// color field and hue strip as the theme editor's, the color taken when the button is let
/// go. Gives the color picked — a swatch's click and the field's drag's end alike, the field's
/// keeps a menu open ([#1254] review; the steps' menus with it, and the track's menu closes
/// only for a swatch, as main asks).
pub fn swatch_row(ui: &mut Ui, current: Option<Rgb>) -> Option<Pick> {
    let mut picked = swatch_strip(ui, current).map(|color| Pick { color, swatch: true });
    let any_id = ui.scope_id().with("any-color");
    // A row that was not drawn in the pass before was closed with the palette around it (a
    // menu's or a color area's): it opens as new, "Any color" shut and its field holding
    // nothing of the last time, so no adjustment starts from an older color than the one
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
    // "+": any color.
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(SWATCH), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, any_open, "Any color"));
    let any = crate::kit::element::BUTTON.response(&response).enter();
    ui.painter().rect_filled(
        rect,
        0.0,
        any.hover_fade(widgets::hover(ui, &response), crate::kit::element::Look::background),
    );
    widgets::text_in(
        ui.painter(),
        rect,
        egui::Align2::CENTER_CENTER,
        "+",
        theme::ui_font(theme::TEXT_SMALL),
        any.color(),
    );
    drop(any);
    if response.clicked() {
        any_open = !any_open;
    }
    if any_open {
        // A swatch clicked is the pick: the click's release would otherwise give the field's
        // pending color too, and an unfinished adjustment won over the swatch ([#1254]
        // review).
        let field = stepped_color_field(ui, any_id.with("value"), current.map_or(Color32::GRAY, color32));
        picked = picked.or(field.map(|color| Pick { color, swatch: false }));
    }
    if let Some(picked) = picked {
        // The field follows the pick: the next adjustment starts from it ([#1254] review).
        ui.data_mut(|d| {
            d.insert_temp(any_id.with("value"), color32(picked.color));
            d.remove::<bool>(any_id.with("value").with("moved"));
        });
    } else if !any_open {
        // Shut, the field keeps nothing of its own: it opens on `current`, whatever changed
        // the color since a pick in a palette that stays shown ([#1254] review).
        ui.data_mut(|d| {
            d.remove::<Color32>(any_id.with("value"));
            d.remove::<bool>(any_id.with("value").with("moved"));
        });
    }
    // A closing "Any color" leaves no stale color behind, as the rows' field does
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

/// The color field: `value` follows the field as it is dragged and is kept in `id` between
/// frames, and the color is given when a drag in it ends — so one pick is one step, one undo
/// of what the caller records it as. Swatch_row's "+" and the browser's Select color rows
/// share it.
pub fn stepped_color_field(ui: &mut Ui, id: Id, start: Color32) -> Option<Rgb> {
    let mut value = ui.data(|d| d.get_temp::<Color32>(id)).unwrap_or(start);
    let changed = egui::color_picker::color_picker_color32(ui, &mut value, egui::color_picker::Alpha::Opaque);
    ui.data_mut(|d| d.insert_temp(id, value));
    // Taken when the drag in the field ends, so one pick is one undo step — or when the
    // field's value is committed from the keyboard: Enter gives the typed color whether or
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
/// The press carries a square of the current color, as the tag editor's color button does,
/// and reads `{label}…` while the field is open. Gives the color picked — a swatch clicked,
/// or the field's drag ended. `id` keeps one row's own picker state between frames.
pub fn select_color_row(ui: &mut Ui, id: Id, current: Option<Rgb>, label: &str) -> Option<Rgb> {
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
    // A `list-row[kind="picker"]`: its fill, shown as the pointer comes over it, and its words.
    let row = crate::kit::element::LIST_ROW.attribute("kind", "picker").response(&response).enter();
    if fill > 0.0 {
        ui.painter().rect_filled(rect, 3.0, row.background().gamma_multiply(fill));
    }
    let square =
        Rect::from_center_size(Pos2::new(rect.left() + 6.0 + SWATCH / 2.0, rect.center().y), Vec2::splat(SWATCH));
    ui.painter().rect_filled(square, 2.0, current.map_or(Color32::GRAY, color32));
    ui.painter().rect_stroke(
        square,
        2.0,
        Stroke::new(0.5, crate::kit::element::COLOR_SWATCH.attribute("kind", "square").look().border()),
        StrokeKind::Inside,
    );
    ui.painter().text(
        Pos2::new(square.right() + 5.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &text,
        theme::ui_font(theme::TEXT_SMALL),
        row.color(),
    );
    drop(row);
    if response.clicked() {
        open = !open;
    }
    if open {
        // A swatch clicked is the pick, and its draft goes below: the click's release would
        // otherwise give the field's pending color, and an adjustment never committed won
        // over the swatch ([#1254] review).
        let field = stepped_color_field(ui, id.with("value"), current.map_or(Color32::GRAY, color32));
        picked = picked.or(field);
    }
    if let Some(picked) = picked {
        // The field's own cache follows the pick, even while it is closed: the next custom
        // adjustment starts from what was picked, not from the older color it still held
        // ([#1254] review, #1196).
        ui.data_mut(|d| {
            d.insert_temp(id.with("value"), color32(picked));
            d.remove::<bool>(id.with("value").with("moved"));
        });
    } else if !open {
        // Nothing is being adjusted: the field's cache stays with the color of what it
        // edits, so an outer change (a command's, another menu's) is not lost when the
        // field reopens ([#1254] review). Being open holds the field's own draft, as a
        // text field holds a typed one.
        ui.data_mut(|d| d.insert_temp(id.with("value"), current.map_or(Color32::GRAY, color32)));
    }
    // A palette closing drops the field's own draft: the mid-adjust color it held goes
    // with the palette ([#1254] review), and a later opening starts from the color the
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

/// A track's color area in `rect` (drawn by the caller): a click opens the palette under it, and
/// a swatch picked there sets the track's color. The area is a button named "<track> color".
pub fn area(
    ui: &Ui,
    rect: Rect,
    id: Id,
    track: TrackId,
    name: &str,
    current: Rgb,
    actions: &mut Vec<Action>,
) -> Response {
    let response = widgets::button_area(ui, rect, id, format!("{name} color"));
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    egui::Popup::menu(&response)
        .id(id.with("palette"))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            track_row(ui, current, &[track], actions);
        });
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_holds_the_colors_new_tracks_take() {
        let palette = swatches();
        // The legacy colors keep what a saved file meant, and need not be swatches.
        use suhina_session::colors::{AUDIO, MIDI};
        assert!([MIDI, AUDIO].iter().all(|color| palette.contains(color)));
        let mut unique = palette.to_vec();
        unique.sort_by_key(|c| (c.0, c.1, c.2));
        unique.dedup();
        assert_eq!(unique.len(), palette.len(), "every swatch is its own color");
    }

    /// A swatch clicked while the field holds an adjustment never committed is the pick: the
    /// click's release gave the pending color instead ([#1254] review).
    #[test]
    fn a_clicked_swatch_wins_over_a_pending_custom_color() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable as _;
        let id = Id::unique("test-select-color");
        let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(400.0, 400.0)).build_ui_state(
            |ui, picked: &mut Vec<Rgb>| {
                if let Some(color) = select_color_row(ui, id, Some(Rgb(1, 2, 3)), "Select color") {
                    picked.push(color);
                }
            },
            Vec::new(),
        );
        harness.run();
        harness.get_by_label("Select color").click();
        harness.run();
        // An adjustment moved in the field and never let go of.
        harness.ctx.data_mut(|d| {
            d.insert_temp(id.with("value"), Color32::from_rgb(9, 9, 9));
            d.insert_temp(id.with("value").with("moved"), true);
        });
        let swatch = swatches()[2];
        harness.get_by_label(&format!("Color {}", hex(swatch))).click();
        harness.run();
        assert_eq!(harness.state().as_slice(), [swatch], "the swatch alone was picked");
    }

    /// A palette closed with its "Any color" open and an adjustment pending opens again
    /// afresh: no older color than the one it edits seeds a later pick ([#1254] review).
    #[test]
    fn a_palette_opened_again_starts_its_any_color_afresh() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable as _;
        let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(400.0, 400.0)).build_ui_state(
            |ui, (shown, any_id): &mut (bool, Option<Id>)| {
                *any_id = Some(ui.scope_id().with("any-color"));
                if *shown {
                    let _ = swatch_row(ui, Some(Rgb(10, 20, 30)));
                }
            },
            (true, None),
        );
        harness.run();
        harness.get_by_label("Any color").click();
        harness.run();
        let any_id = harness.state().1.unwrap_or(Id::NULL);
        assert_eq!(harness.ctx.data(|d| d.get_temp::<bool>(any_id)), Some(true), "Any color opened");
        harness.ctx.data_mut(|d| {
            d.insert_temp(any_id.with("value"), Color32::from_rgb(9, 9, 9));
            d.insert_temp(any_id.with("value").with("moved"), true);
        });
        // The palette closes (its row is not drawn) and opens again.
        harness.state_mut().0 = false;
        harness.run();
        harness.state_mut().0 = true;
        harness.run();
        assert_eq!(harness.ctx.data(|d| d.get_temp::<bool>(any_id)), Some(false), "Any color shut");
        assert_eq!(harness.ctx.data(|d| d.get_temp::<Color32>(any_id.with("value"))), None, "no old color kept");
        assert_eq!(harness.ctx.data(|d| d.get_temp::<bool>(any_id.with("value").with("moved"))), None);
    }

    /// A palette that stays shown after a swatch pick opens its Any color on the color it
    /// edits now, not on the swatch picked before an outer change ([#1254] review).
    #[test]
    fn a_shown_palettes_any_color_opens_on_the_current_color() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable as _;
        let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(400.0, 400.0)).build_ui_state(
            |ui, (current, any_id): &mut (Rgb, Option<Id>)| {
                *any_id = Some(ui.scope_id().with("any-color"));
                if let Some(picked) = swatch_row(ui, Some(*current)) {
                    *current = picked.color;
                }
            },
            (Rgb(10, 20, 30), None),
        );
        harness.run();
        let swatch = swatches()[1];
        harness.get_by_label(&format!("Color {}", hex(swatch))).click();
        harness.run();
        assert_eq!(harness.state().0, swatch);
        // The color changes elsewhere, the palette still shown; Any color opens.
        let outer = Rgb(200, 100, 50);
        harness.state_mut().0 = outer;
        harness.run();
        harness.get_by_label("Any color").click();
        harness.run();
        let any_id = harness.state().1.unwrap_or(Id::NULL);
        assert_eq!(harness.ctx.data(|d| d.get_temp::<Color32>(any_id.with("value"))), Some(color32(outer)));
    }

    #[test]
    fn a_muted_fill_is_dimmer_and_the_hex_is_what_the_command_reads() {
        let color = Rgb(0xD0, 0x8F, 0x6E);
        assert_eq!(fill(color, false), Color32::from_rgb(0xD0, 0x8F, 0x6E));
        assert_ne!(fill(color, true), fill(color, false));
        assert_eq!(hex(color), "#d08f6e");
    }
}
