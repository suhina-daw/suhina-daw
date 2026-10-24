// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Short panes, from trying the window with the detail area pulled up and down: the Arrangement
//! view's note preview at short lane heights, the piano roll's least height and the ways to close
//! it, its notes in view at any height, and a clip column that never wraps
//! (`docs/spec/ui-clip-editor.md` › Least height and closing, `docs/spec/arrangement-editing.md`
//! › Selecting clips).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers, Pos2, Rect, Shape, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use suhina_engine::model::{Device, DeviceKind, LaneData, Note};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::detail::piano_roll::key_name;
use suhina_ui::settings::keymap::shown;
use suhina_ui::settings::{DeviceLayout, Settings};
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);
/// The demo clip the tests edit: on Drums, at bar 17 (beat 64).
const CLIP: &str = "Clip Beat B on Drums at bar 17";
const CLIP_START: f64 = 64.0;
/// The piano roll's least height (› Least height and closing).
const LEAST: f32 = 120.0;

fn window() -> Harness<'static, App> {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness
}

/// The demo project in the Arrangement view with the clip selected, so the piano roll shows it.
fn piano_app() -> Harness<'static, App> {
    let mut harness = window();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label(CLIP).click();
    harness.run();
    harness
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

fn drag(h: &mut Harness<'static, App>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.step();
    button(h, from, true);
    for step in 1..=6 {
        h.hover_at(from + (to - from) * (step as f32 / 6.0));
        h.step();
    }
    button(h, to, false);
    h.run();
}

fn splitter(h: &Harness<'static, App>) -> Rect {
    h.get_by_label("Detail area splitter").rect()
}

/// Drags the detail area's splitter so its middle ends at `y`.
fn pull_to(h: &mut Harness<'static, App>, y: f32) {
    let from = splitter(h).center();
    drag(h, from, pos2(from.x, y));
}

/// Where the detail area ends: the bottom of the view's area, found from the splitter and the
/// detail area's default height before anything is dragged.
fn detail_bottom(h: &Harness<'static, App>) -> f32 {
    splitter(h).bottom() + 200.0
}

fn piano_shows(h: &Harness<'static, App>) -> bool {
    h.query_by_label_contains("Piano roll,").is_some()
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

/// Every shape painted last frame, flattened, with the rectangle it is clipped to.
fn shapes(h: &Harness<'static, App>) -> Vec<(Rect, Shape)> {
    fn walk(clip: Rect, shape: &Shape, out: &mut Vec<(Rect, Shape)>) {
        match shape {
            Shape::Vec(inner) => inner.iter().for_each(|s| walk(clip, s, out)),
            other => out.push((clip, other.clone())),
        }
    }
    let mut out = Vec::new();
    for clipped in &h.output().shapes {
        walk(clipped.clip_rect, &clipped.shape, &mut out);
    }
    out
}

/// The note marks painted inside `clip` and seen there: 2 px lines under its 2 px colour strip.
fn note_marks(h: &Harness<'static, App>, clip: Rect) -> usize {
    shapes(h)
        .into_iter()
        .filter(|(clipped, shape)| match shape {
            Shape::Rect(r) => {
                let seen = r.rect.intersect(*clipped).intersect(clip);
                (r.rect.height() - 2.0).abs() < 0.01 && r.rect.top() > clip.top() + 2.5 && seen.is_positive()
            }
            _ => false,
        })
        .count()
}

/// Text painted on more than one line inside `area`, as (text, lines).
fn wrapped_text(h: &Harness<'static, App>, area: Rect) -> Vec<(String, usize)> {
    shapes(h)
        .into_iter()
        .filter_map(|(_, shape)| match shape {
            Shape::Text(t) if t.galley.rows.len() > 1 && area.contains(t.pos) => {
                Some((t.galley.job.text.clone(), t.galley.rows.len()))
            }
            _ => None,
        })
        .collect()
}

// 1. The Arrangement view's note preview at short heights.

#[test]
fn a_lane_pulled_to_the_top_still_shows_its_notes() {
    let mut h = piano_app();
    let lane = h.get_by_label(CLIP).rect();
    assert!(note_marks(&h, lane) > 0, "the clip shows its notes in the whole view");
    pull_to(&mut h, 40.0);
    assert!(h.query_all_by_label_contains(" on Bass at bar").next().is_none(), "only the edited track");
    let pulled = h.get_by_label(CLIP).rect();
    assert!(
        pulled.height() >= lane.height() - 1.0,
        "the edited lane shows whole: {} of {} px",
        pulled.height(),
        lane.height()
    );
    assert!(note_marks(&h, pulled) > 0, "the clip's notes show while the piano roll is pulled up");
}

#[test]
fn a_lane_at_its_least_height_still_shows_its_notes() {
    let mut h = piano_app();
    // Alt and the wheel make every lane shorter, down to the least 20 px.
    let lane = h.get_by_label(CLIP).rect();
    h.hover_at(lane.center());
    h.step();
    for _ in 0..6 {
        h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::ALT));
        h.input_mut().events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -400.0),
            modifiers: Modifiers::ALT,
            phase: egui::TouchPhase::Move,
        });
        h.run();
    }
    h.input_mut().events.push(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run();
    let short = h.get_by_label(CLIP).rect();
    assert!(short.height() < 21.0, "the lane is at its least height: {short:?}");
    assert!(note_marks(&h, short) > 0, "a 20 px lane still shows its clip's notes");
    let volume = h.get_by_label("Drums volume").rect();
    let header = h.get_by_label("Track Drums").rect();
    assert!(volume.width() >= 48.0, "a compact lane keeps a usable volume field");
    assert!(header.contains_rect(volume), "the volume hit area stays inside the compact header");
    let automation = h.get_by_label("Show automation of Drums").rect();
    let toggle = h.get_by_label("Turn Drums off").rect();
    assert!(automation.right() <= toggle.left(), "automation stays at the left in compact mode");
    assert!(header.contains_rect(automation), "automation remains visible inside the compact header");
}

// 2. The piano roll's least height, and closing it with the splitter.

#[test]
fn the_splitter_stops_at_the_piano_rolls_least_height() {
    let mut h = piano_app();
    let bottom = detail_bottom(&h);
    pull_to(&mut h, bottom - 90.0);
    let height = bottom - splitter(&h).bottom();
    assert!((height - LEAST).abs() < 1.5, "the detail area stops at {LEAST} px, not {height}");
    assert!(piano_shows(&h));
    // Five key rows at the default 12 px row height, at least.
    let grid = h.get_by_label_contains("Piano roll,").rect();
    assert!(grid.height() >= 5.0 * 12.0 - 0.5, "five key rows: the grid is {} px", grid.height());
    // The clip column's five rows are all there, under its title, inside the detail area.
    for label in ["Track: ", "Start: ", "Length: ", "Loop: ", "Notes: "] {
        let row = h.get_by_label_contains(label).rect();
        assert!(row.bottom() <= bottom + 0.5, "{label} is whole: {row:?} over {bottom}");
    }
}

#[test]
fn dragging_the_splitter_down_closes_the_editor_and_up_opens_it() {
    let mut h = piano_app();
    let bottom = detail_bottom(&h);
    pull_to(&mut h, bottom - 10.0);
    assert!(!piano_shows(&h), "dragged below the least height, the editor closes");
    let collapsed = splitter(&h);
    assert!(collapsed.bottom() >= bottom - 0.5, "the splitter stays along the bottom: {collapsed:?}");
    assert!(h.state().selected_clip().is_some(), "the clip stays selected");
    // Dragged up again, the editor opens where the drag ends.
    pull_to(&mut h, bottom - 300.0);
    assert!(piano_shows(&h), "dragged up, the editor opens");
    let height = bottom - splitter(&h).bottom();
    assert!((height - 297.0).abs() < 8.0, "at the height the drag reached: {height}");
}

#[test]
fn going_back_up_in_the_same_drag_keeps_the_editor_open() {
    let mut h = piano_app();
    let bottom = detail_bottom(&h);
    let from = splitter(&h).center();
    h.hover_at(from);
    h.step();
    button(&mut h, from, true);
    for y in [from.y + 60.0, bottom - 10.0, bottom - 20.0, bottom - 150.0] {
        h.hover_at(pos2(from.x, y));
        h.step();
    }
    button(&mut h, pos2(from.x, bottom - 150.0), false);
    h.run();
    assert!(piano_shows(&h), "back above the closing point, the editor is open again");
    let height = bottom - splitter(&h).bottom();
    assert!((height - 147.0).abs() < 8.0, "under the pointer: {height}");
}

// 3. The notes in view at any height.

/// The note nearest the middle of the clip's lowest and highest note, as the piano roll names it.
fn middle_note(h: &Harness<'static, App>) -> String {
    let notes = notes(h);
    let low = notes.iter().map(|n| n.key).min().unwrap();
    let high = notes.iter().map(|n| n.key).max().unwrap();
    let middle = (f32::from(low) + f32::from(high)) / 2.0;
    let note = notes
        .iter()
        .min_by(|a, b| (f32::from(a.key) - middle).abs().total_cmp(&(f32::from(b.key) - middle).abs()))
        .unwrap();
    format!("Note {} at beat {}", key_name(note.key), note.time)
}

#[test]
fn at_the_least_height_the_notes_are_in_view() {
    let mut h = piano_app();
    let bottom = detail_bottom(&h);
    let middle = middle_note(&h);
    assert!(h.query_by_label(&middle).is_some(), "{middle} shows at the default height");
    pull_to(&mut h, bottom - 90.0);
    assert!(h.query_by_label(&middle).is_some(), "{middle} shows at the least height");
    // Taller again, the notes are still centred.
    pull_to(&mut h, bottom - 420.0);
    assert!(h.query_by_label(&middle).is_some(), "{middle} shows when taller");
}

#[test]
fn the_view_keeps_a_scroll_made_by_hand_when_the_height_changes() {
    let mut h = piano_app();
    let bottom = detail_bottom(&h);
    let grid = h.get_by_label_contains("Piano roll,").rect();
    h.hover_at(grid.center());
    h.step();
    h.input_mut().events.push(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, 120.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    h.run();
    let scrolled = h.state().piano_roll().scroll.y;
    pull_to(&mut h, bottom - 300.0);
    assert!(
        (h.state().piano_roll().scroll.y - scrolled).abs() < 0.5,
        "the scroll made by hand stays: {} then {}",
        scrolled,
        h.state().piano_roll().scroll.y
    );
}

// 4. The clip column never wraps.

#[test]
fn the_clip_column_never_wraps_and_leaves_out_rows_that_do_not_fit() {
    for layout in [DeviceLayout::Row, DeviceLayout::Stack] {
        let mut h = piano_app();
        let ctx = h.ctx.clone();
        h.state_mut().set_settings(&ctx, Settings { device_layout: layout, ..Settings::default() });
        h.run();
        // Every note selected, so the column has its selection rows too.
        let grid = h.get_by_label_contains("Piano roll,").rect();
        click(&mut h, pos2(grid.right() - 4.0, grid.top() + 4.0));
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.run();
        for size in [(1280.0, 800.0), (900.0, 500.0), (640.0, 360.0), (420.0, 260.0)] {
            h.set_size(vec2(size.0, size.1));
            h.run();
            if layout == DeviceLayout::Row {
                let c = splitter(&h).center();
                drag(&mut h, c, pos2(c.x, size.1 - 90.0));
            }
            let whole = Rect::from_min_size(Pos2::ZERO, vec2(size.0, size.1));
            let wrapped = wrapped_text(&h, whole);
            assert!(wrapped.is_empty(), "{layout:?} at {size:?}: text wrapped onto lines: {wrapped:?}");
        }
    }
}

#[test]
fn at_the_least_height_the_selection_rows_are_left_out_not_cut() {
    let mut h = piano_app();
    let bottom = detail_bottom(&h);
    let grid = h.get_by_label_contains("Piano roll,").rect();
    click(&mut h, pos2(grid.right() - 4.0, grid.top() + 4.0));
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run();
    assert!(h.query_by_label_contains("Pitch: ").is_some(), "the selection rows show at the default height");
    pull_to(&mut h, bottom - 90.0);
    assert!(h.query_by_label_contains("Notes: ").is_some(), "the Notes row fits");
    for label in ["From: ", "To: ", "Pitch: ", "Velocity: "] {
        if let Some(row) = h.query_by_label_contains(label) {
            assert!(row.rect().bottom() <= bottom + 0.5, "{label} is drawn whole or not at all: {:?}", row.rect());
        }
    }
}

// 5. The ways to close the editor.

fn hide_button(h: &Harness<'static, App>) -> Option<Rect> {
    h.query_all_by_label_contains("Hide editor")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .map(|n| n.rect())
}

#[test]
fn the_x_at_the_end_of_the_toolbar_closes_the_editor() {
    let mut h = piano_app();
    let x = hide_button(&h).expect("the toolbar has a Hide editor button");
    let grid = h.get_by_label_contains("Piano roll,").rect();
    assert!(x.right() >= grid.right() - 12.0, "at the right end of the toolbar: {x:?}, grid {grid:?}");
    assert!(x.bottom() <= grid.top(), "in the toolbar, above the grid");
    let tooltip =
        h.query_all_by_label_contains("Hide editor").find_map(|n| n.accesskit_node().label()).unwrap_or_default();
    assert!(tooltip.contains(&shown("Ctrl+Shift+E")), "its name says its key: {tooltip}");
    click(&mut h, x.center());
    assert!(!piano_shows(&h), "the editor is closed");
    assert!(h.state().selected_clip().is_some(), "the clip stays selected");
}

#[test]
fn selecting_a_clip_opens_the_closed_editor_at_its_height() {
    let mut h = piano_app();
    let bottom = detail_bottom(&h);
    pull_to(&mut h, bottom - 250.0);
    let height = bottom - splitter(&h).bottom();
    let x = hide_button(&h).expect("a Hide editor button").center();
    click(&mut h, x);
    assert!(!piano_shows(&h));
    // A click on the clip again opens it, as tall as it was.
    h.get_by_label(CLIP).click();
    h.run();
    assert!(piano_shows(&h), "selecting the clip opens the editor");
    let again = bottom - splitter(&h).bottom();
    assert!((again - height).abs() < 1.5, "at its height: {again}, was {height}");
}

#[test]
fn shift_tab_hides_the_detail_area_and_a_selection_leaves_it_hidden() {
    let mut h = piano_app();
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run();
    assert!(!piano_shows(&h));
    h.get_by_label(CLIP).click();
    h.run();
    assert!(!piano_shows(&h), "hidden with Shift+Tab, it stays hidden");
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run();
    assert!(piano_shows(&h));
}

/// The View menu's entry `text` (the toolbar's button says its key in brackets).
fn menu_item<'h>(h: &'h Harness<'static, App>, text: &'h str) -> egui_kittest::Node<'h> {
    h.query_all_by_label_contains(text)
        .find(|n| !n.accesskit_node().label().unwrap_or_default().contains('('))
        .expect("a menu entry")
}

#[test]
fn view_hide_editor_and_its_key_close_and_open_the_editor() {
    let mut h = piano_app();
    h.get_by_label("View").click();
    h.run_steps(4);
    menu_item(&h, "Hide editor").click();
    h.run_steps(4);
    assert!(!piano_shows(&h), "View › Hide editor closes it");
    h.get_by_label("View").click();
    h.run_steps(4);
    menu_item(&h, "Show editor").click();
    h.run_steps(4);
    assert!(piano_shows(&h), "View › Show editor opens it");
    // Ctrl+Shift+E does the same.
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
    h.run();
    assert!(!piano_shows(&h), "Ctrl+Shift+E closes it");
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::E);
    h.run();
    assert!(piano_shows(&h), "and opens it again");
}

#[test]
fn escape_in_the_piano_roll_clears_the_selection_and_leaves_the_editor_open() {
    let mut h = piano_app();
    let grid = h.get_by_label_contains("Piano roll,").rect();
    click(&mut h, pos2(grid.right() - 4.0, grid.top() + 4.0));
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run();
    assert!(!h.state().piano_roll().selected.is_empty());
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().piano_roll().selected.is_empty(), "Escape clears the note selection");
    assert!(piano_shows(&h), "and the editor stays open");
}

#[test]
fn a_click_on_empty_arrangement_space_clears_the_clip_selection() {
    let mut h = piano_app();
    // An empty spot of a lane: a point of it no clip covers.
    let clips: Vec<Rect> = h.query_all_by_label_contains("Clip ").map(|n| n.rect()).collect();
    let empty = h
        .query_all_by_label_contains("Lane ")
        .map(|n| n.rect())
        .flat_map(|lane| {
            (0..100).map(move |i| pos2(lane.left() + 4.0 + i as f32 * (lane.width() - 8.0) / 100.0, lane.center().y))
        })
        .find(|p| !clips.iter().any(|c| c.expand(2.0).contains(*p)) && p.x < SIZE.0 - 8.0)
        .expect("an empty spot on some lane");
    click(&mut h, empty);
    assert_eq!(h.state().selected_clip(), None, "the click cleared the clip selection");
    assert!(!piano_shows(&h), "the piano roll has nothing to show");
    assert!(h.query_by_label("Detail area splitter").is_some_and(|s| s.rect().bottom() < detail_bottom(&h) - 100.0));
    // A click on the clip selects it again.
    h.get_by_label(CLIP).click();
    h.run();
    assert!(piano_shows(&h));
}

/// A track whose chain is short (one plug-in without its editor embedded) still
/// lets the detail area be dragged up to its default height: the cap that keeps the area no
/// taller than its panels never holds it under that height (PR #727, a tester's report).
#[test]
fn a_short_chain_does_not_hold_the_detail_area_under_its_default_height() {
    let mut project = demo_project();
    let keys = project.tracks.iter().find(|t| t.name == "Keys").and_then(|t| t.channel).expect("Keys has a channel");
    // Only a plug-in on the track, its editor not embedded: a short panel.
    project.channel_mut(keys).expect("its channel").devices =
        vec![Device::new("Verb", DeviceKind::Vst3 { id: "ABC".into() })];
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(project), &[]);
    let mut h = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&h.ctx);
    h.run();
    h.get_by_label("Arrangement view").click();
    h.run();
    let bottom = detail_bottom(&h);
    let header = h.get_by_label("Track Keys").rect();
    click(&mut h, pos2(header.left() + 30.0, header.center().y));
    // Down to a short area, then up past the default height.
    pull_to(&mut h, bottom - 80.0);
    pull_to(&mut h, bottom - 400.0);
    let height = bottom - splitter(&h).bottom();
    assert!(height >= 199.0, "the area goes up to its default height at least: {height}");
}
