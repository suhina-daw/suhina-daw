// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The grid menu's entries, which the piano roll's toolbar, the Arrangement's toolbar and its lane
//! menus all show, and the key labels beside them.

use egui::Ui;

use crate::detail::piano_roll_grid::{FIXED_VALUES, GridMode, PianoGrid, Width};
use crate::kit::menu;

/// A grid menu's entries, all in the menu itself, compact: Snap to grid; the adaptive widths in
/// rows of three and the fixed values in two rows of five under headings; Narrow grid, Widen grid
/// and Triplet grid, drawn with the kit's menu rows (`kit::menu`), the widths and values as
/// toggles drawn alike ([`menu::choice`]). `keys` are the key labels of
/// snap, narrow, widen and triplet (empty for none). The chosen ones are filled; an entry
/// `enabled` refuses is grayed out. The piano roll's toolbar, the Arrangement's toolbar and its
/// menus show the same entries. Returns the grid picked.
pub fn grid_entries(
    ui: &mut Ui,
    grid: PianoGrid,
    keys: &[String; 4],
    enabled: &dyn Fn(&PianoGrid) -> bool,
) -> Option<PianoGrid> {
    let picked = std::cell::Cell::new(None);
    ui.set_min_width(220.0);
    let pick = |ui: &mut Ui, entry: menu::Entry<'_>, result: PianoGrid| {
        if entry.enabled(enabled(&result)).compact().show(ui).clicked() {
            picked.set(Some(result));
            ui.close();
        }
    };
    // A heading, then `cells` in rows of `columns`, each an equal share of the menu's width: a
    // short name, filled when chosen, the grid it picks and the name read out.
    let table = |ui: &mut Ui, title: &str, columns: usize, cells: Vec<(String, bool, PianoGrid, String)>| {
        menu::heading(ui, title);
        let gap = 2.0;
        // Every cell as wide as the widest, so egui measures the menu as wide as its widest line
        // will be drawn; once measured, an equal share of the menu when that is more.
        let widest = cells.iter().map(|(name, ..)| menu::Entry::new(name).compact().width(ui)).fold(0.0, f32::max);
        let share = (ui.available_width() - gap * (columns as f32 - 1.0)) / columns as f32;
        let width = if ui.is_sizing_pass() { widest } else { widest.max(share) };
        for line in cells.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (name, on, result, label) in line {
                    let cell = egui::vec2(width, ui.spacing().interact_size.y);
                    ui.allocate_ui(cell, |ui| {
                        // The whole cell, also while egui measures the menu (an entry asks then only
                        // for its own words' width).
                        ui.set_min_width(width);
                        // A toggle drawn as the menu's rows are, lit while chosen.
                        if menu::choice(ui, name, label, *on, enabled(result)).clicked() {
                            picked.set(Some(*result));
                            ui.close();
                        }
                    });
                }
            });
        }
    };
    pick(ui, menu::Entry::new("Snap to grid").check(grid.snap).key(Some(&keys[0])), grid.toggle_snap());
    menu::separator(ui);
    let widths = Width::ALL
        .into_iter()
        .map(|w| {
            let result = PianoGrid { mode: GridMode::Adaptive(w), ..grid };
            let label = format!("Adaptive {}", w.name().to_lowercase());
            (w.name().to_string(), grid.mode == GridMode::Adaptive(w), result, label)
        })
        .collect();
    table(ui, "Adaptive grid", 3, widths);
    // Picking a value turns snapping back on (the control bar's none is snap off).
    let values = FIXED_VALUES
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let result = PianoGrid { mode: GridMode::Fixed(i), snap: true, ..grid };
            let name = v.replace(" bars", " Bars").replace(" bar", " Bar");
            (name, grid.mode == GridMode::Fixed(i), result, v.to_string())
        })
        .collect();
    table(ui, "Fixed grid", 5, values);
    menu::separator(ui);
    pick(ui, menu::Entry::new("Narrow grid").key(Some(&keys[1])), grid.narrower());
    pick(ui, menu::Entry::new("Widen grid").key(Some(&keys[2])), grid.wider());
    menu::separator(ui);
    pick(ui, menu::Entry::new("Triplet grid").check(grid.triplet).key(Some(&keys[3])), grid.toggle_triplet());
    picked.get().filter(|p| *p != grid)
}

/// The key labels of `actions` (snap, narrow, widen, triplet) in `keymap`, empty for none.
pub fn grid_key_labels(keymap: &crate::settings::keymap::Keymap, actions: &[&str; 4]) -> [String; 4] {
    actions.map(|a| keymap.label(a).unwrap_or_default())
}

#[cfg(test)]
#[path = "grid_menu_tests.rs"]
mod tests;
