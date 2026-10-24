// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Session view's channel strip in the window: what each mixer height shows, resizing it from
//! its top edge, the fader (one undo step, a double-click for 0 dB), the output dropdown and the
//! device list
//! (`docs/spec/ui-session-view.md` › Mixer); the detail area's two layouts; and the setting that
//! turns off making a clip with a double-click.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::{Device, DeviceKind, Extensions};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::session::mixer_strip::{DEVICES_AT, FADER_AT, NAME_AT, OUTPUT_AT, PAN_AT, SENDS_AT};
use suhina_ui::settings::Settings;
use suhina_ui::settings::view_settings::{DetailLayout, MIXER_HEIGHTS};
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 900.0);

/// The demo project with two devices on Drums, the second one off.
fn project() -> suhina_engine::model::Project {
    let mut project = demo_project();
    for (name, enabled) in [("Utility", true), ("Limiter", false)] {
        project.channels[0].devices.push(Device {
            name: name.into(),
            kind: DeviceKind::Builtin(name.to_lowercase()),
            enabled,
            state: None,
            params: Vec::new(),
            automation: Vec::new(),
            extensions: Extensions::default(),
        });
    }
    project
}

/// The window with the project, `settings` applied, frames close enough for a double-click.
fn window_with(settings: Settings, browser: bool) -> Harness<'static, App> {
    let mut app = App::new();
    app.set_browser_open(browser);
    app.set_session(Session::new(project()), &[]);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, settings);
    harness.run();
    harness
}

fn window_at(height: f32) -> Harness<'static, App> {
    let mut settings = Settings::default();
    settings.view.mixer_height = height as u32;
    window_with(settings, false)
}

/// Which parts Drums' strip shows.
fn parts(harness: &Harness<'static, App>) -> Vec<&'static str> {
    let mut shown = Vec::new();
    if harness.query_by_label("Drums strip").is_some() {
        shown.push("name");
    }
    if harness.query_by_label_contains("Drums strip output:").is_some() {
        shown.push("output");
    }
    if harness.query_by_label("Show Utility on Drums").is_some() {
        shown.push("devices");
    }
    if harness.query_by_label("Drums send to A Reverb").is_some() {
        shown.push("sends");
    }
    if harness.query_by_label("Drums pan").is_some() {
        shown.push("pan");
    }
    let volume = harness.get_by_label("Drums volume").rect().height();
    shown.push(if volume > 40.0 { "fader" } else { "compact" });
    if harness.query_by_label_contains("Drums level:").is_some() {
        shown.push("meter");
    }
    shown
}

fn volume(harness: &Harness<'static, App>) -> f64 {
    harness.state().session().map_or(f64::NAN, |s| s.project().channels[0].volume)
}

#[test]
fn each_mixer_height_shows_its_parts_and_the_toggles_are_always_there() {
    let least = MIXER_HEIGHTS.0 as f32;
    let steps: [(f32, &[&str]); 8] = [
        (least, &["compact"]),
        (FADER_AT - 1.0, &["compact"]),
        (FADER_AT, &["fader", "meter"]),
        (PAN_AT, &["pan", "fader", "meter"]),
        (SENDS_AT, &["sends", "pan", "fader", "meter"]),
        (NAME_AT, &["name", "sends", "pan", "fader", "meter"]),
        (OUTPUT_AT, &["name", "output", "sends", "pan", "fader", "meter"]),
        (DEVICES_AT, &["name", "output", "devices", "sends", "pan", "fader", "meter"]),
    ];
    for (height, expected) in steps {
        let harness = window_at(height);
        assert_eq!(parts(&harness), expected, "at {height} px");
        for always in ["Turn Drums off", "Solo Drums", "Cue Drums"] {
            assert!(harness.query_by_label_contains(always).is_some(), "{always} at {height} px");
        }
        assert!(harness.query_by_label_contains("Arm Drums").is_some(), "arm at {height} px");
    }
}

#[test]
fn the_default_height_shows_the_whole_strip_but_the_device_list() {
    let harness = window_with(Settings::default(), false);
    assert_eq!(parts(&harness), ["name", "sends", "pan", "fader", "meter"]);
    // The fader and its meter stand side by side, the same height.
    let fader = harness.get_by_label("Drums volume").rect();
    let meter = harness.get_by_label_contains("Drums level:").rect();
    assert!(meter.left() > fader.right() && (meter.bottom() - fader.bottom()).abs() < 1.0, "{fader:?} {meter:?}");
}

#[test]
fn the_mixers_top_edge_resizes_it_saves_the_height_and_a_double_click_gives_the_default_back() {
    let mut harness = window_with(Settings::default(), false);
    let edge = harness.get_by_label("Mixer height").rect().center();
    // Up by 100 points: the device list shows.
    harness.drag_at(edge);
    harness.run_steps(1);
    for step in 1..=10 {
        harness.hover_at(edge - vec2(0.0, step as f32 * 10.0));
        harness.run_steps(1);
    }
    harness.drop_at(edge - vec2(0.0, 100.0));
    harness.run();
    assert_eq!(harness.state().settings().view.mixer_height, MIXER_HEIGHTS.1 + 100);
    assert!(parts(&harness).contains(&"devices"));

    // Far down: the least height, the compact volume.
    let edge = harness.get_by_label("Mixer height").rect().center();
    harness.drag_at(edge);
    harness.run_steps(1);
    harness.hover_at(edge + vec2(0.0, 600.0));
    harness.run_steps(1);
    harness.drop_at(edge + vec2(0.0, 600.0));
    harness.run();
    assert_eq!(harness.state().settings().view.mixer_height, MIXER_HEIGHTS.0);
    assert_eq!(parts(&harness), ["compact"]);

    // Far up: it stops under the stop row of the last scene, every slot still in view.
    let edge = harness.get_by_label("Mixer height").rect().center();
    harness.drag_at(edge);
    harness.run_steps(1);
    harness.hover_at(Pos2::new(edge.x, 0.0));
    harness.run_steps(1);
    harness.drop_at(Pos2::new(edge.x, 0.0));
    harness.run();
    let stop = harness.get_by_label("Stop Drums").rect();
    let edge = harness.get_by_label("Mixer height").rect().center();
    assert!(edge.y >= stop.bottom() && edge.y <= stop.bottom() + 4.0, "{edge:?} under {stop:?}");

    // A double-click on the edge: the default height again.
    let ready = harness.ctx.input(|i| i.time) + 1.0;
    while harness.ctx.input(|i| i.time) < ready {
        harness.step();
    }
    for _ in 0..2 {
        harness.hover_at(edge);
        harness.drag_at(edge);
        harness.drop_at(edge);
        harness.step();
    }
    harness.run();
    assert_eq!(harness.state().settings().view.mixer_height, MIXER_HEIGHTS.1);
}

#[test]
fn a_fader_drag_is_one_undo_step_and_a_double_click_resets_it_to_0_db() {
    let mut harness = window_with(Settings::default(), false);
    let fader = harness.get_by_label("Drums volume").rect().center();
    harness.hover_at(fader);
    harness.run_steps(1);
    harness.drag_at(fader);
    harness.run_steps(1);
    // Down lowers the volume, 0.25 dB a point.
    for down in [12.0, 24.0, 48.0] {
        harness.hover_at(fader + vec2(0.0, down));
        harness.run_steps(1);
    }
    harness.drop_at(fader + vec2(0.0, 48.0));
    harness.run();
    let lowered = volume(&harness);
    assert!(lowered < 0.3 && lowered > 0.2, "about -12 dB: {lowered}");

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(volume(&harness), 1.0, "one undo takes the whole drag back");
    assert!(harness.state().session().is_some_and(|s| !s.can_undo()));

    // Lowered again by the wheel, then a double-click: 0 dB.
    harness
        .state_mut()
        .projects_mut()
        .execute("track.setVolume", &serde_json::json!({ "tracks": [1], "db": -9.0 }))
        .unwrap();
    harness.run();
    let ready = harness.ctx.input(|i| i.time) + 1.0;
    while harness.ctx.input(|i| i.time) < ready {
        harness.step();
    }
    for _ in 0..2 {
        harness.hover_at(fader);
        harness.drag_at(fader);
        harness.drop_at(fader);
        harness.step();
    }
    harness.run();
    assert!((volume(&harness) - 1.0).abs() < 1e-9, "{}", volume(&harness));
}

#[test]
fn the_output_dropdown_routes_the_track_into_a_group_as_one_undo_step() {
    let mut harness = window_at(OUTPUT_AT);
    harness
        .state_mut()
        .projects_mut()
        .execute("track.group", &serde_json::json!({ "tracks": ["Keys"], "name": "Synths" }))
        .unwrap();
    harness.run();
    let channel_of = |h: &Harness<'static, App>, name: &str| {
        let p = h.state().session().unwrap().project();
        p.tracks.iter().find(|t| t.name == name).and_then(|t| t.channel)
    };
    let output = |h: &Harness<'static, App>| {
        let p = h.state().session().unwrap().project();
        p.channels.iter().find(|c| Some(c.id) == channel_of(h, "Drums")).and_then(|c| c.output)
    };
    let synths = channel_of(&harness, "Synths");
    assert!(synths.is_some());
    // Drums plays into the main channel.
    let main = output(&harness);
    assert_ne!(main, synths);

    harness.get_by_label("Drums strip output: Main").click();
    harness.run();
    // The menu offers Main and every group but the track itself.
    assert!(harness.query_by_label("Main").is_some());
    harness.get_by_label("Synths").click();
    harness.run();
    assert_eq!(output(&harness), synths);
    assert!(harness.query_by_label("Drums strip output: Synths").is_some(), "the dropdown names the group");
    // A group is not offered as its own output.
    let named = harness.query_all_by_label("Synths").count();
    harness.get_by_label("Synths strip output: Main").click();
    harness.run();
    assert!(harness.query_by_label("Main").is_some());
    assert_eq!(harness.query_all_by_label("Synths").count(), named);
    harness.key_press(Key::Escape);
    harness.run();

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(output(&harness), main, "one undo takes the route back");
    assert!(harness.query_by_label("Drums strip output: Main").is_some());
}

#[test]
fn a_device_in_the_strip_selects_its_track_and_opens_the_detail_area_on_it() {
    let mut harness = window_at(DEVICES_AT + 40.0);
    assert!(harness.query_by_label("Show Limiter on Drums").is_some(), "every device has a row");
    assert!(harness.query_by_label("Show Utility on Bass").is_none());
    // Hidden first, so the click is what opens it.
    harness.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    harness.run();
    assert!(harness.query_by_label("Device Utility").is_none());
    harness.get_by_label("Show Utility on Drums").click();
    harness.run();
    assert!(harness.query_by_label("Device Utility").is_some(), "the detail area shows Drums' chain");
    assert!(harness.query_by_label("Device Limiter").is_some());
}

#[test]
fn the_detail_area_sits_beside_the_browser_or_spans_the_full_width() {
    let mut harness = window_with(Settings::default(), true);
    harness.get_by_label("Track Drums").click();
    harness.run();
    let left = harness.get_by_label("Detail area splitter").rect().left();
    let search = harness.get_by_label("Show MIDI").rect();
    assert!(left > search.right(), "beside the browser by default: {left} after {search:?}");

    let mut settings = Settings::default();
    settings.view.detail_layout = DetailLayout::FullWidth;
    let mut harness = window_with(settings, true);
    harness.get_by_label("Track Drums").click();
    harness.run();
    let splitter = harness.get_by_label("Detail area splitter").rect();
    assert!(splitter.left() < 20.0 && splitter.right() > SIZE.0 - 20.0, "across the whole window: {splitter:?}");
    // The browser ends above the detail area, and the view is right of the browser.
    let search = harness.get_by_label("Show MIDI").rect();
    assert!(search.bottom() < splitter.top());
    assert!(harness.get_by_label("Track Drums").rect().left() > search.right());
    assert!(harness.query_by_label("Device Utility").is_some(), "the chain still shows");
}

/// A double-click on the detail area's splitter closes the area; one on the closed splitter opens
/// it again at its default size.
#[test]
fn a_double_click_on_the_splitter_closes_the_detail_area_and_opens_it_again() {
    let mut harness = window_with(Settings::default(), false);
    harness.get_by_label("Track Drums").click();
    harness.run();
    assert!(harness.query_by_label("Device Utility").is_some(), "the chain shows");
    let splitter = harness.get_by_label("Detail area splitter").rect().center();
    double_click_at(&mut harness, splitter);
    assert!(harness.query_by_label("Device Utility").is_none(), "closed");
    let closed = harness.get_by_label("Detail area splitter").rect().center();
    double_click_at(&mut harness, closed);
    assert!(harness.query_by_label("Device Utility").is_some(), "open again");
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

/// How many clips Keys has, in its slots and on its lane.
fn keys_clips(harness: &Harness<'static, App>) -> (usize, usize) {
    let session = harness.state().session().unwrap();
    let p = session.project();
    let keys = p.tracks.iter().find(|t| t.name == "Keys").map(|t| t.id);
    let slots = p
        .launcher
        .columns
        .iter()
        .filter(|c| Some(c.track) == keys)
        .flat_map(|c| &c.slots)
        .filter(|s| s.clip.is_some())
        .count();
    let lane = p.arrangement.lanes.iter().filter(|l| l.track == keys).map(|l| l.clips.len()).sum();
    (slots, lane)
}

/// A spot of Keys' lane that no clip covers.
fn empty_spot(harness: &Harness<'static, App>) -> Pos2 {
    let lane = harness.get_by_label("Lane Keys").rect();
    let clips: Vec<egui::Rect> = harness.query_all_by_label_contains(" on Keys at bar ").map(|n| n.rect()).collect();
    (0..200)
        .map(|i| egui::pos2(lane.right() - 12.0 - i as f32 * 8.0, lane.center().y))
        .find(|p| lane.contains(*p) && !clips.iter().any(|c| c.expand(2.0).contains(*p)))
        .unwrap()
}

#[test]
fn a_double_click_makes_a_clip_only_while_the_setting_is_on() {
    for on in [true, false] {
        let mut settings = Settings::default();
        settings.view.double_click_clip = on;
        let mut harness = window_with(settings, false);
        let (slots, lane) = keys_clips(&harness);
        let made = usize::from(on);

        let at = harness.get_by_label("Stop Keys (scene 1)").rect().center();
        double_click_at(&mut harness, at);
        assert_eq!(keys_clips(&harness), (slots + made, lane), "Session view, setting on: {on}");

        harness.get_by_label("Arrangement view").click();
        harness.run();
        let at = empty_spot(&harness);
        double_click_at(&mut harness, at);
        assert_eq!(keys_clips(&harness), (slots + made, lane + made), "Arrangement view, setting on: {on}");
    }
}

/// Renders the Session view with a tall mixer to `target/ui-snapshots/mixer-strip.png`, for
/// looking at: `cargo test -p suhina-ui --test mixer_strip -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_strip() {
    for (height, file) in
        [(MIXER_HEIGHTS.1, "mixer-strip.png"), (320, "mixer-strip-tall.png"), (70, "mixer-strip-least.png")]
    {
        let mut app = App::new();
        app.set_browser_open(false);
        app.set_session(Session::new(project()), &[]);
        let mut harness = Harness::builder()
            .with_size(SIZE)
            .with_pixels_per_point(2.0)
            .with_step_dt(0.05)
            .with_max_steps(40)
            .wgpu()
            .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
        theme::install(&harness.ctx);
        let mut settings = Settings::default();
        settings.view.mixer_height = height;
        let ctx = harness.ctx.clone();
        harness.state_mut().set_settings(&ctx, settings);
        harness.run();
        harness
            .state_mut()
            .projects_mut()
            .execute("track.setVolume", &serde_json::json!({ "tracks": [2], "db": -12.0 }))
            .unwrap();
        harness.run();
        harness.remove_cursor();
        harness.run();
        let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
        std::fs::create_dir_all(&out).unwrap_or_else(|e| panic!("{e}"));
        image.save(out.join(file)).unwrap_or_else(|e| panic!("{e}"));
    }
}

/// The names of the devices on Drums, in chain order.
fn drums_devices(harness: &Harness<'static, App>) -> Vec<String> {
    let p = harness.state().session().unwrap().project();
    let channel = p.tracks.iter().find(|t| t.name == "Drums").and_then(|t| t.channel).unwrap();
    p.channel(channel).unwrap().devices.iter().map(|d| d.name.clone()).collect()
}

#[test]
fn devices_picked_in_the_strip_are_copied_pasted_and_deleted_together() {
    let mut harness = window_at(DEVICES_AT + 40.0);
    let before = drums_devices(&harness);
    assert!(before.len() >= 2, "{before:?}");
    // A click picks one; Ctrl (Cmd) adds another.
    harness.get_by_label("Show Utility on Drums").click();
    harness.run();
    harness.get_by_label("Show Limiter on Drums").click_modifiers(Modifiers::COMMAND);
    harness.run();
    // The chain in the detail area lights the same two.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::C);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    let pasted = drums_devices(&harness);
    assert_eq!(pasted.len(), before.len() + 2, "both pasted: {pasted:?}");
    // Delete takes the two picked ones out in one step (the first of each name: the originals).
    harness.query_all_by_label("Show Utility on Drums").next().unwrap().click();
    harness.run();
    harness.query_all_by_label("Show Limiter on Drums").next().unwrap().click_modifiers(Modifiers::COMMAND);
    harness.run();
    harness.key_press(Key::Delete);
    harness.run();
    assert_eq!(drums_devices(&harness).len(), pasted.len() - 2);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(drums_devices(&harness), pasted, "one undo brings both back");
}

/// The devices on Drums.
fn drums(harness: &Harness<'static, App>) -> Vec<Device> {
    let p = harness.state().session().unwrap().project();
    let channel = p.tracks.iter().find(|t| t.name == "Drums").and_then(|t| t.channel).unwrap();
    p.channel(channel).unwrap().devices.clone()
}

#[test]
fn ctrl_g_groups_the_picked_devices_and_the_group_panel_edits_its_branches() {
    let mut harness = window_at(DEVICES_AT + 40.0);
    let before = drums_devices(&harness);
    harness.get_by_label("Show Utility on Drums").click();
    harness.run();
    harness.get_by_label("Show Limiter on Drums").click_modifiers(Modifiers::COMMAND);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::G);
    harness.run();
    let grouped = drums(&harness);
    assert_eq!(grouped.len(), before.len() - 1, "two devices became one group");
    let group = grouped.iter().find_map(Device::as_group).expect("a device group");
    let inside: Vec<&str> = group.branches[0].devices.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(inside, ["Utility", "Limiter"]);
    // The strip lists the group with how many devices it holds.
    assert!(harness.query_by_label("Show Group on Drums").is_some());

    // The group's panel in the detail area: add a branch, mute and solo it.
    harness.get_by_label("Show Group on Drums").click();
    harness.run();
    harness.get_by_label("Add a branch to Group").click();
    harness.run();
    harness.get_by_label("Mute branch Branch 2").click();
    harness.run();
    harness.get_by_label("Solo branch Branch 1").click();
    harness.run();
    let group = drums(&harness).iter().find_map(Device::as_group).cloned().expect("a device group");
    assert_eq!(group.branches.len(), 2);
    assert!(group.branches[1].mute && group.branches[0].solo);
    // The first branch's devices follow the panel in the chain.
    assert!(harness.query_by_label("Device Utility").is_some());

    // Ctrl+Shift+G takes a group of one branch apart; this one has two.
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::G);
    harness.run();
    assert!(drums(&harness).iter().any(|d| d.as_group().is_some()), "two branches stay a group");
    for _ in 0..3 {
        harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        harness.run();
    }
    harness.get_by_label("Show Group on Drums").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::G);
    harness.run();
    assert_eq!(drums_devices(&harness), before, "ungrouped, the chain is as it was");
}

/// The devices on `track`'s channel, by name.
fn chain_of(harness: &Harness<'static, App>, track: &str) -> Vec<String> {
    let project = harness.state().session().unwrap().project();
    let channel = project.tracks.iter().find(|t| t.name == track).and_then(|t| t.channel).unwrap();
    project.channel(channel).unwrap().devices.iter().map(|d| d.name.clone()).collect()
}

fn drag_device(harness: &mut Harness<'static, App>, from: Pos2, to: Pos2) {
    // Stepped: the label beside the pointer repaints while it is dragged.
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(from + (to - from) * 0.5);
    harness.step();
    harness.step();
    harness.hover_at(to);
    harness.step();
    harness.step();
    harness.drop_at(to);
    harness.step();
    harness.run();
}

#[test]
fn a_device_dragged_onto_another_track_moves_there_and_with_ctrl_is_copied() {
    let mut harness = window_at(DEVICES_AT + 40.0);
    let (drums, bass) = (chain_of(&harness, "Drums"), chain_of(&harness, "Bass"));
    let from = harness.get_by_label("Show Limiter on Drums").rect().center();
    let to = harness.get_by_label("Bass channel strip").rect().center();
    drag_device(&mut harness, from, to);
    assert!(!chain_of(&harness, "Drums").contains(&"Limiter".to_string()), "moved away");
    assert_eq!(chain_of(&harness, "Bass").len(), bass.len() + 1);
    assert_eq!(chain_of(&harness, "Bass").last().map(String::as_str), Some("Limiter"));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!((chain_of(&harness, "Drums"), chain_of(&harness, "Bass")), (drums.clone(), bass.clone()), "one undo");
    // With Ctrl held it is copied: Drums keeps it.
    let from = harness.get_by_label("Show Limiter on Drums").rect().center();
    harness.event(egui::Event::ModifiersChanged(Modifiers::COMMAND));
    drag_device(&mut harness, from, to);
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    assert_eq!(chain_of(&harness, "Drums"), drums);
    assert_eq!(chain_of(&harness, "Bass").len(), bass.len() + 1);
}

#[test]
fn a_clicked_volume_takes_a_typed_number_and_the_arrow_keys() {
    let mut harness = window_at(DEVICES_AT + 40.0);
    let db = |h: &Harness<'static, App>| 20.0 * volume(h).log10();
    let name = harness.state().session().unwrap().project().channels[0].name.clone();
    harness.get_by_label(&format!("{name} volume")).click();
    harness.run();
    harness.event(egui::Event::Text("-12".into()));
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert!((db(&harness) + 12.0).abs() < 0.05, "typed: {}", db(&harness));
    // Clicked again, later than a double-click: Up is 1 dB louder, Shift and Down 0.1 dB quieter.
    harness.run_steps(60);
    harness.get_by_label(&format!("{name} volume")).click();
    harness.run();
    harness.key_press(Key::ArrowUp);
    harness.run();
    assert!((db(&harness) + 11.0).abs() < 0.05, "{}", db(&harness));
    harness.key_press_modifiers(Modifiers::SHIFT, Key::ArrowDown);
    harness.run();
    assert!((db(&harness) + 11.1).abs() < 0.05, "{}", db(&harness));
    // With Ctrl held the arrow is the window's shortcut's, not a step of the value.
    let before = volume(&harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::ArrowUp);
    harness.run();
    assert_eq!(volume(&harness), before, "Ctrl+Up does not step the volume");
}

#[test]
fn session_pan_spans_the_strip_and_sends_have_room_for_values() {
    let harness = window_at(320.0);
    let pan = harness.get_by_label("Drums pan").rect();
    let name = harness.get_by_label("Drums strip").rect();
    assert_eq!(pan.x_range(), name.x_range());
    let a = harness.get_by_label("Drums send to A Reverb").rect();
    let b = harness.get_by_label("Drums send to B Delay").rect();
    assert!(a.width() >= 22.0 && b.width() >= 22.0);
    assert_eq!(a.top(), b.top());
    assert!(a.right() < b.left());
    assert!(b.right() <= pan.right());
    let volume = harness.get_by_label("Drums volume").rect();
    let meter = harness.get_by_label_contains("Drums level:").rect();
    // Fader stays left; sends occupy one horizontal row below the controls.
    assert_eq!(volume.left(), pan.left());
    assert_eq!(a.left(), pan.left());
    assert!(a.top() > meter.bottom());
}

#[test]
fn scrolling_the_send_grid_reaches_later_returns_without_moving_the_fader() {
    let mut harness = window_at(SENDS_AT);
    for name in ["C Return", "D Return", "E Return", "F Return", "G Return", "H Return"] {
        harness.state_mut().projects_mut().execute("return.add", &serde_json::json!({"name": name})).unwrap();
    }
    harness.run();
    let fader = harness.get_by_label("Drums volume").rect();
    let before = volume(&harness);
    assert!(harness.query_by_label("Drums send to G Return").is_none());
    let area = harness.get_by_label("Drums send to A Reverb").rect();
    harness.hover_at(area.center());
    harness.step();
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -1000.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    harness.run();
    assert!(harness.query_by_label("Drums send to G Return").is_some());
    assert_eq!(harness.get_by_label("Drums volume").rect(), fader);
    assert_eq!(volume(&harness), before);
}

#[test]
fn further_sends_continue_horizontally_and_shift_wheel_reaches_them() {
    let mut harness = window_at(320.0);
    for name in ["C Return", "D Return", "E Return"] {
        harness.state_mut().projects_mut().execute("return.add", &serde_json::json!({"name": name})).unwrap();
    }
    harness.run();
    let a = harness.get_by_label("Drums send to A Reverb").rect();
    let b = harness.get_by_label("Drums send to B Delay").rect();
    assert_eq!(a.top(), b.top());
    assert!(a.right() < b.left());
    assert!(harness.query_by_label("Drums send to E Return").is_none());
    harness.hover_at(a.center());
    harness.step();
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -1000.0),
        modifiers: Modifiers::SHIFT,
        phase: egui::TouchPhase::Move,
    });
    harness.run();
    let d = harness.get_by_label("Drums send to D Return").rect();
    let e = harness.get_by_label("Drums send to E Return").rect();
    assert_eq!(d.top(), a.top());
    assert_eq!(d.top(), e.top());
    assert!(d.right() < e.left());
}

#[test]
fn session_track_type_stays_in_the_header_with_the_mixer_collapsed() {
    let mut harness = window_at(46.0);
    let marker = harness.get_by_label("Drums: MIDI track").rect();
    let header = harness.get_by_label("Track Drums").rect();
    assert!(header.contains_rect(marker));
    let before = harness.state().session().map(|s| s.project().clone());
    harness.hover_at(marker.center());
    harness.run();
    assert_eq!(harness.state().session().map(|s| s.project().clone()), before);
    harness.get_by_label("Track Drums").click();
    harness.run();
    assert_eq!(harness.state().session().map(|s| s.project().clone()), before);
}
