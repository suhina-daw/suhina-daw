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

/// The device chain in the detail area shows a MIDI effect first, then the instrument, then the
/// audio effects.
#[test]
fn a_midi_effect_shows_before_the_instrument_in_the_chain() {
    let mut project = project();
    let synth = Device { name: "FM Synth".into(), kind: DeviceKind::Builtin("fm-synth".into()), ..device("Pitch") };
    project.channels[0].devices = vec![device("Pitch"), synth, device("Utility")];
    let mut harness = arrangement();
    harness.state_mut().set_session(Session::new(project), &[]);
    harness.run();
    harness.get_by_label(CLIP).click();
    harness.run();
    harness.get_by_label("Show Devices").click();
    harness.run();
    // A device past the area's right edge is not drawn: the FM synth's long list widens its panel.
    let x =
        |name: &str| harness.query_by_label(&format!("Device {name}")).map_or(f32::INFINITY, |n| n.rect().center().x);
    assert!(x("Pitch") < x("FM Synth") && x("FM Synth") < x("Utility"), "MIDI effect, instrument, effect");
    assert!(x("FM Synth").is_finite(), "the instrument is shown");
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
    assert!(harness.query_by_label("Amp Modeler: device on").is_some(), "the power button, not a bypass");
    for label in ["Amp Modeler: eq", "Amp Modeler: hq"] {
        assert!(harness.query_by_label(label).is_some(), "missing {label} toggle");
    }
    // At the width the panel gets, no control is drawn over another (PR #727, a tester's
    // screenshot: Delay over Mid, HQ beside Treble).
    let controls: Vec<(&str, egui::Rect)> = [
        "Amp Modeler: device on",
        "Amp Modeler: gain",
        "Amp Modeler: bass",
        "Amp Modeler: mid",
        "Amp Modeler: treble",
        "Amp Modeler: volume",
        "Amp Modeler: mid freq",
        "Amp Modeler: delay",
        "Amp Modeler: eq",
        "Amp Modeler: hq",
    ]
    .into_iter()
    .map(|label| (label, harness.get_by_label(label).rect()))
    .collect();
    for (i, (a, ra)) in controls.iter().enumerate() {
        for (b, rb) in &controls[i + 1..] {
            assert!(!ra.shrink(1.0).intersects(rb.shrink(1.0)), "{a} {ra:?} is drawn over {b} {rb:?}");
        }
    }
    // The file row's name is cut short beside its steppers, never under them.
    let file = harness.get_by_label("Amp Modeler: model file").rect();
    let stepper = harness.get_by_label("Amp Modeler: previous file").rect();
    assert!(file.right() <= stepper.left() + 1.0, "the file row {file:?} runs under its stepper {stepper:?}");
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
    // The cab is switched off from its header, as other devices are; it has no bypass of its own.
    assert!(harness.query_by_label("Cab Loader: bypass").is_none(), "no bypass toggle");
    for label in ["Cab Loader: phase", "Cab Loader: hp"] {
        assert!(harness.query_by_label(label).is_some(), "missing {label} toggle");
    }
    let controls: Vec<(&str, egui::Rect)> = [
        "Cab Loader: phase",
        "Cab Loader: hp",
        "Cab Loader: bass",
        "Cab Loader: mid",
        "Cab Loader: treble",
        "Cab Loader: mid freq",
        "Cab Loader: delay",
        "Cab Loader: volume",
    ]
    .into_iter()
    .map(|label| (label, harness.get_by_label(label).rect()))
    .collect();
    for (i, (a, ra)) in controls.iter().enumerate() {
        for (b, rb) in &controls[i + 1..] {
            assert!(!ra.shrink(1.0).intersects(rb.shrink(1.0)), "{a} {ra:?} is drawn over {b} {rb:?}");
        }
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

    // One click on the power button switches the device off (device.setEnabled), and another on.
    harness.get_by_label("Amp Modeler: device on").click();
    harness.run();
    assert!(!project_now(&harness).channels[0].devices[0].enabled, "the power click switched it off");
    harness.get_by_label("Amp Modeler: device off").click();
    harness.run();
    assert!(project_now(&harness).channels[0].devices[0].enabled, "and on again from the same button");
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

/// A built-in effect and a MIDI effect show their parameters as controls a click changes, each
/// change one `device.setParam` that undo takes back (#679).
#[test]
fn a_built_in_devices_generated_panel_edits_its_parameters() {
    let mut project = project();
    project.channels[0].devices = vec![device("Pitch"), device("Utility")];
    let mut harness = arrangement();
    harness.state_mut().set_session(Session::new(project), &[]);
    harness.run();
    harness.get_by_label(CLIP).click();
    harness.run();
    harness.get_by_label("Show Devices").click();
    harness.run();
    let value = |harness: &Harness<'static, App>, device: usize, id: &str| {
        project_now(harness).channels[0].devices[device].params.iter().find(|p| p.id == id).map(|p| p.value)
    };
    for (device, label, id) in [(0, "Follow Scale", "followScale"), (1, "Mono", "mono")] {
        assert_eq!(value(&harness, device, id), None, "{id} starts at its default");
        harness.get_by_label(label).click();
        harness.run();
        assert_eq!(value(&harness, device, id), Some(1.0), "{label} turned on");
    }
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_ne!(value(&harness, 1, "mono"), Some(1.0), "undo takes the last change back");
    assert_eq!(value(&harness, 0, "followScale"), Some(1.0));
}

/// An `.sd` effect with an expanded panel: a gain node, its Power toggle in both panels.
fn expanded_device() -> Device {
    use suhina_device_sdk::ui::{CustomControl, DeviceUi, UiBinding, UiGesture};
    use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, Parameter, ParameterControl};
    let mut graph = DeviceGraph::new("org.example.big", "Big Gain", DeviceRole::Effect);
    graph.parameters = vec![
        Parameter::new("gain", "Gain", (0.0, 2.0), 1.0),
        Parameter::new("power", "Power", (0.0, 1.0), 0.0).control(ParameterControl::Toggle),
    ];
    graph.nodes = vec![Node::audio_input("in"), Node::gain("g", "gain"), Node::audio_output("out")];
    graph.connections = vec![Connection::new("in", "g"), Connection::new("g", "out")];
    let mut big = DeviceUi { size: [600.0, 300.0], ..DeviceUi::default() };
    big.controls.push(CustomControl {
        widget: None,
        visible_when: None,
        id: "power".into(),
        name: "Big power switch".into(),
        tooltip: String::new(),
        bounds: [20.0, 20.0, 80.0, 40.0],
        gesture: UiGesture::Toggle,
        x: Some(UiBinding::new("power")),
        y: None,
        drawing: vec![],
    });
    graph.expanded = Some(big);
    Device { state: Some(graph.to_sd().unwrap().into_bytes()), ..Device::new("Big Gain", DeviceKind::Sd) }
}

/// A device with an expanded panel has a button in its header that opens it over the main area;
/// a control in it is the same `device.setParam` as in the compact panel, and the close button
/// and Escape close it (#679).
#[test]
fn an_expanded_panel_opens_from_the_header_and_edits_the_same_parameters() {
    let mut project = project();
    project.channels[0].devices = vec![device("Utility"), expanded_device()];
    let mut harness = arrangement();
    harness.state_mut().set_session(Session::new(project), &[]);
    harness.run();
    harness.get_by_label(CLIP).click();
    harness.run();
    harness.get_by_label("Show Devices").click();
    harness.run();
    assert!(harness.query_by_label("Expand Utility").is_none(), "a device without one has no button");
    assert!(harness.query_by_label("Big power switch").is_none());
    harness.get_by_label("Expand Big Gain").click();
    harness.run();
    let switch = harness.get_by_label("Big power switch").rect();
    assert!(switch.top() < harness.get_by_label("Device Big Gain").rect().top(), "over the main area, above the chain");
    harness.get_by_label("Big power switch").click();
    harness.run();
    let power = |harness: &Harness<'static, App>| {
        project_now(harness).channels[0].devices[1].params.iter().find(|p| p.id == "power").map(|p| p.value)
    };
    assert_eq!(power(&harness), Some(1.0));
    harness.get_by_label("Close expanded panel").click();
    harness.run();
    assert!(harness.query_by_label("Big power switch").is_none(), "the close button closes it");
    harness.get_by_label("Expand Big Gain").click();
    harness.run();
    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.query_by_label("Big power switch").is_none(), "Escape closes it");
    // A panel whose device goes away closes with it.
    harness.get_by_label("Expand Big Gain").click();
    harness.run();
    harness.get_by_label("Remove Big Gain").click();
    harness.run();
    assert!(harness.query_by_label("Close expanded panel").is_none());
}
