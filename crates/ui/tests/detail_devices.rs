// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The detail area's Clip / Devices switch and the double-click on a track that shows its devices
//! (`docs/spec/ui-clip-editor.md` › Where it opens): from an Arrangement header, a Session column's
//! name and its channel strip, opening a hidden detail area, and never changing the project.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::{Device, DeviceKind, DeviceParam, Extensions, Project, TrackId};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 900.0);
/// A MIDI clip on Drums in the Arrangement view, and one in a Session slot.
const CLIP: &str = "Clip Beat B on Drums at bar 17";
const SLOT: &str = "Select Beat B on Drums (scene 3)";

fn device(name: &str) -> Device {
    Device {
        name: name.into(),
        kind: DeviceKind::Builtin(name.to_lowercase()),
        enabled: true,
        state: None,
        params: Vec::new(),
        automation: Vec::new(),
        extensions: Extensions::default(),
    }
}

/// The demo project with a Utility on Drums and a Limiter on Bass.
fn project() -> Project {
    let mut project = demo_project();
    project.channels[0].devices.push(device("Utility"));
    project.channels[1].devices.push(device("Limiter"));
    project
}

/// The window with the project in the Session view, frames close enough for a double-click.
fn window() -> Harness<'static, App> {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(project()), &[]);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness
}

fn arrangement() -> Harness<'static, App> {
    let mut harness = window();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness
}

/// A double-click at `at`, after a second without clicks.
fn double_click_at(harness: &mut Harness<'static, App>, at: Pos2) {
    harness.hover_at(at);
    harness.run();
    let ready = harness.ctx.input(|i| i.time) + 1.0;
    while harness.ctx.input(|i| i.time) < ready {
        harness.step();
    }
    for _ in 0..2 {
        harness.hover_at(at);
        harness.drag_at(at);
        harness.drop_at(at);
        harness.step();
    }
    harness.run();
}

/// A spot of a header's own background, clear of its controls and of the
/// splitter at its left edge: 10 px in, halfway down.
fn header_spot(harness: &Harness<'static, App>, track: &str) -> Pos2 {
    harness.get_by_label(&format!("Track {track}")).rect().left_center() + vec2(10.0, 0.0)
}

/// What the detail area shows: the piano roll (its toolbar's ×), a named device, the Clip / Devices
/// switch.
fn piano(harness: &Harness<'static, App>) -> bool {
    harness.query_by_label_contains("Piano roll,").is_some()
}

fn shows_device(harness: &Harness<'static, App>, name: &str) -> bool {
    harness.query_by_label(&format!("Device {name}")).is_some()
}

fn switch(harness: &Harness<'static, App>) -> bool {
    harness.query_by_label("Show Clip").is_some() && harness.query_by_label("Show Devices").is_some()
}

fn project_now(harness: &Harness<'static, App>) -> Project {
    harness.state().session().unwrap().project().clone()
}

fn hide_detail_area(harness: &mut Harness<'static, App>) {
    harness.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    harness.run();
    assert!(harness.query_by_label("Show Clip").is_none() && !piano(harness), "the detail area is hidden");
}

#[test]
fn the_switch_goes_from_the_clip_to_its_tracks_devices_and_back() {
    let mut harness = arrangement();
    let before = project_now(&harness);
    harness.get_by_label(CLIP).click();
    harness.run();
    assert!(piano(&harness) && switch(&harness), "a MIDI clip shows its piano roll and the switch");
    assert!(!shows_device(&harness, "Utility"));

    harness.get_by_label("Show Devices").click();
    harness.run();
    assert!(!piano(&harness), "the devices take the piano roll's place");
    assert!(shows_device(&harness, "Utility"), "Drums' chain shows");
    assert!(switch(&harness), "the clip column stays, with the switch to go back");
    assert!(harness.query_by_label("Clip Beat B, start 17.1").is_some());

    harness.get_by_label("Show Clip").click();
    harness.run();
    assert!(piano(&harness) && !shows_device(&harness, "Utility"), "the piano roll is back");

    // The key does the same, both ways.
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::D);
    harness.run();
    assert!(!piano(&harness) && shows_device(&harness, "Utility"));
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::D);
    harness.run();
    assert!(piano(&harness) && !shows_device(&harness, "Utility"));

    // On the devices, a click on the selected clip shows its editor again.
    harness.get_by_label("Show Devices").click();
    harness.run();
    assert!(!piano(&harness));
    harness.get_by_label(CLIP).click();
    harness.run();
    assert!(piano(&harness), "a click on a clip edits it");

    // Window state only: the project never changed, and there is nothing to undo.
    assert_eq!(project_now(&harness), before);
    assert!(!harness.state().session().unwrap().can_undo());
}

#[test]
fn the_key_opens_a_hidden_detail_area_on_the_devices_and_needs_a_clip_with_an_editor() {
    let mut harness = arrangement();
    // No clip selected: the key does nothing.
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::D);
    harness.run();
    assert!(!switch(&harness) && !piano(&harness));

    harness.get_by_label(CLIP).click();
    harness.run();
    hide_detail_area(&mut harness);
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::D);
    harness.run();
    assert!(shows_device(&harness, "Utility") && !piano(&harness), "opened on the devices");
}

#[test]
fn a_double_click_on_an_arrangement_header_shows_that_tracks_devices() {
    let mut harness = arrangement();
    let before = project_now(&harness);
    harness.get_by_label(CLIP).click();
    harness.run();
    assert!(piano(&harness));

    // Drums' own header: the clip stays selected, the switch goes to Devices.
    let spot = header_spot(&harness, "Drums");
    double_click_at(&mut harness, spot);
    assert!(!piano(&harness) && shows_device(&harness, "Utility"), "Drums' devices in place of the notes");
    assert!(switch(&harness));
    assert!(harness.state().selected_clip().is_some(), "the clip on that track stays selected");
    assert_eq!(harness.state().arrangement().selected_tracks.first(), Some(&TrackId(1)));

    // Another track's header: its devices, and the clip on Drums is no longer selected.
    let spot = header_spot(&harness, "Bass");
    double_click_at(&mut harness, spot);
    assert!(shows_device(&harness, "Limiter") && !shows_device(&harness, "Utility"));
    assert_eq!(harness.state().selected_clip(), None);
    assert!(!switch(&harness), "no clip, no switch");

    // A single click keeps its meaning: selecting a clip edits it.
    harness.get_by_label(CLIP).click();
    harness.run();
    assert!(piano(&harness));
    assert_eq!(project_now(&harness), before);
}

#[test]
fn a_double_click_on_a_header_opens_the_hidden_detail_area() {
    let mut harness = arrangement();
    hide_detail_area(&mut harness);
    assert!(!shows_device(&harness, "Limiter"));
    let spot = header_spot(&harness, "Bass");
    double_click_at(&mut harness, spot);
    assert!(shows_device(&harness, "Limiter"), "the detail area opened on Bass' devices");
}

#[test]
fn a_double_click_on_a_session_column_name_shows_its_devices() {
    let mut harness = window();
    harness.get_by_label(SLOT).click();
    harness.run();
    assert!(piano(&harness), "the slot's MIDI clip shows its piano roll");
    hide_detail_area(&mut harness);

    let name = harness.get_by_label("Track Bass").rect().center();
    double_click_at(&mut harness, name);
    assert!(shows_device(&harness, "Limiter"), "the detail area opened on Bass' devices");
    assert!(!piano(&harness) && !switch(&harness), "the click on the name cleared the slot selection");
}

#[test]
fn a_double_click_on_the_channel_strip_shows_its_devices_and_keeps_the_slot_on_that_track() {
    let mut harness = window();
    let before = project_now(&harness);
    harness.get_by_label(SLOT).click();
    harness.run();
    assert!(piano(&harness));

    // The strip's name row is its own background for clicks.
    let strip = harness.get_by_label("Drums strip").rect().center();
    double_click_at(&mut harness, strip);
    assert!(!piano(&harness) && shows_device(&harness, "Utility"), "Drums' devices");
    assert!(switch(&harness), "the slot on Drums stays selected, with the switch");
    harness.get_by_label("Show Clip").click();
    harness.run();
    assert!(piano(&harness), "and the switch brings its notes back");

    // Bass' strip, with the detail area hidden: it opens, and the slot on Drums is let go.
    hide_detail_area(&mut harness);
    let strip = harness.get_by_label("Bass strip").rect().center();
    double_click_at(&mut harness, strip);
    assert!(shows_device(&harness, "Limiter") && !switch(&harness));
    assert_eq!(project_now(&harness), before);
}

#[test]
fn a_double_click_on_a_strip_control_is_the_controls_own() {
    let mut harness = window();
    let fader = harness.get_by_label("Drums volume").rect().center();
    hide_detail_area(&mut harness);
    double_click_at(&mut harness, fader);
    assert!(!shows_device(&harness, "Utility"), "the fader's double-click resets it, nothing more");
}

/// The demo project's Drums track with the two NAM devices on its channel, each with its
/// default parameters, as `device.add` leaves them.
fn nam_project() -> Project {
    let mut project = demo_project();
    for (name, id) in [("NAM Amp Modeler", "nam-amp-modeler"), ("NAM Cab Loader", "nam-cab-loader")] {
        let params = suhina_device_registry::info(id)
            .map(|info| info.params.iter().map(|p| DeviceParam::new(p.id, p.default)).collect())
            .unwrap_or_default();
        let device = Device { name: name.into(), kind: DeviceKind::Builtin(id.into()), params, ..device("nam") };
        project.channels[0].devices.push(device);
    }
    project
}

/// A NAM device's panel in the detail area: the knobs, the toggles and the file row's labels are
/// there, and the device is the header's ([docs/spec/nam-amp-and-cab.md] › The panels).
#[test]
fn a_nam_devices_panel_shows_its_controls_and_files() {
    let dir = std::env::temp_dir().join(format!("suhina-ui-nam-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (models, irs) = (dir.join("NAM models"), dir.join("IRs"));
    std::fs::create_dir_all(&models).unwrap();
    std::fs::create_dir_all(&irs).unwrap();
    // The packs: a NAM folder keeps one amp's captures in a subfolder of it; loose files form
    // the "(entered directly)" pack.
    let pack = models.join("Test amp");
    std::fs::create_dir_all(&pack).unwrap();
    let model_path = pack.join("test amp.nam");
    std::fs::write(&model_path, r#"{"version":"0.5.4","architecture":"WaveNet","config":{"layers":[{"input_size":1,"condition_size":1,"channels":1,"head_size":1,"kernel_size":1,"dilations":[1],"activation":"ReLU","gated":false,"head_bias":false}],"head":null,"head_scale":1.0},"weights":[1.0,2.0,0.0,0.0,1.0,0.0,1.0,1.0],"metadata":{"name":"Test amp"}}"#).unwrap();
    let ir_path = irs.join("cab.wav");
    std::fs::write(&ir_path, b"RIFF").unwrap();

    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(nam_project()), &[]);
    app.set_nam_default_folders(models.clone(), irs.clone());
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.get_by_label("Session view").click();
    // The column name's double-click shows the track's devices directly.
    let name = harness.get_by_label("Track Drums").rect().center();
    double_click_at(&mut harness, name);
    assert!(shows_device(&harness, "NAM Amp Modeler"), "the amp's panel header is there");
    for label in [
        "Amp Modeler: gain",
        "Amp Modeler: bass",
        "Amp Modeler: mid",
        "Amp Modeler: treble",
        "Amp Modeler: volume",
        "Amp Modeler: mid freq",
        "Amp Modeler: delay",
    ] {
        assert!(harness.query_by_label(label).is_some(), "missing {label}");
    }
    for label in ["Amp Modeler: eq", "Amp Modeler: hq"] {
        assert!(harness.query_by_label(label).is_some(), "missing {label} toggle");
    }
    // The pack row with its steppers and the file row with its own; the pack is listed.
    for label in [
        "Amp Modeler: pack",
        "Amp Modeler: previous pack",
        "Amp Modeler: next pack",
        "Amp Modeler: model file",
        "Amp Modeler: previous file",
        "Amp Modeler: next file",
    ] {
        assert!(harness.query_by_label(label).is_some(), "missing {label}");
    }

    assert!(shows_device(&harness, "NAM Cab Loader"), "the cab's panel header is there");
    for label in [
        "Cab Loader: bass",
        "Cab Loader: mid",
        "Cab Loader: treble",
        "Cab Loader: mid freq",
        "Cab Loader: delay",
        "Cab Loader: volume",
    ] {
        assert!(harness.query_by_label(label).is_some(), "missing {label}");
    }
    for label in ["Cab Loader: phase", "Cab Loader: hp"] {
        assert!(harness.query_by_label(label).is_some(), "missing {label} toggle");
    }
    for label in [
        "Cab Loader: pack",
        "Cab Loader: previous pack",
        "Cab Loader: next pack",
        "Cab Loader: cab file",
        "Cab Loader: previous file",
        "Cab Loader: next file",
    ] {
        assert!(harness.query_by_label(label).is_some(), "missing {label}");
    }

    // The header's square is the bypass: one click takes the device off (the sound passes it
    // by), another puts it back.
    harness.get_by_label("Bypass NAM Amp Modeler").click();
    harness.run();
    assert!(!project_now(&harness).channels[0].devices[0].enabled, "the square bypassed the amp");
    harness.get_by_label("Bypass NAM Amp Modeler").click();
    harness.run();
    assert!(project_now(&harness).channels[0].devices[0].enabled, "the square put the amp back");
    // One click on the EQ's button turns it on through device.setParam.
    harness.get_by_label("Amp Modeler: eq").click();
    harness.run();
    let project = project_now(&harness);
    let driver = project.channels[0].devices[0].params.iter().find(|p| p.id == "eqOn").unwrap();
    assert_eq!(driver.value, 0.0, "the eq click turned it off (it is on by default)");
    // The pack row's dropdown lists the folder's packs; picking one loads its first file.
    harness.get_by_label("Amp Modeler: pack").click();
    harness.run();
    assert!(
        harness.query_by_label_contains("Amp Modeler: pack: Test amp").is_some(),
        "the open pack row lists the folder's pack"
    );
    harness.get_by_label_contains("Amp Modeler: pack: Test amp").click();
    harness.run();
    assert_eq!(channel_device_state(&harness, 0), model_path.display().to_string());
    // The pack steppers step through the packs (one here: from none, into it).
    harness.get_by_label("Amp Modeler: next pack").click();
    harness.run();
    assert_eq!(channel_device_state(&harness, 0), model_path.display().to_string());
    // One undo takes the file choice back (the Edit › Undo key, Ctrl+Z).
    harness.key_combination_modifiers(Modifiers::COMMAND, &[Key::Z]);
    harness.run();
    assert_eq!(channel_device_state(&harness, 0), "");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A channel device's state as the project holds it, by its index.
fn channel_device_state(harness: &Harness<'static, App>, index: usize) -> String {
    harness.state().session().unwrap().project().channels[0]
        .devices
        .get(index)
        .and_then(|d| d.state.as_deref().map(|state| String::from_utf8_lossy(state).into_owned()))
        .unwrap_or_default()
}
