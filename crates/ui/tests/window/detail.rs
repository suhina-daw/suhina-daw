// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The detail area: devices, their selection and removal, and the piano roll a clip opens.

use super::*;

#[test]
fn a_devices_menu_removes_it_and_undo_brings_it_back() {
    let device_count =
        |h: &Harness<'static, App>| h.state().session().map(|s| s.project().channels[0].devices.len()).unwrap_or(0);
    let mut app = App::new();
    app.set_session(Session::new(project_with_a_device()), &[]);
    let mut harness = window(app);
    harness.get_by_label("Track Drums").click();
    harness.run();
    assert!(harness.query_by_label("Device Utility").is_some());
    // The panel's right-click menu: Remove device takes it off the track.
    harness.get_by_label("Device Utility").click_secondary();
    harness.run();
    harness.get_by_label("Remove device").click();
    harness.run();
    assert_eq!(device_count(&harness), 0, "the device is gone");
    assert!(harness.query_by_label("Device Utility").is_none());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(device_count(&harness), 1, "the device is back");
    assert!(harness.query_by_label("Device Utility").is_some());
    // A click selects the device, and the Delete key takes it off the track the same way.
    harness.get_by_label("Device Utility").click();
    harness.run();
    harness.key_press(Key::Delete);
    harness.run();
    assert_eq!(device_count(&harness), 0, "the selected device is gone");
    assert!(harness.query_by_label("Device Utility").is_none());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(device_count(&harness), 1, "the device is back");
}

#[test]
fn clicking_a_second_device_moves_the_selection_so_delete_takes_it() {
    use suhina_engine::model::{Device, DeviceKind, DeviceParam, Extensions};
    let mut project = project_with_a_device();
    project.channels[0].devices.push(Device {
        name: "Utility 2".into(),
        kind: DeviceKind::Builtin("utility".into()),
        enabled: true,
        state: None,
        params: vec![DeviceParam::new("gain", -3.5)],
        automation: Vec::new(),
        extensions: Extensions::default(),
    });
    let mut app = App::new();
    app.set_session(Session::new(project), &[]);
    let mut harness = window(app);
    harness.get_by_label("Track Drums").click();
    harness.run();
    // Click the FIRST device, then the second: the selection moves to the second, so the
    // Delete key takes the second off the track and the first stays.
    harness.get_by_label("Device Utility").click();
    harness.run();
    harness.get_by_label("Device Utility 2").click();
    harness.run();
    harness.key_press(Key::Delete);
    harness.run();
    let devices = |h: &Harness<'static, App>| {
        h.state()
            .session()
            .map(|s| s.project().channels[0].devices.iter().map(|d| d.name.clone()).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    assert_eq!(devices(&harness), ["Utility"], "the SECOND device went, not the first");
    // And clicking the first again moves the selection back.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    harness.get_by_label("Device Utility").click();
    harness.run();
    harness.key_press(Key::Delete);
    harness.run();
    assert_eq!(devices(&harness), ["Utility 2"], "now the first went");
}

#[test]
fn a_removals_undo_puts_the_selection_back_on_its_device() {
    use suhina_engine::model::{Device, DeviceKind, DeviceParam, Extensions};
    let mut project = project_with_a_device();
    project.channels[0].devices.push(Device {
        name: "Utility 2".into(),
        kind: DeviceKind::Builtin("utility".into()),
        enabled: true,
        state: None,
        params: vec![DeviceParam::new("gain", -3.5)],
        automation: Vec::new(),
        extensions: Extensions::default(),
    });
    let mut app = App::new();
    app.set_session(Session::new(project), &[]);
    let mut harness = window(app);
    let devices = |h: &Harness<'static, App>| {
        h.state()
            .session()
            .map(|s| s.project().channels[0].devices.iter().map(|d| d.name.clone()).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    harness.get_by_label("Track Drums").click();
    harness.run();
    // The second device is selected; the first is REMOVED from its menu: the selection follows
    // what it pointed at ("Utility 2"), which stood an index up afterwards.
    harness.get_by_label("Device Utility 2").click();
    harness.run();
    harness.get_by_label("Device Utility").click_secondary();
    harness.run();
    harness.get_by_label("Remove device").click();
    harness.run();
    assert_eq!(devices(&harness), ["Utility 2"], "the FIRST device went");
    // The undo restores it: the selection is back where it was, so the next Delete takes the
    // SELECTED device, not whatever index pointed at meanwhile.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(devices(&harness), ["Utility", "Utility 2"], "the undo put the device back");
    // Redo, straight after, repeats the removal: the selection follows that too, so Delete
    // takes the same selected device again.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Y);
    harness.run();
    assert_eq!(devices(&harness), ["Utility 2"], "the redo repeated the removal");
    harness.key_press(Key::Delete);
    harness.run();
    assert_eq!(devices(&harness), Vec::<String>::new(), "delete takes the selected device again");
}

#[test]
fn the_detail_area_shows_the_selected_tracks_devices_in_a_row_or_stacked() {
    let mut app = App::new();
    app.set_session(Session::new(project_with_a_device()), &[]);
    let mut harness = window(app);
    assert!(harness.query_by_label("Device Utility").is_none());
    harness.get_by_label("Track Drums").click();
    harness.run();
    assert!(harness.query_by_label("Device Utility").is_some());

    let ctx = harness.ctx.clone();
    let settings =
        suhina_ui::settings::Settings { device_layout: suhina_ui::settings::DeviceLayout::Stack, ..Default::default() };
    harness.state_mut().set_settings(&ctx, settings);
    harness.run();
    assert!(harness.query_by_label("Device Utility").is_some());

    harness.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    harness.run();
    assert!(harness.query_by_label("Device Utility").is_none(), "Shift+Tab hides the detail area");
}

#[test]
fn the_piano_roll_playhead_follows_a_launched_clip_not_the_transport() {
    let project = demo_project();
    let mut renderer = suhina_engine::Renderer::stopped(&project, &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut session = Session::new(project);
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = window(app);
    harness.run();
    // One-bar quantisation: a launch waits for the next bar.
    harness.state_mut().projects_mut().execute("launcher.setQuantization", &json!({ "value": "1 bar" })).unwrap();
    harness.state_mut().projects_mut().execute("transport.play", &json!({})).unwrap();
    let mut block = vec![0.0f32; 2 * 256];
    let mut pump = |harness: &mut Harness<'static, App>, renderer: &mut suhina_engine::Renderer, times: usize| {
        for _ in 0..times {
            harness.step();
            for _ in 0..8 {
                renderer.process(&mut block);
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    };
    // Two beats in: the clip's name selects it without launching, and the detail area opens its
    // piano roll.
    pump(&mut harness, &mut renderer, 24);
    harness.get_by_label("Select Beat A on Drums (scene 1)").click();
    pump(&mut harness, &mut renderer, 3);
    // The clip is only selected, not playing: no playhead.
    assert!(harness.query_by_label_contains("Playhead at beat").is_none(), "no playhead while the clip is not playing");
    // Launch it on the mark: queued until the next bar, then playing.
    harness.get_by_label("Launch Beat A on Drums (scene 1)").click();
    let mut playing = false;
    for _ in 0..200 {
        pump(&mut harness, &mut renderer, 1);
        if harness.query_by_label("Stop Beat A on Drums (scene 1)").is_some() {
            playing = true;
            break;
        }
    }
    assert!(playing, "the clip plays at the bar");
    // The playhead is the clip's own position (the beats since its launch), not the transport's,
    // which by now is past the clip's end — the arrangement mapping would show none at all.
    assert!(
        harness.query_by_label_contains("Playhead at beat").is_some(),
        "the piano roll shows the launched clip's playhead"
    );
    // Stopped, there is none.
    harness.state_mut().projects_mut().execute("transport.stop", &json!({})).unwrap();
    pump(&mut harness, &mut renderer, 5);
    assert!(
        harness.query_by_label_contains("Playhead at beat").is_none(),
        "no playhead while the transport is stopped"
    );
}

#[test]
fn a_double_click_opens_the_new_clips_piano_roll() {
    let mut harness = window_with_step(demo_app(), 1.0 / 60.0);
    // The double-click selects the clip it makes: the detail area opens its properties and its
    // piano roll, empty.
    let at = harness.get_by_label("Stop Keys (scene 1)").rect().center();
    double_click_at(&mut harness, at);
    assert!(harness.query_by_label("Clip MIDI clip, scene 1").is_some(), "the properties show");
    assert!(harness.query_by_label("Piano roll, 0 notes").is_some(), "the piano roll opens");
    // One undo takes the clip off again, and the piano roll with it.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(harness.query_by_label("Piano roll, 0 notes").is_none(), "the piano roll closed");
}

#[test]
fn a_selected_slot_shows_in_the_detail_area_and_keys_move_and_launch_it() {
    let mut harness = window(demo_app());
    assert!(harness.query_by_label("Clip Walk, scene 2").is_none());
    harness.get_by_label("Select Walk on Bass (scene 2)").click();
    harness.run();
    assert!(harness.query_by_label("Clip Walk, scene 2").is_some());

    harness.key_press(Key::ArrowDown);
    harness.run();
    assert!(harness.query_by_label("Clip Walk, scene 3").is_some());
    harness.key_press(Key::ArrowRight);
    harness.run();
    assert!(harness.query_by_label("Clip Chords 2, scene 3").is_some());

    // Enter launches it: here without an audio engine, which the status says.
    harness.key_press(Key::Enter);
    harness.run();
    match harness.state().status() {
        Status::Error(message) => assert!(message.contains("no audio engine"), "{message}"),
        other => panic!("expected the launch to be tried, got {other:?}"),
    }

    // The Arrangement view keeps its own selection.
    harness.get_by_label("Arrangement view").click();
    harness.run();
    assert!(harness.query_by_label("Clip Chords 2, scene 3").is_none());
}

#[test]
fn with_select_on_launch_off_a_launch_leaves_the_detail_area_alone() {
    let mut harness = window(demo_app());
    harness.get_by_label("Launch Walk on Bass (scene 2)").click();
    harness.run();
    assert!(harness.query_by_label("Clip Walk, scene 2").is_some(), "on by default: a launch selects");

    let ctx = harness.ctx.clone();
    let settings = suhina_ui::settings::Settings { select_on_launch: false, ..Default::default() };
    harness.state_mut().set_settings(&ctx, settings);
    harness.state_mut().set_browser_open(false);
    harness.get_by_label("Launch Chords 1 on Keys (scene 2)").click();
    harness.run();
    assert!(harness.query_by_label("Clip Walk, scene 2").is_some(), "the selection stays");
    assert!(harness.query_by_label("Clip Chords 1, scene 2").is_none());
}

#[test]
fn a_device_selects_and_deletes_even_beside_a_shown_clip() {
    let devices =
        |h: &Harness<'static, App>| h.state().session().map(|s| s.project().channels[0].devices.len()).unwrap_or(0);
    let mut app = App::new();
    app.set_session(Session::new(project_with_a_device()), &[]);
    let mut harness = window(app);
    // A slot's clip selected: the detail shows its properties and its piano roll. The switch's
    // Devices brings the track's chain up, whose panels pick as they do with the chain alone.
    harness.get_by_label("Launch Beat A on Drums (scene 1)").click();
    harness.run();
    harness.get_by_label("Show Devices").click();
    harness.run();
    assert!(harness.query_by_label("Device Utility").is_some(), "the chain shows beside the clip");
    // The panel's click selects the device there too, and Delete takes it off the track.
    harness.get_by_label("Device Utility").click();
    harness.run();
    harness.key_press(Key::Delete);
    harness.run();
    assert_eq!(devices(&harness), 0, "the device went, the clip staying selected");
    // One undo puts it back.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(devices(&harness), 1, "the device is back");
}

#[test]
fn removing_a_device_below_the_selected_one_moves_the_selection_with_it() {
    use suhina_engine::model::{Device, DeviceKind, DeviceParam, Extensions};
    let devices = |h: &Harness<'static, App>| {
        h.state()
            .session()
            .map(|s| s.project().channels[0].devices.iter().map(|d| d.name.clone()).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    let mut project = project_with_a_device();
    project.channels[0].devices.push(Device {
        name: "Utility 2".into(),
        kind: DeviceKind::Builtin("utility".into()),
        enabled: true,
        state: None,
        params: vec![DeviceParam::new("gain", -3.5)],
        automation: Vec::new(),
        extensions: Extensions::default(),
    });
    let mut app = App::new();
    app.set_session(Session::new(project), &[]);
    let mut harness = window(app);
    harness.get_by_label("Track Drums").click();
    harness.run();
    let names = devices(&harness);
    let last = names.len() - 1;
    // The LAST device selected, then the FIRST removed: the selection follows its device up an
    // index, so Delete takes the same device off, not its neighbour.
    harness.get_by_label(&format!("Device {}", names[last])).click();
    harness.run();
    harness.get_by_label(&format!("Device {}", names[0])).click_secondary();
    harness.run();
    harness.get_by_label("Remove device").click();
    harness.run();
    assert_eq!(devices(&harness).len(), names.len() - 1, "the first device went");
    harness.key_press(Key::Delete);
    harness.run();
    assert_eq!(
        devices(&harness),
        Vec::<String>::new(),
        "the selected device went with the shift; a stale selection would have deleted nothing"
    );
    // One undo brings the Delete back, another the menu's removal.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(devices(&harness), ["Utility 2"], "the Delete undone: the removed device back");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(devices(&harness), ["Utility", "Utility 2"], "the menu's removal undone too");
}

#[test]
fn a_devices_switch_bypasses_it_and_a_click_again_enables_it() {
    let enabled = |h: &Harness<'static, App>| {
        h.state().session().map(|s| s.project().channels[0].devices[0].enabled).unwrap_or_default()
    };
    let mut app = App::new();
    app.set_session(Session::new(project_with_a_device()), &[]);
    let mut harness = window(app);
    harness.get_by_label("Track Drums").click();
    harness.run();
    // The switch bypasses: the device keeps its settings, the sound passes through.
    harness.get_by_label("Bypass Utility").click();
    harness.run();
    assert!(!enabled(&harness), "the device is bypassed");
    // A click again enables it.
    harness.get_by_label("Enable Utility").click();
    harness.run();
    assert!(enabled(&harness), "the device is enabled again");
    // One undo takes the enable off again; the bypass was an edit too.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(!enabled(&harness), "the undo undid the enable");
}
