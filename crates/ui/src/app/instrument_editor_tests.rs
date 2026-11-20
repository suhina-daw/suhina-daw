// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exercise the two native presentations against the same session, without an audio device.
#![allow(clippy::unwrap_used)] // small test fixtures, never shipped

use super::*;
use egui_kittest::{Harness, kittest::Queryable};
use std::sync::Arc;

fn editor() -> Harness<'static, App> {
    let mut session = Session::new(suhina_session::blank_project());
    session.execute("device.add", &json!({"track":1,"id":"instrument"})).unwrap();
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(session.project().clone()), &[]);
    app.view.selected = Some(TrackId(1));
    app.detail_open = true;
    app.detail_showing = detail_view::Showing::Devices;
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .with_size(Vec2::new(1280.0, 900.0))
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    crate::kit::theme::install(&harness.ctx);
    harness.run_steps(4);
    harness
}

fn parameter(h: &Harness<'_, App>, name: &str) -> f64 {
    h.state().session().unwrap().project().channels[0].devices[0]
        .params
        .iter()
        .find(|p| p.id == name)
        .map(|p| p.value)
        .unwrap_or_else(|| {
            suhina_device_registry::info("instrument").unwrap().params.iter().find(|p| p.id == name).unwrap().default
        })
}

#[test]
fn instrument_editor_pages_are_keyboard_accessible_view_state() {
    let mut h = editor();
    let before = h.state().session().unwrap().project().clone();
    assert!(!h.state().session().unwrap().can_undo());
    h.get_by_label("Open editor").click();
    h.run_steps(4);
    assert!(h.query_by_label("Output gain").is_some());
    h.get_by_label("Model page").focus();
    h.step();
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    assert!(h.query_by_label("Strike position").is_some());
    assert!(h.query_by_label("Output gain").is_none());
    h.key_press(egui::Key::Tab);
    h.step();
    h.key_press(egui::Key::Space);
    h.run_steps(4);
    assert!(h.query_by_label("Sample playback").is_some());
    h.get_by_label("Sample page").click();
    h.run_steps(4);
    assert!(h.query_by_label("Sample playback").is_some(), "repeated selection keeps the page");
    h.get_by_label("Close expanded panel").click();
    h.run_steps(4);
    h.get_by_label("Expand Instrument (experimental)").click();
    h.run_steps(4);
    assert!(h.query_by_label("Sample playback").is_some(), "header reopen remembers the view");
    assert_eq!(h.state().session().unwrap().project(), &before);
    assert!(!h.state().session().unwrap().can_undo());
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(h.query_by_label("Close expanded panel").is_none());
}

#[test]
fn instrument_editor_sound_edits_share_compact_state_and_one_undo() {
    let mut h = editor();
    let before = parameter(&h, "gain");
    h.get_by_label("Open editor").click();
    h.run_steps(4);
    let at = h.get_by_label("Output gain").rect().center();
    h.hover_at(at);
    h.step();
    h.drag_at(at);
    h.step();
    for i in 1..=8 {
        h.hover_at(at + Vec2::new(0.0, i as f32 * 4.0));
        h.step();
    }
    h.drop_at(at + Vec2::new(0.0, 32.0));
    h.run_steps(4);
    let after = parameter(&h, "gain");
    assert!(after < before, "a downward drag changes the native gain: {before} to {after}");
    assert_eq!(h.state().session().unwrap().undo_steps().0, 1);
    h.get_by_label("Model page").click();
    h.run_steps(4);
    h.get_by_label("Close expanded panel").click();
    h.run_steps(4);
    assert!(h.query_by_label("Gain").is_some());
    assert_eq!(parameter(&h, "gain"), after);
    h.state_mut().run(&Action::new("edit.undo", json!({})));
    h.run_steps(4);
    assert_eq!(parameter(&h, "gain"), before);
    assert!(!h.state().session().unwrap().can_undo(), "page changes did not enter history");
}

#[test]
fn instrument_editor_pages_do_not_leak_to_another_project_and_narrow_windows_scroll() {
    let mut h = editor();
    let project = h.state().session().unwrap().project().clone();
    h.get_by_label("Open editor").click();
    h.run_steps(4);
    h.get_by_label("Model page").click();
    h.run_steps(4);
    assert!(h.query_by_label("Strike position").is_some());
    h.state_mut().set_session(Session::new(project), &[]);
    h.state_mut().view.selected = Some(TrackId(1));
    h.state_mut().detail_open = true;
    h.state_mut().detail_showing = detail_view::Showing::Devices;
    h.run_steps(4);
    if h.query_by_label("Close expanded panel").is_none() {
        h.get_by_label("Expand Instrument (experimental)").click();
        h.run_steps(4);
    }
    assert!(h.query_by_label("Output gain").is_some(), "same device address in a new project starts on Play");
    assert!(h.query_by_label("Strike position").is_none());
    h.set_size(Vec2::new(640.0, 600.0));
    h.run_steps(4);
    h.get_by_label("Highest velocity").focus();
    h.run_steps(5);
    let rect = h.get_by_label("Highest velocity").rect();
    assert!(
        rect.center().x > 0.0 && rect.center().x < 640.0 && rect.center().y > 0.0 && rect.center().y < 600.0,
        "keyboard focus scrolls a distant control into the narrow editor: {rect:?}"
    );
    assert!(!h.state().session().unwrap().can_undo());
}

#[test]
fn instrument_expanded_waveform_is_a_drop_target_without_unsupported_sample_menus() {
    let mut h = editor();
    h.get_by_label("Open editor").click();
    h.run_steps(4);
    h.get_by_label("Sample page").click();
    h.run_steps(4);
    let wave =
        h.get_all_by_label("Sample waveform").map(|n| n.rect()).min_by(|a, b| a.top().total_cmp(&b.top())).unwrap();
    let hits: Vec<_> = h.state().sample_drops.iter().filter(|(rect, _, _)| rect.contains(wave.center())).collect();
    assert_eq!(hits.len(), 1, "the expanded waveform is collected after it is drawn");
    assert_eq!((hits[0].1, hits[0].2), (TrackId(1), 0));
    h.get_all_by_label("Sample waveform")
        .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
        .unwrap()
        .click_secondary();
    h.run_steps(4);
    assert!(h.query_by_label("Show whole sample").is_some());
    assert!(h.query_by_label("Grid").is_none());
    assert!(h.query_by_label("Source time signature").is_none());
    assert!(h.query_by_label("Snap to grid (Alt bypasses)").is_none());
}

#[test]
fn instrument_expanded_drop_plan_imports_and_undo_clears_both_presentations() {
    let path = std::env::temp_dir().join(format!("suhina-editor-import-{}.wav", std::process::id()));
    let mut wav = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for frame in 0..480 {
        wav.write_sample(((frame as f32 * 0.1).sin() * 4000.0) as i16).unwrap();
    }
    wav.finalize().unwrap();
    let mut h = editor();
    h.get_by_label("Open editor").click();
    h.run_steps(4);
    h.get_by_label("Sample page").click();
    h.run_steps(4);
    let wave =
        h.get_all_by_label("Sample waveform").map(|n| n.rect()).min_by(|a, b| a.top().total_cmp(&b.top())).unwrap();
    let items = [crate::interaction::drop::system_file(&path).unwrap()];
    let target = h.state().drop_target(&items, Some(wave.center())).unwrap();
    let plan = h.state().plan_drop(&items, target, false).unwrap();
    assert_eq!(plan.commands.len(), 1);
    assert_eq!(plan.commands[0].0, "device.loadSample");
    for (id, params) in plan.commands {
        h.state_mut().run(&Action::new(id, params));
    }
    h.run_steps(4);
    assert_eq!(parameter(&h, "source"), 1.0);
    assert_eq!(h.state().session().unwrap().undo_steps().0, 1);
    let source_id = crate::detail::device_canvas::sample_source_id(TrackId(1), 0);
    assert!(h.ctx.data(|data| data.get_temp::<Arc<suhina_engine::AudioData>>(source_id)).is_some());
    h.state_mut().run(&Action::new("edit.undo", json!({})));
    h.run_steps(4);
    assert_eq!(parameter(&h, "source"), 0.0);
    assert!(h.ctx.data(|data| data.get_temp::<Arc<suhina_engine::AudioData>>(source_id)).is_none());
    assert!(h.state().session().unwrap().project().channels[0].devices[0].sample_file().is_none());
    h.state_mut().run(&Action::new("edit.redo", json!({})));
    h.run_steps(4);
    assert_eq!(parameter(&h, "source"), 1.0);
    assert!(h.ctx.data(|data| data.get_temp::<Arc<suhina_engine::AudioData>>(source_id)).is_some());
    std::fs::remove_file(path).unwrap();
}

// Regression: turning a knob in the expanded editor left it focused, the editor kept the
// keyboard, and the computer keyboard's letters stopped playing notes.
#[test]
fn the_computer_keyboard_still_plays_after_a_knob_in_the_editor_is_used() {
    let mut h = editor();
    h.get_by_label("Open editor").click();
    h.run_steps(4);
    h.get_by_label("Output gain").focus();
    // A control locks the arrows once it has held the focus for a frame, as it always has by
    // the time a person presses one.
    h.run_steps(2);
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(2);
    assert!(crate::detail::device_canvas::owns_keyboard(&h.ctx), "the knob keeps the editor's keys");
    h.state_mut().toggle_typing_keys();
    h.step();
    h.key_down(egui::Key::A);
    h.run_steps(2);
    assert_eq!(h.state().typing.held_count(), 1, "A plays a note while the knob is focused");
    h.key_up(egui::Key::A);
    h.run_steps(2);
    assert_eq!(h.state().typing.held_count(), 0);
}
