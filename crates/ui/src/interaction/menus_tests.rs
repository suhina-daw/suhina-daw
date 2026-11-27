// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// A track's menu is as wide as its widest row, a long MIDI input's here, and every row
/// reaches its right edge: none stops short at a width of its own.
#[test]
fn every_row_of_a_track_menu_reaches_its_edge() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    let input =
        MidiInput { port: MidiPort::Named("A very long name for a MIDI keyboard port".into()), ..MidiInput::default() };
    let ports = vec!["A very long name for a MIDI keyboard port".to_string()];
    let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(900.0, 700.0)).build_ui_state(
        |ui, actions: &mut Vec<Action>| {
            crate::kit::theme::install(ui.ctx());
            let menu = TrackMenu {
                track: TrackId(1),
                name: "Keys",
                color: Rgb(200, 100, 50),
                is_group: false,
                is_return: false,
                selection: &[],
                into: &[],
                out: None,
                midi: Some((&input, &ports)),
                audio: None,
                tracks_copied: false,
                key_readers: &[],
            };
            ui.button("Keys").context_menu(|ui| track_menu(ui, &menu, actions));
        },
        Vec::new(),
    );
    harness.run();
    harness.get_by_label("Keys").click_secondary();
    harness.run();
    harness.run();
    let long = harness.get_by_label_contains("MIDI input: A very long name").rect();
    let delete = harness.get_by_label("Delete track").rect();
    assert!(long.width() > 220.0, "wider than the old cap: {}", long.width());
    assert!((delete.right() - long.right()).abs() < 0.5, "{delete:?} reaches {long:?}");
}

/// The strip's dropdown and the track's menu both offer the same list, pairs included: with
/// no device open, the first eight channels, mono and one pair each, are there to set a
/// project with before the interface arrives.
#[test]
fn audio_input_picks_offer_monos_and_pairs() {
    let picks = audio_input_picks(&AudioInput::default(), 0);
    let labels: Vec<&str> = picks.iter().map(|(l, _)| l.as_str()).collect();
    assert!(labels.contains(&"In 1"), "{labels:?}");
    assert!(labels.contains(&"In 8"), "{labels:?}");
    assert!(labels.contains(&"In 1/2"), "{labels:?}");
    assert!(labels.contains(&"In 3/4"), "{labels:?}");
    let ticked = audio_is(&AudioInput::default(), &picks[0].1);
    assert!(ticked, "no device yet, so no input ticked");
    let monitors = audio_monitor_picks();
    assert_eq!(monitors.len(), 3);
}

#[test]
fn an_audio_clip_is_edited_with_the_ara_plug_in_and_can_stop() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    // Not edited yet: Edit with, which opens it there.
    let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(300.0, 120.0)).build_ui_state(
        |ui, actions: &mut Vec<Action>| ara_menu(ui, TrackId(2), ClipId(7), "Tune", false, actions),
        Vec::new(),
    );
    harness.run();
    assert!(harness.query_by_label_contains("Stop editing").is_none());
    harness.get_by_label_contains("Edit with Tune").click();
    harness.run();
    let action = &harness.state()[0];
    assert_eq!((action.id, action.params.clone()), ("view.araEditor", json!({ "track": 2, "clip": 7 })));
    drop(harness);
    // Edited: Open in, and Stop editing with, which takes the clip off it (#990).
    let mut harness = Harness::builder().allow_missing_glyphs().with_size(egui::vec2(300.0, 120.0)).build_ui_state(
        |ui, actions: &mut Vec<Action>| ara_menu(ui, TrackId(2), ClipId(7), "Tune", true, actions),
        Vec::new(),
    );
    harness.run();
    harness.get_by_label_contains("Open in Tune");
    harness.get_by_label_contains("Stop editing with Tune").click();
    harness.run();
    let action = &harness.state()[0];
    assert_eq!((action.id, action.params.clone()), ("clip.setAra", json!({ "clips": [{ "clip": 7 }], "on": false })));
}

#[test]
fn scene_fields_become_commands() {
    let action = |field, text| scene_field_action(field, 2, text).map(|a| (a.id, a.params));
    assert_eq!(action("name", " Drop "), Some(("scene.rename", json!({ "scene": 2, "name": "Drop" }))));
    assert_eq!(action("tempo", "128"), Some(("scene.setTempo", json!({ "scene": 2, "bpm": 128.0 }))));
    assert_eq!(action("tempo", ""), Some(("scene.setTempo", json!({ "scene": 2 }))));
    assert_eq!(
        action("signature", "7/8"),
        Some(("scene.setTimeSignature", json!({ "scene": 2, "numerator": 7, "denominator": 8 })))
    );
    assert_eq!(action("signature", " "), Some(("scene.setTimeSignature", json!({ "scene": 2 }))));
    // Not a number: sent as text, so the command's error says what is wrong.
    assert_eq!(action("tempo", "fast"), Some(("scene.setTempo", json!({ "scene": 2, "bpm": "fast" }))));
    assert_eq!(action("color", "red"), None);
}

/// A time signature from a menu: the start signature at bar 1, a change at a later bar
/// (#1259, #1264); typed text that is not a signature goes to the command as it is.
#[test]
fn a_time_signature_becomes_the_command_for_its_bar() {
    let four = TimeSignature { numerator: 4, denominator: 4 };
    let typed =
        |bar, text| typed_time_signature_action(bar, four, SameSignature::Nothing, text).map(|a| (a.id, a.params));
    assert_eq!(typed(1, " 6/8 "), Some(("transport.setTimeSignature", json!({ "numerator": 6, "denominator": 8 }))));
    assert_eq!(
        typed(5, "15 / 16"),
        Some(("timeSignature.add", json!({ "bar": 5, "numerator": 15, "denominator": 16 })))
    );
    assert_eq!(
        typed(1, "waltz"),
        Some(("transport.setTimeSignature", json!({ "numerator": "waltz", "denominator": "" })))
    );
    assert_eq!(typed(3, "  "), None);
    let picked = time_signature_action(0, json!(3), json!(4));
    assert_eq!((picked.id, picked.params), ("transport.setTimeSignature", json!({ "numerator": 3, "denominator": 4 })));
}

/// The signature already in effect: nothing where it is set, a change of its own at a bar
/// that only inherits it, and the tempo map again while a launched scene's drives the clock.
#[test]
fn the_signature_in_effect_does_what_its_place_needs() {
    let four = TimeSignature { numerator: 4, denominator: 4 };
    let choose = |bar, same| signature_choice(bar, four, same, json!(4), json!(4)).map(|a| (a.id, a.params));
    assert_eq!(choose(1, SameSignature::Nothing), None);
    assert_eq!(
        choose(9, SameSignature::Set),
        Some(("timeSignature.add", json!({ "bar": 9, "numerator": 4, "denominator": 4 })))
    );
    assert_eq!(choose(1, SameSignature::FollowMap), Some(("transport.followTempoMap", json!({}))));
    assert_eq!(
        typed_time_signature_action(9, four, SameSignature::Set, "4/4").map(|a| a.id),
        Some("timeSignature.add")
    );
    // Another signature is set whatever `same` says.
    assert_eq!(
        signature_choice(1, four, SameSignature::FollowMap, json!(3), json!(4)).map(|a| a.id),
        Some("transport.setTimeSignature")
    );
}
