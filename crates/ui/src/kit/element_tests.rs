// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use std::sync::Arc;

#[test]
fn every_registry_element_has_a_constant() {
    let names: Vec<_> = ALL.iter().map(|name| name.name()).collect();
    let registry: Vec<_> = suhina_theme_package::elements::ELEMENTS.iter().map(|element| element.name).collect();
    assert_eq!(names, registry);
}

/// What the elements drawn by `draw` see, after the frames it takes for the tree to resolve.
fn frames<R>(css: &str, mut draw: impl FnMut() -> R) -> R {
    super::super::skin::configure(&super::super::skin::Skin { css: Some(Arc::from(css)), ..Default::default() });
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
    super::super::skin::configure(&super::super::skin::Skin::default());
    result.unwrap()
}

#[test]
fn an_element_s_style_is_its_own_never_the_innermost_one() {
    let css = "browser { background-color: #102030; } value { background-color: #405060; } \
               value[checked] { color: #FF0000; }";
    let (browser, value, unchecked) = frames(css, || {
        let browser = BROWSER.enter();
        let checked = VALUE.id("main-volume").attr(Checked, true).enter();
        let value = checked.style();
        // The browser asked while one of its fields is in scope still gets its own style.
        let own = browser.style();
        drop(checked);
        let unchecked = VALUE.attr(Checked, false).style();
        (own, value, unchecked)
    });
    let rgb = egui::Color32::from_rgb;
    assert_eq!(browser.map(|s| s.background), Some(rgb(0x10, 0x20, 0x30)));
    let value = value.unwrap();
    assert_eq!(value.background, rgb(0x40, 0x50, 0x60));
    assert_eq!(value.color, rgb(0xff, 0, 0));
    assert_ne!(unchecked.map(|s| s.color), Some(rgb(0xff, 0, 0)));
}

#[test]
fn a_drawn_item_takes_its_colour_attribute() {
    let css = "clip { background-color: attr(color type(<color>), #000000); }";
    let color = egui::Color32::from_rgb(0xff, 0x9a, 0x3c);
    let style = frames(css, || {
        let lane = ARRANGEMENT.enter();
        CLIP.drawn(&lane).attr(Color, color).style()
    });
    assert_eq!(style.map(|s| s.background), Some(color));
}

#[test]
fn paint_uses_the_native_fallbacks_where_the_sheet_sets_nothing() {
    let ctx = egui::Context::default();
    ctx.run_ui(egui::RawInput::default(), |ui| {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(40.0, 20.0));
        let fill = egui::Color32::from_rgb(1, 2, 3);
        let el = BUTTON.native(Native { radius: 4.0, fill }).enter();
        let boxed = el.paint(ui.painter(), rect);
        assert_eq!(boxed, Boxed { content: rect, radius: 4.0 });
    })
    .textures_delta
    .clear();
}
