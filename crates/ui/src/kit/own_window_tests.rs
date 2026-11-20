// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use egui::{Color32, Pos2, Rect};

use super::*;

/// The rectangles `draw` paints in the third frame under the theme `css` on the base sheet (the
/// tree takes frames to resolve), and what it gives.
fn painted<R>(css: &str, mut draw: impl FnMut(&Ui) -> R) -> (Vec<egui::epaint::RectShape>, R) {
    crate::kit::skin::configure(&crate::kit::skin::Skin {
        css: Some(Arc::from(css)),
        base: true,
        ..Default::default()
    });
    let ctx = egui::Context::default();
    let mut result = None;
    let mut shapes = Vec::new();
    for _ in 0..3 {
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(WHOLE), ..Default::default() }, |ui| {
            let _frame = crate::kit::native_css::frame_with_motion(ui, false);
            result = Some(draw(ui));
        });
        output.textures_delta.clear();
        shapes = output
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Rect(rect) => Some(rect),
                _ => None,
            })
            .collect();
    }
    crate::kit::skin::configure(&crate::kit::skin::Skin::default());
    (shapes, result.unwrap())
}

const WHOLE: Rect = Rect { min: Pos2::ZERO, max: Pos2::new(400.0, 300.0) };
const FRAME: Frame = Frame { see_through: true, title_bar: 28.0, corner: 10 };
const SKY: Color32 = Color32::from_rgb(0x20, 0x60, 0xc0);
const GLASS: Color32 = Color32::from_rgb(0xe0, 0xf0, 0xff);

/// The base of a window of its own is `window`'s background over the whole window, in its
/// corners, as under the main window; the system's title bar runs over its top.
#[test]
fn the_base_is_the_window_s_background_in_its_corners() {
    let (shapes, base) = painted("window { background-color: #2060c0; }", |ui| base(ui, FRAME));
    assert!(
        shapes.iter().any(|r| r.rect == WHOLE && r.fill == SKY && r.corner_radius == CornerRadius::same(10)),
        "{shapes:?}"
    );
    assert_eq!(base.under_title_bar(), Rect::from_min_max(Pos2::new(0.0, 28.0), WHOLE.max));
}

/// The element that fills a window is its root: its background covers the whole window in the
/// window's corners, whatever its own `border-radius`; its border is not drawn; its
/// contents go inside its padding alone.
#[test]
fn the_root_of_a_window_paints_its_background_and_keeps_its_padding_only() {
    let css = "dialog { background-color: #e0f0ff; border-width: 3px; border-color: #ff0000; \
               border-radius: 12px; padding: 10px; }";
    let (shapes, content) = painted(css, |ui| {
        let dialog = crate::kit::dialog::enter("color-picker");
        dialog.paint_root(ui.painter(), WHOLE, CornerRadius::same(10))
    });
    let fills: Vec<_> = shapes.iter().filter(|r| r.fill == GLASS).collect();
    assert!(
        fills.iter().any(|r| r.rect == WHOLE && r.corner_radius == CornerRadius::same(10)),
        "the background, in the window's corners: {shapes:?}"
    );
    assert!(!shapes.iter().any(|r| r.stroke.color == Color32::RED), "no border: {shapes:?}");
    assert_eq!(content, WHOLE.shrink(10.0));
    // Over the window, the same dialog has one.
    let (boxed, _) = painted(css, |ui| crate::kit::dialog::enter("color-picker").paint(ui.painter(), WHOLE));
    assert!(boxed.iter().any(|r| r.stroke.color == Color32::RED), "a border: {boxed:?}");
}

/// A window of its own is asked to be see-through where the main window is, and only there.
#[test]
fn a_window_is_see_through_where_the_main_window_is() {
    let asked = |see_through| builder(ViewportBuilder::default(), Frame { see_through, ..FRAME }).transparent;
    assert_eq!(asked(true), Some(true));
    assert_eq!(asked(false), Some(false));
}
