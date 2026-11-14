// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the piano roll draws is each an element a theme styles on its own (#1417): its ruler's
//! numbers inside `ruler`, a muted note's dashed edge, the Focus switch of the clip strip.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use egui::Modifiers;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::{LaneData, Note};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::interaction::key_name::key_name;
use suhina_ui::{App, kit::theme};

const SIZE: (f32, f32) = (1280.0, 800.0);
/// The demo clip the tests edit: on Drums, at bar 17 (beat 64).
const CLIP: &str = "Clip Beat B on Drums at bar 17";
const CLIP_START: f64 = 64.0;

fn engine() -> suhina_engine::Renderer {
    suhina_engine::Renderer::stopped(
        &demo_project(),
        &suhina_engine::AudioPool::new(),
        48_000,
        suhina_session::devices(),
    )
    .unwrap()
}

/// The demo project in the Arrangement view with the clip selected, so the piano roll shows it.
fn piano_app(renderer: &mut suhina_engine::Renderer) -> Harness<'static, App> {
    let mut session = Session::new(demo_project());
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(session, &[]);
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
    harness.get_by_label(CLIP).click();
    harness.run();
    harness
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

/// Chooses a piano roll tool from its toolbar by its name; its label is the name and its key.
fn tool(h: &mut Harness<'static, App>, name: &str) {
    let label = ["1", "2", "3", "4", "5", "6"]
        .iter()
        .map(|key| format!("{name} ({key})"))
        .find(|label| h.query_by_label(label).is_some())
        .expect("a tool button with that name");
    h.get_by_label(&label).click();
    h.run();
}

/// A colour no theme or track uses, the `n`-th of a set.
fn odd(n: u8) -> egui::Color32 {
    egui::Color32::from_rgb(0x13, 0x57, n)
}

fn hex(color: egui::Color32) -> String {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Every shape the window painted in its last frame, flattened.
fn shapes(h: &Harness<'static, App>) -> Vec<egui::Shape> {
    fn walk(shape: &egui::Shape, found: &mut Vec<egui::Shape>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, found)),
            other => found.push(other.clone()),
        }
    }
    let mut found = Vec::new();
    h.output().shapes.iter().for_each(|shape| walk(&shape.shape, &mut found));
    found
}

/// The window's shapes with `css` as the user's sheet, after the frames it takes to apply.
fn shapes_with(h: &mut Harness<'static, App>, css: &str) -> Vec<egui::Shape> {
    suhina_ui::kit::native_css::set_user_sheet(Some(css.to_owned().into()));
    for _ in 0..4 {
        h.step();
    }
    let shown = shapes(h);
    suhina_ui::kit::native_css::set_user_sheet(None);
    shown
}

/// The colours of every text shape that reads `text`.
fn text_colours(shapes: &[egui::Shape], text: &str) -> Vec<egui::Color32> {
    shapes
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Text(shown) if shown.galley.text() == text => Some(shown),
            _ => None,
        })
        .flat_map(|shown| {
            std::iter::once(shown.fallback_color)
                .chain(shown.galley.job.sections.iter().map(|section| section.format.color))
                .chain(shown.override_text_color)
        })
        .collect()
}

/// The piano roll's bar numbers are drawn inside its `ruler`, as the Arrangement's are: a rule for
/// `ruler > ruler-number` reaches them (#1428 review).
#[test]
fn the_piano_roll_s_bar_numbers_are_inside_its_ruler() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let css = format!("piano-roll ruler > ruler-number {{ color: {}; }}", hex(odd(61)));
    let shown = shapes_with(&mut h, &css);
    let numbered = shown.iter().any(|shape| match shape {
        egui::Shape::Text(text) => {
            text.galley.text().parse::<u32>().is_ok()
                && (text.fallback_color == odd(61)
                    || text.galley.job.sections.iter().any(|s| s.format.color == odd(61)))
        }
        _ => false,
    });
    assert!(numbered, "a bar number takes the rule's colour");
}

/// A muted note's edge is dashed only: a theme's `[muted]` border colour paints no solid edge under
/// the dashes that would hide them (#1428 review).
#[test]
fn a_muted_note_s_border_is_its_dashes_alone() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    let first = notes(&h)[0];
    let label = format!("Note {} at beat {}", key_name(first.key), first.time);
    tool(&mut h, "Mute");
    h.get_by_label(&label).click();
    h.run();
    assert!(notes(&h)[0].muted, "the note is muted");
    tool(&mut h, "Select");
    // Nothing picked, so the note shows as muted and not as selected.
    h.key_press(egui::Key::Escape);
    h.run();
    let muted = format!("Note {} at beat {} (muted)", key_name(first.key), first.time);
    let rect = h.get_by_label(&muted).rect();
    let css = format!("piano-note[muted] {{ border-color: {}; }}", hex(odd(62)));
    let shown = shapes_with(&mut h, &css);
    let solid = shown
        .iter()
        .filter(|shape| matches!(shape, egui::Shape::Rect(r) if r.stroke.color == odd(62) && r.stroke.width > 0.0))
        .count();
    let dashes = shown
        .iter()
        .filter(|shape| {
            matches!(shape, egui::Shape::LineSegment { stroke, points } if stroke.color == odd(62)
                && rect.expand(1.0).contains(points[0]))
        })
        .count();
    assert_eq!(solid, 0, "no solid edge under the dashes");
    assert!(dashes > 0, "the dashes take the colour");
}

/// The lit Focus switch keeps its native label ink where only a rule for the plain switch gives a
/// colour: inside the switch the palette's text colours are the switch's own, so the ink is read
/// before it is entered (#1428 review).
#[test]
fn the_lit_focus_switch_keeps_its_label_ink() {
    let mut renderer = engine();
    let mut h = piano_app(&mut renderer);
    h.event(egui::Event::ModifiersChanged(Modifiers::COMMAND));
    h.get_by_label("Clip Walk on Bass at bar 17").click();
    h.run();
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run();
    h.get_by_label("Focus on the first clip").click();
    h.run();
    assert!(h.state().piano_roll().focus_first, "the switch is on");
    let css = format!("button#control-focus-first {{ color: {}; }}", hex(odd(63)));
    let inks = text_colours(&shapes_with(&mut h, &css), "Focus");
    assert!(!inks.is_empty(), "the label is drawn");
    assert!(!inks.contains(&odd(63)), "the plain switch's colour is not the lit one's label: {inks:?}");
    let lit = format!("{css} button#control-focus-first[checked] {{ color: {}; }}", hex(odd(64)));
    assert!(text_colours(&shapes_with(&mut h, &lit), "Focus").contains(&odd(64)), "a `[checked]` rule recolours it");
}
