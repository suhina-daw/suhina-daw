// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's grid menu and keys in the window (`docs/spec/ui-clip-editor.md` › Grid and
//! snapping): the menu ticks the chosen entries and changes the grid, Ctrl+1 to Ctrl+4 change it
//! after a click in the piano roll, a drawn note follows the value, snap off places notes freely,
//! an adaptive grid follows the zoom, and following the control bar greys out what it cannot hold.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers, Pos2, Rect, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use suhina_engine::Quantization;
use suhina_engine::model::{LaneData, Note};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::detail::piano_roll::key_name;
use suhina_ui::detail::piano_roll_grid::{GridMode, PianoGrid, Width};
use suhina_ui::settings::Settings;
use suhina_ui::settings::keymap::shown;
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);
/// The demo clip the tests edit: on Drums, at bar 17 (beat 64).
const CLIP: &str = "Clip Beat B on Drums at bar 17";
const CLIP_START: f64 = 64.0;

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
    let first = n[0];
    let second = *n.iter().find(|m| m.time != first.time && m.key != first.key).unwrap();
    let (a, b) = (note_rect(h, &first), note_rect(h, &second));
    let beat = (b.left() - a.left()) / (second.time - first.time) as f32;
    let row = (a.top() - b.top()) / (f32::from(second.key) - f32::from(first.key));
    pos2(
        a.left() + (time - first.time) as f32 * beat + 2.0,
        a.center().y - (f32::from(key) - f32::from(first.key)) * row,
    )
}

/// Empty key rows in view: between the clip's notes, or just under the lowest.
fn empty_keys(h: &Harness<'static, App>) -> Vec<u8> {
    let n = notes(h);
    let (low, high) = (n.iter().map(|n| n.key).min().unwrap(), n.iter().map(|n| n.key).max().unwrap());
    let mut keys: Vec<u8> = (low..=high).filter(|k| n.iter().all(|n| n.key != *k)).collect();
    keys.extend((low.saturating_sub(3)..low).rev());
    keys
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

/// `count` clicks at `pos`, a frame apart: two are a double-click.
fn clicks(h: &mut Harness<'static, App>, pos: Pos2, count: usize) {
    h.hover_at(pos);
    h.step();
    for _ in 0..count {
        pointer(h, pos, true);
        pointer(h, pos, false);
    }
    h.run();
}

/// Waits long enough that the next click is not part of a double-click.
fn pause(h: &mut Harness<'static, App>) {
    for _ in 0..10 {
        h.step();
    }
}

/// Double-clicks `pos` (an empty spot, in the Select tool) and returns the note it added.
fn draw_at(h: &mut Harness<'static, App>, pos: Pos2) -> Note {
    let before = notes(h);
    pause(h);
    clicks(h, pos, 2);
    let after = notes(h);
    assert_eq!(after.len(), before.len() + 1, "one note is added: {after:?}");
    after.into_iter().find(|n| !before.contains(n)).unwrap()
}

/// Opens the toolbar's grid menu (its button reads `Grid value …`) and picks `entry` in it, as a
/// screen reader picks it: a simulated press does not reach an open menu in the test harness.
fn pick(h: &mut Harness<'static, App>, entry: &str) {
    h.get_by_label_contains("Grid value").click();
    h.run();
    h.get_by_label(entry).click_accesskit();
    h.run();
}

fn grid_label(h: &Harness<'static, App>) -> String {
    h.get_by_label_contains("Grid value").accesskit_node().label().unwrap_or_default()
}

/// Gives the piano roll the keyboard with a click on an empty spot, then presses Ctrl and `key`.
fn ctrl(h: &mut Harness<'static, App>, key: Key) {
    h.key_press_modifiers(Modifiers::COMMAND, key);
    h.run();
}

fn focus(h: &mut Harness<'static, App>) {
    let key = empty_keys(h)[0];
    let at = spot(h, notes(h)[1].time + 0.6, key);
    pause(h);
    clicks(h, at, 1);
    pause(h);
}

#[test]
fn the_menu_ticks_the_chosen_entries_and_changes_the_grid() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    assert_eq!(grid_label(&h), "Grid value 1/16", "a snapping 1/16 by default");
    h.get_by_label("Grid value 1/16").click();
    h.run();
    let toggled =
        |h: &Harness<'static, App>, label: &str| format!("{:?}", h.get_by_label(label).accesskit_node().toggled());
    assert_eq!(toggled(&h, &format!("Snap to grid ({})", shown("Ctrl+4"))), "Some(True)");
    assert_eq!(toggled(&h, "1/16"), "Some(True)");
    assert_eq!(toggled(&h, "1/8"), "Some(False)");
    assert_eq!(toggled(&h, "Adaptive medium"), "Some(False)");
    assert_eq!(toggled(&h, &format!("Triplet grid ({})", shown("Ctrl+3"))), "Some(False)");
    // Each entry with a key shows it, and its label carries it.
    let narrow = format!("Narrow grid ({})", shown("Ctrl+1"));
    let widen = format!("Widen grid ({})", shown("Ctrl+2"));
    for entry in [narrow.as_str(), widen.as_str(), "Adaptive widest", "Adaptive narrowest", "8 bars", "1/32"] {
        assert!(h.query_by_label(entry).is_some(), "{entry} is in the menu");
    }
    h.get_by_label("1/8").click_accesskit();
    h.run();
    assert_eq!(grid_label(&h), "Grid value 1/8");
    pick(&mut h, &format!("Triplet grid ({})", shown("Ctrl+3")));
    assert_eq!(grid_label(&h), "Grid value 1/8T");
    pick(&mut h, "Adaptive wide");
    assert_eq!(grid_label(&h), "Grid value Adaptive T");
    pick(&mut h, &format!("Triplet grid ({})", shown("Ctrl+3")));
    pick(&mut h, &format!("Snap to grid ({})", shown("Ctrl+4")));
    assert_eq!(grid_label(&h), "Grid value Adaptive off");
}

#[test]
fn ctrl_and_one_to_four_change_the_grid_after_a_click_in_the_piano_roll() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    focus(&mut h);
    ctrl(&mut h, Key::Num1);
    assert_eq!(grid_label(&h), "Grid value 1/32", "Ctrl+1 narrows");
    ctrl(&mut h, Key::Num2);
    ctrl(&mut h, Key::Num2);
    assert_eq!(grid_label(&h), "Grid value 1/8", "Ctrl+2 widens");
    ctrl(&mut h, Key::Num3);
    assert_eq!(grid_label(&h), "Grid value 1/8T", "Ctrl+3 toggles triplets");
    ctrl(&mut h, Key::Num4);
    assert_eq!(grid_label(&h), "Grid value 1/8T off", "Ctrl+4 toggles snapping");
    ctrl(&mut h, Key::Num4);
    ctrl(&mut h, Key::Num3);
    assert_eq!(grid_label(&h), "Grid value 1/8");
}

#[test]
fn a_drawn_note_follows_the_chosen_value() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    pick(&mut h, "1/8");
    let keys = empty_keys(&h);
    let start = notes(&h)[1].time.floor() + 1.0;
    let at = spot(&h, start + 0.3, keys[0]);
    let note = draw_at(&mut h, at);
    assert_eq!(note.duration, 0.5, "a 1/8 note is half a beat long");
    assert!((note.time - (start + 0.0)).abs() < 1e-9, "it starts on the 1/8 line before the pointer: {note:?}");
}

#[test]
fn snap_off_lets_a_drawn_note_start_off_the_grid() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    pick(&mut h, &format!("Snap to grid ({})", shown("Ctrl+4")));
    assert_eq!(grid_label(&h), "Grid value 1/16 off");
    let keys = empty_keys(&h);
    let start = notes(&h)[1].time.floor() + 1.0;
    let at = spot(&h, start + 0.1, keys[0]);
    let note = draw_at(&mut h, at);
    let lines = note.time / 0.0625;
    assert!((lines - lines.round()).abs() > 1e-3, "not on any line down to 1/64: {note:?}");
}

#[test]
fn an_adaptive_grid_gets_coarser_as_ctrl_and_the_wheel_zoom_out() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer, Settings::default());
    pick(&mut h, "Adaptive medium");
    let medium = PianoGrid { snap: true, mode: GridMode::Adaptive(Width::Medium), triplet: false };
    let fitted = h.state().piano_roll().beat_width;
    let fine = medium.step(fitted, 4.0);
    assert!(fine <= 0.25, "the clip is fitted wide enough for a sixteenth: {fitted}");
    let keys = empty_keys(&h);
    let beat = notes(&h)[1].time.floor() + 1.0;
    // A note drawn 0.8 past a beat starts on the fine line before it, and is a step long.
    let at = spot(&h, beat + 0.8, keys[0]);
    let first = draw_at(&mut h, at);
    let floor = ((beat + 0.8) / fine).floor() * fine;
    assert!((first.time - floor).abs() < 1e-9 && (first.duration - fine).abs() < 1e-9, "{first:?}");

    // Zoomed out with Ctrl and the wheel until the grid is a beat or coarser.
    let grid = h.get_by_label_contains("Piano roll,").rect();
    for _ in 0..30 {
        if medium.step(h.state().piano_roll().beat_width, 4.0) >= 1.0 {
            break;
        }
        h.hover_at(grid.center());
        h.step();
        h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::COMMAND));
        h.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -60.0),
            modifiers: Modifiers::COMMAND,
            phase: egui::TouchPhase::Move,
        });
        h.step();
        h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
        h.step();
    }
    h.run();
    let coarse = medium.step(h.state().piano_roll().beat_width, 4.0);
    assert!(coarse >= 1.0, "zoomed out: {}", h.state().piano_roll().beat_width);
    let at = spot(&h, beat + 0.8, keys[1]);
    let second = draw_at(&mut h, at);
    let floor = ((beat + 0.8) / coarse).floor() * coarse;
    assert!((second.time - floor).abs() < 1e-9, "on the coarser line: {second:?} for {coarse}");
}

#[test]
fn following_the_control_bar_the_menu_sets_its_value_and_greys_out_the_rest() {
    let mut renderer = engine();
    let settings = Settings { piano_grid_follows: true, ..Settings::default() };
    let mut h = piano_app(&mut renderer, settings);
    let quantization = |h: &Harness<'static, App>| h.state().session().unwrap().quantization();
    h.get_by_label_contains("Grid value").click();
    h.run();
    assert!(h.get_by_label("Adaptive medium").accesskit_node().is_disabled(), "the control bar has no adaptive grid");
    assert!(!h.get_by_label("1/8").accesskit_node().is_disabled());
    h.get_by_label("1/8").click_accesskit();
    h.run();
    assert_eq!(quantization(&h), Quantization::Beats(0.5));
    assert_eq!(grid_label(&h), "Grid value 1/8");
    // Snap off is the control bar's none.
    pick(&mut h, &format!("Snap to grid ({})", shown("Ctrl+4")));
    assert_eq!(quantization(&h), Quantization::None);
    assert_eq!(grid_label(&h), "Grid value 1/16 off");
    // Picking a value snaps again.
    pick(&mut h, "1/4");
    assert_eq!(quantization(&h), Quantization::Beats(1.0));
}

/// Renders the grid menu open, for a look at it (`cargo test -p suhina-ui --test piano_roll_grid
/// -- --ignored`); written to `target/ui-snapshots/`.
#[test]
#[ignore = "renders with a GPU adapter"]
fn render_the_grid_menu() {
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
    h.get_by_label("Grid value 1/16").click();
    h.run();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&out).unwrap();
    h.render().unwrap().save(out.join("piano-roll-grid-menu.png")).unwrap();
}
