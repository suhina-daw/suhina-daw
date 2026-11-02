// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's scale (`docs/spec/ui-clip-editor.md` › Scale and key, › Zoom and scroll ›
//! Fold): the toolbar's Scale menu, the rows and key-column marks of the keys in the clip's scale,
//! and folding the rows to the keys that have notes or to the scale's keys.
//!
//! Folding is window state, like the ghost menu's choice: the rows the grid shows are a list of
//! keys ([`KeyRows`]), and every place that turns a height into a key or a key into a height goes
//! through it, so a folded view draws, hits and adds notes on the rows it shows.

use egui::{Id, Pos2, Rect, Sense, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{ROOT_NAMES, Scale, ScaleMode};

use crate::detail::piano_roll::{PianoClip, key_name};
use crate::kit::theme;
use crate::kit::widgets::Action;

/// How wide the toolbar's Scale menu button is, in points.
pub const MENU_WIDTH: f32 = 56.0;

/// Which key rows the piano roll shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fold {
    /// Every key, 0 to 127.
    #[default]
    Off,
    /// Only the keys that have notes (every key while the clip has none).
    Notes,
    /// Only the keys in the clip's scale, and any key that has a note, so no note is hidden
    /// (every key while the clip has no scale).
    Scale,
}

/// The key rows the grid shows, from the top: every key, or the keys a fold leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyRows {
    /// The shown keys from the highest down; the first `len` are used.
    keys: [u8; 128],
    len: usize,
    /// The row of each key, from the top, or [`HIDDEN`] for a key the fold hides.
    row: [u8; 128],
}

const HIDDEN: u8 = u8::MAX;

impl Default for KeyRows {
    fn default() -> Self {
        let mut rows = Self { keys: [0; 128], len: 128, row: [HIDDEN; 128] };
        for (row, key) in (0..=127u8).rev().enumerate() {
            if let (Some(slot), Some(at)) = (rows.keys.get_mut(row), rows.row.get_mut(usize::from(key))) {
                *slot = key;
                *at = row as u8;
            }
        }
        rows
    }
}

impl KeyRows {
    /// The rows of the keys `shown` keeps, highest first; every key when it keeps none.
    pub fn of(shown: impl Fn(u8) -> bool) -> Self {
        let mut rows = Self { keys: [0; 128], len: 0, row: [HIDDEN; 128] };
        for key in (0..=127u8).rev() {
            if shown(key)
                && let (Some(slot), Some(row)) = (rows.keys.get_mut(rows.len), rows.row.get_mut(usize::from(key)))
            {
                *slot = key;
                *row = rows.len as u8;
                rows.len += 1;
            }
        }
        if rows.len == 0 { Self::default() } else { rows }
    }

    /// How many rows there are.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether there are none (never: an empty fold shows every key).
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether some keys are folded away.
    pub fn folded(&self) -> bool {
        self.len < 128
    }

    /// The row of `key`, from the top; `None` when the fold hides it.
    pub fn row_of(&self, key: u8) -> Option<usize> {
        self.row.get(usize::from(key)).filter(|r| **r != HIDDEN).map(|r| usize::from(*r))
    }

    /// The key in row `row`, from the top.
    pub fn key_of(&self, row: usize) -> Option<u8> {
        (row < self.len).then(|| self.keys.get(row).copied()).flatten()
    }

    /// Whether `key` has a row.
    pub fn shows(&self, key: u8) -> bool {
        self.row_of(key).is_some()
    }

    /// The semitones a vertical drag of `rows_up` rows moves a note on `key` by: it steps through
    /// the shown rows, stopping at the top and the bottom one. Unfolded, a row is a semitone.
    pub fn semitones(&self, key: u8, rows_up: i32) -> i32 {
        let Some(row) = self.row_of(key).filter(|_| self.folded()) else { return rows_up };
        let last = self.len.saturating_sub(1) as i64;
        let target = (row as i64 - i64::from(rows_up)).clamp(0, last);
        self.key_of(target as usize).map_or(0, |to| i32::from(to) - i32::from(key))
    }

    /// The row the middle of `low` to `high` falls in, for centring the view on the notes; the row
    /// nearest each when a key is hidden.
    pub fn middle_row(&self, low: u8, high: u8) -> f32 {
        let near = |key: u8| -> f32 {
            (0..=127u8)
                .flat_map(|d| [key.checked_sub(d), key.checked_add(d).filter(|k| *k <= 127)])
                .flatten()
                .find_map(|k| self.row_of(k))
                .unwrap_or(0) as f32
        };
        (near(low) + near(high)) / 2.0
    }
}

/// The rows `clip` shows with `fold`.
pub fn rows(clip: &PianoClip, fold: Fold) -> KeyRows {
    let mut has_note = [false; 128];
    for note in &clip.notes {
        if let Some(slot) = has_note.get_mut(usize::from(note.key)) {
            *slot = true;
        }
    }
    let noted = |key: u8| has_note.get(usize::from(key)).copied().unwrap_or(false);
    match (fold, clip.scale) {
        (Fold::Notes, _) => KeyRows::of(noted),
        (Fold::Scale, Some(scale)) => KeyRows::of(|key| scale.contains(key) || noted(key)),
        (Fold::Off, _) | (Fold::Scale, None) => KeyRows::default(),
    }
}

/// How a key row is filled: with a scale, the keys in it lighter and its root lighter still;
/// without one, white keys lighter than black keys.
pub(crate) fn row_fill(scale: Option<Scale>, key: u8, black: bool) -> egui::Color32 {
    match scale {
        Some(scale) if scale.contains(key) && key % 12 == scale.root() => theme::control_hover(),
        Some(scale) if scale.contains(key) => theme::control_background(),
        Some(_) => theme::window_background(),
        None if black => theme::window_background(),
        None => theme::panel_background(),
    }
}

/// The 2 px mark at the right of the key column on a key in the scale (the root's brighter).
pub(crate) fn key_mark(painter: &egui::Painter, scale: Option<Scale>, key: u8, row: Rect, color: egui::Color32) {
    if !scale.is_some_and(|s| s.contains(key)) {
        return;
    }
    let mark = Rect::from_min_max(Pos2::new(row.right() - 2.0, row.top()), row.max);
    painter.rect_filled(mark, 0.0, color);
}

/// Labels the key column's rows in view for readers and tests ("Key C3 in scale", "Key C#3"),
/// while the clip has a scale or the rows are folded. The labels sense nothing, so a press there
/// still plays the key.
pub(crate) fn key_readers(
    ui: &Ui,
    keys: Rect,
    clip: &PianoClip,
    rows: &KeyRows,
    y_of: impl Fn(u8) -> f32,
    height: f32,
) {
    if clip.scale.is_none() && !rows.folded() {
        return;
    }
    for row in 0..rows.len() {
        let Some(key) = rows.key_of(row) else { continue };
        let top = y_of(key);
        let rect = Rect::from_min_size(Pos2::new(keys.left(), top), Vec2::new(keys.width(), height)).intersect(keys);
        if !rect.is_positive() {
            continue;
        }
        let label = match clip.scale {
            Some(scale) if scale.contains(key) => format!("Key {} in scale", key_name(key)),
            _ => format!("Key {}", key_name(key)),
        };
        let response = ui.interact(rect, Id::new(("piano-roll-key-row", key)), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label));
    }
}

/// What the Scale menu's button says: "Scale C major", or "Scale off".
pub fn menu_label(scale: Option<Scale>) -> String {
    match scale {
        Some(scale) => format!("Scale {}", scale.describe()),
        None => "Scale off".to_string(),
    }
}

/// The toolbar's Scale menu: Off, a Root submenu with the twelve roots and a Mode submenu with the
/// modes, each a `clip.setScale`; then Fold and Fold to scale, window state. Its button reads
/// "Scale", dimmed while the clip has none; its accessible label and tooltip name the scale.
/// Returns the button's rect.
pub(crate) fn menu(ui: &mut Ui, rect: Rect, clip: &PianoClip, fold: &mut Fold, actions: &mut Vec<Action>) -> Rect {
    let font = theme::ui_font(theme::TEXT_SMALL);
    let scale = clip.scale;
    let mut picked: Option<serde_json::Value> = None;
    let mut folded = None;
    let mut shown = rect;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        let color = if scale.is_none() { theme::text_hint() } else { theme::text_value() };
        let text = egui::RichText::new("Scale   ").font(font.clone()).color(color);
        let menu = ui.menu_button(text, |ui| {
            let entry = |ui: &mut Ui, on: bool, shown: &str, label: &str, enabled: bool| -> bool {
                let response = ui
                    .add_enabled(enabled, egui::Button::selectable(on, egui::RichText::new(shown).font(font.clone())));
                response
                    .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, enabled, on, label));
                let clicked = response.clicked();
                if clicked {
                    ui.close();
                }
                clicked
            };
            if entry(ui, scale.is_none(), "Off", "Scale: Off", true) && scale.is_some() {
                picked = Some(json!({ "off": true }));
            }
            ui.separator();
            let root = scale.map_or("C", Scale::root_name);
            let roots = ui.menu_button(egui::RichText::new(format!("Root: {root}")).font(font.clone()), |ui| {
                for (index, name) in ROOT_NAMES.iter().enumerate() {
                    let on = scale.is_some_and(|s| usize::from(s.root()) == index);
                    if entry(ui, on, name, &format!("Scale root {name}"), true) && !on {
                        picked = Some(json!({ "root": index }));
                    }
                }
            });
            roots.response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Scale root: {root}"))
            });
            let mode = scale.map_or(ScaleMode::Major, |s| s.mode).label();
            let modes = ui.menu_button(egui::RichText::new(format!("Mode: {mode}")).font(font.clone()), |ui| {
                for choice in ScaleMode::ALL {
                    let on = scale.is_some_and(|s| s.mode == choice);
                    if entry(ui, on, choice.label(), &format!("Scale mode {}", choice.label()), true) && !on {
                        picked = Some(json!({ "mode": choice.name() }));
                    }
                }
            });
            modes.response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Scale mode: {mode}"))
            });
            ui.separator();
            if entry(ui, *fold == Fold::Notes, "Fold", "Fold", true) {
                folded = Some(if *fold == Fold::Notes { Fold::Off } else { Fold::Notes });
            }
            if entry(ui, *fold == Fold::Scale, "Fold to scale", "Fold to scale", scale.is_some()) {
                folded = Some(if *fold == Fold::Scale { Fold::Off } else { Fold::Scale });
            }
        });
        let label = menu_label(scale);
        crate::detail::piano_roll::menu_arrow(ui, menu.response.rect, color);
        menu.response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        shown = menu.response.rect;
        let named = scale.map_or_else(|| "none".to_string(), Scale::describe);
        menu.response
            .on_hover_text(format!("Scale: {named}. Marks the keys in the clip's scale; Fold shows fewer rows"));
    });
    if let Some(mut params) = picked {
        if let (Some(params), Some(address)) = (params.as_object_mut(), clip.address.as_object()) {
            for (key, value) in address {
                params.insert(key.clone(), value.clone());
            }
        }
        actions.push(Action::new("clip.setScale", params));
    }
    if let Some(next) = folded {
        *fold = next;
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scale(root: u8, mode: ScaleMode) -> Scale {
        Scale::new(root, mode).unwrap()
    }

    #[test]
    fn unfolded_rows_are_every_key_one_semitone_each() {
        let rows = KeyRows::default();
        assert_eq!(rows.len(), 128);
        assert!(!rows.folded());
        assert_eq!(rows.row_of(127), Some(0));
        assert_eq!(rows.row_of(0), Some(127));
        assert_eq!(rows.key_of(67), Some(60));
        assert_eq!(rows.key_of(128), None);
        assert_eq!(rows.semitones(60, 5), 5, "a row is a semitone");
    }

    #[test]
    fn a_fold_keeps_the_chosen_keys_in_order_and_maps_both_ways() {
        let rows = KeyRows::of(|k| [36, 40, 43, 60].contains(&k));
        assert_eq!(rows.len(), 4);
        assert!(rows.folded());
        assert_eq!((0..4).map(|r| rows.key_of(r)).collect::<Vec<_>>(), [Some(60), Some(43), Some(40), Some(36)]);
        for key in [36, 40, 43, 60] {
            assert_eq!(rows.row_of(key).and_then(|r| rows.key_of(r)), Some(key));
        }
        assert_eq!(rows.row_of(41), None);
        assert!(!rows.shows(61));
        // A drag steps through the rows, and stops at the ends.
        assert_eq!(rows.semitones(40, 1), 3, "40 up one row is 43");
        assert_eq!(rows.semitones(40, 2), 20, "two rows is 60");
        assert_eq!(rows.semitones(40, 9), 20, "never past the top row");
        assert_eq!(rows.semitones(40, -9), -4, "never past the bottom row");
        assert_eq!(rows.middle_row(36, 60), 1.5);
    }

    #[test]
    fn an_empty_fold_shows_every_key() {
        assert_eq!(KeyRows::of(|_| false).len(), 128);
    }

    #[test]
    fn row_colours_follow_the_scale() {
        let c = Some(scale(0, ScaleMode::Major));
        assert_eq!(row_fill(c, 60, false), theme::control_hover(), "the root");
        assert_eq!(row_fill(c, 62, false), theme::control_background(), "in the scale");
        assert_eq!(row_fill(c, 61, true), theme::window_background(), "outside it");
        assert_eq!(row_fill(None, 61, true), theme::window_background());
        assert_eq!(row_fill(None, 60, false), theme::panel_background());
    }

    #[test]
    fn the_menu_label_names_the_scale() {
        assert_eq!(menu_label(None), "Scale off");
        assert_eq!(menu_label(Some(scale(6, ScaleMode::HarmonicMinor))), "Scale F# harmonic minor");
    }
}
