// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll in the window, driven like a user would: a selected MIDI clip shows its notes
//! in the detail area, and drawing, selecting, moving, resizing, deleting and velocity changes
//! become `note.*` commands (`docs/spec/ui-clip-editor.md`).

#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers, Pos2, Rect, Vec2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use suhina_engine::model::{LaneData, Note};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::detail::piano_roll::key_name;
use suhina_ui::kit::widgets::Action;
use suhina_ui::settings::Settings;
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);
/// The demo clip the tests edit: on Drums, at bar 17 (beat 64).
const CLIP: &str = "Clip Beat B on Drums at bar 17";
const CLIP_START: f64 = 64.0;

/// The demo project in the Arrangement view, the grid at an eighth (half a beat), and the clip selected so
/// the piano roll shows it. Frames are 50 ms apart, close enough for a double-click.
fn piano_app(renderer: &mut suhina_engine::Renderer) -> Harness<'static, App> {
    let mut session = Session::new(demo_project());
    // The grid value is the launch quantisation, which needs an engine to take it.
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    app.run(&Action::new("launcher.setQuantization", json!({ "value": "1/8" })));
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    // These tests set the grid in the control bar, so the piano roll follows it (Settings ›
    // Clip editor › Piano roll grid); its own grid value has tests of its own.
    let settings = Settings { piano_grid_follows: true, ..Settings::default() };
    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, settings);
    harness.run();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label(CLIP).click();
    harness.run();
    harness
}

fn engine() -> suhina_engine::Renderer {
    suhina_engine::Renderer::stopped(&demo_project(), &suhina_engine::AudioPool::new(), 48_000).unwrap()
}

/// The edited clip's notes in the project.
fn notes(h: &Harness<'static, App>) -> Vec<Note> {
    let session = h.state().session().unwrap();
    let project = session.project();
    let clip = project.arrangement.lanes[0].clips.iter().find(|c| c.start == CLIP_START).unwrap();
    match &project.content(clip.content).unwrap().lanes[0].data {
        LaneData::Notes(notes) => notes.clone(),
        LaneData::Audio(_) => Vec::new(),
    }
}

fn note_label(note: &Note) -> String {
    format!("Note {} at beat {}", key_name(note.key), note.time)
}

fn note_rect(h: &Harness<'static, App>, note: &Note) -> Rect {
    h.get_by_label(&note_label(note)).rect()
}

/// Points per beat and per key row, measured from two notes on screen.
fn scale(h: &Harness<'static, App>) -> (f32, f32) {
    let n = notes(h);
    let (a, b) = (note_rect(h, &n[0]), note_rect(h, &n[1]));
    let beat = (b.left() - a.left()) / (n[1].time - n[0].time) as f32;
    let row = (a.top() - b.top()) / (f32::from(n[1].key) - f32::from(n[0].key));
    (beat, row)
}

fn button(h: &mut Harness<'static, App>, pos: Pos2, pressed: bool, modifiers: Modifiers) {
    h.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers,
    });
    h.step();
}

fn click(h: &mut Harness<'static, App>, pos: Pos2, modifiers: Modifiers) {
    h.input_mut().events.push(egui::Event::ModifiersChanged(modifiers));
    h.hover_at(pos);
    h.step();
    button(h, pos, true, modifiers);
    button(h, pos, false, modifiers);
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
}

/// Presses and releases in one frame each, a few frames apart: a double-click.
fn double_click(h: &mut Harness<'static, App>, pos: Pos2) {
    h.hover_at(pos);
    h.run();
    for _ in 0..2 {
        h.hover_at(pos);
        h.drag_at(pos);
        h.drop_at(pos);
        h.step();
    }
    h.run();
}

/// Presses at `from`, moves to `to` in a few frames and lets go, holding `modifiers`.
fn drag(h: &mut Harness<'static, App>, from: Pos2, to: Pos2, modifiers: Modifiers) {
    h.input_mut().events.push(egui::Event::ModifiersChanged(modifiers));
    h.hover_at(from);
    h.step();
    button(h, from, true, modifiers);
    for step in 1..=4 {
        h.hover_at(from + (to - from) * (step as f32 / 4.0));
        h.step();
    }
    button(h, to, false, modifiers);
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run();
}

fn selected(h: &Harness<'static, App>) -> Vec<usize> {
    h.state().piano_roll().selected.iter().copied().collect()
}

fn undo(h: &mut Harness<'static, App>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
}

#[test]
fn a_selected_midi_clip_shows_its_notes_in_the_piano_roll() {
    let mut renderer = engine();
    let h = piano_app(&mut renderer);
    let count = notes(&h).len();
    assert!(count > 0);
    assert!(h.query_by_label(&format!("Piano roll, {count} notes")).is_some());
    for note in notes(&h) {
        assert!(h.query_by_label(&note_label(&note)).is_some(), "{}", note_label(&note));
    }
    // Every note has its velocity stem in the lane.
    assert_eq!(h.query_all_by_label_contains("Velocity of note").count(), count);
}

#[test]
fn a_double_click_draws_a_note_on_the_grid_and_another_deletes_it() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    let (beat, row) = scale(&h);
    let first = note_rect(&h, &before[0]);
    // Half a beat and a bit after the first note, in its row: the note starts on the eighth
    // before the pointer, an eighth long.
    let at = pos2(first.left() + beat * 0.6, first.center().y);
    double_click(&mut h, at);
    let after = notes(&h);
    assert_eq!(after.len(), before.len() + 1);
    let added = after.last().unwrap();
    assert_eq!((added.time, added.duration, added.key), (before[0].time + 0.5, 0.5, before[0].key));
    assert_eq!(suhina_session::midi_velocity(added.velocity), 100);
    assert_eq!(selected(&h), [before.len()]);
    // A double-click on it deletes it again; a row higher is a key higher.
    let at = note_rect(&h, added).center();
    double_click(&mut h, at);
    assert_eq!(notes(&h), before);
    double_click(&mut h, at - vec2(0.0, row));
    assert_eq!(notes(&h).last().unwrap().key, before[0].key + 1);
    undo(&mut h);
    assert_eq!(notes(&h), before);
}

#[test]
fn clicks_and_a_rectangle_select_notes() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let n = notes(&h);
    let at = note_rect(&h, &n[1]).center();
    click(&mut h, at, Modifiers::NONE);
    h.run();
    assert_eq!(selected(&h), [1]);
    let at = note_rect(&h, &n[2]).center();
    click(&mut h, at, Modifiers::SHIFT);
    h.run();
    assert_eq!(selected(&h), [1, 2]);
    let at = note_rect(&h, &n[1]).center();
    click(&mut h, at, Modifiers::SHIFT);
    h.run();
    assert_eq!(selected(&h), [2]);
    // A click on an empty spot of the grid clears; a rectangle around the first two notes
    // selects them. The empty spot is inside the note grid (not the key column at its left).
    let (a, b) = (note_rect(&h, &n[0]), note_rect(&h, &n[1]));
    let grid = h.get_by_label_contains("Piano roll,").rect();
    let gap = pos2(a.right() + 2.0, a.center().y);
    assert!(gap.x > grid.left(), "the spot is in the grid, not the key column: {gap:?} {grid:?}");
    click(&mut h, gap, Modifiers::NONE);
    h.run();
    assert!(selected(&h).is_empty());
    // The rectangle starts inside the grid, not in the key column at its left (a clip whose
    // first note starts at the clip's start begins at the grid's left edge).
    let around = a.union(b).expand(3.0);
    let from = pos2(grid.left() + 2.0, around.top().max(grid.top() + 1.0));
    drag(&mut h, from, around.right_bottom(), Modifiers::NONE);
    assert_eq!(selected(&h), [0, 1]);
    // Nothing in the project changed.
    assert_eq!(notes(&h), n);
}

#[test]
fn dragging_a_note_moves_the_selection_by_grid_steps_and_alt_moves_freely() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    let (beat, row) = scale(&h);
    let from = note_rect(&h, &before[0]).center() - vec2(6.0, 0.0);
    // 1.3 beats right and one row up: an eighth grid makes it 1.5 beats, three steps.
    drag(&mut h, from, from + vec2(beat * 1.3, -row), Modifiers::NONE);
    let moved = notes(&h)[0];
    assert_eq!((moved.time, moved.key), (before[0].time + 1.5, before[0].key + 1));
    assert_eq!(&notes(&h)[1..], &before[1..]);
    undo(&mut h);
    assert_eq!(notes(&h), before);
    // With Alt pressed once the drag has begun, it is not snapped (Alt held at the press
    // changes the note's velocity instead: › Velocity).
    let from = note_rect(&h, &before[0]).center() - vec2(6.0, 0.0);
    let to = from + vec2(beat * 1.3, 0.0);
    h.hover_at(from);
    h.step();
    button(&mut h, from, true, Modifiers::NONE);
    h.hover_at(from + (to - from) * 0.25);
    h.step();
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::ALT));
    for step in 2..=4 {
        h.hover_at(from + (to - from) * (step as f32 / 4.0));
        h.step();
    }
    button(&mut h, to, false, Modifiers::ALT);
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run();
    let free = notes(&h)[0].time - before[0].time;
    assert!((free - 1.3).abs() < 0.05, "{free}");
}

#[test]
fn dragging_a_notes_end_resizes_the_selection_on_the_grid() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    let (beat, _) = scale(&h);
    let at = note_rect(&h, &before[0]).center();
    click(&mut h, at, Modifiers::NONE);
    let at = note_rect(&h, &before[1]).center();
    click(&mut h, at, Modifiers::SHIFT);
    h.run();
    let end = note_rect(&h, &before[0]).right_center() - vec2(3.5, 0.0);
    drag(&mut h, end, end + vec2(beat * 0.3, 0.0), Modifiers::NONE);
    let after = notes(&h);
    // The end, half a beat after the start, goes 0.3 beats on: onto the line at beat 1.
    let grown = after[0].duration - before[0].duration;
    assert!((grown - 0.5).abs() < 1e-9, "{grown}");
    assert!((after[1].duration - before[1].duration - 0.5).abs() < 1e-9);
    assert_eq!(after[2], before[2]);
}

#[test]
fn delete_removes_the_selected_notes_and_not_the_clip() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    let clips = |h: &Harness<'static, App>| h.state().session().unwrap().project().arrangement.lanes[0].clips.len();
    let count = clips(&h);
    let at = note_rect(&h, &before[1]).center();
    click(&mut h, at, Modifiers::NONE);
    h.run();
    h.key_press(Key::Delete);
    h.run();
    assert_eq!(notes(&h).len(), before.len() - 1);
    assert_eq!(clips(&h), count);
    assert!(selected(&h).is_empty());
    undo(&mut h);
    assert_eq!(notes(&h), before);
}

#[test]
fn dragging_a_velocity_stem_changes_the_selections_velocity() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    let velocity = |n: &Note| suhina_session::midi_velocity(n.velocity);
    let label =
        format!("Velocity of note {} at beat {}: {}", key_name(before[0].key), before[0].time, velocity(&before[0]));
    let stem = h.get_by_label(&label).rect();
    let from = stem.center();
    drag(&mut h, from, from - vec2(0.0, 30.0), Modifiers::NONE);
    let after = notes(&h);
    assert!(velocity(&after[0]) > velocity(&before[0]), "{} {}", velocity(&after[0]), velocity(&before[0]));
    assert_eq!(after[1], before[1]);
    assert_eq!(selected(&h), [0]);
}

#[test]
fn the_wheel_scrolls_and_ctrl_zooms() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let grid = h.get_by_label_contains("Piano roll,").rect();
    let wheel = |h: &mut Harness<'static, App>, delta: Vec2, modifiers: Modifiers| {
        h.hover_at(grid.center());
        h.step();
        h.input_mut().events.push(egui::Event::ModifiersChanged(modifiers));
        h.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta,
            modifiers,
            phase: egui::TouchPhase::Move,
        });
        h.run();
        h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
        h.run();
    };
    let (width, scroll) = (h.state().piano_roll().beat_width, h.state().piano_roll().scroll.y);
    wheel(&mut h, vec2(0.0, -60.0), Modifiers::NONE);
    assert_ne!(h.state().piano_roll().scroll.y, scroll);
    wheel(&mut h, vec2(0.0, 60.0), Modifiers::COMMAND);
    assert!(h.state().piano_roll().beat_width > width);
    let height = h.state().piano_roll().key_height;
    wheel(&mut h, vec2(0.0, 60.0), Modifiers::COMMAND | Modifiers::SHIFT);
    assert!(h.state().piano_roll().key_height > height);
}

/// Clicks note `index`, which selects it alone and gives the piano roll the keyboard.
fn pick(h: &mut Harness<'static, App>, index: usize) {
    let at = note_rect(h, &notes(h)[index]).center();
    click(h, at, Modifiers::NONE);
    h.run();
}

fn key(h: &mut Harness<'static, App>, modifiers: Modifiers, key: Key) {
    h.key_press_modifiers(modifiers, key);
    h.run();
}

#[test]
fn ctrl_a_selects_every_note_and_escape_clears() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    pick(&mut h, 0);
    key(&mut h, Modifiers::COMMAND, Key::A);
    assert_eq!(selected(&h), (0..before.len()).collect::<Vec<_>>());
    key(&mut h, Modifiers::NONE, Key::Escape);
    assert!(selected(&h).is_empty());
    assert_eq!(notes(&h), before);
    assert!(!h.state().session().unwrap().can_undo(), "selecting is no undo step");
}

#[test]
fn arrows_move_the_selection_by_a_grid_step_a_semitone_or_an_octave() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    let first = before[0];
    pick(&mut h, 0);
    // The grid is an eighth: half a beat.
    key(&mut h, Modifiers::NONE, Key::ArrowRight);
    assert_eq!(notes(&h)[0].time, first.time + 0.5);
    key(&mut h, Modifiers::NONE, Key::ArrowLeft);
    assert_eq!(notes(&h)[0].time, first.time);
    key(&mut h, Modifiers::NONE, Key::ArrowUp);
    assert_eq!(notes(&h)[0].key, first.key + 1);
    key(&mut h, Modifiers::NONE, Key::ArrowDown);
    assert_eq!(notes(&h)[0].key, first.key);
    key(&mut h, Modifiers::SHIFT, Key::ArrowUp);
    assert_eq!(notes(&h)[0].key, first.key + 12);
    key(&mut h, Modifiers::SHIFT, Key::ArrowDown);
    assert_eq!(notes(&h)[0], first);
    // The other notes stayed; each key was one undo step.
    assert_eq!(&notes(&h)[1..], &before[1..]);
    undo(&mut h);
    assert_eq!(notes(&h)[0].key, first.key + 12);
    assert_eq!(selected(&h), [0]);
}

#[test]
fn shift_left_and_right_change_the_selections_length_by_a_grid_step() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let first = notes(&h)[0];
    pick(&mut h, 0);
    key(&mut h, Modifiers::SHIFT, Key::ArrowRight);
    key(&mut h, Modifiers::SHIFT, Key::ArrowRight);
    assert!((notes(&h)[0].duration - first.duration - 1.0).abs() < 1e-9);
    key(&mut h, Modifiers::SHIFT, Key::ArrowLeft);
    assert!((notes(&h)[0].duration - first.duration - 0.5).abs() < 1e-9);
    // Never shorter than a step by the keys.
    for _ in 0..40 {
        h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowLeft);
        h.step();
    }
    h.run();
    let length = notes(&h)[0].duration;
    assert!((0.5 - 1e-9..1.0).contains(&length), "{length}");
}

#[test]
fn copy_and_paste_put_the_notes_after_the_selection_and_select_them() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    pick(&mut h, 1);
    key(&mut h, Modifiers::COMMAND, Key::C);
    assert_eq!(notes(&h), before, "copy changes nothing");
    key(&mut h, Modifiers::COMMAND, Key::V);
    let after = notes(&h);
    assert_eq!(after.len(), before.len() + 1);
    let pasted = after[before.len()];
    assert_eq!((pasted.time, pasted.key), (before[1].time + before[1].duration, before[1].key));
    assert_eq!(selected(&h), [before.len()]);
    // Again: after the pasted note, which is now the selection.
    key(&mut h, Modifiers::COMMAND, Key::V);
    assert_eq!(notes(&h)[before.len() + 1].time, pasted.time + pasted.duration);
    undo(&mut h);
    undo(&mut h);
    assert_eq!(notes(&h), before);
}

#[test]
fn cut_removes_the_notes_and_paste_brings_them_back() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    let clips = |h: &Harness<'static, App>| h.state().session().unwrap().project().arrangement.lanes[0].clips.len();
    let count = clips(&h);
    pick(&mut h, 0);
    key(&mut h, Modifiers::COMMAND, Key::X);
    assert_eq!(notes(&h), &before[1..]);
    assert_eq!(clips(&h), count, "the clip stays");
    assert!(selected(&h).is_empty());
    // Nothing selected: the paste goes where the command puts it, right after the cut note.
    key(&mut h, Modifiers::COMMAND, Key::V);
    let back = *notes(&h).last().unwrap();
    assert_eq!((back.time, back.key), (before[0].time + before[0].duration, before[0].key));
    undo(&mut h);
    undo(&mut h);
    assert_eq!(notes(&h), before);
}

#[test]
fn ctrl_d_duplicates_the_selection_right_after_it() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    pick(&mut h, 0);
    let at = note_rect(&h, &before[1]).center();
    click(&mut h, at, Modifiers::SHIFT);
    h.run();
    key(&mut h, Modifiers::COMMAND, Key::D);
    let after = notes(&h);
    assert_eq!(after.len(), before.len() + 2);
    let span = (before[0].time + before[0].duration).max(before[1].time + before[1].duration)
        - before[0].time.min(before[1].time);
    assert_eq!(after[before.len()].time, before[0].time + span);
    assert_eq!(after[before.len() + 1].time, before[1].time + span);
    assert_eq!(selected(&h), [before.len(), before.len() + 1]);
    undo(&mut h);
    assert_eq!(notes(&h), before);
}

#[test]
fn the_keys_leave_the_notes_alone_when_the_piano_roll_does_not_have_the_keyboard() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    pick(&mut h, 0);
    // A click outside the piano roll (on the clip in the arrangement) takes the keyboard away.
    h.get_by_label(CLIP).click();
    h.run();
    assert!(!h.state().piano_roll().focused);
    for (modifiers, k) in [
        (Modifiers::NONE, Key::ArrowRight),
        (Modifiers::NONE, Key::ArrowUp),
        (Modifiers::SHIFT, Key::ArrowRight),
        (Modifiers::COMMAND, Key::A),
    ] {
        key(&mut h, modifiers, k);
    }
    assert_eq!(notes(&h), before);
    assert!(!h.state().session().unwrap().can_undo());
}
