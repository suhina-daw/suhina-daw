// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Session view and the window, driven like a user would: clicks become the commands the
//! CLI and MCP run, and edits change the project through the session.

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use suhina_engine::model::TrackId;
use suhina_session::Session;
use suhina_ui::arrangement::arrangement_view::{self, ArrangementViewState};
use suhina_ui::arrangement::timeline::Timeline;
use suhina_ui::demo::demo_project;
use suhina_ui::interaction::grid::{Grid, SlotState};
use suhina_ui::kit::widgets::Action;
use suhina_ui::session::session_view::{self, SessionViewState};
use suhina_ui::settings::themes::FileAction;
use suhina_ui::{App, OpenAs, Status, View, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);

/// A file dropped on the window in a test.
#[derive(Debug)]
struct TestFile(std::path::PathBuf);

impl egui::DroppedFile for TestFile {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(Vec::new())
    }
}

struct SessionHarness {
    grid: Grid,
    state: SessionViewState,
    actions: Vec<Action>,
}

fn view() -> Harness<'static, SessionHarness> {
    let view = SessionHarness {
        grid: Grid::build(&demo_project(), None),
        state: SessionViewState::default(),
        actions: Vec::new(),
    };
    let mut harness = Harness::builder().with_size(SIZE).build_ui_state(
        |ui, v: &mut SessionHarness| session_view::show(ui, &v.grid, &[], &mut v.state, &mut v.actions),
        view,
    );
    theme::install(&harness.ctx);
    harness.run();
    harness
}

fn clicked(harness: &mut Harness<'static, SessionHarness>, label: &str) -> Vec<Action> {
    harness.state_mut().actions.clear();
    harness.get_by_label(label).click();
    harness.run();
    harness.state().actions.clone()
}

#[test]
fn clicking_a_clip_launches_it() {
    let mut harness = view();
    assert_eq!(
        clicked(&mut harness, "Launch Walk on Bass (scene 2)"),
        [Action::new("clip.launch", json!({ "track": 2, "scene": 2 }))]
    );
}

#[test]
fn clicking_an_empty_slot_with_a_stop_button_stops_the_track() {
    let mut harness = view();
    assert_eq!(
        clicked(&mut harness, "Stop Keys (scene 1)"),
        [Action::new("clip.launch", json!({ "track": 3, "scene": 1 }))]
    );
}

#[test]
fn scenes_and_stop_buttons_send_their_commands() {
    let mut harness = view();
    assert_eq!(clicked(&mut harness, "Launch scene 3 Build"), [Action::new("scene.launch", json!({ "scene": 3 }))]);
    // A playing row's click stops its clips.
    harness.state_mut().grid.scenes[2].playing = true;
    harness.run();
    assert_eq!(clicked(&mut harness, "Stop scene 3 Build"), [Action::new("scene.stop", json!({ "scene": 3 }))]);
    harness.state_mut().grid.scenes[2].playing = false;
    harness.run();
    assert_eq!(clicked(&mut harness, "Stop Drums"), [Action::new("track.stop", json!({ "track": 1 }))]);
    assert_eq!(clicked(&mut harness, "Stop all clips"), [Action::new("launcher.stopAll", json!({}))]);
    assert_eq!(
        clicked(&mut harness, "Turn Vox off"),
        [Action::new("track.setMute", json!({ "tracks": [4], "mute": true }))]
    );
}

#[test]
fn a_scene_menu_inserts_duplicates_and_deletes_scenes() {
    let mut harness = view();
    for (entry, action) in [
        ("Insert scene below", Action::new("scene.insert", json!({ "scene": 4 }))),
        ("Duplicate scene", Action::new("scene.duplicate", json!({ "scene": 3 }))),
        ("Delete scene", Action::new("scene.delete", json!({ "scene": 3 }))),
    ] {
        harness.state_mut().actions.clear();
        harness.get_by_label("Launch scene 3 Build").click_secondary();
        harness.run();
        harness.get_by_label(entry).click();
        harness.run();
        assert_eq!(harness.state().actions, [action], "{entry}");
    }
}

#[test]
fn return_tracks_have_a_mixer_but_no_slots() {
    let harness = view();
    assert!(harness.query_by_label("Turn A Reverb off").is_some());
    assert!(harness.query_by_label_contains("on A Reverb").is_none());
}

fn window(app: App) -> Harness<'static, App> {
    window_with_step(app, 0.25)
}

/// Like [`window`], with `step` seconds a frame (a double-click needs frames closer together),
/// and a second of frames for animations to settle.
fn window_with_step(app: App, step: f32) -> Harness<'static, App> {
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(step)
        .with_max_steps((1.0 / step).ceil() as u64)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    // Frames, not a run to rest: a window that repaints every frame while it settles (the
    // sample map's grid building, an animation) would spend the run's max and fail, where
    // these frames advance the same second.
    harness.run_steps(((1.0_f64 / step as f64).ceil() as usize).max(1));
    harness
}

/// The demo project in the window, with the browser hidden so the views have the whole width
/// (the browser has tests of its own, in `browser.rs`).
fn demo_app() -> App {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(demo_project()), &[]);
    app
}

/// The demo project with its arrangement stretched to 512 beats (a stretch of silence after
/// the clips): an export of it takes seconds, so a test can step through the window while it
/// is inside the running export, not at its race's end.
fn long_demo_project() -> suhina_engine::model::Project {
    let mut project = demo_project();
    // The Drums lane's own content (11), placed again out at 256 beats, so the arrangement
    // ends at 512: the export takes seconds, and a test steps through the window while it
    // is inside the running export, not at its race's end.
    project.arrangement.lanes[0].clips.push(suhina_engine::model::Clip::new(
        suhina_engine::model::ContentId(11),
        256.0,
        256.0,
    ));
    project
}

fn long_demo_app() -> App {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(long_demo_project()), &[]);
    app
}

/// The demo with the clip menus' name field on (Settings › Arrangement › Name field in clip
/// menus), as the tests of renaming in the menu use it.
fn demo_app_with_name_field() -> App {
    let mut app = demo_app();
    let mut settings = app.settings().clone();
    settings.view.menu_name_field = true;
    app.set_settings(&egui::Context::default(), settings);
    app
}

#[test]
fn turning_a_track_off_is_an_edit_that_undoes() {
    let mut harness = window(demo_app());
    let muted = |h: &Harness<'static, App>| h.state().session().map(|s| s.project().channels[1].mute);
    assert_eq!(muted(&harness), Some(false));

    harness.get_by_label("Turn Bass off").click();
    harness.run();
    assert_eq!(muted(&harness), Some(true));
    assert!(harness.query_by_label("Turn Bass on").is_some());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(muted(&harness), Some(false));
}

#[test]
fn launching_without_an_audio_engine_says_why() {
    let mut harness = window(demo_app());
    harness.get_by_label("Launch Beat A on Drums (scene 1)").click();
    harness.run();
    match harness.state().status() {
        Status::Error(message) => assert!(message.contains("no audio engine"), "{message}"),
        other => panic!("expected an error, got {other:?}"),
    }
}

/// An audio track's strip, grown past the input block: it carries "Audio From" with what it is,
/// and the three monitor picks with the on one lit; the strip's picks run the same
/// `track.setAudioInput` the track menu's do. A notes track's strip has nothing of it.
#[test]
fn an_audio_tracks_strip_shows_audio_from_and_the_monitor_picks() {
    use suhina_engine::model::{ContentKinds, Monitor};
    let mut project = demo_project();
    project.tracks[0].content = ContentKinds { notes: false, audio: true };
    let name = project.tracks[0].name.clone();
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(project), &[]);
    let mut settings = suhina_ui::settings::Settings::default();
    settings.view.mixer_height = 320;
    app.set_settings(&egui::Context::default(), settings);
    let mut harness = window(app);

    // The rows are there: the source (the test's device has no channels, so eight are offered)
    // and the three monitor buttons.
    assert!(harness.query_by_label_contains(&format!("{name} audio from: No input")).is_some());
    assert!(harness.query_by_label(&format!("{name} monitor In")).is_some());
    assert!(harness.query_by_label(&format!("{name} monitor Auto")).is_some());
    assert!(harness.query_by_label(&format!("{name} monitor Off")).is_some());
    assert!(
        harness.query_by_label_contains("Keys audio from:").is_none(),
        "a track that plays notes has no audio block"
    );

    // The dropdown offers the same list the track menu's does (`audio_input_picks`, and that
    // menu is driven by the track-menu test and the hand check: under kittest's window the
    // strip's popup itself does not open — its picks are the menu's). The monitor's buttons
    // are the strip's own: driven here, with the project as the outcome.
    harness.get_by_label(&format!("{name} monitor In")).click();
    harness.run();
    let heard = |h: &Harness<'static, App>| h.state().session().map(|s| s.project().tracks[0].audio_input);
    assert_eq!(heard(&harness).map(|i| i.monitor), Some(Monitor::In));
    harness.get_by_label(&format!("{name} monitor In")).click();
    harness.run();
    assert_eq!(heard(&harness).map(|i| i.monitor), Some(Monitor::In), "the re-click changes nothing");
    // One undo takes the monitor back; clicking Auto sets it again.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(heard(&harness).map(|i| i.monitor), Some(Monitor::Auto));
    harness.get_by_label(&format!("{name} monitor Auto")).click();
    harness.run();
    assert_eq!(heard(&harness).map(|i| i.monitor), Some(Monitor::Auto));
}

#[test]
fn the_plugin_key_says_when_a_track_has_no_running_plug_in() {
    let mut harness = window(demo_app());
    // No device panel selection and no plug-in on the demo's tracks: the key says so.
    harness.get_by_label("Track Bass").click();
    harness.run();
    harness.key_press(Key::P);
    harness.run();
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.contains("no running plug-in")),
        "{:?}",
        harness.state().status()
    );
    // The action is on the Shortcuts page under its default key, mappable like any other
    // (the keymap tests cover the default and the conflicts).
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Keys").click();
    harness.run();
    assert!(harness.query_by_label_contains("Open or close the selected track's plug-in editor").is_some());
    harness.key_press(Key::Escape);
    harness.run();
}

#[test]
fn an_empty_window_asks_for_a_project() {
    let harness = window(App::new());
    assert!(harness.state().session().is_none());
    assert!(matches!(harness.state().status(), Status::Hint(_)));
}

#[test]
fn tab_and_the_view_switch_change_the_view() {
    let mut harness = window(demo_app());
    assert_eq!(harness.state().view(), View::Session);
    harness.key_press(Key::Tab);
    harness.run();
    assert_eq!(harness.state().view(), View::Arrangement);
    assert!(harness.query_by_label("Clip Beat B on Drums at bar 17").is_some());
    harness.get_by_label("Session view").click();
    harness.run();
    assert_eq!(harness.state().view(), View::Session);
}

fn arrangement_app() -> Harness<'static, App> {
    let mut harness = window(demo_app());
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness
}

#[test]
fn dragging_a_clip_moves_it_on_the_bar_grid() {
    let mut harness = arrangement_app();
    let from = harness.get_by_label("Clip Beat B on Drums at bar 17").rect().center_top() + egui::vec2(0.0, 6.0);
    // 8 points a beat by default, so 64 points are two bars.
    let to = from + egui::vec2(64.0, 0.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + egui::vec2(30.0, 0.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    let starts: Vec<f64> = harness
        .state()
        .session()
        .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| c.start).collect())
        .unwrap_or_default();
    assert!(starts.contains(&72.0), "{starts:?}");
    assert!(!starts.contains(&64.0), "{starts:?}");
}

#[test]
fn moving_the_view_with_the_middle_button_over_a_clip_leaves_the_clip_alone() {
    let mut harness = arrangement_app();
    let starts = |h: &Harness<'static, App>| {
        h.state().session().map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| c.start).collect::<Vec<_>>())
    };
    let before = starts(&harness);
    let from = harness.get_by_label("Clip Beat B on Drums at bar 17").rect().center_top() + egui::vec2(0.0, 6.0);
    let button = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Middle,
        pressed,
        modifiers: Modifiers::NONE,
    };
    harness.hover_at(from);
    harness.run();
    harness.input_mut().events.push(button(from, true));
    harness.run();
    for step in 1..=4 {
        harness.hover_at(from + egui::vec2(-20.0 * step as f32, 0.0));
        harness.run();
    }
    let to = from + egui::vec2(-80.0, 0.0);
    harness.input_mut().events.push(button(to, false));
    harness.run();
    assert_eq!(starts(&harness), before);
}

/// Clicks the name bar of the clip labelled `label`, which picks it whole.
fn click_name_bar(harness: &mut Harness<'static, App>, label: &str) {
    let at = harness.get_by_label(label).rect().center_top() + egui::vec2(0.0, 6.0);
    harness.hover_at(at);
    harness.run();
    harness.drag_at(at);
    harness.drop_at(at);
    harness.run();
}

/// Drags the clip labelled `label` by `by` points in the Arrangement view.
fn drag_clip(harness: &mut Harness<'static, App>, label: &str, by: egui::Vec2) {
    // By its name bar: its body selects time.
    let from = harness.get_by_label(label).rect().center_top() + egui::vec2(0.0, 6.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + by * 0.5);
    harness.run();
    harness.hover_at(from + by);
    harness.run();
    harness.drop_at(from + by);
    harness.run();
}

#[test]
fn a_dropped_clip_replaces_what_it_covers_unless_midi_stacking_is_on() {
    let starts = |h: &Harness<'static, App>| {
        h.state()
            .session()
            .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>())
    };
    // Beat B (bars 17-28) moves two bars later, over Fill (bars 29-32), which is shortened.
    let mut harness = arrangement_app();
    drag_clip(&mut harness, "Clip Beat B on Drums at bar 17", egui::vec2(64.0, 0.0));
    assert_eq!(starts(&harness).map(|s| s.contains(&(120.0, 8.0))), Some(true));

    let mut harness = arrangement_app();
    let settings = suhina_ui::settings::Settings { midi_stacking: true, ..Default::default() };
    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, settings);
    harness.run();
    drag_clip(&mut harness, "Clip Beat B on Drums at bar 17", egui::vec2(64.0, 0.0));
    assert_eq!(starts(&harness).map(|s| s.contains(&(112.0, 16.0))), Some(true));
}

#[test]
fn dragging_a_clip_down_moves_it_to_the_next_track() {
    let mut harness = arrangement_app();
    let from = harness.get_by_label("Clip Beat B on Drums at bar 17").rect().center_top() + egui::vec2(0.0, 6.0);
    // Lanes are 48 points high with a line between them.
    let to = from + egui::vec2(0.0, 49.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + egui::vec2(0.0, 20.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    let project = harness.state().session().map(|s| s.project().clone()).unwrap_or_default();
    let drums: Vec<f64> = project.arrangement.lanes[0].clips.iter().map(|c| c.start).collect();
    let bass: Vec<f64> = project.arrangement.lanes[1].clips.iter().map(|c| c.start).collect();
    assert!(!drums.contains(&64.0), "{drums:?}");
    assert!(bass.contains(&64.0), "{bass:?}");
}

/// The demo project in the Arrangement view with `renderer` (stopped, made from the demo
/// project) as its player, driven by the test instead of an audio device.
fn arrangement_app_with_engine(renderer: &mut suhina_engine::Renderer) -> Harness<'static, App> {
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_session(session, &[]);
    let mut harness = window(app);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness
}

/// Renders a block, so the engine takes the commands sent so far.
fn render_a_block(renderer: &mut suhina_engine::Renderer) {
    renderer.process(&mut [0.0f32; 2 * 64]);
}

/// Markers under the ruler: M adds one at the play start marker, a right-click on the marker row
/// adds one there on the grid, a click on a marker jumps to it and loops to the next marker, its
/// menu deletes it.
#[test]
fn markers_are_added_jumped_to_and_deleted_from_the_marker_row() {
    let mut renderer =
        suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut harness = arrangement_app_with_engine(&mut renderer);
    let monitor = renderer.monitor();
    let markers = |h: &Harness<'static, App>| -> Vec<(f64, String)> {
        h.state()
            .session()
            .map_or(Vec::new(), |s| s.project().arrangement.markers.iter().map(|m| (m.beat, m.name.clone())).collect())
    };
    // The play start marker to bar 5 (beat 16), then M adds a marker there.
    let ruler = harness.get_by_label("Ruler").rect();
    let at = egui::pos2(ruler.left() + 130.0, ruler.center().y);
    harness.hover_at(at);
    harness.run();
    harness.drag_at(at);
    harness.drop_at(at);
    harness.run();
    render_a_block(&mut renderer);
    harness.key_press(Key::M);
    harness.run();
    assert_eq!(markers(&harness), [(16.0, "Marker 1".to_string())]);

    // A right-click in the middle of the row offers a marker on the bar there (8 points a beat).
    let row = harness.get_by_label("Markers").rect();
    let bar = ((f64::from(row.center().x - row.left()) / 8.0) / 4.0).round() * 4.0;
    assert!(bar > 16.0, "{bar}");
    harness.get_by_label("Markers").click_secondary();
    harness.run();
    harness.get_by_label("Add marker here").click();
    harness.run();
    assert_eq!(markers(&harness), [(16.0, "Marker 1".to_string()), (bar, "Marker 2".to_string())]);

    // A click on a marker moves the play start marker there.
    harness.get_by_label("Marker Marker 2").click();
    harness.run();
    render_a_block(&mut renderer);
    assert_eq!(monitor.start_beat(), bar);
    let looped = |h: &Harness<'static, App>| {
        let transport = h.state().session().map(|s| s.project().transport.clone()).unwrap_or_default();
        (transport.loop_range.map(|r| (r.start, r.end)), transport.loop_enabled)
    };
    assert_ne!(looped(&harness), (Some((16.0, bar)), true), "the last marker only jumps");

    // A click on a marker with one after it loops from it to the next.
    harness.get_by_label("Marker Marker 1").click();
    harness.run();
    render_a_block(&mut renderer);
    assert_eq!(looped(&harness), (Some((16.0, bar)), true));
    assert_eq!(monitor.start_beat(), 16.0);

    // Its menu deletes it, and undo brings it back.
    harness.get_by_label("Marker Marker 1").click_secondary();
    harness.run();
    harness.get_by_label("Delete marker").click();
    harness.run();
    assert_eq!(markers(&harness), [(bar, "Marker 2".to_string())]);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(markers(&harness).len(), 2);

    // Dragged two bars to the right (64 points), it lands on the bar grid there.
    let from = harness.get_by_label("Marker Marker 1").rect().left_center() + egui::vec2(4.0, 0.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + egui::vec2(30.0, 0.0));
    harness.run();
    harness.hover_at(from + egui::vec2(64.0, 0.0));
    harness.run();
    harness.drop_at(from + egui::vec2(64.0, 0.0));
    harness.run();
    assert_eq!(markers(&harness)[0], (24.0, "Marker 1".to_string()));
}

/// The loop brace row under the ruler: a drag draws a region on the bar grid and turns looping
/// on, Ctrl+L and a click on the brace turn it off and on, its end and its middle are dragged,
/// and undo takes an edit back (`docs/spec/arrangement-editing.md` › Loop region).
#[test]
fn a_loop_region_is_drawn_resized_moved_and_toggled_in_the_loop_brace_row() {
    let mut renderer =
        suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut harness = arrangement_app_with_engine(&mut renderer);
    let looped = |h: &Harness<'static, App>| {
        let transport = h.state().session().map(|s| s.project().transport.clone()).unwrap_or_default();
        (transport.loop_range.map(|r| (r.start, r.end)), transport.loop_enabled)
    };
    let drag = |h: &mut Harness<'static, App>, from: egui::Pos2, by: f32| {
        h.hover_at(from);
        h.run();
        h.drag_at(from);
        h.run();
        h.hover_at(from + egui::vec2(by / 2.0, 0.0));
        h.run();
        h.hover_at(from + egui::vec2(by, 0.0));
        h.run();
        h.drop_at(from + egui::vec2(by, 0.0));
        h.run();
    };
    // 8 points a beat: from beat 4 to beat 12, a little off the bars, lands on them.
    let row = harness.get_by_label("Loop brace").rect();
    let at_beat = |beat: f32| egui::pos2(row.left() + beat * 8.0, row.center().y);
    drag(&mut harness, at_beat(4.0) + egui::vec2(3.0, 0.0), 8.0 * 8.0 - 5.0);
    assert_eq!(looped(&harness), (Some((4.0, 12.0)), true));

    // Ctrl+L turns it off, a click on the brace on again.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::L);
    harness.run();
    assert_eq!(looped(&harness), (Some((4.0, 12.0)), false));
    harness.hover_at(at_beat(8.0));
    harness.run();
    harness.drag_at(at_beat(8.0));
    harness.drop_at(at_beat(8.0));
    harness.run();
    assert_eq!(looped(&harness), (Some((4.0, 12.0)), true));

    // The end dragged two bars right, then the whole brace one bar right.
    drag(&mut harness, at_beat(12.0), 64.0);
    assert_eq!(looped(&harness), (Some((4.0, 20.0)), true));
    drag(&mut harness, at_beat(10.0), 32.0);
    assert_eq!(looped(&harness), (Some((8.0, 24.0)), true));

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(looped(&harness), (Some((4.0, 20.0)), true));
}

#[test]
fn a_click_on_the_ruler_moves_the_play_start_marker_on_the_grid() {
    let mut renderer =
        suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut harness = arrangement_app_with_engine(&mut renderer);
    let monitor = renderer.monitor();
    let ruler = harness.get_by_label("Ruler").rect();
    // 8 points a beat by default, so the grid is a bar: 130 points in is beat 16.25, bar 5.
    let at = egui::pos2(ruler.left() + 130.0, ruler.center().y);
    harness.hover_at(at);
    harness.run();
    harness.drag_at(at);
    harness.drop_at(at);
    harness.run();
    render_a_block(&mut renderer);
    assert_eq!(monitor.start_beat(), 16.0);
    assert!(!monitor.running());

    // Play starts there; Stop goes back to it, as the setting says by default.
    harness.get_by_label("Play").click();
    harness.run();
    render_a_block(&mut renderer);
    assert!(monitor.running() && monitor.beat() >= 16.0);
    for _ in 0..400 {
        render_a_block(&mut renderer);
    }
    harness.get_by_label("Stop").click();
    // While playing the window repaints on its own: step it instead of running it to rest.
    harness.run_steps(2);
    render_a_block(&mut renderer);
    assert_eq!((monitor.running(), monitor.start_beat(), monitor.beat()), (false, 16.0, 16.0));

    // With Stop set to stay, the marker moves to where it stopped.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Arrangement").click();
    harness.run();
    harness.get_by_label("Stop: Stay").click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_changed_settings().map(|s| s.stop),
        Some(suhina_ui::settings::StopReturn::Here)
    );
    harness.get_by_label("Close settings").click();
    harness.run();
    harness.get_by_label("Play").click();
    harness.run();
    for _ in 0..375 {
        render_a_block(&mut renderer);
    }
    harness.get_by_label("Stop").click();
    // While playing the window repaints on its own: step it instead of running it to rest.
    harness.run_steps(2);
    render_a_block(&mut renderer);
    // 375 blocks of 64 frames at 120 bpm are one beat, and the blocks around Play and Stop.
    assert!((monitor.start_beat() - 17.0).abs() < 0.03, "{}", monitor.start_beat());
}

/// The demo project with only Drums heard, held notes and no sends, so what is heard is one
/// track's pan and volume.
fn drums_alone() -> suhina_engine::model::Project {
    let mut project = demo_project();
    for channel in &mut project.channels {
        channel.sends.clear();
        channel.mute = channel.id.0 != 1 && channel.id.0 <= 4;
    }
    for content in &mut project.contents {
        for lane in &mut content.lanes {
            if let suhina_engine::model::LaneData::Notes(notes) = &mut lane.data {
                notes.iter_mut().for_each(|n| n.duration = 1.0);
            }
        }
    }
    project
}

/// The level of the left and right side over the next quarter of a second of `renderer`.
fn sides(renderer: &mut suhina_engine::Renderer) -> (f64, f64) {
    let mut out = vec![0.0f32; 2 * 12_000];
    for block in out.chunks_mut(2 * 64) {
        renderer.process(block);
    }
    let rms = |side: usize| {
        let sum: f64 = out.iter().skip(side).step_by(2).map(|s| f64::from(*s).powi(2)).sum();
        (sum / 12_000.0).sqrt()
    };
    (rms(0), rms(1))
}

/// Starts playing `renderer` from the window and moves the pointer onto `label`, pressed.
fn press_while_playing(harness: &mut Harness<'static, App>, renderer: &mut suhina_engine::Renderer, label: &str) {
    harness.get_by_label("Play").click();
    // While playing the window repaints on its own: step it instead of running it to rest.
    harness.run_steps(2);
    let at = harness.get_by_label(label).rect().center();
    harness.hover_at(at);
    harness.run_steps(1);
    harness.drag_at(at);
    harness.run_steps(1);
    sides(renderer);
}

#[test]
fn a_pan_drag_is_heard_while_dragging_and_is_one_undo_step() {
    let project = drums_alone();
    let mut renderer = suhina_engine::Renderer::stopped(&project, &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut session = Session::new(project);
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    // Frames close enough together for a double-click.
    let mut harness = window_with_step(app, 1.0 / 60.0);
    let pan = |h: &Harness<'static, App>| h.state().session().map_or(f64::NAN, |s| s.project().channels[0].pan);

    press_while_playing(&mut harness, &mut renderer, "Drums pan");
    let (left, right) = sides(&mut renderer);
    assert!(left > 0.01 && (left / right - 1.0).abs() < 0.01, "{left} {right}");
    let knob = harness.get_by_label("Drums pan").rect().center();
    // Up raises the pan, 0.01 a point; the button is still held after each step.
    let mut heard = Vec::new();
    for up in [20.0, 40.0, 60.0] {
        harness.hover_at(knob - egui::vec2(0.0, up));
        harness.run_steps(1);
        let (left, right) = sides(&mut renderer);
        heard.push((pan(&harness), right / left));
    }
    for pair in heard.windows(2) {
        assert!(pair[1].0 > pair[0].0 && pair[1].1 > pair[0].1, "louder on the right as it pans: {heard:?}");
    }
    // At a pan of p the sides are cos and sin of (p + 1) * 45 degrees.
    for &(p, ratio) in &heard {
        let want = ((p + 1.0) * std::f64::consts::FRAC_PI_4).tan();
        assert!(p > 0.0 && (ratio - want).abs() < 0.05 * want, "{p}: heard {ratio}, want {want}");
    }
    harness.drop_at(knob - egui::vec2(0.0, 60.0));
    harness.run_steps(2);
    let dropped = pan(&harness);
    assert!((dropped - heard[2].0).abs() < 1e-9, "{dropped} {heard:?}");

    // The whole drag is one step: one undo goes back to the centre, and there is nothing older.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run_steps(2);
    assert_eq!(pan(&harness), 0.0);
    assert!(harness.state().session().is_some_and(|s| !s.can_undo()));
    let (left, right) = sides(&mut renderer);
    assert!((left / right - 1.0).abs() < 0.01, "{left} {right}");

    // Redo brings the drag's end back; a double-click then centres the knob as a step of its own.
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    harness.run_steps(2);
    assert_eq!(pan(&harness), dropped);
    // Half a second later, as a person would, so the clicks do not count with the drag.
    harness.run_steps(30);
    for _ in 0..2 {
        harness.hover_at(knob);
        harness.drag_at(knob);
        harness.drop_at(knob);
        harness.step();
    }
    harness.run_steps(2);
    assert_eq!(pan(&harness), 0.0);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run_steps(2);
    assert_eq!(pan(&harness), dropped);
}

#[test]
fn a_volume_drag_in_the_arrangement_is_heard_while_dragging_and_is_one_undo_step() {
    let project = drums_alone();
    let mut renderer = suhina_engine::Renderer::stopped(&project, &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut session = Session::new(project);
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = window(app);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    let volume = |h: &Harness<'static, App>| h.state().session().map_or(f64::NAN, |s| s.project().channels[0].volume);

    press_while_playing(&mut harness, &mut renderer, "Drums volume");
    let (before, _) = sides(&mut renderer);
    let field = harness.get_by_label("Drums volume").rect().center();
    // Down lowers the volume, 0.25 dB a point: 24 points is about -6 dB, 48 about -12 dB.
    let mut heard = Vec::new();
    for down in [24.0, 48.0] {
        harness.hover_at(field + egui::vec2(0.0, down));
        harness.run_steps(1);
        let (left, _) = sides(&mut renderer);
        heard.push((volume(&harness), left / before));
    }
    for &(gain, ratio) in &heard {
        assert!(gain < 0.6 && (ratio - gain).abs() < 0.02, "heard {ratio} at a gain of {gain}: {heard:?}");
    }
    assert!(heard[1].0 < heard[0].0, "{heard:?}");
    harness.drop_at(field + egui::vec2(0.0, 48.0));
    harness.run_steps(2);

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run_steps(2);
    assert_eq!(volume(&harness), 1.0);
    assert!(harness.state().session().is_some_and(|s| !s.can_undo()));
}

#[test]
fn play_starts_from_the_selected_clip_with_shift_space_or_the_setting() {
    let mut renderer =
        suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut harness = arrangement_app_with_engine(&mut renderer);
    let monitor = renderer.monitor();
    let stop = |harness: &mut Harness<'static, App>, renderer: &mut suhina_engine::Renderer| {
        harness.get_by_label("Stop").click();
        harness.run_steps(2);
        render_a_block(renderer);
    };

    // By default Play starts from the selected clip: picked whole, by its name bar.
    click_name_bar(&mut harness, "Clip Beat B on Drums at bar 17");
    harness.get_by_label("Play").click();
    harness.run_steps(2);
    render_a_block(&mut renderer);
    assert_eq!((monitor.running(), monitor.start_beat()), (true, 64.0));
    stop(&mut harness, &mut renderer);

    // Shift+Space plays from the selected clip's start (bar 17, beat 64), and Stop returns there.
    harness.key_press_modifiers(Modifiers::SHIFT, Key::Space);
    harness.run_steps(2);
    render_a_block(&mut renderer);
    assert_eq!((monitor.running(), monitor.start_beat()), (true, 64.0));
    for _ in 0..100 {
        render_a_block(&mut renderer);
    }
    stop(&mut harness, &mut renderer);
    assert_eq!((monitor.running(), monitor.beat()), (false, 64.0));

    // With Play from set to the selected clip, Space and the Play button start there too.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Arrangement").click();
    harness.run();
    harness.get_by_label("Play from: Marker").click();
    harness.run();
    harness.get_by_label("Play from: Selected clip").click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_changed_settings().map(|s| s.play_from),
        Some(suhina_ui::settings::PlayFrom::Clip)
    );
    harness.get_by_label("Close settings").click();
    harness.run();
    click_name_bar(&mut harness, "Clip Beat A on Drums at bar 1");
    harness.key_press(Key::Space);
    harness.run_steps(2);
    render_a_block(&mut renderer);
    assert_eq!((monitor.running(), monitor.start_beat()), (true, 0.0));
    stop(&mut harness, &mut renderer);
    click_name_bar(&mut harness, "Clip Beat B on Drums at bar 17");
    harness.get_by_label("Play").click();
    harness.run_steps(2);
    render_a_block(&mut renderer);
    assert_eq!((monitor.running(), monitor.start_beat()), (true, 64.0));
    stop(&mut harness, &mut renderer);

    // A click in the body of the selected clip sets where play starts, the clip still picked:
    // two bars in (8 points a beat), Play starts there and not from the clip's start.
    let body = harness.get_by_label("Clip Beat B on Drums at bar 17").rect().left_center() + egui::vec2(64.0, 0.0);
    harness.hover_at(body);
    harness.run();
    harness.drag_at(body);
    harness.drop_at(body);
    harness.run_steps(2);
    assert!(harness.state().arrangement().selected_clip().is_some(), "the clip stays picked");
    harness.get_by_label("Play").click();
    harness.run_steps(2);
    render_a_block(&mut renderer);
    assert_eq!((monitor.running(), monitor.start_beat()), (true, 72.0));
    stop(&mut harness, &mut renderer);

    // In the Session view the selection is not shown, so Play starts from the marker; stopping
    // again first puts the marker at the beginning.
    stop(&mut harness, &mut renderer);
    assert_eq!(monitor.start_beat(), 0.0);
    harness.get_by_label("Session view").click();
    harness.run();
    harness.key_press(Key::Space);
    harness.run_steps(2);
    render_a_block(&mut renderer);
    assert_eq!((monitor.running(), monitor.start_beat()), (true, 0.0));
}

#[test]
fn arrangement_headers_turn_tracks_off() {
    let mut harness = arrangement_app();
    harness.get_by_label("Turn Keys off").click();
    harness.run();
    assert_eq!(harness.state().session().map(|s| s.project().channels[2].mute), Some(true));
    assert!(harness.query_by_label("Keys volume").is_some());
    assert!(harness.query_by_label("Keys pan").is_some());
}

#[test]
fn ctrl_and_plus_or_minus_zoom_the_whole_window_and_the_zoom_is_saved() {
    let mut harness = window(demo_app());
    assert_eq!(harness.ctx.zoom_factor(), 1.0);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Plus);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Equals);
    harness.run();
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.zoom), Some(120));
    assert!((harness.ctx.zoom_factor() - 1.2).abs() < 1e-6);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Minus);
    harness.run();
    assert_eq!(harness.state().settings().zoom, 110);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Num0);
    harness.run();
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.zoom), Some(100));
    assert_eq!(harness.ctx.zoom_factor(), 1.0);
}

#[test]
fn a_saved_zoom_is_used_from_the_start_and_settings_can_change_it() {
    let mut app = demo_app();
    let ctx = egui::Context::default();
    let settings = suhina_ui::settings::Settings { zoom: 150, ..Default::default() };
    app.set_settings(&ctx, settings);
    let mut harness = window(app);
    assert!((harness.ctx.zoom_factor() - 1.5).abs() < 1e-6);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Appearance").click();
    harness.run();
    harness.get_by_label("Zoom: -").click();
    harness.run();
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.zoom), Some(140));
    harness.get_by_label("Zoom: 140 %").click();
    harness.run();
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.zoom), Some(100));
    assert_eq!(harness.ctx.zoom_factor(), 1.0);
}

#[test]
fn settings_change_right_away_and_are_handed_out_to_be_saved() {
    let mut harness = window(demo_app());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    assert!(harness.state().settings_open());
    harness.get_by_label("Settings page: Appearance").click();
    harness.run();
    harness.get_by_label("Animations").click();
    harness.run();
    harness.get_by_label("Settings page: Arrangement").click();
    harness.run();
    harness.get_by_label("Track headers: Left").click();
    harness.run();
    let saved = harness.state_mut().take_changed_settings();
    assert_eq!(saved.as_ref().map(|s| s.animations), Some(false));
    assert_eq!(saved.map(|s| s.track_headers), Some(suhina_ui::settings::Side::Left));
    assert!(!suhina_ui::kit::motion::enabled(&harness.ctx));
    assert_eq!(harness.state_mut().take_changed_settings(), None);
    harness.get_by_label("Close settings").click();
    harness.run();
    assert!(!harness.state().settings_open());
}

#[test]
fn a_theme_picked_in_settings_colours_the_window_and_is_saved() {
    let mut harness = window(demo_app());
    let mut colors = theme::Palette::DARK;
    colors.accent = egui::Color32::from_rgb(0x12, 0xAB, 0x9F);
    let night = suhina_ui::settings::themes::ThemeEntry {
        id: "night".into(),
        theme: theme::Theme { name: "Night".into(), colors },
    };
    let ctx = harness.ctx.clone();
    harness.state_mut().set_themes(&ctx, vec![night], Some("themes".into()));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Theme").click();
    harness.run();
    assert!(harness.query_by_label("Theme: Suhina Dark").is_some());
    harness.get_by_label("Theme: Night").click();
    harness.run();
    assert_eq!(theme::accent(), colors.accent);
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.theme), Some("night".into()));

    // A copy to change becomes the theme in use, and is handed out to be written.
    harness.get_by_label("Duplicate theme").click();
    harness.run();
    let saved: Vec<_> = harness
        .state_mut()
        .take_theme_actions()
        .into_iter()
        .map(|a| match a {
            FileAction::Save { id, theme } => (id, theme.name),
            other => (format!("{other:?}"), String::new()),
        })
        .collect();
    assert_eq!(saved, [("night-copy".to_string(), "Night copy".to_string())]);
    assert_eq!(harness.state().settings().theme, "night-copy");
    for button in ["Reload themes", "Open the themes folder", "Back up themes", "Export the theme template"] {
        harness.get_by_label(button).click();
        harness.run();
    }
    assert_eq!(
        harness.state_mut().take_theme_actions(),
        [FileAction::Reload, FileAction::OpenFolder, FileAction::Backup, FileAction::ExportTemplate]
    );

    // A theme file dropped on the window is handed out to be brought in.
    harness.input_mut().dropped_files.push(std::sync::Arc::new(TestFile("dark blue.json".into())));
    harness.run();
    assert_eq!(harness.state_mut().take_theme_actions(), [FileAction::Import("dark blue.json".into())]);

    // A file saved elsewhere redraws the window.
    colors.accent = egui::Color32::from_rgb(1, 2, 3);
    let edited = suhina_ui::settings::themes::ThemeEntry {
        id: "night-copy".into(),
        theme: theme::Theme { name: "Night copy".into(), colors },
    };
    harness.state_mut().put_theme(&ctx, edited);
    assert_eq!(theme::accent(), colors.accent);
}

#[test]
fn tab_only_switches_the_view() {
    let mut harness = window(demo_app());
    harness.run();
    for expected in [View::Arrangement, View::Session, View::Arrangement] {
        harness.key_press(Key::Tab);
        harness.run();
        assert_eq!(harness.state().view(), expected);
        // No widget took the keyboard, so the next Tab is a shortcut again.
        assert!(!harness.ctx.egui_wants_keyboard_input());
    }
    for key in [Key::ArrowDown, Key::ArrowRight] {
        harness.key_press(key);
        harness.run();
        assert!(!harness.ctx.egui_wants_keyboard_input());
    }
}

#[test]
fn ctrl_t_adds_an_audio_track_and_ctrl_shift_t_a_midi_one() {
    let mut harness = window(demo_app());
    // (audio tracks, MIDI tracks)
    let kinds = |h: &Harness<'static, App>| {
        let p = h.state().session().unwrap().project();
        (p.tracks.iter().filter(|t| t.content.audio).count(), p.tracks.iter().filter(|t| t.content.notes).count())
    };
    let (audio, midi) = kinds(&harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::T);
    harness.run();
    assert_eq!(kinds(&harness), (audio + 1, midi), "Ctrl+T: an audio track");
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::T);
    harness.run();
    assert_eq!(kinds(&harness), (audio + 1, midi + 1), "Ctrl+Shift+T: a MIDI track");
}

#[test]
fn ctrl_n_asks_for_a_new_project_and_ctrl_s_saves_one() {
    let dir = std::env::temp_dir().join(format!("suhina-ui-save-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&dir);
    let mut app = demo_app();
    app.set_default_folders(dir.join("Projects"), dir.join("Templates"));
    let mut harness = window(app);

    // Ctrl+N: the dialog, which asks for its lists; a blank project is handed out to be made.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::N);
    harness.run();
    assert!(harness.state().project_dialog_open());
    assert_eq!(harness.state_mut().take_lists_wanted(), Some(dir.join("Templates")));
    harness.get_by_label("New blank project").click();
    harness.run();
    assert_eq!(harness.state_mut().take_new_request(), Some(suhina_ui::NewProject::Blank));
    assert!(!harness.state().project_dialog_open());

    // Ctrl+S on a project without a .suhina file asks for a name, in the projects folder.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::S);
    harness.run();
    harness.get_by_label("Name").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Name").type_text("Night drive");
    harness.run();
    harness.get_by_label("Save").click();
    harness.run();
    let saved = dir.join("Projects").join("Night drive.suhina");
    assert!(saved.is_file(), "{:?}", harness.state().status());
    assert_eq!(harness.state_mut().take_saved_path(), Some(saved.clone()));
    let session = harness.state().session();
    assert_eq!(session.and_then(|s| s.source()), Some(saved.as_path()));

    // Saved again without asking; a template is a copy in the templates folder.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::S);
    harness.run();
    assert!(harness.query_by_label("Name").is_none(), "asked again");
    harness.state_mut().do_action("file.saveTemplate", &mut Vec::new());
    harness.run();
    harness.get_by_label("Save").click();
    harness.run();
    assert!(dir.join("Templates").join("Night drive.suhina").is_file());
    assert_eq!(harness.state().session().and_then(|s| s.source()), Some(saved.as_path()));
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn solo_is_exclusive_unless_ctrl_is_held() {
    let mut harness = window(demo_app());
    let solos = |h: &Harness<'static, App>| {
        h.state().session().map(|s| s.project().channels.iter().take(4).map(|c| c.solo).collect::<Vec<_>>())
    };
    harness.get_by_label("Solo Drums").click();
    harness.run();
    harness.get_by_label("Solo Keys").click();
    harness.run();
    assert_eq!(solos(&harness), Some(vec![false, false, true, false]));
    harness.get_by_label("Solo Vox").click_modifiers(Modifiers::COMMAND);
    harness.run();
    assert_eq!(solos(&harness), Some(vec![false, false, true, true]));

    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label("End solo on Keys").click();
    harness.run();
    assert_eq!(solos(&harness), Some(vec![false, false, false, true]));
}

#[test]
fn the_cue_button_cues_a_track_in_both_views_and_undoes() {
    let mut harness = window(demo_app());
    let cues = |h: &Harness<'static, App>| {
        h.state().session().map(|s| s.project().channels.iter().take(4).map(|c| c.cue).collect::<Vec<_>>())
    };
    harness.get_by_label("Cue Keys on the headphones").click();
    harness.run();
    harness.get_by_label("Cue Drums on the headphones").click();
    harness.run();
    // Not exclusive: several tracks can be cued at once.
    assert_eq!(cues(&harness), Some(vec![true, false, true, false]));

    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label("Stop cueing Keys").click();
    harness.run();
    assert_eq!(cues(&harness), Some(vec![true, false, false, false]));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(cues(&harness), Some(vec![true, false, true, false]));
}

#[test]
fn arming_is_exclusive_unless_ctrl_is_held_and_changes_no_project() {
    let mut harness = window(demo_app());
    let armed = |h: &Harness<'static, App>| {
        h.state().session().map(|s| s.project().tracks.iter().take(4).map(|t| s.is_armed(t.id)).collect::<Vec<_>>())
    };
    harness.get_by_label("Arm Drums for recording").click();
    harness.run();
    harness.get_by_label("Arm Keys for recording").click();
    harness.run();
    assert_eq!(armed(&harness), Some(vec![false, false, true, false]));
    harness.get_by_label("Arm Vox for recording").click_modifiers(Modifiers::COMMAND);
    harness.run();
    assert_eq!(armed(&harness), Some(vec![false, false, true, true]));
    assert!(!harness.state().session().is_some_and(Session::can_undo), "arming is not an edit");

    // The Arrangement headers show it and disarm too.
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label("Disarm Keys").click();
    harness.run();
    assert_eq!(armed(&harness), Some(vec![false, false, false, true]));
    assert!(harness.query_by_label("Disarm Vox").is_some());
    // Return tracks play no notes: nothing to arm.
    assert!(harness.query_by_label("Arm A Reverb for recording").is_none());
}

/// MIDI ports by name that open into nothing.
struct StandMidi(Vec<String>);

impl suhina_session::MidiSystem for StandMidi {
    fn ports(&self) -> Result<Vec<String>, String> {
        Ok(self.0.clone())
    }

    fn open(
        &self,
        _name: &str,
        _input: std::sync::Arc<suhina_session::LiveInput>,
    ) -> Result<Box<dyn std::any::Any + Send>, String> {
        Ok(Box::new(()))
    }
}

#[test]
fn a_tracks_menu_chooses_the_midi_port_and_channel_it_hears() {
    use suhina_engine::model::{MidiInput, MidiPort};
    let mut app = demo_app();
    app.projects_mut().set_midi_system(std::sync::Arc::new(StandMidi(vec!["Keys In".into(), "Pads In".into()])));
    let mut harness = window(app);
    let input = |h: &Harness<'static, App>| {
        h.state()
            .session()
            .and_then(|s| s.project().tracks.iter().find(|t| t.name == "Keys").map(|t| t.midi_input.clone()))
    };
    harness.get_by_label("Track Keys").click_secondary();
    harness.run();
    harness.get_by_label_contains("MIDI input: All ports").click();
    harness.run();
    harness.get_by_label("Pads In").click();
    harness.run();
    assert_eq!(
        input(&harness),
        Some(MidiInput { port: MidiPort::Named("Pads In".into()), channel: None, ..Default::default() })
    );

    harness.get_by_label("Track Keys").click_secondary();
    harness.run();
    harness.get_by_label_contains("MIDI input: Pads In").click();
    harness.run();
    harness.get_by_label("Ch 10").click();
    harness.run();
    assert_eq!(
        input(&harness),
        Some(MidiInput { port: MidiPort::Named("Pads In".into()), channel: Some(10), ..Default::default() })
    );
    assert!(harness.state().session().is_some_and(Session::can_undo), "an edit, saved with the project");
}

#[test]
fn an_audio_track_is_armed_to_hear_its_input_and_its_menu_chooses_the_channels() {
    use suhina_engine::model::{AudioInput, ContentKinds, InputChannels, Monitor};
    let mut project = demo_project();
    project.tracks[0].content = ContentKinds { notes: false, audio: true };
    let name = project.tracks[0].name.clone();
    let mut app = App::new();
    app.set_session(Session::new(project), &[]);
    let mut harness = window(app);
    harness.get_by_label(&format!("Arm {name} for recording")).click();
    harness.run();
    assert!(harness.state().session().is_some_and(|s| s.is_armed(suhina_engine::model::TrackId(1))));

    let input = |h: &Harness<'static, App>| h.state().session().map(|s| s.project().tracks[0].audio_input);
    harness.get_by_label(&format!("Track {name}")).click_secondary();
    harness.run();
    harness.get_by_label_contains("Audio input: No input").click();
    harness.run();
    assert!(harness.query_by_label_contains("No input device").is_some(), "says why only eight are offered");
    harness.get_by_label("In 3/4").click();
    harness.run();
    assert_eq!(input(&harness), Some(AudioInput { channels: InputChannels::Stereo(3), monitor: Monitor::Auto }));
    harness.get_by_label(&format!("Track {name}")).click_secondary();
    harness.run();
    harness.get_by_label_contains("Audio input: In 3/4").click();
    harness.run();
    harness.get_by_label("Monitor in").click();
    harness.run();
    assert_eq!(input(&harness), Some(AudioInput { channels: InputChannels::Stereo(3), monitor: Monitor::In }));
    // A track that plays notes has a MIDI input instead.
    harness.get_by_label("Track Keys").click_secondary();
    harness.run();
    assert!(harness.query_by_label_contains("MIDI input:").is_some());
    assert!(harness.query_by_label_contains("Audio input:").is_none());
}

#[test]
fn settings_audio_chooses_the_input_device_and_saves_it() {
    let mut app = demo_app();
    app.projects_mut().set_audio_system(std::sync::Arc::new(StandAudio("Stand")));
    let mut harness = window(app);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Audio").click();
    for _ in 0..3 {
        harness.step();
    }
    harness.get_by_label("Input: Off").click();
    harness.step();
    harness.step();
    harness.get_by_label("Stand In (2 in)").click();
    harness.step();
    harness.step();
    let chosen = harness.state().projects().output_choice().input.clone();
    assert_eq!(chosen, suhina_session::InputChoice::Named("Stand In".into()));
    let saved = harness.state_mut().take_changed_settings().map(|s| s.audio_input);
    assert_eq!(saved.as_deref(), Some("Stand In"));
}

#[test]
fn settings_audio_lists_the_midi_inputs_and_one_turned_off_is_saved() {
    let mut app = demo_app();
    app.projects_mut().set_audio_system(std::sync::Arc::new(StandAudio("Stand")));
    app.projects_mut().set_midi_system(std::sync::Arc::new(StandMidi(vec!["Keys In".into(), "Pads In".into()])));
    let mut harness = window(app);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: MIDI").click();
    for _ in 0..3 {
        harness.step();
    }
    harness.get_by_label("MIDI input Pads In").click();
    harness.step();
    harness.step();
    assert_eq!(harness.state().projects().midi_inputs_off(), ["Pads In"]);
    let saved = harness.state_mut().take_changed_settings().map(|s| s.midi_inputs_off);
    assert_eq!(saved, Some(vec!["Pads In".to_string()]));
}

/// The demo project with a group track, "Group", that tracks can play into.
fn project_with_a_group() -> suhina_engine::model::Project {
    use suhina_engine::model::{Channel, ChannelId, ChannelRole, ContentKinds, Track};
    let mut project = demo_project();
    project.tracks.push(Track {
        id: TrackId(20),
        name: "Group".into(),
        content: ContentKinds::default(),
        channel: Some(ChannelId(120)),
        ..project.tracks[0].clone()
    });
    project.channels.push(Channel {
        id: ChannelId(120),
        name: "Group".into(),
        role: ChannelRole::Submix,
        sends: Vec::new(),
        ..project.channels[0].clone()
    });
    project
}

#[test]
fn a_folded_group_hides_its_columns_in_the_session_view() {
    // Drums (track 1) inside the group.
    let mut project = project_with_a_group();
    project.tracks[0].parent = Some(suhina_engine::model::TrackId(20));
    let group = project.tracks.pop().unwrap();
    project.tracks.insert(0, group);
    let view =
        SessionHarness { grid: Grid::build(&project, None), state: SessionViewState::default(), actions: vec![] };
    let mut harness = Harness::builder().with_size(SIZE).build_ui_state(
        |ui, v: &mut SessionHarness| session_view::show(ui, &v.grid, &[], &mut v.state, &mut v.actions),
        view,
    );
    theme::install(&harness.ctx);
    harness.run();
    assert!(harness.query_by_label("Track Drums").is_some());
    harness.get_by_label("Fold Group").click();
    harness.run();
    assert!(harness.query_by_label("Track Drums").is_none());
    assert!(harness.query_by_label("Track Bass").is_some());
    harness.get_by_label("Unfold Group").click();
    harness.run();
    assert!(harness.query_by_label("Track Drums").is_some());
    assert!(harness.state().actions.is_empty());
}

#[test]
fn a_tall_header_routes_its_track_into_a_group() {
    struct Arrangement {
        timeline: Timeline,
        state: ArrangementViewState,
        actions: Vec<Action>,
    }
    let mut state = ArrangementViewState::default();
    state.set_lane_height(suhina_engine::model::TrackId(2), 90.0);
    let view = Arrangement { timeline: Timeline::build(&project_with_a_group(), None), state, actions: Vec::new() };
    let mut harness = Harness::builder().with_size(SIZE).build_ui_state(
        |ui, v: &mut Arrangement| arrangement_view::show(ui, &v.timeline, &mut v.state, &mut v.actions),
        view,
    );
    theme::install(&harness.ctx);
    harness.run();
    // Only the tall lane shows its output.
    assert_eq!(harness.query_all_by_label_contains("output:").count(), 1);
    harness.get_by_label("Bass output: Main").click();
    harness.run();
    harness.get_by_label("Group").click();
    harness.run();
    assert_eq!(harness.state().actions, [Action::new("track.setOutput", json!({ "track": 2, "to": 20 }))]);
}

fn names(h: &Harness<'static, App>) -> Vec<String> {
    h.state().session().map(|s| s.project().tracks.iter().map(|t| t.name.clone()).collect()).unwrap_or_default()
}

#[test]
fn selected_tracks_group_with_ctrl_g() {
    let mut harness = arrangement_app();
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.get_by_label("Track Bass").click_modifiers(Modifiers::COMMAND);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::G);
    harness.run();
    assert_eq!(names(&harness)[..3], ["Group", "Drums", "Bass"]);
    assert!(harness.query_by_label("Track Group").is_some());
}

#[test]
fn shift_clicking_a_header_takes_the_run_of_tracks_between() {
    let mut harness = arrangement_app();
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.get_by_label("Track Keys").click_modifiers(Modifiers::SHIFT);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::G);
    harness.run();
    assert_eq!(names(&harness)[..4], ["Group", "Drums", "Bass", "Keys"]);
}

/// How many arrangement clips each of the first four tracks has.
fn clip_counts(h: &Harness<'static, App>) -> Vec<usize> {
    h.state()
        .session()
        .map(|s| {
            let p = s.project();
            p.tracks
                .iter()
                .filter_map(|t| p.arrangement.lanes.iter().find(|l| l.track == Some(t.id)))
                .map(|l| l.clips.len())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn shift_clicking_a_clip_takes_the_run_between_and_the_key_mutes_it() {
    let mut harness = arrangement_app();
    harness.get_by_label("Clip Beat A on Drums at bar 1").click();
    harness.run();
    harness.get_by_label("Clip Beat B on Drums at bar 17").click_modifiers(Modifiers::SHIFT);
    harness.run();
    // The run from bar 1 to bar 17 on Drums: the first two clips, and no others.
    harness.key_press(Key::Num0);
    harness.run();
    let muted: Vec<bool> = harness
        .state()
        .session()
        .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| c.enabled).collect())
        .unwrap_or_default();
    assert_eq!(muted, [false, false, true, true], "the run went quiet, the rest plays on");

    // The same key again turns the selection back on.
    harness.key_press(Key::Num0);
    harness.run();
    let back: Vec<bool> = harness
        .state()
        .session()
        .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| c.enabled).collect())
        .unwrap_or_default();
    assert_eq!(back, [true, true, true, true]);
}

#[test]
fn ctrl_clicking_adds_a_clip_and_cut_takes_the_selection_in_one_step() {
    let mut harness = arrangement_app();
    let before = clip_counts(&harness);
    harness.get_by_label("Clip Beat A on Drums at bar 1").click();
    harness.run();
    harness.get_by_label("Clip Beat B on Drums at bar 17").click_modifiers(Modifiers::COMMAND);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::X);
    harness.run();
    assert_eq!(clip_counts(&harness)[0], before[0] - 2, "both clicked clips went together");

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(clip_counts(&harness), before, "one undo brings both back");
}

#[test]
fn select_all_and_delete_clear_the_arrangement_in_one_step() {
    let mut harness = arrangement_app();
    let before = clip_counts(&harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.key_press(Key::Delete);
    harness.run();
    assert!(clip_counts(&harness).iter().all(|n| *n == 0), "{:?}", clip_counts(&harness));

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(clip_counts(&harness), before, "one undo brings the whole arrangement back");
}

#[test]
fn a_clips_menu_mutes_the_clip_it_fell_on() {
    let mut harness = arrangement_app();
    harness.get_by_label("Clip Beat A on Drums at bar 1").click_secondary();
    harness.run();
    harness.get_by_label_contains("Mute").click();
    harness.run();
    assert!(!harness.state().session().is_some_and(|s| s.project().arrangement.lanes[0].clips[0].enabled));

    // With the clip off, the menu offers to turn it back on.
    harness.get_by_label("Clip Beat A on Drums at bar 1").click_secondary();
    harness.run();
    assert!(harness.query_by_label_contains("Unmute").is_some());
}

/// A linear gain in dB, as the engine keeps it.
fn gain_of(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

#[test]
fn alt_and_the_wheel_over_a_fader_change_the_volume_and_the_wheel_alone_does_not() {
    let mut harness = arrangement_app();
    let at = harness.get_by_label("Drums volume").rect().center();
    harness.hover_at(at);
    harness.run();
    let volume = |h: &Harness<'static, App>| h.state().session().map(|s| s.project().channels[0].volume).unwrap_or(1.0);
    // One line of a wheel turned up, as a wheel reports it (the content moves down).
    let tick = |modifiers: Modifiers| egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(0.0, 1.0),
        modifiers,
        phase: egui::TouchPhase::Move,
    };
    // The wheel alone scrolls the view: scrolling down the headers never moves a fader.
    harness.input_mut().events.push(tick(Modifiers::NONE));
    harness.run();
    assert!((volume(&harness) - 1.0).abs() < 1e-9, "the wheel alone left it: {}", volume(&harness));
    // Alt and the wheel move the fader, and the view stays where it is.
    let scroll = harness.state().arrangement().scroll;
    harness.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    harness.input_mut().events.push(tick(Modifiers::ALT));
    harness.run();
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    assert!((volume(&harness) - gain_of(1.0)).abs() < 1e-6, "one tick raised it by 1 dB: {}", volume(&harness));
    assert_eq!(harness.state().arrangement().scroll, scroll, "and the view stayed where it was");
}

#[test]
fn dragging_one_fader_of_a_selection_moves_every_track_in_proportion() {
    let mut harness = arrangement_app();
    harness.state_mut().run(&Action::new("track.setVolume", json!({ "tracks": [1], "db": -6.0 })));
    harness.run();
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.get_by_label("Track Bass").click_modifiers(Modifiers::COMMAND);
    harness.run();
    let volumes = |h: &Harness<'static, App>| -> Vec<f64> {
        h.state().session().map(|s| s.project().channels.iter().map(|c| c.volume).collect()).unwrap_or_default()
    };
    let before = volumes(&harness);

    // 20 points down the field is 5 dB quieter: for the track whose field was dragged, and for the
    // other track of the selection, so what was between them stays.
    let at = harness.get_by_label("Drums volume").rect().center();
    harness.hover_at(at);
    harness.run();
    harness.drag_at(at);
    harness.step();
    harness.hover_at(at + egui::vec2(0.0, 20.0));
    harness.step();
    harness.drop_at(at + egui::vec2(0.0, 20.0));
    harness.run();
    let after = volumes(&harness);
    let quieter = gain_of(-5.0);
    assert!(before.iter().all(|v| *v > 0.0), "{before:?}");
    for (track, (was, now)) in ["Drums", "Bass"].iter().zip(before.iter().zip(&after)) {
        assert!((now / was - quieter).abs() < 1e-3, "{track}: {was} -> {now}");
    }
}

#[test]
fn a_selections_track_on_and_solo_buttons_take_the_whole_selection() {
    let mut harness = arrangement_app();
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.get_by_label("Track Bass").click_modifiers(Modifiers::COMMAND);
    harness.run();
    harness.get_by_label("Turn Drums off").click();
    harness.run();
    let muted: Vec<bool> =
        harness.state().session().map(|s| s.project().channels.iter().map(|c| c.mute).collect()).unwrap_or_default();
    assert!(muted[0] && muted[1], "both selected tracks went quiet: {muted:?}");

    harness.get_by_label("Solo Drums").click();
    harness.run();
    let soloed: Vec<bool> =
        harness.state().session().map(|s| s.project().channels.iter().map(|c| c.solo).collect()).unwrap_or_default();
    assert!(soloed[0] && soloed[1], "and both soloed together: {soloed:?}");
}

#[test]
fn a_groups_fold_triangle_hides_its_tracks_and_draws_their_clips_on_its_row() {
    let mut harness = arrangement_app();
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.get_by_label("Track Bass").click_modifiers(Modifiers::COMMAND);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::G);
    harness.run();
    assert!(harness.query_by_label("Track Drums").is_some(), "a group's tracks keep their lanes");

    harness.get_by_label("Fold Group").click();
    harness.run();
    assert!(harness.query_by_label("Track Group").is_some(), "the group keeps its row");
    assert!(harness.query_by_label("Track Drums").is_none(), "folding takes its tracks' lanes away");
    assert!(harness.query_by_label("Track Bass").is_none());
    // The group's row draws every track inside it as a thin row of its clips' marks, so it shows
    // how much is in there and where; the clips themselves are picked once it is unfolded.
    assert!(harness.query_by_label_contains("Clip Walk on Bass").is_none(), "marks, not clips to pick");

    harness.get_by_label("Unfold Group").click();
    harness.run();
    assert!(harness.query_by_label("Track Drums").is_some(), "unfolding brings the lanes back");
}

#[test]
fn an_edit_asks_for_a_frame_so_the_views_show_it_at_once() {
    let mut harness = arrangement_app();
    harness.run();
    assert!(!harness.ctx.has_requested_repaint(), "nothing to draw");
    // A click on a header runs its command after the views drew: the window asks for one more
    // frame, so the new state is on screen without a second click or a pointer move.
    harness.get_by_label("Turn Bass off").click();
    harness.step();
    assert!(harness.ctx.has_requested_repaint(), "an edit asks for a frame");
    harness.run();
    assert!(harness.query_by_label("Turn Bass on").is_some(), "and the frame shows it");
}

#[test]
fn menus_delete_a_track_and_duplicate_a_clip() {
    let mut harness = arrangement_app();
    harness.get_by_label("Track Keys").click_secondary();
    harness.run();
    harness.get_by_label("Delete track").click();
    harness.run();
    assert!(!names(&harness).contains(&"Keys".to_string()));

    let clips =
        |h: &Harness<'static, App>| h.state().session().map_or(0, |s| s.project().arrangement.lanes[0].clips.len());
    let before = clips(&harness);
    harness.get_by_label("Clip Fill on Drums at bar 29").click_secondary();
    harness.run();
    harness.get_by_label_contains("Duplicate").click();
    harness.run();
    // The copy sits after Fill and shortens the clip it lands on, so there is one more clip.
    assert_eq!(clips(&harness), before + 1);
    let fill_copies = harness.state().session().map_or(0, |s| {
        s.project().arrangement.lanes[0].clips.iter().filter(|c| c.start == 128.0 && c.length == 16.0).count()
    });
    assert_eq!(fill_copies, 1, "the copy of Fill sits at bar 33");
}

#[test]
fn dragging_a_header_moves_the_track() {
    let mut harness = arrangement_app();
    let from = harness.get_by_label("Track Vox").rect().center();
    // Three lanes up (49 points each), into the upper half of Drums.
    let to = from - egui::vec2(0.0, 3.0 * 49.0 + 14.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from - egui::vec2(0.0, 40.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    assert_eq!(names(&harness)[..4], ["Vox", "Drums", "Bass", "Keys"]);
}

fn parent_name(h: &Harness<'static, App>, name: &str) -> Option<String> {
    let session = h.state().session()?;
    let p = session.project();
    let parent = p.tracks.iter().find(|t| t.name == name)?.parent?;
    p.track(parent).map(|t| t.name.clone())
}

#[test]
fn groups_nest_fold_and_take_tracks_from_the_menu_and_by_dragging() {
    let mut harness = arrangement_app();
    // Drums into a group, and that group into another.
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::G);
    harness.run();
    harness.get_by_label("Track Group").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::G);
    harness.run();
    assert_eq!(names(&harness)[..3], ["Group", "Group", "Drums"]);
    assert_eq!(parent_name(&harness, "Drums").as_deref(), Some("Group"));

    // Keys into the outer group from its menu.
    harness.get_by_label("Track Keys").click_secondary();
    harness.run();
    harness.get_by_label_contains("Move into group").click();
    harness.run();
    harness.get_all_by_label("Group").next().unwrap().click();
    harness.run();
    assert!(parent_name(&harness, "Keys").is_some());

    // Folding the outer group hides everything inside it; unfolding shows it again.
    assert_eq!(harness.query_all_by_label("Fold Group").count(), 2);
    harness.get_all_by_label("Fold Group").next().unwrap().click();
    harness.run();
    assert!(harness.query_by_label("Track Drums").is_none());
    assert!(harness.query_by_label("Track Keys").is_none());
    assert_eq!(harness.query_all_by_label("Track Group").count(), 1);
    harness.get_by_label("Unfold Group").click();
    harness.run();
    assert!(harness.query_by_label("Track Drums").is_some());

    // Keys out of its group again, from its menu.
    harness.get_by_label("Track Keys").click_secondary();
    harness.run();
    harness.get_by_label("Move out of group").click();
    harness.run();
    assert_eq!(parent_name(&harness, "Keys"), None);
    assert_eq!(harness.state().session().map(|s| s.project().validate()), Some(Ok(())));
}

#[test]
fn dragging_a_header_below_a_group_header_puts_the_track_in_the_group() {
    let mut harness = arrangement_app();
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::G);
    harness.run();
    let from = harness.get_by_label("Track Vox").rect().center();
    let group = harness.get_by_label("Track Group").rect();
    // The lower half of the group's header.
    let to = egui::pos2(from.x, group.bottom() - 6.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from - egui::vec2(0.0, 40.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    assert_eq!(names(&harness)[..3], ["Group", "Vox", "Drums"]);
    assert_eq!(parent_name(&harness, "Vox").as_deref(), Some("Group"));
}

/// The demo project with an audio track, "Loop", whose clip plays `loop.wav` (two seconds of a
/// tone that fades out), and the waveforms of that file.
fn project_with_audio() -> (suhina_engine::model::Project, suhina_ui::kit::waveform::Waveforms) {
    use suhina_engine::model::*;
    let mut project = demo_project();
    project.tracks.push(Track {
        id: TrackId(30),
        name: "Loop".into(),
        content: ContentKinds { notes: false, audio: true },
        channel: Some(ChannelId(130)),
        ..project.tracks[0].clone()
    });
    project.channels.push(Channel {
        id: ChannelId(130),
        name: "Loop".into(),
        sends: Vec::new(),
        ..project.channels[0].clone()
    });
    let region = AudioRegion {
        file: "loop.wav".into(),
        absolute_file: None,
        sample_rate: Some(8000),
        channels: Some(1),
        duration_seconds: Some(2.0),
        warped: true,
        stretch: StretchMode::default(),
        warp_markers: vec![WarpMarker { time: 0.0, audio_time: 0.0 }, WarpMarker { time: 4.0, audio_time: 2.0 }],
        gain: 1.0,
        transpose: 0.0,
        stream: None,
    };
    project.contents.push(ClipContent {
        id: ContentId(300),
        name: "Loop".into(),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Audio(region) }],
        extensions: Extensions::default(),
        scale: None,
    });
    let clip = Clip {
        id: Default::default(),
        content: ContentId(300),
        start: 0.0,
        length: 16.0,
        offset: 0.0,
        loop_range: Some(LoopRange { start: 0.0, end: 4.0 }),
        enabled: true,
        fade_in: 0.0,
        fade_out: 0.0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        follow_action: Default::default(),
        launch: Default::default(),
        color: None,
        extensions: Extensions::default(),
    };
    project.arrangement.lanes.push(ArrangementLane {
        id: Default::default(),
        name: "Loop".into(),
        track: Some(TrackId(30)),
        clips: vec![clip],
    });
    let samples: Vec<f32> = (0..16000).map(|i| (i as f32 * 0.3).sin() * (1.0 - i as f32 / 16000.0)).collect();
    let mut pool = suhina_engine::AudioPool::new();
    pool.files.insert(
        "loop.wav".into(),
        std::sync::Arc::new(suhina_engine::AudioData { sample_rate: 8000, channels: 1, samples }),
    );
    (project, suhina_ui::kit::waveform::from_pool(&pool))
}

fn audio_app() -> App {
    let (project, waveforms) = project_with_audio();
    let mut app = App::new();
    app.set_session(Session::new(project), &[]);
    app.set_waveforms(waveforms);
    app
}

#[test]
fn dragging_an_audio_clips_gain_sets_it() {
    let mut harness = window(audio_app());
    harness.get_by_label("Arrangement view").click();
    harness.run();
    let field = harness.get_by_label("Gain of Loop on Loop").rect().center();
    harness.hover_at(field);
    harness.run();
    harness.drag_at(field);
    harness.run();
    harness.hover_at(field - egui::vec2(0.0, 15.0));
    harness.run();
    harness.hover_at(field - egui::vec2(0.0, 30.0));
    harness.run();
    harness.drop_at(field - egui::vec2(0.0, 30.0));
    harness.run();
    let gain = harness.state().session().and_then(|s| {
        s.project().content(suhina_engine::model::ContentId(300)).and_then(|c| match &c.lanes[0].data {
            suhina_engine::model::LaneData::Audio(region) => Some(region.gain),
            suhina_engine::model::LaneData::Notes(_) => None,
        })
    });
    // 30 points up at 0.1 dB a point: +3 dB.
    assert!(gain.is_some_and(|g| (g - 1.4125).abs() < 1e-3), "{gain:?}");
}

/// The Loop clip's fade-in and fade-out in the project.
fn loop_fades(harness: &Harness<'static, App>) -> Option<(f64, f64)> {
    let session = harness.state().session()?;
    let lane =
        session.project().arrangement.lanes.iter().find(|l| l.track == Some(suhina_engine::model::TrackId(30)))?;
    lane.clips.first().map(|clip| (clip.fade_in, clip.fade_out))
}

/// Drags the control labelled `label` sideways by `dx` points.
fn drag_sideways(harness: &mut Harness<'static, App>, label: &str, dx: f32) {
    let from = harness.get_by_label(label).rect().center();
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + egui::vec2(dx / 2.0, 0.0));
    harness.run();
    harness.hover_at(from + egui::vec2(dx, 0.0));
    harness.run();
    harness.drop_at(from + egui::vec2(dx, 0.0));
    harness.run();
}

#[test]
fn dragging_a_fade_handle_sets_the_fade_on_the_grid_and_a_double_click_removes_it() {
    let mut harness = window_with_step(audio_app(), 1.0 / 60.0);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    // The handles show on the selected clip.
    assert!(harness.query_by_label("Fade-in of Loop on Loop").is_none());
    harness.get_by_label("Clip Loop on Loop at bar 1").click();
    harness.run();

    // 8 points a beat and a bar grid at this zoom: 3 beats right ends the fade on bar 2.
    drag_sideways(&mut harness, "Fade-in of Loop on Loop", 24.0);
    assert_eq!(loop_fades(&harness), Some((4.0, 0.0)));
    // The fade-out, 3 beats left from the clip's end at beat 16: on bar 4, 4 beats long.
    drag_sideways(&mut harness, "Fade-out of Loop on Loop", -24.0);
    assert_eq!(loop_fades(&harness), Some((4.0, 4.0)));
    // Each drag is one undo step.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(loop_fades(&harness), Some((4.0, 0.0)));

    // A fade cannot reach past the other one: the fade-in stops where the clip has room.
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
    harness.run();
    drag_sideways(&mut harness, "Fade-in of Loop on Loop", 200.0);
    assert_eq!(loop_fades(&harness), Some((12.0, 4.0)));

    // A double-click removes the fade.
    let handle = harness.get_by_label("Fade-in of Loop on Loop").rect().center();
    harness.hover_at(handle);
    harness.run();
    // Press and release in one frame each, a few frames apart: a double-click.
    for _ in 0..2 {
        harness.hover_at(handle);
        harness.drag_at(handle);
        harness.drop_at(handle);
        harness.step();
    }
    harness.run();
    assert_eq!(loop_fades(&harness), Some((0.0, 4.0)));
}

/// The Loop clip's fade-in shape and curve in the project.
fn loop_fade_in_curve(harness: &Harness<'static, App>) -> Option<suhina_engine::model::FadeCurve> {
    let session = harness.state().session()?;
    let lane =
        session.project().arrangement.lanes.iter().find(|l| l.track == Some(suhina_engine::model::TrackId(30)))?;
    lane.clips.first().map(|clip| clip.fade_in_curve)
}

/// A right-click on a fade handle picks the shape; the curve handle on the fade's line bends it
/// with a vertical drag and a double-click straightens it (#197).
#[test]
fn a_fades_shape_is_picked_from_its_menu_and_its_curve_dragged() {
    use suhina_engine::model::{FadeCurve, FadeShape};
    let mut harness = window_with_step(audio_app(), 1.0 / 60.0);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label("Clip Loop on Loop at bar 1").click();
    harness.run();
    // A 4-beat fade-in, 32 points wide: wide enough for a curve handle.
    drag_sideways(&mut harness, "Fade-in of Loop on Loop", 24.0);
    assert_eq!(loop_fade_in_curve(&harness), Some(FadeCurve::default()));

    harness.get_by_label("Fade-in of Loop on Loop").click_secondary();
    harness.run();
    harness.get_by_label("Equal power").click();
    harness.run();
    assert_eq!(loop_fade_in_curve(&harness), Some(FadeCurve { shape: FadeShape::EqualPower, curve: 0.0 }));

    // Down by 12 points: the handle follows the pointer, and the curve bends towards −.
    let handle = harness.get_by_label("Fade-in curve of Loop on Loop").rect();
    let from = handle.center();
    let to = from + egui::vec2(0.0, 12.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from.lerp(to, 0.5));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    let bent = loop_fade_in_curve(&harness).map(|c| c.curve).unwrap_or_default();
    assert!(bent < -0.05 && bent > -1.0, "{bent}");
    let now = harness.get_by_label("Fade-in curve of Loop on Loop").rect().center();
    assert!((now.y - to.y).abs() < 1.0, "the handle is at {now:?}, the pointer let go at {to:?}");
    assert_eq!(loop_fade_in_curve(&harness).map(|c| c.shape), Some(FadeShape::EqualPower));

    // A double-click sets the curve back to 0 (half a second after the drag, as a person would).
    let handle = harness.get_by_label("Fade-in curve of Loop on Loop").rect().center();
    for _ in 0..30 {
        harness.step();
    }
    for _ in 0..2 {
        harness.hover_at(handle);
        harness.drag_at(handle);
        harness.drop_at(handle);
        harness.step();
    }
    harness.run();
    assert_eq!(loop_fade_in_curve(&harness), Some(FadeCurve { shape: FadeShape::EqualPower, curve: 0.0 }));
}

fn two_projects() -> Harness<'static, App> {
    let mut app = App::new();
    app.open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    app.open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    window(app)
}

#[test]
fn a_return_in_the_arrangement_has_a_tracks_controls_and_main_a_volume_bar_and_they_grow_upward() {
    let mut harness = arrangement_app();
    // A return's header is a track's: volume and pan, its on switch, its automation lanes.
    for label in ["A Reverb volume", "A Reverb pan", "Turn A Reverb off"] {
        assert!(harness.query_by_label(label).is_some(), "missing {label}");
    }
    assert!(harness.query_by_label("Show automation of A Reverb").is_some(), "its automation lanes button");
    // A track's header has a send knob to the return, as its strip has.
    assert!(harness.query_by_label("Drums send to A Reverb").is_some(), "a track's send knob");
    assert!(harness.query_by_label("A Reverb send to A Reverb").is_none(), "no send on a return");
    // Main: a bar a drag sets, one undo step.
    let main = |h: &Harness<'static, App>| {
        let p = h.state().session().unwrap().project();
        p.channels.iter().find(|c| c.role == suhina_engine::model::ChannelRole::Master).map(|c| c.volume).unwrap()
    };
    let bar = harness.get_by_label("Main volume").rect().center();
    harness.hover_at(bar);
    harness.step();
    harness.drag_at(bar);
    harness.step();
    for y in [10.0, 20.0, 40.0] {
        harness.hover_at(bar + egui::vec2(0.0, y));
        harness.step();
    }
    harness.drop_at(bar + egui::vec2(0.0, 40.0));
    harness.run_steps(3);
    assert!(main(&harness) < 1.0, "dragged down, quieter: {}", main(&harness));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run_steps(3);
    assert!((main(&harness) - 1.0).abs() < 1e-9, "one undo step");

    // The fixed rows grow upward: Main's top edge dragged up 30 points takes its bar and the
    // return above it up as far.
    let (bar_top, reverb_top) =
        (harness.get_by_label("Main volume").rect().top(), harness.get_by_label("A Reverb volume").rect().top());
    let edge = egui::pos2(harness.get_by_label("Main volume").rect().center().x, bar_top - 1.0);
    harness.hover_at(edge);
    harness.step();
    harness.drag_at(edge);
    harness.step();
    for y in [-10.0, -20.0, -30.0] {
        harness.hover_at(edge + egui::vec2(0.0, y));
        harness.step();
    }
    harness.drop_at(edge + egui::vec2(0.0, -30.0));
    harness.run_steps(3);
    assert_eq!(harness.get_by_label("Main volume").rect().top(), bar_top - 30.0);
    assert_eq!(harness.get_by_label("A Reverb volume").rect().top(), reverb_top - 30.0);
}

#[test]
fn hide_returns_takes_the_return_rows_out_of_the_arrangement_and_show_brings_them_back() {
    let mut harness = arrangement_app();
    assert!(harness.query_by_label("A Reverb volume").is_some());
    harness.get_by_label("Hide returns").click();
    harness.run_steps(3);
    assert!(harness.query_by_label("A Reverb volume").is_none(), "the return's row is gone");
    assert!(harness.query_by_label("Main volume").is_some(), "Main stays");
    assert!(harness.state().settings().view.returns_hidden, "kept as a setting");
    harness.get_by_label("Show returns").click();
    harness.run_steps(3);
    assert!(harness.query_by_label("A Reverb volume").is_some(), "back again");
}

#[test]
fn the_waveform_zoom_draws_every_audio_clip_larger_and_is_kept_as_a_setting() {
    let mut harness = arrangement_app();
    assert_eq!(harness.state().arrangement().waveform_zoom, 1.0);
    // One zoom for the whole Arrangement: the keys step it, the setting keeps it.
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowUp);
    harness.run_steps(2);
    assert!((harness.state().arrangement().waveform_zoom - 1.25).abs() < 1e-6);
    assert_eq!(harness.state().settings().view.waveform_zoom, 125);
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowDown);
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowDown);
    harness.run_steps(2);
    assert_eq!(harness.state().arrangement().waveform_zoom, 1.0, "never below the clips' own size");
    // The slider by the ruler, plain to see: a click at its right end zooms all the way.
    let slider = harness.get_by_label("Waveform zoom").rect();
    assert!(slider.width() >= 80.0 && slider.height() >= 10.0, "a slider to see and grab: {slider:?}");
    harness.hover_at(egui::pos2(slider.right() - 1.0, slider.center().y));
    harness.step();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: egui::pos2(slider.right() - 1.0, slider.center().y),
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        harness.step();
    }
    harness.run_steps(2);
    let zoom = harness.state().arrangement().waveform_zoom;
    assert!(zoom > 12.0, "{zoom}");
    // The wheel over the slider steps it, and the view behind it stays where it is.
    let scroll = harness.state().arrangement().scroll;
    harness.hover_at(slider.center());
    harness.step();
    harness.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        delta: egui::vec2(-1.0, -1.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    // The wheel is smoothed over frames: long enough for all of it to land.
    harness.run_steps(30);
    assert!((harness.state().arrangement().waveform_zoom - zoom / 1.25).abs() < 1e-4);
    assert_eq!(harness.state().arrangement().scroll, scroll, "the lanes did not scroll");
}

#[test]
fn ctrl_tab_shows_the_open_projects_and_switches_when_ctrl_is_let_go() {
    let mut app = App::new();
    for _ in 0..3 {
        app.open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    }
    let mut harness = window(app);
    let ids = harness.state().projects().ids();
    assert_eq!(harness.state().projects().active(), ids.get(2).copied());
    // Ctrl held, Tab tapped twice: the list shows, the pick moves on, nothing switches yet.
    harness.event(egui::Event::ModifiersChanged(Modifiers::COMMAND));
    harness.step();
    tab_with_ctrl_held(&mut harness);
    assert!(harness.query_by_label("Switch to Untitled 1").is_some(), "the open projects are listed");
    tab_with_ctrl_held(&mut harness);
    assert_eq!(harness.state().projects().active(), ids.get(2).copied(), "not while Ctrl is down");
    // Ctrl let go: to the second one along (wrapping from the third: the first, then the second).
    harness.event(egui::Event::ModifiersChanged(Modifiers::default()));
    harness.run();
    assert_eq!(harness.state().projects().active(), ids.get(1).copied());
    assert!(harness.query_by_label("Switch to Untitled 1").is_none(), "the list is gone");
}

/// Tab pressed and let go while Ctrl (Cmd) stays down.
fn tab_with_ctrl_held(harness: &mut Harness<'static, App>) {
    for pressed in [true, false] {
        harness.event(egui::Event::Key {
            key: Key::Tab,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        });
        harness.step();
    }
    harness.step();
    harness.step();
}

#[test]
fn tabs_switch_keep_each_projects_edits_and_close() {
    let mut harness = two_projects();
    let ids = harness.state().projects().ids();
    assert_eq!(harness.state().projects().active(), ids.get(1).copied());

    // Turn a track off in the second project, then show the first: it is untouched.
    harness.get_by_label("Turn Bass off").click();
    harness.run();
    harness.get_by_label("Tab Untitled 1").click();
    harness.run();
    harness.run();
    assert_eq!(harness.state().projects().active(), ids.first().copied());
    assert_eq!(harness.state_mut().take_player_request(), ids.first().copied());
    assert!(harness.query_by_label("Turn Bass off").is_some());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Tab);
    harness.run();
    harness.run();
    assert_eq!(harness.state().projects().active(), ids.get(1).copied());
    assert!(harness.query_by_label("Turn Bass on").is_some());

    // It has an edit, so closing it asks first.
    harness.get_by_label("Close Untitled 2").click();
    harness.run();
    harness.get_by_label("Don't save the changes").click();
    harness.run();
    harness.run();
    assert_eq!(harness.state().projects().len(), 1);
    assert!(harness.query_by_label("Tab Untitled 1").is_none(), "one project shows no tabs");
}

#[test]
fn opening_while_a_project_is_open_asks_and_can_remember() {
    let mut harness = window(demo_app());
    harness.state_mut().request_open("other.als".into());
    harness.run();
    assert!(harness.state_mut().take_open_request().is_none());
    harness.get_by_label("Remember").click();
    harness.run();
    harness.get_by_label("In a new tab").click();
    harness.run();
    let request = harness.state_mut().take_open_request();
    assert_eq!(request, Some(("other.als".into(), OpenAs::NewTab)));
    assert_eq!(harness.state().settings().on_open, suhina_ui::settings::OnOpen::NewTab);

    // Remembered: the next one opens in a new tab without asking.
    harness.state_mut().request_open("third.als".into());
    harness.run();
    assert_eq!(harness.state_mut().take_open_request(), Some(("third.als".into(), OpenAs::NewTab)));
}

#[test]
fn settings_add_and_remove_plugin_folders_and_scan_in_the_background() {
    let base = std::env::temp_dir().join(format!("suhina-ui-plugins-{}", std::process::id()));
    let folder = base.join("my plug-ins");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    let library = suhina_plugins::library::Library::new(Some(base.join("settings")), base.join("no-such-host"))
        .without_standard_folders();
    let mut app = App::with_library(library);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = window(app);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Plug-ins").click();
    harness.run();

    let field = harness.get_by_label("Plug-in folder to add");
    field.focus();
    field.type_text(&folder.display().to_string());
    harness.run();
    harness.get_by_label("Add plug-in folder").click();
    harness.run();
    harness.run();
    let remove = harness.query_all_by_label_contains("Remove ").count();
    assert_eq!(remove, 1, "the added folder is listed with a Remove button");

    harness.get_by_label("Scan").click();
    // The window keeps redrawing while the scan runs, so it is stepped, not run until still.
    harness.step();
    for _ in 0..200 {
        if !harness.state().scanning() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        harness.step();
    }
    match harness.state().status() {
        Status::Error(message) => assert!(message.contains("plug-in host"), "{message}"),
        other => panic!("expected the missing host to be reported, got {other:?}"),
    }

    harness.get_by_label_contains("Remove ").click();
    harness.run();
    harness.run();
    assert_eq!(harness.query_all_by_label_contains("Remove ").count(), 0);
    let _cleanup = std::fs::remove_dir_all(&base);
}

#[test]
fn a_key_set_in_settings_runs_its_action_and_is_saved() {
    let mut harness = window(demo_app());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Keys").click();
    harness.run();
    harness.get_by_label("Change keys of view.arrangement").click();
    harness.run();
    harness.key_press(Key::A);
    harness.run();
    assert_eq!(harness.state().keymap().binding_text("view.arrangement").as_deref(), Some("A"));
    assert!(harness.state_mut().take_changed_keymap().is_some());

    harness.get_by_label("Close settings").click();
    harness.run();
    harness.key_press(Key::A);
    harness.run();
    assert_eq!(harness.state().view(), View::Arrangement);
}

#[test]
fn settings_set_the_toolbar_size_and_the_mouse_switches() {
    let mut harness = window(demo_app());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Clip editor").click();
    harness.run();
    // The piano roll's row shows the size in use, Large by default.
    assert!(harness.query_by_label("Toolbar size: Large").is_some(), "the row reads its value");
    assert!(harness.query_by_label("Toolbar size: Normal").is_some());
    assert!(harness.query_by_label("Toolbar size: Larger").is_some());
    assert_eq!(harness.state().settings().toolbar_size, suhina_ui::settings::ToolbarSize::Large);
    harness.get_by_label("Toolbar size: Larger").click();
    harness.run();
    assert_eq!(harness.state().settings().toolbar_size, suhina_ui::settings::ToolbarSize::Larger);
    assert_eq!(
        harness.state_mut().take_changed_settings().map(|s| s.toolbar_size),
        Some(suhina_ui::settings::ToolbarSize::Larger)
    );

    harness.get_by_label("Settings page: Keys").click();
    harness.run();
    // The workflow settings write to the keymap, which is handed out to be saved. They are the
    // page's last rows: the wheel scrolls them into view.
    for _ in 0..20 {
        harness.hover_at(egui::pos2(SIZE.0 / 2.0, SIZE.1 / 2.0));
        harness.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -120.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        harness.run();
    }
    assert!(
        harness.query_by_label("Right click on a note: Every tool").is_some(),
        "each setting is a row of its short choices"
    );
    assert!(!harness.state().keymap().mouse().right_deletes, "off until it is turned on");
    // Under the window's edge until the page is scrolled to it.
    harness.get_by_label("Right click on a note: Every tool").scroll_to_me();
    harness.run();
    harness.get_by_label("Right click on a note: Every tool").click();
    harness.run();
    assert!(harness.state().keymap().mouse().right_deletes);
    assert!(harness.state_mut().take_changed_keymap().is_some(), "the keymap is saved with it");
    assert_eq!(harness.state().keymap().preset(), suhina_ui::settings::keymap::Preset::Custom, "the mode is Custom");
    // And back again.
    harness.get_by_label("Right click on a note: Nothing").click();
    harness.run();
    assert!(!harness.state().keymap().mouse().right_deletes);
    // Every setting has its row.
    for row in [
        "Click on empty grid: Draws",
        "Shift+right drag: Splits",
        "Alt+right button: Plays",
        "Duplicate key: Ctrl+B",
        "Double-click on empty grid: Nothing",
        "Double-click on a note: Nothing",
    ] {
        assert!(harness.query_by_label(row).is_some(), "{row}");
    }
    // A preset chosen on the Shortcuts page turns all four mouse switches on.
    harness.get_by_label("Settings page: Keys").click();
    harness.run();
    harness.get_by_label("Preset: Mouse-first").click();
    harness.run();
    assert!(harness.state().keymap().mouse().all_on());
    assert_eq!(
        harness.state_mut().take_changed_keymap().map(|k| k.mouse()),
        Some(suhina_ui::settings::keymap::MouseSettings::ALL_ON)
    );
    assert!(harness.query_by_label("Preset: Custom").is_some(), "Custom is one of the modes there too");
    harness.get_by_label("Settings page: Keys").click();
    harness.run();
    assert!(harness.query_by_label("Right click on a note: Every tool").is_some());
    harness.get_by_label("Close settings").click();
    harness.run();
}

#[test]
fn keys_copy_paste_and_delete_the_selected_clip() {
    let mut harness = arrangement_app();
    let spans = |h: &Harness<'static, App>| {
        h.state()
            .session()
            .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    harness.get_by_label("Clip Fill on Drums at bar 29").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::C);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    // The copy lands right after Fill (bar 33) and shortens what it covers.
    assert!(spans(&harness).contains(&(128.0, 16.0)), "{:?}", spans(&harness));

    let before = spans(&harness).len();
    harness.get_by_label("Clip Beat A on Drums at bar 1").click();
    harness.run();
    harness.key_press(Key::Delete);
    harness.run();
    assert_eq!(spans(&harness).len(), before - 1);
    assert!(!spans(&harness).contains(&(0.0, 64.0)));
}

/// The selection names a clip by its id (ADR 0025): when a clip before it moves away, the
/// selected clip's index changes, and the key still acts on the clip that was clicked.
#[test]
fn the_selected_clip_stays_selected_when_a_clip_before_it_moves() {
    let mut harness = arrangement_app();
    let drums = |h: &Harness<'static, App>| {
        h.state().session().map(|s| s.project().arrangement.lanes[0].clips.clone()).unwrap_or_default()
    };
    harness.get_by_label("Clip Fill on Drums at bar 29").click();
    harness.run();
    let fill = drums(&harness).iter().find(|c| c.start == 112.0).map(|c| c.id).unwrap();
    assert_eq!(harness.state().selected_clip(), Some((TrackId(1), fill)));
    let index = |h: &Harness<'static, App>| drums(h).iter().position(|c| c.id == fill);
    let before = index(&harness);

    // Beat A, the first clip on Drums, moves to the end: Fill is one place earlier on the lane.
    let beat_a = drums(&harness)[0].id;
    harness.state_mut().run(&Action::new("clip.move", json!({ "clip": beat_a.0, "start": 400 })));
    harness.run();
    assert_eq!(index(&harness), before.map(|i| i - 1));
    assert_eq!(harness.state().selected_clip(), Some((TrackId(1), fill)), "still Fill");

    // Delete takes away Fill, not the clip that is now where Fill was.
    harness.key_press(Key::Delete);
    harness.run();
    let left = drums(&harness);
    assert!(left.iter().all(|c| c.id != fill), "Fill is gone");
    assert!(left.iter().any(|c| c.id == beat_a), "Beat A is still there");
}

#[test]
fn paste_and_duplicate_keys_follow_the_midi_stacking_setting() {
    let spans = |h: &Harness<'static, App>| {
        h.state()
            .session()
            .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    for key in [Key::V, Key::D] {
        let mut harness = arrangement_app();
        let settings = suhina_ui::settings::Settings { midi_stacking: true, ..Default::default() };
        let ctx = harness.ctx.clone();
        harness.state_mut().set_settings(&ctx, settings);
        harness.run();
        let before = spans(&harness);
        harness.get_by_label("Clip Beat B on Drums at bar 17").click();
        harness.run();
        harness.key_press_modifiers(Modifiers::COMMAND, Key::C);
        harness.run();
        harness.key_press_modifiers(Modifiers::COMMAND, key);
        harness.run();
        // The copy of Beat B (bars 17-28) lies over Fill (bars 29-32), which keeps its span.
        let after = spans(&harness);
        assert!(after.contains(&(112.0, 48.0)), "{key:?}: {after:?}");
        assert_eq!(after.len(), before.len() + 1, "{key:?}: {after:?}");
        assert!(before.iter().all(|s| after.contains(s)), "{key:?}: {before:?} -> {after:?}");
    }
}

/// The demo project with a utility device on Drums.
fn project_with_a_device() -> suhina_engine::model::Project {
    use suhina_engine::model::{Device, DeviceKind, DeviceParam, Extensions};
    let mut project = demo_project();
    project.channels[0].devices.push(Device {
        name: "Utility".into(),
        kind: DeviceKind::Builtin("utility".into()),
        enabled: true,
        state: None,
        params: vec![DeviceParam::new("gain", -3.5), DeviceParam::new("width", 1.2)],
        automation: Vec::new(),
        extensions: Extensions::default(),
    });
    project
}

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
fn dragging_a_session_column_header_moves_the_track() {
    let mut harness = window(demo_app());
    let from = harness.get_by_label("Track Vox").rect().center();
    // Three columns left (117 points each), into the left half of Drums.
    let to = from - egui::vec2(3.0 * 117.0 + 30.0, 0.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from - egui::vec2(60.0, 0.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    assert_eq!(names(&harness)[..4], ["Vox", "Drums", "Bass", "Keys"]);
}

/// Renders the window with the demo project and the piano roll of the **Beat B** clip on **Drums**
/// to `target/ui-snapshots/piano-roll-*.png`, to look the toolbar, the note clipboard buttons and
/// the quantise block over: the row layout, the quantise block open in it, and the stacked detail
/// area with the block open. Needs a GPU adapter, so it runs only when asked:
/// `cargo test -p suhina-ui --test window -- --ignored render_the_piano_roll`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_piano_roll() {
    let mut renderer = suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000)
        .unwrap_or_else(|e| panic!("{e}"));
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = window_with_step(app, 0.05);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label("Clip Beat B on Drums at bar 17").click();
    harness.run();
    // A note selected, so no clipboard button is greyed out.
    let note = harness
        .query_all_by_label_contains("Note ")
        .next()
        .unwrap_or_else(|| panic!("a note in the grid"))
        .rect()
        .center();
    harness.hover_at(note);
    harness.step();
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: note,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        harness.step();
    }
    harness.run();
    harness.remove_cursor();
    harness.run();

    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    let save = |harness: &mut Harness<'static, App>, name: &str| {
        let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
        image.save(folder.join(name)).unwrap_or_else(|e| panic!("{e}"));
    };
    save(&mut harness, "piano-roll.png");

    harness.state_mut().piano_roll_mut().quantize_open = true;
    harness.run();
    assert!(harness.state().piano_roll().quantize_open, "the block is open");
    assert!(harness.query_by_label("Quantise notes").is_some(), "and its controls are drawn");
    save(&mut harness, "piano-roll-quantise.png");

    let ctx = harness.ctx.clone();
    let stacked =
        suhina_ui::settings::Settings { device_layout: suhina_ui::settings::DeviceLayout::Stack, ..Default::default() };
    harness.state_mut().set_settings(&ctx, stacked);
    harness.run();
    harness.state_mut().piano_roll_mut().quantize_open = true;
    harness.run();
    save(&mut harness, "piano-roll-quantise-stacked.png");
}

/// Renders the window with the demo project to `target/ui-snapshots/session.png` and
/// `arrangement.png`, to compare with the mockups in `docs/design/reference/`. Needs a GPU adapter, so it runs only
/// when asked: `cargo test -p suhina-ui -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_session_window() {
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.ui(ui), demo_app());
    theme::install(&harness.ctx);
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    image.save(folder.join("session.png")).unwrap_or_else(|e| panic!("{e}"));

    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("arrangement.png")).unwrap_or_else(|e| panic!("{e}"));

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("settings.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.state_mut().set_project_lists(
        vec!["C:/Music/Suhina/Projects/Night drive.suhina".into(), "C:/Music/Sets/Old jam.als".into()],
        vec!["C:/Music/Suhina/Templates/Beat starter.suhina".into(), "C:/Music/Suhina/Templates/Live rig.als".into()],
    );
    harness.state_mut().open_project_dialog(false);
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("project-dialog.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.key_press(Key::Escape);
    harness.run();
    harness.state_mut().do_action("file.saveAs", &mut Vec::new());
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("save-prompt.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.key_press(Key::Escape);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Theme").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("settings-theme.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.get_by_label("Settings page: Arrangement").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("settings-arrangement.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.get_by_label("Settings page: Keys").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("shortcuts.png")).unwrap_or_else(|e| panic!("{e}"));
    harness.get_by_label("Settings page: Projects and backups").click();
    harness.run();

    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.ui(ui), audio_app());
    theme::install(&harness.ctx);
    harness.run();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("arrangement-audio.png")).unwrap_or_else(|e| panic!("{e}"));

    // The detail area with a track's devices, in a row and stacked.
    let mut harness = Harness::builder().with_size(SIZE).with_pixels_per_point(2.0).wgpu().build_ui_state(
        |ui, app: &mut App| app.ui(ui),
        {
            let mut app = App::new();
            app.set_session(Session::new(project_with_a_device()), &[]);
            app
        },
    );
    theme::install(&harness.ctx);
    harness.run();
    harness.get_by_label("Track Drums").click();
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("detail-row.png")).unwrap_or_else(|e| panic!("{e}"));
    let ctx = harness.ctx.clone();
    let settings =
        suhina_ui::settings::Settings { device_layout: suhina_ui::settings::DeviceLayout::Stack, ..Default::default() };
    harness.state_mut().set_settings(&ctx, settings);
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("detail-stack.png")).unwrap_or_else(|e| panic!("{e}"));

    // A Live Set dragged over the window darkens it.
    harness.input_mut().hovered_files.push(egui::HoveredFile { path: Some("set.als".into()), ..Default::default() });
    harness.run_steps(12);
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    image.save(folder.join("drop.png")).unwrap_or_else(|e| panic!("{e}"));
}

/// Renders the Session and Arrangement views in every theme that comes with Suhina, to
/// `target/ui-snapshots/theme-<id>-session.png` and `-arrangement.png`, to look them over. Needs
/// a GPU adapter: `cargo test -p suhina-ui -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_every_built_in_theme() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    for (id, _) in theme::built_in_themes() {
        let mut harness =
            Harness::builder().with_size(SIZE).wgpu().build_ui_state(|ui, app: &mut App| app.ui(ui), demo_app());
        theme::install(&harness.ctx);
        let ctx = harness.ctx.clone();
        harness.state_mut().use_theme(&ctx, id);
        harness.run();
        harness.remove_cursor();
        harness.run();
        let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
        image.save(folder.join(format!("theme-{id}-session.png"))).unwrap_or_else(|e| panic!("{e}"));
        harness.get_by_label("Arrangement view").click();
        harness.run();
        harness.remove_cursor();
        harness.run();
        let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
        image.save(folder.join(format!("theme-{id}-arrangement.png"))).unwrap_or_else(|e| panic!("{e}"));
    }
}

/// An audio system with one device on one host, whose players run nowhere. `WASAPI` is the host
/// whose shared mode Windows gives the device's buffer in, so the Settings row there names the
/// engine's own block; its exclusive mode is possible, and does ask the device. `ALSA` is a host
/// whose driver declares no buffer range of its own: it keeps its own size, and Suhina's own are
/// offered.
struct StandAudio(&'static str);

impl suhina_session::AudioSystem for StandAudio {
    fn hosts(&self) -> Vec<suhina_session::audio::HostInfo> {
        use suhina_session::audio::{DeviceInfo, HostInfo};
        let wasapi = self.0 == "WASAPI";
        let undeclared = self.0 == "ALSA";
        vec![HostInfo {
            name: self.0.into(),
            default: true,
            devices: vec![DeviceInfo {
                name: if wasapi { "Apollo" } else { "Stand Out" }.into(),
                default: true,
                channels: 2,
                sample_rates: vec![48_000],
                default_sample_rate: Some(48_000),
                // WASAPI shared mode plays the period Windows gives the device (480 frames here),
                // whatever is asked, so the sizes listed there are the engine's own block; a
                // driver that declared nothing is offered those too, and asked for none of them.
                buffer_sizes: if wasapi || undeclared {
                    vec![64, 128, 256, 512, 1024, 2048, 4096]
                } else {
                    vec![256, 512]
                },
                buffer_range: if undeclared {
                    None
                } else if wasapi {
                    Some((480, 480))
                } else {
                    Some((256, 512))
                },
                exclusive: wasapi,
            }],
            inputs: vec![suhina_session::audio::InputInfo {
                name: "Stand In".into(),
                default: true,
                channels: 2,
                sample_rates: vec![48_000],
            }],
            error: None,
        }]
    }

    fn open(
        &self,
        project: &suhina_engine::model::Project,
        audio: &suhina_engine::AudioPool,
        _assets: &suhina_engine::DeviceAssets,
        _choice: &suhina_session::OutputChoice,
        _previous: Option<suhina_session::Player>,
    ) -> Result<suhina_session::audio::Opened, String> {
        let mut renderer = suhina_engine::Renderer::stopped(project, audio, 48_000).map_err(|e| e.to_string())?;
        let player = suhina_session::Player::new(renderer.controller(), None);
        Ok(suhina_session::audio::Opened { player, notice: None, plugins_kept: true })
    }
}

/// The window with Settings › Audio open, on `host` as its only audio host. (`Result` so that a
/// failure is raised inside the test, where a panic is expected, and not in this helper.)
fn settings_audio_on(host: &'static str) -> Result<Harness<'static, App>, String> {
    let mut app = demo_app();
    app.projects_mut().set_audio_system(std::sync::Arc::new(StandAudio(host)));
    app.projects_mut().open_output(None).map_err(|e| e.0)?;
    let mut harness = window(app);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Settings page: Audio").click();
    // The page redraws while it shows the latency, so it is stepped, not run until still.
    for _ in 0..3 {
        harness.step();
    }
    Ok(harness)
}

#[test]
fn settings_audio_lists_the_devices_and_a_buffer_picked_is_chosen_and_saved() {
    let mut harness = settings_audio_on("Stand").unwrap_or_else(|e| panic!("{e}"));
    assert!(harness.query_by_label("Driver: Default (Stand)").is_some());
    assert!(harness.query_by_label("Device: Default (Stand Out)").is_some());
    harness.get_by_label("Buffer size: Default (1024 frames)").click();
    harness.step();
    harness.step();
    harness.get_by_label("512 frames (10.7 ms)").click();
    harness.step();
    harness.step();
    assert_eq!(harness.state().projects().output_choice().buffer_frames, Some(512));
    let saved = harness.state_mut().take_changed_settings().map(|s| s.audio_buffer);
    assert_eq!(saved, Some(512));
    assert!(harness.query_by_label("Buffer size: 512 frames (10.7 ms)").is_some());
}

/// WASAPI shared mode plays the period Windows gives the device, so the Settings row is Suhina's
/// own engine block there: it never claims the device takes a size, whatever the driver does (an
/// Apollo's takes 2048 frames at most). Exclusive mode does ask the device, so the row is the
/// device's own buffer then.
#[test]
fn settings_audio_names_the_engine_block_on_wasapi_shared() {
    let mut harness = settings_audio_on("WASAPI").unwrap_or_else(|e| panic!("{e}"));
    assert!(harness.query_by_label("Engine block: Default (1024 frames)").is_some());
    assert!(harness.query_by_label("Buffer size: Default (1024 frames)").is_none());
    // Exclusive mode takes the device for Suhina alone and asks it for the size, so the row says
    // what the number is there.
    harness.get_by_label("Exclusive mode").click();
    for _ in 0..3 {
        harness.step();
    }
    assert!(harness.state().projects().output_choice().exclusive);
    assert!(harness.query_by_label("Buffer size: Default (1024 frames)").is_some());
    assert!(harness.query_by_label("Engine block: Default (1024 frames)").is_none());
}

/// A driver that declares no buffer range of its own keeps its own size (Suhina asks it for
/// none), so the Settings row is the engine's block there too, and the sizes on offer are
/// Suhina's own.
#[test]
fn settings_audio_names_the_engine_block_when_the_driver_declares_no_sizes() {
    let harness = settings_audio_on("ALSA").unwrap_or_else(|e| panic!("{e}"));
    assert!(harness.query_by_label("Engine block: Default (1024 frames)").is_some());
    assert!(harness.query_by_label("Buffer size: Default (1024 frames)").is_none());
}

#[test]
fn the_control_bar_shows_the_cpu_load_only_while_an_engine_plays() {
    // With an engine attached as its player, the control bar shows the load.
    let mut renderer =
        suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_session(session, &[]);
    let mut harness = window(app);
    harness.run();
    assert!(harness.query_by_label("CPU load 0.0%").is_some());

    // Without one, there is nothing to measure and nothing shown.
    let mut app = App::new();
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = window(app);
    harness.run();
    assert!(harness.query_by_label("CPU load").is_none());
}

#[test]
fn the_control_bar_shows_the_ram_usage_whether_or_not_an_engine_plays() {
    // The program's own RAM is shown without an engine too: it is the process's, not the
    // engine's.
    let mut app = App::new();
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = window(app);
    harness.run();
    let ram = harness.query_by_label_contains("RAM usage");
    assert!(ram.is_some(), "the RAM shows next to the CPU's place");
    // In megabytes or gigabytes, as the issue asks.
    let text = format!("{:?}", ram.unwrap());
    assert!(text.contains("MB") || text.contains("GB"), "{text}");
}

#[test]
fn the_view_switch_is_the_last_control_at_the_right_with_the_cpu_and_ram_left_of_it() {
    // With an engine attached, all four show: RAM, CPU, then the view switch at the edge.
    let mut renderer =
        suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_session(session, &[]);
    let mut harness = window(app);
    harness.run();
    let ram = harness.get_by_label_contains("RAM usage").rect();
    let cpu = harness.get_by_label_contains("CPU load").rect();
    let arrangement = harness.get_by_label("Arrangement view").rect();
    let session = harness.get_by_label("Session view").rect();
    let switch_left = arrangement.left().min(session.left());
    let switch_right = arrangement.right().max(session.right());
    assert!(ram.right() <= cpu.left(), "RAM {ram:?} left of CPU {cpu:?}");
    assert!(cpu.right() <= switch_left, "CPU {cpu:?} left of the view switch at {switch_left}");
    // The switch keeps its place at the right edge.
    assert!(SIZE.0 - switch_right <= 16.0, "the switch ends at {switch_right}");
}

#[test]
fn a_narrow_window_leaves_out_what_would_reach_the_position_readout() {
    // At the smallest window the app allows, the blocks that show keep their order and stay
    // right of the transport in the centre (play, stop and the 64 px position).
    let mut renderer =
        suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap();
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_session(session, &[]);
    let size = (640.0, 400.0);
    let mut harness = Harness::builder()
        .with_size(size)
        .with_step_dt(0.25)
        .with_max_steps(4)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    let transport_end = size.0 / 2.0 + (28.0 * 2.0 + 1.0 + theme::PAD + 64.0) / 2.0;
    let arrangement = harness.get_by_label("Arrangement view").rect();
    let session = harness.get_by_label("Session view").rect();
    let switch_left = arrangement.left().min(session.left());
    assert!(switch_left > transport_end, "the switch at {switch_left} clears the position");
    let cpu = harness.query_by_label_contains("CPU load").map(|n| n.rect());
    let ram = harness.query_by_label_contains("RAM usage").map(|n| n.rect());
    // The CPU still fits beside the switch; the RAM, further left, may not.
    assert!(cpu.is_some(), "the CPU load fits at 640 px");
    if let Some(cpu) = cpu {
        assert!(cpu.left() >= transport_end && cpu.right() <= switch_left, "CPU at {cpu:?}");
    }
    if let Some(ram) = ram {
        assert!(ram.left() >= transport_end, "RAM at {ram:?}");
        assert!(cpu.is_some_and(|cpu| ram.right() <= cpu.left()), "RAM {ram:?} left of the CPU");
    }
}

/// The label of the active project's Close button in the tab row.
fn active_close_label(harness: &Harness<'static, App>) -> String {
    let projects = harness.state().projects();
    let name = projects.active().and_then(|id| projects.name(id)).unwrap_or_default();
    format!("Close {name}")
}

/// Whether the window asked to close itself in its last frame.
fn asked_to_close(harness: &Harness<'static, App>) -> bool {
    harness.output().viewport_output.values().any(|v| v.commands.contains(&egui::ViewportCommand::Close))
}

#[test]
fn closing_or_quitting_with_unsaved_changes_asks_first() {
    let dir = std::env::temp_dir().join(format!("suhina-ui-unsaved-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&dir);
    let mut app = demo_app();
    app.set_default_folders(dir.join("Projects"), dir.join("Templates"));
    app.open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    let mut harness = window(app);
    let open = |h: &Harness<'static, App>| h.state().projects().ids().len();
    let unsaved = |h: &Harness<'static, App>| h.state().session().is_some_and(Session::has_unsaved_changes);
    assert_eq!(open(&harness), 2);

    // An edit in the second project; closing its tab asks first, and Cancel keeps it open.
    harness.get_by_label("Turn Bass off").click();
    harness.run();
    assert!(unsaved(&harness));
    let label = active_close_label(&harness);
    if let Some(close) = harness.get_all_by_label(&label).last() {
        close.click();
    }
    harness.run();
    assert!(harness.query_by_label("Don't save the changes").is_some());
    harness.get_by_label("Cancel and keep the project open").click();
    harness.run();
    assert_eq!(open(&harness), 2);
    assert!(harness.query_by_label("Don't save the changes").is_none());

    // Don't save closes it.
    let label = active_close_label(&harness);
    if let Some(close) = harness.get_all_by_label(&label).last() {
        close.click();
    }
    harness.run();
    harness.get_by_label("Don't save the changes").click();
    harness.run();
    assert_eq!(open(&harness), 1);

    // A project without changes closes without asking; quitting with one that has them asks,
    // and Save first asks for a name, saves, then lets the window close.
    harness.get_by_label("Turn Bass off").click();
    harness.run();
    harness.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    harness.run();
    assert!(!asked_to_close(&harness));
    assert!(harness.state().unsaved_prompt_open());
    harness.get_by_label("Save the changes").click();
    harness.run();
    harness.get_by_label("Name").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Name").type_text("Before quitting");
    harness.run();
    harness.get_by_label("Save").click();
    harness.step();
    harness.step();
    let saved = dir.join("Projects").join("Before quitting.suhina");
    assert!(saved.is_file(), "{:?}", harness.state().status());
    assert!(!unsaved(&harness));
    assert!(!harness.state().unsaved_prompt_open());
    // The window lets go of the projects off its own thread, then closes.
    let mut closed = false;
    for _ in 0..300 {
        if asked_to_close(&harness) {
            closed = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        harness.step();
    }
    assert!(closed, "the window closes once its projects are let go of");
    assert!(harness.state().projects().ids().is_empty());
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_sample_map_opens_in_a_window_of_its_own_and_docks_back() {
    // The map lays itself out on a thread and asks for frames until it is done, which takes
    // longer than `run` waits on a slow machine: step a fixed number of frames instead.
    let mut harness = window(demo_app());
    // Hiding the browser for the test changed a setting already.
    let _ = harness.state_mut().take_changed_settings();
    assert!(harness.query_by_label("Dock").is_none());
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT, Key::M);
    harness.run_steps(4);
    // Without windows of their own (as here) the toolkit draws it as a window inside this one.
    assert!(harness.state().sample_map.view.window);
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.map_window), Some(true));
    assert!(harness.query_by_label("Dock").is_some(), "the map's bar offers to dock it");
    assert!(harness.query_by_label("Large").is_none(), "no large view while it has a window");
    harness.get_by_label("Dock").click();
    harness.run_steps(4);
    assert!(!harness.state().sample_map.view.window);
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.map_window), Some(false));
    assert!(harness.query_by_label("Dock").is_none());

    // The View menu does the same.
    harness.get_by_label("View").click();
    harness.run_steps(4);
    harness.get_by_label_contains("Sample map in a window").click();
    harness.run_steps(4);
    assert!(harness.state().sample_map.view.window);
}

#[test]
fn a_saved_map_window_opens_at_the_start() {
    let mut app = demo_app();
    let settings = suhina_ui::settings::Settings {
        map_window: true,
        map_window_rect: Some([100, 80, 700, 500]),
        browser_open: false,
        ..Default::default()
    };
    app.set_settings(&egui::Context::default(), settings);
    // The map is laid out on a thread of its own, which a slow machine takes longer than a
    // second of frames for: it is given up to ten seconds of real time to finish.
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(0.25)
        .with_max_steps(4)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    let settle = |harness: &mut Harness<'static, App>| {
        for _ in 0..200 {
            if harness.try_run().is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    };
    settle(&mut harness);
    assert!(harness.state().sample_map.view.window);
    assert!(harness.query_by_label("Dock").is_some());
    // The large view takes the map out of its window; the map's layout repaints on the way,
    // so the frames are stepped and not run to rest (a repaint each frame would out-run
    // `Harness::run`'s max steps).
    harness.key_press_modifiers(Modifiers::COMMAND | Modifiers::ALT | Modifiers::SHIFT, Key::M);
    harness.run_steps(3);
    assert!(!harness.state().sample_map.view.window);
    assert!(harness.state().sample_map.view.large);
    assert_eq!(harness.state_mut().take_changed_settings().map(|s| s.map_window), Some(false));
}

/// Renders the Arrangement view with an audio clip, linear and in decibels, to
/// `target/ui-snapshots/arrangement-waveform*.png`, to look at the clip waveform in the window:
/// `cargo test -p suhina-ui --test window -- --ignored render_the_clip_waveform`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_clip_waveform() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    for db in [false, true] {
        let mut app = audio_app();
        app.set_browser_open(false);
        let mut harness = Harness::builder().with_size(SIZE).wgpu().build_ui_state(|ui, app: &mut App| app.ui(ui), app);
        theme::install(&harness.ctx);
        let ctx = harness.ctx.clone();
        let mut settings = harness.state().settings().clone();
        settings.waveform_db = db;
        harness.state_mut().set_settings(&ctx, settings);
        harness.run();
        harness.get_by_label("Arrangement view").click();
        harness.run();
        harness.remove_cursor();
        harness.run();
        let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
        let name = if db { "arrangement-waveform-db.png" } else { "arrangement-waveform.png" };
        image.save(folder.join(name)).unwrap_or_else(|e| panic!("{e}"));
    }
}

#[test]
fn settings_open_as_a_dialog_over_the_view_and_escape_closes_them() {
    let mut harness = window(demo_app());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    assert!(harness.state().settings_open());
    assert!(harness.query_by_label("Close settings").is_some());
    // The view stays behind the dialog.
    assert!(harness.query_by_label("Turn Bass off").is_some(), "the view is still drawn");
    harness.key_press(Key::Escape);
    harness.run();
    assert!(!harness.state().settings_open());
}

/// The wheel and pinch over an open dialog reach only the dialog: the view behind it keeps its
/// scroll and zoom (#232).
#[test]
fn the_wheel_over_open_settings_leaves_the_arrangement_alone() {
    let mut harness = arrangement_app();
    let clip = |h: &Harness<'static, App>| h.get_by_label("Clip Beat B on Drums at bar 17").rect();
    let before = clip(&harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    assert!(harness.state().settings_open());
    // Over the dialog's backdrop, where the clip is, and over the dialog itself.
    for at in [before.center(), egui::pos2(SIZE.0 / 2.0, SIZE.1 / 2.0)] {
        harness.hover_at(at);
        harness.run();
        for modifiers in [Modifiers::NONE, Modifiers::COMMAND, Modifiers::ALT] {
            harness.input_mut().events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(-120.0, -120.0),
                phase: egui::TouchPhase::Move,
                modifiers,
            });
            harness.input_mut().events.push(egui::Event::Zoom(1.5));
            harness.run();
        }
    }
    harness.key_press(Key::Escape);
    harness.run();
    assert!(!harness.state().settings_open());
    assert_eq!(clip(&harness), before, "the arrangement neither scrolled nor zoomed");
}

/// Presses and lets go twice at `at`, a frame apart: a double-click. It waits a second first,
/// as a person would, so an earlier click does not make it a triple-click.
fn double_click_at(harness: &mut Harness<'static, App>, at: egui::Pos2) {
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

/// The name of each clip's content in Keys' slots, `None` for an empty slot.
fn keys_slots(harness: &Harness<'static, App>) -> Vec<Option<String>> {
    let Some(session) = harness.state().session() else { return Vec::new() };
    let p = session.project();
    let keys = p.tracks.iter().find(|t| t.name == "Keys").map(|t| t.id);
    let column = p.launcher.columns.iter().find(|c| Some(c.track) == keys);
    column
        .map(|c| {
            c.slots
                .iter()
                .map(|s| s.clip.as_ref().and_then(|clip| p.content(clip.content)).map(|c| c.name.clone()))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_double_click_on_an_empty_slot_of_a_midi_track_makes_an_empty_clip_there() {
    let mut harness = window_with_step(demo_app(), 1.0 / 60.0);
    assert_eq!(keys_slots(&harness).first(), Some(&None));
    let at = harness.get_by_label("Stop Keys (scene 1)").rect().center();
    double_click_at(&mut harness, at);
    assert_eq!(keys_slots(&harness).first(), Some(&Some("MIDI clip".to_string())));
    assert!(harness.query_by_label("Launch MIDI clip on Keys (scene 1)").is_some());
    // One undo step takes it back.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(keys_slots(&harness).first(), Some(&None));
    // A double-click on a slot with a clip launches it and makes nothing.
    let before = harness.state().session().map(|s| s.project().clone());
    let at = harness.get_by_label("Launch Walk on Bass (scene 2)").rect().center();
    double_click_at(&mut harness, at);
    assert_eq!(harness.state().session().map(|s| s.project().clone()), before);
}

#[test]
fn a_double_click_on_an_empty_slot_of_an_audio_track_makes_an_empty_clip_there() {
    let mut harness = window_with_step(demo_app(), 1.0 / 60.0);
    // An audio track next to the demo's MIDI ones.
    harness.get_by_label("+ Audio track").click();
    harness.run();
    assert!(harness.query_by_label("Stop Audio 1 (scene 1)").is_some(), "the track's empty slot is there");
    let at = harness.get_by_label("Stop Audio 1 (scene 1)").rect().center();
    double_click_at(&mut harness, at);
    assert!(
        harness.query_by_label("Launch Audio clip on Audio 1 (scene 1)").is_some(),
        "the empty audio clip is there"
    );
    // One undo step takes it back.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(harness.query_by_label("Stop Audio 1 (scene 1)").is_some(), "the slot is empty again");
}

/// The audio ask is the three of them at once: the double-click makes the empty audio clip,
/// the menu's name field renames it, and the drag to another slot moves it, like a MIDI clip.
#[test]
fn an_audio_clip_renames_and_moves_like_a_midi_one() {
    let mut harness = window_with_step(demo_app_with_name_field(), 1.0 / 60.0);
    // An audio track and its empty clip, made with the double-click.
    harness.get_by_label("+ Audio track").click();
    harness.run();
    let at = harness.get_by_label("Stop Audio 1 (scene 1)").rect().center();
    double_click_at(&mut harness, at);
    harness.run();
    assert!(harness.query_by_label("Launch Audio clip on Audio 1 (scene 1)").is_some());

    // Renaming through the slot's menu: click into the field, select all, type, Enter.
    harness.get_by_label("Launch Audio clip on Audio 1 (scene 1)").click_secondary();
    harness.run();
    harness.get_by_label_contains("Rename clip Audio clip").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.get_by_label_contains("Rename clip Audio clip").type_text("Break");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.query_by_label("Launch Break on Audio 1 (scene 1)").is_some(), "the audio clip took the name");

    // Dragging it to the next scene moves it.
    let from = harness.get_by_label("Launch Break on Audio 1 (scene 1)").rect().center();
    let to = harness.get_by_label("Stop Audio 1 (scene 2)").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(to);
    harness.step();
    harness.drop_at(to);
    harness.run();
    assert!(harness.query_by_label("Launch Break on Audio 1 (scene 2)").is_some(), "the audio clip moved by drag");
    assert!(harness.query_by_label("Stop Audio 1 (scene 1)").is_some(), "the source emptied");
    assert!(
        matches!(harness.state().status(), Status::Done(text) if text.contains("Moved")),
        "{:?}",
        harness.state().status()
    );
}

#[test]
fn a_double_click_on_an_empty_spot_of_an_audio_lane_makes_a_clip_there() {
    let mut harness = window_with_step(demo_app(), 1.0 / 60.0);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    // An audio track next to the demo's MIDI ones; its empty lane shows in the arrangement.
    harness.get_by_label("+ Audio track").click();
    harness.run();
    assert!(harness.query_by_label("Lane Audio 1").is_some(), "the lane is there");
    let at = harness.get_by_label("Lane Audio 1").rect().center();
    double_click_at(&mut harness, at);
    let made = harness.state().session().and_then(|s| {
        s.project()
            .arrangement
            .lanes
            .iter()
            .find(|l| l.name == "Audio 1")
            .map(|l| l.clips.len().max(1) == l.clips.len() && !l.clips.is_empty())
    });
    assert!(made.unwrap_or(false), "the empty audio clip is in the lane");
    // The made clip is selected; the detail area shows it.
    assert!(harness.query_all_by_label_contains("Audio clip").next().is_some(), "it is selected");
}

#[test]
fn the_delete_key_takes_a_selected_slot_clip_off_its_slot() {
    let mut harness = window_with_step(demo_app(), 1.0 / 60.0);
    // A double-click makes the empty clip in Keys' slot; its name area selects the clip.
    let at = harness.get_by_label("Stop Keys (scene 1)").rect().center();
    double_click_at(&mut harness, at);
    harness.get_by_label("Select MIDI clip on Keys (scene 1)").click();
    harness.run();
    // Delete takes the selected clip off its slot: the empty slot's stop control is back.
    harness.key_press(Key::Delete);
    harness.run();
    assert!(harness.query_by_label("Stop Keys (scene 1)").is_some(), "the slot is empty again");
    assert!(harness.query_by_label("Launch MIDI clip on Keys (scene 1)").is_none(), "the clip is gone");
    // One undo puts it back.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(harness.query_by_label("Launch MIDI clip on Keys (scene 1)").is_some(), "the clip is back");
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
fn a_double_click_on_an_empty_spot_of_a_midi_lane_makes_a_bar_long_clip_on_the_grid_and_selects_it() {
    let mut harness = window_with_step(demo_app(), 1.0 / 60.0);
    harness.get_by_label("Arrangement view").click();
    harness.run();
    let lane = harness.get_by_label("Lane Keys").rect();
    let clips: Vec<egui::Rect> = harness.query_all_by_label_contains(" on Keys at bar ").map(|n| n.rect()).collect();
    // The rightmost spot of the lane that no clip covers, a little into a bar (8 points a beat).
    let at = (0..200)
        .map(|i| egui::pos2(lane.right() - 12.0 - i as f32 * 8.0, lane.center().y))
        .find(|p| lane.contains(*p) && !clips.iter().any(|c| c.expand(2.0).contains(*p)))
        .unwrap();
    let count = |h: &Harness<'static, App>| {
        h.state().session().map_or(0, |s| {
            let p = s.project();
            let keys = p.tracks.iter().find(|t| t.name == "Keys").map(|t| t.id);
            p.arrangement.lanes.iter().filter(|l| l.track == keys).map(|l| l.clips.len()).sum::<usize>()
        })
    };
    let before = count(&harness);
    double_click_at(&mut harness, at);
    assert_eq!(count(&harness), before + 1);
    let session = harness.state().session().unwrap();
    let p = session.project();
    let keys = p.tracks.iter().find(|t| t.name == "Keys").map(|t| t.id).unwrap();
    let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(keys)).unwrap();
    let made = lane.clips.iter().find(|c| p.content(c.content).is_some_and(|x| x.name == "MIDI clip")).unwrap();
    assert_eq!(made.length, 4.0, "one bar of 4/4");
    assert_eq!(made.start % 4.0, 0.0, "on the bar grid at this zoom: {}", made.start);
    assert_eq!(harness.state().selected_clip(), Some((keys, made.id)), "the new clip is selected");
}

#[test]
fn a_clips_name_selects_it_and_its_mark_launches_it() {
    let mut harness = view();
    let walk = (TrackId(2), 1);
    assert!(clicked(&mut harness, "Select Walk on Bass (scene 2)").is_empty(), "selecting does not launch");
    assert_eq!(harness.state().state.selected_slot, Some(walk));
    assert_eq!(harness.state().state.selected, Some(TrackId(2)), "its track is selected with it");

    // A launch selects the clip too, unless Select on launch is off.
    assert_eq!(
        clicked(&mut harness, "Launch Chords 1 on Keys (scene 2)"),
        [Action::new("clip.launch", json!({ "track": 3, "scene": 2 }))]
    );
    assert_eq!(harness.state().state.selected_slot, Some((TrackId(3), 1)));
    harness.state_mut().state.keep_selection_on_launch = true;
    clicked(&mut harness, "Launch Walk on Bass (scene 2)");
    assert_eq!(harness.state().state.selected_slot, Some((TrackId(3), 1)));

    // A track's header selects the track and ends the clip's selection.
    clicked(&mut harness, "Track Drums");
    assert_eq!((harness.state().state.selected_slot, harness.state().state.selected), (None, Some(TrackId(1))));
}

#[test]
fn the_arrows_step_through_the_slots_of_tracks_and_stop_at_the_edges() {
    use suhina_ui::session::session_view::{Step, step_slot};
    let grid = Grid::build(&demo_project(), None);
    let (drums, bass, vox) = (TrackId(1), TrackId(2), TrackId(4));
    assert_eq!(step_slot(&grid, (bass, 1), Step::Up), Some((bass, 0)));
    assert_eq!(step_slot(&grid, (bass, 1), Step::Down), Some((bass, 2)));
    assert_eq!(step_slot(&grid, (bass, 1), Step::Left), Some((drums, 1)));
    assert_eq!(step_slot(&grid, (bass, 1), Step::Right), Some((TrackId(3), 1)));
    assert_eq!(step_slot(&grid, (bass, 0), Step::Up), None);
    assert_eq!(step_slot(&grid, (bass, 5), Step::Down), None);
    assert_eq!(step_slot(&grid, (drums, 2), Step::Left), None);
    // Return tracks have no slots, so the last track is the edge.
    assert_eq!(step_slot(&grid, (vox, 2), Step::Right), None);
    assert_eq!(step_slot(&grid, (TrackId(10), 0), Step::Down), None, "a return track is not in the grid of slots");
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

/// Bass's send to the A Reverb return in the project: level and before the fader.
fn bass_send(harness: &Harness<'static, App>) -> Option<(f64, bool)> {
    let project = harness.state().session()?.project();
    let bass = project.tracks.iter().find(|t| t.name == "Bass")?.channel?;
    let reverb = project.tracks.iter().find(|t| t.name == "A Reverb")?.channel?;
    let send = project.channel(bass)?.sends.iter().find(|s| s.target == reverb)?;
    Some((send.level, send.pre_fader))
}

#[test]
fn a_send_knob_sets_the_send_level_and_its_menu_the_mode() {
    let mut harness = window(demo_app());
    let before = bass_send(&harness).map(|(level, _)| 20.0 * level.log10()).unwrap_or_default();
    let knob = harness.get_by_label("Bass send to A Reverb").rect().center();
    harness.hover_at(knob);
    harness.run();
    // Stepped: the send is heard while it is dragged, an edit every frame (#705).
    harness.drag_at(knob);
    harness.step();
    harness.hover_at(knob - egui::vec2(0.0, 10.0));
    harness.step();
    let during = bass_send(&harness).map(|(level, _)| 20.0 * level.log10()).unwrap_or_default();
    assert!(during > before, "louder while dragged: {before} to {during}");
    harness.hover_at(knob - egui::vec2(0.0, 20.0));
    harness.step();
    harness.drop_at(knob - egui::vec2(0.0, 20.0));
    harness.step();
    harness.step();
    // 20 points up at 0.25 dB a point: 5 dB louder, rounded to 0.1 dB.
    let after = bass_send(&harness).map(|(level, _)| 20.0 * level.log10()).unwrap_or_default();
    assert!((after - before - 5.0).abs() < 0.11, "{before} to {after}");

    harness.get_by_label("Bass send to A Reverb").click_secondary();
    harness.run();
    harness.get_by_label("Before the fader (pre)").click();
    harness.run();
    assert_eq!(bass_send(&harness).map(|(_, pre)| pre), Some(true));

    // Two undo steps: the mode, then the level.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    let undone = bass_send(&harness).map(|(level, pre)| (20.0 * level.log10(), pre));
    assert!(undone.is_some_and(|(db, pre)| (db - before).abs() < 1e-9 && !pre), "{undone:?}");
}

/// A project saved in `dir` as `Song.suhina`, then changed and backed up by a run that never
/// ends normally (a crash), its backups and journal in `dir/settings`. Returns the project file
/// and the backup.
#[allow(clippy::unwrap_used)] // a test helper outside #[test] functions
fn crashed_run(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let file = dir.join("Song.suhina");
    Session::new(demo_project()).save(&file, false).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let mut projects = suhina_session::Projects::new();
    projects.set_backup_folder(Some(dir.join("settings")));
    projects.add(Session::open(&file).unwrap().0, None).unwrap();
    projects.execute("transport.setTempo", &json!({ "bpm": 97 })).unwrap();
    let written = projects.execute("project.autosave", &json!({ "now": true })).unwrap();
    (file, std::path::PathBuf::from(written["written"].as_str().unwrap()))
}

#[test]
fn a_backup_a_crash_left_is_offered_on_start_and_opens_tied_to_its_file() {
    let dir = std::env::temp_dir().join(format!("suhina-ui-recovery-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&dir);
    let (file, backup) = crashed_run(&dir);

    let mut app = App::new();
    app.set_browser_open(false);
    app.projects_mut().set_backup_folder(Some(dir.join("settings")));
    app.offer_recovery();
    let mut harness = window(app);
    assert!(harness.query_by_label("Open the backup").is_some());
    // Show the folder hands the backup to the app and keeps asking.
    harness.get_by_label("Show the backup's folder").click();
    harness.run();
    assert_eq!(harness.state_mut().take_reveal_request(), Some(backup.clone()));
    assert!(harness.query_by_label("Open the backup").is_some());
    harness.get_by_label("Open the backup").click();
    harness.run();
    assert!(harness.query_by_label("Open the backup").is_none());
    let offer = harness.state_mut().take_recovery_request().expect("the backup to open");
    assert_eq!((offer.backup.clone(), offer.file.clone()), (backup.clone(), Some(file.clone())));
    // What the app does with it.
    let (session, _) = Session::recover(&offer.backup, offer.file.as_deref()).unwrap();
    harness.state_mut().open_project(session, &[], OpenAs::NewTab);
    harness.run();
    let session = harness.state().session().unwrap();
    assert!(session.has_unsaved_changes());
    assert_eq!((session.source(), session.project().transport.tempo_bpm), (Some(file.as_path()), 97.0));

    // Offered once: the next start finds nothing.
    assert!(harness.state_mut().finish_backups());
    let mut next = App::new();
    next.projects_mut().set_backup_folder(Some(dir.join("settings")));
    next.offer_recovery();
    let harness = window(next);
    assert!(harness.query_by_label("Open the backup").is_none());
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn ignoring_an_offered_backup_leaves_it_and_asks_no_more() {
    let dir = std::env::temp_dir().join(format!("suhina-ui-recovery-ignore-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&dir);
    let (_file, backup) = crashed_run(&dir);
    let mut app = App::new();
    app.projects_mut().set_backup_folder(Some(dir.join("settings")));
    app.offer_recovery();
    let mut harness = window(app);
    harness.get_by_label("Ignore the backup").click();
    harness.run();
    assert!(harness.query_by_label("Open the backup").is_none());
    assert!(harness.state_mut().take_recovery_request().is_none());
    assert!(harness.state().projects().is_empty());
    assert!(harness.state_mut().finish_backups());
    assert!(backup.is_file(), "the backup stays in its folder");
    let mut next = App::new();
    next.projects_mut().set_backup_folder(Some(dir.join("settings")));
    next.offer_recovery();
    assert!(window(next).query_by_label("Open the backup").is_none());
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_critical_command_from_the_window_is_backed_up_first() {
    let dir = std::env::temp_dir().join(format!("suhina-ui-critical-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&dir);
    let file = dir.join("Song.suhina");
    Session::new(demo_project()).save(&file, false).unwrap();
    let mut app = App::new();
    app.projects_mut().set_backup_folder(Some(dir.join("settings")));
    app.projects_mut().set_backup_settings(suhina_session::backup::Interval::Critical, true);
    app.set_session(Session::open(&file).unwrap().0, &[]);
    app.run(&Action::new("transport.setTempo", json!({ "bpm": 101 })));
    assert!(!dir.join(".backup").exists(), "an ordinary edit is not critical");
    let track = app.session().unwrap().project().tracks.first().unwrap().id.0;
    app.run(&Action::new("track.delete", json!({ "track": track })));
    let tracks = app.session().unwrap().project().tracks.len();
    // The backup thread finishes, and quitting is critical too: a second backup has the delete.
    assert!(app.finish_backups());
    let mut kept: Vec<usize> = std::fs::read_dir(dir.join(".backup"))
        .unwrap()
        .map(|e| Session::open(&e.unwrap().path()).unwrap().0.project().tracks.len())
        .collect();
    kept.sort();
    assert_eq!(kept, [tracks, tracks + 1]);
    let _clean = std::fs::remove_dir_all(&dir);
}

/// Picks `value` in the control bar's grid box, as a user does: open it, click the value.
fn choose_grid(harness: &mut Harness<'static, App>, value: &str) {
    harness.get_by_label("Grid and launch quantisation").click();
    harness.run();
    harness.get_by_label(value).click();
    harness.run();
}

/// The grid value of the active project's session.
fn grid_of(harness: &Harness<'static, App>) -> Option<suhina_engine::Quantization> {
    harness.state().session().map(|s| s.quantization())
}

/// Zooms the Arrangement view in eight times (64 points a beat) around a point just inside
/// Beat B's left edge, so that edge stays in view; returns where the edge is then.
fn zoom_in_at_beat_b(harness: &mut Harness<'static, App>) -> f32 {
    let clip = harness.get_by_label("Clip Beat B on Drums at bar 17").rect();
    // 8 points a beat by default: 4 points in is half a beat into the clip, which starts on beat 64.
    let at = egui::pos2(clip.left() + 4.0, clip.center().y);
    harness.hover_at(at);
    harness.run();
    harness.input_mut().events.push(egui::Event::Zoom(8.0));
    harness.run();
    let left = harness.get_by_label("Clip Beat B on Drums at bar 17").rect().left();
    assert!((left - (at.x - 32.0)).abs() <= 1.0, "zoomed to 64 points a beat around the pointer: {left}");
    left
}

/// Drags Beat B by `by` points, taking hold of it just inside its left edge, which stays in
/// view when zoomed in.
fn drag_beat_b(harness: &mut Harness<'static, App>, by: egui::Vec2) {
    let clip = harness.get_by_label("Clip Beat B on Drums at bar 17").rect();
    // By its name bar: its body selects time.
    let from = egui::pos2(clip.left() + 16.0, clip.top() + 6.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + by * 0.5);
    harness.run();
    harness.hover_at(from + by);
    harness.run();
    harness.drop_at(from + by);
    harness.run();
}

/// Where Beat B (on beat 64 in the demo) starts now, after a move of less than two bars.
fn beat_b_start(harness: &Harness<'static, App>) -> Option<f64> {
    harness
        .state()
        .session()?
        .project()
        .arrangement
        .lanes
        .first()?
        .clips
        .iter()
        .map(|c| c.start)
        .find(|s| (63.9..72.0).contains(s))
}

#[test]
fn the_grid_value_in_the_control_bar_sets_where_a_dragged_clip_lands() {
    let mut harness = arrangement_app();
    assert_eq!(grid_of(&harness), Some(suhina_engine::Quantization::Bars(1)));
    // No audio engine is needed to choose the grid.
    choose_grid(&mut harness, "1/16");
    assert_eq!(grid_of(&harness), Some(suhina_engine::Quantization::Beats(0.25)));
    assert!(matches!(harness.state().status(), Status::Done(s) if s == "Grid 1/16"));
    zoom_in_at_beat_b(&mut harness);

    // 1.3 beats right (83.2 points at 64 a beat): on the nearest sixteenth, beat 65.25.
    drag_beat_b(&mut harness, egui::vec2(83.2, 0.0));
    assert_eq!(beat_b_start(&harness), Some(65.25));
    // The move is one undo step.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(beat_b_start(&harness), Some(64.0));

    // With Alt held it lands where it is let go, off the grid.
    harness.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    drag_beat_b(&mut harness, egui::vec2(83.2, 0.0));
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    harness.run();
    let free = beat_b_start(&harness).unwrap_or_default();
    assert!((free - 65.3).abs() < 0.02 && (free * 4.0).fract().abs() > 0.01, "{free}");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();

    // 1 bar: 3.5 beats right lands on the next bar, beat 68, at the same zoom.
    choose_grid(&mut harness, "1 bar");
    drag_beat_b(&mut harness, egui::vec2(224.0, 0.0));
    assert_eq!(beat_b_start(&harness), Some(68.0));
}

#[test]
fn a_triplet_or_none_grid_places_a_dragged_clip_by_its_own_rule() {
    let mut harness = arrangement_app();
    choose_grid(&mut harness, "1/16T");
    zoom_in_at_beat_b(&mut harness);
    // 1.3 beats right: the nearest sixth of a beat is 64 + 8/6.
    drag_beat_b(&mut harness, egui::vec2(83.2, 0.0));
    let start = beat_b_start(&harness).unwrap_or_default();
    assert!((start - (64.0 + 8.0 / 6.0)).abs() < 1e-9, "{start}");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();

    // none: nothing snaps, as if Alt were held.
    choose_grid(&mut harness, "none");
    assert_eq!(grid_of(&harness), Some(suhina_engine::Quantization::None));
    drag_beat_b(&mut harness, egui::vec2(83.2, 0.0));
    let free = beat_b_start(&harness).unwrap_or_default();
    assert!((free - 65.3).abs() < 0.02 && (free * 4.0).fract().abs() > 0.01, "{free}");
}

#[test]
fn each_project_tab_keeps_its_own_grid() {
    let mut harness = two_projects();
    // The browser has tests of its own; the views get the whole width.
    harness.state_mut().set_browser_open(false);
    harness.run();
    let ids = harness.state().projects().ids();
    // The second project shows: 1/16 there.
    choose_grid(&mut harness, "1/16");
    assert_eq!(grid_of(&harness), Some(suhina_engine::Quantization::Beats(0.25)));

    // The first keeps 1 bar: in its Arrangement a short drag at a fine zoom lands on a bar.
    harness.get_by_label("Tab Untitled 1").click();
    harness.run();
    harness.run();
    assert_eq!(harness.state().projects().active(), ids.first().copied());
    assert_eq!(grid_of(&harness), Some(suhina_engine::Quantization::Bars(1)));
    harness.get_by_label("Arrangement view").click();
    harness.run();
    zoom_in_at_beat_b(&mut harness);
    drag_beat_b(&mut harness, egui::vec2(160.0, 0.0));
    assert_eq!(beat_b_start(&harness), Some(68.0));

    // Back in the second, its 1/16 is still chosen and is its grid.
    harness.get_by_label("Tab Untitled 2").click();
    harness.run();
    harness.run();
    assert_eq!(grid_of(&harness), Some(suhina_engine::Quantization::Beats(0.25)));
    harness.get_by_label("Arrangement view").click();
    harness.run();
    zoom_in_at_beat_b(&mut harness);
    drag_beat_b(&mut harness, egui::vec2(83.2, 0.0));
    assert_eq!(beat_b_start(&harness), Some(65.25));
}

/// A window with nothing open that starts as `settings` say, given the last project `last`.
fn started(settings: suhina_ui::settings::Settings, last: Option<std::path::PathBuf>) -> Harness<'static, App> {
    let mut app = App::new();
    app.set_settings(&egui::Context::default(), settings);
    app.open_at_start(last);
    window(app)
}

fn at_start(choice: suhina_ui::settings::AtStart) -> suhina_ui::settings::Settings {
    suhina_ui::settings::Settings { at_start: choice, ..Default::default() }
}

#[test]
fn at_start_the_start_panel_shows_unless_the_settings_say_otherwise() {
    use suhina_ui::settings::AtStart;
    let mut harness = started(at_start(AtStart::Panel), None);
    assert!(!harness.state().project_dialog_open());
    assert!(harness.state().start_panel_open());
    assert!(harness.query_by_label("New project").is_some(), "the start panel shows");
    assert!(harness.state_mut().take_lists_wanted().is_some(), "and asks for its lists");
    assert_eq!(harness.state_mut().take_new_request(), None);

    let mut harness = started(at_start(AtStart::Blank), None);
    assert!(!harness.state().project_dialog_open());
    assert_eq!(harness.state_mut().take_new_request(), Some(suhina_ui::NewProject::Blank));
}

#[test]
fn at_start_a_new_project_comes_from_the_start_template_or_the_panel_says_why_not() {
    use suhina_ui::settings::AtStart;
    let folder = std::env::temp_dir().join(format!("suhina-at-start-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    let template = folder.join("Four on the floor.suhina");
    std::fs::write(&template, b"{}").unwrap_or_else(|e| panic!("{e}"));
    let mut settings = at_start(AtStart::Template);
    settings.templates_folder = folder.display().to_string();
    // A name in the templates folder, or a whole path.
    for named in ["Four on the floor.suhina".to_string(), template.display().to_string()] {
        settings.start_template = named;
        let mut harness = started(settings.clone(), None);
        assert!(!harness.state().project_dialog_open());
        assert_eq!(harness.state_mut().take_new_request(), Some(suhina_ui::NewProject::Template(template.clone())));
    }
    settings.start_template = "Gone.suhina".into();
    let mut harness = started(settings.clone(), None);
    assert!(!harness.state().project_dialog_open());
    assert!(harness.query_by_label("New project").is_some(), "the start panel shows");
    assert_eq!(harness.state_mut().take_new_request(), None);
    assert!(
        matches!(harness.state().status(), Status::Hint(h) if h.contains("not there")),
        "{:?}",
        harness.state().status()
    );
    settings.start_template = String::new();
    let harness = started(settings, None);
    assert!(harness.query_by_label("New project").is_some());
    assert!(matches!(harness.state().status(), Status::Hint(h) if h.contains("No template")));
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn at_start_the_last_project_opens_or_the_panel_says_there_is_none() {
    use suhina_ui::settings::AtStart;
    let last = std::path::PathBuf::from("/songs/Last night.suhina");
    let mut harness = started(at_start(AtStart::Last), Some(last.clone()));
    assert!(!harness.state().project_dialog_open());
    assert_eq!(harness.state_mut().take_open_request(), Some((last, OpenAs::NewTab)));

    let mut harness = started(at_start(AtStart::Last), None);
    assert!(harness.query_by_label("New project").is_some());
    assert_eq!(harness.state_mut().take_open_request(), None);
    assert!(matches!(harness.state().status(), Status::Hint(h) if h.contains("No project was opened before")));
}

#[test]
fn at_start_is_chosen_in_settings_and_template_asks_for_the_template() {
    use suhina_ui::settings::AtStart;
    let mut harness = window(demo_app());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    assert!(harness.query_by_label("Start template").is_none(), "only for a template");
    harness.get_by_label("At start: Template").click();
    harness.run();
    assert_eq!(harness.state().settings().at_start, AtStart::Template);
    assert!(harness.query_by_label("Start template").is_some());
    harness.get_by_label("At start: Last").click();
    harness.run();
    assert_eq!(harness.state().settings().at_start, AtStart::Last);
    let saved = harness.state_mut().take_changed_settings().map(|s| s.to_text()).unwrap_or_default();
    assert!(saved.contains("at_start = last"), "{saved}");
}

#[test]
fn the_start_panel_opens_recent_projects_forgets_them_and_comes_back_when_the_last_project_closes() {
    let night = std::path::PathBuf::from("/songs/Night drive.suhina");
    let morning = std::path::PathBuf::from("/songs/Morning.suhina");
    let mut app = App::new();
    app.set_project_lists(vec![night.clone(), morning.clone()], Vec::new());
    let mut harness = window(app);
    assert!(harness.state().start_panel_open());
    harness.get_by_label("Open Night drive").click();
    harness.run();
    assert_eq!(harness.state_mut().take_open_request(), Some((night, OpenAs::NewTab)));

    harness.get_by_label("Open Morning").click_secondary();
    harness.run();
    harness.get_by_label("Remove from list").click();
    harness.run();
    assert_eq!(harness.state_mut().take_forget_request(), Some(morning));
    assert!(harness.query_by_label("Open Morning").is_none(), "it is gone from the panel at once");

    harness.get_by_label("Audio settings").click();
    // The Audio page repaints while it shows, for its live figures.
    harness.run_steps(2);
    assert!(harness.state().settings_open());
    harness.key_press(Key::Escape);
    harness.run_steps(2);
    assert!(!harness.state().settings_open());

    harness.get_by_label("New project").click();
    harness.run();
    assert_eq!(harness.state_mut().take_new_request(), Some(suhina_ui::NewProject::Blank));

    // With a project open the panel is gone; when the last one closes it is back, with its lists
    // read again.
    let _ = harness.state_mut().take_lists_wanted();
    harness.state_mut().set_session(Session::new(demo_project()), &[]);
    harness.run();
    assert!(harness.query_by_label("New project").is_none());
    harness.get_by_label("Projects").click();
    harness.run();
    harness.get_by_label("Close project").click();
    harness.run();
    if let Some(dont) = harness.query_by_label("Don't save the changes") {
        dont.click();
        harness.run();
    }
    assert!(harness.state().start_panel_open());
    assert!(harness.query_by_label("New project").is_some());
    assert!(harness.state_mut().take_lists_wanted().is_some());
}

/// Renders the start panel to `target/ui-snapshots/start-panel.png`. Needs a GPU adapter:
/// `cargo test -p suhina-ui -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_start_panel() {
    let mut app = App::new();
    app.set_project_lists(
        vec![
            "C:/Music/Suhina/Projects/Night drive.suhina".into(),
            "C:/Music/Suhina/Projects/Morning.suhina".into(),
            "C:/Music/Sets/Old jam.als".into(),
        ],
        vec!["C:/Music/Suhina/Templates/Beat starter.suhina".into()],
    );
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(2.0)
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("{e}"));
    image.save(folder.join("start-panel.png")).unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn browse_buttons_ask_for_the_systems_dialog_and_fill_their_fields() {
    use suhina_ui::interaction::pick::PickField;
    // Settings › General: the folders and the start template.
    let mut harness = window(demo_app());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();
    harness.get_by_label("Browse for Projects folder").click();
    harness.run();
    let request = harness.state_mut().take_pick_request().unwrap_or_else(|| panic!("no dialog asked for"));
    assert_eq!(request.field, PickField::ProjectsFolder);
    assert!(request.field.folder());
    let _ = harness.state_mut().take_changed_settings();
    harness.state_mut().set_picked(PickField::ProjectsFolder, "/music/projects".into());
    harness.run();
    assert_eq!(harness.state().settings().projects_folder, "/music/projects");
    let saved = harness.state_mut().take_changed_settings().map(|s| s.to_text()).unwrap_or_default();
    assert!(saved.contains("projects_folder = /music/projects"), "{saved}");
    harness.get_by_label("At start: Template").click();
    harness.run();
    harness.get_by_label("Browse for Start template").click();
    harness.run();
    let request = harness.state_mut().take_pick_request().unwrap_or_else(|| panic!("no dialog asked for"));
    assert_eq!(request.field, PickField::StartTemplate);
    assert_eq!(request.field.extensions(), ["suhina", "als"]);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    harness.run();

    // Save as: the folder; the prompt stays open meanwhile.
    harness.state_mut().do_action("file.saveAs", &mut Vec::new());
    harness.run();
    harness.get_by_label("Browse for the folder").click();
    harness.run();
    assert_eq!(harness.state_mut().take_pick_request().map(|r| r.field), Some(PickField::SaveFolder));
    harness.state_mut().set_picked(PickField::SaveFolder, "/music/songs".into());
    harness.run();
    assert_eq!(harness.get_by_label("Folder").value().as_deref(), Some("/music/songs"));
    harness.key_press(Key::Escape);
    harness.run();

    // File › Open…: a project, which then opens; cancelling the dialog keeps File › Open.
    harness.state_mut().open_project_dialog(true);
    harness.run();
    harness.get_by_label("Browse for a project").click();
    harness.run();
    assert_eq!(harness.state_mut().take_pick_request().map(|r| r.field), Some(PickField::OpenProject));
    assert!(harness.state().project_dialog_open(), "the dialog stays while the system's is shown");
    harness.state_mut().set_picked(PickField::OpenProject, "/music/songs/Night.suhina".into());
    harness.run();
    assert!(!harness.state().project_dialog_open());
    // A project is open, so Suhina asks how to open it, as for any other way of opening one.
    harness.get_by_label("In a new tab").click();
    harness.run();
    assert_eq!(
        harness.state_mut().take_open_request().map(|(path, _)| path),
        Some(std::path::PathBuf::from("/music/songs/Night.suhina"))
    );
}

#[test]
fn open_on_the_start_panel_shows_the_systems_dialog() {
    use suhina_ui::interaction::pick::PickField;
    let mut harness = window(App::new());
    harness.get_by_label("Open a project").click();
    harness.run();
    assert_eq!(harness.state_mut().take_pick_request().map(|r| r.field), Some(PickField::OpenProject));
    harness.state_mut().set_picked(PickField::OpenProject, "/music/Old jam.als".into());
    harness.run();
    assert_eq!(
        harness.state_mut().take_open_request().map(|(path, _)| path),
        Some(std::path::PathBuf::from("/music/Old jam.als"))
    );
}

#[test]
fn help_about_shows_the_version_and_copies_it() {
    let mut harness = window(demo_app());
    let version = suhina_ui::app::project_dialog::VERSION;
    let build = suhina_ui::app::project_dialog::BUILD;
    assert!(!build.is_empty(), "the build script sets the build string");
    assert!(harness.query_by_label_contains(version).is_none());
    harness.get_by_label("Help").click();
    harness.run();
    harness.get_by_label_contains("About Suhina").click();
    harness.run();
    let line = suhina_ui::app::project_dialog::version_line();
    assert!(line.starts_with(&format!("Suhina {version} build ")), "{line}");
    assert!(line.contains(build), "the copied line carries the build string: {line}");
    harness.get_by_label(&format!("Copy version: {line}")).click();
    harness.run();
    assert_eq!(harness.state().status(), &Status::Done(format!("Copied {line}")));
    // Still open after copying; Escape closes it.
    assert!(harness.query_by_label_contains(version).is_some());
    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.query_by_label_contains(version).is_none());
}

#[test]
fn help_check_for_updates_opens_the_dialog_and_offers_no_gh_on_the_nightly_channel() {
    let mut harness = window(demo_app());
    harness.get_by_label("Help").click();
    harness.run();
    harness.get_by_label_contains("Check for updates").click();
    harness.run();
    harness.run();
    assert!(harness.query_by_label("Check for updates now").is_some());
    // The GitHub CLI is only offered for test builds, and only connected on a click.
    assert!(harness.query_by_label("Connect Suhina to your gh").is_none());
    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.query_by_label("Check for updates now").is_none());
}

#[test]
fn a_slots_menu_copies_its_clip_and_pastes_once_something_is_copied() {
    let mut harness = view();
    harness.get_by_label("Launch Walk on Bass (scene 2)").click_secondary();
    harness.run();
    assert_eq!(harness.state().state.selected_slot, Some((suhina_engine::model::TrackId(2), 1)));
    harness.state_mut().actions.clear();
    harness.get_by_label_contains("Copy").click();
    harness.run();
    assert_eq!(harness.state().actions, [Action::new("clip.copy", json!({ "track": 2, "scene": 2 }))]);

    // Nothing copied: Paste is there but does nothing.
    harness.get_by_label("Stop Keys (scene 1)").click_secondary();
    harness.run();
    harness.state_mut().actions.clear();
    harness.get_by_label_contains("Paste").click();
    harness.run();
    assert!(harness.state().actions.is_empty());
    harness.key_press(Key::Escape);
    harness.run();

    harness.state_mut().state.can_paste = true;
    harness.run();
    harness.get_by_label("Stop Keys (scene 1)").click_secondary();
    harness.run();
    harness.state_mut().actions.clear();
    harness.get_by_label_contains("Paste").click();
    harness.run();
    assert_eq!(harness.state().actions, [Action::new("clip.paste", json!({ "track": 3, "scene": 1 }))]);
}

#[test]
fn a_playing_trigger_clips_click_stops_it() {
    let mut harness = view();
    // Walk's slot in scene 2, playing: its mark is the stop square, and the click stops the
    // track (a trigger clip listens for no release, so the press stops instead of relaunching).
    harness.state_mut().grid.columns[1].slots[1].state = SlotState::Playing;
    harness.run();
    assert_eq!(
        clicked(&mut harness, "Stop Walk on Bass (scene 2)"),
        [Action::new("track.stop", json!({ "track": 2 }))]
    );
}

#[test]
fn clicking_a_slot_selects_it_and_still_launches_or_stops() {
    let mut harness = view();
    assert_eq!(
        clicked(&mut harness, "Stop Keys (scene 1)"),
        [Action::new("clip.launch", json!({ "track": 3, "scene": 1 }))]
    );
    assert_eq!(harness.state().state.selected_slot, Some((suhina_engine::model::TrackId(3), 0)));
    clicked(&mut harness, "Launch Walk on Bass (scene 2)");
    assert_eq!(harness.state().state.selected_slot, Some((suhina_engine::model::TrackId(2), 1)));
}

/// What Drums' slot in `scene` (from 0) holds: its clip's content name.
fn drums_slot(harness: &Harness<'static, App>, scene: usize) -> Option<String> {
    let session = harness.state().session()?;
    let p = session.project();
    let clip = p.launcher.columns[0].slots[scene].clip.as_ref()?;
    p.content(clip.content).map(|c| c.name.clone())
}

#[test]
fn an_arrangement_clip_copied_with_ctrl_c_pastes_into_the_selected_slot() {
    let mut harness = arrangement_app();
    harness.get_by_label("Clip Fill on Drums at bar 29").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::C);
    harness.run();
    harness.key_press(Key::Tab);
    harness.run();
    assert_eq!(harness.state().view(), View::Session);
    // Ctrl+V with no slot picked says what to do.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    assert!(matches!(harness.state().status(), Status::Hint(_)));
    harness.get_by_label("Stop Drums (scene 5)").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    assert_eq!(drums_slot(&harness, 4).as_deref(), Some("Fill"));
    let arranged = harness.state().session().map_or(0, |s| s.project().arrangement.lanes[0].clips.len());
    assert_eq!(arranged, 4, "the arrangement keeps its clips");
    // One undo step empties the slot again.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(drums_slot(&harness, 4), None);

    // A slot's clip copies with Ctrl+C and pastes into another slot.
    harness.get_by_label("Launch Fill on Drums (scene 4)").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::C);
    harness.run();
    harness.get_by_label("Stop Drums (scene 5)").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    assert_eq!(drums_slot(&harness, 4).as_deref(), Some("Fill"));
}

/// Holds the pointer still for a second, a frame at a time, so a hover that waits fires.
fn wait_over(harness: &mut Harness<'static, App>, at: egui::Pos2) {
    for _ in 0..4 {
        harness.hover_at(at);
        harness.step();
    }
}

#[test]
fn an_arrangement_clip_dragged_over_the_session_switch_drops_into_a_slot() {
    let mut harness = arrangement_app();
    let from = harness.get_by_label("Clip Fill on Drums at bar 29").rect().center_top() + egui::vec2(0.0, 6.0);
    let switch = harness.get_by_label("Session view").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(from + egui::vec2(10.0, 0.0));
    harness.step();
    wait_over(&mut harness, switch);
    assert_eq!(harness.state().view(), View::Session);
    let slot = harness.get_by_label("Stop Drums (scene 5)").rect().center();
    harness.hover_at(slot);
    harness.step();
    harness.drop_at(slot);
    harness.run();
    assert_eq!(drums_slot(&harness, 4).as_deref(), Some("Fill"));
    let starts: Vec<f64> = harness
        .state()
        .session()
        .map(|s| s.project().arrangement.lanes[0].clips.iter().map(|c| c.start).collect())
        .unwrap_or_default();
    assert_eq!(starts, [0.0, 64.0, 112.0, 128.0], "the arrangement clip stays where it was");
}

#[test]
fn a_slot_clip_dragged_over_the_arrangement_switch_drops_on_a_lane() {
    let mut harness = window(demo_app());
    let from = harness.get_by_label("Launch Walk on Bass (scene 2)").rect().center();
    let switch = harness.get_by_label("Arrangement view").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(from + egui::vec2(0.0, 12.0));
    harness.step();
    wait_over(&mut harness, switch);
    assert_eq!(harness.state().view(), View::Arrangement);
    // Bars 1 to 8 of Bass are empty; Root starts at bar 9, 8 points a beat.
    let root = harness.get_by_label("Clip Root on Bass at bar 9").rect();
    let to = egui::pos2(root.left() - 16.0 * 8.0, root.center().y);
    harness.hover_at(to);
    harness.step();
    harness.drop_at(to);
    harness.run();
    let bass: Vec<(f64, f64)> = harness
        .state()
        .session()
        .map(|s| s.project().arrangement.lanes[1].clips.iter().map(|c| (c.start, c.length)).collect())
        .unwrap_or_default();
    assert!(bass.contains(&(16.0, 4.0)), "{bass:?}");
    assert!(matches!(harness.state().status(), Status::Done(text) if text.contains("Copied Walk to Bass")));
}

#[test]
fn escape_or_a_drop_on_nothing_leaves_the_project_alone() {
    let project = |h: &Harness<'static, App>| h.state().session().map(|s| s.project().clone());
    let mut harness = window(demo_app());
    let before = project(&harness);
    let from = harness.get_by_label("Launch Walk on Bass (scene 2)").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(from + egui::vec2(0.0, 12.0));
    harness.step();
    harness.key_press(Key::Escape);
    harness.run();
    harness.drop_at(from + egui::vec2(0.0, 12.0));
    harness.run();
    assert_eq!(project(&harness), before);
    // Dropped back on its own slot, or on a track header: nothing either.
    for to in [from, harness.get_by_label("Track Keys").rect().center()] {
        harness.hover_at(from);
        harness.step();
        harness.drag_at(from);
        harness.step();
        harness.hover_at(from + egui::vec2(0.0, 12.0));
        harness.step();
        harness.hover_at(to);
        harness.step();
        harness.drop_at(to);
        harness.run();
        assert_eq!(project(&harness), before);
    }
}

/// Needs an audio device, so it is ignored by default, like the preview test.
#[test]
#[ignore]
fn launching_a_clip_greys_the_arrangement_until_back_to_arrangement() {
    let mut session = Session::new(demo_project());
    let (player, _) = suhina_output::open_player(session.project(), session.audio()).unwrap();
    session.attach_player(player);
    let mut app = App::new();
    app.set_session(session, &[]);
    let mut harness = window(app);
    harness.run();
    // Launch a clip while the transport plays, on a fine grid so it fires at once.
    harness.state_mut().projects_mut().execute("transport.play", &json!({})).unwrap();
    harness.state_mut().projects_mut().execute("launcher.setQuantization", &json!({ "value": "1/32" })).unwrap();
    harness.state_mut().projects_mut().execute("clip.launch", &json!({ "track": 1, "scene": 1 })).unwrap();
    for _ in 0..30 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    eprintln!(
        "LAUNCHER STATE: {:?}",
        harness.state_mut().projects_mut().execute("launcher.state", &json!({})).unwrap()
    );
    // The Arrangement view: the Drums lane dims and Back to arrangement shows in the ruler.
    harness.get_by_label("Arrangement view").click();
    harness.run_steps(5);
    assert!(
        harness.query_by_label("Back to arrangement").is_some(),
        "the button shows while a track plays launched clips"
    );
    // Clicking it returns every track to its arrangement clips: the launched clip stops at
    // the next boundary (a 32nd here) with its fade, so wait for it.
    harness.get_by_label("Back to arrangement").click();
    let mut back = true;
    for _ in 0..200 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
        if harness.query_by_label("Back to arrangement").is_none() {
            back = false;
            break;
        }
    }
    assert!(!back, "the arrangement is back");
}

#[test]
fn a_slot_drag_to_another_slot_moves_it_ctrl_drags_copy_and_an_occupied_one_swaps() {
    let slots = |h: &Harness<'static, App>| {
        h.state()
            .session()
            .map(|s| {
                let p = s.project();
                let of = |track: u32, scene: usize| {
                    p.launcher
                        .columns
                        .iter()
                        .find(|c| c.track == TrackId(track))
                        .and_then(|c| c.slots.get(scene))
                        .and_then(|s| s.clip.as_ref())
                        .map(|c| c.content)
                };
                (of(2, 1), of(2, 3), of(1, 0))
            })
            .unwrap_or((None, None, None))
    };
    let mut harness = window(demo_app());
    // Walk (Bass, scene 2) dragged to Bass' empty scene 4: it moves, the source empties.
    let from = harness.get_by_label("Launch Walk on Bass (scene 2)").rect().center();
    let to = harness.get_by_label("Stop Bass (scene 4)").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(to);
    harness.step();
    harness.drop_at(to);
    harness.run();
    let (first, target, _) = slots(&harness);
    assert!(first.is_none(), "the source slot emptied");
    assert!(target.is_some(), "the clip is in scene 4 now");
    // The grid rebuilt: the moved clip's slot shows it, the emptied one is a stop control.
    assert!(harness.query_by_label("Launch Walk on Bass (scene 4)").is_some(), "the grid shows the moved clip");
    assert!(harness.query_by_label("Stop Bass (scene 2)").is_some(), "the source's slot is empty again");
    assert!(
        matches!(harness.state().status(), Status::Done(text) if text.contains("Moved")),
        "{:?}",
        harness.state().status()
    );
    // One undo moves it back.
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    let (first, target, walk) = slots(&harness);
    assert!(first.is_some() && target.is_none(), "the clip is back");
    let _ = walk;
    // Ctrl+drag copies: both slots hold a clip (the copies' contents are their own). The
    // modifier is held over the drop: its state is set in the input directly (kittest's own
    // key presses reset the modifiers right after), and egui's state keeps it until changed.
    let from = harness.get_by_label("Launch Walk on Bass (scene 2)").rect().center();
    let to = harness.get_by_label("Stop Bass (scene 4)").rect().center();
    harness.hover_at(from);
    harness.step();
    harness.drag_at(from);
    harness.step();
    harness.hover_at(to);
    harness.step();
    harness.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::COMMAND));
    harness.step();
    harness.drop_at(to);
    // The drop's frame runs with the modifier still held; the reset comes after it.
    harness.run();
    harness.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::default()));
    harness.run();
    let (_, target, _) = slots(&harness);
    assert!(target.is_some(), "the copy landed");
    assert!(harness.query_by_label("Launch Walk on Bass (scene 2)").is_some(), "the source kept its clip");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    // A drag onto an occupied slot swaps the two: Walk to Beat's slot, Beat to Walk's.
    let (walk_content, _, beat_content) = slots(&harness);
    let walk = harness.get_by_label("Launch Walk on Bass (scene 2)").rect().center();
    let beat = harness.get_by_label("Launch Beat A on Drums (scene 1)").rect().center();
    harness.hover_at(walk);
    harness.step();
    harness.drag_at(walk);
    harness.step();
    harness.hover_at(beat);
    harness.step();
    harness.drop_at(beat);
    harness.run();
    let (bass, _, drums) = slots(&harness);
    assert!(
        matches!(harness.state().status(), Status::Done(text) if text.contains("Swapped")),
        "{:?}",
        harness.state().status()
    );
    assert_eq!((bass, drums), (beat_content, walk_content), "Beat A is in Walk's slot and Walk in Beat A's");
}

#[test]
fn a_slots_context_menu_renames_its_clip() {
    let mut harness = window(demo_app_with_name_field());
    // Walk's slot in Bass, scene 2: a right-click opens the menu with the clip's name field
    // (the menu stays open on a click inside: CloseOnClickOutside).
    harness.get_by_label("Launch Walk on Bass (scene 2)").click_secondary();
    harness.run();
    assert!(harness.query_by_label_contains("Rename clip Walk").is_some(), "the menu carries the clip's name field");
    harness.get_by_label_contains("Rename clip Walk").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.get_by_label_contains("Rename clip Walk").type_text("Walk the bass");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    // The slot's label carries the new name; one undo brings Walk back.
    assert!(harness.query_by_label("Launch Walk the bass on Bass (scene 2)").is_some(), "the slot shows the new name");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(harness.query_by_label("Launch Walk on Bass (scene 2)").is_some(), "the undo brings the name back");
}

#[test]
fn an_empty_rename_field_clears_the_clips_name() {
    let mut harness = window(demo_app_with_name_field());
    harness.get_by_label("Launch Walk on Bass (scene 2)").click_secondary();
    harness.run();
    harness.get_by_label_contains("Rename clip Walk").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    harness.key_press(Key::Backspace);
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.query_by_label("Launch clip on Bass (scene 2)").is_some(), "the empty draft clears the name");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(harness.query_by_label("Launch Walk on Bass (scene 2)").is_some(), "the undo brings the name back");
}

#[test]
fn a_rename_draft_dies_with_its_menu() {
    let mut harness = window(demo_app_with_name_field());
    harness.get_by_label("Launch Walk on Bass (scene 2)").click_secondary();
    harness.run();
    harness.get_by_label_contains("Rename clip Walk").click();
    harness.run();
    harness.get_by_label_contains("Rename clip Walk").type_text("X");
    harness.run();
    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.query_by_label_contains("Rename clip").is_none(), "the menu closed");
    // Reopening starts from the name as it is now, not the typing that was never applied.
    harness.get_by_label("Launch Walk on Bass (scene 2)").click_secondary();
    harness.run();
    let field = harness.get_by_label_contains("Rename clip Walk");
    assert_eq!(field.value().as_deref(), Some("Walk"), "the reopened field carries the name, not the stale draft");
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

#[test]
fn rename_in_a_slots_menu_edits_the_name_on_the_slot_and_ctrl_r_does_too() {
    let mut harness = window(demo_app());
    // By default the menu carries Rename, not a name field.
    harness.get_by_label("Launch Walk on Bass (scene 2)").click_secondary();
    harness.run();
    assert!(harness.query_by_label_contains("Rename clip Walk").is_none(), "no field in the menu by default");
    // The item's label carries its key too ("Rename" and Ctrl+R).
    harness.get_by_label_contains("Rename").click();
    harness.run();
    assert!(harness.query_by_label("Clip name").is_some(), "Rename puts a name field on the slot");
    assert!(harness.query_by_label("Cancel").is_none(), "no dialog: the field is all there is");
    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.query_by_label("Clip name").is_none(), "Escape leaves the name as it was");
    assert!(harness.query_by_label("Launch Walk on Bass (scene 2)").is_some());
    // Ctrl+R on the selected slot does the same; Enter renames.
    harness.get_by_label("Launch Walk on Bass (scene 2)").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::R);
    harness.run();
    assert!(harness.query_by_label("Clip name").is_some(), "Ctrl+R edits the selected clip's name");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Clip name").type_text("Riff");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.query_by_label("Launch Riff on Bass (scene 2)").is_some(), "the selected slot's clip is renamed");
}

#[test]
fn ctrl_r_on_an_arrangement_clip_edits_its_name_on_the_clip() {
    let mut harness = arrangement_app();
    let clip = harness.get_by_label("Clip Beat B on Drums at bar 17").rect();
    harness.get_by_label("Clip Beat B on Drums at bar 17").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::R);
    harness.run();
    let field = harness.get_by_label("Clip name").rect();
    assert!((field.top() - clip.top()).abs() < 4.0, "the field is on the clip: {field:?} on {clip:?}");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Clip name").type_text("Fill");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.query_by_label("Clip Fill on Drums at bar 17").is_some(), "renamed in place");
}

#[test]
fn dragging_one_of_several_selected_clips_moves_them_all() {
    let mut harness = arrangement_app();
    let starts = |h: &Harness<'static, App>| {
        h.state().session().map(|s| {
            s.project().arrangement.lanes.iter().flat_map(|l| l.clips.iter().map(|c| c.start)).collect::<Vec<_>>()
        })
    };
    let before = starts(&harness).unwrap();
    assert!(before.len() > 1);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    // Two bars later: 64 points at 8 points a beat.
    drag_clip(&mut harness, "Clip Beat B on Drums at bar 17", egui::vec2(64.0, 0.0));
    let after = starts(&harness).unwrap();
    let mut want: Vec<f64> = before.iter().map(|s| s + 8.0).collect();
    let mut got = after.clone();
    want.sort_by(f64::total_cmp);
    got.sort_by(f64::total_cmp);
    assert_eq!(got, want, "every selected clip moved by two bars");
}

#[test]
fn dragging_a_midi_clips_right_edge_makes_it_longer_on_the_grid() {
    let mut harness = arrangement_app();
    let beat_b = |h: &Harness<'static, App>| {
        let p = h.state().session().unwrap().project();
        let track = p.tracks.iter().find(|t| t.name == "Drums").map(|t| t.id);
        let lane = p.arrangement.lanes.iter().find(|l| l.track == track).unwrap();
        lane.clips.iter().find(|c| (c.start - 64.0).abs() < 1e-6).map(|c| c.length).unwrap()
    };
    let before = beat_b(&harness);
    let edge = harness.get_by_label("Right edge of Beat B on Drums").rect().center();
    // One bar: 32 points at 8 points a beat.
    let to = edge + egui::vec2(32.0, 0.0);
    harness.hover_at(edge);
    harness.run();
    harness.drag_at(edge);
    harness.run();
    harness.hover_at(edge + egui::vec2(16.0, 0.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    assert_eq!(beat_b(&harness), before + 4.0, "a bar longer");
}

/// The clip is drawn at its new length from the frame the button comes up on. In the window the
/// trim runs after that frame is drawn, and the clip showed as it was before the drag for that
/// one frame; the test harness draws the frame again once the trim ran, so it can only check that
/// the clip ends where it was let go.
#[test]
fn a_released_clip_edge_stays_where_it_was_let_go() {
    let mut harness = arrangement_app();
    let clip = harness.get_by_label("Clip Beat B on Drums at bar 17").rect();
    let edge = harness.get_by_label("Right edge of Beat B on Drums").rect().center();
    let to = edge + egui::vec2(32.0, 0.0);
    harness.hover_at(edge);
    harness.run();
    harness.drag_at(edge);
    harness.run();
    harness.hover_at(edge + egui::vec2(16.0, 0.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    let body_ends = |h: &Harness<'static, App>| -> Vec<f32> {
        let mut out = Vec::new();
        for clipped in &h.output().shapes {
            let mut stack = vec![clipped.shape.clone()];
            while let Some(shape) = stack.pop() {
                match shape {
                    egui::Shape::Vec(inner) => stack.extend(inner),
                    egui::Shape::Rect(r)
                        if r.fill.a() > 0
                            && (r.rect.left() - clip.left()).abs() < 1.5
                            && (r.rect.top() - clip.top()).abs() < 1.5 =>
                    {
                        out.push(r.rect.right());
                    }
                    _ => {}
                }
            }
        }
        out
    };
    let dragged = body_ends(&harness).into_iter().fold(f32::MIN, f32::max);
    harness.drop_at(to);
    harness.step();
    let released = body_ends(&harness).into_iter().fold(f32::MIN, f32::max);
    assert!((released - dragged).abs() < 1.5, "drawn at {released} the frame it was let go, {dragged} while dragged");
    assert!(released > clip.right() + 16.0, "not back at its old end {}", clip.right());
}

#[test]
fn ctrl_l_loops_the_time_selection_even_with_clips_in_it_selected() {
    let mut harness = arrangement_app();
    let clip = harness.get_by_label("Clip Beat B on Drums at bar 17").rect();
    // A selection rectangle from empty space before the clip into it picks the clip too.
    let ruler = harness.get_by_label("Ruler").rect();
    let (from, to) =
        (egui::pos2(clip.left() - 64.0, ruler.center().y), egui::pos2(clip.left() + 32.0, ruler.center().y));
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    let span = harness.state().arrangement().time_selection().map(|s| (s.start, s.end)).unwrap();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::L);
    harness.run();
    let transport = harness.state().session().unwrap().project().transport.clone();
    assert!(transport.loop_enabled);
    assert_eq!(transport.loop_range.map(|r| (r.start, r.end)), Some(span), "the loop is the selection");
}

#[test]
fn touching_a_short_clips_edge_with_a_coarse_grid_changes_nothing() {
    let mut harness = arrangement_app();
    let length = |h: &Harness<'static, App>| {
        let p = h.state().session().unwrap().project();
        let track = p.tracks.iter().find(|t| t.name == "Drums").map(|t| t.id);
        let lane = p.arrangement.lanes.iter().find(|l| l.track == track).unwrap();
        lane.clips.iter().find(|c| (c.start - 64.0).abs() < 1e-6).map(|c| c.length).unwrap()
    };
    let before = length(&harness);
    let edge = harness.get_by_label("Right edge of Beat B on Drums").rect().center();
    // A press and a let-go two points along: the edge stays where it was.
    harness.hover_at(edge);
    harness.run();
    harness.drag_at(edge);
    harness.run();
    harness.hover_at(edge + egui::vec2(2.0, 0.0));
    harness.run();
    harness.drop_at(edge + egui::vec2(2.0, 0.0));
    harness.run();
    assert_eq!(length(&harness), before);
}

#[test]
fn ctrl_dragging_a_clip_copies_it_and_leaves_the_original() {
    let mut harness = arrangement_app();
    let starts = |h: &Harness<'static, App>| {
        let p = h.state().session().unwrap().project();
        let track = p.tracks.iter().find(|t| t.name == "Drums").map(|t| t.id);
        p.arrangement.lanes.iter().find(|l| l.track == track).unwrap().clips.iter().map(|c| c.start).collect::<Vec<_>>()
    };
    let before = starts(&harness);
    let from = harness.get_by_label("Clip Beat B on Drums at bar 17").rect().center_top() + egui::vec2(0.0, 6.0);
    let to = from + egui::vec2(256.0, 0.0);
    harness.hover_at(from);
    harness.run();
    // Ctrl (Cmd) held through the drag.
    harness.event(egui::Event::ModifiersChanged(Modifiers::COMMAND));
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + egui::vec2(128.0, 0.0));
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    harness.event(egui::Event::ModifiersChanged(Modifiers::default()));
    harness.run();
    let after = starts(&harness);
    // (The copy replaces what it lands on, as a moved clip does.)
    assert!(after.contains(&64.0), "the original stays at bar 17: {before:?} -> {after:?}");
    assert!(after.contains(&96.0), "the copy is 8 bars later");
}

#[test]
fn a_drag_on_the_ruler_selects_time_and_delete_removes_only_what_is_inside() {
    let mut harness = arrangement_app();
    let drums = |h: &Harness<'static, App>| {
        h.state().session().map(|s| {
            let p = s.project();
            let track = p.tracks.iter().find(|t| t.name == "Drums").map(|t| t.id);
            p.arrangement
                .lanes
                .iter()
                .find(|l| l.track == track)
                .map(|l| l.clips.iter().map(|c| (c.start, c.length)).collect::<Vec<_>>())
        })
    };
    let before = drums(&harness).flatten().unwrap();
    let clip = harness.get_by_label("Clip Beat B on Drums at bar 17").rect();
    let ruler = harness.get_by_label("Ruler").rect();
    // One bar into the clip to one bar before its end, on the bar grid (32 points a bar).
    let (from, to) =
        (egui::pos2(clip.left() + 33.0, ruler.center().y), egui::pos2(clip.right() - 33.0, ruler.center().y));
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(to);
    harness.run();
    harness.drop_at(to);
    harness.run();
    assert!(harness.state().arrangement().time_selection().is_some(), "the ruler's drag selected time");
    harness.key_press(Key::Delete);
    harness.run();
    let after = drums(&harness).flatten().unwrap();
    assert_eq!(after.len(), before.len() + 1, "the clip split around the gap: {before:?} -> {after:?}");
    let total = |c: &[(f64, f64)]| c.iter().map(|(_, l)| l).sum::<f64>();
    assert!(total(&after) < total(&before), "the inside went");
}

#[test]
fn ctrl_r_after_a_header_click_renames_the_track_in_its_header() {
    let mut harness = arrangement_app();
    harness.get_by_label("Track Bass").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::R);
    harness.run();
    assert!(harness.query_by_label("Track name").is_some(), "Ctrl+R edits the track's name");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Track name").type_text("Sub");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    let names: Vec<String> =
        harness.state().session().unwrap().project().tracks.iter().map(|t| t.name.clone()).collect();
    assert!(names.contains(&"Sub".to_string()) && !names.contains(&"Bass".to_string()), "{names:?}");
}

#[test]
fn ctrl_r_with_several_clips_selected_renames_them_all() {
    let mut harness = arrangement_app();
    harness.get_by_label("Clip Beat B on Drums at bar 17").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.run();
    let count = harness.state().arrangement().selected_clips().len();
    assert!(count > 1, "Ctrl+A selects several");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::R);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("Clip name").type_text("Same");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    let project = harness.state().session().unwrap().project().clone();
    let named = project
        .arrangement
        .lanes
        .iter()
        .flat_map(|l| &l.clips)
        .filter(|c| project.contents.iter().any(|x| x.id == c.content && x.name == "Same"))
        .count();
    assert_eq!(named, count);
}

#[test]
fn the_export_dialog_writes_the_wav_the_demo_names() {
    let mut harness = window(demo_app());
    let dir = std::env::temp_dir().join(format!("suhina-export-dialog-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("Demo mix.wav");

    // File › Export… opens the dialog over the window.
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Export…").click();
    harness.run();
    assert!(harness.query_by_label("File to export to").is_some(), "the export dialog is open");

    // The file to write, typed over what the dialog filled in.
    harness.get_by_label("File to export to").click();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("File to export to").type_text(path.display().to_string().as_str());
    harness.run();

    // Export runs on its worker; the window keeps living while it renders (the wait runs
    // frames beside the real thread), and the result lands in the status.
    harness.get_by_label("Export the mix to the file").click();
    waits_for_export(&mut harness);
    assert!(path.exists(), "the export wrote {}", path.display());
    assert!(
        matches!(harness.state().status(), Status::Done(text) if text.contains("Exported")),
        "{:?}",
        harness.state().status()
    );
    assert!(harness.query_by_label("File to export to").is_none(), "the finished export closed the dialog");
    // A wav: the RIFF and WAVE marks of the file's first bytes.
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");

    // An export over it is refused until overwrite is on.
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Export…").click();
    harness.run();
    harness.get_by_label("File to export to").click();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("File to export to").type_text(path.display().to_string().as_str());
    harness.get_by_label("Export the mix to the file").click();
    waits_for_export(&mut harness);
    assert!(
        matches!(harness.state().status(), Status::Error(text) if text.contains("already exists")),
        "{:?}",
        harness.state().status()
    );
    // A failed export keeps the dialog open with its settings; Overwrite makes it go through.
    assert!(harness.query_by_label("File to export to").is_some(), "the dialog stays open after a failure");
    harness.get_by_label("Overwrite").click();
    harness.run();
    harness.get_by_label("Export the mix to the file").click();
    waits_for_export(&mut harness);
    assert!(
        matches!(harness.state().status(), Status::Done(text) if text.contains("Exported")),
        "{:?}",
        harness.state().status()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_named_mp3_shows_the_mp3_rate_and_exports_an_mp3() {
    let mut harness = window(demo_app());
    let dir = std::env::temp_dir().join(format!("suhina-export-mp3-dialog-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("Demo mix.mp3");

    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Export…").click();
    harness.run();
    // A wav name shows no mp3 rate.
    assert!(harness.query_by_label_contains("MP3 rate").is_none());

    // From the file name: an .mp3 name is an mp3, its rate row shows, and 48 000 stays.
    harness.get_by_label("File to export to").click();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("File to export to").type_text(path.display().to_string().as_str());
    harness.run();
    assert!(harness.query_by_label("Sample rate 48000 Hz").is_some(), "48 000 is kept for mp3");
    harness.get_by_label("MP3 rate 320 kbps").click();
    harness.run();
    harness.get_by_label("VBR V2").click();
    harness.run();
    assert!(harness.query_by_label("MP3 rate VBR V2").is_some(), "the rate row took the choice");

    harness.get_by_label("Export the mix to the file").click();
    waits_for_export(&mut harness);
    assert!(
        matches!(harness.state().status(), Status::Done(text) if text.contains("Exported")),
        "{:?}",
        harness.state().status()
    );
    // An mp3: the demo was never saved, so no ID3 header; the stream starts at a frame's sync.
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes[0], 0xFF);
    assert_eq!(bytes[1] & 0xE0, 0xE0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Runs frames until the export's result is in the status (the export renders on a real
/// worker thread); the deadline only guards against a hang, not how long a busy runner takes.
fn waits_for_export(harness: &mut Harness<'static, App>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while std::time::Instant::now() < deadline {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let exporting = matches!(harness.state().status(), Status::Hint(text) if text.starts_with("Exporting"));
        if !exporting {
            return;
        }
    }
    assert!(
        !matches!(harness.state().status(), Status::Hint(text) if text.starts_with("Exporting")),
        "the export did not finish: {:?}",
        harness.state().status()
    );
}

/// Runs frames until the status says Exporting…, at most two seconds.
fn waits_while_exporting(harness: &mut Harness<'static, App>) {
    for _ in 0..200 {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
        if matches!(harness.state().status(), Status::Hint(text) if text.starts_with("Exporting")) {
            return;
        }
    }
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.starts_with("Exporting")),
        "the export did not start: {:?}",
        harness.state().status()
    );
}

/// The frames a WAV file holds, walked from its `data` chunk, and the depth its `fmt ` chunk
/// names: the export's length is checked against the arrangement's end.
#[allow(clippy::unwrap_used, clippy::panic)] // a test helper outside #[test] functions
fn wav_frames(path: &std::path::Path) -> u64 {
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[..4], b"RIFF", "a wav");
    assert_eq!(&bytes[8..12], b"WAVE", "a wav");
    let (mut at, mut width) = (12_usize, 0_u64);
    while at + 8 <= bytes.len() {
        let id: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if id == *b"fmt " {
            let bits = u16::from_le_bytes(bytes[at + 8 + 14..at + 8 + 16].try_into().unwrap());
            let channels = u16::from_le_bytes(bytes[at + 8 + 2..at + 8 + 4].try_into().unwrap());
            width = u64::from(bits) * u64::from(channels) / 8;
        }
        if id == *b"data" {
            assert!(width > 0, "the fmt chunk came before the data chunk");
            return u64::from(size as u32) / width;
        }
        at += 8 + size + (size & 1);
    }
    panic!("no data chunk in {}", path.display());
}

/// Closing an export's busy dialog mid-run leaves its worker running; File › Export… again
/// shows the running job (Exporting…, no second Export to click), and the first export's
/// result is the one that lands.
#[test]
fn a_closed_export_dialogs_worker_still_runs_and_the_reopened_dialog_shows_it() {
    let mut harness = window(long_demo_app());
    let dir = std::env::temp_dir().join(format!("suhina-export-running-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("Demo mix.wav");

    // File › Export…, the file typed, Export clicked: the dialog turns into Exporting….
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Export…").click();
    harness.run();
    harness.get_by_label("File to export to").click();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("File to export to").type_text(path.display().to_string().as_str());
    harness.run();
    harness.get_by_label("Export the mix to the file").click();
    waits_while_exporting(&mut harness);

    // The dialog closed while the export renders: the worker goes on, the status holds. The
    // frames here are bounded steps: the window repaints every frame while the export runs.
    harness.get_by_label("Close Export audio").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("File to export to").is_none(), "the dialog is closed");
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.starts_with("Exporting")),
        "the export renders on: {:?}",
        harness.state().status()
    );

    // Reopening shows the running job again: Exporting…, and no Export button to start another.
    harness.get_by_label("File").click();
    harness.run_steps(2);
    harness.get_by_label("Export…").click();
    harness.run_steps(2);
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.starts_with("Exporting")),
        "the reopened dialog shows the running export: {:?}",
        harness.state().status()
    );
    assert!(
        harness.query_by_label("Export the mix to the file").is_none(),
        "a second export is not offered while one runs"
    );

    // The export the user closed the window of is the one whose result lands.
    waits_for_export(&mut harness);
    assert!(path.exists(), "the export wrote {}", path.display());
    assert!(
        matches!(harness.state().status(), Status::Done(text) if text.contains("Exported")),
        "{:?}",
        harness.state().status()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Switching to another project while the export renders changes nothing about it: the
/// operation is the first project's, its file the first project's sound, and the feedback
/// names it whatever tab is showing (the spec's tabbed workflow, two projects open).
#[test]
fn switching_projects_while_the_export_renders_keeps_the_export_and_names_it() {
    // The second project is the long demo (the export takes seconds, so the switch lands
    // inside the render); the first is switched to while it runs.
    let mut app = App::new();
    app.set_browser_open(false);
    app.open_project(Session::new(demo_project()), &[], OpenAs::NewTab);
    app.open_project(Session::new(long_demo_project()), &[], OpenAs::NewTab);
    let mut harness = window(app);
    let ids = harness.state().projects().ids();
    assert_eq!(harness.state().projects().active(), ids.get(1).copied());
    let first = ids.first().copied();
    // The export belongs to the project active when Export was clicked (the second tab); its
    // name is what the feedback carries.
    let export_name = harness.state().projects().name(ids.get(1).copied().unwrap_or(ids[0])).unwrap_or("").to_string();
    let dir = std::env::temp_dir().join(format!("suhina-export-switch-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("Long mix.wav");

    // File › Export…, the file typed, Export clicked: Exporting…, and the name is the long
    // project's.
    harness.get_by_label("File").click();
    harness.run();
    harness.get_by_label("Export…").click();
    harness.run();
    harness.get_by_label("File to export to").click();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::A);
    harness.get_by_label("File to export to").type_text(path.display().to_string().as_str());
    harness.run();
    harness.get_by_label("Export the mix to the file").click();
    waits_while_exporting(&mut harness);
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.contains(&format!("Exporting {export_name}"))),
        "the export's project is named while it runs: {:?}",
        harness.state().status()
    );

    // The dialog closes; the worker goes on. Then the other tab shows: the export goes on,
    // still the second project's. The frames are steps: the window repaints every frame
    // while the export runs.
    harness.get_by_label("Close Export audio").click();
    harness.run_steps(2);
    assert!(harness.query_by_label("File to export to").is_none(), "the dialog closed; the export renders on");
    harness.get_by_label("Tab Untitled 1").click();
    harness.run_steps(2);
    assert_eq!(harness.state().projects().active(), first, "the other project shows");
    assert!(
        matches!(harness.state().status(), Status::Hint(text) if text.contains(&format!("Exporting {export_name}"))),
        "the status still names the export's project: {:?}",
        harness.state().status()
    );

    waits_for_export(&mut harness);
    assert!(path.exists(), "the export wrote {}", path.display());
    assert!(
        matches!(harness.state().status(), Status::Done(text) if text.contains(&format!("Exported {export_name} to"))),
        "the finished export names the project it belongs to: {:?}",
        harness.state().status()
    );
    // And the file is the second project's render, not the tab now showing: its frames are
    // what the engine renders of the same project (the same length the whole-mix export
    // writes, the demo project's own render being far shorter).
    let project = long_demo_project();
    let expected = suhina_engine::Renderer::stopped(&project, &suhina_engine::AudioPool::new(), 48_000)
        .expect("the long project renders")
        .length();
    let frames = wav_frames(&path);
    assert_eq!(frames, expected, "the exported file is the second project's render");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A slot's right-click menu deletes its clip, MIDI or audio, as the Delete key does, in one
/// undo step; an empty slot's Delete is greyed out (#842).
#[test]
fn a_slots_context_menu_deletes_its_clip() {
    // A MIDI clip: Walk, in Bass's second slot.
    let mut harness = window(demo_app());
    harness.get_by_label("Launch Walk on Bass (scene 2)").click_secondary();
    harness.run();
    harness.get_by_label_contains("Delete").click();
    harness.run();
    assert!(harness.query_by_label("Launch Walk on Bass (scene 2)").is_none(), "the clip is gone");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(harness.query_by_label("Launch Walk on Bass (scene 2)").is_some(), "one undo brings it back");

    // An empty slot: nothing to delete.
    harness.get_by_label("Stop Keys (scene 1)").click_secondary();
    harness.run();
    assert!(egui_kittest::kittest::NodeT::accesskit_node(&harness.get_by_label_contains("Delete")).is_disabled());
    harness.key_press(Key::Escape);
    harness.run();

    // An audio clip: the Loop track's, put in its first slot.
    let (mut project, waveforms) = project_with_audio();
    let clip = project.arrangement.lanes.last().and_then(|l| l.clips.first()).cloned().unwrap();
    project.launcher.columns.push(suhina_engine::model::LauncherColumn {
        track: suhina_engine::model::TrackId(30),
        slots: vec![suhina_engine::model::Slot { clip: Some(clip), has_stop: true }],
    });
    let mut app = App::new();
    app.set_session(Session::new(project), &[]);
    app.set_waveforms(waveforms);
    let mut harness = window(app);
    harness.get_by_label("Launch Loop on Loop (scene 1)").click_secondary();
    harness.run();
    harness.get_by_label_contains("Delete").click();
    harness.run();
    assert!(harness.query_by_label("Launch Loop on Loop (scene 1)").is_none(), "the audio clip is gone");
}
