// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_engine::model::Rgb;

#[test]
fn f2_opens_a_nonmodal_picker_that_follows_tracks_and_reuses_recent_colours() {
    let mut h = window(demo_app());
    h.get_by_label("Track Drums").click();
    h.run();
    h.key_press(Key::F2);
    h.run();
    assert!(h.query_by_label("Hex colour").is_some());
    let colour = suhina_ui::interaction::colour_picker::swatches()[7];
    let value = suhina_ui::interaction::colour_picker::hex(colour);
    h.get_by_label(&format!("Palette {value}")).click();
    h.run();
    assert_eq!(h.state().session().unwrap().project().track(TrackId(1)).unwrap().color, Some(colour));
    assert_eq!(h.state().settings().recent_colours, [colour]);
    // The underlying DAW accepts a new selection without closing the tool window.
    h.get_by_label("Track Bass").click();
    h.run();
    h.get_by_label(&format!("Recent colour {value}")).click();
    h.run();
    assert_eq!(h.state().session().unwrap().project().track(TrackId(2)).unwrap().color, Some(colour));
    assert_eq!(h.state().settings().recent_colours, [colour]);
    assert_eq!(h.state().session().unwrap().undo_steps().0, 2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_ne!(h.state().session().unwrap().project().track(TrackId(2)).unwrap().color, Some(colour));
    assert!(h.query_by_label("Hex colour").is_some());
}

#[test]
fn hex_entry_is_exact_and_invalid_input_cannot_recolour_the_track() {
    let mut app = demo_app();
    app.run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    h.get_by_label("Hex colour").click();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(egui::Event::Text("#2577e7".into()));
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().session().unwrap().project().track(TrackId(1)).unwrap().color, Some(Rgb(37, 119, 231)));
    let depth = h.state().session().unwrap().undo_steps();
    h.get_by_label("Hex colour").click();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(egui::Event::Text("#12zz".into()));
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().session().unwrap().undo_steps(), depth);
    assert_eq!(h.state().session().unwrap().project().track(TrackId(1)).unwrap().color, Some(Rgb(37, 119, 231)));
    h.key_press(Key::Escape);
    h.run();
    assert!(h.query_by_label("Hex colour").is_some());
    h.get_by_label("Default").click();
    h.run();
    assert!(h.state().session().unwrap().project().track(TrackId(1)).unwrap().color.is_none());
    assert_eq!(h.state().settings().recent_colours, [Rgb(37, 119, 231)]);
}

#[test]
fn escape_discards_a_valid_text_draft_instead_of_committing_on_focus_loss() {
    let mut app = demo_app();
    app.run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    let before = h.state().session().unwrap().project().clone();
    h.get_by_label("Hex colour").click();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(egui::Event::Text("#2577e7".into()));
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().session().unwrap().project(), &before);
    assert!(h.state().settings().recent_colours.is_empty());
    h.get_by_label("› Fine adjustment").click();
    h.run();
    h.get_by_label("Red").click();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(egui::Event::Text("17".into()));
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().session().unwrap().project(), &before);
}

#[test]
fn a_multitrack_choice_is_one_undo_and_project_switch_closes_the_picker() {
    let mut h = window(demo_app());
    let before = h.state().session().unwrap().project().clone();
    h.get_by_label("Track Drums").click();
    h.run();
    h.get_by_label("Track Bass").click_modifiers(Modifiers::COMMAND);
    h.run();
    h.key_press(Key::F2);
    h.run();
    h.get_by_label("Palette #2fd9c4").click();
    h.run();
    for id in [TrackId(1), TrackId(2)] {
        assert_eq!(h.state().session().unwrap().project().track(id).unwrap().color, Some(Rgb(47, 217, 196)));
    }
    assert_eq!(h.state().session().unwrap().undo_steps().0, 1);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(h.state().session().unwrap().project(), &before);
    h.state_mut().open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    h.run();
    assert!(h.query_by_label("Hex colour").is_none());
}

#[test]
fn escape_stops_a_live_drag_until_release_without_undoing_an_intervening_edit() {
    for intervening_edit in [false, true] {
        let mut app = demo_app();
        app.run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
        let mut h = window(app);
        let before = h.state().session().unwrap().project().track(TrackId(1)).unwrap().color;
        h.get_by_label("› Fine adjustment").click();
        h.run();
        let field = h.get_by_label("Colour field").rect();
        let from = field.left_top() + field.size() * 0.3;
        let to = field.left_top() + field.size() * 0.7;
        h.hover_at(from);
        h.step();
        h.drag_at(from);
        h.step();
        h.hover_at(to);
        h.step();
        let dragged = h.state().session().unwrap().project().track(TrackId(1)).unwrap().color;
        assert_ne!(dragged, before);
        if intervening_edit {
            h.state_mut().run(&Action::new("track.rename", json!({"track":2,"name":"Keep this edit"})));
        }
        h.key_press(Key::Escape);
        h.step();
        h.hover_at(from);
        h.step();
        h.drop_at(from);
        h.run();
        let session = h.state().session().unwrap();
        if intervening_edit {
            assert_eq!(session.project().track(TrackId(2)).unwrap().name, "Keep this edit");
            assert_eq!(session.project().track(TrackId(1)).unwrap().color, dragged);
            assert_eq!(session.undo_steps().0, 2);
        } else {
            assert_eq!(session.project().track(TrackId(1)).unwrap().color, before);
            assert_eq!(session.undo_steps().0, 0);
            assert!(h.state().settings().recent_colours.is_empty());
        }
    }
}

#[test]
fn deleting_the_last_target_disables_the_open_picker() {
    let mut app = demo_app();
    app.run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    h.state_mut().run(&Action::new("track.delete", json!({"track":1})));
    h.run();
    let before = h.state().session().unwrap().project().clone();
    h.get_by_label("Palette #2fd9c4").click();
    h.run();
    assert_eq!(h.state().session().unwrap().project(), &before);
    assert!(h.query_by_label("Hex colour").is_some());
}

#[test]
fn closing_keeps_the_applied_colour_and_reopening_preserves_exact_values() {
    let mut app = demo_app();
    app.run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
    let mut h = window(app);
    h.get_by_label("Palette #2fd9c4").click();
    h.run();
    let chosen = h.state().session().unwrap().project().clone();
    h.get_by_label("Close window").click();
    h.run();
    assert!(h.query_by_label("Hex colour").is_none());
    assert_eq!(h.state().session().unwrap().project(), &chosen);
    h.state_mut().run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
    h.run();
    h.get_by_label("› Fine adjustment").click();
    h.run();
    h.get_by_label("Wheel").click();
    h.run();
    assert_eq!(h.state().session().unwrap().project(), &chosen);
    assert_eq!(h.state().session().unwrap().undo_steps().0, 1);
    assert_eq!(h.state().settings().recent_colours, [Rgb(47, 217, 196)]);
}
