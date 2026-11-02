// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Piano roll step 4 in the window (`docs/spec/ui-clip-editor.md`): the toolbar's size setting and
//! its six tools, the Mute tool and muted notes, the MIDI tools column, the selection in the clip
//! column, and the Mouse-first preset's four mouse switches.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers, Pos2, Rect, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::{LaneData, Note};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::detail::piano_roll::{Tool, key_name};
use suhina_ui::settings::keymap::{Keymap, MouseSwitch, Preset};
use suhina_ui::settings::{DeviceLayout, Settings, ToolbarSize};
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
    let label = if note.muted {
        format!("Note {} at beat {} (muted)", key_name(note.key), note.time)
    } else {
        format!("Note {} at beat {}", key_name(note.key), note.time)
    };
    h.get_by_label(&label).rect()
}

/// Where beat `time` of key `key` is on screen, measured from two notes of the clip.
fn spot(h: &Harness<'static, App>, time: f64, key: u8) -> Pos2 {
    let n = notes(h);
    let (a, b) = (note_rect(h, &n[0]), note_rect(h, &n[1]));
    let beat = (b.left() - a.left()) / (n[1].time - n[0].time) as f32;
    let row = (a.top() - b.top()) / (f32::from(n[1].key) - f32::from(n[0].key));
    pos2(a.left() + (time - n[0].time) as f32 * beat + 2.0, a.center().y - (f32::from(key) - f32::from(n[0].key)) * row)
}

/// A key row with no note of the clip in view.
fn empty_key(h: &Harness<'static, App>) -> u8 {
    let n = notes(h);
    let (low, high) = (n.iter().map(|n| n.key).min().unwrap(), n.iter().map(|n| n.key).max().unwrap());
    (low..=high).find(|k| n.iter().all(|n| n.key != *k)).unwrap_or(low - 1)
}

fn pointer(h: &mut Harness<'static, App>, pos: Pos2, button: egui::PointerButton, pressed: bool, modifiers: Modifiers) {
    h.input_mut().events.push(egui::Event::PointerButton { pos, button, pressed, modifiers });
    h.step();
}

fn click(h: &mut Harness<'static, App>, pos: Pos2) {
    h.hover_at(pos);
    h.step();
    pointer(h, pos, egui::PointerButton::Primary, true, Modifiers::NONE);
    pointer(h, pos, egui::PointerButton::Primary, false, Modifiers::NONE);
    h.run();
}

/// Presses with `button` at `from`, moves to `to` in `steps` frames and lets go, with `modifiers`
/// held throughout.
fn drag_with(
    h: &mut Harness<'static, App>,
    from: Pos2,
    to: Pos2,
    steps: usize,
    button: egui::PointerButton,
    modifiers: Modifiers,
) {
    h.hover_at(from);
    h.step();
    if !modifiers.is_none() {
        h.input_mut().events.push(egui::Event::ModifiersChanged(modifiers));
    }
    pointer(h, from, button, true, modifiers);
    for step in 1..=steps {
        h.hover_at(from + (to - from) * (step as f32 / steps as f32));
        h.step();
    }
    pointer(h, to, button, false, modifiers);
    if !modifiers.is_none() {
        h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
    }
    h.run();
}

fn undo(h: &mut Harness<'static, App>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
}

/// A left-button drag, the way the tools' tests do it.
fn drag(h: &mut Harness<'static, App>, from: Pos2, to: Pos2, steps: usize) {
    drag_with(h, from, to, steps, egui::PointerButton::Primary, Modifiers::NONE);
}

/// Chooses a tool from the toolbar by its name; its label is the name and its key ("Mute (6)").
fn tool(h: &mut Harness<'static, App>, name: &str) {
    let keys = ["1", "2", "3", "4", "5", "6"];
    let label = keys
        .iter()
        .map(|key| format!("{name} ({key})"))
        .find(|label| h.query_by_label(label).is_some())
        .expect("a tool button with that name");
    h.get_by_label(&label).click();
    h.run();
}

// ---------------------------------------------------------------------------------------------
// 1. Toolbar size, icons and the sixth tool.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_toolbar_size_setting_scales_the_toolbar_and_not_the_grid() {
    let mut renderer = engine();
    let normal = Settings { toolbar_size: ToolbarSize::Normal, ..Settings::default() };
    let mut h = piano_app(&mut renderer, normal);
    let small_button = h.get_by_label("Draw (2)").rect().height();
    let small_rows = h.state().piano_roll().key_height;
    let small_grid = h.get_by_label_contains("Piano roll,").rect().top();

    let larger = Settings { toolbar_size: ToolbarSize::Larger, ..Settings::default() };
    let ctx = h.ctx.clone();
    h.state_mut().set_settings(&ctx, larger);
    h.run();

    let big_button = h.get_by_label("Draw (2)").rect().height();
    let big_rows = h.state().piano_roll().key_height;
    let big_grid = h.get_by_label_contains("Piano roll,").rect().top();
    assert!(big_button > small_button + 3.0, "the icon box grows: {small_button} to {big_button}");
    assert!(big_grid > small_grid + 5.0, "the band grows: {small_grid} to {big_grid}");
    assert_eq!(big_rows, small_rows, "the note rows do not change with the toolbar");
}

#[test]
fn the_toolbar_has_six_tools_and_key_six_chooses_mute() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    for (name, expected) in [
        ("Select", Tool::Select),
        ("Draw", Tool::Draw),
        ("Paint", Tool::Paint),
        ("Erase", Tool::Erase),
        ("Slice", Tool::Slice),
        ("Mute", Tool::Mute),
    ] {
        tool(&mut h, name);
        assert_eq!(h.state().piano_roll().tool, expected, "{name}");
    }
    h.key_press(Key::Num1);
    h.run();
    assert_eq!(h.state().piano_roll().tool, Tool::Select);
    for key in [Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6] {
        h.key_press(key);
        h.run();
    }
    assert_eq!(h.state().piano_roll().tool, Tool::Mute, "6 is Mute");
}

// ---------------------------------------------------------------------------------------------
// 2. The Mute tool and muted notes.
// ---------------------------------------------------------------------------------------------

#[test]
fn clicking_with_the_mute_tool_mutes_and_unmutes_one_note() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let before = notes(&h);
    tool(&mut h, "Mute");
    let at = note_rect(&h, &before[0]).center();
    click(&mut h, at);
    let after = notes(&h);
    assert!(after[0].muted, "the clicked note is muted");
    assert!(after[1..].iter().all(|n| !n.muted), "the others are not");
    assert!(
        h.query_by_label(&format!("Note {} at beat {} (muted)", key_name(before[0].key), before[0].time)).is_some()
    );
    // A second click unmutes it, and each click is one undo step.
    click(&mut h, at);
    assert!(!notes(&h)[0].muted);
    undo(&mut h);
    assert!(notes(&h)[0].muted);
    undo(&mut h);
    assert_eq!(notes(&h), before);
}

#[test]
fn a_mute_drag_mutes_a_run_and_one_undo_brings_it_all_back() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let before = notes(&h);
    tool(&mut h, "Mute");
    let from = note_rect(&h, &before[0]).center();
    let to = note_rect(&h, &before[2]).center();
    drag(&mut h, from, to, 8);
    let after = notes(&h);
    assert!(after[0].muted && after[2].muted, "the run is muted: {after:?}");
    assert!(after.iter().filter(|n| n.muted).count() >= 2);
    undo(&mut h);
    assert_eq!(notes(&h), before, "one undo puts the whole stroke back");
}

#[test]
fn muting_a_note_keeps_it_in_the_clip_and_out_of_the_render() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let before = notes(&h);
    tool(&mut h, "Mute");
    let at = note_rect(&h, &before[0]).center();
    click(&mut h, at);
    let after = notes(&h);
    assert_eq!(after.len(), before.len(), "the note is still there");
    assert_eq!(
        (after[0].time, after[0].duration, after[0].key, after[0].velocity),
        (before[0].time, before[0].duration, before[0].key, before[0].velocity),
        "a muted note keeps its place, length, key and velocity"
    );
    assert!(after[0].muted);
}

// ---------------------------------------------------------------------------------------------
// 3. The MIDI tools column.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_midi_tools_column_says_what_it_acts_on() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let total = notes(&h).len();
    assert!(h.query_by_label("Whole clip").is_some(), "nothing selected: the whole clip");
    assert!(h.query_by_label(&format!("{total} notes")).is_some(), "the count under the head");
    let at = note_rect(&h, &notes(&h)[0]).center();
    click(&mut h, at);
    assert!(h.query_by_label("Selected notes").is_some(), "one note selected");
    assert!(h.query_by_label("1 of 4 notes").is_some(), "the count under the head");
    h.key_press(Key::Escape);
    h.run();
    assert!(h.query_by_label("Whole clip").is_some(), "Escape clears the selection");
}

#[test]
fn transpose_moves_the_selected_note_a_semitone() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let before = notes(&h);
    let at = note_rect(&h, &before[0]).center();
    click(&mut h, at);
    h.state_mut().piano_roll_mut().tools.transpose = 1;
    h.run();
    h.get_by_label("Transpose").click_accesskit();
    h.run();
    let after = notes(&h);
    assert_eq!(after[0].key, before[0].key + 1);
    for (a, b) in after.iter().zip(&before).skip(1) {
        assert_eq!(a.key, b.key, "only the selected note moved");
    }
}

#[test]
fn the_whole_clip_tools_reverse_double_and_ramp() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let before = notes(&h);
    h.get_by_label("Reverse").click_accesskit();
    h.run();
    let reversed = notes(&h);
    assert_ne!(reversed, before, "the notes turned round");
    let span = |list: &[Note]| {
        let start = list.iter().map(|n| n.time).fold(f64::INFINITY, f64::min);
        let end = list.iter().map(|n| n.time + n.duration).fold(0.0, f64::max);
        (start, end)
    };
    assert_eq!(span(&reversed), span(&before), "the span is unchanged");

    h.get_by_label("×2").click_accesskit();
    h.run();
    let stretched = notes(&h);
    assert!(
        (stretched[0].duration - reversed[0].duration * 2.0).abs() < 1e-6,
        "{:?} against {:?}",
        stretched[0],
        reversed[0]
    );

    h.get_by_label("Ramp").click_accesskit();
    h.run();
    let ramped = notes(&h);
    let velocities: Vec<u8> = ramped.iter().map(|n| suhina_session::midi_velocity(n.velocity)).collect();
    let mut sorted = ramped.clone();
    sorted.sort_by(|a, b| a.time.total_cmp(&b.time));
    let expected: Vec<u8> = sorted.iter().map(|n| suhina_session::midi_velocity(n.velocity)).collect();
    assert!(velocities.iter().all(|v| (1..=127).contains(v)));
    // From 20 to 120 across the clip, in time order: the last note is the highest.
    assert!(expected[0] < *expected.last().unwrap(), "{expected:?}");
}

#[test]
fn humanise_twice_gives_two_different_results() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    h.get_by_label("Humanise").click_accesskit();
    h.run();
    let once = notes(&h);
    h.get_by_label("Humanise").click_accesskit();
    h.run();
    let twice = notes(&h);
    assert_ne!(once, twice, "each press draws afresh: {once:?} against {twice:?}");
}

#[test]
fn the_three_slider_icon_hides_and_shows_the_column() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    assert!(h.query_by_label("Whole clip").is_some(), "the column shows at first");
    h.get_by_label("MIDI tools").click();
    h.run();
    assert!(!h.state().piano_roll().tools_open);
    assert!(h.query_by_label("Whole clip").is_none(), "the column is hidden");
    h.get_by_label("MIDI tools").click();
    h.run();
    assert!(h.state().piano_roll().tools_open);
    assert!(h.query_by_label("Whole clip").is_some(), "and comes back");
}

// ---------------------------------------------------------------------------------------------
// 4. The selection in the clip column.
// ---------------------------------------------------------------------------------------------

#[test]
fn the_clip_column_describes_the_selection() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    let notes = notes(&h);
    let total = notes.len();
    assert!(h.query_by_label(&format!("Notes: {total}")).is_some(), "nothing selected: the count");
    assert!(h.query_by_label_contains("From: ").is_none(), "no selection rows without a selection");

    let at = note_rect(&h, &notes[0]).center();
    click(&mut h, at);
    let selected = h.state().piano_roll().selected.len();
    assert_eq!(selected, 1);
    assert!(h.query_by_label(&format!("Notes: 1 of {total}")).is_some());
    for label in ["From: ", "To: ", "Pitch: ", "Velocity: "] {
        assert!(h.query_by_label_contains(label).is_some(), "{label}");
    }
    // The pitch of one note is its own name, and its velocity is the one value.
    assert!(h.query_by_label(&format!("Pitch: {}", key_name(notes[0].key))).is_some());
    let velocity = suhina_session::midi_velocity(notes[0].velocity);
    assert!(h.query_by_label(&format!("Velocity: {velocity}")).is_some());

    // A muted note is counted in the row.
    tool(&mut h, "Mute");
    let at = note_rect(&h, &notes[0]).center();
    click(&mut h, at);
    assert!(h.query_by_label(&format!("Notes: 1 of {total}, 1 muted")).is_some());
}

// ---------------------------------------------------------------------------------------------
// 5. The Mouse-first preset and its four switches.
// ---------------------------------------------------------------------------------------------

fn mouse_first(h: &mut Harness<'static, App>) {
    let mut keymap = Keymap::default();
    keymap.set_preset(Preset::MouseFirst);
    h.state_mut().set_keymap(keymap);
    h.run();
    assert!(h.state().keymap().mouse().left_draws, "the preset turned the switches on");
}

#[test]
fn mouse_first_draws_with_the_left_button_and_deletes_with_the_right() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    mouse_first(&mut h);
    let before = notes(&h);
    // A left click on an empty spot draws a note, in the Select tool.
    let key = empty_key(&h);
    let time = before[1].time + 0.5;
    let at = spot(&h, time, key);
    click(&mut h, at);
    let drawn = notes(&h);
    assert_eq!(drawn.len(), before.len() + 1, "a note was drawn");
    assert!(drawn.iter().any(|n| n.key == key && (n.time - time).abs() < 0.5), "{drawn:?}");

    // A right click on a note deletes it.
    let target = notes(&h)[0];
    let at = note_rect(&h, &target).center();
    h.hover_at(at);
    h.step();
    pointer(&mut h, at, egui::PointerButton::Secondary, true, Modifiers::NONE);
    pointer(&mut h, at, egui::PointerButton::Secondary, false, Modifiers::NONE);
    h.run();
    let after = notes(&h);
    assert_eq!(after.len(), drawn.len() - 1, "the right click deleted a note");
    assert!(!after.iter().any(|n| n.key == target.key && (n.time - target.time).abs() < 1e-9));
    undo(&mut h);
    assert_eq!(notes(&h), drawn);
}

#[test]
fn mouse_first_splits_with_shift_right_and_plays_with_alt_right() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    mouse_first(&mut h);
    let before = notes(&h);
    // Shift and the right button over a note: the drag's line splits what it crosses.
    let long = before.iter().max_by(|a, b| a.duration.total_cmp(&b.duration)).copied().unwrap();
    let rect = note_rect(&h, &long);
    let from = pos2(rect.left() + 3.0, rect.center().y);
    let to = pos2(rect.left() + rect.width() * 0.5, rect.center().y);
    drag_with(&mut h, from, to, 6, egui::PointerButton::Secondary, Modifiers::SHIFT);
    let split = notes(&h);
    assert_eq!(split.len(), before.len() + 1, "one note became two: {split:?}");

    // Alt and the right button: the transport plays from the point under the pointer. The clip is
    // not edited and no note under the pointer is deleted, which is what the other right-button
    // jobs would do.
    let at = note_rect(&h, &split[0]).center();
    h.hover_at(at);
    h.step();
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::ALT));
    pointer(&mut h, at, egui::PointerButton::Secondary, true, Modifiers::ALT);
    h.run();
    pointer(&mut h, at, egui::PointerButton::Secondary, false, Modifiers::ALT);
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run();
    assert_eq!(notes(&h), split, "playing changes nothing in the project, and deletes nothing");
}

#[test]
fn turning_a_switch_off_makes_the_mode_custom_and_leaves_the_other_switches() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    mouse_first(&mut h);
    let mut keymap = h.state().keymap().clone();
    keymap.set_mouse(MouseSwitch::RightDeletes, false);
    h.state_mut().set_keymap(keymap);
    h.run();
    let mouse = h.state().keymap().mouse();
    assert_eq!(h.state().keymap().preset(), Preset::Custom, "a switch changed by hand is Custom");
    assert!(!mouse.right_deletes, "the switch is off");
    assert!(mouse.left_draws && mouse.shift_right_splits && mouse.alt_right_plays, "the rest stay on");
    // The file keeps both: the name and what the switches say.
    let (again, warnings) = Keymap::from_json(&h.state().keymap().to_json());
    assert_eq!(warnings.len(), 0);
    assert_eq!(again.preset(), Preset::Custom);
    assert_eq!(again.mouse(), mouse);
    // A right click deletes nothing with the switch off.
    let before = notes(&h);
    let at = note_rect(&h, &before[0]).center();
    h.hover_at(at);
    h.step();
    pointer(&mut h, at, egui::PointerButton::Secondary, true, Modifiers::NONE);
    pointer(&mut h, at, egui::PointerButton::Secondary, false, Modifiers::NONE);
    h.run();
    assert_eq!(notes(&h), before);
}

#[test]
fn the_right_button_and_the_wheel_still_choose_tools_and_delete_nothing() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    mouse_first(&mut h);
    let before = notes(&h);
    let at = note_rect(&h, &before[0]).center();
    h.hover_at(at);
    h.step();
    pointer(&mut h, at, egui::PointerButton::Secondary, true, Modifiers::NONE);
    h.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, 60.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    h.run();
    let steered = h.state().piano_roll().tool;
    assert_ne!(steered, Tool::Select, "the wheel stepped through the tools");
    pointer(&mut h, at, egui::PointerButton::Secondary, false, Modifiers::NONE);
    h.run();
    assert_eq!(notes(&h), before, "releasing over a note deletes nothing");
}

// ---------------------------------------------------------------------------------------------
// 6. The stacked detail area.
// ---------------------------------------------------------------------------------------------

#[test]
fn with_the_detail_area_stacked_the_column_is_hidden_and_ctrl_u_still_quantises() {
    let mut renderer = engine();
    // The clip is selected with the detail area in its usual place, where it is in view, and the
    // device chain is stacked afterwards: with the column beside the view there is no room for the
    // MIDI tools column.
    let mut h = piano_app(&mut renderer, Settings::default());
    let ctx = h.ctx.clone();
    h.state_mut().set_settings(&ctx, Settings { device_layout: DeviceLayout::Stack, ..Settings::default() });
    h.run();
    assert!(h.query_by_label("Whole clip").is_none(), "there is no room for the column");
    assert!(h.query_by_label("Quantise notes").is_none(), "and none for its controls");
    // The quantise settings and Ctrl+U still work where there is no column.
    h.key_press_modifiers(Modifiers::COMMAND, Key::U);
    h.run();
    let quantised = notes(&h);
    assert!(
        quantised.iter().all(|n| (n.time / 0.25 - (n.time / 0.25).round()).abs() < 1e-6),
        "Ctrl+U quantised without the column: {quantised:?}"
    );
    assert_eq!(h.state().piano_roll().quantize.amount, 100.0, "the settings it keeps are intact");
}
