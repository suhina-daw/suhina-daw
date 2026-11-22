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

/// Save as preset… on the device's menu (`device.savePreset`, docs/decisions/
/// 0036-library-and-shipped-content.md › 5): the field comes with the device's name, a typed
/// name saves the device's values into the library, and the status bar says where.
#[test]
fn a_devices_menu_saves_a_preset_under_the_typed_name() {
    let dir = std::env::temp_dir().join(format!("suhina-ui-save-preset-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut session = Session::new(project_with_a_device());
    session.set_library_base(dir.clone());
    let mut app = App::new();
    app.set_session(session, &[]);
    let mut harness = window(app);
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.get_by_label("Device Utility").click_secondary();
    harness.run();
    harness.get_by_label("Preset name for Utility").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.get_by_label("Preset name for Utility").type_text("Home Squeeze");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    let path = dir.join("Presets/utility/Home Squeeze.utility.preset.suhina");
    assert!(path.exists(), "the preset saved into the library, but the status bar says {:?}", harness.state().status());
    let read = suhina_device_registry::preset(&std::fs::read(&path).unwrap(), 1).unwrap();
    assert_eq!(read.name, "Home Squeeze");
    assert!(read.params.iter().any(|(id, value)| *id == "gain" && *value == -3.5), "{read:?}");
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.contains("saved into the library's Presets folder")),
        "{:?}",
        harness.state().status()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn clicking_a_second_device_moves_the_selection_so_delete_takes_it() {
    use suhina_engine::model::{Device, DeviceKind, DeviceParam, Extensions};
    let mut project = project_with_a_device();
    project.channels[0].devices.push(Device {
        instrument: false,
        inputs: Vec::new(),
        id: Default::default(),
        name: "Utility 2".into(),
        kind: DeviceKind::Builtin("utility".into()),
        enabled: true,
        state: None,
        params: vec![DeviceParam::new("gain", -3.5)],
        automation: Vec::new(),
        clip_host: false,
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
        instrument: false,
        inputs: Vec::new(),
        id: Default::default(),
        name: "Utility 2".into(),
        kind: DeviceKind::Builtin("utility".into()),
        enabled: true,
        state: None,
        params: vec![DeviceParam::new("gain", -3.5)],
        automation: Vec::new(),
        clip_host: false,
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
    // Closed stacked, it keeps its splitter along the window's right edge, to drag it open again.
    let splitter = harness.get_by_label("Detail area splitter").rect();
    assert!(splitter.height() > splitter.width(), "an upright splitter at the right: {splitter:?}");
}

#[test]
fn the_piano_roll_playhead_follows_a_launched_clip_not_the_transport() {
    let project = demo_project();
    let mut renderer =
        suhina_engine::Renderer::stopped(&project, &suhina_engine::AudioPool::new(), 48_000, suhina_session::devices())
            .unwrap();
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

/// In the Launcher view too, a header clicked after a device was picked gives Delete to its
/// track: the track goes, the device picked before stays (`docs/spec/arrangement-editing.md` ›
/// the track headers).
#[test]
fn a_launcher_header_click_after_a_device_pick_gives_delete_to_the_track() {
    let devices =
        |h: &Harness<'static, App>| h.state().session().map(|s| s.project().channels[0].devices.len()).unwrap_or(0);
    let mut app = App::new();
    app.set_session(Session::new(project_with_a_device()), &[]);
    let mut harness = window(app);
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.get_by_label("Device Utility").click();
    harness.run();
    harness.get_by_label("Track Bass").click();
    harness.run();
    harness.key_press(Key::Delete);
    harness.run();
    assert!(!names(&harness).iter().any(|n| n == "Bass"), "Delete removed the track: {:?}", names(&harness));
    assert_eq!(devices(&harness), 1, "the device picked before the header click stays");
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
        instrument: false,
        inputs: Vec::new(),
        id: Default::default(),
        name: "Utility 2".into(),
        kind: DeviceKind::Builtin("utility".into()),
        enabled: true,
        state: None,
        params: vec![DeviceParam::new("gain", -3.5)],
        automation: Vec::new(),
        clip_host: false,
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

/// Without a player (no audio output), a device's automated parameter still shows the
/// automation's value at the start, not its own (#811).
#[test]
fn an_automated_parameter_shows_its_lane_without_a_player() {
    use egui_kittest::kittest::NodeT;
    use suhina_engine::model::{AutomationLane, AutomationPoint};
    let mut project = project_with_a_device();
    let mut lane = AutomationLane::new("gain");
    lane.points = vec![AutomationPoint::new(0.0, -12.0)].into();
    project.channels[0].devices[0].automation.push(lane);
    let mut app = App::new();
    app.set_session(Session::new(project), &[]);
    let mut harness = window(app);
    harness.get_by_label("Track Drums").click();
    harness.run();
    let gain = harness.get_by_label("Gain").accesskit_node().numeric_value();
    assert_eq!(gain, Some(-12.0), "the lane's value, not the device's -3.5");
}

/// The window on the Arrangement view with the audio project (one clip, `Loop`, warped on two
/// markers at beats 0 and 4), not opened yet.
#[allow(clippy::unwrap_used)]
fn audio_window() -> Harness<'static, App> {
    let (project, waveforms) = super::project_with_audio();
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(project), &[]);
    app.set_waveforms(waveforms);
    let mut harness = window_with_step(app, 1.0 / 60.0);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness
}

/// [`audio_window`] with the clip opened in the audio editor.
fn opened_audio_editor() -> Harness<'static, App> {
    let mut harness = audio_window();
    let at = harness.get_by_label("Clip Loop on Loop at bar 1").rect().center();
    double_click_at(&mut harness, at);
    harness.run();
    harness
}

/// [`opened_audio_editor`] with a software engine behind the session, so the transport commands
/// the editor sends reach a play start marker the test reads (`renderer.monitor()`).
fn opened_audio_editor_with_engine(renderer: &mut suhina_engine::Renderer) -> Harness<'static, App> {
    let (project, waveforms) = super::project_with_audio();
    let mut session = Session::new(project);
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    app.set_waveforms(waveforms);
    let mut harness = window_with_step(app, 1.0 / 60.0);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    let at = harness.get_by_label("Clip Loop on Loop at bar 1").rect().center();
    double_click_at(&mut harness, at);
    harness.run();
    // The play start marker far past the clip, where no click in the editor can put it.
    harness.state_mut().run(&Action::new("transport.setStart", json!({ "beats": 40.0 })));
    harness.run();
    renderer.process(&mut [0.0f32; 2 * 64]);
    harness
}

/// The warp markers of the `Loop` clip as (beat, seconds).
#[allow(clippy::unwrap_used)]
fn loop_markers(harness: &Harness<'static, App>) -> Vec<(f64, f64)> {
    let project = harness.state().session().unwrap().project();
    let lane = project.arrangement.lanes.iter().find(|l| l.name == "Loop").unwrap();
    lane.clips[0].audio.warp_markers.iter().map(|m| (m.time, m.audio_time)).collect()
}

#[allow(clippy::unwrap_used)]
fn loop_clip_id(harness: &Harness<'static, App>) -> u32 {
    let project = harness.state().session().unwrap().project();
    project.arrangement.lanes.iter().find(|l| l.name == "Loop").unwrap().clips[0].id.0
}

fn drag_from_to(harness: &mut Harness<'static, App>, from: egui::Pos2, to: egui::Pos2) {
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(to);
    harness.step();
    harness.drop_at(to);
    harness.run();
}

/// An audio clip opened with a double-click shows its waveform in the detail area, where a MIDI
/// clip shows its piano roll (`docs/spec/ui-audio-editor.md`).
#[test]
fn a_double_clicked_audio_clip_opens_the_audio_editor() {
    let mut harness = audio_window();
    assert!(harness.query_by_label("Audio editor").is_none(), "nothing is opened yet");
    let at = harness.get_by_label("Clip Loop on Loop at bar 1").rect().center();
    double_click_at(&mut harness, at);
    harness.run();
    assert!(harness.query_by_label("Audio editor").is_some(), "the waveform is on show");
    // Its two warp markers, and the buttons along its ruler.
    assert!(harness.query_by_label("Warp marker 1").is_some() && harness.query_by_label("Warp marker 2").is_some());
    assert!(harness.query_by_label("Fit").is_some() && harness.query_by_label("Reset warp…").is_some());
}

/// A double-click on the waveform pins a warp marker where the sound is, a double-click on a
/// marker takes it away, and each is one undo step.
#[test]
fn a_double_click_adds_a_warp_marker_and_a_double_click_on_it_removes_it() {
    let mut harness = opened_audio_editor();
    assert_eq!(loop_markers(&harness), [(0.0, 0.0), (4.0, 2.0)]);
    let (first, last) = (
        harness.get_by_label("Warp marker 1").rect().center().x,
        harness.get_by_label("Warp marker 2").rect().center().x,
    );
    // Halfway between them is beat 2, where the sound is second 1.
    let wave = harness.get_by_label("Audio editor").rect();
    let at = egui::pos2((first + last) / 2.0, wave.center().y + 20.0);
    double_click_at(&mut harness, at);
    assert_eq!(loop_markers(&harness), [(0.0, 0.0), (2.0, 1.0), (4.0, 2.0)]);
    // The new marker is the middle one; a double-click on its flag takes it away again (its line
    // is the waveform's unless "Drag markers by line" is on).
    let on_it = harness.get_by_label("Warp marker 2").rect().center();
    double_click_at(&mut harness, on_it);
    assert_eq!(loop_markers(&harness), [(0.0, 0.0), (4.0, 2.0)]);
    // A warped clip keeps two: a double-click on one of them does nothing.
    let on_last = harness.get_by_label("Warp marker 2").rect().center();
    double_click_at(&mut harness, on_last - egui::vec2(2.0, 0.0));
    assert_eq!(loop_markers(&harness).len(), 2);
}

/// Dragging a warp marker moves it, on the grid, and the sound it is pinned to goes with it.
#[test]
fn dragging_a_warp_marker_moves_the_sound_pinned_to_it() {
    let mut harness = opened_audio_editor();
    let id = loop_clip_id(&harness);
    harness.state_mut().run(&Action::new("clip.addWarpMarker", json!({ "clip": id, "beat": 2.0 })));
    harness.run();
    assert_eq!(loop_markers(&harness), [(0.0, 0.0), (2.0, 1.0), (4.0, 2.0)]);
    let (first, last) = (
        harness.get_by_label("Warp marker 1").rect().center().x,
        harness.get_by_label("Warp marker 3").rect().center().x,
    );
    let per_beat = (last - first) / 4.0;
    let from = harness.get_by_label("Warp marker 2").rect().center();
    // Half a beat to the right: beat 2.5 on the grid, the same second of the file.
    drag_from_to(&mut harness, from, from + egui::vec2(per_beat / 2.0, 0.0));
    assert_eq!(loop_markers(&harness), [(0.0, 0.0), (2.5, 1.0), (4.0, 2.0)]);
    // Dragged past its neighbor it stops short of it.
    let from = harness.get_by_label("Warp marker 2").rect().center();
    drag_from_to(&mut harness, from, from + egui::vec2(per_beat * 3.0, 0.0));
    let markers = loop_markers(&harness);
    assert!(markers[1].0 < 4.0 && markers[1].0 > 3.9, "{markers:?}");
    assert_eq!(markers.len(), 3);
}

/// The Reset warp button asks whether to keep the warp markers or reset them; keeping changes
/// nothing, resetting puts back the two markers at the ends.
#[test]
fn the_reset_dialog_keeps_the_markers_or_resets_them() {
    let mut harness = opened_audio_editor();
    let id = loop_clip_id(&harness);
    harness.state_mut().run(&Action::new("clip.addWarpMarker", json!({ "clip": id, "beat": 2.0 })));
    harness.run();
    assert!(harness.query_by_label("Keep warp markers").is_none(), "no dialog yet");
    harness.get_by_label("Reset warp…").click();
    harness.run();
    assert!(harness.query_by_label("Keep warp markers").is_some(), "the dialog asks");
    harness.get_by_label("Keep warp markers").click();
    harness.run();
    assert!(harness.query_by_label("Keep warp markers").is_none(), "and goes");
    assert_eq!(loop_markers(&harness).len(), 3, "the markers are as they were");
    harness.get_by_label("Reset warp…").click();
    harness.run();
    harness.get_by_label("Reset to clip tempo").click();
    harness.run();
    assert!(harness.query_by_label("Keep warp markers").is_none());
    assert_eq!(loop_markers(&harness), [(0.0, 0.0), (4.0, 2.0)]);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(loop_markers(&harness).len(), 3, "one undo step brings the markers back");
}

/// The audio editor's right-click menu is the kit's rows: an item with nothing to act on is grayed
/// and takes no click, one that has adds its command.
#[test]
fn the_audio_editors_menu_grays_what_has_nothing_to_act_on() {
    let mut harness = opened_audio_editor();
    harness.get_by_label("Audio editor").click_secondary();
    harness.run();
    // Two markers, which a warped clip keeps, and no time selection.
    assert!(
        egui_kittest::kittest::NodeT::accesskit_node(&harness.get_by_label("Delete")).is_disabled(),
        "Delete is grayed"
    );
    assert!(egui_kittest::kittest::NodeT::accesskit_node(&harness.get_by_label("Loop selection")).is_disabled());
    harness.get_by_label("Delete").click();
    harness.run();
    assert_eq!(loop_markers(&harness).len(), 2, "a grayed item does nothing");
    // The menu is still open: a click on a grayed item does not close it.
    assert!(!egui_kittest::kittest::NodeT::accesskit_node(&harness.get_by_label("Add warp marker here")).is_disabled());
    harness.get_by_label("Add warp marker here").click();
    harness.run();
    assert_eq!(loop_markers(&harness).len(), 3, "a marker where the menu was opened");
}

/// The Clip / Devices switch's key works for an audio clip as for a MIDI clip: the editor gives
/// way to the track's devices and comes back.
#[test]
fn the_devices_switch_takes_an_audio_clip_to_its_devices_and_back() {
    let mut harness = opened_audio_editor();
    assert!(harness.query_by_label("Audio editor").is_some());
    // A window action (Ctrl+Shift+D), not a command: it runs on the next frame.
    harness.state_mut().queue_action("view.devices");
    harness.run();
    assert!(harness.query_by_label("Audio editor").is_none(), "the devices took the editor's place");
    // A window action (Ctrl+Shift+D), not a command: it runs on the next frame.
    harness.state_mut().queue_action("view.devices");
    harness.run();
    assert!(harness.query_by_label("Audio editor").is_some(), "and the editor is back");
}

/// With the devices stacked the audio editor still lies along the bottom, as the piano roll does:
/// closed, its splitter runs along the bottom, not up the window's side.
#[test]
fn with_the_devices_stacked_the_audio_editor_stays_along_the_bottom() {
    let mut harness = opened_audio_editor();
    let ctx = harness.ctx.clone();
    let settings =
        suhina_ui::settings::Settings { device_layout: suhina_ui::settings::DeviceLayout::Stack, ..Default::default() };
    harness.state_mut().set_settings(&ctx, settings);
    harness.run();
    let editor = harness.get_by_label("Audio editor").rect();
    assert!(editor.width() > editor.height(), "along the bottom: {editor:?}");
    harness.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    harness.run();
    let splitter = harness.get_by_label("Detail area splitter").rect();
    assert!(splitter.width() > splitter.height(), "a splitter along the bottom: {splitter:?}");
}

/// Presses and lets go once at `at`, a second after anything before it, so it is a click and not
/// part of a double-click.
fn click_once_at(harness: &mut Harness<'static, App>, at: egui::Pos2) {
    harness.hover_at(at);
    harness.run();
    let ready = harness.ctx.input(|i| i.time) + 1.0;
    while harness.ctx.input(|i| i.time) < ready {
        harness.step();
    }
    harness.hover_at(at);
    harness.drag_at(at);
    harness.drop_at(at);
    harness.step();
    harness.run();
}

#[allow(clippy::unwrap_used)]
fn loop_clips(harness: &Harness<'static, App>) -> usize {
    let project = harness.state().session().unwrap().project();
    project.arrangement.lanes.iter().find(|l| l.name == "Loop").unwrap().clips.len()
}

/// With the audio editor holding the keyboard, Delete removes the warp marker clicked and never
/// the clip behind the editor; with the two markers a warped clip keeps, it does nothing.
#[test]
fn delete_in_the_audio_editor_removes_the_marker_not_the_clip() {
    let mut harness = opened_audio_editor();
    let id = loop_clip_id(&harness);
    harness.state_mut().run(&Action::new("clip.addWarpMarker", json!({ "clip": id, "beat": 2.0 })));
    harness.run();
    assert_eq!(loop_markers(&harness).len(), 3);
    // On its flag in the marker lane (its line takes the pointer only with the setting on).
    let on_it = harness.get_by_label("Warp marker 2").rect().center();
    click_once_at(&mut harness, on_it);
    assert!(harness.state().audio_editor().focused(), "the click gave the editor the keyboard");
    assert_eq!(harness.state().audio_editor().picked(), [2.0], "the marker clicked is picked");
    harness.key_press(Key::Delete);
    harness.run();
    assert_eq!(loop_markers(&harness), [(0.0, 0.0), (4.0, 2.0)], "the picked marker is gone");
    assert_eq!(loop_clips(&harness), 1, "the clip is still there");
    // Two markers left: the end one picked on its flag, Delete does nothing, and still not to
    // the clip.
    let on_last = harness.get_by_label("Warp marker 2").rect().center();
    click_once_at(&mut harness, on_last);
    assert_eq!(harness.state().audio_editor().picked(), [4.0], "the end marker is picked");
    harness.key_press(Key::Delete);
    harness.run();
    assert_eq!(loop_markers(&harness).len(), 2);
    assert_eq!(loop_clips(&harness), 1, "the clip is still there");
}

/// With the audio editor pressed last, Ctrl+A selects the whole file and picks its warp markers.
#[test]
fn ctrl_a_in_the_audio_editor_selects_the_whole_file() {
    let mut harness = opened_audio_editor();
    harness.get_by_label("Audio editor").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    let editor = harness.state().audio_editor();
    let (start, end) = editor.span().expect("a time selection");
    let beats: Vec<f64> = loop_markers(&harness).iter().map(|m| m.0).collect();
    assert!(start <= beats[0] + 1e-9 && end >= beats[beats.len() - 1] - 1e-9, "{start}..{end} covers {beats:?}");
    assert_eq!(editor.picked(), beats.as_slice(), "every marker is picked");
}

#[test]
fn ctrl_click_picks_several_warp_markers_and_delete_removes_them_in_one_step() {
    let mut harness = opened_audio_editor();
    let id = loop_clip_id(&harness);
    for beat in [1.0, 2.0, 3.0] {
        harness.state_mut().run(&Action::new("clip.addWarpMarker", json!({ "clip": id, "beat": beat })));
    }
    harness.run();
    assert_eq!(loop_markers(&harness).len(), 5);
    let second = harness.get_by_label("Warp marker 2").rect().center();
    click_once_at(&mut harness, second);
    harness.get_by_label("Warp marker 4").click_modifiers(Modifiers::COMMAND);
    harness.run();
    assert_eq!(harness.state().audio_editor().picked(), [1.0, 3.0], "Ctrl adds the second");
    harness.key_press(Key::Delete);
    harness.run();
    assert_eq!(loop_markers(&harness), [(0.0, 0.0), (2.0, 1.0), (4.0, 2.0)], "both picked markers are gone");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(loop_markers(&harness).len(), 5, "one undo brings both back");
}

/// A warp marker dragged past the editor's right edge still moves when the button is let go
/// there, and the editor shows no drag afterwards.
#[test]
fn a_marker_dragged_past_the_editors_edge_still_moves() {
    let mut harness = opened_audio_editor();
    let wave = harness.get_by_label("Audio editor").rect();
    // From its flag on the marker lane: its line takes the pointer only with the setting on.
    let flag = harness.get_by_label("Warp marker 2").rect().center();
    let from = egui::pos2(flag.x - 2.0, flag.y);
    drag_from_to(&mut harness, from, egui::pos2(wave.right() + 60.0, from.y));
    let markers = loop_markers(&harness);
    assert_eq!(markers.len(), 2);
    assert!(markers[1].0 > 4.0, "the end marker moved later: {markers:?}");
    assert_eq!(markers[1].1, 2.0, "with the same second of the file");
    assert!(!harness.state().audio_editor().dragging(), "no drag is left over");
}

/// An audio clip with a color of its own is drawn in the editor in that color, not its track's.
#[test]
#[allow(clippy::unwrap_used)]
fn the_audio_editor_takes_the_clips_own_color() {
    let mut harness = opened_audio_editor();
    let id = loop_clip_id(&harness);
    let track = {
        let project = harness.state().session().unwrap().project();
        project.arrangement.lanes.iter().find(|l| l.name == "Loop").unwrap().track.unwrap()
    };
    let shown = |harness: &Harness<'static, App>| {
        let project = harness.state().session().unwrap().project();
        suhina_ui::detail::audio_editor::arrangement_clip(project, track, suhina_engine::model::ClipId(id))
            .unwrap()
            .color
    };
    let before = shown(&harness);
    harness.state_mut().run(&Action::new("clip.setColor", json!({ "track": track.0, "clip": id, "color": "#123456" })));
    harness.run();
    assert_eq!(shown(&harness), egui::Color32::from_rgb(0x12, 0x34, 0x56));
    harness.state_mut().run(&Action::new("clip.setColor", json!({ "track": track.0, "clip": id, "color": "none" })));
    harness.run();
    assert_eq!(shown(&harness), before, "back to the track's color");
}

/// The Transients button pins markers on the file's transients: one command, one undo step.
#[test]
fn the_transients_button_places_markers_in_one_step() {
    let mut harness = opened_audio_editor();
    let before = loop_markers(&harness);
    harness.get_by_label("Add warp markers at transients").click();
    harness.run();
    // The click waits for the note starts the editor finds in the background, then goes.
    suhina_ui::detail::audio_editor::await_transients(&harness.ctx);
    harness.run();
    let after = loop_markers(&harness);
    assert!(after.len() >= before.len(), "{after:?}");
    // Every marker it adds keeps its sound where it plays: beat and second on the clip's tempo.
    for (beat, seconds) in &after {
        assert!((beat - seconds * 2.0).abs() < 1e-6, "{beat} beats for {seconds} s");
    }
    if after.len() > before.len() {
        harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        harness.run();
        assert_eq!(loop_markers(&harness), before, "one undo step");
    }
}

/// The clip panel's Warp part shows the settings of the clip's warp mode, and its editor buttons
/// while the audio editor shows the clip; the editor's ruler has none of them.
#[test]
fn the_warp_part_shows_the_modes_settings_and_the_editors_buttons() {
    let mut harness = opened_audio_editor();
    let id = loop_clip_id(&harness);
    // The default mode, Smooth, has no settings; the editor's buttons are there.
    assert!(harness.query_by_label("Fit").is_some() && harness.query_by_label("Reset warp…").is_some());
    assert!(harness.query_by_label("Snap to grid: On").is_some(), "the editor's grid snaps");
    harness.state_mut().run(&Action::new("clip.setAudio", json!({ "clip": id, "mode": "texture" })));
    harness.run();
    assert!(harness.query_by_label("Grain: 60 ms").is_some() && harness.query_by_label("Scatter: 25 %").is_some());
    harness.state_mut().run(&Action::new("clip.setAudio", json!({ "clip": id, "mode": "rhythmic", "tail": "off" })));
    harness.run();
    assert!(harness.query_by_label("Grain: 60 ms").is_none(), "Rhythmic has its own settings");
    assert!(harness.query_by_label("Envelope: 100 %").is_some());
    // Snap off from the panel: the editor's grid stops snapping.
    harness.get_by_label("Snap to grid: On").click();
    harness.run();
    assert!(!harness.state().audio_editor().grid().snap);
}

/// With the audio editor pressed last, Ctrl+1 and Ctrl+2 make its grid finer and coarser and
/// Ctrl+4 turns snapping off, leaving the Arrangement's grid alone.
#[test]
fn the_grid_keys_change_the_audio_editors_grid() {
    let mut harness = opened_audio_editor();
    let arrangement = harness.state().arrangement().grid;
    let wave = harness.get_by_label("Audio editor").rect();
    harness.get_by_label("Audio editor").click();
    harness.run();
    let _ = wave;
    let before = harness.state().audio_editor().grid();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Num1);
    harness.run();
    let finer = harness.state().audio_editor().grid();
    assert_ne!(finer, before, "Ctrl+1 made it finer");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Num2);
    harness.run();
    assert_eq!(harness.state().audio_editor().grid(), before, "Ctrl+2 made it coarser again");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Num4);
    harness.run();
    assert!(!harness.state().audio_editor().grid().snap, "Ctrl+4 turned snapping off");
    assert_eq!(harness.state().arrangement().grid, arrangement, "the Arrangement's grid is left alone");
}

/// The audio clip's panel is made narrower by its right edge: its parts fold into fewer columns,
/// every one still there, and the audio editor takes the room; a double-click makes it as wide
/// as it goes again (`docs/spec/clip-panel.md` › Layout).
#[test]
fn the_audio_clips_panel_narrows_and_folds_its_parts() {
    let mut harness = opened_audio_editor();
    let wide = harness.get_by_label("Audio editor").rect();
    let gain = harness.get_by_label("Envelope: Off").rect();
    let handle = harness.get_by_label("Clip panel width").rect().center();
    drag_from_to(&mut harness, handle, egui::pos2(handle.x - 300.0, handle.y));
    let narrow = harness.get_by_label("Audio editor").rect();
    assert!(narrow.left() < wide.left() - 150.0, "the editor took the room: {wide:?} -> {narrow:?}");
    assert!(narrow.left() > wide.left() - 320.0, "the panel stops at its narrowest: {narrow:?}");
    // Every part is still there, some under others.
    for label in ["Fit", "Reset warp…", "Snap to grid: On", "Envelope: Off"] {
        assert!(harness.query_by_label(label).is_some(), "{label}");
    }
    assert_ne!(harness.get_by_label("Envelope: Off").rect(), gain, "the parts moved as they folded");
    // A double-click makes it as wide as it goes.
    let handle = harness.get_by_label("Clip panel width").rect().center();
    harness.hover_at(handle);
    harness.step();
    double_click_at(&mut harness, handle);
    harness.run();
    assert_eq!(harness.get_by_label("Audio editor").rect().left(), wide.left());
}

/// A double-click on the audio editor's ruler fits the file again and does not seek (neither of
/// its clicks moves playback); a click alone on the ruler still does
/// (`docs/spec/ui-audio-editor.md` › Zoom and scroll).
#[test]
fn a_double_click_on_the_audio_editors_ruler_fits_without_a_seek() {
    let (project, _) = super::project_with_audio();
    let mut renderer =
        suhina_engine::Renderer::stopped(&project, &suhina_engine::AudioPool::new(), 48_000, suhina_session::devices())
            .unwrap();
    let monitor = renderer.monitor();
    let mut harness = opened_audio_editor_with_engine(&mut renderer);
    assert_eq!(monitor.start_beat(), 40.0);
    let fitted = harness.state().audio_editor().beat_width();
    let editor = harness.get_by_label("Audio editor").rect();
    // Zoomed in first, so the fit shows.
    harness.hover_at(editor.center());
    harness.run();
    harness.event(egui::Event::Zoom(2.0));
    harness.run();
    assert!(harness.state().audio_editor().beat_width() > fitted * 1.5, "zoomed in");
    // The ruler is under the editor's header bar.
    let on_ruler = egui::pos2(editor.center().x, editor.top() + 20.0 + 14.0);
    double_click_at(&mut harness, on_ruler);
    renderer.process(&mut [0.0f32; 2 * 64]);
    assert!((harness.state().audio_editor().beat_width() - fitted).abs() < 1e-3, "fitted again");
    assert_eq!(monitor.start_beat(), 40.0, "no seek from either click of the double-click");
    click_once_at(&mut harness, on_ruler);
    renderer.process(&mut [0.0f32; 2 * 64]);
    assert!(monitor.start_beat() < 40.0, "a click alone seeks: {}", monitor.start_beat());
}

/// A double-click on the waveform adds a warp marker and does not move the play start marker;
/// a click alone there still does (`docs/spec/ui-audio-editor.md`).
#[test]
fn a_double_click_on_the_waveform_adds_a_marker_without_moving_the_play_start() {
    let (project, _) = super::project_with_audio();
    let mut renderer =
        suhina_engine::Renderer::stopped(&project, &suhina_engine::AudioPool::new(), 48_000, suhina_session::devices())
            .unwrap();
    let monitor = renderer.monitor();
    let mut harness = opened_audio_editor_with_engine(&mut renderer);
    let (first, last) = (
        harness.get_by_label("Warp marker 1").rect().center().x,
        harness.get_by_label("Warp marker 2").rect().center().x,
    );
    let wave = harness.get_by_label("Audio editor").rect();
    let at = egui::pos2((first + last) / 2.0, wave.center().y + 20.0);
    double_click_at(&mut harness, at);
    renderer.process(&mut [0.0f32; 2 * 64]);
    assert_eq!(loop_markers(&harness).len(), 3, "a marker was added");
    assert_eq!(monitor.start_beat(), 40.0, "the double-click did not move the play start marker");
    click_once_at(&mut harness, egui::pos2((first + at.x) / 2.0, at.y));
    renderer.process(&mut [0.0f32; 2 * 64]);
    assert!(monitor.start_beat() < 40.0, "a click alone moves it: {}", monitor.start_beat());
}

/// The commands that place markers on note starts take those the editor found in the background
/// (`transients`), so the window never analyses the file itself: one made before they are found
/// waits and goes once they are; one that brings its own list goes as it is
/// (`docs/spec/ui-audio-editor.md` › Note starts found once).
#[test]
fn the_transient_commands_take_the_note_starts_found_in_the_background() {
    let mut harness = opened_audio_editor();
    let ctx = harness.ctx.clone();
    let id = loop_clip_id(&harness);
    let quantize =
        Action::new("clip.quantizeWarpMarkers", json!({ "track": 30, "clip": id, "grid": "1/16", "amount": 100.0 }));
    let markers = Action::new("clip.addWarpMarkersAtTransients", json!({ "track": 30, "clip": id }));
    // Sent now if they are found already, else held until they are.
    let mut sent = harness.state_mut().with_transients(&ctx, vec![quantize, markers]);
    suhina_ui::detail::audio_editor::await_transients(&ctx);
    sent.extend(harness.state_mut().with_transients(&ctx, Vec::new()));
    let ids: Vec<&str> = sent.iter().map(|a| a.id).collect();
    assert_eq!(ids, ["clip.quantizeWarpMarkers", "clip.addWarpMarkersAtTransients"], "each sent once, in order");
    for action in &sent {
        let seconds = action.params.get("transients").and_then(serde_json::Value::as_array);
        let seconds = seconds.unwrap_or_else(|| panic!("{} carries the note starts: {}", action.id, action.params));
        assert!(seconds.iter().all(|s| s.as_f64().is_some_and(|s| (0.0..=2.0).contains(&s))), "{seconds:?}");
    }
    // A list of its own is kept.
    let own = Action::new("clip.addWarpMarkersAtTransients", json!({ "track": 30, "clip": id, "transients": [0.5] }));
    assert_eq!(harness.state_mut().with_transients(&ctx, vec![own.clone()]), [own]);
}
