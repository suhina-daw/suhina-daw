// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use egui::Key;
use egui::accesskit::Toggled;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

#[derive(Default)]
struct State {
    clicks: usize,
    on: bool,
    wheel: bool,
    open: bool,
    hue: f32,
    dragged: bool,
}

/// A column of every control, each acting on `State`.
fn harness() -> Harness<'static, State> {
    Harness::builder().allow_missing_glyphs().with_size(egui::vec2(320.0, 320.0)).build_ui_state(
        |ui, state: &mut State| {
            crate::kit::theme::install(ui.ctx());
            super::heading(ui, "Palette");
            super::note(ui, "Colours you use appear here");
            super::separator(ui);
            if super::button(ui, "Random").clicked() {
                state.clicks += 1;
            }
            if super::check_box(ui, "Steps", state.on).clicked() {
                state.on = !state.on;
            }
            if let Some(wheel) = super::options(ui, "View", &[("Field", false), ("Wheel", true)], state.wheel) {
                state.wheel = wheel;
            }
            if super::disclosure(ui, "Fine adjustment", state.open).clicked() {
                state.open = !state.open;
            }
            let response = super::slider(ui, "Hue", &mut state.hue, 0.0..=360.0, 1.0, |h| format!("{h:.0}°"));
            state.dragged |= response.dragged();
        },
        State::default(),
    )
}

#[test]
fn buttons_tick_boxes_choices_and_sections_report_their_clicks_and_states() {
    let mut harness = harness();
    harness.run();
    harness.get_by_label("Palette");
    harness.get_by_label("Colours you use appear here");
    harness.get_by_label("Random").click();
    harness.run();
    assert_eq!(harness.state().clicks, 1);
    assert_eq!(harness.get_by_label("Steps").accesskit_node().toggled(), Some(Toggled::False));
    harness.get_by_label("Steps").click();
    harness.run();
    assert!(harness.state().on);
    assert_eq!(harness.get_by_label("Steps").accesskit_node().toggled(), Some(Toggled::True));
    assert_eq!(harness.get_by_label("View: Field").accesskit_node().toggled(), Some(Toggled::True), "the chosen part");
    harness.get_by_label("View: Wheel").click();
    harness.run();
    assert!(harness.state().wheel);
    harness.get_by_label("View: Wheel").click();
    harness.run();
    assert!(harness.state().wheel, "the chosen part asks for nothing");
    harness.get_by_label("Fine adjustment").click();
    harness.run();
    assert!(harness.state().open);
    assert_eq!(harness.get_by_label("Fine adjustment").accesskit_node().toggled(), Some(Toggled::True));
}

#[test]
fn a_slider_takes_a_click_and_then_the_arrow_keys() {
    let mut harness = harness();
    harness.run();
    let rail = harness.get_by_label("Hue").rect();
    // The track is the rail less 4 points each side: its middle is half the range.
    harness.get_by_label("Hue").click();
    harness.run();
    let hue = harness.state().hue;
    assert!((hue - 180.0).abs() < 2.0, "a click in the middle of {rail:?} gave {hue}");
    assert!(!harness.state().dragged, "a click is not a drag");
    harness.key_press(Key::ArrowRight);
    harness.run();
    assert!((harness.state().hue - (hue + 1.0)).abs() < 1e-3, "the click gave it the keys");
    harness.key_press(Key::ArrowLeft);
    harness.key_press(Key::ArrowLeft);
    harness.run();
    assert!((harness.state().hue - (hue - 1.0)).abs() < 1e-3);
    assert_eq!(harness.get_by_label("Hue").accesskit_node().numeric_value(), Some(f64::from(harness.state().hue)));
}
