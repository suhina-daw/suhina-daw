// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's scale (`docs/spec/ui-clip-editor.md` › Scale and key, › Zoom and scroll ›
//! Fold): the toolbar's Scale menu sets the clip's scale (a command, undone with Ctrl+Z), the keys
//! in the scale are marked and named, and Fold and Fold to scale show fewer rows that still hit
//! and add notes on their own keys.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers, Pos2, Rect};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use suhina_engine::model::{LaneData, Note, Scale, ScaleMode};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::detail::piano_roll_scale::Fold;
use suhina_ui::settings::Settings;
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);
/// The demo clip the tests edit: Walk on Bass, at bar 17 (beat 64), the first clip on Bass' lane.
const CLIP: &str = "Clip Walk on Bass at bar 17";

fn engine() -> suhina_engine::Renderer {
    suhina_engine::Renderer::stopped(
        &demo_project(),
        &suhina_engine::AudioPool::new(),
        48_000,
        suhina_session::devices(),
    )
    .unwrap()
}

/// The app at 1280 × 800 with the browser closed and the MIDI tools column open (the default),
/// editing Walk.
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
    assert!(harness.state().piano_roll().tools_open, "the MIDI tools column is open");
    harness
}

/// The edited clip's content: its notes and its scale.
fn edited(h: &Harness<'static, App>) -> (Vec<Note>, Option<Scale>) {
    let project = h.state().session().unwrap().project();
    let clip = &project.arrangement.lanes[1].clips[1];
    assert_eq!(clip.start, 64.0);
    let content = project.content(clip.content).unwrap();
    let notes = match &content.lanes[0].data {
        LaneData::Notes(notes) => notes.to_vec(),
        LaneData::Audio(_) => Vec::new(),
    };
    (notes, content.scale)
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

fn clicks(h: &mut Harness<'static, App>, pos: Pos2, count: usize) {
    h.hover_at(pos);
    h.step();
    for _ in 0..count {
        pointer(h, pos, true);
        pointer(h, pos, false);
    }
    h.run();
}

/// Opens the Scale menu, whose button reads `now`, then the submenu `sub` (if any), and picks
/// `entry`.
fn pick(h: &mut Harness<'static, App>, now: &str, sub: Option<&str>, entry: &str) {
    h.get_by_label(now).click();
    h.run();
    if let Some(sub) = sub {
        h.get_by_label(sub).click();
        h.run();
    }
    h.get_by_label(entry).click();
    h.run();
}

/// The labels of the key column's rows in view, top to bottom.
fn key_rows(h: &Harness<'static, App>) -> Vec<(String, Rect)> {
    let mut rows: Vec<(String, Rect)> = h
        .query_all_by_label_contains("Key ")
        .filter_map(|n| {
            let label = n.accesskit_node().label()?;
            label.starts_with("Key ").then(|| (label.to_string(), n.rect()))
        })
        .collect();
    rows.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()));
    rows
}

#[test]
fn the_scale_menu_sets_the_clips_scale_and_ctrl_z_undoes_it() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    // It fits the toolbar at 1280 px with the MIDI tools column open.
    assert!(h.query_by_label("Scale off").is_some(), "the menu is in the toolbar");
    assert_eq!(edited(&h).1, None);
    assert!(key_rows(&h).is_empty(), "without a scale or a fold the rows are not named");

    pick(&mut h, "Scale off", Some("Scale root: C"), "Scale root D");
    assert_eq!(edited(&h).1, Scale::new(2, ScaleMode::Major), "a root on its own is major");
    assert!(h.query_by_label("Scale D major").is_some());
    pick(&mut h, "Scale D major", Some("Scale mode: Major"), "Scale mode Dorian");
    assert_eq!(edited(&h).1, Scale::new(2, ScaleMode::Dorian));
    assert!(h.query_by_label("Scale D dorian").is_some());

    // Ctrl+Z takes back one pick at a time.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(edited(&h).1, Scale::new(2, ScaleMode::Major));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(edited(&h).1, None);
    assert!(h.query_by_label("Scale off").is_some());

    // Off clears it again.
    pick(&mut h, "Scale off", Some("Scale mode: Major"), "Scale mode Minor");
    assert_eq!(edited(&h).1, Scale::new(0, ScaleMode::Minor));
    pick(&mut h, "Scale C minor", None, "Scale: Off");
    assert_eq!(edited(&h).1, None);
}

#[test]
fn the_keys_in_the_scale_are_marked_and_named() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    pick(&mut h, "Scale off", Some("Scale root: C"), "Scale root E");
    let rows = key_rows(&h);
    assert!(rows.len() > 5, "the rows in view are named: {rows:?}");
    // E major: E F# G# A B C# D#. Every row in view says whether it is in the scale.
    let scale = Scale::new(4, ScaleMode::Major).unwrap();
    for (label, _) in &rows {
        let name = label.trim_start_matches("Key ").trim_end_matches(" in scale");
        let key = (0..=127u8).find(|k| suhina_ui::interaction::key_name::key_name(*k) == name).unwrap();
        assert_eq!(label.ends_with(" in scale"), scale.contains(key), "{label}");
    }
    assert!(rows.iter().any(|(l, _)| l.ends_with(" in scale")));
    assert!(rows.iter().any(|(l, _)| !l.ends_with(" in scale")));
    // Nothing of this hides a row: they are one key apart.
    assert_eq!(h.state().piano_roll().fold, Fold::Off);
}

#[test]
fn fold_shows_only_the_rows_with_notes_and_a_double_click_adds_on_the_rows_key() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let (before, _) = edited(&h);
    let mut keys: Vec<u8> = before.iter().map(|n| n.key).collect();
    keys.sort_unstable();
    keys.dedup();
    keys.reverse();
    pick(&mut h, "Scale off", None, "Fold");
    assert_eq!(h.state().piano_roll().fold, Fold::Notes);
    let rows = key_rows(&h);
    let names: Vec<String> =
        keys.iter().map(|k| format!("Key {}", suhina_ui::interaction::key_name::key_name(*k))).collect();
    assert_eq!(rows.iter().map(|(l, _)| l.clone()).collect::<Vec<_>>(), names, "one row per key with notes");
    // Every note is still drawn and found.
    assert_eq!(h.query_all_by_label_contains("Note ").count(), before.len());
    // Nothing of this is a command.
    assert!(!h.state().session().unwrap().can_undo());

    // A double-click on an empty spot of the lowest row adds a note on that row's key.
    let (_, lowest) = rows.last().unwrap().clone();
    let grid = h.get_by_label(&format!("Piano roll, {} notes", before.len())).rect();
    let taken: Vec<Rect> = h.query_all_by_label_contains("Note ").map(|n| n.rect()).collect();
    let y = lowest.center().y;
    let x = (0..40)
        .map(|i| grid.left() + 8.0 + i as f32 * (grid.width() - 16.0) / 40.0)
        .find(|x| taken.iter().all(|r| !r.expand(6.0).contains(Pos2::new(*x, y))))
        .unwrap();
    clicks(&mut h, Pos2::new(x, y), 2);
    let (after, _) = edited(&h);
    assert_eq!(after.len(), before.len() + 1, "{after:?}");
    let added = after.iter().find(|n| !before.contains(n)).unwrap();
    assert_eq!(added.key, *keys.last().unwrap(), "the new note is on the row's key");

    // Fold again: every key shows, and the rows are no longer named.
    pick(&mut h, "Scale off", None, "Fold");
    assert_eq!(h.state().piano_roll().fold, Fold::Off);
    assert!(key_rows(&h).is_empty());
}

#[test]
fn fold_to_scale_shows_the_scales_keys_and_the_keys_with_notes() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    // Without a scale there is nothing to fold to: the entry is greyed out.
    h.get_by_label("Scale off").click();
    h.run();
    assert!(h.get_by_label("Fold to scale").accesskit_node().is_disabled());
    h.get_by_label("Scale off").click();
    h.run();

    pick(&mut h, "Scale off", Some("Scale mode: Major"), "Scale mode Major pentatonic");
    pick(&mut h, "Scale C major pentatonic", None, "Fold to scale");
    assert_eq!(h.state().piano_roll().fold, Fold::Scale);
    let (notes, scale) = edited(&h);
    let scale = scale.unwrap();
    let rows = key_rows(&h);
    assert!(rows.len() > 3);
    // Every row is a key of C major pentatonic (C D E G A), or a key a note is on.
    for (label, _) in &rows {
        let name = label.trim_start_matches("Key ").trim_end_matches(" in scale");
        let key = (0..=127u8).find(|k| suhina_ui::interaction::key_name::key_name(*k) == name).unwrap();
        assert!(scale.contains(key) || notes.iter().any(|n| n.key == key), "{label}");
    }
    // Rows sit one under the other, a key row apart, with no gaps for the folded keys.
    for pair in rows.windows(2) {
        assert!((pair[1].1.top() - pair[0].1.bottom()).abs() < 1.5, "{pair:?}");
    }
    assert_eq!(h.query_all_by_label_contains("Note ").count(), notes.len());
}
