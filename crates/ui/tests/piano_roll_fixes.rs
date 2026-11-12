// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fixes found by trying the piano roll package's build: the playhead in the piano roll, the
//! clipboard keys as the real window sends them, the right button and the wheel never scrolling,
//! and dragging a velocity bar (`docs/spec/ui-clip-editor.md`).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use egui::{Modifiers, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use suhina_engine::model::{LaneData, Note};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::detail::piano_roll::Tool;
use suhina_ui::interaction::key_name::key_name;
use suhina_ui::kit::widgets::Action;
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);
/// The demo clip the tests edit: on Drums, at bar 17 (beat 64), a one-bar loop.
const CLIP: &str = "Clip Beat B on Drums at bar 17";
const CLIP_START: f64 = 64.0;

/// The demo project with `renderer` as its engine, in the Arrangement view, the clip selected.
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

/// Renders a block, so the engine takes the commands sent so far.
fn render_a_block(renderer: &mut suhina_engine::Renderer) {
    renderer.process(&mut [0.0f32; 2 * 64]);
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

fn note_rect(h: &Harness<'static, App>, note: &Note) -> Rect {
    h.get_by_label(&format!("Note {} at beat {}", key_name(note.key), note.time)).rect()
}

fn button(h: &mut Harness<'static, App>, pos: Pos2, pressed: bool, which: egui::PointerButton) {
    h.input_mut().events.push(egui::Event::PointerButton { pos, button: which, pressed, modifiers: Modifiers::NONE });
    h.step();
}

fn click(h: &mut Harness<'static, App>, pos: Pos2) {
    h.hover_at(pos);
    h.step();
    button(h, pos, true, egui::PointerButton::Primary);
    button(h, pos, false, egui::PointerButton::Primary);
    h.run();
}

fn drag(h: &mut Harness<'static, App>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.step();
    button(h, from, true, egui::PointerButton::Primary);
    for step in 1..=5 {
        h.hover_at(from + (to - from) * (step as f32 / 5.0));
        h.step();
    }
    button(h, to, false, egui::PointerButton::Primary);
    h.run();
}

/// The beat the piano roll's playhead shows, if it shows one.
fn playhead(h: &Harness<'static, App>) -> Option<f64> {
    let label = h.query_by_label_contains("Playhead at beat")?.accesskit_node().label()?;
    label.trim_start_matches("Playhead at beat ").parse().ok()
}

#[test]
fn the_piano_roll_shows_the_playhead_in_the_clip_while_the_transport_runs() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    assert_eq!(playhead(&h), None, "no playhead while stopped");
    // Two beats into the clip: the playhead is at content beat 2.
    h.state_mut().run(&Action::new("transport.setPosition", json!({ "beats": CLIP_START + 2.0 })));
    h.state_mut().run(&Action::new("transport.play", json!({})));
    render_a_block(&mut renderer);
    h.run_steps(3);
    let at = playhead(&h).expect("a playhead while the transport runs");
    assert!((2.0..2.1).contains(&at), "{at}");
    // Five beats in, the one-bar loop has come round: content beat 1.
    h.state_mut().run(&Action::new("transport.setPosition", json!({ "beats": CLIP_START + 5.0 })));
    render_a_block(&mut renderer);
    h.run_steps(3);
    let at = playhead(&h).expect("a playhead in the loop's second pass");
    assert!((1.0..1.1).contains(&at), "{at}");
    // Before the clip, nothing; stopped, nothing.
    h.state_mut().run(&Action::new("transport.setPosition", json!({ "beats": 8.0 })));
    render_a_block(&mut renderer);
    h.run_steps(3);
    assert_eq!(playhead(&h), None);
    h.state_mut().run(&Action::new("transport.stop", json!({})));
    render_a_block(&mut renderer);
    h.run_steps(3);
    assert_eq!(playhead(&h), None);
}

/// Sends what the real window sends for Ctrl+X, Ctrl+C or Ctrl+V: a cut, copy or paste event,
/// not a key.
fn clipboard(h: &mut Harness<'static, App>, event: egui::Event) {
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::COMMAND));
    h.input_mut().events.push(event);
    h.step();
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run();
}

#[test]
fn ctrl_x_c_and_v_reach_the_piano_roll_as_the_window_sends_them() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let before = notes(&h);
    // Select a note with the mouse, then cut: it goes.
    let at = note_rect(&h, &before[1]).center();
    click(&mut h, at);
    clipboard(&mut h, egui::Event::Cut);
    assert_eq!(notes(&h).len(), before.len() - 1, "Ctrl+X cut the selected note");
    // Paste brings it back (a cut put a line on the system clipboard, so the window sends a
    // paste event for Ctrl+V).
    clipboard(&mut h, egui::Event::Paste("Suhina: notes copied".into()));
    assert_eq!(notes(&h).len(), before.len(), "Ctrl+V pasted it");
    // Copy and paste: one more.
    let at = note_rect(&h, &before[0]).center();
    click(&mut h, at);
    clipboard(&mut h, egui::Event::Copy);
    clipboard(&mut h, egui::Event::Paste("Suhina: notes copied".into()));
    assert_eq!(notes(&h).len(), before.len() + 1, "Ctrl+C and Ctrl+V added a copy");
}

#[test]
fn the_right_button_and_the_wheel_stop_at_the_first_and_last_tool_and_never_scroll() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let grid = h.get_by_label_contains("Piano roll,").rect();
    let at = grid.center();
    h.hover_at(at);
    h.step();
    let scroll = h.state().piano_roll().scroll;
    button(&mut h, at, true, egui::PointerButton::Secondary);
    let wheel = |h: &mut Harness<'static, App>, delta: f32| {
        h.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, delta),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
        });
        h.step();
    };
    // Back from the first tool: it stays the first.
    wheel(&mut h, -60.0);
    assert_eq!(h.state().piano_roll().tool, Tool::Select);
    // Far forward: it stops at the last tool (Mute, the sixth), it does not come round to the
    // first.
    for _ in 0..10 {
        wheel(&mut h, 60.0);
    }
    assert_eq!(h.state().piano_roll().tool, Tool::Mute);
    wheel(&mut h, -60.0);
    assert_eq!(h.state().piano_roll().tool, Tool::Slice);
    // Let go, and let the wheel's smoothing run out: nothing has scrolled.
    button(&mut h, at, false, egui::PointerButton::Secondary);
    for _ in 0..20 {
        h.step();
    }
    assert_eq!(h.state().piano_roll().scroll, scroll, "the wheel scrolled nothing");
    // After that, the wheel scrolls again.
    wheel(&mut h, -60.0);
    h.run();
    assert_ne!(h.state().piano_roll().scroll, scroll, "the wheel scrolls once the button is up");
}

#[test]
fn dragging_a_velocity_bar_sets_its_notes_velocity() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let velocity = |h: &Harness<'static, App>, i: usize| suhina_session::midi_velocity(notes(h)[i].velocity);
    let before = notes(&h);
    let stem = h
        .get_by_label_contains(&format!("Velocity of note {} at beat {}", key_name(before[1].key), before[1].time))
        .rect();
    // Grab the bar over the middle of its note, away from the thin stem, and pull it to the top.
    let x = note_rect(&h, &before[1]).center().x;
    drag(&mut h, pos2(x, stem.bottom() - 6.0), pos2(x, stem.top() + 1.0));
    assert!(velocity(&h, 1) >= 120, "{}", velocity(&h, 1));
    assert_eq!(notes(&h)[0], before[0], "the other notes are left alone");
    // Down near the bottom: a low velocity; one undo step takes it back to the top.
    drag(&mut h, pos2(x, stem.top() + 4.0), pos2(x, stem.bottom() - 3.0));
    assert!(velocity(&h, 1) <= 10, "{}", velocity(&h, 1));
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::Z);
    h.run();
    assert!(velocity(&h, 1) >= 120);
}
