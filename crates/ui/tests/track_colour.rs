// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Track colours in the window: the Arrangement header's colour band, the Launcher column's
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
use suhina_ui::kit::selection::{SelectionColour, SelectionLook};
use suhina_ui::{App, interaction::colour_picker, kit::theme};

/// The colour variable `variable` (a name without `--`) as the window's root resolved it in the
/// last frame: the theme in use's colour.
#[allow(clippy::expect_used)] // a test helper outside #[test] functions
fn live(variable: &str) -> egui::Color32 {
    suhina_ui::kit::native_css::root_color(&format!("--{variable}"))
        .or_else(|| suhina_ui::kit::theme::default_color(variable))
        .expect("a colour variable")
}

const SIZE: (f32, f32) = (1280.0, 900.0);

fn window() -> Harness<'static, App> {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size(SIZE)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness
}

/// Draws the window in `colors` from the next frame: the user's sheet sets them, as the app gives
/// each frame the theme's own colours.
fn use_colours(colors: &[(&str, Color32)]) {
    let colors: theme::Colors = colors.iter().map(|(variable, color)| ((*variable).to_owned(), *color)).collect();
    let rule = theme::colors_rule(&colors).unwrap_or_default();
    suhina_ui::kit::native_css::set_user_sheet(Some(rule.into()));
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
    let (header_background, header_selected, control_selected) =
        (Color32::from_rgb(31, 45, 53), Color32::from_rgb(65, 72, 84), Color32::from_rgb(233, 229, 222));
    use_colours(&[
        ("track-headerBackground", header_background),
        ("track-headerSelectedBackground", header_selected),
        ("control-selectedBackground", control_selected),
    ]);
    harness.run();
    let header = harness.get_by_label("Track Drums").rect();
    let color = default_drums();
    let has_surface = filled_with(&harness, header, suhina_ui::kit::motion::mix(header_background, color, 0.82))
        || filled_with(
            &harness,
            header,
            suhina_ui::kit::motion::mix(
                suhina_ui::kit::motion::mix(header_selected, color, 0.82),
                live("text-primary"),
                0.07,
            ),
        );
    let neutral_volumes = ["Drums volume", "Bass volume", "A Reverb volume"]
        .into_iter()
        .all(|label| filled_with(&harness, harness.get_by_label(label).rect(), control_selected));
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert!(has_surface, "the header must tint the editable theme surface with its track color");
    assert!(neutral_volumes, "volume fills share the neutral theme colour regardless of track colour");
}

/// With Settings › Appearance › Selection colour › Clip's own colour, a selected header keeps its
/// track's colour on the theme's selected surface, as before there was one selection colour.
#[test]
fn with_clips_own_colour_a_selected_header_keeps_its_theme_surface_and_track_colour() {
    let mut harness = arrangement();
    selection_colour(&mut harness, SelectionColour::Own, Color32::BLACK);
    let before = harness.state().session().unwrap().project().clone();
    harness.get_by_label("Track Drums").click();
    harness.run();
    let header_selected = Color32::from_rgb(180, 35, 110);
    use_colours(&[("track-headerSelectedBackground", header_selected)]);
    harness.run();
    let header = harness.get_by_label("Track Drums").rect();
    let expected = suhina_ui::kit::motion::mix(
        suhina_ui::kit::motion::mix(header_selected, default_drums(), 0.82),
        live("text-primary"),
        0.07,
    );
    let shown = filled_with(&harness, header, expected);
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert!(shown, "selected header must use track.headerSelectedBackground");
    assert_eq!(harness.state().session().unwrap().project(), &before);
}

#[test]
fn arrangement_grid_uses_its_own_theme_colors() {
    let mut harness = arrangement();
    let (grid_major, grid_minor) = (Color32::from_rgb(117, 83, 149), Color32::from_rgb(53, 79, 101));
    use_colours(&[("grid-major", grid_major), ("grid-minor", grid_minor)]);
    harness.run();
    fn has_line(shape: &Shape, color: Color32) -> bool {
        match shape {
            Shape::LineSegment { stroke, .. } => stroke.color == color,
            Shape::Vec(shapes) => shapes.iter().any(|shape| has_line(shape, color)),
            _ => false,
        }
    }
    let has_major =
        harness.output().shapes.iter().any(|shape| {
            has_line(&shape.shape, suhina_ui::kit::motion::mix(live("window-background"), grid_major, 0.55))
        });
    suhina_ui::kit::native_css::set_user_sheet(None);
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

/// The demo's first track has no colour of its own, so it shows Suhina's first legacy colour.
fn default_drums() -> Color32 {
    colour_picker::color32(suhina_session::colours::LEGACY[0])
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
    harness.get_by_label(&format!("Colour {}", colour_picker::hex(suhina_session::colours::LEGACY[0]))).click();
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
    assert!(h.query_by_label("Exact track heights").is_some(), "search keeps Advanced visible");
    assert!(h.query_by_label("Medium height").is_some(), "search keeps size tuning visible");
    h.get_by_label("Default track size: Medium").click();
    h.run();
    assert_eq!(h.state().settings().view.track_sizes.tracks, suhina_ui::settings::track_sizes::TrackSize::Medium);
    assert_eq!(
        h.state().arrangement().lane_height(suhina_engine::model::TrackId(1)),
        64.0,
        "default applies live before closing Settings, even after Resize all"
    );
    h.get_by_label("Exact track heights").click();
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

/// The MIDI monitoring chip stays inside its header and clear of the automation button: at the
/// default width and at the narrowest both take their own clicks, and a low lane at the narrowest,
/// where the chip's row has no room for it, drops the chip (#876).
#[test]
fn the_midi_monitor_chip_keeps_clear_of_the_automation_button() {
    let mut h = arrangement();
    let clear = |h: &Harness<'static, App>| {
        let lanes = h.get_by_label("Show automation of Drums").rect();
        let header = h.get_by_label("Track Drums").rect();
        assert!(header.contains_rect(lanes), "{lanes:?} in {header:?}");
        h.query_by_label("MIDI monitoring of Drums: Auto").map(|chip| {
            let chip = chip.rect();
            assert!(!chip.intersects(lanes) && header.contains_rect(chip), "{chip:?} {lanes:?}");
        })
    };
    let chip_takes_its_click = |h: &mut Harness<'static, App>| {
        h.get_by_label("MIDI monitoring of Drums: Auto").click();
        h.run();
        assert!(h.query_by_label("Always on").is_some(), "the chip takes its click");
        h.key_press(Key::Escape);
        h.run();
    };
    assert!(clear(&h).is_some(), "the default width shows the chip");
    chip_takes_its_click(&mut h);
    // With the headers at the right, dragging their left edge right narrows them to the least.
    let mut settings = h.state().settings().clone();
    settings.track_headers = Some(suhina_ui::settings::Side::Right);
    let ctx = h.ctx.clone();
    h.state_mut().set_settings(&ctx, settings);
    h.run();
    let ruler = h.get_by_label("Ruler").rect();
    let edge = egui::pos2(ruler.right(), ruler.bottom() + 30.0);
    h.hover_at(edge);
    h.run();
    h.drag_at(edge);
    h.hover_at(edge + egui::vec2(300.0, 0.0));
    h.run();
    h.drop_at(edge + egui::vec2(300.0, 0.0));
    h.run();
    assert_eq!(h.state().arrangement().header_width, 160.0);
    assert!(clear(&h).is_some(), "a two-row lane has room for the chip at the narrowest");
    chip_takes_its_click(&mut h);
    h.get_by_label("Resize all tracks").click();
    h.run();
    h.get_by_label("Tiny").click();
    h.run();
    assert!(clear(&h).is_none(), "a low lane at the narrowest drops the chip");
    h.get_by_label("Show automation of Drums").click();
    h.run();
    assert!(h.query_by_label("Hide automation of Drums").is_some(), "the button takes its click");
}

/// The "+" in a track's menu opens a colour field: dragging in it keeps the menu open, and the
/// colour is set once, as one undo step, when the button comes up (#712).
#[test]
fn a_colour_dragged_in_the_menus_picker_keeps_the_menu_open_and_is_one_step() {
    let mut harness = arrangement();
    let before = drums_colour(&harness);
    harness.get_by_label("Track Drums").click_secondary();
    harness.run();
    harness.get_by_label("Any colour").click();
    harness.run();
    // The saturation and value field, under the row of swatches and the colour's own row, from
    // the swatches' left edge.
    let first = harness.get_by_label(&format!("Colour {}", colour_picker::hex(colour_picker::swatches()[0]))).rect();
    let from = egui::pos2(first.left() + 30.0, first.bottom() + 80.0);
    let to = from + egui::vec2(40.0, 20.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.step();
    for step in 1..=4 {
        harness.hover_at(from + (to - from) * (step as f32 / 4.0));
        harness.step();
    }
    assert!(harness.query_by_label("Any colour").is_some(), "the menu stays open while the colour is dragged");
    assert_eq!(drums_colour(&harness), before, "nothing is set before the button comes up");
    harness.drop_at(to);
    harness.step();
    harness.run();
    let picked = drums_colour(&harness);
    assert_ne!(picked, before, "the colour dragged to is set");
    assert!(harness.query_by_label("Any colour").is_some(), "the menu stays open when the button comes up");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(drums_colour(&harness), before, "one undo takes the whole drag back");
}

/// The same in the palette the colour area opens: the palette stays open when the button comes up.
#[test]
fn a_colour_dragged_in_the_colour_areas_picker_keeps_the_palette_open_and_is_one_step() {
    let mut harness = arrangement();
    let before = drums_colour(&harness);
    harness.get_by_label("Drums colour").click();
    harness.run();
    harness.get_by_label("Any colour").click();
    harness.run();
    // The saturation and value field, under the row of swatches and the colour's own row, from
    // the swatches' left edge.
    let first = harness.get_by_label(&format!("Colour {}", colour_picker::hex(colour_picker::swatches()[0]))).rect();
    let from = egui::pos2(first.left() + 30.0, first.bottom() + 80.0);
    let to = from + egui::vec2(40.0, 20.0);
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.step();
    for step in 1..=4 {
        harness.hover_at(from + (to - from) * (step as f32 / 4.0));
        harness.step();
    }
    assert!(harness.query_by_label("Any colour").is_some(), "the menu stays open while the colour is dragged");
    assert_eq!(drums_colour(&harness), before, "nothing is set before the button comes up");
    harness.drop_at(to);
    harness.step();
    harness.run();
    let picked = drums_colour(&harness);
    assert_ne!(picked, before, "the colour dragged to is set");
    assert!(harness.query_by_label("Any colour").is_some(), "the menu stays open when the button comes up");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(drums_colour(&harness), before, "one undo takes the whole drag back");
}

/// A padded track header still folds and resizes by the whole lane: collapsing and expanding
/// gives back the lane's own height, not its content box's, and the resize edge is on the line
/// between lanes, not inside the padding (#1336).
#[test]
fn a_padded_header_folds_back_to_its_height_and_resizes_from_its_edge() {
    suhina_ui::kit::native_css::set_user_sheet(Some("track-header { padding: 3px 0 8px; }".into()));
    let mut h = arrangement();
    for _ in 0..2 {
        h.get_by_label("Collapse Drums").click();
        h.run();
        h.get_by_label("Expand Drums").click();
        h.run();
    }
    let header = h.get_by_label("Track Drums").rect();
    assert_eq!(header.height(), 40.0, "folding twice keeps the lane's height");
    let from = egui::pos2(header.center().x, header.bottom() - 1.0);
    h.hover_at(from);
    h.run();
    h.drag_at(from);
    h.run();
    h.hover_at(from + egui::vec2(0.0, 10.0));
    h.run();
    h.hover_at(from + egui::vec2(0.0, 20.0));
    h.run();
    h.drop_at(from + egui::vec2(0.0, 20.0));
    h.run();
    let resized = h.get_by_label("Track Drums").rect().height();
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert!(resized > header.height() + 10.0, "the lane's own edge resizes it: {resized}");
}

/// The Arrangement with a theme whose only rules are `css`, in use.
fn arrangement_with_theme(css: &str) -> Harness<'static, App> {
    let (lanes, warnings) = theme::from_folder(theme::ThemeMeta::new("Lanes", "#123456"), css);
    assert!(warnings.is_empty(), "{warnings:?}");
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(demo_project()), &[]);
    let ctx = egui::Context::default();
    app.set_themes(&ctx, vec![suhina_ui::settings::themes::ThemeEntry { id: "lanes".into(), theme: lanes }], None);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .with_size(SIZE)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    let ctx = harness.ctx.clone();
    harness.state_mut().use_theme(&ctx, "lanes");
    harness.run();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness
}

#[test]
fn a_theme_colours_a_track_s_lane_from_its_color_attribute() {
    // A colour no other part of the window uses, mixed from the track's own.
    let harness = arrangement_with_theme(
        "track-lane { background-color: color-mix(in srgb, attr(color type(<color>)), #000000 50%); }",
    );
    let header = harness.get_by_label("Track Drums").rect();
    let own = suhina_session::colours::LEGACY[0];
    let half = |c: u8| c / 2 + (c % 2);
    let near = |a: u8, b: u8| a.abs_diff(b) <= 1;
    let lane = filled_rects(&harness).into_iter().find(|r| {
        let [red, green, blue, _] = r.fill.to_srgba_unmultiplied();
        near(red, half(own.0))
            && near(green, half(own.1))
            && near(blue, half(own.2))
            && r.rect.left() >= header.right() - 1.0
            && (r.rect.center().y - header.center().y).abs() < header.height() / 2.0
    });
    let lane = lane.unwrap_or_else(|| {
        let near_row: Vec<_> = filled_rects(&harness)
            .into_iter()
            .filter(|r| {
                r.rect.left() >= header.right() - 1.0
                    && (r.rect.center().y - header.center().y).abs() < header.height() / 2.0
            })
            .map(|r| (r.rect, r.fill))
            .collect();
        panic!("Drums' lane is filled with its colour mixed with black: {header:?} {near_row:?}")
    });
    assert!(lane.rect.width() > 400.0, "the whole row: {:?}", lane.rect);
}

/// Whether the last frame drew a filled rectangle of `colour` over `area` that nothing clips off it.
fn drawn_unclipped(harness: &Harness<'static, App>, area: Rect, colour: Color32) -> bool {
    harness.output().shapes.iter().any(|clipped| {
        let mut rects = Vec::new();
        let mut stack = vec![&clipped.shape];
        while let Some(shape) = stack.pop() {
            match shape {
                Shape::Rect(r) => rects.push(r.clone()),
                Shape::Vec(v) => stack.extend(v),
                _ => {}
            }
        }
        clipped.clip_rect.expand(0.5).contains_rect(area)
            && rects.iter().any(|r| r.fill == colour && r.rect.expand(0.5).contains_rect(area))
    })
}

#[test]
fn a_lane_that_hides_its_overflow_leaves_the_header_and_the_automation_lanes_alone() {
    let mut harness = arrangement_with_theme("track-lane { overflow: hidden; }");
    let band = harness.get_by_label("Drums colour").rect();
    assert!(drawn_unclipped(&harness, band, default_drums()), "the header's band is not cut to the timeline");
    harness.get_by_label("Show automation of Drums").click();
    harness.run();
    let band = harness.get_by_label("Drums colour").rect();
    assert!(drawn_unclipped(&harness, band, default_drums()), "nor with an automation lane open");
    let lane = harness.get_by_label("Volume automation on Drums").rect();
    let cut = harness
        .output()
        .shapes
        .iter()
        .any(|c| c.shape.visual_bounding_rect().intersects(lane) && !c.clip_rect.expand(0.5).contains_rect(lane));
    assert!(!cut, "the automation lane's header is not cut either");
}

/// Sets Settings › Appearance › Selection colour to `mode`, with `custom` for Custom.
fn selection_colour(harness: &mut Harness<'static, App>, mode: SelectionColour, custom: Color32) {
    let ctx = harness.ctx.clone();
    let mut settings = harness.state().settings().clone();
    settings.view.selection = SelectionLook { mode, custom };
    harness.state_mut().set_settings(&ctx, settings);
    harness.run();
}

/// A click on what is labelled `label`, with `modifiers` held (Ctrl adds to the selection): at its
/// centre, or 6 points under its top for a clip's name bar, which picks the clip whole.
fn pick(harness: &mut Harness<'static, App>, label: &str, name_bar: bool, modifiers: Modifiers) {
    let rect = harness.get_by_label(label).rect();
    let at = if name_bar { rect.center_top() + egui::vec2(0.0, 6.0) } else { rect.center() };
    harness.event(egui::Event::ModifiersChanged(modifiers));
    harness.hover_at(at);
    harness.run();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers });
        harness.run();
    }
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    harness.run();
}

/// The opaque fill of the rectangle the last frame drew over exactly `area`.
fn fill_of(harness: &Harness<'static, App>, area: Rect) -> Option<Color32> {
    filled_rects(harness)
        .into_iter()
        .filter(|r| r.fill.a() == 255)
        .find(|r| (r.rect.min - area.min).length() < 0.6 && (r.rect.max - area.max).length() < 0.6)
        .map(|r| r.fill)
}

/// A colour no theme uses, for the selection.
const PICKED: Color32 = Color32::from_rgb(0xC8, 0x28, 0x3C);

/// Two selected headers of different colours and two selected clips of different colours are all
/// filled with the theme's `selection.background`, a clip with the very colour of a header (#1217).
#[test]
fn selected_headers_and_clips_share_the_themes_selection_colour() {
    let mut harness = arrangement();
    use_colours(&[("selection-background", PICKED)]);
    harness.run();
    pick(&mut harness, "Track Drums", false, Modifiers::NONE);
    pick(&mut harness, "Track Bass", false, Modifiers::COMMAND);
    let headers = ["Track Drums", "Track Bass"].map(|label| harness.get_by_label(label).rect());
    let headers_filled = headers.iter().all(|header| filled_with(&harness, *header, PICKED));
    let (beat, walk) = ("Clip Beat B on Drums at bar 17", "Clip Walk on Bass at bar 17");
    pick(&mut harness, beat, true, Modifiers::NONE);
    pick(&mut harness, walk, true, Modifiers::COMMAND);
    let clips = [beat, walk].map(|label| fill_of(&harness, harness.get_by_label(label).rect()));
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert!(headers_filled, "both selected headers take selection.background");
    assert_eq!(clips, [Some(PICKED), Some(PICKED)], "both selected clips take a header's fill");
}

/// A clip picked by a click in its body, which sets the play start there, is filled with the
/// selection colour as one picked by its name bar is, while its name bar keeps the clip's own
/// colour.
#[test]
fn a_clip_picked_in_its_body_takes_the_selection_colour_and_its_name_bar_keeps_its_own() {
    let mut harness = arrangement();
    use_colours(&[("selection-background", PICKED)]);
    harness.run();
    let beat = "Clip Beat B on Drums at bar 17";
    let before = fill_of(&harness, harness.get_by_label(beat).rect());
    pick(&mut harness, beat, false, Modifiers::NONE);
    let rect = harness.get_by_label(beat).rect();
    let body = fill_of(&harness, rect);
    // The name bar along the clip's top: a fill over the clip's width, not the selection colour.
    let bar = filled_rects(&harness).into_iter().any(|r| {
        r.fill.a() == 255
            && r.fill != PICKED
            && (r.rect.top() - rect.top()).abs() < 1.0
            && r.rect.height() < rect.height() / 2.0
            && r.rect.width() > rect.width() * 0.8
    });
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert_ne!(before, Some(PICKED), "not selected before the click");
    assert_eq!(body, Some(PICKED), "a click in the body selects the clip and fills it");
    assert!(bar, "the name bar keeps the clip's own colour");
}

/// With Clip's own colour, two selected clips of different colours keep different fills (#1218).
#[test]
fn with_clips_own_colour_two_selected_clips_keep_their_own_colours() {
    let mut harness = arrangement();
    selection_colour(&mut harness, SelectionColour::Own, PICKED);
    let (beat, walk) = ("Clip Beat B on Drums at bar 17", "Clip Walk on Bass at bar 17");
    pick(&mut harness, beat, true, Modifiers::NONE);
    pick(&mut harness, walk, true, Modifiers::COMMAND);
    let [a, b] = [beat, walk].map(|label| fill_of(&harness, harness.get_by_label(label).rect()));
    assert!(a.is_some() && b.is_some(), "{a:?} {b:?}");
    assert_ne!(a, b, "each keeps its own colour");
    let theirs = live("selection-background");
    assert!(a != Some(theirs) && b != Some(theirs), "neither takes the selection colour");
}

/// A custom selection colour fills selected headers and clips in both views at once, and From
/// theme gives the theme's back (#1218).
#[test]
fn a_custom_selection_colour_fills_headers_and_clips_and_from_theme_gives_the_themes_back() {
    let mut harness = arrangement();
    selection_colour(&mut harness, SelectionColour::Custom, PICKED);
    pick(&mut harness, "Track Drums", false, Modifiers::NONE);
    let header = harness.get_by_label("Track Drums").rect();
    assert!(filled_with(&harness, header, PICKED), "the header takes the chosen colour");
    let beat = "Clip Beat B on Drums at bar 17";
    pick(&mut harness, beat, true, Modifiers::NONE);
    assert_eq!(fill_of(&harness, harness.get_by_label(beat).rect()), Some(PICKED), "the clip too");
    selection_colour(&mut harness, SelectionColour::Theme, PICKED);
    assert_eq!(fill_of(&harness, harness.get_by_label(beat).rect()), Some(live("selection-background")));

    let mut launcher = window();
    selection_colour(&mut launcher, SelectionColour::Custom, PICKED);
    pick(&mut launcher, "Track Drums", false, Modifiers::NONE);
    pick(&mut launcher, "Track Bass", false, Modifiers::COMMAND);
    for label in ["Track Drums", "Track Bass"] {
        let header = launcher.get_by_label(label).rect();
        assert!(filled_with(&launcher, header, PICKED), "{label} in the Launcher takes the chosen colour");
    }
}

/// The rectangles the last frame filled with a light tint of `colour`: its own hue, mostly see-through.
fn tinted_with(harness: &Harness<'static, App>, colour: Color32) -> Vec<Rect> {
    let [r, g, b, _] = colour.to_srgba_unmultiplied();
    filled_rects(harness)
        .into_iter()
        .filter(|shape| {
            let [fr, fg, fb, fa] = shape.fill.to_srgba_unmultiplied();
            let near = |a: u8, b: u8| a.abs_diff(b) <= 8;
            (1..=96).contains(&fa) && near(fr, r) && near(fg, g) && near(fb, b)
        })
        .map(|shape| shape.rect)
        .collect()
}

/// A theme's background for every Launcher column stays under the slots when a column is
/// selected: it is not drawn over the header, which keeps the selection colour (#1524 review).
#[test]
fn a_themes_column_background_does_not_cover_a_selected_column() {
    let mut harness = window();
    let column = Color32::from_rgb(0x20, 0x20, 0x20);
    suhina_ui::kit::native_css::set_user_sheet(Some(
        format!(
            ":root {{ --selection-background: {}; }} track-column {{ background-color: #202020; }}",
            theme::hex(PICKED)
        )
        .into(),
    ));
    harness.run();
    pick(&mut harness, "Track Drums", false, Modifiers::NONE);
    let header = harness.get_by_label("Track Drums").rect();
    let covered = filled_rects(&harness).iter().any(|r| {
        let over = r.rect.intersect(header);
        r.fill == column && over.width() > 1.0 && over.height() > 1.0
    });
    let filled = filled_with(&harness, header, PICKED);
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert!(!covered, "the column's background is not drawn over the selected header");
    assert!(filled, "the selected header keeps the selection colour");
}

/// Only the selected tracks' lanes are tinted with the selection colour, and the tint follows a
/// change of selection (#1217).
#[test]
fn only_the_selected_tracks_lanes_are_tinted_and_the_tint_follows_the_selection() {
    let mut harness = arrangement();
    use_colours(&[("selection-background", PICKED)]);
    harness.run();
    let rows = |harness: &Harness<'static, App>| {
        ["Track Drums", "Track Bass"].map(|label| harness.get_by_label(label).rect().y_range())
    };
    let on = |harness: &Harness<'static, App>, row: egui::Rangef| {
        tinted_with(harness, PICKED).iter().any(|r| r.y_range().intersection(row).span() > 4.0)
    };
    pick(&mut harness, "Track Drums", false, Modifiers::NONE);
    let [drums, bass] = rows(&harness);
    let first = (on(&harness, drums), on(&harness, bass));
    pick(&mut harness, "Track Bass", false, Modifiers::NONE);
    let [drums, bass] = rows(&harness);
    let then = (on(&harness, drums), on(&harness, bass));
    suhina_ui::kit::native_css::set_user_sheet(None);
    assert_eq!(first, (true, false), "Drums' lane alone is tinted");
    assert_eq!(then, (false, true), "the tint follows the selection to Bass");
}
