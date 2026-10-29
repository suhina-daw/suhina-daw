// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's workflow modes and its double-click (`docs/spec/ui-clip-editor.md` › Workflow
//! modes, › Double-click): the toolbar's workflow menu, the Custom mode, and what a double-click
//! does in every part of the editor with every tool.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers, Pos2, Rect, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::{LaneData, Note};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::detail::piano_roll::key_name;
use suhina_ui::settings::Settings;
use suhina_ui::settings::keymap::shown;
use suhina_ui::settings::keymap::{Keymap, Preset};
use suhina_ui::settings::workflow::{Setting, Workflow};
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
        LaneData::Notes(notes) => notes.clone(),
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

fn pointer(h: &mut Harness<'static, App>, pos: Pos2, pressed: bool) {
    h.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
    h.step();
}

/// `count` clicks at `pos`, a frame apart: two are a double-click. They come right after the
/// clicks that opened the clip, which is how a quick hand does it: the piano roll counts its own.
fn clicks(h: &mut Harness<'static, App>, pos: Pos2, count: usize) {
    h.hover_at(pos);
    h.step();
    for _ in 0..count {
        pointer(h, pos, true);
        pointer(h, pos, false);
    }
    h.run();
}

fn double_click(h: &mut Harness<'static, App>, pos: Pos2) {
    clicks(h, pos, 2);
}

/// Waits long enough that the next click is not part of a double-click.
fn pause(h: &mut Harness<'static, App>) {
    for _ in 0..10 {
        h.step();
    }
}

fn undo(h: &mut Harness<'static, App>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
}

/// Chooses a tool from the toolbar by its name; its label is the name and its key ("Mute (6)").
fn tool(h: &mut Harness<'static, App>, name: &str) {
    let label = ["1", "2", "3", "4", "5", "6"]
        .iter()
        .map(|key| format!("{name} ({key})"))
        .find(|label| h.query_by_label(label).is_some())
        .expect("a tool button with that name");
    h.get_by_label(&label).click();
    h.run();
}

fn selected(h: &Harness<'static, App>) -> usize {
    h.state().piano_roll().selected.len()
}

// ---------------------------------------------------------------------------------------------
// Double-click in the grid.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_double_click_on_empty_grid_adds_one_note_even_right_after_another_click() {
    for preset in Preset::ALL {
        let mut renderer = engine();
        let mut h = with_preset(&mut renderer, preset);
        let before = notes(&h);
        // Right after the clicks that opened the clip: the pair is still a double-click.
        let at = empty_spot(&h);
        double_click(&mut h, at);
        let after = notes(&h);
        assert_eq!(after.len(), before.len() + 1, "{preset:?} adds one note: {after:?}");
        assert_eq!(selected(&h), 1, "{preset:?} selects it");
        // One undo step takes it back, whatever the preset (Mouse-first's first click drew it).
        undo(&mut h);
        assert_eq!(notes(&h), before, "{preset:?}");
    }
}

#[test]
fn a_double_click_on_a_note_or_its_edge_deletes_it_in_select() {
    for edge in [false, true] {
        let mut renderer = engine();
        let mut h = with_preset(&mut renderer, Preset::SelectFirst);
        let before = notes(&h);
        let rect = note_rect(&h, &before[0]);
        let at = if edge { pos2(rect.right() - 1.5, rect.center().y) } else { rect.center() };
        double_click(&mut h, at);
        let after = notes(&h);
        assert_eq!(after.len(), before.len() - 1, "edge {edge}");
        assert!(!after.contains(&before[0]), "the note under the pointer went");
        undo(&mut h);
        assert_eq!(notes(&h), before);
    }
}

#[test]
fn a_third_quick_click_starts_a_new_pair_rather_than_undoing_the_double_click() {
    let mut renderer = engine();
    let mut h = with_preset(&mut renderer, Preset::SelectFirst);
    let before = notes(&h);
    let at = empty_spot(&h);
    clicks(&mut h, at, 3);
    let after = notes(&h);
    assert_eq!(after.len(), before.len() + 1, "the pair added a note; the third click only selects it");
    assert_eq!(selected(&h), 1);
}

#[test]
fn draw_and_paint_add_one_note_on_a_double_click() {
    for name in ["Draw", "Paint"] {
        let mut renderer = engine();
        let mut h = with_preset(&mut renderer, Preset::SelectFirst);
        tool(&mut h, name);
        let before = notes(&h);
        let at = empty_spot(&h);
        double_click(&mut h, at);
        assert_eq!(notes(&h).len(), before.len() + 1, "{name}");
        assert_eq!(selected(&h), 1, "{name}");
        // On a note, a double-click selects it and deletes nothing.
        pause(&mut h);
        let at = note_rect(&h, &before[0]).center();
        double_click(&mut h, at);
        assert_eq!(notes(&h).len(), before.len() + 1, "{name} deletes nothing");
    }
}

#[test]
fn erase_deletes_one_note_and_mute_mutes_once_on_a_double_click() {
    let mut renderer = engine();
    let mut h = with_preset(&mut renderer, Preset::SelectFirst);
    tool(&mut h, "Erase");
    let before = notes(&h);
    let at = note_rect(&h, &before[0]).center();
    double_click(&mut h, at);
    assert_eq!(notes(&h).len(), before.len() - 1, "one note, not two");
    undo(&mut h);
    assert_eq!(notes(&h), before);

    pause(&mut h);
    tool(&mut h, "Mute");
    let at = note_rect(&h, &before[0]).center();
    double_click(&mut h, at);
    let muted = notes(&h);
    assert!(muted[0].muted, "the double-click muted the note and did not flip it back");
    assert_eq!(muted.iter().filter(|n| n.muted).count(), 1);
    undo(&mut h);
    assert_eq!(notes(&h), before, "one undo step");
}

#[test]
fn slice_does_nothing_on_a_double_click() {
    let mut renderer = engine();
    let mut h = with_preset(&mut renderer, Preset::SelectFirst);
    tool(&mut h, "Slice");
    let before = notes(&h);
    let at = empty_spot(&h);
    double_click(&mut h, at);
    let at = note_rect(&h, &before[0]).center();
    double_click(&mut h, at);
    assert_eq!(notes(&h), before);
}

#[test]
fn a_double_click_in_the_velocity_lane_or_the_keys_edits_nothing() {
    let mut renderer = engine();
    let mut h = with_preset(&mut renderer, Preset::SelectFirst);
    let before = notes(&h);
    let first = before[0];
    let velocity = suhina_session::midi_velocity(first.velocity);
    let stem = format!("Velocity of note {} at beat {}: {velocity}", key_name(first.key), first.time);
    let at = h.get_by_label(&stem).rect().center();
    double_click(&mut h, at);
    assert_eq!(notes(&h), before, "the lane edits nothing");
    assert_eq!(selected(&h), 1, "it selects the stem's note");
    pause(&mut h);
    let grid = h.get_by_label(&format!("Piano roll, {} notes", before.len())).rect();
    let key = pos2(grid.left() - 18.0, note_rect(&h, &first).center().y);
    let at = key;
    double_click(&mut h, at);
    assert_eq!(notes(&h), before, "the keys edit nothing");
    assert_eq!(h.state().piano_roll().playing_key(), None, "and nothing sounds once let go");
}

#[test]
fn the_double_click_settings_turn_adding_and_deleting_off() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let mut keymap = Keymap::default();
    keymap.set_workflow(Setting::DoubleClickEmpty, 1);
    keymap.set_workflow(Setting::DoubleClickNote, 1);
    h.state_mut().set_keymap(keymap);
    h.run();
    let before = notes(&h);
    let at = empty_spot(&h);
    double_click(&mut h, at);
    assert_eq!(notes(&h), before, "no note added");
    assert_eq!(selected(&h), 0, "the clicks cleared the selection");
    pause(&mut h);
    let at = note_rect(&h, &before[0]).center();
    double_click(&mut h, at);
    assert_eq!(notes(&h), before, "no note deleted");
    assert_eq!(selected(&h), 1, "the note is selected, as a click does");
}

// ---------------------------------------------------------------------------------------------
// The workflow menu in the toolbar.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_toolbar_menu_switches_the_workflow_and_a_setting_makes_it_custom() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    assert!(h.query_by_label("Workflow Select-first").is_some(), "the menu shows the mode");
    h.get_by_label("Workflow Select-first").click();
    h.run();
    h.get_by_label("Mouse-first").click();
    h.run();
    assert_eq!(h.state().keymap().preset(), Preset::MouseFirst);
    assert_eq!(h.state().keymap().workflow(), Workflow::MOUSE_FIRST, "the preset filled in its settings");
    assert!(h.state_mut().take_changed_keymap().is_some(), "the window saves the keymap");
    assert!(h.query_by_label("Workflow Mouse-first").is_some());

    // One setting changed in the same menu: the mode is Custom, the rest stay Mouse-first's.
    h.get_by_label("Workflow Mouse-first").click();
    h.run();
    h.get_by_label_contains("Double-click on a note: Deletes it").click();
    h.run();
    h.get_by_label("Double-click on a note: Does nothing").click();
    h.run();
    let keymap = h.state().keymap().clone();
    assert_eq!(keymap.preset(), Preset::Custom);
    assert!(!keymap.workflow().double_click_deletes);
    assert!(keymap.workflow().empty_click_draws, "the other settings are kept");
    assert!(h.query_by_label("Workflow Custom").is_some());
    // The choice is kept with the keys.
    let (again, warnings) = Keymap::from_json(&keymap.to_json());
    assert_eq!((again, warnings.len()), (keymap, 0));

    // And it works at once: a double-click on a note deletes nothing now.
    pause(&mut h);
    let before = notes(&h);
    let at = note_rect(&h, &before[0]).center();
    double_click(&mut h, at);
    assert_eq!(notes(&h), before);
}

#[test]
fn the_duplicate_setting_in_the_menu_moves_the_key() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    h.get_by_label("Workflow Select-first").click();
    h.run();
    h.get_by_label_contains("Duplicate key: Ctrl+D").click();
    h.run();
    h.get_by_label("Duplicate key: Ctrl+B").click();
    h.run();
    let keymap = h.state().keymap();
    assert_eq!(keymap.preset(), Preset::Custom);
    assert_eq!(keymap.binding_text("selection.duplicate").as_deref(), Some("Ctrl+B"));
    assert_eq!(keymap.binding_text("view.browser").as_deref(), Some("Ctrl+Shift+B"));
    // The toolbar's Duplicate button shows the key it has now.
    assert!(h.query_by_label(&format!("Duplicate ({})", shown("Ctrl+B"))).is_some());
}

#[test]
fn settings_clip_editor_shows_the_workflow_rows() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run();
    h.get_by_label("Settings page: Keys").click();
    h.run();
    // The workflow rows are on the Keys page (scrolling a long page: settings_window.rs).
    let last = "Double-click on a note: Nothing";
    let rect = h.get_by_label(last).rect();
    assert!(rect.bottom() <= SIZE.1, "the last row is in view: {rect:?}");
    h.get_by_label(last).click();
    h.run();
    assert!(!h.state().keymap().workflow().double_click_deletes, "and its row works there");
    assert_eq!(h.state().keymap().preset(), Preset::Custom);
}

/// Renders the piano roll with its workflow menu open, and Settings › Clip editor with the workflow
/// rows, to `target/ui-snapshots/`, for looking at:
/// `cargo test -p suhina-ui --test piano_roll_workflow -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_workflow_menu_and_settings() {
    let mut renderer = engine();
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
    let mut h = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(0.05)
        .with_max_steps(40)
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&h.ctx);
    h.run();
    h.get_by_label("Arrangement view").click();
    h.run();
    h.get_by_label(CLIP).click();
    h.run();
    h.get_by_label("Workflow Select-first").click();
    h.run();
    h.get_by_label_contains("Double-click on a note: Deletes it").hover();
    h.run();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&out).unwrap();
    h.render().unwrap().save(out.join("workflow-menu.png")).unwrap();
    h.key_press(Key::Escape);
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run();
    h.get_by_label("Settings page: Keys").click();
    h.run();
    h.render().unwrap().save(out.join("workflow-settings.png")).unwrap();
}
