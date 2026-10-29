// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The EQ's panel (docs/spec/eq.md › The panel): the response display with the bands' dots,
//! the selected band's strips, the type menus and numbered selectors, the globals' column and
//! the Edit switch — every edit the same `device.setParam` command a generated list writes.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::{Device, DeviceKind, DeviceParam, Extensions, Project, TrackId};
use suhina_session::Session;
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 900.0);
const CLIP: &str = "Clip Beat B on Drums at bar 17";

/// The EQ with its parameters at its defaults, as `device.add` leaves it.
fn eq_device() -> Device {
    let params = suhina_device_registry::info("eq")
        .map(|info| info.params.iter().map(|p| DeviceParam::new(p.id, p.default)).collect())
        .unwrap_or_default();
    Device {
        name: "EQ".into(),
        kind: DeviceKind::Builtin("eq".into()),
        enabled: true,
        state: None,
        params,
        automation: Vec::new(),
        extensions: Extensions::default(),
    }
}

fn project() -> Project {
    let mut project = suhina_ui::demo::demo_project();
    project.channels[0].devices.push(eq_device());
    project
}

/// The window with the EQ's panel showing in the detail area's device chain.
fn window() -> Harness<'static, App> {
    window_with(project())
}

/// The window with `project`'s selected clip's device chain showing in the detail area.
fn window_with(project: Project) -> Harness<'static, App> {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(project), &[]);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label(CLIP).click();
    harness.run();
    harness.get_by_label("Show Devices").click();
    harness.run();
    harness
}

fn parameter(harness: &Harness<'static, App>, id: &str) -> f64 {
    harness.state().session().unwrap().project().channels[0].devices[0]
        .params
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.value)
        .expect("the parameter is on the device")
}

/// A parameter of the EQ inside the grouped project's group: the group is the track's first
/// device, the EQ its branch 1's first.
fn grouped_parameter(harness: &Harness<'static, App>, id: &str) -> f64 {
    let project = harness.state().session().unwrap().project();
    let group = project.channels[0].devices[0].as_group().expect("the track's first device is the group");
    group.branches[0].devices[0]
        .params
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.value)
        .expect("the grouped EQ has the parameter")
}

/// The demo project with an EQ on Drums' channel.
fn eq_project() -> Project {
    let mut project = suhina_ui::demo::demo_project();
    project.channels[0].devices.push(eq_device());
    project
}

/// The EQ panel's analyser end to end: with the session's player a real renderer and the
/// transport playing, the panel's watch reaches the engine and the device's output lands in
/// its ring, and the panel keeps the ring's moving count and asks for the next frame while
/// the engine appends (docs/spec/eq.md › The panel › The analyser).
#[test]
fn the_eq_panels_analyser_receives_the_playing_output() {
    let mut renderer =
        suhina_engine::Renderer::stopped(&eq_project(), &suhina_engine::AudioPool::new(), 48_000).expect("a renderer");
    let mut session = Session::new(eq_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = Harness::builder()
        .with_size((1280.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label(CLIP).click();
    harness.run();
    harness.get_by_label("Show Devices").click();
    harness.run();
    // The panel is drawn: it claims its watch row through the frame's monitor.
    assert!(harness.query_by_label("EQ response display").is_some());
    let mut played = |harness: &mut Harness<'static, App>| {
        harness.get_by_label("Play").click();
        harness.run_steps(2);
        let mut block = [0.0f32; 2 * 256];
        for _ in 0..47 {
            renderer.process(&mut block);
        }
        harness.run_steps(2);
        harness
            .ctx
            .data(|d| d.get_temp::<std::sync::Arc<suhina_engine::MeterMonitor>>(egui::Id::new("eq-spectrum")))
            .and_then(|monitor| monitor.spectrum().samples(TrackId(1), 0))
            .unwrap_or_else(|| panic!("the EQ's ring holds the playing output"))
    };
    let (samples, valid) = played(&mut harness);
    assert!(valid > 0, "the engine appended to the ring");
    assert!(
        samples.iter().any(|&s| s.abs() > 0.01),
        "the playing output is in the ring, peak {}",
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    );
    // The panel drew its analyser from the ring and asked for the next frame while the
    // engine appends: the count it keeps moved with the engine's blocks.
    let cursor = harness.ctx.data(|d| d.get_temp::<usize>(egui::Id::new(("eq-spectrum-cursor", 1u32, 0usize))));
    assert!(cursor.is_some_and(|cursor| cursor > 0), "the panel follows the ring while it plays");
}

/// Drums with one device: a group whose first branch holds the EQ — a grouped EQ's panel.
fn grouped_eq_project() -> Project {
    let mut project = suhina_ui::demo::demo_project();
    let mut grouped = eq_device();
    grouped.name = "EQ B".into();
    let mut branch = suhina_engine::model::Branch::new("Branch 1");
    branch.devices.push(grouped);
    let group = Device {
        name: "Group".into(),
        kind: DeviceKind::Group(Box::new(suhina_engine::model::DeviceGroup::with_branches(vec![branch]))),
        enabled: true,
        state: None,
        params: suhina_engine::GROUP_PARAMS.iter().map(|&(id, _, _, default)| DeviceParam::new(id, default)).collect(),
        automation: Vec::new(),
        extensions: Extensions::default(),
    };
    project.channels[0].devices.push(group);
    project
}

/// A grouped EQ's panel claims no watch: nothing inside a group is tapped yet, and its
/// branch-local index would name a top-level device's row and show that device's spectrum as
/// its own (#1050 keeps the real fix; docs/spec/eq.md › The panel › The analyser).
#[test]
fn a_grouped_eqs_panel_claims_no_watch() {
    let mut renderer =
        suhina_engine::Renderer::stopped(&grouped_eq_project(), &suhina_engine::AudioPool::new(), 48_000)
            .expect("a renderer");
    let mut session = Session::new(grouped_eq_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = Harness::builder()
        .with_size((1280.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label(CLIP).click();
    harness.run();
    harness.get_by_label("Show Devices").click();
    harness.run();
    assert!(harness.query_by_label("EQ response display").is_some(), "the grouped EQ's panel is drawn");
    let monitor = harness
        .ctx
        .data(|d| d.get_temp::<std::sync::Arc<suhina_engine::MeterMonitor>>(egui::Id::new("eq-spectrum")))
        .expect("the frame carries the monitor");
    // The grouped EQ's branch-local index is 0: no row may watch it, or a top-level device at
    // index 0 would be tapped into that row and show as the grouped EQ's own spectrum.
    assert!(monitor.spectrum().appended(TrackId(1), 0).is_none(), "the grouped panel claims no watch");
}

/// A grouped EQ's panel edits the grouped EQ: every `device.setParam` it writes carries the
/// device's full path ("0/0/0"), not its branch-local index, which would name whatever device
/// sits at that index on the track instead.
#[test]
fn a_grouped_eqs_panel_edits_the_grouped_device() {
    let mut harness = window_with(grouped_eq_project());
    assert!(harness.query_by_label("EQ response display").is_some(), "the grouped EQ's panel is drawn");
    assert!(!grouped_parameter(&harness, "a5On").ge(&0.5), "the grouped EQ's band 5 is off at the defaults");
    harness.get_by_label("Band 5").click();
    harness.run();
    assert!(grouped_parameter(&harness, "a5On").ge(&0.5), "the click switched the grouped EQ's band 5 on");
    // One undo takes the switching back.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(!grouped_parameter(&harness, "a5On").ge(&0.5), "undo takes the switching back");
}

/// A session without a player drops the previous session's meter monitor from the frame's
/// temp data: a monitor belongs to one engine, and a panel of the next session must not read
/// the previous engine's spectrum as its own.
#[test]
fn a_session_without_a_player_drops_the_previous_sessions_monitor() {
    let mut renderer =
        suhina_engine::Renderer::stopped(&eq_project(), &suhina_engine::AudioPool::new(), 48_000).expect("a renderer");
    let mut session = Session::new(eq_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    let monitor_id = egui::Id::new("eq-spectrum");
    assert!(
        harness.ctx.data(|d| d.get_temp::<std::sync::Arc<suhina_engine::MeterMonitor>>(monitor_id)).is_some(),
        "the frame carries the player's monitor"
    );
    // A session without a player takes over: the previous session's monitor must not stay in
    // the frame's data for the next session's panels to read as their own.
    harness.state_mut().set_session(Session::new(eq_project()), &[]);
    harness.run();
    assert!(
        harness.ctx.data(|d| d.get_temp::<std::sync::Arc<suhina_engine::MeterMonitor>>(monitor_id)).is_none(),
        "the stale monitor is gone"
    );
}

#[test]
fn the_panel_shows_the_display_the_bands_and_the_globals() {
    let harness = window();
    assert!(harness.query_by_label("EQ response display").is_some(), "the display is there");
    for band in 1..=8 {
        assert!(harness.query_by_label(&format!("Band {band}")).is_some(), "the band's selector");
        assert!(harness.query_by_label(&format!("Band {band} type")).is_some(), "the band's type menu");
        assert!(harness.query_by_label_contains(&format!("Band {band}: ")).is_some(), "the band's dot");
    }
    // The defaults select band 1: its three strips are the left column.
    for field in ["Frequency", "Gain", "Q"] {
        assert!(harness.query_by_label(&format!("Band 1 {field}")).is_some(), "the strip {field}");
        assert!(
            harness.query_by_label(&format!("Band 2 {field}")).is_none(),
            "only the selected band's {field} strip is there"
        );
    }
    for label in ["Mode", "Adaptive Q", "Scale", "Output"] {
        assert!(harness.query_by_label(label).is_some(), "the global {label}");
    }
    assert!(harness.query_by_label("Edit curve A").is_none(), "Stereo runs curve A on both sides and has no switch");
}

#[test]
fn a_number_button_selects_its_band_and_switches_it() {
    let mut harness = window();
    assert!(!parameter(&harness, "a5On").ge(&0.5), "band 5 is off at the defaults");
    harness.get_by_label("Band 5").click();
    harness.run();
    assert!(parameter(&harness, "a5On").ge(&0.5), "the click switched band 5 on");
    assert!(harness.query_by_label("Band 5 Frequency").is_some(), "it is selected: its strips show");
    harness.get_by_label("Band 5").click();
    harness.run();
    assert!(!parameter(&harness, "a5On").ge(&0.5), "the click on the selected band switched it off");
    assert!(harness.query_by_label("Band 5 Frequency").is_some(), "it stays selected while off");
    // One undo takes the switching back; the selection is panel state and stays.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(parameter(&harness, "a5On").ge(&0.5), "undo brings band 5 back on");
}

#[test]
fn a_dots_drag_moves_its_bands_frequency_and_gain() {
    let mut harness = window();
    let before = (parameter(&harness, "a3Frequency"), parameter(&harness, "a3Gain"));
    let dot = harness.get_by_label_contains("Band 3: ").rect().center();
    harness.hover_at(dot);
    harness.run();
    harness.drag_at(dot);
    harness.run();
    for step in 1..=12 {
        harness.hover_at(dot + vec2(step as f32 * 6.0, step as f32 * 2.0));
        harness.run();
    }
    harness.drop_at(dot + vec2(72.0, 24.0));
    harness.run();
    let frequency = parameter(&harness, "a3Frequency");
    let gain = parameter(&harness, "a3Gain");
    assert!(frequency > before.0 * 2.0, "dragging right raised the frequency, {before:.0?} to {frequency:.0}");
    assert!(gain < before.1 - 1.0, "dragging down lowered the gain, {before:.1?} to {gain:.2}");
    // One undo takes the whole drag back.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!((parameter(&harness, "a3Frequency") - before.0).abs() < 1.0e-6, "undo takes the drag back");
}

#[test]
fn the_type_menu_sets_the_bands_type() {
    let mut harness = window();
    assert_eq!(parameter(&harness, "a1Type"), 2.0, "band 1 is a low shelf at the defaults");
    harness.get_by_label("Band 1 type").click();
    harness.run();
    harness.get_by_label("Low Cut 48").click();
    harness.run();
    assert_eq!(parameter(&harness, "a1Type"), 0.0, "the menu picked Low Cut 48");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(parameter(&harness, "a1Type"), 2.0, "undo takes the type back");
}

#[test]
fn the_edit_switch_edits_curve_b_in_the_stereo_spread_modes() {
    let mut harness = window();
    harness.get_by_label("Mode").click();
    harness.run();
    harness.get_by_label("Left/Right").click();
    harness.run();
    assert_eq!(parameter(&harness, "mode"), 1.0, "the mode menu switched to Left/Right");
    assert!(harness.query_by_label("Edit curve A").is_some(), "the switch appears in Left/Right");
    // The strips still name curve A's parameters until B is edited.
    assert!(harness.query_by_label("Band 1 Frequency").is_some());
    harness.get_by_label("Edit curve B").click();
    harness.run();
    assert!(harness.query_by_label("Band 1 Frequency B").is_some(), "the strips follow the edited curve");
    // Band 1 of curve B is off at the defaults, and its selector switches it.
    assert!(!parameter(&harness, "b1On").ge(&0.5));
    harness.get_by_label("Band 1").click();
    harness.run();
    assert!(parameter(&harness, "b1On").ge(&0.5), "the selector switched curve B's band 1 on");
    assert!(parameter(&harness, "a1On").ge(&0.5), "curve A's band 1 is untouched: still on");
    assert!(harness.query_by_label("Band 1 Frequency B").is_some(), "curve B's band 1 is the selected one");
    // Back to Stereo: the switch goes, and the strips return to curve A.
    harness.get_by_label("Mode").click();
    harness.run();
    harness.get_by_label("Stereo").click();
    harness.run();
    assert_eq!(parameter(&harness, "mode"), 0.0);
    assert!(harness.query_by_label("Edit curve A").is_none(), "the switch goes in Stereo");
    assert!(harness.query_by_label("Band 1 Frequency").is_some(), "the strips are curve A's again");
}

/// Renders the window with the EQ's panel showing to `target/ui-snapshots/eq-panel.png`,
/// `eq-panel-b.png` (curve B edited in Left/Right) and `eq-panel-spectrum.png` (the transport
/// playing through the EQ, so the analyser's grey line shows behind the curve), to look the
/// display, the dots, the strips, the rows, the switch and the line over. Needs a GPU
/// adapter, so it runs only when asked:
/// `cargo test -p suhina-ui --test main detail_eq_panel -- --ignored render_the_panel`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_panel() {
    let mut harness = window();
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&folder).unwrap();
    let save = |harness: &mut Harness<'static, App>, name: &str| {
        let image = harness.render().expect("render failed");
        image.save(folder.join(name)).expect("save failed");
    };
    save(&mut harness, "eq-panel.png");
    // The Mode menu open, to look the dropdown and its room below the control over.
    harness.get_by_label("Mode").click();
    harness.run();
    save(&mut harness, "eq-panel-mode-menu.png");
    harness.get_by_label("Stereo").click();
    harness.run();
    harness.get_by_label("Mode").click();
    harness.run();
    harness.get_by_label("Left/Right").click();
    harness.run();
    harness.get_by_label("Edit curve B").click();
    harness.run();
    save(&mut harness, "eq-panel-b.png");

    // With a real renderer playing through the EQ, the analyser's grey line behind the curve.
    let mut renderer =
        suhina_engine::Renderer::stopped(&eq_project(), &suhina_engine::AudioPool::new(), 48_000).expect("a renderer");
    let mut session = Session::new(eq_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = Harness::builder()
        .with_size((1280.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label(CLIP).click();
    harness.run();
    harness.get_by_label("Show Devices").click();
    harness.run();
    save(&mut harness, "eq-panel-idle.png");
    harness.get_by_label("Play").click();
    harness.run_steps(2);
    let mut block = [0.0f32; 2 * 256];
    for _ in 0..47 {
        renderer.process(&mut block);
    }
    harness.run_steps(2);
    save(&mut harness, "eq-panel-spectrum.png");
}
