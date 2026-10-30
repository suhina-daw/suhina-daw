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

/// `harness`'s track headers on the right of the timeline, where these tests drag their edge.
fn headers_right(harness: &mut Harness<'static, App>) {
    let mut settings = harness.state().settings().clone();
    settings.track_headers = Some(suhina_ui::settings::Side::Right);
    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, settings);
    harness.run();
}

fn arrangement(session: Session) -> Harness<'static, App> {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1600.0, 800.0))
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
    let at = egui::pos2(fill.left() + 4.0 * harness.state().arrangement().beat_width, ruler.center().y);
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
fn track_type_is_on_the_name_row_and_does_not_change_the_project() {
    let mut project = demo_project();
    project.tracks[0].content.notes = true;
    project.tracks[0].content.audio = false;
    project.tracks[1].content.notes = false;
    project.tracks[1].content.audio = true;
    project.tracks[2].content.notes = true;
    project.tracks[2].content.audio = true;
    let mut harness = arrangement(Session::new(project));
    for (name, kind) in [("Drums", "MIDI track"), ("Bass", "Audio track"), ("Keys", "MIDI and audio track")] {
        let marker = harness.get_by_label(&format!("{name}: {kind}")).rect();
        let mute = harness.get_by_label(&format!("Turn {name} off")).rect();
        assert!(marker.bottom() <= mute.top());
    }
    let before = harness.state().session().map(|s| s.project().clone());
    let marker = harness.get_by_label("Drums: MIDI track").rect();
    harness.hover_at(marker.center());
    harness.run();
    assert_eq!(harness.state().session().map(|s| s.project().clone()), before);
}

#[test]
fn narrow_header_volume_starts_below_automation() {
    let mut harness = arrangement(Session::new(demo_project()));
    headers_right(&mut harness);
    let ruler = harness.get_by_label("Ruler").rect();
    let edge = egui::pos2(ruler.right(), ruler.bottom() + 30.0);
    harness.hover_at(edge);
    harness.run();
    harness.drag_at(edge);
    harness.hover_at(edge + egui::vec2(66.0, 0.0));
    harness.run();
    harness.drop_at(edge + egui::vec2(66.0, 0.0));
    harness.run();
    assert!(harness.state().arrangement().header_width < 300.0);
    let automation = harness.get_by_label("Show automation of Drums").rect();
    let volume = harness.get_by_label("Drums volume").rect();
    let mute = harness.get_by_label("Turn Drums off").rect();
    assert!(volume.top() >= automation.bottom());
    assert!((volume.left() - automation.left()).abs() < 1.0);
    assert!(volume.left() < mute.left());
}

#[test]
fn minimum_width_headers_keep_names_controls_and_meters_separate() {
    for depth in [0, 1, 8] {
        let mut session = Session::new(demo_project());
        for index in 0..depth {
            session
                .execute("track.group", &serde_json::json!({"tracks": [1], "name": format!("Group {index}")}))
                .unwrap();
        }
        let mut harness = arrangement(session);
        headers_right(&mut harness);
        // Keep all eight group rows and the child visible above the detail area.
        harness.set_size(egui::vec2(1600.0, 1200.0));
        harness.run();
        let ruler = harness.get_by_label("Ruler").rect();
        let edge = egui::pos2(ruler.right(), ruler.bottom() + 30.0);
        harness.hover_at(edge);
        harness.run();
        harness.drag_at(edge);
        harness.hover_at(edge + egui::vec2(300.0, 0.0));
        harness.run();
        harness.drop_at(edge + egui::vec2(300.0, 0.0));
        harness.run();
        assert_eq!(harness.state().arrangement().header_width, 160.0);
        let header = harness.get_by_label("Track Drums").rect();
        let controls = [
            harness.get_by_label("Turn Drums off").rect(),
            harness.get_by_label("Solo Drums").rect(),
            harness.get_by_label_contains("Cue Drums").rect(),
            harness.get_by_label_contains("Arm Drums").rect(),
            harness.get_by_label("Drums volume").rect(),
            harness.get_by_label("Show automation of Drums").rect(),
            harness.get_by_label_contains("Drums level:").rect(),
        ];
        for (index, control) in controls.iter().enumerate() {
            assert!(
                control.is_positive() && header.contains_rect(*control),
                "depth {depth}: {control:?} outside {header:?}"
            );
            for other in &controls[index + 1..] {
                assert!(!control.intersects(*other), "depth {depth}: {control:?} overlaps {other:?}");
            }
        }
        let name = harness
            .ctx
            .data(|data| data.get_temp::<suhina_ui::app::NameSpot>(egui::Id::new(("track-name-rect", 1_u64))))
            .unwrap()
            .rect;
        assert!(name.width() >= 48.0 && header.contains_rect(name), "unusable name: {name:?}");
        for control in controls {
            assert!(!name.intersects(control), "name overlaps {control:?}");
        }
        harness.get_by_label_contains("Arm Drums").click();
        harness.run();
        assert!(harness.state().session().unwrap().is_armed(suhina_engine::model::TrackId(1)));
    }
}
