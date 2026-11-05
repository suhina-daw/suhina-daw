// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's ruler (`docs/spec/ui-clip-editor.md` › Layout › Ruler, › Zoom and scroll): the
//! loop handles and the clip's end marker dragged on the grid (Alt: free), one undo step each;
//! dragging the ruler up and down zooms time and sideways scrolls it; Z and Shift+Z.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers, Pos2, Rect, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::{Clip, LaneData, Note, Project};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::detail::piano_roll::key_name;
use suhina_ui::settings::Settings;
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);
/// The demo clip the tests edit: on Drums, at bar 17 (beat 64), 48 beats long, looping its first
/// bar (beats 0 to 4); the next clip on the lane starts where it ends.
const CLIP: &str = "Clip Beat B on Drums at bar 17";
const CLIP_START: f64 = 64.0;

fn piano_app(renderer: &mut suhina_engine::Renderer, project: Project) -> Harness<'static, App> {
    let mut session = Session::new(project);
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, Settings::default());
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

/// The demo project with the edited clip not looping: it plays its 48 beats once.
fn unlooped() -> Project {
    let mut project = demo_project();
    let clip = project.arrangement.lanes[0].clips.iter_mut().find(|c| c.start == CLIP_START).unwrap();
    clip.loop_range = None;
    project
}

/// The edited clip in the project.
fn clip(h: &Harness<'static, App>) -> Clip {
    let session = h.state().session().unwrap();
    session.project().arrangement.lanes[0].clips.iter().find(|c| c.start == CLIP_START).unwrap().clone()
}

fn notes(h: &Harness<'static, App>) -> Vec<Note> {
    let session = h.state().session().unwrap();
    let project = session.project();
    match &project.content(clip(h).content).unwrap().lanes[0].data {
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

/// The note grid on screen.
fn grid(h: &Harness<'static, App>) -> Rect {
    let count = notes(h).len();
    h.get_by_label(&format!("Piano roll, {count} notes")).rect()
}

fn ruler(h: &Harness<'static, App>) -> Rect {
    h.get_by_label("Piano roll ruler").rect()
}

/// Points per beat, measured from two notes on screen, and the x of beat 0.
fn axis(h: &Harness<'static, App>) -> (f32, f32) {
    let n = notes(h);
    let (a, b) = (note_rect(h, &n[0]), note_rect(h, &n[1]));
    let beat = (b.left() - a.left()) / (n[1].time - n[0].time) as f32;
    (beat, a.left() - n[0].time as f32 * beat)
}

/// Where a handle or marker is: the middle of its named spot (cut at the edges of the ruler).
fn handle(h: &Harness<'static, App>, label: &str) -> Pos2 {
    h.get_by_label(label).rect().center()
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

fn undo(h: &mut Harness<'static, App>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
}

fn loop_end(h: &Harness<'static, App>) -> f64 {
    clip(h).loop_range.unwrap().end
}

#[test]
fn the_ruler_names_its_loop_handles_and_markers() {
    let mut renderer = engine();
    let h = piano_app(&mut renderer, demo_project());
    for label in ["Piano roll ruler", "Loop start", "Loop end", "Clip start marker"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    // A looping clip has no end marker: its loop's end is where its content ends.
    assert!(h.query_by_label("Clip end marker").is_none());
    // The loop row is the ruler's top 8 points; the handles are in it.
    let (ruler, end) = (ruler(&h), handle(&h, "Loop end"));
    assert!(end.y < ruler.top() + 14.0, "{end:?} in {ruler:?}");
    let marker = handle(&h, "Clip start marker");
    assert!(marker.y > ruler.top() + 14.0, "{marker:?} under the loop row");
}

#[test]
fn dragging_the_loop_end_snaps_it_to_the_grid_and_one_undo_takes_it_back() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, demo_project());
    assert_eq!(loop_end(&h), 4.0);
    let (beat, zero) = axis(&h);
    let from = handle(&h, "Loop end");
    // To just short of beat 3: it lands on a grid line, not where it was let go.
    drag(&mut h, from, pos2(zero + 2.97 * beat, from.y), Modifiers::NONE);
    let end = loop_end(&h);
    assert!((end - 3.0).abs() < 1e-9, "snapped to beat 3, got {end}");
    assert_eq!(clip(&h).loop_range.unwrap().start, 0.0, "the loop start stays");
    undo(&mut h);
    assert_eq!(loop_end(&h), 4.0);
}

#[test]
fn alt_drags_the_loop_end_off_the_grid() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, demo_project());
    let (beat, zero) = axis(&h);
    let from = handle(&h, "Loop end");
    // The handle goes where the pointer is let go.
    let target = zero + 2.97 * beat;
    drag(&mut h, from, pos2(target, from.y), Modifiers::ALT);
    let end = loop_end(&h);
    let expected = f64::from((target - zero) / beat);
    assert!((end - expected).abs() < 0.01, "free at {expected}, got {end}");
    assert!(((end * 16.0).round() - end * 16.0).abs() > 1e-6, "not on a sixteenth: {end}");
}

#[test]
fn dragging_the_loop_start_moves_it_and_notes_outside_the_loop_stay_listed() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, demo_project());
    let before = notes(&h);
    let (beat, zero) = axis(&h);
    let from = handle(&h, "Loop start");
    drag(&mut h, from, pos2(zero + 2.02 * beat, from.y), Modifiers::NONE);
    let range = clip(&h).loop_range.unwrap();
    assert_eq!((range.start, range.end), (2.0, 4.0));
    // Notes before the loop are shaded, not removed: each is still there and named.
    assert_eq!(notes(&h), before);
    for note in before.iter().filter(|n| n.time < 2.0) {
        assert!(h.query_by_label(&note_label(note)).is_some(), "{note:?}");
    }
}

#[test]
fn dragging_the_end_marker_changes_the_clips_length_in_one_undo_step() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, unlooped());
    assert_eq!(clip(&h).length, 48.0);
    let (beat, zero) = axis(&h);
    let from = handle(&h, "Clip end marker");
    drag(&mut h, from, pos2(zero + 40.1 * beat, from.y), Modifiers::NONE);
    let after = clip(&h);
    assert!((after.length - 40.0).abs() < 1e-9, "on the grid at 40 beats, got {}", after.length);
    assert_eq!(after.start, CLIP_START, "the clip keeps its place");
    undo(&mut h);
    assert_eq!(clip(&h).length, 48.0);
    // The next clip on the lane starts where this one ends: dragging later changes nothing.
    let from = handle(&h, "Clip end marker");
    let to = pos2(ruler(&h).right() - 1.0, from.y);
    drag(&mut h, pos2(from.x - 1.0, from.y), to, Modifiers::NONE);
    assert_eq!(clip(&h).length, 48.0);
}

#[test]
fn dragging_the_ruler_up_and_down_zooms_time_and_sideways_scrolls_it() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, demo_project());
    // The latest note, near where the drags start, so it stays in view.
    let n = *notes(&h).iter().max_by(|a, b| a.time.total_cmp(&b.time)).unwrap();
    let ruler = ruler(&h);
    // A spot on the bar numbers away from the start marker.
    let at = pos2(ruler.center().x, ruler.bottom() - 4.0);
    let width = note_rect(&h, &n).width();
    drag(&mut h, at, at - egui::vec2(0.0, 60.0), Modifiers::NONE);
    let narrower = note_rect(&h, &n).width();
    assert!(narrower < width * 0.8, "up zooms out: {width} to {narrower}");
    drag(&mut h, at, at + egui::vec2(0.0, 40.0), Modifiers::NONE);
    let wider = note_rect(&h, &n).width();
    assert!(wider > narrower * 1.2, "down zooms in: {narrower} to {wider}");
    // Nothing was edited.
    assert_eq!(clip(&h).loop_range.unwrap().end, 4.0);
    // Sideways: the notes follow the pointer.
    let left = note_rect(&h, &n).left();
    let scroll = h.state().piano_roll().scroll.x;
    drag(&mut h, at, at - egui::vec2(80.0, 0.0), Modifiers::NONE);
    assert!(h.state().piano_roll().scroll.x > scroll, "scrolled later in time");
    assert!(note_rect(&h, &n).left() < left - 40.0);
}

#[test]
fn z_zooms_to_the_selection_and_shift_z_to_the_whole_clip() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, demo_project());
    let ruler_rect = ruler(&h);
    let at = pos2(ruler_rect.center().x, ruler_rect.bottom() - 4.0);
    // Zoomed out first, so the keys have something to undo.
    drag(&mut h, at, at - egui::vec2(0.0, 80.0), Modifiers::NONE);
    // Every note selected (after the press in the ruler, the piano roll has the keyboard).
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run();
    h.key_press(Key::Z);
    h.run();
    let n = notes(&h);
    let first = n.iter().min_by(|a, b| a.time.total_cmp(&b.time)).unwrap();
    let last = n.iter().max_by(|a, b| (a.time + a.duration).total_cmp(&(b.time + b.duration))).unwrap();
    let grid = grid(&h);
    assert!((note_rect(&h, first).left() - grid.left()).abs() < 2.0, "the first note at the left edge");
    let (beat, zero) = axis(&h);
    let end = zero + (last.time + last.duration) as f32 * beat;
    assert!((end - grid.right()).abs() < 2.0, "the last note's end at the right edge: {end} vs {}", grid.right());
    // Zoomed in by a drag, Shift+Z fits the clip's 4 beats again.
    drag(&mut h, at, at + egui::vec2(0.0, 60.0), Modifiers::NONE);
    h.key_press_modifiers(Modifiers::SHIFT, Key::Z);
    h.run();
    let (beat, zero) = axis(&h);
    assert!((zero - grid.left()).abs() < 2.0, "beat 0 at the left edge");
    assert!((zero + 4.0 * beat - grid.right()).abs() < 2.0, "beat 4 at the right edge");
}
