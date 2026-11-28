// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's MPE handles driven as a user would (#930, `docs/spec/ui-clip-editor.md`
//! › MPE): a press on a handle is the handle's, over another note or the lane, and a drag that
//! leaves the grid still ends in one edit.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use egui::{Modifiers, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::{ContentLane, ExpressionKind, ExpressionPoint, LaneData, Note, NoteExpression, Shared};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::interaction::key_name::key_name;
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);
/// The demo clip the tests edit: on Drums, at bar 17 (beat 64).
const CLIP: &str = "Clip Beat B on Drums at bar 17";
const CLIP_START: f64 = 64.0;

/// Note A, C (key 60) from beat 0 for 2 beats, whose curves have a point at beat 1: its pitch 7 semitones
/// up (on the row of note B), its pressure at 0.5. Note B, G (key 67) from beat 0.5 for a beat, under A's pitch
/// point.
fn notes_a_and_b() -> Vec<Note> {
    let mut a = Note::new(0.0, 2.0, 60, 0.8);
    a.expression = Some(Shared::new(NoteExpression {
        pitch: vec![ExpressionPoint::new(0.0, 0.0), ExpressionPoint::new(1.0, 7.0)],
        pressure: vec![ExpressionPoint::new(0.0, 0.2), ExpressionPoint::new(1.0, 0.5)],
        ..NoteExpression::default()
    }));
    vec![a, Note::new(0.5, 1.0, 67, 0.8)]
}

/// The demo project with Beat B holding notes A and B, the clip open in the piano roll, note A
/// selected and MPE on.
fn mpe_app() -> Harness<'static, App> {
    mpe_app_with(notes_a_and_b())
}

/// [`mpe_app`] with `notes` in Beat B, the first selected.
fn mpe_app_with(notes: Vec<Note>) -> Harness<'static, App> {
    let mut project = demo_project();
    let content = project.arrangement.lanes[0].clips.iter().find(|c| c.start == CLIP_START).unwrap().content;
    let content = project.contents.iter_mut().find(|c| c.id == content).unwrap();
    content.lanes = Shared::new(vec![ContentLane { target: None, data: LaneData::Notes(notes.into()) }]);
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(project), &[]);
    app.set_piano_grid(suhina_ui::detail::piano_roll_grid::PianoGrid::from_value("1/8"));
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .with_size(SIZE)
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    // MPE on before the clip opens, so the notes are fitted to the room its lanes leave.
    // The pressure lane alone below the notes, so it has the room to be worked in.
    harness.state_mut().piano_roll_mut().expression = Some(ExpressionKind::Pitch);
    harness.state_mut().piano_roll_mut().show_mpe_lanes([false, true, false, false]);
    crate::window::open_clip(&mut harness, CLIP);
    let roll = harness.state_mut().piano_roll_mut();
    roll.selected = [0].into();
    harness.run();
    harness
}

fn notes(h: &Harness<'static, App>) -> Vec<Note> {
    let session = h.state().session().unwrap();
    let project = session.project();
    let clip = project.arrangement.lanes[0].clips.iter().find(|c| c.start == CLIP_START).unwrap();
    match &project.content(clip.content).unwrap().lanes[0].data {
        LaneData::Notes(notes) => notes.to_vec(),
        LaneData::Audio(_) => Vec::new(),
    }
}

fn curve(h: &Harness<'static, App>, kind: ExpressionKind) -> Vec<ExpressionPoint> {
    notes(h)[0].expression().map(|e| e.curve(kind).to_vec()).unwrap_or_default()
}

fn rect(h: &Harness<'static, App>, label: &str) -> Rect {
    h.get_by_label(label).rect()
}

/// Where note A's points at beat 1 are: x one beat after A's start (B's start is half a beat
/// after it), its pitch on note B's row, measured from the notes on screen.
fn beat_one(h: &Harness<'static, App>) -> (f32, f32) {
    let (a, b) =
        (rect(h, &format!("Note {} at beat 0", key_name(60))), rect(h, &format!("Note {} at beat 0.5", key_name(67))));
    let x = a.left() + 2.0 * (b.left() - a.left());
    (x, b.center().y)
}

fn selected(h: &Harness<'static, App>) -> Vec<usize> {
    h.state().piano_roll().selected.iter().copied().collect()
}

fn button(h: &mut Harness<'static, App>, pos: Pos2, pressed: bool) {
    h.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
    h.step();
}

/// Presses at `from`, moves to `to` in four steps and lets go.
fn drag(h: &mut Harness<'static, App>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.step();
    button(h, from, true);
    for step in 1..=4 {
        h.hover_at(from + (to - from) * (step as f32 / 4.0));
        h.step();
    }
    button(h, to, false);
    h.run();
}

#[test]
fn a_pitch_handle_over_another_note_is_dragged_and_selects_nothing() {
    let mut h = mpe_app();
    let before = notes(&h);
    let (x, y) = beat_one(&h);
    let row = rect(&h, &format!("Note {} at beat 0.5", key_name(67))).height();
    drag(&mut h, pos2(x, y), pos2(x, y - 2.0 * row));
    assert_eq!(selected(&h), vec![0], "the press was the handle's, not note B's");
    let after = notes(&h);
    assert_eq!(after[1], before[1], "note B did not move");
    let pitch = curve(&h, ExpressionKind::Pitch);
    assert_eq!(pitch.len(), 2);
    assert!(pitch[1].value > 7.0, "the point went up: {pitch:?}");
}

#[test]
fn a_pressure_handle_in_the_lane_is_dragged() {
    let mut h = mpe_app();
    let (x, _) = beat_one(&h);
    let lane = rect(&h, "Pressure lane");
    let y = |value: f32| lane.bottom() - 2.0 - (lane.height() - 4.0) * value;
    drag(&mut h, pos2(x, y(0.5)), pos2(x, y(0.8)));
    let pressure = curve(&h, ExpressionKind::Pressure);
    assert_eq!(pressure.len(), 2, "moved, not a point added: {pressure:?}");
    assert!((pressure[1].value - 0.8).abs() < 0.02, "{pressure:?}");
    assert_eq!(pressure[1].offset, 1.0);
}

#[test]
fn a_handle_dragged_out_of_the_grid_still_ends_in_one_edit() {
    let mut h = mpe_app();
    let (x, y) = beat_one(&h);
    let grid_top = rect(&h, &format!("Note {} at beat 0", key_name(60))).top().min(y) - 2000.0;
    drag(&mut h, pos2(x, y), pos2(x, grid_top.max(1.0)));
    let pitch = curve(&h, ExpressionKind::Pitch);
    assert!(pitch[1].value > 7.0, "the release outside the grid sent the edit: {pitch:?}");
    // The drag is over: a click on note B selects it as usual.
    let b = rect(&h, &format!("Note {} at beat 0.5", key_name(67))).center() + vec2(0.0, 0.0);
    h.hover_at(b);
    h.step();
    button(&mut h, b, true);
    button(&mut h, b, false);
    h.run();
    assert_eq!(selected(&h), vec![1]);
}

/// Note A bent two semitones up by beat 1, onto the row of note B (D, key 62, from beat 0.5 for a
/// beat): rows close enough that both are well inside the grid, so a drag over them is not one
/// held at its edge, which scrolls the view.
fn bent_app() -> Harness<'static, App> {
    let mut a = Note::new(0.0, 2.0, 60, 0.8);
    a.expression = Some(Shared::new(NoteExpression {
        pitch: vec![ExpressionPoint::new(0.0, 0.0), ExpressionPoint::new(1.0, 2.0)],
        ..NoteExpression::default()
    }));
    mpe_app_with(vec![a, Note::new(0.5, 1.0, 62, 0.8)])
}

/// Note B's row in [`bent_app`].
fn row_b(h: &Harness<'static, App>) -> Rect {
    rect(h, &format!("Note {} at beat 0.5", key_name(62)))
}

/// The x of `beat` in note A's time in [`bent_app`].
fn beat_x(h: &Harness<'static, App>, beat: f32) -> f32 {
    let (a, b) = (rect(h, &format!("Note {} at beat 0", key_name(60))), row_b(h));
    a.left() + beat * 2.0 * (b.left() - a.left())
}

#[test]
fn a_bent_notes_band_is_dragged_as_the_note() {
    // Bent notes are the default: note A is drawn along its curve, on note B's row after beat 1.
    let mut h = bent_app();
    let before = curve(&h, ExpressionKind::Pitch);
    let y = row_b(&h).center().y;
    let (from, to) = (beat_x(&h, 1.75), beat_x(&h, 2.75));
    drag(&mut h, pos2(from, y), pos2(to, y));
    let a = &notes(&h)[0];
    assert_eq!(a.key, 60, "the note kept its key");
    assert!((a.time - 1.0).abs() < 1e-6, "the band moved the note a beat: {}", a.time);
    assert_eq!(curve(&h, ExpressionKind::Pitch), before, "its bend went with it");
}

#[test]
fn a_bend_handle_curves_the_bend() {
    let mut h = bent_app();
    let (low, high) = (rect(&h, &format!("Note {} at beat 0", key_name(60))).center().y, row_b(&h).center().y);
    let middle = pos2(beat_x(&h, 0.5), (low + high) / 2.0);
    drag(&mut h, middle, middle - vec2(0.0, 25.0));
    let pitch = curve(&h, ExpressionKind::Pitch);
    assert_eq!(pitch.len(), 2, "no point added: {pitch:?}");
    assert!(pitch[0].curve > 0.2, "the bend bulged up: {pitch:?}");
    assert_eq!(notes(&h)[0].time, 0.0, "the note did not move");
}

#[test]
fn with_bends_drawn_as_lines_the_bend_is_not_the_note() {
    let mut h = bent_app();
    let settings = suhina_ui::settings::Settings { bent_notes: false, ..h.state().settings().clone() };
    let ctx = h.ctx.clone();
    h.state_mut().set_settings(&ctx, settings);
    h.run();
    let y = row_b(&h).center().y;
    let (from, to) = (beat_x(&h, 1.75), beat_x(&h, 2.75));
    drag(&mut h, pos2(from, y), pos2(to, y));
    assert_eq!(notes(&h)[0].time, 0.0, "note B's row after B is empty grid in line mode");
}

#[test]
fn a_kept_lfos_end_is_dragged_and_its_bar_picks_it() {
    // Note A bent as in `bent_app`, keeping a sine over its whole two beats.
    let mut a = Note::new(0.0, 2.0, 60, 0.8);
    let lfo = suhina_engine::model::Lfo {
        wave: suhina_engine::model::LfoWave::Sine,
        shape: 0.0,
        period: 0.25,
        shift: 0.0,
        amount: 0.5,
        base: 0.0,
        attack: 0.0,
        decay: 0.0,
        seed: 1,
    };
    a.expression = Some(Shared::new(NoteExpression {
        pitch: vec![ExpressionPoint::new(0.0, 0.0), ExpressionPoint::new(1.0, 2.0)],
        lfos: vec![suhina_engine::model::NoteLfo::new(ExpressionKind::Pitch, (0.0, 2.0), lfo)],
        ..NoteExpression::default()
    }));
    let mut h = mpe_app_with(vec![a, Note::new(0.5, 1.0, 62, 0.8)]);
    let end = rect(&h, "LFO end").center();
    let half_beat = beat_x(&h, 0.5) - beat_x(&h, 0.0);
    drag(&mut h, end, end - vec2(half_beat, 0.0));
    let kept = notes(&h)[0].expression().unwrap().lfos.clone();
    assert_eq!(kept.len(), 1);
    assert!((kept[0].to - 1.5).abs() < 1e-6, "the end moved half a beat earlier: {:?}", kept[0]);
    assert_eq!(kept[0].from, 0.0);
    assert_eq!(h.state().piano_roll().mpe_kept(), Some(0), "the drag picked it");
}
