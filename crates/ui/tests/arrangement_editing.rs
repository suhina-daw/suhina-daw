// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Splitting and consolidating clips in the Arrangement view (`docs/spec/arrangement-editing.md`
//! › Splitting, Consolidating): the menu entries and the keys become `clip.split` and
//! `clip.consolidate`.

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::Clip;
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::{App, kit::theme};

fn arrangement(session: Session) -> Harness<'static, App> {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_step_dt(0.25)
        .with_max_steps(4)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness
}

fn drums(harness: &Harness<'static, App>) -> Vec<Clip> {
    harness.state().session().map(|s| s.project().arrangement.lanes[0].clips.clone()).unwrap_or_default()
}

#[test]
fn split_here_cuts_a_clip_where_it_was_right_clicked() {
    let mut harness = arrangement(Session::new(demo_project()));
    let before = drums(&harness);
    // Fill is four bars from beat 112; a right-click in its middle is beat 120, on the bar grid.
    harness.get_by_label("Clip Fill on Drums at bar 29").click_secondary();
    harness.run();
    harness.get_by_label("Split here").click();
    harness.run();
    let after = drums(&harness);
    assert_eq!(after.len(), before.len() + 1);
    let parts: Vec<(f64, f64)> =
        after.iter().filter(|c| c.start >= 112.0 && c.start < 128.0).map(|c| (c.start, c.length)).collect();
    assert_eq!(parts, [(112.0, 8.0), (120.0, 8.0)]);
}

#[test]
fn ctrl_e_splits_the_selected_clip_at_the_playhead() {
    let project = demo_project();
    let mut renderer = suhina_engine::Renderer::stopped(&project, &suhina_engine::AudioPool::new(), 48_000)
        .unwrap_or_else(|e| {
            panic!("{e}");
        });
    let mut session = Session::new(project);
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut harness = arrangement(session);
    harness.get_by_label("Clip Fill on Drums at bar 29").click();
    harness.run();
    // The playhead is at the start, outside Fill: nothing is split.
    let before = drums(&harness);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::E);
    harness.run();
    assert_eq!(drums(&harness), before);

    // A click on the ruler moves the playhead to bar 30 (beat 116); the next block takes it.
    let ruler = harness.get_by_label("Ruler").rect();
    let fill = harness.get_by_label("Clip Fill on Drums at bar 29").rect();
    let at = egui::pos2(fill.left() + 4.0 * 8.0 + 1.0, ruler.center().y);
    harness.hover_at(at);
    harness.run();
    harness.drag_at(at);
    harness.drop_at(at);
    harness.run();
    renderer.process(&mut [0.0f32; 2 * 64]);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::E);
    harness.run();
    let parts: Vec<(f64, f64)> =
        drums(&harness).iter().filter(|c| c.start >= 112.0 && c.start < 128.0).map(|c| (c.start, c.length)).collect();
    assert_eq!(parts, [(112.0, 4.0), (116.0, 12.0)]);
}

#[test]
fn ctrl_j_consolidates_the_selected_clip_and_undo_brings_it_back() {
    let mut harness = arrangement(Session::new(demo_project()));
    let before = drums(&harness);
    harness.get_by_label("Clip Fill on Drums at bar 29").click();
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::J);
    harness.run();
    let after = drums(&harness);
    assert_eq!(after.len(), before.len());
    let fill = |clips: &[Clip]| clips.iter().find(|c| c.start == 112.0).map(|c| (c.content, c.length));
    let (old, new) = (fill(&before), fill(&after));
    assert_eq!(new.map(|(_, length)| length), Some(16.0));
    assert_ne!(old.map(|(content, _)| content), new.map(|(content, _)| content), "a new content");

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(drums(&harness), before);
}

#[test]
fn a_clips_label_counts_bars_through_the_time_signature_changes() {
    // 3/4 from bar 9 (beat 32): Fill at beat 112 starts in bar 35, not bar 29 as 4/4 throughout.
    let mut session = Session::new(demo_project());
    session.execute("timeSignature.add", &serde_json::json!({ "bar": 9, "numerator": 3, "denominator": 4 })).unwrap();
    let harness = arrangement(session);
    assert!(harness.query_by_label("Clip Fill on Drums at bar 35").is_some());
    assert!(harness.query_by_label("Clip Fill on Drums at bar 29").is_none());
}
