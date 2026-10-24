// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The record controls (`docs/spec/recording.md` › In the window): the control bar's record
//! button and count-in menu, and the record button of an armed track's empty slot, clicked as a
//! user would, with an engine rendered offline.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::{AudioPool, Renderer};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::{App, kit::theme};

fn app_with_engine(renderer: &mut Renderer) -> Harness<'static, App> {
    let mut session = Session::new(demo_project());
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
    harness.run();
    harness
}

fn render_a_block(renderer: &mut Renderer) {
    renderer.process(&mut [0.0f32; 2 * 64]);
}

#[test]
fn an_armed_tracks_empty_slot_records_and_a_second_click_ends_the_take() {
    let mut renderer = Renderer::stopped(&demo_project(), &AudioPool::new(), 48_000).unwrap();
    let mut harness = app_with_engine(&mut renderer);
    let keys = suhina_engine::model::TrackId(3);
    assert!(harness.query_by_label("Record on Keys (scene 1)").is_none(), "only on armed tracks");
    harness.get_by_label("Arm Keys for recording").click();
    harness.run_steps(3);
    // A slot with a clip has no record button.
    assert!(harness.query_by_label("Record on Keys (scene 2)").is_none());
    harness.get_by_label("Record on Keys (scene 1)").click();
    harness.run_steps(3);
    assert_eq!(harness.state().session().and_then(|s| s.recording_slot(keys)), Some(0));
    render_a_block(&mut renderer);
    harness.run_steps(3);
    harness.get_by_label("End the recording on Keys (scene 1)").click();
    harness.run_steps(3);
    assert_eq!(harness.state().session().and_then(|s| s.recording_slot(keys)), None, "ending");
}

#[test]
fn the_control_bar_records_into_the_arrangement_and_sets_the_count_in() {
    let mut renderer = Renderer::stopped(&demo_project(), &AudioPool::new(), 48_000).unwrap();
    let mut harness = app_with_engine(&mut renderer);
    let records = |h: &Harness<'static, App>| h.state().session().is_some_and(Session::records_arrangement);
    // Nothing armed: the click says why.
    harness.get_by_label("Record into the arrangement").click();
    harness.run_steps(3);
    assert!(!records(&harness));
    match harness.state().status() {
        suhina_ui::Status::Error(message) => assert!(message.contains("arm"), "{message}"),
        other => panic!("expected an error, got {other:?}"),
    }
    harness.get_by_label("Arm Keys for recording").click();
    harness.run_steps(3);
    harness.get_by_label("Record into the arrangement").click();
    harness.run_steps(3);
    assert!(records(&harness));
    render_a_block(&mut renderer);
    harness.get_by_label("Stop recording").click();
    harness.run_steps(3);
    assert!(!records(&harness));

    harness.get_by_label("Count-in").click();
    harness.run_steps(3);
    harness.get_by_label("2 bars").click();
    harness.run_steps(3);
    assert_eq!(harness.state().session().map(Session::count_in_bars), Some(2));
}
