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
    assert_eq!(h.state().session().unwrap().undo_steps(), 2);
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
    h.type_text("#2577e7");
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(h.state().session().unwrap().project().track(TrackId(1)).unwrap().color, Some(Rgb(37, 119, 231)));
    let depth = h.state().session().unwrap().undo_steps();
    h.get_by_label("Hex colour").click();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.type_text("#12zz");
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
