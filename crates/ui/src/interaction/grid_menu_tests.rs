// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::detail::piano_roll_grid::PianoGrid;

/// A button whose right-click menu holds the grid's entries.
fn harness() -> Harness<'static> {
    Harness::builder().allow_missing_glyphs().with_size(egui::vec2(800.0, 600.0)).build_ui(|ui| {
        crate::kit::theme::install(ui.ctx());
        ui.button("Open").context_menu(|ui| {
            let keys = ["Ctrl+4".to_string(), "Ctrl+1".to_string(), "Ctrl+2".to_string(), "Ctrl+3".to_string()];
            super::grid_entries(ui, PianoGrid::default(), &keys, &|_| true);
        });
    })
}

/// The menu is drawn at the width egui measured from its first shown frame: no line of the
/// tables is wider than the menu, which would widen it a frame later (seen as a menu that opens
/// narrow and then grows).
#[test]
fn the_grid_menu_keeps_its_width_from_its_first_frame() {
    let mut harness = harness();
    harness.run();
    harness.get_by_label("Open").click_secondary();
    let mut widths = Vec::new();
    for _ in 0..6 {
        harness.step();
        if let Some(snap) = harness.query_by_label("Snap to grid") {
            widths.push(snap.rect().width());
        }
    }
    assert!(widths.len() > 1, "the menu showed");
    assert!(widths.windows(2).all(|w| (w[0] - w[1]).abs() < 0.5), "one width from the start: {widths:?}");
    let menu = harness.get_by_label("Snap to grid").rect();
    for cell in ["8 bars", "1/32", "Adaptive narrowest", "Adaptive widest"] {
        let rect = harness.get_by_label(cell).rect();
        assert!(rect.right() <= menu.right() + 0.5, "{cell} ends inside the menu: {rect:?} in {menu:?}");
    }
}
