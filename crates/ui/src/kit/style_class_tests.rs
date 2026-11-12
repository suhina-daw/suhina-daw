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
        theme::set_palette(&ctx, changed);
        inside
    });
    assert_eq!(inside, theme::parse_hex("#123456").unwrap());
    assert_eq!(theme::palette(), changed);
    theme::set_palette(&ctx, original);
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
