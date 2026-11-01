// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's second step in the window (`docs/spec/ui-clip-editor.md`): pulled to the top
//! by its splitter, Alt and the wheel zooming key rows, the toolbar's tools (Select, Draw, Paint,
//! Erase) and its own grid value.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use suhina_engine::model::{LaneData, Note};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::detail::piano_roll::{Tool, key_name};
use suhina_ui::kit::widgets::Action;
use suhina_ui::settings::keymap::shown;
use suhina_ui::settings::keymap::{Keymap, Preset};
use suhina_ui::settings::{PianoRollTop, Settings};
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);
/// The demo clip the tests edit: on Drums, at bar 17 (beat 64).
const CLIP: &str = "Clip Beat B on Drums at bar 17";
const CLIP_START: f64 = 64.0;

/// The demo project in the Arrangement view with the clip selected, so the piano roll shows it;
/// `settings` as Settings has them.
fn piano_app(renderer: &mut suhina_engine::Renderer, settings: Settings) -> Harness<'static, App> {
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

fn note_rect(h: &Harness<'static, App>, note: &Note) -> Rect {
    h.get_by_label(&format!("Note {} at beat {}", key_name(note.key), note.time)).rect()
}

/// Where beat `time` of key `key` is on screen, measured from two notes.
fn spot(h: &Harness<'static, App>, time: f64, key: u8) -> Pos2 {
    let n = notes(h);
    let (a, b) = (note_rect(h, &n[0]), note_rect(h, &n[1]));
    let beat = (b.left() - a.left()) / (n[1].time - n[0].time) as f32;
    let row = (a.top() - b.top()) / (f32::from(n[1].key) - f32::from(n[0].key));
    pos2(a.left() + (time - n[0].time) as f32 * beat + 2.0, a.center().y - (f32::from(key) - f32::from(n[0].key)) * row)
}

/// An empty key row in view: one between the clip's notes, or just under the lowest.
fn empty_key(h: &Harness<'static, App>) -> u8 {
    let n = notes(h);
    let (low, high) = (n.iter().map(|n| n.key).min().unwrap(), n.iter().map(|n| n.key).max().unwrap());
    (low..=high).find(|k| n.iter().all(|n| n.key != *k)).unwrap_or(low - 1)
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

fn click(h: &mut Harness<'static, App>, pos: Pos2) {
    h.hover_at(pos);
    h.step();
    button(h, pos, true);
    button(h, pos, false);
    h.run();
}

/// Presses at `from`, moves to `to` in `steps` frames and lets go.
fn drag(h: &mut Harness<'static, App>, from: Pos2, to: Pos2, steps: usize) {
    h.hover_at(from);
    h.step();
    button(h, from, true);
    for step in 1..=steps {
        h.hover_at(from + (to - from) * (step as f32 / steps as f32));
        h.step();
    }
    button(h, to, false);
    h.run();
}

fn undo(h: &mut Harness<'static, App>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
}

/// Chooses a tool from the toolbar: by name, or by the key it has ("Draw", "Draw (2)").
fn tool(h: &mut Harness<'static, App>, name: &str) {
    // The toolbar's tool buttons are icons; their label is the tool's name and key ("Draw (2)",
    // "Select (1)") and their tooltip the same.
    let keys = ["1", "2", "3", "4", "5"];
    let label = keys
        .iter()
        .map(|key| format!("{name} ({key})"))
        .find(|label| h.query_by_label(label).is_some())
        .expect("a tool button with that name");
    h.get_by_label(&label).click();
    h.run();
}

fn splitter(h: &Harness<'static, App>) -> Rect {
    h.get_by_label("Detail area splitter").rect()
}

/// Drags the detail area's splitter to `y`.
fn pull_to(h: &mut Harness<'static, App>, y: f32) {
    let from = splitter(h).center();
    drag(h, from, pos2(from.x, y), 6);
}

#[test]
fn pulled_to_the_top_the_view_shows_only_the_edited_track() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let start = splitter(&h).top();
    assert!(h.query_all_by_label_contains(" on Bass at bar").next().is_some(), "the other tracks show first");

    pull_to(&mut h, 40.0);
    // Above the piano roll: the ruler and the edited track's lane, nothing else.
    assert!(h.query_by_label(CLIP).is_some(), "the edited clip stays in view");
    assert!(h.query_all_by_label_contains(" on Bass at bar").next().is_none(), "no other track shows");
    let top = splitter(&h).top();
    assert!(top < start - 100.0, "the splitter went up from {start} to {top}");
    let lane = h.get_by_label(CLIP).rect();
    assert!(lane.bottom() <= top + 1.0, "the edited track sits right above the piano roll");

    // Dragged down again, the view comes back with every track.
    pull_to(&mut h, 500.0);
    assert!(h.query_all_by_label_contains(" on Bass at bar").next().is_some());
    assert!((splitter(&h).center().y - 500.0).abs() < 20.0, "at {:?}", splitter(&h));
}

#[test]
fn pulled_to_the_top_over_the_whole_window_nothing_stays_above() {
    let mut renderer = engine();
    let settings = Settings { piano_roll_top: PianoRollTop::Window, ..Settings::default() };
    let mut h = piano_app(&mut renderer, settings);
    pull_to(&mut h, 40.0);
    assert!(h.query_by_label(CLIP).is_none(), "no arrangement shows");
    assert!(h.query_by_label_contains("Piano roll,").is_some(), "the piano roll does");
    // A double-click on the splitter brings the view back, and another takes it to the top.
    let at = splitter(&h).center();
    h.get_by_label("Detail area splitter").hover();
    h.run();
    for _ in 0..2 {
        button(&mut h, at, true);
        button(&mut h, at, false);
    }
    h.run();
    assert!(h.query_by_label(CLIP).is_some(), "the view is back");
}

#[test]
fn the_view_shows_every_track_again_once_the_piano_roll_is_gone() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    pull_to(&mut h, 40.0);
    // Shift+Tab hides the detail area: the view is whole again.
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run();
    assert!(h.query_all_by_label_contains(" on Bass at bar").next().is_some());
}

#[test]
fn alt_and_the_wheel_zoom_key_rows() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let grid = h.get_by_label_contains("Piano roll,").rect();
    let height = h.state().piano_roll().key_height;
    h.hover_at(grid.center());
    h.step();
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::ALT));
    h.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, 60.0),
        modifiers: Modifiers::ALT,
        phase: egui::TouchPhase::Move,
    });
    h.run();
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run();
    assert!(h.state().piano_roll().key_height > height, "rows grew from {height}");
}

#[test]
fn the_draw_tool_adds_a_note_with_a_click_and_its_length_with_a_drag() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    tool(&mut h, "Draw");
    assert_eq!(h.state().piano_roll().tool, Tool::Draw);
    let before = notes(&h).len();
    let key = empty_key(&h);
    let first = notes(&h)[0].time;
    let p0 = spot(&h, first, key);
    click(&mut h, p0);
    let after = notes(&h);
    assert_eq!(after.len(), before + 1);
    // The piano roll's own grid is a sixteenth: the note is a quarter beat long.
    let added = after.last().unwrap();
    assert_eq!((added.key, added.duration), (key, 0.25));

    // A drag from an empty spot sets the length, on the grid.
    let at = spot(&h, first + 2.0, key);
    let to = spot(&h, first + 3.0, key);
    drag(&mut h, at, to, 4);
    let drawn = *notes(&h).last().unwrap();
    assert_eq!(notes(&h).len(), before + 2);
    assert!((drawn.duration - 1.0).abs() < 1e-9, "{drawn:?}");
    undo(&mut h);
    assert_eq!(notes(&h).len(), before + 1, "one drawn note is one undo step");
}

#[test]
fn b_switches_between_select_and_draw() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    // B is the piano roll's after a click in it.
    let at = note_rect(&h, &notes(&h)[0]).center();
    click(&mut h, at);
    h.key_press(Key::B);
    h.run();
    assert_eq!(h.state().piano_roll().tool, Tool::Draw);
    h.key_press(Key::B);
    h.run();
    assert_eq!(h.state().piano_roll().tool, Tool::Select);
}

#[test]
fn the_paint_tool_paints_a_row_of_notes_in_one_undo_step() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    tool(&mut h, "Paint");
    let before = notes(&h).len();
    let key = empty_key(&h);
    let first = notes(&h)[0].time;
    // Across a beat at a sixteenth: four notes, a quarter beat apart.
    let p0 = spot(&h, first, key);
    let p1 = spot(&h, first + 0.9, key);
    drag(&mut h, p0, p1, 12);
    let painted: Vec<Note> = notes(&h).into_iter().skip(before).collect();
    assert_eq!(painted.len(), 4, "{painted:?}");
    for (i, note) in painted.iter().enumerate() {
        assert_eq!(note.key, key);
        assert!((note.time - (painted[0].time + i as f64 * 0.25)).abs() < 1e-9, "{painted:?}");
    }
    undo(&mut h);
    assert_eq!(notes(&h).len(), before, "the whole stroke is one undo step");
}

#[test]
fn the_erase_tool_deletes_what_it_passes_over_in_one_undo_step() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    // Two notes side by side in an empty row to erase with one stroke.
    let key = empty_key(&h);
    let first = notes(&h)[0].time;
    let mut params = h.state().piano_roll_address().unwrap();
    params["notes"] = json!([
        { "time": first, "length": 0.25, "key": key, "velocity": 100 },
        { "time": first + 0.5, "length": 0.25, "key": key, "velocity": 100 },
    ]);
    h.state_mut().run(&Action::new("note.add", params));
    h.run();
    let before = notes(&h).len();
    tool(&mut h, "Erase");
    let p0 = spot(&h, first + 0.05, key);
    let p1 = spot(&h, first + 0.6, key);
    drag(&mut h, p0, p1, 10);
    assert_eq!(notes(&h).len(), before - 2);
    undo(&mut h);
    assert_eq!(notes(&h).len(), before);
    // A click erases one.
    let at = note_rect(&h, notes(&h).last().unwrap()).center();
    click(&mut h, at);
    assert_eq!(notes(&h).len(), before - 1);
}

#[test]
fn the_piano_roll_keeps_its_own_grid_unless_settings_link_it_to_the_control_bar() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    // The control bar's grid value sets the Arrangement view's only.
    h.state_mut().run(&Action::new("launcher.setQuantization", json!({ "value": "1/4" })));
    h.run();
    assert!(h.query_by_label("Grid value 1/16").is_some(), "the piano roll's own grid shows");
    tool(&mut h, "Draw");
    let key = empty_key(&h);
    let first = notes(&h)[0].time;
    let p0 = spot(&h, first, key);
    click(&mut h, p0);
    assert_eq!(notes(&h).last().unwrap().duration, 0.25);

    // Following the control bar, its value is the piano roll's.
    let settings = Settings { piano_grid_follows: true, ..Settings::default() };
    let ctx = h.ctx.clone();
    h.state_mut().set_settings(&ctx, settings);
    h.run();
    assert!(h.query_by_label("Grid value 1/4").is_some());
}

#[test]
fn the_toolbar_grid_menu_changes_the_piano_rolls_grid() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    h.get_by_label("Grid value 1/16").click();
    h.run();
    // The menu's item is picked as a screen reader picks it: a simulated press does not reach
    // an open menu in the test harness (neither the control bar's).
    h.get_by_label("1/8").click_accesskit();
    h.run();
    assert!(h.query_by_label("Grid value 1/8").is_some());
    tool(&mut h, "Draw");
    let key = empty_key(&h);
    let first = notes(&h)[0].time;
    let p0 = spot(&h, first + 1.0, key);
    click(&mut h, p0);
    assert_eq!(notes(&h).last().unwrap().duration, 0.5);
}

#[test]
fn a_new_note_takes_the_length_of_the_note_last_clicked() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let key = empty_key(&h);
    let first = notes(&h)[0].time;
    let mut params = h.state().piano_roll_address().unwrap();
    params["notes"] = json!([{ "time": first, "length": 1.5, "key": key, "velocity": 60 }]);
    h.state_mut().run(&Action::new("note.add", params));
    h.run();
    tool(&mut h, "Draw");
    let long = *notes(&h).last().unwrap();
    let p0 = note_rect(&h, &long).center();
    click(&mut h, p0);
    let p0 = spot(&h, first + 2.0, key);
    click(&mut h, p0);
    let drawn = notes(&h).last().copied().unwrap();
    assert_eq!(drawn.duration, 1.5);
    assert_eq!(suhina_session::midi_velocity(drawn.velocity), 60);
}

#[test]
fn settings_clip_editor_has_both_piano_roll_settings_and_they_change_the_settings() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run();
    assert!(h.state().settings_open());
    h.get_by_label("Settings page: Clip editor").click();
    h.run();
    // Both rows fit on the page, inside the window.
    for name in ["Full-height piano roll covers: Whole window", "Piano roll grid source: Control bar"] {
        let rect = h.get_by_label(name).rect();
        assert!(rect.bottom() < SIZE.1, "{name} at {rect:?}");
    }
    h.get_by_label("Full-height piano roll covers: Whole window").click();
    h.run();
    h.get_by_label("Piano roll grid source: Control bar").click();
    h.run();
    assert_eq!(h.state().settings().piano_roll_top, PianoRollTop::Window);
    assert!(h.state().settings().piano_grid_follows);
}

/// The number keys choose the tools, the way the toolbar's icons do (after a click in the piano
/// roll, so the keys keep their meaning elsewhere).
#[test]
fn the_number_keys_choose_the_tools() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    // Without the keyboard the number keys do nothing.
    h.key_press(Key::Num3);
    h.run();
    assert_eq!(h.state().piano_roll().tool, Tool::Select, "the piano roll does not have the keyboard");
    // A click in the piano roll gives it the keyboard; the notes are the demo clip's.
    let at = note_rect(&h, &notes(&h)[0]).center();
    click(&mut h, at);
    for (key, tool) in [
        (Key::Num1, Tool::Select),
        (Key::Num2, Tool::Draw),
        (Key::Num3, Tool::Paint),
        (Key::Num4, Tool::Erase),
        (Key::Num5, Tool::Slice),
    ] {
        h.key_press(key);
        h.run();
        assert_eq!(h.state().piano_roll().tool, tool, "{key:?}");
    }
    // B keeps switching between Select and Draw; the toolbar's icons show the key.
    h.key_press(Key::B);
    h.run();
    assert_eq!(h.state().piano_roll().tool, Tool::Draw);
    assert!(h.query_by_label("Draw (2)").is_some(), "the tool's key is in its label");
    h.key_press(Key::B);
    h.run();
    assert_eq!(h.state().piano_roll().tool, Tool::Select);
}

/// The Slice tool: a line drawn across notes splits each where it crosses, snapped to the grid,
/// in one undo step. A cut that lands on a note's start or end leaves that note alone.
#[test]
fn the_slice_tool_splits_the_notes_a_cut_line_crosses() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let key = empty_key(&h);
    let first = notes(&h)[0].time;
    // One long note in an empty row, from beat 2 to beat 2.8, so a cut at 2.5 can split it.
    let mut params = h.state().piano_roll_address().unwrap();
    params["notes"] = json!([{ "time": first + 2.0, "length": 0.8, "key": key, "velocity": 100 }]);
    h.state_mut().run(&Action::new("note.add", params));
    h.run();
    let before = notes(&h).len();
    tool(&mut h, "Slice");
    assert_eq!(h.state().piano_roll().tool, Tool::Slice);

    // A drag across the middle of the note: the cut snaps to the sixteenth's line at 2.5 and
    // the note becomes two.
    let from = spot(&h, first + 2.45, key);
    let to = spot(&h, first + 2.55, key);
    drag(&mut h, from, to, 4);
    let after = notes(&h);
    assert_eq!(after.len(), before + 1, "one note was split in two");
    let halves: Vec<Note> = after.iter().skip(before).copied().collect();
    assert_eq!(halves.len(), 1);
    let first_half = after[before - 1];
    assert_eq!((first_half.time, first_half.key), (first + 2.0, key));
    assert!((first_half.duration - 0.5).abs() < 1e-9, "{first_half:?}");
    assert_eq!((halves[0].time, halves[0].key), (first + 2.5, key));
    assert!((halves[0].duration - 0.3).abs() < 1e-9, "{:?}", halves[0]);
    // One stroke is one undo step.
    undo(&mut h);
    assert_eq!(notes(&h).len(), before);

    // A cut that lands on the note's end leaves it alone; a cut across an empty row does too.
    let from = spot(&h, first + 2.6, key);
    drag(&mut h, from, from + vec2(0.0, 0.0), 2);
    assert_eq!(notes(&h).len(), before, "a cut that crosses nothing splits nothing");
    let beyond = spot(&h, first + 5.0, empty_key(&h));
    drag(&mut h, beyond, beyond + vec2(0.0, 4.0), 2);
    assert_eq!(notes(&h).len(), before);

    // One cut line across two notes splits both, in one command and one undo step. The beats
    // here are close together so both notes are in view.
    let mut params = h.state().piano_roll_address().unwrap();
    params["notes"] = json!([
        { "time": first + 0.2, "length": 0.8, "key": key, "velocity": 100 },
        { "time": first + 0.4, "length": 0.8, "key": key + 2, "velocity": 100 },
    ]);
    h.state_mut().run(&Action::new("note.add", params));
    h.run();
    let count = notes(&h).len();
    let from = spot(&h, first + 0.6, key);
    let to = spot(&h, first + 0.6, key + 3);
    drag(&mut h, from, to, 6);
    let after: Vec<(f64, u8)> = notes(&h).into_iter().skip(count).map(|n| (n.time, n.key)).collect();
    assert_eq!(notes(&h).len(), count + 2, "both notes the line crosses were split: {after:?}");
    undo(&mut h);
    assert_eq!(notes(&h).len(), count, "one stroke, one undo step");
}

#[test]
fn the_right_button_and_the_wheel_choose_the_tools() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let grid = h.get_by_label_contains("Piano roll,").rect();
    let at = grid.center();
    // The right button held over the grid, then a wheel step: the next tool.
    let wheel = |h: &mut Harness<'static, App>, delta: f32| {
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.step();
        h.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, delta),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.step();
    };
    h.hover_at(at);
    h.step();
    let scroll = h.state().piano_roll().scroll;
    wheel(&mut h, 60.0);
    assert_eq!(h.state().piano_roll().tool, Tool::Draw, "one step on from Select");
    assert_eq!(h.state().piano_roll().scroll, scroll, "the wheel did not scroll");
    wheel(&mut h, 60.0);
    assert_eq!(h.state().piano_roll().tool, Tool::Paint);
    wheel(&mut h, -60.0);
    assert_eq!(h.state().piano_roll().tool, Tool::Draw, "the other way is the previous tool");
    // Letting the right button go opens nothing and changes no selection.
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert_eq!(h.state().piano_roll().tool, Tool::Draw);
    assert!(h.state().piano_roll().selected.is_empty());
    assert!(!h.state().piano_roll().quantize_open, "no menu opened");
}

/// Shift and Draw: a drag draws notes back to back, each as long as the note last drawn or
/// clicked, in the row the drag started in, as one `note.add` and one undo step.
#[test]
fn shift_and_draw_repeats_the_last_notes_length() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let key = empty_key(&h);
    let first = notes(&h)[0].time;
    // A note two grid steps long (an eighth grid: a quarter beat), to be the last one clicked,
    // so the stroke repeats its length.
    let mut params = h.state().piano_roll_address().unwrap();
    params["notes"] = json!([{ "time": first, "length": 0.5, "key": key, "velocity": 100 }]);
    h.state_mut().run(&Action::new("note.add", params));
    h.run();
    let before = notes(&h).len();
    tool(&mut h, "Draw");
    let at = note_rect(&h, notes(&h).last().unwrap()).center();
    click(&mut h, at);
    assert_eq!(notes(&h).len(), before, "clicking the note selected it, did not add one");

    // Shift and a sideways drag in an empty row just under it: back-to-back notes, one per half
    // beat, from where the press was.
    let row = key.wrapping_sub(1);
    let from = spot(&h, first + 2.0, row);
    let to = spot(&h, first + 4.4, row);
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::SHIFT));
    drag(&mut h, from, to, 8);
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
    let drawn: Vec<Note> = notes(&h).into_iter().skip(before).collect();
    assert!(drawn.len() >= 4, "{drawn:?}");
    for (i, note) in drawn.iter().enumerate() {
        assert_eq!(note.key, row, "the stroke stays in the row it started in");
        assert!((note.duration - 0.5).abs() < 1e-9, "the last note's length: {note:?}");
        assert!((note.time - (drawn[0].time + i as f64 * 0.5)).abs() < 1e-9, "{drawn:?}");
    }
    undo(&mut h);
    assert_eq!(notes(&h).len(), before, "the whole stroke is one undo step");
}

/// Clicks one of the toolbar's note clipboard buttons by name; their label is the name and the
/// key they have ("Copy (Ctrl+C)", "Copy (⌘C)" on macOS), so a button whose key the keymap has moved is still found.
fn clipboard(h: &mut Harness<'static, App>, name: &str) {
    let action = match name {
        "Cut" => "selection.cut",
        "Copy" => "selection.copy",
        "Paste" => "selection.paste",
        _ => "selection.duplicate",
    };
    let key = h.state().keymap().label(action).unwrap_or_default();
    let label = format!("{name} ({key})");
    assert!(h.query_by_label(&label).is_some(), "the {name} button reads {label:?}");
    h.get_by_label(&label).click();
    h.run();
}

#[test]
fn the_toolbar_cuts_copies_pastes_and_duplicates_the_selection() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let start = notes(&h).len();
    // The buttons show the key they run.
    assert!(h.query_by_label(&format!("Copy ({})", shown("Ctrl+C"))).is_some(), "Copy names its key");
    assert!(h.query_by_label(&format!("Cut ({})", shown("Ctrl+X"))).is_some(), "Cut names its key");
    // Nothing selected: the clipboard buttons are greyed out, so a click does nothing.
    clipboard(&mut h, "Copy");
    assert_eq!(notes(&h).len(), start, "nothing to copy");
    // A click in the piano roll selects a note; the buttons then run the same commands as the
    // keys (Ctrl+C, Ctrl+V, Ctrl+D, Ctrl+X).
    let at = note_rect(&h, &notes(&h)[0]).center();
    click(&mut h, at);
    h.run();
    clipboard(&mut h, "Copy");
    clipboard(&mut h, "Paste");
    assert!(notes(&h).len() > start, "the toolbar's Paste added the copied notes");
    let pasted = notes(&h).len();
    clipboard(&mut h, "Duplicate");
    assert!(notes(&h).len() > pasted);
    let before_cut = notes(&h).len();
    clipboard(&mut h, "Cut");
    assert!(notes(&h).len() < before_cut, "the toolbar's Cut deleted the selection");
}

/// The Settings › Keys presets: Draw-first makes Ctrl+B duplicate notes and a right-click in
/// Draw delete one; Select-first, the default, keeps Ctrl+D and does nothing on a right-click.
#[test]
fn the_shortcuts_presets_set_their_keys_and_the_right_click() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    assert_eq!(h.state().keymap().preset(), Preset::SelectFirst);
    // Draw a note in an empty row to right-click later.
    let key = empty_key(&h);
    let first = notes(&h)[0].time;
    let mut params = h.state().piano_roll_address().unwrap();
    params["notes"] = json!([{ "time": first, "length": 0.25, "key": key, "velocity": 100 }]);
    h.state_mut().run(&Action::new("note.add", params));
    h.run();
    let before = notes(&h).len();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run();
    assert!(h.state().settings_open());
    h.get_by_label("Settings page: Keys").click();
    h.run();
    h.get_by_label("Preset: Draw-first").click();
    h.run();
    assert_eq!(h.state().keymap().preset(), Preset::DrawFirst);
    assert_eq!(h.state().keymap().binding_text("selection.duplicate").as_deref(), Some("Ctrl+B"));
    // The choice is saved with the keymap.
    let saved = h.state_mut().take_changed_keymap().unwrap_or_else(|| panic!("the keymap changed"));
    assert!(saved.to_json().contains("\"preset\": \"Draw-first\""));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run();
    assert!(!h.state().settings_open());

    // In Draw, a right-click on a note deletes it.
    tool(&mut h, "Draw");
    let at = note_rect(&h, notes(&h).last().unwrap()).center();
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert_eq!(notes(&h).len(), before - 1, "the right-click deleted the note");
    undo(&mut h);
    assert_eq!(notes(&h).len(), before);

    // Ctrl+B duplicates the selected notes instead of toggling the browser.
    let browser = h.state().settings().browser_open;
    let at = note_rect(&h, notes(&h).last().unwrap()).center();
    click(&mut h, at);
    h.key_press_modifiers(Modifiers::COMMAND, Key::B);
    h.run();
    assert!(notes(&h).len() > before, "Ctrl+B duplicated the notes");
    assert_eq!(h.state().settings().browser_open, browser, "and did not touch the browser");

    // Back to Select-first: Ctrl+D again, and a right-click does nothing.
    let map = Keymap::default();
    h.state_mut().set_keymap(map);
    h.run();
    assert_eq!(h.state().keymap().preset(), Preset::SelectFirst);
    let count = notes(&h).len();
    let at = note_rect(&h, notes(&h).last().unwrap()).center();
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert_eq!(notes(&h).len(), count, "Select-first leaves the note alone");
}

/// The key column plays: a press sends `preview.noteOn` through the track's instrument, the
/// release sends `preview.noteOff`, and Ctrl+click selects the notes of that pitch.
#[test]
fn the_key_column_plays_and_ctrl_click_selects_a_pitch() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let before = notes(&h);
    let grid = h.get_by_label_contains("Piano roll,").rect();
    // The row of the first note: the key column is the 36 points left of the grid.
    let row = note_rect(&h, &before[0]).center().y;
    let at = egui::pos2(grid.left() - 28.0, row);
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    assert_eq!(h.state().piano_roll().playing_key(), Some(before[0].key), "the press plays the key");
    h.run();
    assert_eq!(h.state().piano_roll().playing_key(), Some(before[0].key), "it stays on while held");
    h.input_mut().events.push(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert_eq!(h.state().piano_roll().playing_key(), None, "the release let the key go");
    // The session ran the two statements the UI sends; the engine side of them is tested in
    // crates/session and crates/engine (`preview.noteOn`, `preview.noteOff`).
    let session = h.state().session().unwrap();
    assert!(session.player().is_some());

    // Ctrl+click on a key selects every note of that pitch, and changes nothing else.
    let with_ctrl = |h: &mut Harness<'static, App>, pos: egui::Pos2| {
        h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::COMMAND));
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::COMMAND,
        });
        h.step();
        h.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::COMMAND,
        });
        h.run();
        h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
    };
    with_ctrl(&mut h, at);
    let expected: Vec<usize> =
        before.iter().enumerate().filter(|(_, n)| n.key == before[0].key).map(|(i, _)| i).collect();
    assert_eq!(h.state().piano_roll().selected.iter().copied().collect::<Vec<_>>(), expected);
    assert_eq!(notes(&h), before, "selection only");
    assert_eq!(h.state().piano_roll().playing_key(), None, "no sound with Ctrl");
}

#[test]
fn a_paint_stroke_that_starts_on_a_note_paints_and_leaves_that_note_alone() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    tool(&mut h, "Paint");
    let before = notes(&h);
    let start = before[0];
    // From the middle of a note to a beat later, in its row.
    let from = note_rect(&h, &start).center();
    let to = spot(&h, start.time + 1.0, start.key);
    drag(&mut h, from, to, 12);
    let after = notes(&h);
    assert_eq!(after[0], start, "the note the stroke started on did not move");
    let painted: Vec<&Note> = after.iter().skip(before.len()).collect();
    assert!(!painted.is_empty(), "the stroke painted");
    assert!(painted.iter().all(|n| n.key == start.key && n.time > start.time), "{painted:?}");
}
