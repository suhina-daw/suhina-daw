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

/// A colour a theme gives a knob, or a view around a fader, does not reach the parts whose artwork
/// keeps a colour of its own: a knob's body and a fader's rail and cap keep their palette colours
/// (the user-agent tier's rules on the parts themselves), and only the knob's pointer takes the
/// knob's colour (#1019 review).
#[test]
fn a_colour_around_a_knob_or_fader_does_not_reach_its_body_rail_or_cap() {
    let css = ":root { --control-background: #112233; --control-hoverBackground: #223344; \
               --text-value: #FFFFFF; } mixer { color: #00FF00; } knob { color: #FF0000; }";
    let unset = egui::Color32::from_rgb(1, 2, 3);
    let (body, pointer, rail, cap) = frames(css, || {
        let _mixer = MIXER.enter();
        let knob = KNOB.enter();
        let body = KNOB_BODY.enter().computed_color_or(unset);
        let pointer = KNOB_POINTER.enter().color_or(unset);
        drop(knob);
        let rail = super::super::mixer_controls::rail_ink(&FADER_RAIL.enter());
        let cap = super::super::mixer_controls::cap_ink(&FADER_CAP.enter(), super::super::skin::State::Normal);
        (body, pointer, rail, cap)
    });
    let rgb = egui::Color32::from_rgb;
    let mix = super::super::motion::mix;
    assert_eq!(body, rgb(0x11, 0x22, 0x33));
    assert_eq!(pointer, rgb(0xff, 0, 0));
    let near =
        |a: egui::Color32, b: egui::Color32| a.to_array().iter().zip(b.to_array()).all(|(x, y)| x.abs_diff(y) <= 1);
    assert!(near(rail, mix(rgb(0x11, 0x22, 0x33), egui::Color32::BLACK, 0.65)), "{rail:?}");
    assert!(near(cap, mix(rgb(0x11, 0x22, 0x33), egui::Color32::WHITE, 0.28)), "{cap:?}");
}

/// A fader's cap under the pointer takes the hover colour of the palette's tier
/// (`fader-cap:hover`), which applies when the cap is entered with its pointer state.
#[test]
fn a_hovered_fader_cap_takes_its_hover_colour() {
    let css = ":root { --control-background: #112233; --control-hoverBackground: #808080; \
               --text-value: #FFFFFF; }";
    let (normal, hovered) = frames(css, || {
        let normal = FADER_CAP.pointer(false, false, false).enter().computed_color_or(egui::Color32::BLACK);
        let hovered = FADER_CAP.pointer(true, false, false).enter().computed_color_or(egui::Color32::BLACK);
        (normal, hovered)
    });
    let near =
        |a: egui::Color32, b: egui::Color32| a.to_array().iter().zip(b.to_array()).all(|(x, y)| x.abs_diff(y) <= 1);
    let mix = super::super::motion::mix;
    let rgb = egui::Color32::from_rgb;
    assert!(near(normal, mix(rgb(0x11, 0x22, 0x33), egui::Color32::WHITE, 0.28)), "{normal:?}");
    assert!(near(hovered, mix(rgb(0x80, 0x80, 0x80), egui::Color32::WHITE, 0.28)), "{hovered:?}");
}

/// A knob's body entered with the knob's response has its pointer states, so a theme's
/// `knob-body:hover` applies to it.
#[test]
fn a_part_entered_with_its_controls_response_takes_its_hover_rule() {
    let css = "knob-body { color: #000000; } knob-body:hover { color: #00FF00; }";
    super::super::skin::configure(&super::super::skin::Skin { css: Some(Arc::from(css)), ..Default::default() });
    let ctx = egui::Context::default();
    let mut colours = Vec::new();
    for _ in 0..3 {
        let input =
            egui::RawInput { events: vec![egui::Event::PointerMoved(egui::pos2(10.0, 10.0))], ..Default::default() };
        ctx.run_ui(input, |ui| {
            let _frame = super::super::native_css::frame_with_motion(ui, false);
            let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(20.0, 20.0));
            let response = ui.interact(rect, egui::Id::unique("knob"), egui::Sense::click_and_drag());
            colours.push((
                KNOB_BODY.response_if(Some(&response)).enter().computed_color_or(egui::Color32::RED),
                KNOB_BODY.response_if(None).enter().computed_color_or(egui::Color32::RED),
            ));
        })
        .textures_delta
        .clear();
    }
    super::super::skin::configure(&super::super::skin::Skin::default());
    assert_eq!(colours.last(), Some(&(egui::Color32::from_rgb(0, 0xFF, 0), egui::Color32::BLACK)), "{colours:?}");
}

/// A sound on the sample map takes the user-agent sheet's colour for its kind, not a colour a
/// theme gives the map around it, and a theme's rule for a kind replaces it.
#[test]
fn a_sample_point_takes_its_kinds_colour_not_the_maps() {
    let css = "sample-map { color: #00FF00; } sample-point[kind=\"snare\"] { color: #0000FF; }";
    let (kick, snare) = frames(css, || {
        let _map = SAMPLE_MAP.enter();
        let kick = SAMPLE_POINT.attribute("kind", "kick").enter().computed_color_or(egui::Color32::BLACK);
        let snare = SAMPLE_POINT.attribute("kind", "snare").enter().computed_color_or(egui::Color32::BLACK);
        (kick, snare)
    });
    assert_eq!(kick, egui::Color32::from_rgb(0xD0, 0x6E, 0x5E));
    assert_eq!(snare, egui::Color32::from_rgb(0, 0, 0xff));
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
        let nested: Vec<_> = (0..16).map(|_| WORKSPACE_PANEL.enter()).collect();
        let style = LABEL.style();
        drop(nested);
        (style, LABEL.style())
    });
    assert!(styles.0.is_none(), "the seventeenth element in depth has no room");
    assert!(styles.1.is_some());
    assert!(super::super::native_css::budget().unstyled > 0, "the lookups it made are counted");
    super::super::skin::configure(&super::super::skin::Skin::default());
}

/// The shapes one settled frame of `draw` paints, with `css` as the theme's sheet.
fn shapes(css: &str, draw: impl FnMut(&egui::Ui)) -> Vec<egui::Shape> {
    clipped(css, draw).into_iter().map(|(shape, _)| shape).collect()
}

/// [`shapes`], each with the rectangle that clips it.
fn clipped(css: &str, mut draw: impl FnMut(&egui::Ui)) -> Vec<(egui::Shape, Rect)> {
    super::super::skin::configure(&super::super::skin::Skin { css: Some(Arc::from(css)), ..Default::default() });
    let ctx = egui::Context::default();
    let input = || egui::RawInput {
        screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))),
        ..Default::default()
    };
    let mut output = ctx.run_ui(input(), |ui| {
        let _frame = super::super::native_css::frame_with_motion(ui, false);
        draw(ui);
    });
    output.textures_delta.clear();
    super::super::skin::configure(&super::super::skin::Skin::default());
    fn flat(shape: egui::Shape, clip: Rect, out: &mut Vec<(egui::Shape, Rect)>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.into_iter().for_each(|s| flat(s, clip, out)),
            shape => out.push((shape, clip)),
        }
    }
    let mut all = Vec::new();
    output.shapes.into_iter().for_each(|clipped| flat(clipped.shape, clipped.clip_rect, &mut all));
    all
}

use egui::Rect;

/// Where in `shapes` a rectangle stroked with `color` is, and its rectangle.
fn stroke_of(shapes: &[egui::Shape], color: egui::Color32) -> Option<(usize, Rect)> {
    shapes.iter().enumerate().find_map(|(at, shape)| match shape {
        egui::Shape::Rect(rect) if rect.stroke.width > 0.0 && rect.stroke.color == color => Some((at, rect.rect)),
        _ => None,
    })
}

/// Where in `shapes` a rectangle filled with `color` is.
fn fill_of(shapes: &[egui::Shape], color: egui::Color32) -> Option<usize> {
    shapes.iter().position(|shape| matches!(shape, egui::Shape::Rect(rect) if rect.fill == color))
}

#[test]
fn every_element_s_border_comes_before_its_contents() {
    let border = egui::Color32::from_rgb(0xFF, 0, 0);
    let row = egui::Color32::from_rgb(0, 0xFF, 0);
    let css = "browser, track-header, button { border-width: 1px; border-color: #FF0000; }";
    let area = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(100.0, 40.0));
    for element in [BROWSER, TRACK_HEADER, BUTTON] {
        let painted = shapes(css, |ui| {
            let el = element.enter();
            el.paint(ui.painter(), area);
            // A row as wide as the box, as a panel's rows are.
            ui.painter().rect_filled(area, 0.0, row);
        });
        let (border_at, _) = stroke_of(&painted, border).expect("the border");
        let row_at = fill_of(&painted, row).expect("the row");
        assert!(border_at < row_at, "{}: {painted:?}", element.name());
    }
}

#[test]
fn a_child_s_highlight_is_drawn_over_its_parent_s_inset_shadow() {
    let highlight = egui::Color32::from_rgb(0, 0xFF, 0);
    let css = "session { box-shadow: inset 0 0 0 1px #FF0000; background-color: #101010; }";
    let area = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(200.0, 100.0));
    let painted = shapes(css, |ui| {
        let session = SESSION.enter();
        let inside = session.paint(ui.painter(), area).content;
        // The selected track's highlight, along the region's edge.
        ui.painter().rect_filled(Rect::from_min_size(inside.min, egui::vec2(40.0, inside.height())), 0.0, highlight);
    });
    let shadow_at = painted.iter().rposition(|shape| matches!(shape, egui::Shape::Mesh(_))).expect("the inset shadow");
    assert!(fill_of(&painted, highlight).expect("the highlight") > shadow_at, "{painted:?}");
}

#[test]
fn border_and_padding_keep_the_contents_inside_the_box() {
    let area = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(100.0, 40.0));
    let mut content = None;
    shapes("track-header { padding: 4px 8px 2px; }", |ui| {
        content = Some(TRACK_HEADER.enter().paint(ui.painter(), area).content);
    });
    assert_eq!(content, Some(Rect::from_min_max(egui::pos2(18.0, 14.0), egui::pos2(102.0, 48.0))));
    shapes("browser { padding: 4px; border-width: 1px 0 2px; border-color: #FF0000; }", |ui| {
        content = Some(BROWSER.enter().paint(ui.painter(), area).content);
    });
    assert_eq!(content, Some(Rect::from_min_max(egui::pos2(14.0, 15.0), egui::pos2(106.0, 44.0))));
    shapes("track-header { padding: 40px; }", |ui| {
        content = Some(TRACK_HEADER.enter().paint(ui.painter(), area).content);
    });
    assert_eq!(content.map(|c| c.area()), Some(0.0), "padding larger than the box leaves no room");
}

/// Padding in `em` is the element's font size times it, as the layout takes it, so the content
/// box painted agrees with the box the sheet lays out (#1336).
#[test]
fn padding_in_em_takes_the_element_s_font_size() {
    let area = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(100.0, 40.0));
    let mut content = None;
    shapes("track-header { font-size: 10px; padding: 0.5em 1em; }", |ui| {
        content = Some(TRACK_HEADER.enter().paint(ui.painter(), area).content);
    });
    assert_eq!(content, Some(Rect::from_min_max(egui::pos2(20.0, 15.0), egui::pos2(100.0, 45.0))));
}

/// Padding past 512 points, in points or in percent of a toolbar's width, is the room the layout
/// gives it too: one limit for both, so the content box and the laid-out children agree (#1336).
#[test]
fn large_padding_is_the_layout_s_in_the_content_box() {
    let area = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1000.0, 40.0));
    let mut content = None;
    shapes("toolbar { padding-left: 60%; }", |ui| {
        content = Some(TOOLBAR.enter().paint(ui.painter(), area).content);
    });
    assert_eq!(content.map(|c| c.left()), Some(600.0), "60% of the toolbar's 1000 points");
    shapes("track-header { padding-left: 700px; }", |ui| {
        content = Some(TRACK_HEADER.enter().paint(ui.painter(), area).content);
    });
    assert_eq!(content.map(|c| c.left()), Some(700.0));
}

#[test]
fn sides_of_different_widths_are_drawn_each() {
    let area = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(100.0, 40.0));
    let red = egui::Color32::from_rgb(0xFF, 0, 0);
    let painted = shapes("toolbar { border-width: 0 0 1px; border-color: #FF0000; }", |ui| {
        TOOLBAR.enter().paint(ui.painter(), area);
    });
    let lines: Vec<Rect> = painted
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Rect(rect) if rect.fill == red => Some(rect.rect),
            _ => None,
        })
        .collect();
    assert_eq!(
        lines,
        vec![Rect::from_min_max(egui::pos2(10.0, 49.0), egui::pos2(110.0, 50.0))],
        "only the line under it"
    );
    assert!(stroke_of(&painted, red).is_none());
    // Wider together than the box is tall, the top and bottom meet without overlapping.
    let short = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(100.0, 20.0));
    let painted = shapes("toolbar { border-width: 16px 0 16px 1px; border-color: #FF0000; }", |ui| {
        TOOLBAR.enter().paint(ui.painter(), short);
    });
    let tall: f32 = painted
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Rect(rect) if rect.fill == red && rect.rect.width() > 50.0 => Some(rect.rect.height()),
            _ => None,
        })
        .sum();
    assert_eq!(tall, 20.0, "{painted:?}");
}

#[test]
fn a_parent_that_hides_its_overflow_clips_its_children_to_its_padding_box() {
    let area = Rect::from_min_size(egui::pos2(40.0, 40.0), egui::vec2(100.0, 40.0));
    let child = egui::Color32::from_rgb(0, 0xFF, 0);
    let menu = egui::Color32::from_rgb(0, 0, 0xFF);
    let child_clip = |css: &str| {
        let painted =
            clipped(css, |ui| {
                let browser = BROWSER.enter();
                browser.paint(ui.painter(), area);
                // A child painting past the region, through a painter of its own.
                ui.ctx().layer_painter(ui.layer_id()).rect_filled(area.expand(20.0), 0.0, child);
                // A menu above the views: another layer, which no region clips.
                ui.ctx()
                    .layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("menu")))
                    .rect_filled(area.expand(20.0), 0.0, menu);
            });
        let clip_of = |color| {
            painted
                .iter()
                .find(|(shape, _)| matches!(shape, egui::Shape::Rect(r) if r.fill == color))
                .map(|(_, clip)| *clip)
        };
        (clip_of(child).expect("the child"), clip_of(menu).expect("the menu"))
    };
    let (clip, menu_clip) = child_clip("browser { border-width: 2px; border-color: #FF0000; }");
    assert_eq!(clip, area.shrink(2.0), "the region's padding box (regions hide their overflow)");
    assert!(menu_clip.contains_rect(area.expand(10.0)), "a menu is not clipped: {menu_clip:?}");
    // Counted only in debug builds, as the window tests run.
    #[cfg(debug_assertions)]
    assert!(super::overflow_cuts() > 0, "the cut is counted");
    let (clip, _) = child_clip("browser { overflow: visible; }");
    assert!(clip.contains_rect(area.expand(10.0)), "overflow: visible lets it pass: {clip:?}");
}

#[test]
fn a_parent_s_outline_is_drawn_over_its_children_and_past_its_own_clip() {
    let outline = egui::Color32::from_rgb(0, 0, 0xFF);
    let row = egui::Color32::from_rgb(0, 0xFF, 0);
    let area = Rect::from_min_size(egui::pos2(50.0, 50.0), egui::vec2(100.0, 40.0));
    let painted = clipped("browser { outline: 2px solid #0000FF; }", |ui| {
        let browser = BROWSER.enter();
        browser.paint(ui.painter(), area);
        ui.painter().rect_filled(area, 0.0, row);
    });
    let at = |color| {
        painted.iter().position(|(shape, _)| match shape {
            egui::Shape::Rect(rect) => rect.fill == color || rect.stroke.color == color,
            _ => false,
        })
    };
    assert!(at(outline).expect("the outline") > at(row).expect("the row"));
    let (_, clip) = painted[at(outline).unwrap_or_default()];
    assert!(clip.contains_rect(area.expand(2.0)), "its own overflow does not clip it: {clip:?}");
}

#[test]
fn an_outline_is_drawn_last_and_outside_the_border() {
    let outline = egui::Color32::from_rgb(0, 0, 0xFF);
    let css = "button { outline: 2px solid #0000FF; outline-offset: 3px; }";
    let area = Rect::from_min_size(egui::pos2(50.0, 50.0), egui::vec2(60.0, 20.0));
    let painted = shapes(css, |ui| {
        let el = BUTTON.enter();
        el.paint(ui.painter(), area);
        ui.painter().rect_filled(area, 0.0, egui::Color32::WHITE);
    });
    let (at, rect) = stroke_of(&painted, outline).expect("the outline");
    assert_eq!(rect, area.expand(3.0));
    assert!(at > fill_of(&painted, egui::Color32::WHITE).unwrap(), "after the contents");
    let none = shapes("button { outline-width: 2px; outline-color: #0000FF; }", |ui| {
        BUTTON.enter().paint(ui.painter(), area);
    });
    assert!(stroke_of(&none, outline).is_none(), "no style, no outline");
}

/// The clips of the meshes (a glow) one frame paints when a toolbar `bar` holds a section whose
/// painter is clipped at `section`, and in it a glowing button at `button`.
fn glow_clips(bar: Rect, section: Rect, button: Rect) -> Vec<Rect> {
    let css = "button { box-shadow: 0 0 6px 4px #FF00FF; background-color: #202020; }";
    super::super::skin::configure(&super::super::skin::Skin { css: Some(Arc::from(css)), ..Default::default() });
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let _frame = super::super::native_css::frame_with_motion(ui, false);
        let toolbar = TOOLBAR.enter();
        toolbar.paint_background(ui.painter(), bar, 0.0);
        let _module = TOOLBAR_MODULE.id("transport").enter();
        let painter = ui.painter().with_clip_rect(section);
        BUTTON.enter().paint(&painter, button);
    });
    output.textures_delta.clear();
    super::super::skin::configure(&super::super::skin::Skin::default());
    output
        .shapes
        .iter()
        .filter(|clipped| matches!(clipped.shape, egui::Shape::Mesh(_)))
        .map(|clipped| clipped.clip_rect)
        .collect()
}

#[test]
fn a_toolbar_control_s_glow_passes_its_section_and_stops_at_the_toolbar() {
    let bar = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 40.0));
    // The section's painter is clipped at its edge, as the toolbar's modules are.
    let module = Rect::from_min_size(egui::pos2(100.0, 0.0), egui::vec2(40.0, 40.0));
    let button = Rect::from_min_size(egui::pos2(102.0, 4.0), egui::vec2(36.0, 32.0));
    let clips = glow_clips(bar, module, button);
    assert!(!clips.is_empty(), "the glow is painted");
    for clip in &clips {
        assert!(clip.left() < module.left() && clip.right() > module.right(), "passes the section: {clip:?}");
        assert!(bar.contains_rect(*clip), "stops at the toolbar: {clip:?}");
    }
}

#[test]
fn a_glow_stops_at_a_viewport_edge_that_cuts_its_control() {
    // A scroll area inside the toolbar, scrolled so the button's top is out of view: its
    // viewport's clip cuts the button at the top, and the glow stays below that edge.
    let bar = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 200.0));
    let viewport = Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(100.0, 100.0));
    let button = Rect::from_min_size(egui::pos2(120.0, 40.0), egui::vec2(36.0, 32.0));
    let clips = glow_clips(bar, viewport, button);
    assert!(!clips.is_empty(), "the glow is painted");
    for clip in &clips {
        assert_eq!(clip.top(), viewport.top(), "not over what is above the viewport: {clip:?}");
    }
}

#[test]
fn a_child_of_a_parent_held_for_a_transition_is_not_kept_under_the_parent_s_new_chain() {
    // The header's colour moves to blue on hover over a second; the button inherits it.
    let css = "track-header { color: #FF0000; transition: color 1s linear; } \
               track-header:hover { color: #0000FF; }";
    super::super::skin::configure(&super::super::skin::Skin { css: Some(Arc::from(css)), ..Default::default() });
    let ctx = egui::Context::default();
    let run = |time: f64, header: Option<bool>| {
        let mut color = None;
        ctx.run_ui(egui::RawInput { time: Some(time), ..Default::default() }, |ui| {
            let _frame = super::super::native_css::frame_with_motion(ui, true);
            if let Some(hovered) = header {
                let _header = TRACK_HEADER.pointer(hovered, false, false).enter();
                color = BUTTON.style().map(|style| style.color);
            }
        })
        .textures_delta
        .clear();
        (color, super::super::native_css::budget().asked)
    };
    for time in [0.0, 0.1, 0.2] {
        run(time, Some(false));
    }
    // Hovered: the header holds red while its transition starts, and the button inherits that.
    assert_eq!(run(0.3, Some(true)).0, Some(egui::Color32::from_rgb(0xFF, 0, 0)));
    // The header goes, and comes back hovered with nothing to move from: blue at once, and the
    // button with it, rather than the red it inherited under the same chain before.
    run(0.4, None);
    run(0.5, None);
    let (color, asked) = run(0.6, Some(true));
    super::super::skin::configure(&super::super::skin::Skin::default());
    assert_eq!(color, Some(egui::Color32::from_rgb(0, 0, 0xFF)));
    assert!(!asked, "the frame painted the whole tree's styles");
}

/// A drawn item's [`Look`]: its own `color` for a line or a mark, never the text colour a view's
/// rule gives what is in it; its text colour inherited; the token wherever no rule reaches it
/// (#1417).
#[test]
fn a_look_takes_its_own_rules_and_the_token_elsewhere() {
    let rgb = egui::Color32::from_rgb;
    let token = rgb(1, 2, 3);
    let css = "session { color: #FF0000; } separator[kind=\"slot\"] { color: #00FF00; } \
               slot-mark { background-color: #0000FF; border-color: #FFFF00; }";
    let (slot_line, column_line, column_text, mark) = frames(css, || {
        let _view = SESSION.enter();
        let slot = SEPARATOR.attribute("kind", "slot").look();
        let column = SEPARATOR.attribute("kind", "column").look();
        let mark = SLOT_MARK.look();
        (slot.color_or(token), column.color_or(token), column.text_or(token), mark)
    });
    assert_eq!(slot_line, rgb(0, 0xff, 0), "a rule on the line gives its colour");
    assert_eq!(column_line, token, "the view's text colour does not reach a line");
    assert_eq!(column_text, rgb(0xff, 0, 0), "but it is the text colour inside it");
    assert_eq!(mark.background_or(token), rgb(0, 0, 0xff));
    assert_eq!(mark.border_or(token), rgb(0xff, 0xff, 0));
    assert_eq!(mark.color_or(token), token);
    // Without a sheet, every colour is the token's.
    let unstyled = Look::default();
    assert_eq!(
        (unstyled.color_or(token), unstyled.background_or(token), unstyled.border_or(token)),
        (token, token, token)
    );
}

/// `[queued]` is an attribute the app gives, as `[playing]` is (#1417).
#[test]
fn queued_is_an_attribute() {
    let css = "clip[queued] { background-color: #102030; }";
    let (queued, waiting) = frames(css, || {
        let _view = SESSION.enter();
        (
            CLIP.attr(Queued, true).look().background_or(egui::Color32::BLACK),
            CLIP.look().background_or(egui::Color32::BLACK),
        )
    });
    assert_eq!(queued, egui::Color32::from_rgb(0x10, 0x20, 0x30));
    assert_eq!(waiting, egui::Color32::BLACK);
}
