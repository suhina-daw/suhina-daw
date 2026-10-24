// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn shortcuts_read_and_write_the_same() {
    for text in [
        "Ctrl+Shift+Z",
        "Space",
        "Delete",
        "Ctrl+Comma",
        "Alt+F4",
        "Ctrl+Tab",
        "Ctrl+Plus",
        "Ctrl+Minus",
        "Ctrl+Equals",
        "Ctrl+0",
        "Minus",
        "Enter",
        "Up",
        "Escape",
        "Shift+Up",
        "Left",
        "Ctrl+A",
    ] {
        let shortcut = Shortcut::parse(text).unwrap_or_else(|| panic!("{text}"));
        assert_eq!(Shortcut::parse(&shortcut.text()), Some(shortcut), "{text}");
    }
    assert_eq!(Shortcut::parse("ctrl+z"), Shortcut::parse("Ctrl+Z"));
    assert!(Shortcut::parse("Hyper+Z").is_none());
    assert!(Shortcut::parse("Ctrl+Nonsense").is_none());
    assert_eq!(Shortcut::parse("Ctrl+0"), Some(Shortcut::new(Modifiers::COMMAND, Key::Num0)));
    assert_eq!(Shortcut::parse("Ctrl++"), Some(Shortcut::new(Modifiers::COMMAND, Key::Plus)));
}

#[test]
fn a_keymap_file_overrides_the_defaults_and_warns_about_the_rest() {
    let (keymap, warnings) = Keymap::from_json(
        r#"{ "edit.undo": ["Alt+Z"], "selection.delete": [], "dance": ["X"], "edit.redo": ["Ctrl+Bogus"] }"#,
    );
    assert_eq!(keymap.binding_text("edit.undo").as_deref(), Some("Alt+Z"));
    assert!(keymap.keys("selection.delete").is_empty());
    assert_eq!(keymap.binding_text("transport.toggle").as_deref(), Some("Space"));
    assert_eq!(warnings.len(), 2);
    let (again, none) = Keymap::from_json(&keymap.to_json());
    assert_eq!((again, none.len()), (keymap, 0));
}

#[test]
fn every_default_is_an_action_and_there_are_no_conflicts() {
    let known = actions();
    for (id, _) in DEFAULTS {
        assert!(known.iter().any(|a| a.id == *id), "{id}");
    }
    assert!(Keymap::default().conflicts().is_empty());
    for id in ["view.zoomIn", "view.zoomOut", "view.zoomReset"] {
        assert!(!Keymap::default().keys(id).is_empty(), "{id} has a key");
    }
    assert!(known.iter().any(|a| a.id == "transport.play"), "commands without parameters are actions");
    assert!(!known.iter().any(|a| a.id == "track.setVolume"), "commands that need parameters are not");
    // The piano roll's tools have their number keys and their own labels.
    for (index, id) in [
        "pianoRoll.tool1",
        "pianoRoll.tool2",
        "pianoRoll.tool3",
        "pianoRoll.tool4",
        "pianoRoll.tool5",
        "pianoRoll.tool6",
    ]
    .into_iter()
    .enumerate()
    {
        let action = known.iter().find(|a| a.id == id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!(action.group, PIANO_ROLL);
        assert!(piano_roll_only(id), "{id} is read only while the piano roll has the keyboard");
        let number = (index + 1).to_string();
        assert_eq!(Keymap::default().binding_text(id).as_deref(), Some(number.as_str()), "{id}");
    }
}

#[test]
fn the_grid_keys_are_ctrl_and_one_to_four_in_the_piano_roll() {
    let known = actions();
    for (id, key) in [
        ("pianoRoll.gridNarrow", "Ctrl+1"),
        ("pianoRoll.gridWiden", "Ctrl+2"),
        ("pianoRoll.gridTriplet", "Ctrl+3"),
        ("pianoRoll.gridSnap", "Ctrl+4"),
    ] {
        let action = known.iter().find(|a| a.id == id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!(action.group, PIANO_ROLL);
        assert!(piano_roll_only(id), "{id} is read only while the piano roll has the keyboard");
        assert_eq!(Keymap::default().binding_text(id).as_deref(), Some(key), "{id}");
    }
    assert!(Keymap::default().conflicts().is_empty());
}

#[test]
fn z_and_shift_z_zoom_the_piano_roll() {
    let known = actions();
    for (id, key) in [("pianoRoll.zoomSelection", "Z"), ("pianoRoll.zoomClip", "Shift+Z")] {
        let action = known.iter().find(|a| a.id == id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!(action.group, PIANO_ROLL);
        assert!(piano_roll_only(id), "{id} is read only while the piano roll has the keyboard");
        assert_eq!(Keymap::default().binding_text(id).as_deref(), Some(key), "{id}");
    }
    assert!(Keymap::default().conflicts().is_empty());
}

#[test]
fn the_sixth_tool_is_the_mute_tool_on_the_six_key() {
    let known = actions();
    let action =
        known.iter().find(|a| a.id == "pianoRoll.tool6").unwrap_or_else(|| panic!("the Mute tool is an action"));
    assert_eq!((action.label.as_str(), action.group), ("Choose the Mute tool", PIANO_ROLL));
    assert!(piano_roll_only("pianoRoll.tool6"), "read only while the piano roll has the keyboard");
    assert_eq!(Keymap::default().binding_text("pianoRoll.tool6").as_deref(), Some("6"));
}

#[test]
fn the_presets_set_their_keys_and_are_kept_in_the_file() {
    for preset in Preset::ALL {
        let mut keymap = Keymap::default();
        keymap.set_preset(preset);
        assert_eq!(keymap.preset(), preset);
        assert!(keymap.conflicts().is_empty(), "{preset:?} has no key bound twice");
        assert_eq!(keymap.binding_text("selection.duplicate").as_deref(), Some(preset.duplicate_key()));
        assert_eq!(preset.right_click_deletes(), matches!(preset, Preset::DrawFirst | Preset::MouseFirst));
        assert_eq!(keymap.mouse(), preset.mouse(), "{preset:?} sets its mouse switches");
        // Every other key is the default's: the preset only changes what it names.
        for (id, _) in DEFAULTS {
            if id != "selection.duplicate" && id != "view.browser" {
                assert_eq!(keymap.keys(id), Keymap::default().keys(id), "{id} under {preset:?}");
            }
        }
        assert!(!keymap.keys("view.browser").is_empty(), "{preset:?} leaves the browser a key");
        // The choice comes back from the file.
        let (again, warnings) = Keymap::from_json(&keymap.to_json());
        assert_eq!((&again, warnings.len()), (&keymap, 0));
        assert_eq!(again.preset(), preset);
    }
}

#[test]
fn the_three_presets_and_their_mouse_switches_round_trip_through_the_file() {
    for preset in Preset::ALL {
        let mut keymap = Keymap::default();
        keymap.set_preset(preset);
        let text = keymap.to_json();
        assert!(text.contains(&format!("\"preset\": \"{}\"", preset.name())), "{text}");
        for switch in MouseSwitch::ALL {
            assert!(text.contains(switch.json_key()), "{} is in the file", switch.name());
        }
        let (again, warnings) = Keymap::from_json(&text);
        assert_eq!((again.mouse(), warnings.len()), (preset.mouse(), 0), "{preset:?}");
        assert_eq!(again, keymap, "{preset:?}");
    }
}

#[test]
fn mouse_first_turns_the_switches_on_and_the_other_presets_off() {
    let mut keymap = Keymap::default();
    assert_eq!(keymap.mouse(), MouseSettings::default(), "the default keymap leaves them off");
    for preset in [Preset::SelectFirst, Preset::DrawFirst] {
        keymap.set_mouse(MouseSwitch::RightDeletes, true);
        keymap.set_preset(preset);
        assert_eq!(keymap.mouse(), MouseSettings::default(), "{preset:?} turns every switch off");
        assert!(!keymap.mouse().all_on());
    }
    keymap.set_preset(Preset::MouseFirst);
    for switch in MouseSwitch::ALL {
        assert!(keymap.mouse().get(switch), "{} is on with Mouse-first", switch.name());
    }
    assert!(keymap.mouse().all_on());
    assert!(keymap.preset().right_click_deletes(), "and a right-click on a note deletes it");
}

#[test]
fn the_mouse_switches_are_read_from_the_file_and_nonsense_is_warned_about() {
    let (keymap, warnings) = Keymap::from_json(r#"{ "mouseRightDeletes": true, "mouseLeftDraws": false }"#);
    assert_eq!((keymap.mouse().right_deletes, keymap.mouse().left_draws), (true, false));
    assert!(warnings.is_empty(), "{warnings:?}");
    // A file without them takes all four off.
    let (none, warnings) = Keymap::from_json(r#"{ "edit.undo": ["Alt+Z"] }"#);
    assert_eq!((none.mouse(), warnings.len()), (MouseSettings::default(), 0));
    assert_eq!(none.binding_text("edit.undo").as_deref(), Some("Alt+Z"));
    // A value that is not true or false is skipped with a warning, as a bad key is.
    let (bad, warnings) = Keymap::from_json(r#"{ "mouseRightDeletes": "yes" }"#);
    assert_eq!((bad.mouse().right_deletes, warnings.len()), (false, 1));
    assert!(warnings[0].contains("mouseRightDeletes"), "{}", warnings[0]);
    // A mouse entry that is not one of the four is skipped, with a warning naming it.
    let (other, warnings) = Keymap::from_json(r#"{ "mouseMiddlePans": true }"#);
    assert_eq!((other.mouse(), warnings.len()), (MouseSettings::default(), 1));
    assert!(warnings[0].contains("mouseMiddlePans"), "{}", warnings[0]);
}

#[test]
fn a_mouse_switch_changed_by_hand_makes_the_mode_custom_and_keeps_the_others() {
    let mut keymap = Keymap::default();
    keymap.set_preset(Preset::MouseFirst);
    keymap.set_mouse(MouseSwitch::RightDeletes, false);
    assert_eq!(keymap.preset(), Preset::Custom, "a setting changed by hand is Custom");
    assert!(!keymap.mouse().right_deletes);
    for switch in MouseSwitch::ALL {
        if switch != MouseSwitch::RightDeletes {
            assert!(keymap.mouse().get(switch), "{} stays on", switch.name());
        }
    }
    // What was turned off by hand goes through the file as it is.
    let (again, warnings) = Keymap::from_json(&keymap.to_json());
    assert_eq!((again.preset(), again.mouse().right_deletes, warnings.len()), (Preset::Custom, false, 0));
    assert!(again.mouse().left_draws, "the other three are still on");
    // Choosing Mouse-first again turns it back on.
    keymap.set_preset(Preset::MouseFirst);
    assert!(keymap.mouse().all_on());
    assert_eq!(keymap.preset(), Preset::MouseFirst);
}

#[test]
fn an_earlier_file_with_a_switch_off_by_hand_opens_as_custom() {
    // A file from before the workflow settings: Mouse-first's name with one switch off.
    let text = r#"{ "preset": "Mouse-first", "mouseLeftDraws": true, "mouseRightDeletes": false,
            "mouseShiftRightSplits": true, "mouseAltRightPlays": true }"#;
    let (keymap, warnings) = Keymap::from_json(text);
    assert_eq!((keymap.preset(), warnings.len()), (Preset::Custom, 0));
    assert!(keymap.mouse().left_draws && !keymap.mouse().right_deletes);
    // Draw-first's file of before: its switches all off, its right-click still deletes in
    // Draw and Paint, and it stays Draw-first.
    let mut draw_first = Keymap::default();
    draw_first.set_preset(Preset::DrawFirst);
    let mut map: serde_json::Map<String, Value> = serde_json::from_str(&draw_first.to_json()).unwrap();
    map.retain(|k, _| !Setting::is_json_key(k));
    let (old, warnings) = Keymap::from_json(&Value::Object(map).to_string());
    assert_eq!((old.preset(), warnings.len()), (Preset::DrawFirst, 0));
    assert!(old.workflow().right_click_deletes(true));
}

#[test]
fn a_workflow_setting_makes_the_mode_custom_and_custom_keeps_the_settings() {
    let mut keymap = Keymap::default();
    keymap.set_preset(Preset::DrawFirst);
    keymap.set_workflow(Setting::DoubleClickNote, 1);
    assert_eq!(keymap.preset(), Preset::Custom);
    assert!(!keymap.workflow().double_click_deletes);
    assert_eq!(keymap.binding_text("selection.duplicate").as_deref(), Some("Ctrl+B"), "Draw-first's key stays");
    // The duplicate setting binds the key, and gives the browser its own back.
    keymap.set_workflow(Setting::Duplicate, 0);
    assert_eq!(keymap.binding_text("selection.duplicate").as_deref(), Some("Ctrl+D"));
    assert_eq!(keymap.binding_text("view.browser").as_deref(), Some("Ctrl+B"));
    assert!(keymap.conflicts().is_empty());
    // Choosing Custom changes no setting; a preset fills them all in again.
    let before = keymap.workflow();
    keymap.set_preset(Preset::Custom);
    assert_eq!(keymap.workflow(), before);
    let (again, warnings) = Keymap::from_json(&keymap.to_json());
    assert_eq!((&again, warnings.len()), (&keymap, 0), "Custom and its settings go through the file");
    keymap.set_preset(Preset::SelectFirst);
    assert_eq!(keymap.workflow(), Workflow::SELECT_FIRST);
    // A choice that is already set changes nothing, not even the mode.
    keymap.set_workflow(Setting::EmptyClick, 0);
    assert_eq!(keymap.preset(), Preset::SelectFirst);
}

#[test]
fn a_preset_chosen_again_puts_keys_changed_by_hand_back() {
    let mut keymap = Keymap::default();
    keymap.set_preset(Preset::DrawFirst);
    keymap.set("selection.duplicate", vec![Shortcut::new(Modifiers::COMMAND, Key::D)]);
    keymap.set_preset(Preset::DrawFirst);
    assert_eq!(keymap.binding_text("selection.duplicate").as_deref(), Some("Ctrl+B"));
    // And choosing the other preset sets its own keys.
    keymap.set_preset(Preset::SelectFirst);
    assert_eq!(keymap.binding_text("selection.duplicate").as_deref(), Some("Ctrl+D"));
    assert_eq!(keymap.binding_text("view.browser").as_deref(), Some("Ctrl+B"));
    // The mouse switches come back the same way: choosing the preset again puts a switch
    // changed by hand back, and another preset turns every one of them off.
    keymap.set_preset(Preset::MouseFirst);
    keymap.set_mouse(MouseSwitch::LeftDraws, false);
    keymap.set_preset(Preset::MouseFirst);
    assert!(keymap.mouse().all_on(), "a switch changed by hand comes back with the preset");
    keymap.set_preset(Preset::SelectFirst);
    assert_eq!(keymap.mouse(), MouseSettings::default());
}

#[test]
fn an_unknown_or_malformed_preset_in_the_file_falls_back_with_a_warning() {
    let (keymap, warnings) = Keymap::from_json(r#"{ "preset": "Fancy-first", "edit.undo": ["Alt+Z"] }"#);
    assert_eq!(keymap.preset(), Preset::SelectFirst, "an unknown preset is the default one");
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("Fancy-first"), "{}", warnings[0]);
    assert_eq!(keymap.binding_text("edit.undo").as_deref(), Some("Alt+Z"), "the rest of the file is read");
    // A preset that is not text at all, and a file that is not JSON.
    let (number, warnings) = Keymap::from_json(r#"{ "preset": 3 }"#);
    assert_eq!((number.preset(), warnings.len()), (Preset::SelectFirst, 1));
    let (broken, warnings) = Keymap::from_json("{ not json");
    assert_eq!((broken.preset(), warnings.len()), (Preset::SelectFirst, 1));
    // The name is read without case, as a hand-edited file may write it.
    let (named, warnings) = Keymap::from_json(r#"{ "preset": "draw-first" }"#);
    assert_eq!((named.preset(), warnings.len()), (Preset::DrawFirst, 0));
}

#[test]
fn conflicts_are_found() {
    let mut keymap = Keymap::default();
    keymap.set("transport.play", vec![Shortcut::new(Modifiers::NONE, Key::Space)]);
    let conflicts = keymap.conflicts();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].1.len(), 2);
    // The selected slot and the piano roll share the arrows: they never work at once.
    assert_eq!(Keymap::default().keys("selection.up"), Keymap::default().keys("pianoRoll.up"));
    assert!(!overlap("selection.up", "pianoRoll.up"));
    assert!(overlap("selection.up", "transport.play"));
}

#[test]
fn the_clip_devices_switch_has_its_own_key_under_every_preset() {
    let action =
        actions().into_iter().find(|a| a.id == "view.devices").unwrap_or_else(|| panic!("the switch is an action"));
    assert_eq!(action.group, "View");
    assert_eq!(Keymap::default().binding_text("view.devices").as_deref(), Some("Ctrl+Shift+D"));
    for preset in Preset::ALL {
        let mut keymap = Keymap::default();
        keymap.set_preset(preset);
        assert_eq!(keymap.binding_text("view.devices").as_deref(), Some("Ctrl+Shift+D"), "{preset:?}");
        assert!(keymap.conflicts().is_empty(), "{preset:?}");
    }
    // Rebound and unbound like any other action, and read back from the file.
    let (keymap, warnings) = Keymap::from_json(r#"{ "view.devices": [] }"#);
    assert_eq!((keymap.keys("view.devices").len(), warnings.len()), (0, 0));
}

#[test]
fn the_plugin_toggle_has_its_own_key_under_every_preset() {
    let action =
        actions().into_iter().find(|a| a.id == "view.plugin").unwrap_or_else(|| panic!("the toggle is an action"));
    assert_eq!(action.group, "View");
    assert_eq!(Keymap::default().binding_text("view.plugin").as_deref(), Some("P"));
    for preset in Preset::ALL {
        let mut keymap = Keymap::default();
        keymap.set_preset(preset);
        assert_eq!(keymap.binding_text("view.plugin").as_deref(), Some("P"), "{preset:?}");
        assert!(keymap.conflicts().is_empty(), "{preset:?}");
    }
    // Mappable like any other: unbound in the file, it stays unbound.
    let (keymap, warnings) = Keymap::from_json(r#"{ "view.plugin": [] }"#);
    assert_eq!((keymap.keys("view.plugin").len(), warnings.len()), (0, 0));
}
