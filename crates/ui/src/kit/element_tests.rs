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
fn a_drawn_item_takes_its_color_attribute() {
    let css = "clip { background-color: attr(color type(<color>), #000000); }";
    let color = egui::Color32::from_rgb(0xff, 0x9a, 0x3c);
    let style = frames(css, || {
        let _lane = ARRANGEMENT.enter();
        CLIP.color(color).style()
    });
    assert_eq!(style.map(|s| s.background), Some(color));
}

/// A color a theme gives a knob, or a view around a fader, does not reach the parts whose artwork
/// keeps a color of its own: a knob's body and a fader's rail and cap keep their palette colors
/// (the user-agent tier's rules on the parts themselves), and only the knob's pointer takes the
/// knob's color (#1019 review).
#[test]
fn a_color_around_a_knob_or_fader_does_not_reach_its_body_rail_or_cap() {
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
        let cap = super::super::mixer_controls::cap_ink(&FADER_CAP.enter());
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

/// A fader's cap under the pointer takes the hover color of the palette's tier
/// (`fader-cap:hover`), which applies when the cap is entered with its pointer state.
#[test]
fn a_hovered_fader_cap_takes_its_hover_color() {
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
    let mut colors = Vec::new();
    for _ in 0..3 {
        let input =
            egui::RawInput { events: vec![egui::Event::PointerMoved(egui::pos2(10.0, 10.0))], ..Default::default() };
        ctx.run_ui(input, |ui| {
            let _frame = super::super::native_css::frame_with_motion(ui, false);
            let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(20.0, 20.0));
            let response = ui.interact(rect, egui::Id::unique("knob"), egui::Sense::click_and_drag());
            colors.push((
                KNOB_BODY.response_if(Some(&response)).enter().computed_color_or(egui::Color32::RED),
                KNOB_BODY.response_if(None).enter().computed_color_or(egui::Color32::RED),
            ));
        })
        .textures_delta
        .clear();
    }
    super::super::skin::configure(&super::super::skin::Skin::default());
    assert_eq!(colors.last(), Some(&(egui::Color32::from_rgb(0, 0xFF, 0), egui::Color32::BLACK)), "{colors:?}");
}

/// A sound on the sample map takes the user-agent sheet's color for its kind, not a color a
/// theme gives the map around it, and a theme's rule for a kind replaces it.
#[test]
fn a_sample_point_takes_its_kinds_color_not_the_maps() {
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

/// The color of a `slot` while its `clip` is entered by `draw`, after the frames the tree takes.
fn slot_color_around(css: &str, mut draw: impl FnMut()) -> egui::Color32 {
    frames(css, || {
        let slot = SLOT.enter();
        draw();
        slot.style().map(|s| s.color).unwrap_or_default()
    })
}

/// A clip entered with the pointer over it is a real hover, as `Builder::pointer(..).enter()` has
/// always been (the Launcher's clip, #1241): the slot around it is `:hover` too, and a rule for
/// the clip inside a hovered slot applies.
#[test]
fn an_entered_element_given_the_pointer_makes_its_ancestors_hover() {
    let rgb = egui::Color32::from_rgb;
    let css = "slot { color: #112233; } slot:hover { color: #445566; } clip { color: #102030; } \
               slot:hover clip { color: #708090; }";
    assert_eq!(slot_color_around(css, || drop(CLIP.pointer(false, false, false).enter())), rgb(0x11, 0x22, 0x33));
    assert_eq!(slot_color_around(css, || drop(CLIP.pointer(true, false, false).enter())), rgb(0x44, 0x55, 0x66));
    let clip = frames(css, || {
        let _slot = SLOT.enter();
        CLIP.pointer(true, false, false).enter().style().map(|s| s.color).unwrap_or_default()
    });
    assert_eq!(clip, rgb(0x70, 0x80, 0x90), "`slot:hover clip` matches");
}

/// A question about how an element looks under the pointer (`probe`, `look`, `style`) is not the
/// pointer: the slot around it is not `:hover` for it.
#[test]
fn a_look_asked_for_under_the_pointer_does_not_make_its_ancestors_hover() {
    let rgb = egui::Color32::from_rgb;
    let css = "slot { color: #112233; } slot:hover { color: #445566; } clip:hover { color: #708090; }";
    for ask in [
        (|| CLIP.pointer(true, false, false).probe().color()) as fn() -> egui::Color32,
        || CLIP.pointer(true, false, false).look().color(),
        || CLIP.pointer(true, false, false).style().map(|s| s.color).unwrap_or_default(),
    ] {
        let mut asked = egui::Color32::TRANSPARENT;
        let slot = slot_color_around(css, || asked = ask());
        assert_eq!(asked, rgb(0x70, 0x80, 0x90), "the hovered look is asked for");
        assert_eq!(slot, rgb(0x11, 0x22, 0x33), "and the slot stays as it was");
    }
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
fn a_border_painted_alone_follows_the_sheet_s_radius() {
    // A bar's or a page's border whose background its caller paints (`paint_border`) is rounded
    // as the sheet says, as its gradients are; square where the sheet gives no radius (#1487).
    let border = egui::Color32::from_rgb(0xFF, 0, 0);
    let area = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(100.0, 40.0));
    let corner = |css: &str| {
        let painted = shapes(css, |ui| {
            SETTINGS.enter().paint_border(ui.painter(), area);
        });
        painted.iter().find_map(|shape| match shape {
            egui::Shape::Rect(rect) if rect.stroke.color == border => Some(rect.corner_radius),
            _ => None,
        })
    };
    let round = corner("settings { border-width: 1px; border-color: #FF0000; border-radius: 8px; }");
    assert_eq!(round, Some(egui::CornerRadius::same(8)));
    let tight = corner("settings { border-width: 1px; border-color: #FF0000; border-radius: 80px; }");
    assert_eq!(tight, Some(egui::CornerRadius::same(20)), "no rounder than half the box");
    let square = corner("settings { border-width: 1px; border-color: #FF0000; }");
    assert_eq!(square, Some(egui::CornerRadius::ZERO));
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
    // The header's color moves to blue on hover over a second; the label inherits it.
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
                color = LABEL.style().map(|style| style.color);
            }
        })
        .textures_delta
        .clear();
        (color, super::super::native_css::budget().asked)
    };
    for time in [0.0, 0.1, 0.2] {
        run(time, Some(false));
    }
    // Hovered: the header holds red while its transition starts, and the label inherits that.
    assert_eq!(run(0.3, Some(true)).0, Some(egui::Color32::from_rgb(0xFF, 0, 0)));
    // The header goes, and comes back hovered with nothing to move from: blue at once, and the
    // label with it, rather than the red it inherited under the same chain before.
    run(0.4, None);
    run(0.5, None);
    let (color, asked) = run(0.6, Some(true));
    super::super::skin::configure(&super::super::skin::Skin::default());
    assert_eq!(color, Some(egui::Color32::from_rgb(0, 0, 0xFF)));
    assert!(!asked, "the frame painted the whole tree's styles");
}

/// A drawn item's [`Look`]: its own `color` for a line or a mark, never the text color a view's
/// rule gives what is in it; its text color inherited; the token wherever no rule reaches it
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
        // A kind with no rule of the base sheet's: those take their own color, not the view's.
        let column = SEPARATOR.attribute("kind", "lane").look();
        let mark = SLOT_MARK.look();
        (slot.own_color_or(token), column.own_color_or(token), column.color_or(token), mark)
    });
    assert_eq!(slot_line, rgb(0, 0xff, 0), "a rule on the line gives its color");
    assert_eq!(column_line, token, "the view's text color does not reach a line");
    assert_eq!(column_text, rgb(0xff, 0, 0), "but it is the text color inside it");
    assert_eq!(mark.background_or(token), rgb(0, 0, 0xff));
    assert_eq!(mark.border_or(token), rgb(0xff, 0xff, 0));
    assert_eq!(mark.own_color_or(token), token);
    // Without a sheet, every color is the token's.
    let unstyled = Look::default();
    assert_eq!(
        (unstyled.own_color_or(token), unstyled.background_or(token), unstyled.border_or(token)),
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

/// A rule for every `button` does not hide which toggle is on: while lit, only a rule for the
/// state (`button[checked]`) gives it a color (#1417 review).
#[test]
fn a_rule_for_every_button_does_not_hide_a_lit_one() {
    let rgb = egui::Color32::from_rgb;
    let token = rgb(1, 2, 3);
    let lit_fill = |css: &str| {
        frames(css, || {
            let _view = SESSION.enter();
            let plain = BUTTON.id("main-metronome").probe();
            BUTTON.id("main-metronome").attr(Checked, true).look().unlike(&plain, Paint::Background, token)
        })
    };
    assert_eq!(lit_fill("button { background-color: #102030; }"), token);
    assert_eq!(
        lit_fill("button { background-color: #102030; } button[checked] { background-color: #405060; }"),
        rgb(0x40, 0x50, 0x60)
    );
}

/// A comparison lookup (`probe`) is not drawn, so it is no sibling: `button + button` does not
/// match the one drawn after it (#1428 review).
#[test]
fn a_probe_is_no_sibling_of_what_is_drawn_after_it() {
    let rgb = egui::Color32::from_rgb;
    let token = rgb(1, 2, 3);
    let css = "button + button { background-color: #102030; }";
    let (after_probe, after_look) = frames(css, || {
        let _view = SESSION.enter();
        let _probe = BUTTON.id("a").probe();
        let after_probe = BUTTON.id("b").look().background_or(token);
        let _look = BUTTON.id("c").look();
        let after_look = BUTTON.id("d").look().background_or(token);
        (after_probe, after_look)
    });
    assert_eq!(after_probe, token, "the probe took no place beside the button");
    assert_eq!(after_look, rgb(0x10, 0x20, 0x30), "a drawn button is a sibling");
}

/// A rule for every stop button does not hide the queued one: its color comes from a rule for
/// `[queued]` or stays the native accent (#1428 review).
#[test]
fn a_rule_for_every_slot_does_not_hide_a_queued_one() {
    let rgb = egui::Color32::from_rgb;
    let token = rgb(1, 2, 3);
    let queued_ink = |css: &str| {
        frames(css, || {
            let _view = SESSION.enter();
            let plain = SLOT.attribute("kind", "stop").probe();
            SLOT.attribute("kind", "stop").attr(Queued, true).look().unlike(&plain, Paint::OwnColor, token)
        })
    };
    assert_eq!(queued_ink("slot[kind=\"stop\"] { color: #102030; }"), token);
    assert_eq!(
        queued_ink("slot[kind=\"stop\"] { color: #102030; } slot[kind=\"stop\"][queued] { color: #405060; }"),
        rgb(0x40, 0x50, 0x60)
    );
}

/// With CSS motion on, a probe that shares its key with the drawn element it describes (here the
/// same place, no id or response) is its own chain's style, not that element's painted values
/// held for a transition: a `[selected]` rule is not on both sides of the comparison (#1428
/// review).
#[test]
fn a_probe_is_its_own_style_with_motion_on() {
    let rgb = egui::Color32::from_rgb;
    let token = rgb(1, 2, 3);
    let css = "automation-point { color: #102030; transition: color 1s linear; } \
               automation-point[selected] { color: #405060; }";
    super::super::skin::configure(&super::super::skin::Skin { css: Some(Arc::from(css)), ..Default::default() });
    let ctx = egui::Context::default();
    let mut picked = token;
    for frame in 0..5 {
        ctx.run_ui(egui::RawInput { time: Some(f64::from(frame) * 0.1), ..Default::default() }, |ui| {
            let _frame = super::super::native_css::frame_with_motion(ui, true);
            let _view = ARRANGEMENT.enter();
            let plain = AUTOMATION_POINT.probe();
            picked = AUTOMATION_POINT.attr(Selected, true).look().unlike(&plain, Paint::OwnColor, token);
        })
        .textures_delta
        .clear();
    }
    super::super::skin::configure(&super::super::skin::Skin::default());
    assert_eq!(picked, rgb(0x40, 0x50, 0x60));
}

/// The plain look `enter_against_plain` compares with is the same element with only the state
/// off: its pointer state too, so a `:hover` rule is not taken for the lit state's (#1428 review).
#[test]
fn the_plain_look_keeps_the_pointer_state() {
    let rgb = egui::Color32::from_rgb;
    let token = rgb(1, 2, 3);
    let lit_fill = |css: &str| {
        frames(css, || {
            let _view = SESSION.enter();
            let (el, plain) =
                BUTTON.id("main-metronome").pointer(true, false, false).enter_against_plain(Checked, true);
            super::super::widgets::toggle_inks(&el, plain.as_ref(), (token, token), (token, token)).0
        })
    };
    assert_eq!(lit_fill("button:hover { background-color: #102030; }"), token);
    assert_eq!(
        lit_fill("button:hover { background-color: #102030; } button[checked]:hover { background-color: #405060; }"),
        rgb(0x40, 0x50, 0x60)
    );
    // Off, nothing is compared: the element's own rules apply as they are.
    let off = frames("button:hover { background-color: #102030; }", || {
        let _view = SESSION.enter();
        let (el, plain) = BUTTON.id("main-metronome").pointer(true, false, false).enter_against_plain(Checked, false);
        (plain.is_none(), el.background_or(token))
    });
    assert_eq!(off, (true, rgb(0x10, 0x20, 0x30)));
}

/// A muted slot clip with CSS motion on: its plain probe shares its key with the drawn clip (no id
/// or response, the same place once the probe is taken out), and is still its own chain's style,
/// so the `[muted]` rule's background and border come back rather than the fallback token
/// (#1428 review).
#[test]
fn a_muted_clip_s_colors_come_back_through_a_probe_with_motion_on() {
    let rgb = egui::Color32::from_rgb;
    let token = rgb(1, 2, 3);
    let css = "clip { background-color: #102030; border-color: #203040; \
                      transition: background-color 1s linear, border-color 1s linear; } \
               clip[muted] { background-color: #405060; border-color: #506070; }";
    super::super::skin::configure(&super::super::skin::Skin { css: Some(Arc::from(css)), ..Default::default() });
    let ctx = egui::Context::default();
    let mut seen = (token, token);
    for frame in 0..5 {
        ctx.run_ui(egui::RawInput { time: Some(f64::from(frame) * 0.1), ..Default::default() }, |ui| {
            let _frame = super::super::native_css::frame_with_motion(ui, true);
            let _view = SESSION.enter();
            let color = rgb(0x80, 0x20, 0x20);
            let plain = CLIP.color(color).probe();
            let drawn = CLIP.color(color).attr(Muted, true).enter();
            let state = drawn.look();
            seen = (state.unlike(&plain, Paint::Background, token), state.unlike(&plain, Paint::Border, token));
        })
        .textures_delta
        .clear();
    }
    super::super::skin::configure(&super::super::skin::Skin::default());
    assert_eq!(seen, (rgb(0x40, 0x50, 0x60), rgb(0x50, 0x60, 0x70)));
}

/// A toggle's colors ([`super::super::widgets::Toggle`]): off, the button's own where the sheet
/// gives them; lit, only what a rule for the lit state gives, the ink worked out from the lit fill
/// where that rule gives no ink of its own (#1023).
#[test]
fn a_toggle_is_lit_only_by_a_rule_for_its_lit_state() {
    use super::super::widgets::{Toggle, UnlitInk};
    let rgb = egui::Color32::from_rgb;
    let (off, on) = ((rgb(1, 1, 1), rgb(2, 2, 2)), rgb(3, 3, 3));
    // The ink on a fill: its red channel turned to blue, so a test can tell which fill it is for.
    let ink_on = |fill: egui::Color32| rgb(0, 0, fill.to_srgba_unmultiplied()[0]);
    // `on`, or fading out (off but still lit a little).
    let toggle = |css: &str, on_now: bool, fading: bool| {
        frames(css, || {
            let _view = SESSION.enter();
            let (el, sides) = BUTTON.id("solo").enter_toggle(&[Checked], on_now, fading);
            Toggle::of(&el, sides.as_ref(), UnlitInk::Inherited, off, on, ink_on)
        })
    };
    let general = "button { background-color: #102030; color: #405060; }";
    assert_eq!(
        toggle(general, true, false),
        Toggle { off: (rgb(0x10, 0x20, 0x30), rgb(0x40, 0x50, 0x60)), on: (on, ink_on(on)), own_image: false },
        "a rule for every button colors the unlit side only"
    );
    assert_eq!(toggle(general, false, false).off, (rgb(0x10, 0x20, 0x30), rgb(0x40, 0x50, 0x60)));
    let lit = format!("{general} button[checked] {{ background-color: #708090; }}");
    assert_eq!(toggle(&lit, true, false).on, (rgb(0x70, 0x80, 0x90), rgb(0, 0, 0x70)), "the ink follows the lit fill");
    // Fading out, the lit side is still the lit rule's, so the fade does not jump to the token.
    assert_eq!(toggle(&lit, false, true), toggle(&lit, true, false), "the same two sides fading out");
    assert_eq!(toggle(&lit, false, false).on, (on, ink_on(on)), "settled off, nothing is looked up");
    let inked = format!("{lit} button[checked] {{ color: #A0B0C0; }}");
    assert_eq!(toggle(&inked, true, false).on, (rgb(0x70, 0x80, 0x90), rgb(0xA0, 0xB0, 0xC0)));
    // The unlit ink: the button's own where a theme gives it one, else the token.
    let own_ink = frames("session { color: #405060; }", || {
        let _view = SESSION.enter();
        let (el, sides) = BUTTON.id("solo").enter_toggle(&[Checked], false, false);
        let ink = |unlit| Toggle::of(&el, sides.as_ref(), unlit, off, on, ink_on).off.1;
        (ink(UnlitInk::Inherited), ink(UnlitInk::Own))
    });
    // The base sheet gives the button its own ink, so a view's color reaches neither.
    assert_eq!(own_ink, (off.1, off.1));
    // Faded half way, half of each.
    let half =
        Toggle { off: (rgb(0, 0, 0), rgb(0, 0, 0)), on: (rgb(200, 200, 200), rgb(100, 100, 100)), own_image: false }
            .at(0.5);
    assert_eq!(
        half,
        (
            super::super::motion::mix(rgb(0, 0, 0), rgb(200, 200, 200), 0.5),
            super::super::motion::mix(rgb(0, 0, 0), rgb(100, 100, 100), 0.5)
        )
    );
}

/// [`Entered::paint_over`] paints the fill it is given over the sheet's `background-color`, which
/// [`Entered::paint_with`] takes instead (#1023).
#[test]
fn paint_over_keeps_the_fill_it_is_given() {
    let rgb = egui::Color32::from_rgb;
    super::super::skin::configure(&super::super::skin::Skin {
        css: Some(Arc::from("button { background-color: #102030; }")),
        ..Default::default()
    });
    let ctx = egui::Context::default();
    let fill = rgb(0x70, 0x80, 0x90);
    let mut fills = (Vec::new(), Vec::new());
    for _ in 0..3 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let _frame = super::super::native_css::frame_with_motion(ui, false);
            let el = BUTTON.enter();
            let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(20.0, 10.0));
            el.paint_with(ui.painter(), rect, Native { radius: 0.0, fill });
            el.paint_over(ui.painter(), rect.translate(egui::vec2(40.0, 0.0)), Native { radius: 0.0, fill }, 1.0);
        });
        output.textures_delta.clear();
        let rects: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) => Some((rect.rect.left(), rect.fill)),
                _ => None,
            })
            .collect();
        fills = (
            rects.iter().filter(|(x, _)| *x == 10.0).map(|(_, fill)| *fill).collect(),
            rects.iter().filter(|(x, _)| *x == 50.0).map(|(_, fill)| *fill).collect(),
        );
    }
    super::super::skin::configure(&super::super::skin::Skin::default());
    assert_eq!(fills, (vec![rgb(0x10, 0x20, 0x30)], vec![fill]));
}

/// `accent-color` where the sheet gives one, else the token, on an element and on a look.
#[test]
fn accent_or_reads_the_accent_color() {
    let rgb = egui::Color32::from_rgb;
    let token = rgb(1, 2, 3);
    let (entered, look, unstyled) = frames("knob { accent-color: #102030; }", || {
        let unstyled = SLIDER.enter().look().accent_or(token);
        let el = KNOB.enter();
        (el.look().accent_or(token), KNOB.look().accent_or(token), unstyled)
    });
    assert_eq!((entered, look, unstyled), (rgb(0x10, 0x20, 0x30), rgb(0x10, 0x20, 0x30), token));
}

/// An EQ's selected bands' dots take the selection's color unless a theme gives its own.
#[test]
fn an_eqs_selected_bands_take_the_selection_color_or_the_themes() {
    let rgb = egui::Color32::from_rgb;
    let selected = || EQ_BAND.attr(Selected, true).look().accent();
    assert_eq!(frames("", selected), super::super::theme::studio("selection"));
    assert_eq!(frames("eq-band[selected] { accent-color: #102030; }", selected), rgb(0x10, 0x20, 0x30));
}

/// A toggle's background image: the plain button's fades out under the lit fill, a lit state's own
/// stays; [`Entered::paint_over`] paints none of it at 0 (#1435 review).
#[test]
fn a_rule_s_background_image_does_not_cover_a_lit_toggle() {
    use super::super::widgets::{Toggle, UnlitInk};
    let rgb = egui::Color32::from_rgb;
    let image = |css: &str| {
        frames(css, || {
            let (el, sides) = BUTTON.id("solo").enter_toggle(&[Checked], true, false);
            let toggle =
                Toggle::of(&el, sides.as_ref(), UnlitInk::Own, (rgb(1, 1, 1), rgb(1, 1, 1)), rgb(2, 2, 2), |c| c);
            (toggle.image(0.0), toggle.image(0.5), toggle.image(1.0))
        })
    };
    let general = "button { background-image: linear-gradient(#102030, #405060); }";
    assert_eq!(image(general), (1.0, 0.5, 0.0), "the plain button's image fades out");
    let own = format!("{general} button[checked] {{ background-image: linear-gradient(#708090, #a0b0c0); }}");
    assert_eq!(image(&own), (1.0, 1.0, 1.0), "the lit state's own image stays");

    // Painted with none of the image, the box is the fill alone.
    super::super::skin::configure(&super::super::skin::Skin { css: Some(Arc::from(general)), ..Default::default() });
    let ctx = egui::Context::default();
    let mut meshes = (0, 0);
    for _ in 0..3 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let _frame = super::super::native_css::frame_with_motion(ui, false);
            let el = BUTTON.enter();
            let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(20.0, 10.0));
            let native = Native { radius: 0.0, fill: rgb(0x70, 0x80, 0x90) };
            el.paint_over(ui.painter(), rect, native, 1.0);
            el.paint_over(ui.painter(), rect.translate(egui::vec2(40.0, 0.0)), native, 0.0);
        });
        output.textures_delta.clear();
        let at = |x: f32| {
            output
                .shapes
                .iter()
                .filter(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if (mesh.calc_bounds().left() - x).abs() < 0.5))
                .count()
        };
        meshes = (at(10.0), at(50.0));
    }
    super::super::skin::configure(&super::super::skin::Skin::default());
    assert!(meshes.0 > 0, "the whole image paints");
    assert_eq!(meshes.1, 0, "none of it paints");
}

/// `unlike` and `image_unlike` count only what differs from the plain look.
#[test]
fn a_look_s_accent_and_image_are_compared_with_the_plain_one() {
    let rgb = egui::Color32::from_rgb;
    let token = rgb(1, 2, 3);
    let css = "dropdown { accent-color: #102030; } dropdown[kind=\"subtle\"] { background-image: linear-gradient(red, blue); }";
    let (general, own, image) = frames(css, || {
        let plain = DROPDOWN.probe();
        let subtle = DROPDOWN.attribute("kind", "subtle").look();
        (subtle.unlike(&plain, Paint::Accent, token), subtle.accent_or(token), subtle.image_unlike(&plain))
    });
    assert_eq!((general, own, image), (token, rgb(0x10, 0x20, 0x30), true));
}

/// The inks of a view's words: a rule for a `label` kind gives it, a name and a title take the text
/// color of the view they are in, and a kind the base sheet colors (a hint, a caption, a value, an
/// error) takes only a rule of its own, never the color a rule gives a view around it (#1449).
#[test]
fn the_ink_helpers_read_label_kinds_from_the_sheet() {
    let rgb = egui::Color32::from_rgb;
    let css = "dialog { color: #102030; } label[kind=\"caption\"] { color: #405060; } \
               label[kind=\"error\"] { color: #ff0000; }";
    let (name, hint, caption, error, value, title) = frames(css, || {
        let _dialog = DIALOG.enter();
        (name_ink(), hint_ink(), caption_ink(), error_ink(), value_ink(), title_ink())
    });
    let plain = frames("", || (name_ink(), hint_ink(), caption_ink(), error_ink(), value_ink(), title_ink()));
    assert_eq!(name, rgb(0x10, 0x20, 0x30), "a name inherits the dialog's color");
    assert_eq!(title, name, "so does a title");
    assert_eq!(caption, rgb(0x40, 0x50, 0x60), "a caption takes a rule for itself");
    assert_eq!(error, rgb(0xff, 0, 0));
    assert_eq!(hint, plain.1, "a hint keeps the base sheet's ink under the dialog's color");
    assert_eq!(value, plain.4, "so does a value");
    // With no rule, each is the base sheet's, which reads the color variables.
    let tokens = (
        crate::kit::theme::studio("text-primary"),
        crate::kit::theme::studio("text-hint"),
        crate::kit::theme::studio("text-secondary"),
        crate::kit::theme::studio("recording"),
        crate::kit::theme::studio("text-value"),
        crate::kit::theme::studio("text-primary"),
    );
    assert_eq!(plain, tokens, "with no rule, every word is the variable's");
}

/// A `list-row` and a `dialog-title` give their `background-color`, and no rule leaves the token.
#[test]
fn a_list_row_and_a_dialog_title_take_their_own_background() {
    let rgb = egui::Color32::from_rgb;
    let token = rgb(1, 2, 3);
    let (row, selected, title, plain) = frames(
        "list-row { background-color: #102030; } list-row[selected] { color: #405060; } \
         dialog-title { background-color: #708090; }",
        || {
            (
                LIST_ROW.look().background_or(token),
                LIST_ROW.attr(Selected, true).look().color_or(token),
                DIALOG_TITLE.look().background_or(token),
                COLOR_SWATCH.look().border_or(token),
            )
        },
    );
    assert_eq!(
        (row, selected, title, plain),
        (rgb(0x10, 0x20, 0x30), rgb(0x40, 0x50, 0x60), rgb(0x70, 0x80, 0x90), token)
    );
}

/// `unlike` of [`Paint::Color`] counts the text color a rule around the item gives too, but only where it
/// differs from the plain item's: a rule for every row does not hide the picked one (#1440 review).
#[test]
fn color_unlike_ignores_what_the_plain_item_has() {
    let rgb = egui::Color32::from_rgb;
    let token = rgb(1, 2, 3);
    let picked_ink = |css: &str| {
        frames(css, || {
            let _view = SESSION.enter();
            let plain = LIST_ROW.probe();
            LIST_ROW.attr(Selected, true).look().unlike(&plain, Paint::Color, token)
        })
    };
    assert_eq!(picked_ink("list-row { color: #102030; }"), token);
    assert_eq!(
        picked_ink("list-row { color: #102030; } list-row[selected] { color: #405060; }"),
        rgb(0x40, 0x50, 0x60)
    );
    assert_eq!(picked_ink(""), token);
}

/// The base sheet's muted gray is the Arrangement's: a muted Arrangement clip and track header are
/// gray, and a deactivated Launcher clip, outside the Arrangement, keeps the look of its own
/// `session clip[muted]` rule (#1393 review). A base
/// sheet rule is never authored, so a drawn element's `background_or` keeps its fallback whatever
/// the rule says: the Arrangement reads the computed background (`computed_background_or`), and
/// `base_background` is its gray only where no sheet styles the window.
#[test]
fn the_muted_gray_reaches_the_arrangement_not_the_launcher() {
    use super::super::native_css::base_background;
    let gray = Some(egui::Color32::from_rgb(0xA9, 0xA6, 0xA0));
    let muted = [("muted", "")];
    assert_eq!(base_background(Some("arrangement"), "clip", &muted), gray, "arrangement clip[muted]");
    assert_eq!(base_background(Some("arrangement"), "track-header", &muted), gray, "arrangement track-header[muted]");
    assert_ne!(base_background(Some("session"), "clip", &muted), gray, "a Launcher clip keeps its own look");
    assert_eq!(base_background(None, "clip", &muted), None, "a clip outside the Arrangement");
}

/// `probe_in` looks an element up as if drawn inside an ancestor, so a rule that reaches it only
/// there counts, and takes both out of the style tree again (#1393 review).
#[test]
fn a_probe_in_an_ancestor_takes_the_ancestor_s_rules_and_leaves_no_node() {
    let rgb = egui::Color32::from_rgb;
    let token = rgb(1, 2, 3);
    let seen = frames("arrangement clip[muted] { background-color: #405060; }", || {
        let before = BUTTON.enter().node();
        let inside = CLIP.attr(Muted, true).probe_in(ARRANGEMENT).background_or(token);
        let outside = CLIP.attr(Muted, true).probe().background_or(token);
        let after = BUTTON.enter().node();
        (inside, outside, before.zip(after).map(|(b, a)| a - b))
    });
    assert_eq!(seen, (rgb(0x40, 0x50, 0x60), token, Some(1)));
}

/// A theme whose sheet does not parse, with no working styles to keep (the app started on it),
/// still gets the user-agent and base tiers: text and fills stay opaque (#1474 review).
#[test]
fn a_theme_sheet_that_does_not_parse_still_gets_the_base_design() {
    // A thread of its own: its style engine has never had a working sheet to keep.
    let (name, panel) =
        std::thread::spawn(|| frames("}", || (name_ink(), app_fill("panel")))).join().expect("the frames ran");
    assert_eq!(name.a(), 255, "the name ink: {name:?}");
    assert_eq!(panel.a(), 255, "the panel fill: {panel:?}");
}

#[test]
fn the_inspector_clips_a_child_by_an_overflow_box_only_while_it_is_open() {
    let area = Rect::from_min_size(egui::pos2(40.0, 40.0), egui::vec2(100.0, 40.0));
    let wide = area.expand(20.0);
    super::super::skin::configure(&super::super::skin::Skin {
        css: Some(Arc::from("browser { overflow: hidden; }")),
        ..Default::default()
    });
    let ctx = egui::Context::default();
    let mut boxes = Vec::new();
    // The tree resolves over the first frames.
    for _ in 0..3 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let _frame = super::super::native_css::frame_with_motion(ui, false);
            let browser = BROWSER.enter();
            browser.paint(ui.painter(), area);
            let before = VALUE.id("before").enter();
            before.paint(ui.painter(), wide);
            drop(before);
            browser.close_box();
            // Painted after the box closed, with its scope still open: nothing cuts it.
            let after = VALUE.id("after").enter();
            after.paint(ui.painter(), wide);
            drop(after);
            boxes = super::super::native_css::inspector::frame_elements();
        });
        output.textures_delta.clear();
    }
    super::super::skin::configure(&super::super::skin::Skin::default());
    // The browser, then the child entered before the box closed, then the one after.
    let painted = |at: usize| boxes.get(at).and_then(|e| e.painted).map(|(rect, _)| rect);
    assert_eq!(painted(1), Some(area), "cut by the open box: {boxes:?}");
    assert_eq!(painted(2), Some(wide), "past a closed box, whole");
}

/// Each built-in theme dims a push button that cannot be pressed now (Settings' Scan during a
/// scan), and Suhina Sheet draws none of its button artwork on the buttons drawn as a menu's rows:
/// the sheet decides both, and a theme's rule for every `button` hides the base sheet's (#1696).
#[test]
fn built_in_themes_dim_a_disabled_button_and_sheet_s_row_buttons_have_no_artwork() {
    let themes = [
        ("suhina-studio", include_str!("../../themes/suhina-studio/theme.css")),
        ("suhina-light", include_str!("../../themes/suhina-light/theme.css")),
        ("suhina-glass", include_str!("../../themes/suhina-glass/theme.css")),
        ("suhina-sheet", include_str!("../../themes/suhina-sheet/theme.css")),
        ("suhina-blocks", include_str!("../../themes/suhina-blocks/theme.css")),
        ("suhina-meadow", include_str!("../../themes/suhina-meadow/theme.css")),
    ];
    for (name, css) in themes {
        let (enabled, disabled) = frames(css, || {
            (
                BUTTON.attribute("kind", "push").pointer(false, false, false).enter().color(),
                BUTTON.attribute("kind", "push").pointer(false, false, true).enter().color(),
            )
        });
        assert_ne!(enabled, disabled, "{name}: a disabled push button is dimmed");
    }
    let sheet = include_str!("../../themes/suhina-sheet/theme.css");
    let (entry, more, push) = frames(sheet, || {
        let none = |kind| super::super::skin::no_artwork(&BUTTON.attribute("kind", kind).enter());
        (none("entry"), none("more"), none("push"))
    });
    assert!(entry && more, "Sheet's row buttons take their CSS look");
    assert!(!push, "Sheet's push buttons keep its artwork");
}

/// The window's own buttons (Windows) look like the system's in every theme: no fill at rest, a
/// faint gray under the pointer, and Close the system's red with a white cross. The user-agent
/// sheet gives them, and neither the base sheet nor a theme's rule for every `button` reaches them.
#[test]
fn the_window_buttons_look_like_the_system_s_in_every_theme() {
    // Each entered on its own, not inside the one before.
    let window = |css: &'static str, kind: &'static str, hovered| {
        frames(css, || {
            let el = WINDOW_BUTTON.attribute("kind", kind).pointer(hovered, false, false).enter();
            (el.background(), el.color())
        })
    };
    let every_button = "button { background-color: #00ff00; color: #00ff00; border-radius: 9px; image: none; }";
    for css in ["", every_button] {
        let (rest, _) = window(css, "minimize", false);
        let (hovered, _) = window(css, "minimize", true);
        let (close, close_ink) = window(css, "close", true);
        assert_eq!(rest.a(), 0, "{css}: {rest:?}");
        assert_eq!(hovered, egui::Color32::from_rgba_unmultiplied(128, 128, 128, 46), "{css}");
        assert_eq!((close, close_ink), (egui::Color32::from_rgb(0xc4, 0x2b, 0x1c), egui::Color32::WHITE), "{css}");
    }
}
