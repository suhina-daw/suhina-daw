// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn a_dialog_panel_takes_only_its_own_background() {
    let token = egui::Color32::from_rgb(1, 2, 3);
    assert_eq!(own_background_or(token), token);
    let window = egui::Color32::from_rgb(200, 200, 200);
    // An enclosing window's colour is not the dialog's own.
    assert_eq!(with_surface(window, || own_background_or(token)), token);
}

#[test]
fn a_dialog_panel_in_a_window_scope_takes_the_dialog_s_own_background_or_the_token() {
    use crate::kit::element::{DIALOG, WINDOW};
    let token = egui::Color32::from_rgb(1, 2, 3);
    let bare = frames("window { background-color: #c8c8c8; }", || {
        let _window = WINDOW.enter();
        let _dialog = DIALOG.enter();
        own_background_or(token)
    });
    assert_eq!(bare, token, "an authored window is not the dialog's");
    let styled = frames("window { background-color: #c8c8c8; } dialog { background-color: #102030; }", || {
        let _window = WINDOW.enter();
        let _dialog = DIALOG.enter();
        own_background_or(token)
    });
    assert_eq!(styled, egui::Color32::from_rgb(0x10, 0x20, 0x30));
}

#[test]
fn a_modal_takes_its_regions_surface_only_when_it_is_opaque() {
    let token = egui::Color32::from_rgb(1, 2, 3);
    let opaque = egui::Color32::from_rgb(10, 20, 30);
    assert_eq!(opaque_surface_or(token), token, "no surface");
    assert_eq!(with_surface(opaque, || opaque_surface_or(token)), opaque);
    let clear = egui::Color32::from_rgba_unmultiplied(10, 20, 30, 120);
    assert_eq!(with_surface(clear, || opaque_surface_or(token)), token, "a see-through surface hides nothing");
    assert_eq!(with_surface(egui::Color32::TRANSPARENT, || opaque_surface_or(token)), token);
}

use crate::kit::element::{FADER, KNOB, MIXER, PAN, SEND, test_el};
use crate::kit::skin;

/// What `draw` sees once the frames it takes for the tree to resolve have passed, with `css` as
/// the theme's sheet.
fn frames<R>(css: &str, mut draw: impl FnMut() -> R) -> R {
    skin::configure(&skin::Skin { css: Some(css.into()), ..Default::default() });
    let ctx = egui::Context::default();
    let mut result = None;
    for _ in 0..3 {
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let _frame = super::super::native_css::frame_with_motion(ui, false);
            result = Some(draw());
        })
        .textures_delta
        .clear();
    }
    skin::configure(&skin::Skin::default());
    result.unwrap()
}

#[test]
fn settings_inherit_and_the_nearest_element_s_wins() {
    let css = "track-column { track-artwork: on; } track-column clip { track-artwork: off; }";
    let seen = frames(css, || {
        let column = test_el("track-column").enter();
        let slot = test_el("slot").enter();
        let clip = test_el("clip").enter();
        (track_artwork(&column), track_artwork(&slot), track_artwork(&clip))
    });
    assert_eq!(seen, (true, true, false));
}

#[test]
fn a_pan_s_appearance_is_its_own_and_reaches_no_other_control() {
    for (value, forced) in [("knob", Some(true)), ("field", Some(false)), ("native", None)] {
        let css = format!("mixer pan {{ appearance: {value}; }}");
        let seen = frames(&css, || {
            let _mixer = MIXER.enter();
            let pan = PAN.enter();
            let own = [false, true].map(|preference| knob_preference(&pan, preference));
            drop(pan);
            let send = SEND.enter();
            (own, [false, true].map(|preference| knob_preference(&send, preference)))
        });
        assert_eq!(seen.0, [false, true].map(|preference| forced.unwrap_or(preference)), "{value}");
        assert_eq!(seen.1, [false, true], "a send keeps the user's choice");
    }
}

#[test]
fn drag_pointer_overrides_the_setting_for_the_controls_inside_it() {
    for (value, forced) in [("hidden", Some(true)), ("visible", Some(false)), ("auto", None)] {
        let css = format!("mixer {{ drag-pointer: {value}; }}");
        let seen = frames(&css, || {
            let mixer = MIXER.enter();
            let fader = FADER.enter();
            let inside = [false, true].map(|setting| drag_hides_pointer(&fader, setting));
            drop(fader);
            drop(mixer);
            let knob = KNOB.enter();
            (inside, [false, true].map(|setting| drag_hides_pointer(&knob, setting)))
        });
        assert_eq!(seen.0, [false, true].map(|setting| forced.unwrap_or(setting)), "{value}");
        assert_eq!(seen.1, [false, true], "a knob outside the mixer keeps the setting");
    }
}

#[test]
fn a_live_theme_change_survives_all_enclosing_style_scopes() {
    let original = theme::palette();
    let mut changed = original;
    changed.text_primary = egui::Color32::from_rgb(40, 50, 60);
    changed.grid_major = egui::Color32::from_rgba_unmultiplied(80, 90, 100, 120);
    let ctx = egui::Context::default();
    let inside = frames("browser { color: #123456; }", || {
        let _window = test_el("window").enter();
        let _browser = test_el("browser").enter();
        let inside = theme::text_primary();
        theme::use_palette(&ctx, changed);
        inside
    });
    assert_eq!(inside, theme::parse_hex("#123456").unwrap());
    assert_eq!(theme::palette(), changed);
    theme::use_palette(&ctx, original);
}

#[test]
fn ordinary_widget_style_preserves_ui_identity_and_restores_caller() {
    let css = "settings { color: #123456; font-size: 19px; }";
    skin::configure(&skin::Skin { css: Some(css.into()), ..Default::default() });
    let context = egui::Context::default();
    context
        .run_ui(egui::RawInput::default(), |ui| {
            let _frame = super::super::native_css::frame_with_motion(ui, false);
            let original = ui.style().clone();
            let original_clip = ui.clip_rect();
            let id = ui.scope_id();
            {
                let scope = crate::kit::element::test_el("settings").enter();
                let limited = egui::Rect::from_min_size(ui.max_rect().min, egui::Vec2::splat(16.0));
                let styled = scope.ui(ui).clipped_to(limited);
                assert_eq!(styled.scope_id(), id);
                assert_eq!(styled.clip_rect(), original_clip.intersect(limited));
                assert_eq!(styled.visuals().override_text_color, theme::parse_hex("#123456"));
                assert!(styled.style().text_styles.values().all(|font| font.size == 19.0));
            }
            assert!(std::sync::Arc::ptr_eq(ui.style(), &original));
            assert_eq!(ui.clip_rect(), original_clip);
        })
        .textures_delta
        .clear();
    skin::configure(&skin::Skin::default());
}
#[test]
fn descendant_styles_are_scoped_and_restore_palette() {
    let original = theme::palette();
    let seen = frames("browser { color: #123456; } browser button { color: #ABCDEF; }", || {
        let browser = test_el("browser").enter();
        let in_browser = theme::text_primary();
        let in_button = {
            let _button = test_el("button").enter();
            theme::text_primary()
        };
        let after = theme::text_primary();
        drop(browser);
        (in_browser, in_button, after, theme::palette())
    });
    assert_eq!(seen.0, theme::parse_hex("#123456").unwrap());
    assert_eq!(seen.1, theme::parse_hex("#ABCDEF").unwrap());
    assert_eq!(seen.2, seen.0);
    assert_eq!(seen.3, original);
}

// The root's `Visuals` come from the sheet (#1442).

/// egui's `Visuals` at the root after frames of a window `width` wide in `ctx`, with `css` as the
/// theme's sheet.
fn root_after(ctx: &egui::Context, css: &str, width: f32) -> egui::Visuals {
    skin::configure(&skin::Skin { css: Some(css.into()), ..Default::default() });
    frames_in(ctx, width)
}

fn frames_in(ctx: &egui::Context, width: f32) -> egui::Visuals {
    for _ in 0..3 {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 600.0))),
                ..Default::default()
            },
            |ui| {
                let _frame = super::super::native_css::frame_with_motion(ui, false);
            },
        )
        .textures_delta
        .clear();
    }
    ctx.global_style().visuals.clone()
}

fn rgb(r: u8, g: u8, b: u8) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}

#[test]
fn a_rule_for_each_element_reaches_the_root_visuals() {
    let css = "button { background-color: #101010; color: #111111; border-color: #121212; border-width: 2px; border-radius: 3px; } \
               button:hover { background-color: #202020; } \
               button:active { background-color: #303030; } \
               button:disabled { background-color: #404040; } \
               button[checked] { background-color: #505050; } \
               text-input { background-color: #060606; } \
               text-input:focus { border-color: #070707; border-width: 2px; } \
               menu { background-color: #080808; border-color: #090909; border-width: 3px; border-radius: 5px; box-shadow: 2px 3px 8px #0a0a0a; } \
               scroll-bar { background-color: #0b0b0b; } \
               scroll-bar:hover { background-color: #0c0c0c; }";
    let ctx = egui::Context::default();
    let v = root_after(&ctx, css, 800.0);
    assert_eq!(v.widgets.inactive.bg_fill, rgb(0x0b, 0x0b, 0x0b), "a scroll handle's fill is the later rule's");
    assert_eq!(v.widgets.inactive.weak_bg_fill, rgb(0x10, 0x10, 0x10));
    assert_eq!(v.widgets.inactive.fg_stroke.color, rgb(0x11, 0x11, 0x11));
    assert_eq!(v.widgets.inactive.bg_stroke, egui::Stroke::new(2.0, rgb(0x12, 0x12, 0x12)));
    assert_eq!(v.widgets.inactive.corner_radius, egui::CornerRadius::same(3));
    assert_eq!(v.widgets.hovered.weak_bg_fill, rgb(0x20, 0x20, 0x20));
    assert_eq!(v.widgets.hovered.bg_fill, rgb(0x0c, 0x0c, 0x0c));
    assert_eq!(v.widgets.active.weak_bg_fill, rgb(0x30, 0x30, 0x30));
    assert_eq!(v.widgets.open.weak_bg_fill, rgb(0x30, 0x30, 0x30), "an open button is a pressed one");
    assert_eq!(v.widgets.noninteractive.weak_bg_fill, rgb(0x40, 0x40, 0x40));
    assert_eq!(v.selection.bg_fill, rgb(0x50, 0x50, 0x50));
    assert_eq!(v.extreme_bg_color, rgb(6, 6, 6));
    assert_eq!(v.text_edit_bg_color, Some(rgb(6, 6, 6)));
    assert_eq!(v.selection.stroke, egui::Stroke::new(2.0, rgb(7, 7, 7)), "a focused field's outline");
    assert_eq!(v.window_fill, rgb(8, 8, 8));
    assert_eq!(v.window_stroke, egui::Stroke::new(3.0, rgb(9, 9, 9)));
    assert_eq!(v.menu_corner_radius, egui::CornerRadius::same(5));
    assert_eq!(
        v.popup_shadow,
        egui::epaint::Shadow { offset: [2, 3], blur: 8, spread: 0, color: egui::Color32::from_rgb(10, 10, 10) }
    );
    // What no rule says stays the palette's.
    let fallback = theme::palette_visuals_for(&theme::palette());
    assert_eq!(v.panel_fill, fallback.panel_fill);
    assert_eq!(v.faint_bg_color, fallback.faint_bg_color);
    assert_eq!(v.widgets.hovered.fg_stroke.width, fallback.widgets.hovered.fg_stroke.width);
    skin::configure(&skin::Skin::default());
}

#[test]
fn a_rule_that_matches_only_below_a_width_reaches_the_root_visuals_after_the_reset() {
    let css = "@media (max-width: 700px) { button { background-color: #112233; } }";
    let ctx = egui::Context::default();
    let wide = root_after(&ctx, css, 800.0);
    assert_eq!(wide, theme::palette_visuals_for(&theme::palette()), "wide, nothing is said");
    let narrow = frames_in(&ctx, 500.0);
    assert_eq!(narrow.widgets.inactive.weak_bg_fill, rgb(0x11, 0x22, 0x33));
    assert_eq!(frames_in(&ctx, 800.0), wide, "the rule goes with the width");
    skin::configure(&skin::Skin::default());
}

#[test]
fn a_palette_change_such_as_the_window_s_opacity_reaches_the_root_visuals() {
    let saved = theme::palette();
    let ctx = egui::Context::default();
    for css in ["", "menu { border-radius: 4px; }"] {
        let before = root_after(&ctx, css, 800.0);
        let mut see_through = saved;
        see_through.panel_background = see_through.panel_background.gamma_multiply(0.5);
        see_through.border = see_through.border.gamma_multiply(0.5);
        theme::use_palette(&ctx, see_through);
        let after = frames_in(&ctx, 800.0);
        assert_eq!(after.panel_fill, see_through.panel_background, "{css:?}");
        assert_eq!(after.window_fill, see_through.panel_background, "{css:?}");
        assert_eq!(after.window_stroke.color, see_through.border, "{css:?}");
        assert_ne!(before.panel_fill, after.panel_fill);
        theme::use_palette(&ctx, saved);
        assert_eq!(frames_in(&ctx, 800.0).panel_fill, before.panel_fill, "{css:?}");
    }
    skin::configure(&skin::Skin::default());
}

/// With no rule for egui's widgets, the root's `Visuals` are the palette's, for every built-in
/// theme and variant; a theme that has rules for them (the three with their own artwork) shows
/// them, and with its sheet taken away is the palette's too.
#[test]
fn with_no_rule_the_root_visuals_are_the_palettes_in_every_built_in_theme() {
    let saved = theme::palette();
    let with_rules = ["suhina-sheet", "suhina-meadow", "suhina-blocks"];
    let mut themes: Vec<(String, theme::Theme)> =
        theme::built_in_themes().into_iter().map(|(id, theme)| (id.to_owned(), theme)).collect();
    themes.extend(theme::built_in_variants());
    for (id, theme) in &themes {
        let ctx = egui::Context::default();
        theme::use_palette(&ctx, theme.palette());
        let expected = theme::palette_visuals_for(&theme.palette());
        skin::configure(&theme.skin);
        let built = frames_in(&ctx, 800.0);
        let base = id.split('@').next().unwrap_or(id);
        if with_rules.contains(&base) {
            assert_ne!(built, expected, "{id}: its rules for egui's widgets reach the root");
        } else {
            assert_eq!(built, expected, "{id}");
        }
        // Without a sheet: the fallback alone.
        let ctx = egui::Context::default();
        theme::use_palette(&ctx, theme.palette());
        skin::configure(&skin::Skin::default());
        assert_eq!(frames_in(&ctx, 800.0), expected, "{id}: no sheet");
    }
    theme::use_palette(&egui::Context::default(), saved);
    skin::configure(&skin::Skin::default());
}

#[test]
fn looking_up_the_root_visuals_adds_nothing_to_the_style_tree() {
    let css = "button { background-color: #101010; } menu { background-color: #080808; } text-input { color: red; }";
    skin::configure(&skin::Skin { css: Some(css.into()), ..Default::default() });
    // A new context builds them in this frame.
    let ctx = egui::Context::default();
    let mut looked_up = false;
    ctx.run_ui(egui::RawInput::default(), |ui| {
        let _frame = super::super::native_css::frame_with_motion(ui, false);
        looked_up = ui.ctx().global_style().visuals.window_fill == rgb(8, 8, 8);
        assert_eq!(super::super::native_css::budget().nodes, 0);
    })
    .textures_delta
    .clear();
    assert!(looked_up, "the frame did look them up");
    skin::configure(&skin::Skin::default());
}

#[test]
fn a_rule_that_gives_only_a_border_colour_keeps_the_outline_s_width() {
    let fallback = theme::palette_visuals_for(&theme::palette());
    let hairline = fallback.widgets.noninteractive.bg_stroke.width;
    assert!(hairline > 0.0, "egui draws its separators with this stroke");
    let ctx = egui::Context::default();
    let red = egui::Color32::from_rgb(255, 0, 0);
    let v = root_after(&ctx, "button:disabled { border-color: #ff0000; } menu { border-color: #ff0000; }", 800.0);
    assert_eq!(v.widgets.noninteractive.bg_stroke, egui::Stroke::new(hairline, red));
    assert_eq!(v.window_stroke, egui::Stroke::new(fallback.window_stroke.width, red));
    // An authored width, 0 included, is the width.
    let v = root_after(&ctx, "button:disabled { border-color: #ff0000; border-width: 0; }", 800.0);
    assert_eq!(v.widgets.noninteractive.bg_stroke.width, 0.0);
    let v = root_after(&ctx, "button:disabled { border-width: 3px; }", 800.0);
    assert_eq!(v.widgets.noninteractive.bg_stroke.width, 3.0);
    skin::configure(&skin::Skin::default());
}

#[test]
fn a_scoped_ui_takes_a_colour_only_border_rule_with_the_width_it_had() {
    skin::configure(&skin::Skin {
        css: Some("button:disabled { border-color: #ff0000; }".into()),
        ..Default::default()
    });
    let ctx = egui::Context::default();
    let mut seen = None;
    for _ in 0..3 {
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let _frame = super::super::native_css::frame_with_motion(ui, false);
            let before = ui.visuals().widgets.noninteractive.bg_stroke.width;
            let scoped = ui_for(ui, None, false);
            seen = Some((before, scoped.ui.visuals().widgets.noninteractive.bg_stroke));
        })
        .textures_delta
        .clear();
    }
    let (before, stroke) = seen.expect("ran");
    assert_eq!(stroke, egui::Stroke::new(before, rgb(255, 0, 0)));
    skin::configure(&skin::Skin::default());
}

#[test]
fn a_rule_for_every_button_is_not_the_selection_s() {
    let fallback = theme::palette_visuals_for(&theme::palette());
    let ctx = egui::Context::default();
    let v = root_after(&ctx, "button { background-color: #101010; color: #111111; }", 800.0);
    assert_eq!(v.selection.bg_fill, fallback.selection.bg_fill);
    assert_eq!(v.selection.stroke.color, fallback.selection.stroke.color);
    let v = root_after(
        &ctx,
        "button { background-color: #101010; color: #111111; } button[checked] { background-color: #202020; }",
        800.0,
    );
    assert_eq!(v.selection.bg_fill, rgb(0x20, 0x20, 0x20));
    assert_eq!(v.selection.stroke.color, fallback.selection.stroke.color);
    skin::configure(&skin::Skin::default());
}

#[test]
fn the_root_visuals_are_built_again_only_when_the_styles_or_the_colours_changed() {
    let ctx = egui::Context::default();
    let css = "menu { border-radius: 4px; }";
    root_after(&ctx, css, 800.0);
    // A mark a rebuild would write over.
    let mark = rgb(1, 2, 3);
    ctx.all_styles_mut(|style| style.visuals.window_fill = mark);
    assert_eq!(frames_in(&ctx, 800.0).window_fill, mark, "frames with nothing changed leave them");
    assert_ne!(frames_in(&ctx, 500.0).window_fill, mark, "a new width resets the styles");
    ctx.all_styles_mut(|style| style.visuals.window_fill = mark);
    let mut changed = theme::palette();
    changed.accent = rgb(4, 5, 6);
    theme::use_palette(&ctx, changed);
    assert_ne!(frames_in(&ctx, 500.0).window_fill, mark, "a colour change rebuilds them");
    theme::use_palette(&ctx, theme::Palette::DARK);
    skin::configure(&skin::Skin::default());
}
