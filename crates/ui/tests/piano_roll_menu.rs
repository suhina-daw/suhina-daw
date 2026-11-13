// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's right-click menu (`docs/spec/ui-clip-editor.md` › Right-click menu): when a
//! right click opens it and when the right button keeps its other jobs, what a right click on a
//! note selects, and what its items send.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use egui::{Modifiers, Pos2, Rect, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::{LaneData, Note};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::interaction::key_name::key_name;
use suhina_ui::settings::Settings;
use suhina_ui::settings::keymap::{Keymap, Preset};
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);
/// The demo clip the tests edit: on Drums, at bar 17 (beat 64).
const CLIP: &str = "Clip Beat B on Drums at bar 17";
const CLIP_START: f64 = 64.0;

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
    suhina_engine::Renderer::stopped(
        &demo_project(),
        &suhina_engine::AudioPool::new(),
        48_000,
        suhina_session::devices(),
    )
    .unwrap()
}

/// The demo project's piano roll with `preset`'s workflow.
fn with_preset(renderer: &mut suhina_engine::Renderer, preset: Preset) -> Harness<'static, App> {
    let mut h = piano_app(renderer);
    let mut keymap = Keymap::default();
    keymap.set_preset(preset);
    h.state_mut().set_keymap(keymap);
    h.run();
    h
}

/// The edited clip's notes in the project.
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
    let muted = if note.muted { " (muted)" } else { "" };
    h.get_by_label(&format!("Note {} at beat {}{muted}", key_name(note.key), note.time)).rect()
}

/// Where beat `time` of key `key` is on screen, measured from two notes of the clip.
fn spot(h: &Harness<'static, App>, time: f64, key: u8) -> Pos2 {
    let n = notes(h);
    let (a, b) = (note_rect(h, &n[0]), note_rect(h, &n[1]));
    let beat = (b.left() - a.left()) / (n[1].time - n[0].time) as f32;
    let row = (a.top() - b.top()) / (f32::from(n[1].key) - f32::from(n[0].key));
    pos2(a.left() + (time - n[0].time) as f32 * beat + 2.0, a.center().y - (f32::from(key) - f32::from(n[0].key)) * row)
}

/// An empty spot of the grid: a key row with no note, half a beat after the second note.
fn empty_spot(h: &Harness<'static, App>) -> Pos2 {
    let n = notes(h);
    let (low, high) = (n.iter().map(|n| n.key).min().unwrap(), n.iter().map(|n| n.key).max().unwrap());
    let key = (low..=high).find(|k| n.iter().all(|n| n.key != *k)).unwrap_or(low - 1);
    spot(h, n[1].time + 0.5, key)
}

fn press(h: &mut Harness<'static, App>, pos: Pos2, button: egui::PointerButton, pressed: bool) {
    h.input_mut().events.push(egui::Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE });
    h.step();
}

/// Waits long enough that the next click is not part of a double-click.
fn pause(h: &mut Harness<'static, App>) {
    for _ in 0..10 {
        h.step();
    }
}

/// A right click at `pos`: pressed and let go without moving.
fn right_click(h: &mut Harness<'static, App>, pos: Pos2) {
    h.hover_at(pos);
    h.step();
    press(h, pos, egui::PointerButton::Secondary, true);
    press(h, pos, egui::PointerButton::Secondary, false);
    h.run();
}

fn menu_open(h: &Harness<'static, App>) -> bool {
    h.query_by_label("Invert selection").is_some()
}

/// The item `name` of the open menu, picked.
fn item(h: &mut Harness<'static, App>, name: &str) {
    h.get_by_label(name).click();
    h.run();
}

/// Bass's first note, drawn faintly behind the edited clip's first beat.
const BASS_GHOST: &str = "Ghost note E1 at beat 0 from Bass";

#[test]
fn a_right_click_on_the_empty_grid_opens_the_menu_with_select_first() {
    let mut renderer = engine();
    let mut h = with_preset(&mut renderer, Preset::SelectFirst);
    let empty = empty_spot(&h);
    right_click(&mut h, empty);
    assert!(menu_open(&h), "the menu opens where the right button does nothing else");
    assert!(h.query_by_label_contains("Snap to grid").is_some(), "the grid's entries are in the menu itself");
    assert!(h.query_by_label("Zoom to clip").is_none(), "Z and Shift+Z zoom; the menu does not");
    assert!(h.query_by_label("Set loop end here").is_some());
}

#[test]
fn a_right_click_on_a_note_deletes_it_with_mouse_first_and_opens_no_menu() {
    let mut renderer = engine();
    let mut h = with_preset(&mut renderer, Preset::MouseFirst);
    let before = notes(&h);
    let on = note_rect(&h, &before[0]).center();
    right_click(&mut h, on);
    assert!(!menu_open(&h));
    assert_eq!(notes(&h).len(), before.len() - 1, "the right button goes on deleting");
    pause(&mut h);
    // From the empty grid it opens.
    let empty = empty_spot(&h);
    right_click(&mut h, empty);
    assert!(menu_open(&h));
}

#[test]
fn a_right_click_on_an_unselected_note_selects_it_alone() {
    let mut renderer = engine();
    let mut h = with_preset(&mut renderer, Preset::SelectFirst);
    let n = notes(&h);
    let on = note_rect(&h, &n[1]).center();
    right_click(&mut h, on);
    assert!(menu_open(&h));
    assert_eq!(h.state().piano_roll().selected.iter().copied().collect::<Vec<_>>(), vec![1]);
}

#[test]
fn invert_selection_selects_the_notes_that_were_not_selected() {
    let mut renderer = engine();
    let mut h = with_preset(&mut renderer, Preset::SelectFirst);
    let n = notes(&h);
    let on = note_rect(&h, &n[0]).center();
    right_click(&mut h, on);
    item(&mut h, "Invert selection");
    let selected: Vec<usize> = h.state().piano_roll().selected.iter().copied().collect();
    assert_eq!(selected, (1..n.len()).collect::<Vec<_>>());
    assert!(!menu_open(&h), "a pick closes the menu");
}

#[test]
fn the_items_send_their_commands() {
    let mut renderer = engine();
    let mut h = with_preset(&mut renderer, Preset::SelectFirst);
    let n = notes(&h);
    let on = note_rect(&h, &n[0]).center();
    right_click(&mut h, on);
    item(&mut h, "Mute");
    assert!(notes(&h)[0].muted, "Mute: note.setMuted on the clicked note");
    pause(&mut h);
    let on = note_rect(&h, &notes(&h)[0]).center();
    right_click(&mut h, on);
    item(&mut h, "Unmute");
    assert!(!notes(&h)[0].muted);
    pause(&mut h);
    let on = note_rect(&h, &notes(&h)[0]).center();
    right_click(&mut h, on);
    item(&mut h, "Delete");
    assert_eq!(notes(&h).len(), n.len() - 1, "Delete: note.delete");
    pause(&mut h);
    let empty = empty_spot(&h);
    right_click(&mut h, empty);
    item(&mut h, "Select all");
    assert_eq!(h.state().piano_roll().selected.len(), n.len() - 1);
}

#[test]
fn two_quick_right_clicks_on_a_ghost_note_open_its_clip_and_no_menu() {
    let mut renderer = engine();
    let mut h = with_preset(&mut renderer, Preset::SelectFirst);
    let ghost = h.get_by_label(BASS_GHOST).rect().center();
    h.hover_at(ghost);
    h.step();
    for _ in 0..2 {
        press(&mut h, ghost, egui::PointerButton::Secondary, true);
        press(&mut h, ghost, egui::PointerButton::Secondary, false);
    }
    pause(&mut h);
    h.run();
    assert!(!menu_open(&h), "the double right click goes into the clip");
    assert!(!h.state().piano_roll().menu_open());
}

#[test]
fn one_right_click_on_a_ghost_note_opens_the_menu_after_the_double_click_time() {
    let mut renderer = engine();
    let mut h = with_preset(&mut renderer, Preset::SelectFirst);
    let ghost = h.get_by_label(BASS_GHOST).rect().center();
    h.hover_at(ghost);
    h.step();
    press(&mut h, ghost, egui::PointerButton::Secondary, true);
    press(&mut h, ghost, egui::PointerButton::Secondary, false);
    assert!(!menu_open(&h), "it waits for a second click");
    pause(&mut h);
    h.run();
    assert!(menu_open(&h));
}
