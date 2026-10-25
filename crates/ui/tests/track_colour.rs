// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Track colours in the window: the Arrangement header's colour band, the Session column's
//! coloured header and the channel strip's band down its edge are drawn in the track's colour over their area;
//! a click on a colour area opens the palette, a swatch sets the colour in one undo step
//! (`docs/design/components.md` › Colour palette).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use egui::epaint::{RectShape, Shape};
use egui::{Color32, Key, Modifiers, Rect};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::Rgb;
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::{App, interaction::colour_picker, kit::theme};

const SIZE: (f32, f32) = (1280.0, 900.0);

fn window() -> Harness<'static, App> {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness
}

fn arrangement() -> Harness<'static, App> {
    let mut harness = window();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness
}

/// Drums' colour as the project has it (`None`: the default one).
fn drums_colour(harness: &Harness<'static, App>) -> Option<Rgb> {
    harness.state().session().unwrap().project().tracks[0].color
}

/// Every filled rectangle the last frame drew.
fn filled_rects(harness: &Harness<'static, App>) -> Vec<RectShape> {
    fn walk(shape: &Shape, out: &mut Vec<RectShape>) {
        match shape {
            Shape::Rect(rect) => out.push(rect.clone()),
            Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &harness.output().shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// Whether the last frame filled all of `area` with `colour`.
fn filled_with(harness: &Harness<'static, App>, area: Rect, colour: Color32) -> bool {
    filled_rects(harness).iter().any(|r| r.fill == colour && r.rect.expand(0.5).contains_rect(area))
}

fn default_drums() -> Color32 {
    colour_picker::color32(colour_picker::swatches()[0])
}

#[test]
fn the_arrangement_header_has_a_band_of_the_track_colour_down_its_whole_height() {
    let harness = arrangement();
    let header = harness.get_by_label("Track Drums").rect();
    let band = harness.get_by_label("Drums colour").rect();
    assert!((band.height() - header.height()).abs() < 1.0, "the band runs the header's height: {band:?} {header:?}");
    assert!((band.left() - header.left()).abs() < 1.0 && band.width() >= 5.0, "{band:?} {header:?}");
    assert!(filled_with(&harness, band, default_drums()), "the band is drawn in Drums' colour");
}

#[test]
fn the_session_header_and_the_strip_show_the_track_colour() {
    let harness = window();
    let header = harness.get_by_label("Track Drums").rect();
    assert!(filled_with(&harness, header, default_drums()), "the column's header is Drums' colour: {header:?}");
    // The strip's band runs down its whole height, and its name row stands on the colour too.
    let band = harness.get_by_label("Drums colour").rect();
    let strip = harness.get_by_label("Drums channel strip").rect();
    assert!((band.height() - strip.height()).abs() < 1.0 && (band.left() - strip.left()).abs() < 1.0, "{band:?}");
    assert!(filled_with(&harness, band, default_drums()));
    let name = harness.get_by_label("Drums strip").rect();
    assert!(filled_with(&harness, name, default_drums()), "the strip's name row is Drums' colour");
}

#[test]
fn the_colour_area_opens_the_palette_a_swatch_sets_the_colour_and_undo_brings_it_back() {
    let mut harness = arrangement();
    let pick = colour_picker::swatches()[4];
    let label = format!("Colour {}", colour_picker::hex(pick));
    assert!(harness.query_by_label(&label).is_none(), "the palette is closed");
    harness.get_by_label("Drums colour").click();
    harness.run();
    harness.get_by_label(&label).click();
    harness.run();
    assert_eq!(drums_colour(&harness), Some(pick));
    assert!(harness.query_by_label(&label).is_none(), "picking closes the palette");
    let band = harness.get_by_label("Drums colour").rect();
    assert!(filled_with(&harness, band, colour_picker::color32(pick)), "the band shows the new colour");

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(drums_colour(&harness), None, "one undo brings the old colour back");
    assert!(!harness.state().session().unwrap().can_undo());
}

#[test]
fn the_strips_band_opens_the_same_palette_and_the_menu_offers_it_too() {
    let mut harness = window();
    let pick = colour_picker::swatches()[6];
    let label = format!("Colour {}", colour_picker::hex(pick));
    harness.get_by_label("Drums colour").click();
    harness.run();
    for colour in colour_picker::swatches() {
        assert!(harness.query_by_label(&format!("Colour {}", colour_picker::hex(colour))).is_some());
    }
    harness.get_by_label(&label).click();
    harness.run();
    assert_eq!(drums_colour(&harness), Some(pick));
    let header = harness.get_by_label("Track Drums").rect();
    assert!(filled_with(&harness, header, colour_picker::color32(pick)), "the header follows");

    // The header's right-click menu has the same swatches.
    let back = colour_picker::swatches()[0];
    harness.get_by_label("Track Drums").click_secondary();
    harness.run();
    harness.get_by_label(&format!("Colour {}", colour_picker::hex(back))).click();
    harness.run();
    assert_eq!(drums_colour(&harness), Some(back));
}

#[test]
fn picking_the_colour_the_track_has_changes_nothing() {
    let mut harness = arrangement();
    harness.get_by_label("Drums colour").click();
    harness.run();
    harness.get_by_label(&format!("Colour {}", colour_picker::hex(colour_picker::swatches()[0]))).click();
    harness.run();
    assert!(!harness.state().session().unwrap().can_undo(), "no command for the same colour");
}

#[test]
fn midi_monitor_is_separate_from_arm_and_offers_contextual_choices() {
    let mut h = arrangement();
    let header = h.get_by_label("Track Drums").rect();
    let monitor = h.get_by_label("MIDI monitoring of Drums: Auto").rect();
    let arm = h.get_by_label("Arm Drums for recording").rect();
    assert!(header.contains_rect(monitor));
    assert!(monitor.left() >= arm.right());
    h.get_by_label("MIDI monitoring of Drums: Auto").click();
    h.run();
    assert!(h.query_by_label("Hear your MIDI keyboard").is_some());
    assert!(h.query_by_label("Hear MIDI when this track is ready to record.").is_some());
    let off = h.get_by_label("Off").rect().center();
    h.hover_at(off);
    h.run();
    assert!(h.query_by_label("Live MIDI is silent. You can still record it.").is_some());
    h.get_by_label("Always on").click();
    h.run();
    assert!(h.query_by_label("MIDI monitoring of Drums: On").is_some());
    assert!(!h.state().session().unwrap().is_armed(suhina_engine::model::TrackId(1)));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert!(h.query_by_label("MIDI monitoring of Drums: Auto").is_some());
}

/// The MIDI monitoring chip never runs into the automation button: at the default header width
/// both take their own clicks, and a header dragged to its narrowest drops the chip (#876).
#[test]
fn the_midi_monitor_chip_keeps_clear_of_the_automation_button() {
    let mut h = arrangement();
    let clear = |h: &Harness<'static, App>| {
        let lanes = h.get_by_label("Show automation of Drums").rect();
        let header = h.get_by_label("Track Drums").rect();
        assert!(header.contains_rect(lanes), "{lanes:?} in {header:?}");
        h.query_by_label("MIDI monitoring of Drums: Auto").map(|chip| {
            let chip = chip.rect();
            assert!(chip.right() <= lanes.left() && header.contains_rect(chip), "{chip:?} {lanes:?}");
        })
    };
    assert!(clear(&h).is_some(), "the default width shows the chip");
    h.get_by_label("MIDI monitoring of Drums: Auto").click();
    h.run();
    assert!(h.query_by_label("Always on").is_some(), "the chip takes its click");
    h.key_press(Key::Escape);
    h.run();
    // The headers are at the right: dragging their left edge right narrows them to the least.
    let edge = h.get_by_label("Track Drums").rect().left_center() - egui::vec2(1.0, 0.0);
    h.hover_at(edge);
    h.run();
    h.drag_at(edge);
    h.run();
    h.hover_at(edge + egui::vec2(200.0, 0.0));
    h.run();
    h.drop_at(edge + egui::vec2(200.0, 0.0));
    h.run();
    assert_eq!(h.state().arrangement().header_width, 160.0);
    assert!(clear(&h).is_none(), "the narrowest header drops the chip");
    h.get_by_label("Show automation of Drums").click();
    h.run();
    assert!(h.query_by_label("Hide automation of Drums").is_some(), "the button takes its click");
}
