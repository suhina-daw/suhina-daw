// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use std::sync::Arc;

#[test]
fn every_registry_element_has_a_constant() {
    let names: Vec<_> = ALL.iter().map(|name| name.name()).collect();
    let registry: Vec<_> = suhina_theme_package::elements::ELEMENTS.iter().map(|element| element.name).collect();
    assert_eq!(names, registry);
    assert!(ALL.iter().enumerate().all(|(at, name)| usize::from(name.index()) == at), "an index is its place");
    assert_eq!(named("knob"), Some(KNOB));
    assert_eq!(named("no-such-element"), None);
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
        let _lane = ARRANGEMENT.enter();
        CLIP.color(color).style()
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

/// One frame of `draw` in `ctx` with `css` as the theme's sheet; what it returned, and whether the
/// style engine asked for another frame.
fn one_frame<R>(ctx: &egui::Context, css: &str, mut draw: impl FnMut() -> R) -> (R, bool) {
    super::super::skin::configure(&super::super::skin::Skin { css: Some(Arc::from(css)), ..Default::default() });
    let mut result = None;
    ctx.run_ui(egui::RawInput::default(), |ui| {
        let _frame = super::super::native_css::frame_with_motion(ui, false);
        result = Some(draw());
    })
    .textures_delta
    .clear();
    (result.unwrap(), super::super::native_css::budget().asked)
}

#[test]
fn a_state_given_in_a_frame_styles_its_element_in_that_frame() {
    let css = "button { background-color: #000000; } button:hover { background-color: #FF0000; } \
               button[checked] { color: #00FF00; }";
    let ctx = egui::Context::default();
    let red = egui::Color32::from_rgb(0xFF, 0, 0);
    let (idle, asked) = one_frame(&ctx, css, || BUTTON.style().map(|s| s.background));
    assert_eq!(idle, Some(egui::Color32::BLACK), "the first frame with a sheet is styled already");
    assert!(!asked, "and needs no other frame");
    // The pointer arrives: the button is red in the frame that knows it, with no frame after.
    let (hovered, asked) = one_frame(&ctx, css, || BUTTON.pointer(true, false, false).style().map(|s| s.background));
    assert_eq!(hovered, Some(red));
    assert!(!asked);
    let (checked, asked) = one_frame(&ctx, css, || BUTTON.attr(Checked, true).style().map(|s| s.color));
    assert_eq!(checked, Some(egui::Color32::from_rgb(0, 0xFF, 0)));
    assert!(!asked);
    // A new sheet: its styles in its first frame.
    let (changed, asked) =
        one_frame(&ctx, "button { background-color: #0000FF; }", || BUTTON.style().map(|s| s.background));
    assert_eq!(changed, Some(egui::Color32::BLUE));
    assert!(!asked);
    super::super::skin::configure(&super::super::skin::Skin::default());
}

#[test]
fn a_style_that_depends_on_siblings_takes_one_more_frame() {
    let css = "button + button { color: #FF0000; }";
    let ctx = egui::Context::default();
    let second = || {
        let _first = BUTTON.id("play").enter();
        drop(_first);
        BUTTON.id("stop").style().map(|s| s.color)
    };
    let red = Some(egui::Color32::from_rgb(0xFF, 0, 0));
    let (first, asked) = one_frame(&ctx, css, second);
    assert_ne!(first, red, "a style worked out in the frame does not know the siblings");
    assert!(asked, "so the whole tree's asks for one more frame");
    let (then, asked) = one_frame(&ctx, css, second);
    assert_eq!(then, red);
    assert!(!asked);
    super::super::skin::configure(&super::super::skin::Skin::default());
}

#[test]
fn elements_past_the_tree_s_depth_get_no_style_and_are_counted() {
    let ctx = egui::Context::default();
    let (styles, _) = one_frame(&ctx, "label { color: #FF0000; }", || {
        let nested: Vec<_> = (0..16).map(|_| WORKSPACE.enter()).collect();
        let style = LABEL.style();
        drop(nested);
        (style, LABEL.style())
    });
    assert!(styles.0.is_none(), "the seventeenth element in depth has no room");
    assert!(styles.1.is_some());
    assert!(super::super::native_css::budget().unstyled > 0, "the lookups it made are counted");
    super::super::skin::configure(&super::super::skin::Skin::default());
}
