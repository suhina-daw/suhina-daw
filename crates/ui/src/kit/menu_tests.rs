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

/// A button whose right-click menu has two entries with keys, the second grayed, and a note
/// longer than the menu is wide.
fn entries() -> Harness<'static, Vec<String>> {
    Harness::builder().allow_missing_glyphs().with_size(egui::vec2(640.0, 480.0)).build_ui_state(
        |ui, picked: &mut Vec<String>| {
            crate::kit::theme::install(ui.ctx());
            let response = ui.button("Open");
            response.context_menu(|ui| {
                for (name, key, enabled) in [("Copy", "Ctrl+C", true), ("Paste", "Ctrl+V", false)] {
                    if super::entry(ui, name, Some(key), enabled).clicked() {
                        picked.push(name.to_string());
                        ui.close();
                    }
                }
                super::note(ui, "Live MIDI is silent, and you can still record it from here.", Some(120.0));
            });
        },
        Vec::new(),
    )
}

#[test]
fn a_grayed_entry_takes_no_click_and_an_enabled_one_does() {
    let mut harness = entries();
    harness.run();
    harness.get_by_label("Open").click_secondary();
    harness.run();
    assert!(harness.get_by_label("Paste").accesskit_node().is_disabled(), "grayed while it does nothing");
    assert!(!harness.get_by_label("Copy").accesskit_node().is_disabled());
    harness.get_by_label("Paste").click();
    harness.run();
    assert!(harness.state().is_empty(), "a grayed entry does nothing");
    // A click anywhere in a menu closes it, egui's way: open it again.
    if harness.query_by_label("Copy").is_none() {
        harness.get_by_label("Open").click_secondary();
        harness.run();
    }
    harness.get_by_label("Copy").click();
    harness.run();
    assert_eq!(harness.state(), &vec!["Copy".to_string()]);
}

#[test]
fn a_note_wraps_to_the_width_it_is_given() {
    let mut harness = entries();
    harness.run();
    harness.get_by_label("Open").click_secondary();
    harness.run();
    let note = harness.get_by_label("Live MIDI is silent, and you can still record it from here.");
    let bounds = note.accesskit_node().raw_bounds().expect("the note is laid out");
    assert!(bounds.height() > crate::kit::theme::ROW as f64, "on more than one line: {}", bounds.height());
    assert!((bounds.width() - 120.0).abs() < 0.5, "as wide as given: {}", bounds.width());
}

#[test]
fn a_menu_is_as_wide_as_its_widest_entry_not_the_room_it_is_offered() {
    let mut harness = harness();
    harness.run();
    harness.get_by_label("Open").click_secondary();
    harness.run();
    harness.run();
    let widest = harness.get_by_label("Your own: a folder of sound files").rect().width();
    let entry = harness.get_by_label("Post-fader").rect().width();
    assert!(widest < 320.0, "the window offers 640: {widest}");
    assert!((entry - widest).abs() < 0.5, "every row across the menu: {entry} and {widest}");
}
