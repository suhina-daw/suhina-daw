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

#[test]
fn default_header_is_40_px_and_keeps_controls_and_sends_inside() {
    let harness = arrangement();
    let header = harness.get_by_label("Track Drums").rect();
    assert!((header.height() - 40.0).abs() < 0.5, "default header: {header:?}");
    let automation = harness.get_by_label("Show automation of Drums").rect();
    let volume = harness.get_by_label("Drums volume").rect();
    assert!(automation.left() < volume.left(), "automation stays at the left");
    assert!(header.contains_rect(automation), "automation is visible at the default height");
    for label in ["Turn Drums off", "Solo Drums", "Drums volume", "Drums send to A Reverb"] {
        let control = harness.get_by_label(label).rect();
        assert!(control.height() >= 16.0, "{label}: {control:?}");
        assert!(header.contains_rect(control), "{label} must fit inside its header");
    }
}

#[test]
fn arrangement_header_surfaces_follow_custom_theme_tokens() {
    let mut harness = arrangement();
    let previous = theme::palette();
    let mut custom = previous;
    custom.track_header_background = Color32::from_rgb(31, 45, 53);
    custom.track_header_selected = Color32::from_rgb(65, 72, 84);
    custom.control_selected = Color32::from_rgb(233, 229, 222);
    theme::set_palette(&harness.ctx, custom);
    harness.run();
    let header = harness.get_by_label("Track Drums").rect();
    let color = default_drums();
    let has_surface =
        filled_with(&harness, header, suhina_ui::kit::motion::mix(custom.track_header_background, color, 0.82))
            || filled_with(
                &harness,
                header,
                suhina_ui::kit::motion::mix(
                    suhina_ui::kit::motion::mix(custom.track_header_selected, color, 0.82),
                    theme::text_primary(),
                    0.07,
                ),
            );
    let neutral_volumes = ["Drums volume", "Bass volume", "A Reverb volume"]
        .into_iter()
        .all(|label| filled_with(&harness, harness.get_by_label(label).rect(), custom.control_selected));
    theme::set_palette(&harness.ctx, previous);
    assert!(has_surface, "the header must tint the editable theme surface with its track color");
    assert!(neutral_volumes, "volume fills share the neutral theme colour regardless of track colour");
}

#[test]
fn selected_header_uses_its_theme_surface_without_recolouring_the_track() {
    let mut harness = arrangement();
    let before = harness.state().session().unwrap().project().clone();
    harness.get_by_label("Track Drums").click();
    harness.run();
    let previous = theme::palette();
    let mut custom = previous;
    custom.track_header_selected = Color32::from_rgb(180, 35, 110);
    theme::set_palette(&harness.ctx, custom);
    harness.run();
    let header = harness.get_by_label("Track Drums").rect();
    let expected = suhina_ui::kit::motion::mix(
        suhina_ui::kit::motion::mix(custom.track_header_selected, default_drums(), 0.82),
        custom.text_primary,
        0.07,
    );
    let shown = filled_with(&harness, header, expected);
    theme::set_palette(&harness.ctx, previous);
    assert!(shown, "selected header must use track.headerSelectedBackground");
    assert_eq!(harness.state().session().unwrap().project(), &before);
}

#[test]
fn arrangement_grid_uses_its_own_theme_colors() {
    let mut harness = arrangement();
    let previous = theme::palette();
    let mut custom = previous;
    custom.grid_major = Color32::from_rgb(117, 83, 149);
    custom.grid_minor = Color32::from_rgb(53, 79, 101);
    theme::set_palette(&harness.ctx, custom);
    harness.run();
    fn has_line(shape: &Shape, color: Color32) -> bool {
        match shape {
            Shape::LineSegment { stroke, .. } => stroke.color == color,
            Shape::Vec(shapes) => shapes.iter().any(|shape| has_line(shape, color)),
            _ => false,
        }
    }
    let has_major = harness.output().shapes.iter().any(|shape| {
        has_line(&shape.shape, suhina_ui::kit::motion::mix(custom.window_background, custom.grid_major, 0.55))
    });
    theme::set_palette(&harness.ctx, previous);
    assert!(has_major, "timeline grid lines must follow their own theme token");
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
    theme::track_color_1()
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
    let original_colour = drums_colour(&harness);
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
    assert_eq!(drums_colour(&harness), original_colour, "one undo brings the old colour back");
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
    harness
        .get_by_label(&format!(
            "Colour {}",
            colour_picker::hex(suhina_ui::interaction::grid::to_rgb(theme::track_color_1()))
        ))
        .click();
    harness.run();
    assert!(!harness.state().session().unwrap().can_undo(), "no command for the same colour");
}

#[test]
fn return_defaults_to_compact_and_quick_expand_is_reversible() {
    let mut h = arrangement();
    let label = "Track A Reverb";
    let small = h.get_by_label(label).rect();
    assert!((small.height() - 20.0).abs() < 0.5, "return starts compact: {small:?}");
    h.get_by_label("Expand A Reverb").click();
    h.run();
    let expanded = h.get_by_label(label).rect();
    assert!((expanded.height() - 40.0).abs() < 0.5, "return expands: {expanded:?}");
    assert!(h.query_by_label("Show automation of A Reverb").is_some());
    h.get_by_label("Collapse A Reverb").click();
    h.run();
    assert!((h.get_by_label(label).rect().height() - 20.0).abs() < 0.5);
    assert!((h.get_by_label("Track Drums").rect().height() - 40.0).abs() < 0.5);
    h.get_by_label("Collapse Drums").click();
    h.run();
    assert!(h.query_by_label("Drums volume").is_some());
    assert!(h.query_by_label("Show automation of Drums").is_some());
    h.get_by_label("Expand Drums").click();
    h.run();
    assert!((h.get_by_label("Track Drums").rect().height() - 40.0).abs() < 0.5);
}

#[test]
fn tiny_rows_keep_controls_inside_the_header_and_tune_live() {
    use suhina_ui::settings::track_sizes::TrackSize;
    let mut h = arrangement();
    h.get_by_label("Resize all tracks").click();
    h.run();
    for size in TrackSize::ALL {
        assert!(h.query_by_label(size.name()).is_some());
    }
    h.get_by_label("Tiny").click();
    h.run();
    let header = h.get_by_label("Track Drums").rect();
    assert_eq!(header.height(), 14.0);
    assert_eq!(h.get_by_label("Track A Reverb").rect().height(), 14.0);
    for label in ["Show automation of Drums", "Drums volume", "Turn Drums off", "Expand Drums"] {
        let control = h.get_by_label(label).rect();
        assert!(header.contains_rect(control), "{label}: {control:?} outside {header:?}");
    }
    let mut settings = h.state().settings().clone();
    settings.view.track_sizes.set_height(TrackSize::Tiny, 17);
    let ctx = h.ctx.clone();
    h.state_mut().set_settings(&ctx, settings);
    h.run();
    assert_eq!(h.get_by_label("Track Drums").rect().height(), 17.0);
    h.get_by_label("Expand Drums").click();
    h.run();
    assert_eq!(h.get_by_label("Track Drums").rect().height(), 40.0);
    assert!(!h.state().session().unwrap().can_undo());
}

#[test]
fn custom_presets_apply_live_and_resize_all_includes_hidden_returns() {
    use suhina_ui::settings::track_sizes::TrackSize;
    let mut h = arrangement();
    let mut settings = h.state().settings().clone();
    settings.view.track_sizes.set_height(TrackSize::Small, 44);
    settings.view.track_sizes.set_height(TrackSize::Compact, 24);
    let ctx = h.ctx.clone();
    h.state_mut().set_settings(&ctx, settings);
    h.run();
    assert!((h.get_by_label("Track Drums").rect().height() - 44.0).abs() < 0.5);
    assert!((h.get_by_label("Track A Reverb").rect().height() - 24.0).abs() < 0.5);
    h.get_by_label("Hide returns").click();
    h.run();
    h.get_by_label("Resize all tracks").click();
    h.run();
    h.get_by_label("Medium").click();
    h.run();
    assert!((h.get_by_label("Track Drums").rect().height() - 64.0).abs() < 0.5);
    h.get_by_label("Show returns").click();
    h.run();
    assert!((h.get_by_label("Track A Reverb").rect().height() - 64.0).abs() < 0.5);
    assert!(!h.state().session().unwrap().can_undo(), "resizing never edits the music project");
}

#[test]
fn named_track_default_can_be_changed_from_settings_search() {
    let mut h = arrangement();
    // Explicit Resize all overrides must not block later changes to the default.
    h.get_by_label("Resize all tracks").click();
    h.run();
    h.get_by_label("Large").click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::Comma);
    h.run();
    h.get_by_label("Search settings").focus();
    h.run();
    h.get_by_label("Search settings").type_text("default track size");
    h.run();
    assert!(h.query_by_label("Advanced track sizes: On").is_some(), "search keeps Advanced visible");
    assert!(h.query_by_label("Medium height").is_some(), "search keeps size tuning visible");
    h.get_by_label("Default track size: Medium").click();
    h.run();
    assert_eq!(h.state().settings().view.track_sizes.tracks, suhina_ui::settings::track_sizes::TrackSize::Medium);
    assert_eq!(
        h.state().arrangement().lane_height(suhina_engine::model::TrackId(1)),
        64.0,
        "default applies live before closing Settings, even after Resize all"
    );
    h.get_by_label("Advanced track sizes: On").click();
    h.run();
    h.get_by_label("Medium height").click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run();
    h.input_mut().events.push(egui::Event::Text("78".into()));
    h.run();
    h.key_press(Key::Enter);
    h.run();
    let tuned = h.state().settings().view.track_sizes.height(suhina_ui::settings::track_sizes::TrackSize::Medium);
    assert_eq!(tuned, 78, "the user can type an exact height in Advanced");
    assert_eq!(
        h.state().arrangement().lane_height(suhina_engine::model::TrackId(1)),
        f32::from(tuned),
        "height tuning applies live without closing Settings"
    );
    h.get_by_label("Close settings").click();
    h.run();
    assert!((h.get_by_label("Track Drums").rect().height() - f32::from(tuned)).abs() < 0.5);
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
