// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

/// A button whose right-click menu has a heading, two entries (the first chosen), a line, a
/// submenu with one entry and a note; what was clicked is kept.
fn harness() -> Harness<'static, Vec<String>> {
    Harness::builder().allow_missing_glyphs().with_size(egui::vec2(640.0, 480.0)).build_ui_state(
        |ui, picked: &mut Vec<String>| {
            crate::kit::theme::install(ui.ctx());
            let response = ui.button("Open");
            response.context_menu(|ui| {
                super::heading(ui, "Plays");
                for (name, chosen) in [("Pre-fader", true), ("Post-fader", false)] {
                    if super::item(ui, name, chosen).clicked() {
                        picked.push(name.to_string());
                        ui.close();
                    }
                }
                super::separator(ui);
                super::submenu(ui, "Sound", true, |ui| {
                    if super::item(ui, "Wood", false).clicked() {
                        picked.push("Wood".to_string());
                        ui.close();
                    }
                });
                super::hint(ui, "Your own: a folder of sound files");
            });
        },
        Vec::new(),
    )
}

#[test]
fn an_entry_reports_its_click_and_its_choice() {
    let mut harness = harness();
    harness.run();
    harness.get_by_label("Open").click_secondary();
    harness.run();
    harness.get_by_label("Plays");
    assert_eq!(
        harness.get_by_label("Pre-fader").accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True),
        "the chosen entry"
    );
    assert_eq!(harness.get_by_label("Post-fader").accesskit_node().toggled(), Some(egui::accesskit::Toggled::False));
    harness.get_by_label("Your own: a folder of sound files");
    harness.get_by_label("Post-fader").click();
    harness.run();
    assert_eq!(harness.state(), &vec!["Post-fader".to_string()]);
    assert!(harness.query_by_label("Pre-fader").is_none(), "the click closed the menu");
}

#[test]
fn a_submenu_opens_under_the_pointer_and_its_entry_clicks() {
    let mut harness = harness();
    harness.run();
    harness.get_by_label("Open").click_secondary();
    harness.run();
    assert!(harness.query_by_label("Wood").is_none(), "closed until pointed at");
    harness.get_by_label("Sound").hover();
    harness.run();
    harness.get_by_label("Wood").click();
    harness.run();
    assert_eq!(harness.state(), &vec!["Wood".to_string()]);
}
