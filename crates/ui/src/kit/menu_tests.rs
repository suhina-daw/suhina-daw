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
    assert_eq!(
        harness.get_by_label("Copy").accesskit_node().data().keyboard_shortcut(),
        Some("Ctrl+C"),
        "read out with it"
    );
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

#[test]
fn a_toggle_reads_as_a_check_box() {
    let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(640.0, 480.0)).build_ui(|ui| {
        crate::kit::theme::install(ui.ctx());
        ui.button("Open").context_menu(|ui| {
            super::toggle(ui, "Legato", true);
        });
    });
    harness.run();
    harness.get_by_label("Open").click_secondary();
    harness.run();
    let legato = harness.get_by_label("Legato");
    assert_eq!(legato.accesskit_node().role(), egui::accesskit::Role::CheckBox);
    assert_eq!(legato.accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
}

/// A button whose right-click menu has a grayed choice named apart from its words, a check box
/// with a key, an entry with a value, a grayed submenu, a submenu a click inside keeps open and
/// a slider; what was clicked is kept, and the slider's value.
fn built() -> Harness<'static, (Vec<String>, f32)> {
    Harness::builder().allow_missing_glyphs().with_size(egui::vec2(640.0, 480.0)).build_ui_state(
        |ui, (picked, level): &mut (Vec<String>, f32)| {
            crate::kit::theme::install(ui.ctx());
            ui.button("Open").context_menu(|ui| {
                let mut click = |ui: &mut egui::Ui, entry: super::Entry<'_>, name: &str| {
                    if entry.show(ui).clicked() {
                        picked.push(name.to_string());
                    }
                };
                click(ui, super::Entry::new("Wood").chosen(false).enabled(false).named("Sound: Wood"), "Wood");
                click(ui, super::Entry::new("Snap").check(true).key(Some("Ctrl+4")), "Snap");
                click(ui, super::Entry::new("Mode").value("Draw"), "Mode");
                super::Entry::new("Recolor").enabled(false).submenu(ui, |ui| {
                    super::item(ui, "Red", false);
                });
                super::Entry::new("Meter").keep_open().submenu(ui, |ui| {
                    if super::item(ui, "4", false).clicked() {
                        picked.push("4".to_string());
                    }
                });
                super::Entry::new("Source").marked(true).submenu(ui, |ui| {
                    super::item(ui, "Clip", true);
                });
                super::slider(ui, "Depth", level, 0.0..=100.0, 1.0, |v| format!("{v:.0} %"));
            });
        },
        (Vec::new(), 50.0),
    )
}

#[test]
fn a_built_entry_reads_its_name_its_state_and_its_key() {
    let mut harness = built();
    harness.run();
    harness.get_by_label("Open").click_secondary();
    harness.run();
    let wood = harness.get_by_label("Sound: Wood");
    assert!(wood.accesskit_node().is_disabled(), "a grayed choice");
    assert_eq!(wood.accesskit_node().toggled(), Some(egui::accesskit::Toggled::False));
    let snap = harness.get_by_label("Snap");
    assert_eq!(snap.accesskit_node().role(), egui::accesskit::Role::CheckBox);
    assert_eq!(snap.accesskit_node().data().keyboard_shortcut(), Some("Ctrl+4"));
    let source = harness.get_by_label("Source").accesskit_node();
    assert_eq!(source.toggled(), Some(egui::accesskit::Toggled::True), "a marked submenu reads as chosen");
    let mode = harness.get_by_label("Mode");
    assert_eq!(mode.accesskit_node().data().keyboard_shortcut(), None, "a value is not a key");
    harness.get_by_label("Sound: Wood").click();
    harness.run();
    assert!(harness.state().0.is_empty(), "a grayed choice takes no click");
}

#[test]
fn a_grayed_submenu_does_not_open() {
    let mut harness = built();
    harness.run();
    harness.get_by_label("Open").click_secondary();
    harness.run();
    assert!(harness.get_by_label("Recolor").accesskit_node().is_disabled());
    harness.get_by_label("Recolor").hover();
    harness.run();
    assert!(harness.query_by_label("Red").is_none(), "its entries never show");
}

#[test]
fn a_submenu_kept_open_stays_open_after_a_click_inside() {
    let mut harness = built();
    harness.run();
    harness.get_by_label("Open").click_secondary();
    harness.run();
    harness.get_by_label("Meter").hover();
    harness.run();
    harness.get_by_label("4").click();
    harness.run();
    assert_eq!(harness.state().0, vec!["4".to_string()]);
    assert!(harness.query_by_label("4").is_some(), "the click did not close it");
}

/// A button whose menu (closed only by a click outside, as the automation lanes' are) holds a
/// slider; its value is kept.
fn slider() -> Harness<'static, f32> {
    slider_over(100.0, 1.0)
}

/// A menu with a slider from 0 to `end` in `step`s, at half way.
fn slider_over(end: f32, step: f32) -> Harness<'static, f32> {
    Harness::builder().allow_missing_glyphs().with_size(egui::vec2(640.0, 480.0)).build_ui_state(
        move |ui, level: &mut f32| {
            crate::kit::theme::install(ui.ctx());
            let response = ui.button("Open");
            egui::Popup::context_menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(
                |ui| {
                    super::entry(ui, "Loop selection", None, true);
                    super::slider(ui, "Depth", level, 0.0..=end, step, |v| format!("{v:.0} %"));
                },
            );
        },
        end / 2.0,
    )
}

#[test]
fn a_slider_in_a_menu_moves_and_keeps_the_menu_narrow() {
    let mut harness = slider();
    harness.run();
    harness.get_by_label("Open").click_secondary();
    harness.run();
    harness.run();
    // Every row spans the menu, so an entry's width is the menu's.
    let menu = harness.get_by_label("Loop selection").rect().width();
    assert!(menu < 320.0, "the window offers 640: {menu}");
    harness.get_by_label_contains("Depth").click();
    harness.run();
    harness.key_press(egui::Key::ArrowRight);
    harness.run();
    assert_eq!(*harness.state(), 51.0, "the click took the keys and the arrow moved it a step");
}

#[test]
fn a_slider_in_a_menu_lands_on_whole_steps() {
    let mut harness = slider();
    harness.run();
    harness.get_by_label("Open").click_secondary();
    harness.run();
    harness.run();
    // The rail's handle runs 4 points in from each end; a click 37.3 % along it.
    let rail = harness.get_by_label_contains("Depth").rect();
    let at = egui::pos2(rail.left() + 4.0 + 0.373 * (rail.width() - 8.0), rail.center().y);
    harness.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    harness.run();
    assert_eq!(*harness.state(), 37.0, "the click set the step it shows");
}

/// A click `along` the slider's rail (0 to 1) in an open menu.
fn click_rail(harness: &mut Harness<'static, f32>, along: f32) {
    harness.run();
    harness.get_by_label("Open").click_secondary();
    harness.run();
    harness.run();
    let rail = harness.get_by_label_contains("Depth").rect();
    let at = egui::pos2(rail.left() + 4.0 + along * (rail.width() - 8.0), rail.center().y);
    harness.event(egui::Event::PointerMoved(at));
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    harness.run();
}

#[test]
fn a_slider_in_a_menu_reaches_the_end_of_an_uneven_range() {
    // From 0 to 10 in steps of 3: the steps stop at 9, the end is still reachable.
    let mut harness = slider_over(10.0, 3.0);
    click_rail(&mut harness, 0.99);
    assert_eq!(*harness.state(), 10.0, "the click near the end set the end");
    let mut harness = slider_over(10.0, 3.0);
    click_rail(&mut harness, 0.4);
    assert_eq!(*harness.state(), 3.0, "a click inside the range still lands on a step");
}
