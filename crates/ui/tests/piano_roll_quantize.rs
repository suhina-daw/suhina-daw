// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Quantise in the piano roll, driven like a user would: Ctrl+U quantises the selected notes (or
//! every note) with the last settings, and the Quantise panel sets them
//! (`docs/spec/ui-clip-editor.md` › Quantise).

#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::{Value, json};
use suhina_engine::model::{LaneData, Note};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::detail::piano_roll::key_name;
use suhina_ui::kit::widgets::Action;
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);
/// The demo clip the tests edit: on Drums, at bar 17 (beat 64).
const CLIP: &str = "Clip Beat B on Drums at bar 17";
const CLIP_START: f64 = 64.0;

/// The demo project in the Arrangement view with the clip selected, so the piano roll shows it,
/// and every note of the clip moved a tenth of a beat late, off the grid.
fn piano_app(renderer: &mut suhina_engine::Renderer) -> Harness<'static, App> {
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size(SIZE)
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label(CLIP).click();
    harness.run();
    let mut params = address(&harness);
    params["notes"] = json!((0..notes(&harness).len()).collect::<Vec<_>>());
    params["beats"] = json!(0.1);
    harness.state_mut().run(&Action::new("note.move", params));
    harness.run();
    harness
}

fn engine() -> suhina_engine::Renderer {
    suhina_engine::Renderer::stopped(
        &demo_project(),
        &suhina_engine::AudioPool::new(),
        48_000,
        suhina_session::devices(),
    )
    .unwrap()
}

/// The params that name the edited clip in a `note.*` command.
fn address(h: &Harness<'static, App>) -> Value {
    let project = h.state().session().unwrap().project();
    let lane = &project.arrangement.lanes[0];
    let index = lane.clips.iter().position(|c| c.start == CLIP_START).unwrap();
    json!({ "track": lane.track.unwrap().0, "index": index })
}

/// The edited clip's notes in the project.
fn notes(h: &Harness<'static, App>) -> Vec<Note> {
    let project = h.state().session().unwrap().project();
    let clip = project.arrangement.lanes[0].clips.iter().find(|c| c.start == CLIP_START).unwrap();
    match &project.content(clip.content).unwrap().lanes[0].data {
        LaneData::Notes(notes) => notes.to_vec(),
        LaneData::Audio(_) => Vec::new(),
    }
}

fn on_grid(beat: f64, step: f64) -> bool {
    let lines = beat / step;
    (lines - lines.round()).abs() < 1e-6
}

fn quantise(h: &mut Harness<'static, App>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::U);
    h.run();
}

#[test]
fn ctrl_u_quantises_every_note_when_none_is_selected_and_undoes_in_one_step() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let late = notes(&h);
    assert!(late.iter().all(|n| !on_grid(n.time, 0.25)), "the notes start off the sixteenths");
    quantise(&mut h);
    let after = notes(&h);
    // The default: sixteenths, all the way, ends kept.
    assert!(after.iter().all(|n| on_grid(n.time, 0.25)), "{after:?}");
    for (a, b) in after.iter().zip(&late) {
        assert!((a.time - (b.time - 0.1)).abs() < 1e-9);
        assert_eq!((a.duration, a.key), (b.duration, b.key));
    }
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(notes(&h), late);
}

#[test]
fn ctrl_u_quantises_only_the_selected_notes() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let late = notes(&h);
    let label = format!("Note {} at beat {}", key_name(late[1].key), (late[1].time * 1e6).round() / 1e6);
    h.get_by_label(&label).click();
    h.run();
    assert_eq!(h.state().piano_roll().selected.iter().copied().collect::<Vec<_>>(), [1]);
    quantise(&mut h);
    let after = notes(&h);
    assert!(on_grid(after[1].time, 0.25));
    assert_eq!(after[0], late[0]);
    assert_eq!(&after[2..], &late[2..]);
}

#[test]
fn the_quantise_block_in_the_clip_column_sets_the_grid_and_ends_that_ctrl_u_keeps() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let late = notes(&h);
    // The quantise button is the toolbar's icon; its label is its name.
    h.get_by_label("Quantise").click();
    h.run();
    assert!(h.state().piano_roll().quantize_open);
    // The block is part of the clip column on the left, not a panel over the note grid: its
    // controls sit left of the grid and outside it.
    let grid = h.get_by_label_contains("Piano roll,").rect();
    for button in ["Quantise notes", "Close quantise"] {
        let rect = h.get_by_label(button).rect();
        assert!(!rect.intersects(grid), "{button} is in the column: {rect:?} {grid:?}");
    }
    h.get_by_label("Quantise grid 1/16").click();
    h.run();
    assert!(h.query_by_label("1/4").is_some(), "the grid menu is open");
    h.get_by_label("1/4").click_accesskit();
    h.run();
    h.get_by_label("Move ends too").click();
    h.run();
    let settings = h.state().piano_roll().quantize;
    assert_eq!((settings.grid, settings.amount, settings.ends), ("1/4", 100.0, true));
    h.get_by_label("Quantise notes").click();
    h.run();
    let after = notes(&h);
    assert_ne!(after, late);
    assert!(after.iter().all(|n| on_grid(n.time, 1.0) && on_grid(n.time + n.duration, 1.0)), "{after:?}");
    // Escape closes the block and the properties move back up; Ctrl+U after an undo uses the
    // same settings.
    h.key_press(Key::Escape);
    h.run();
    assert!(!h.state().piano_roll().quantize_open);
    assert_eq!(h.get_by_label_contains("Piano roll,").rect(), grid, "the view is as it was");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(notes(&h), late);
    quantise(&mut h);
    assert_eq!(notes(&h), after);
}

#[test]
fn ctrl_u_does_nothing_while_the_piano_roll_is_hidden() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let late = notes(&h);
    // Shift+Tab hides the detail area and with it the piano roll.
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run();
    quantise(&mut h);
    assert_eq!(notes(&h), late);
}
