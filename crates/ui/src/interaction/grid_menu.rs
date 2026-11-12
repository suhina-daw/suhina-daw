// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The grid menu's entries, which the piano roll's toolbar, the Arrangement's toolbar and its lane
//! menus all show, and the key labels beside them.

use egui::Ui;

use crate::detail::piano_roll_grid::{FIXED_VALUES, GridMode, PianoGrid, Width};
use crate::kit::theme;

/// A grid menu's entries, all in the menu itself, compact: Snap to grid; the adaptive widths in
/// rows of three and the fixed values in two rows of five under small headings; Narrow grid,
/// Widen grid and Triplet grid. `keys` are the key labels of snap, narrow, widen and triplet
/// (empty for none). The chosen ones carry a tick; an entry `enabled` refuses is greyed out. The
/// piano roll's toolbar, the Arrangement's toolbar and its menus show the same entries. Returns
/// the grid picked.
pub fn grid_entries(
    ui: &mut Ui,
    grid: PianoGrid,
    keys: &[String; 4],
    enabled: &dyn Fn(&PianoGrid) -> bool,
) -> Option<PianoGrid> {
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mono = theme::mono_font(theme::TEXT_SMALL);
    let picked = std::cell::Cell::new(None);
    ui.set_min_width(220.0);
    // A tick before a chosen entry, the room for one before the others, so names line up.
    let ticked = |on: bool, name: &str| format!("{} {name}", if on { "\u{2713}" } else { "\u{2002}" });
    // One row: its text, its key at the right, a tick when `on` says so.
    let row = |ui: &mut Ui, name: &str, key: &str, on: Option<bool>, result: PianoGrid| {
        let allowed = enabled(&result);
        let text = egui::RichText::new(ticked(on.unwrap_or(false), name)).font(font.clone());
        let button = egui::Button::new(text)
            .frame(false)
            .shortcut_text(egui::RichText::new(key).font(mono.clone()).color(theme::text_hint()));
        let response = ui.add_enabled(allowed, button);
        let label = if key.is_empty() { name.to_string() } else { format!("{name} ({key})") };
        response.widget_info(|| match on {
            Some(on) => egui::WidgetInfo::selected(egui::Role::Button, allowed, on, &label),
            None => egui::WidgetInfo::labeled(egui::Role::Button, allowed, &label),
        });
        if response.clicked() {
            picked.set(Some(result));
            ui.close();
        }
    };
    // A heading, then `cells` in rows of `columns`: a short name, ticked when chosen, the grid
    // it picks and its accessible label.
    let table = |ui: &mut Ui, title: &str, columns: usize, cells: Vec<(String, bool, PianoGrid, String)>| {
        ui.label(egui::RichText::new(title).font(font.clone()).color(theme::text_hint()));
        egui::Grid::new(("grid-menu", title)).spacing(egui::vec2(10.0, 2.0)).show(ui, |ui| {
            for (index, (name, on, result, label)) in cells.into_iter().enumerate() {
                let allowed = enabled(&result);
                let text = egui::RichText::new(ticked(on, &name)).font(font.clone());
                let response = ui.add_enabled(allowed, egui::Button::new(text).frame(false));
                response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, allowed, on, &label));
                if response.clicked() {
                    picked.set(Some(result));
                    ui.close();
                }
                if (index + 1) % columns == 0 {
                    ui.end_row();
                }
            }
        });
    };
    row(ui, "Snap to grid", &keys[0], Some(grid.snap), grid.toggle_snap());
    ui.separator();
    let widths = Width::ALL
        .into_iter()
        .map(|w| {
            let result = PianoGrid { mode: GridMode::Adaptive(w), ..grid };
            let label = format!("Adaptive {}", w.name().to_lowercase());
            (w.name().to_string(), grid.mode == GridMode::Adaptive(w), result, label)
        })
        .collect();
    table(ui, "Adaptive grid:", 3, widths);
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
    table(ui, "Fixed grid:", 5, values);
    ui.separator();
    row(ui, "Narrow grid", &keys[1], None, grid.narrower());
    row(ui, "Widen grid", &keys[2], None, grid.wider());
    ui.separator();
    row(ui, "Triplet grid", &keys[3], Some(grid.triplet), grid.toggle_triplet());
    picked.get().filter(|p| *p != grid)
}

/// The key labels of `actions` (snap, narrow, widen, triplet) in `keymap`, empty for none.
pub fn grid_key_labels(keymap: &crate::settings::keymap::Keymap, actions: &[&str; 4]) -> [String; 4] {
    actions.map(|a| keymap.label(a).unwrap_or_default())
}
