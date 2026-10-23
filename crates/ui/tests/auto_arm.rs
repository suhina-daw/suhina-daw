// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Auto-arm (Settings › MIDI, #834): selecting a track arms it, so a MIDI controller plays the
//! track the user looks at. Tracks are selected by clicking their headers, as a user would, on
//! the demo project with an engine rendered offline.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use suhina_engine::model::{ContentKinds, Project, TrackId};
use suhina_engine::{AudioPool, Renderer};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::kit::widgets::Action;
use suhina_ui::settings::Settings;
use suhina_ui::{App, kit::theme};

const DRUMS: TrackId = TrackId(1);
const BASS: TrackId = TrackId(2);
const KEYS: TrackId = TrackId(3);
const VOX: TrackId = TrackId(4);

fn app(renderer: &mut Renderer, auto_arm: bool) -> Harness<'static, App> {
    app_on(demo_project(), renderer, auto_arm)
}

fn app_on(project: Project, renderer: &mut Renderer, auto_arm: bool) -> Harness<'static, App> {
    let mut session = Session::new(project);
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = Harness::builder()
        .with_size((1280.0, 800.0))
        .with_step_dt(0.25)
        .with_max_steps(4)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, Settings { auto_arm, ..Default::default() });
    harness.run();
    harness
}

fn engine() -> Renderer {
    Renderer::stopped(&demo_project(), &AudioPool::new(), 48_000).unwrap()
}

fn armed(harness: &Harness<'static, App>) -> Vec<TrackId> {
    let mut armed = harness.state().session().map(Session::armed).unwrap_or_default();
    armed.sort();
    armed
}

fn select(harness: &mut Harness<'static, App>, name: &str) {
    harness.get_by_label(&format!("Track {name}")).click();
    harness.run_steps(3);
}

#[test]
fn off_by_default_selecting_a_track_arms_nothing() {
    assert!(!Settings::default().auto_arm);
    let mut renderer = engine();
    let mut harness = app(&mut renderer, false);
    select(&mut harness, "Keys");
    select(&mut harness, "Bass");
    assert_eq!(armed(&harness), []);
}

#[test]
fn selecting_a_track_arms_it_and_disarms_the_one_armed_before() {
    let mut renderer = engine();
    let mut harness = app(&mut renderer, true);
    select(&mut harness, "Keys");
    assert_eq!(armed(&harness), [KEYS]);
    select(&mut harness, "Bass");
    assert_eq!(armed(&harness), [BASS], "the track armed by the last selection is disarmed");
}

#[test]
fn a_track_armed_by_hand_stays_armed() {
    let mut renderer = engine();
    let mut harness = app(&mut renderer, true);
    harness.state_mut().run(&Action::new("track.setArm", json!({ "track": DRUMS.0, "armed": true })));
    harness.run_steps(3);
    select(&mut harness, "Keys");
    assert_eq!(armed(&harness), [DRUMS, KEYS]);
    select(&mut harness, "Bass");
    assert_eq!(armed(&harness), [DRUMS, BASS], "only the auto-armed track moves");
    // Selecting the hand-armed track does not make it auto-arm's: leaving it keeps it armed.
    select(&mut harness, "Drums");
    assert_eq!(armed(&harness), [DRUMS], "Bass, auto-armed, is let go; Drums was armed already");
    select(&mut harness, "Keys");
    assert_eq!(armed(&harness), [DRUMS, KEYS]);
}

#[test]
fn a_return_track_is_not_armed_and_leaves_the_arm_where_it_is() {
    let mut renderer = engine();
    let mut harness = app(&mut renderer, true);
    select(&mut harness, "Keys");
    select(&mut harness, "A Reverb");
    assert_eq!(armed(&harness), [KEYS]);
}

#[test]
fn turning_it_on_arms_the_selected_track_and_off_leaves_the_arm() {
    let mut renderer = engine();
    let mut harness = app(&mut renderer, false);
    select(&mut harness, "Keys");
    assert_eq!(armed(&harness), []);
    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, Settings { auto_arm: true, ..Default::default() });
    harness.run_steps(3);
    assert_eq!(armed(&harness), [KEYS]);
    harness.state_mut().set_settings(&ctx, Settings { auto_arm: false, ..Default::default() });
    harness.run_steps(3);
    select(&mut harness, "Bass");
    assert_eq!(armed(&harness), [KEYS], "off: the arm stays where it was");
}

#[test]
fn a_track_disarmed_and_armed_by_hand_while_it_is_off_is_the_users() {
    let mut renderer = engine();
    let mut harness = app(&mut renderer, true);
    select(&mut harness, "Keys");
    assert_eq!(armed(&harness), [KEYS], "auto-arm's");
    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, Settings { auto_arm: false, ..Default::default() });
    harness.run_steps(3);
    // Off, Keys is disarmed and armed again by hand: it is the user's now.
    harness.state_mut().run(&Action::new("track.setArm", json!({ "track": KEYS.0, "armed": false })));
    harness.run_steps(3);
    harness.state_mut().run(&Action::new("track.setArm", json!({ "track": KEYS.0, "armed": true })));
    harness.run_steps(3);
    harness.state_mut().set_settings(&ctx, Settings { auto_arm: true, ..Default::default() });
    harness.run_steps(3);
    select(&mut harness, "Bass");
    assert_eq!(armed(&harness), [BASS, KEYS], "Keys, armed by hand, stays armed");
}

#[test]
fn the_arm_does_not_move_while_a_take_records() {
    let mut renderer = engine();
    let mut harness = app(&mut renderer, true);
    select(&mut harness, "Keys");
    harness.get_by_label("Record on Keys (scene 1)").click();
    harness.run_steps(3);
    assert_eq!(harness.state().session().and_then(|s| s.recording_slot(KEYS)), Some(0));
    renderer.process(&mut [0.0f32; 2 * 64]);
    select(&mut harness, "Bass");
    assert_eq!(armed(&harness), [KEYS], "the take is not cut off");
    harness.get_by_label("End the recording on Keys (scene 1)").click();
    harness.run_steps(3);
    // Once it ends (its notes on their way waited for, a real moment) and is a clip, the track
    // selected meanwhile is armed. Until then it still records: the session's tests hold that.
    std::thread::sleep(std::time::Duration::from_millis(400));
    harness.run_steps(3);
    assert_eq!(armed(&harness), [BASS]);
}

#[test]
fn selecting_a_track_in_the_arrangement_arms_it_too() {
    let mut renderer = engine();
    let mut harness = app(&mut renderer, true);
    harness.get_by_label("Arrangement view").click();
    harness.run_steps(3);
    select(&mut harness, "Keys");
    assert_eq!(armed(&harness), [KEYS]);
    select(&mut harness, "Drums");
    assert_eq!(armed(&harness), [DRUMS]);
}

#[test]
fn selecting_a_clip_slot_empty_or_not_arms_its_track() {
    let mut renderer = engine();
    let mut harness = app(&mut renderer, true);
    // Keys' first slot is empty (in the demo it holds a stop button), Bass's holds a clip.
    harness.get_by_label("Stop Keys (scene 1)").click();
    harness.run_steps(3);
    assert_eq!(armed(&harness), [KEYS], "an empty slot");
    harness.get_by_label("Select Root on Bass (scene 1)").click();
    harness.run_steps(3);
    assert_eq!(armed(&harness), [BASS], "a slot with a clip");
    harness.get_by_label("Stop Keys (scene 1)").click_secondary();
    harness.run_steps(3);
    assert_eq!(armed(&harness), [KEYS], "a right-click on a slot");
}

#[test]
fn an_audio_track_is_armed_too() {
    // The demo's Vox, made an audio track: it plays audio, not notes.
    let mut project = demo_project();
    let vox = project.track_mut(VOX).unwrap();
    vox.content = ContentKinds { notes: false, audio: true };
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), 48_000).unwrap();
    let mut harness = app_on(project, &mut renderer, true);
    select(&mut harness, "Keys");
    select(&mut harness, "Vox");
    assert_eq!(armed(&harness), [VOX]);
    select(&mut harness, "Keys");
    assert_eq!(armed(&harness), [KEYS]);
}
