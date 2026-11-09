// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's scale (`docs/spec/ui-clip-editor.md` › Scale and key, › Zoom and scroll ›
//! Fold): the clip panel's Scale switch, root and mode set the clip's scale (a command, undone with
//! Ctrl+Z), the keys in the scale are marked and named, the toolbar's Highlight tints them, and its
//! Fold and Fold to scale show fewer rows that still hit and add notes on their own keys.

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

/// Opens the clip panel's list whose field reads `field` and picks `entry`.
fn pick(h: &mut Harness<'static, App>, field: &str, entry: &str) {
    h.get_by_label(field).click();
    h.run();
    h.get_by_label(entry).click();
    h.run();
}

/// Clicks the control labelled `label`.
fn press(h: &mut Harness<'static, App>, label: &str) {
    h.get_by_label(label).click();
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
fn the_clip_panel_sets_the_clips_scale_and_ctrl_z_undoes_it() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    // Fold, Scale and Highlight are in the toolbar, at 1280 px with the tools open.
    for word in ["Fold", "Scale", "Highlight scale"] {
        assert!(h.query_by_label(word).is_some(), "{word} is in the toolbar");
    }
    assert_eq!(edited(&h).1, None);
    assert!(h.query_by_label("Scale switch: Off").is_some());
    assert!(key_rows(&h).is_empty(), "without a scale or a fold the rows are not named");

    // Off, the fields show the default scale, C major; a pick turns the scale on with it.
    pick(&mut h, "Scale root: C", "D");
    assert_eq!(edited(&h).1, Scale::new(2, ScaleMode::Major));
    assert!(h.query_by_label("Scale switch: On").is_some());
    pick(&mut h, "Scale mode: Major", "Dorian");
    assert_eq!(edited(&h).1, Scale::new(2, ScaleMode::Dorian));

    // Ctrl+Z takes back one pick at a time.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(edited(&h).1, Scale::new(2, ScaleMode::Major));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(edited(&h).1, None);

    // The switch turns the scale on (the default) and off; off and on again brings back the
    // scale the clip had.
    press(&mut h, "Scale switch: Off");
    assert_eq!(edited(&h).1, Scale::new(0, ScaleMode::Major));
    pick(&mut h, "Scale mode: Major", "Minor");
    press(&mut h, "Scale switch: On");
    assert_eq!(edited(&h).1, None);
    assert!(h.query_by_label("Scale mode: Minor").is_some(), "off, it shows the scale it had");
    press(&mut h, "Scale switch: Off");
    assert_eq!(edited(&h).1, Scale::new(0, ScaleMode::Minor));
}

/// Settings' Default scale is what the switch turns on for a clip that never had a scale.
#[test]
fn the_default_scale_setting_is_what_the_switch_turns_on() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let settings = Settings { default_scale_root: 7, default_scale_mode: ScaleMode::Minor, ..Settings::default() };
    let ctx = h.ctx.clone();
    h.state_mut().set_settings(&ctx, settings);
    h.run();
    assert!(h.query_by_label("Scale root: G").is_some() && h.query_by_label("Scale mode: Minor").is_some());
    press(&mut h, "Scale switch: Off");
    assert_eq!(edited(&h).1, Scale::new(7, ScaleMode::Minor));
}

#[test]
fn the_keys_in_the_scale_are_marked_and_named() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let (notes, _) = edited(&h);
    pick(&mut h, "Scale root: C", "E");
    let rows = key_rows(&h);
    assert!(rows.len() > 5, "the rows in view are named: {rows:?}");
    // E major: E F# G# A B C# D#. Every row in view says whether it is in the scale; a scale picked
    // folds nothing, so the whole keyboard shows with the scale's rows marked (and highlighted).
    let scale = Scale::new(4, ScaleMode::Major).unwrap();
    for (label, _) in &rows {
        let name = label.trim_start_matches("Key ").trim_end_matches(" in scale");
        let key = (0..=127u8).find(|k| suhina_ui::detail::piano_roll::key_name(*k) == name).unwrap();
        assert_eq!(label.ends_with(" in scale"), scale.contains(key), "{label}");
    }
    assert!(rows.iter().any(|(l, _)| l.ends_with(" in scale")));
    let outside = |key: u8| !scale.contains(key) && notes.iter().all(|n| n.key != key);
    assert!(
        rows.iter().any(|(label, _)| {
            let name = label.trim_start_matches("Key ");
            (0..=127u8).any(|k| suhina_ui::detail::piano_roll::key_name(k) == name && outside(k))
        }),
        "a key outside the scale, with no notes, shows: {rows:?}"
    );
    assert_eq!(h.state().piano_roll().fold, Fold::Off, "Fold stays off");
    // Highlight turns the tint off and on again, kept in Settings; the scale stays the clip's.
    assert!(h.state().piano_roll().highlight_scale);
    press(&mut h, "Highlight scale");
    assert!(!h.state().piano_roll().highlight_scale && !h.state().settings().highlight_scale);
    assert_eq!(edited(&h).1, Scale::new(4, ScaleMode::Major));
    press(&mut h, "Highlight scale");
    assert!(h.state().piano_roll().highlight_scale && h.state().settings().highlight_scale);
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
    press(&mut h, "Fold");
    assert_eq!(h.state().piano_roll().fold, Fold::Notes);
    let rows = key_rows(&h);
    let names: Vec<String> =
        keys.iter().map(|k| format!("Key {}", suhina_ui::detail::piano_roll::key_name(*k))).collect();
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
    press(&mut h, "Fold");
    assert_eq!(h.state().piano_roll().fold, Fold::Off);
    assert!(key_rows(&h).is_empty());
}

/// The toolbar's Scale folds the rows to the clip's scale, turning the scale on when the clip has
/// none (the switch's scale); pressed again it shows every row, and the clip keeps its scale,
/// highlighted on the whole keyboard. The clip panel's switch turns the scale off, and Scale then
/// lights no more.
#[test]
fn the_toolbar_scale_folds_to_the_clips_scale() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let lit = |h: &Harness<'static, App>| h.get_by_label("Scale").accesskit_node().is_selected() == Some(true);
    assert!(!lit(&h));
    press(&mut h, "Scale");
    assert_eq!(edited(&h).1, Scale::new(0, ScaleMode::Major), "the default scale");
    assert!(h.query_by_label("Scale switch: On").is_some(), "the panel's switch is on too");
    assert!(lit(&h));
    assert_eq!(h.state().piano_roll().fold, Fold::Scale);
    // Again: every row, the clip's scale kept.
    press(&mut h, "Scale");
    assert!(!lit(&h));
    assert_eq!(h.state().piano_roll().fold, Fold::Off);
    assert_eq!(edited(&h).1, Scale::new(0, ScaleMode::Major), "the clip keeps its scale");
    press(&mut h, "Scale");
    pick(&mut h, "Scale mode: Major", "Dorian");
    press(&mut h, "Scale switch: On");
    assert_eq!(edited(&h).1, None);
    assert!(!lit(&h));
    // On again from the toolbar: the scale the clip had, kept by either.
    press(&mut h, "Scale");
    assert_eq!(edited(&h).1, Scale::new(0, ScaleMode::Dorian));
}

/// A click on the scale's root or mode gives it the arrow keys: with its list closed again (a second
/// click), Down and Up pick the next and the previous entry without the list, each a
/// `clip.setScale`; the piano roll's notes stay where they are.
#[test]
fn the_arrow_keys_step_the_scale_root_and_mode_once_clicked() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let (notes, scale) = edited(&h);
    assert_eq!(scale, None);
    // Opened and closed again: the field keeps the keys.
    press(&mut h, "Scale root: C");
    press(&mut h, "Scale root: C");
    h.key_press(Key::ArrowDown);
    h.run();
    assert_eq!(edited(&h).1, Scale::new(1, ScaleMode::Major), "Down: the next root, the scale on");
    h.key_press(Key::ArrowDown);
    h.run();
    h.key_press(Key::ArrowUp);
    h.run();
    assert_eq!(edited(&h).1, Scale::new(1, ScaleMode::Major), "Down, then Up: back one");
    assert_eq!(edited(&h).0, notes, "the arrows moved no note");
    // The mode the same way.
    let mode_label = h.query_all_by_label_contains("Scale mode: ").find_map(|n| n.accesskit_node().label()).unwrap();
    press(&mut h, &mode_label);
    press(&mut h, &mode_label);
    h.key_press(Key::ArrowDown);
    h.run();
    let after = edited(&h).1.unwrap();
    assert_eq!(after.root(), 1, "the root stays");
    assert_ne!(after.mode, ScaleMode::Major, "Down: the next mode");
}

/// Switching the scale on from the toolbar centres the notes in the rows it shows, wherever the
/// rows were scrolled: the view never lands on rows without notes.
#[test]
fn the_toolbar_scale_centres_the_notes_in_its_rows() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let (notes, _) = edited(&h);
    // Scrolled up to the highest keys, far from the notes.
    h.state_mut().piano_roll_mut().scroll.y = 0.0;
    h.run();
    assert_eq!(h.query_all_by_label_contains("Note ").count(), 0, "no note in view up there");
    press(&mut h, "Scale");
    assert!(edited(&h).1.is_some());
    assert_eq!(h.query_all_by_label_contains("Note ").count(), notes.len(), "the notes are in view again");
}

/// A scale picked folds nothing; the toolbar's Scale shows the scale's keys and the keys with
/// notes; Fold, the keys with notes alone.
#[test]
fn the_scale_shows_its_keys_and_fold_the_keys_with_notes() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    pick(&mut h, "Scale mode: Major", "Major pentatonic");
    assert_eq!(h.state().piano_roll().fold, Fold::Off, "the scale picked, every row still shows");
    press(&mut h, "Scale");
    assert_eq!(h.state().piano_roll().fold, Fold::Scale);
    let (notes, scale) = edited(&h);
    let scale = scale.unwrap();
    let rows = key_rows(&h);
    assert!(rows.len() > 3);
    // Every row is a key of C major pentatonic (C D E G A), or a key a note is on.
    for (label, _) in &rows {
        let name = label.trim_start_matches("Key ").trim_end_matches(" in scale");
        let key = (0..=127u8).find(|k| suhina_ui::detail::piano_roll::key_name(*k) == name).unwrap();
        assert!(scale.contains(key) || notes.iter().any(|n| n.key == key), "{label}");
    }
    assert!(
        rows.iter().any(|(label, _)| {
            let name = label.trim_start_matches("Key ").trim_end_matches(" in scale");
            notes.iter().all(|n| suhina_ui::detail::piano_roll::key_name(n.key) != name)
        }),
        "a key of the scale without notes shows"
    );
    // Rows sit one under the other, a key row apart, with no gaps for the folded keys.
    for pair in rows.windows(2) {
        assert!((pair[1].1.top() - pair[0].1.bottom()).abs() < 1.5, "{pair:?}");
    }
    assert_eq!(h.query_all_by_label_contains("Note ").count(), notes.len());
    // Fold: the keys with notes alone, the scale kept.
    press(&mut h, "Fold");
    assert_eq!(h.state().piano_roll().fold, Fold::Notes);
    for (label, _) in &key_rows(&h) {
        let name = label.trim_start_matches("Key ").trim_end_matches(" in scale");
        assert!(notes.iter().any(|n| suhina_ui::detail::piano_roll::key_name(n.key) == name), "{label}");
    }
    assert!(edited(&h).1.is_some());
}
