// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's gestures of #351, driven as a user would (`docs/spec/ui-clip-editor.md`
//! › Resizing, › Moving, › Velocity, › Layout, › Playing the keys): the left edge, Ctrl+drag to
//! copy, Alt+drag for velocity, a velocity line, the lane's top edge and the Audition button.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use suhina_engine::model::{LaneData, Note};
use suhina_session::{Session, midi_velocity};
use suhina_ui::demo::demo_project;
use suhina_ui::detail::piano_roll::key_name;
use suhina_ui::kit::widgets::Action;
use suhina_ui::settings::Settings;
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);
/// The demo clip the tests edit: on Drums, at bar 17 (beat 64). Its notes start on beats 0 to 3,
/// each half a beat long, velocity 102.
const CLIP: &str = "Clip Beat B on Drums at bar 17";
const CLIP_START: f64 = 64.0;

/// The demo project in the Arrangement view, the grid at an eighth (half a beat), and the clip
/// selected so the piano roll shows it.
fn piano_app(renderer: &mut suhina_engine::Renderer) -> Harness<'static, App> {
    let mut session = Session::new(demo_project());
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

fn notes(h: &Harness<'static, App>) -> Vec<Note> {
    let session = h.state().session().unwrap();
    let project = session.project();
    let clip = project.arrangement.lanes[0].clips.iter().find(|c| c.start == CLIP_START).unwrap();
    match &project.content(clip.content).unwrap().lanes[0].data {
        LaneData::Notes(notes) => notes.clone(),
        LaneData::Audio(_) => Vec::new(),
    }
}

fn note_rect(h: &Harness<'static, App>, note: &Note) -> Rect {
    h.get_by_label(&format!("Note {} at beat {}", key_name(note.key), note.time)).rect()
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

/// Presses at `from` with `modifiers` held, moves to `to` in four steps (calling `during` after
/// the second) and lets go.
fn drag_with(
    h: &mut Harness<'static, App>,
    from: Pos2,
    to: Pos2,
    modifiers: Modifiers,
    during: &mut dyn FnMut(&mut Harness<'static, App>),
) {
    h.input_mut().events.push(egui::Event::ModifiersChanged(modifiers));
    h.hover_at(from);
    h.step();
    button(h, from, true, modifiers);
    for step in 1..=4 {
        h.hover_at(from + (to - from) * (step as f32 / 4.0));
        h.step();
        if step == 2 {
            during(h);
        }
    }
    button(h, to, false, modifiers);
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run();
}

fn drag(h: &mut Harness<'static, App>, from: Pos2, to: Pos2, modifiers: Modifiers) {
    drag_with(h, from, to, modifiers, &mut |_| {});
}

fn click(h: &mut Harness<'static, App>, pos: Pos2) {
    h.hover_at(pos);
    h.step();
    button(h, pos, true, Modifiers::NONE);
    button(h, pos, false, Modifiers::NONE);
    h.run();
}

fn undo(h: &mut Harness<'static, App>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
}

fn selected(h: &Harness<'static, App>) -> Vec<usize> {
    h.state().piano_roll().selected.iter().copied().collect()
}

fn velocities(h: &Harness<'static, App>) -> Vec<u8> {
    notes(h).iter().map(|n| midi_velocity(n.velocity)).collect()
}

/// The velocity lane: where the stems are drawn.
fn lane(h: &Harness<'static, App>) -> Rect {
    h.query_all_by_label_contains("Velocity of note").next().unwrap().rect()
}

// ---------------------------------------------------------------------------------------------

#[test]
fn dragging_a_notes_left_edge_moves_its_start_on_the_grid_and_keeps_its_end() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    let (beat, _) = scale(&h);
    let edge = note_rect(&h, &before[1]).left_center() + vec2(1.5, 0.0);
    // 0.3 beats earlier: the start goes to the line at half a beat, the end stays at 1.5.
    drag(&mut h, edge, edge - vec2(beat * 0.3, 0.0), Modifiers::NONE);
    let after = notes(&h);
    assert_eq!((after[1].time, after[1].duration), (0.5, 1.0), "{:?}", after[1]);
    assert_eq!((after[1].key, after[1].velocity), (before[1].key, before[1].velocity));
    assert_eq!(after[0], before[0]);
    // One undo step puts it back.
    undo(&mut h);
    assert_eq!(notes(&h), before);

    // With Alt it is not snapped.
    let edge = note_rect(&h, &before[1]).left_center() + vec2(1.5, 0.0);
    drag(&mut h, edge, edge - vec2(beat * 0.3, 0.0), Modifiers::ALT);
    let after = notes(&h);
    assert!((after[1].time - 0.7).abs() < 0.03, "{:?}", after[1]);
    assert!((after[1].time + after[1].duration - 1.5).abs() < 1e-9, "the end stays: {:?}", after[1]);
    undo(&mut h);
    assert_eq!(notes(&h), before);

    // Dragged far past the end, it stops a hundredth of a beat before it.
    let edge = note_rect(&h, &before[1]).left_center() + vec2(1.5, 0.0);
    drag(&mut h, edge, edge + vec2(beat * 0.8, 0.0), Modifiers::ALT);
    let after = notes(&h);
    assert!((after[1].duration - 0.01).abs() < 1e-6, "{:?}", after[1]);
    assert!((after[1].time + after[1].duration - 1.5).abs() < 1e-9);
}

#[test]
fn the_left_edge_of_the_first_note_never_goes_before_beat_0() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    let (beat, _) = scale(&h);
    let edge = note_rect(&h, &before[1]).left_center() + vec2(1.5, 0.0);
    // Note 0 starts at beat 0: with both selected, the drag stops there for both.
    let first = note_rect(&h, &before[0]).center();
    click(&mut h, first);
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::SHIFT));
    let second = note_rect(&h, &before[1]).center();
    h.hover_at(second);
    h.step();
    button(&mut h, second, true, Modifiers::SHIFT);
    button(&mut h, second, false, Modifiers::SHIFT);
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run();
    assert_eq!(selected(&h), [0, 1]);
    drag(&mut h, edge, edge - vec2(beat * 2.0, 0.0), Modifiers::NONE);
    let after = notes(&h);
    assert_eq!(after, before, "no start can move earlier than beat 0, so nothing changes");
}

#[test]
fn ctrl_drag_copies_the_selection_and_selects_the_copies() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    let (beat, row) = scale(&h);
    let from = note_rect(&h, &before[0]).center();
    // 1.3 beats on and a row up: the copy lands 1.5 beats on (three eighths), a semitone higher.
    drag(&mut h, from, from + vec2(beat * 1.3, -row), Modifiers::COMMAND);
    let after = notes(&h);
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(&after[..before.len()], &before[..], "the originals stay");
    let copy = after[before.len()];
    assert_eq!((copy.time, copy.key, copy.duration), (before[0].time + 1.5, before[0].key + 1, before[0].duration));
    assert_eq!(copy.velocity, before[0].velocity);
    assert_eq!(selected(&h), [before.len()], "the copy is selected");
    undo(&mut h);
    assert_eq!(notes(&h), before, "one undo step");
}

#[test]
fn alt_drag_on_a_note_changes_its_velocity_and_shows_it() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    let start = velocities(&h)[2];
    let from = note_rect(&h, &before[2]).center();
    let mut shown = None;
    drag_with(&mut h, from, from - vec2(0.0, 20.0), Modifiers::ALT, &mut |h| {
        shown = h.query_by_label(&format!("Velocity {}", start + 10)).map(|_| ());
    });
    assert!(shown.is_some(), "the velocity is shown while dragging");
    let after = notes(&h);
    assert_eq!(midi_velocity(after[2].velocity), start + 20);
    assert_eq!((after[2].time, after[2].key), (before[2].time, before[2].key), "Alt+drag does not move it");
    assert_eq!(&after[..2], &before[..2]);
    undo(&mut h);
    assert_eq!(notes(&h), before);
}

#[test]
fn a_line_drawn_in_the_velocity_lane_sets_every_stem_it_spans_in_one_step() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    let (beat, _) = scale(&h);
    let lane = lane(&h);
    let x0 = note_rect(&h, &before[0]).left();
    // From an empty spot after the last note, near the top, down to just before the second note,
    // near the bottom: the line spans the second, third and fourth stems.
    let from = pos2(x0 + beat * 3.75, lane.top() + 8.0);
    let to = pos2(x0 + beat * 0.9, lane.bottom() - 8.0);
    drag(&mut h, from, to, Modifiers::NONE);
    let v = velocities(&h);
    assert_eq!(v[0], velocities_before(&before)[0], "a stem outside the line keeps its velocity");
    assert!(v[3] > v[2] && v[2] > v[1], "the line's height at each stem: {v:?}");
    assert!(v[3] > 70 && v[1] < 40, "{v:?}");
    undo(&mut h);
    assert_eq!(notes(&h), before, "the whole line is one undo step");

    // With Alt a line can start on a bar: across the first two stems, near the top.
    let from = pos2(x0 - 1.0, lane.top() + 8.0);
    let to = pos2(x0 + beat * 1.1, lane.top() + 8.0);
    drag(&mut h, from, to, Modifiers::ALT);
    let v = velocities(&h);
    assert!(v[0] > 104 && v[1] > 104, "both above the 102 they had: {v:?}");
    assert_eq!(&v[2..], &velocities_before(&before)[2..]);
}

fn velocities_before(notes: &[Note]) -> Vec<u8> {
    notes.iter().map(|n| midi_velocity(n.velocity)).collect()
}

#[test]
fn the_lane_top_edge_makes_the_lane_taller_or_shorter_within_limits() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    let height = lane(&h).height();
    let edge = h.get_by_label("Velocity lane top edge").rect().center();
    drag(&mut h, edge, edge - vec2(0.0, 40.0), Modifiers::NONE);
    let taller = lane(&h).height();
    assert!((taller - height - 40.0).abs() < 2.0, "{height} -> {taller}");
    assert!(h.state().piano_roll().lane_height().is_some());
    // Down a long way: no shorter than 24 points.
    let edge = h.get_by_label("Velocity lane top edge").rect().center();
    drag(&mut h, edge, edge + vec2(0.0, 400.0), Modifiers::NONE);
    assert!((lane(&h).height() - 24.0).abs() < 1.0, "{}", lane(&h).height());
    // Up a long way: five key rows stay above it.
    let edge = h.get_by_label("Velocity lane top edge").rect().center();
    drag(&mut h, edge, edge - vec2(0.0, 1000.0), Modifiers::NONE);
    let grid = h.get_by_label_contains("Piano roll,").rect();
    assert!(grid.height() >= 5.0 * 12.0 - 1.0, "{grid:?}");
    // It is window state: the notes are untouched.
    assert_eq!(notes(&h), before);
}

#[test]
fn audition_sounds_moved_and_drawn_notes_once_when_it_is_on() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    // Off by default: a move sounds nothing.
    let at = note_rect(&h, &before[1]).center();
    click(&mut h, at);
    h.key_press(Key::ArrowUp);
    h.step();
    assert_eq!(notes(&h)[1].key, before[1].key + 1);
    assert!(h.state().piano_roll().auditioning().is_empty());

    h.get_by_label("Audition").click();
    h.run();
    assert!(h.state().piano_roll().audition);
    // On: the arrow key's move sounds the moved note where it went, then lets it go.
    h.key_press(Key::ArrowUp);
    h.step();
    assert_eq!(h.state().piano_roll().auditioning(), [before[1].key + 2]);
    for _ in 0..10 {
        h.step();
    }
    assert!(h.state().piano_roll().auditioning().is_empty(), "the note goes off a moment later");

    // A dragged note sounds too (the scale from the first and third notes: the second is now
    // above the rows in view).
    let n = notes(&h);
    let (a, c) = (note_rect(&h, &n[0]), note_rect(&h, &n[2]));
    let beat = (c.left() - a.left()) / 2.0;
    let row = (a.top() - c.top()) / f32::from(n[2].key - n[0].key);
    let from = note_rect(&h, &notes(&h)[3]).center();
    let to = from + vec2(-beat, -row);
    h.hover_at(from);
    h.step();
    button(&mut h, from, true, Modifiers::NONE);
    for step in 1..=4 {
        h.hover_at(from + (to - from) * (step as f32 / 4.0));
        h.step();
    }
    button(&mut h, to, false, Modifiers::NONE);
    h.step();
    let moved = notes(&h)[3];
    assert_eq!((moved.time, moved.key), (before[3].time - 1.0, before[3].key + 1));
    assert_eq!(h.state().piano_roll().auditioning(), [moved.key]);
}

#[test]
fn audition_sounds_a_note_drawn_with_the_draw_tool() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    h.get_by_label("Audition").click();
    h.run();
    h.get_by_label("Draw (2)").click();
    h.run();
    let (beat, _) = scale(&h);
    // Half a beat after the third note, in its row: an empty spot.
    let at = note_rect(&h, &before[2]).left_center() + vec2(beat * 0.6, 0.0);
    h.hover_at(at);
    h.step();
    button(&mut h, at, true, Modifiers::NONE);
    button(&mut h, at, false, Modifiers::NONE);
    let after = notes(&h);
    assert_eq!(after.len(), before.len() + 1, "the click drew a note");
    assert_eq!(h.state().piano_roll().auditioning(), [before[2].key], "and it sounds");
}

/// With the audition on, a press on a note sounds it at once, and dragging it sounds each new
/// pitch while the button is still down (#939).
#[test]
fn audition_sounds_a_note_on_the_press_and_at_each_pitch_of_a_drag() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    h.state_mut().piano_roll_mut().audition = true;
    h.run();
    let first = notes(&h)[0];
    let rect = note_rect(&h, &first);
    let at = rect.center();
    h.input_mut().events.push(egui::Event::PointerMoved(at));
    h.step();
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    assert_eq!(h.state().piano_roll().auditioning(), [first.key], "on the press");
    let up = at + egui::vec2(0.0, -2.0 * rect.height());
    h.input_mut().events.push(egui::Event::PointerMoved(up));
    h.step();
    h.input_mut().events.push(egui::Event::PointerMoved(up + egui::vec2(0.0, -0.5)));
    h.step();
    assert_ne!(h.state().piano_roll().auditioning(), [first.key], "a new pitch while it is held");
    assert!(!h.state().piano_roll().auditioning().is_empty());
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: up,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
}
