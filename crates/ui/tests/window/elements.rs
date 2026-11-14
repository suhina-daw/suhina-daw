// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the Launcher, the mixer and the Arrangement draw is each an element a theme styles on its
//! own (#1417): a rule for one reaches what it paints, and nothing else takes its colour.

use super::*;

/// Every colour the window painted in its last frame: fills, strokes, lines and text.
fn painted(h: &Harness<'static, App>) -> Vec<egui::Color32> {
    fn walk(shape: &egui::Shape, found: &mut Vec<egui::Color32>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, found)),
            egui::Shape::Rect(rect) => found.extend([rect.fill, rect.stroke.color]),
            egui::Shape::LineSegment { stroke, .. } => found.push(stroke.color),
            egui::Shape::Path(path) => {
                found.push(path.fill);
                if let egui::epaint::PathStroke { color: egui::epaint::ColorMode::Solid(color), .. } = &path.stroke {
                    found.push(*color);
                }
            }
            egui::Shape::Circle(circle) => found.extend([circle.fill, circle.stroke.color]),
            egui::Shape::Text(text) => {
                found.push(text.fallback_color);
                found.extend(text.galley.job.sections.iter().map(|section| section.format.color));
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    h.output().shapes.iter().for_each(|shape| walk(&shape.shape, &mut found));
    found
}

/// A colour no theme or track uses, the `n`-th of a set.
fn odd(n: u8) -> egui::Color32 {
    egui::Color32::from_rgb(0x13, 0x57, n)
}

fn hex(color: egui::Color32) -> String {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Shows `h` with `rules` (selector, property, colour) as the user's sheet, then every rule's
/// colour that no shape painted.
fn missing(h: &mut Harness<'static, App>, rules: &[(&str, &str, egui::Color32)]) -> Vec<String> {
    let css: String = rules
        .iter()
        .map(|(selector, property, color)| format!("{selector} {{ {property}: {}; }}\n", hex(*color)))
        .collect();
    suhina_ui::kit::native_css::set_user_sheet(Some(css.into()));
    for _ in 0..4 {
        h.step();
    }
    let shown = painted(h);
    suhina_ui::kit::native_css::set_user_sheet(None);
    rules
        .iter()
        .filter(|(_, _, color)| !shown.contains(color))
        .map(|(selector, property, _)| format!("{selector} {property}"))
        .collect()
}

#[test]
fn the_launcher_s_and_the_mixer_s_parts_take_their_own_rules() {
    let mut h = window(demo_app());
    let rules = [
        ("separator[kind=\"slot\"]", "color", odd(1)),
        ("separator[kind=\"column\"]", "color", odd(2)),
        ("separator[kind=\"band\"]", "color", odd(3)),
        ("scene", "background-color", odd(4)),
        ("label[kind=\"scene-number\"]", "color", odd(5)),
        ("slot[kind=\"stop\"]", "background-color", odd(6)),
        ("scene[kind=\"stop-all\"]", "background-color", odd(7)),
        ("track-header[kind=\"main\"]", "background-color", odd(8)),
        ("fader-scale", "color", odd(10)),
    ];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

#[test]
fn the_arrangement_s_parts_take_their_own_rules() {
    let mut h = arrangement_app();
    let rules = [
        ("grid-line[kind=\"bar\"]", "color", odd(21)),
        ("separator[kind=\"row\"]", "color", odd(23)),
        ("separator[kind=\"band\"]", "color", odd(24)),
        ("separator[kind=\"lane\"]", "color", odd(25)),
        ("track-header[kind=\"main\"]", "background-color", odd(26)),
        ("start-marker", "color", odd(27)),
    ];
    assert_eq!(missing(&mut h, &rules), Vec::<String>::new());
}

/// How many painted fills and strokes are `color`.
fn count(h: &Harness<'static, App>, color: egui::Color32) -> usize {
    painted(h).into_iter().filter(|shown| *shown == color).count()
}

/// A rule for every Arrangement tool does not hide the chosen one: only a `[checked]` rule gives
/// it a colour of its own (#1428 review).
#[test]
fn a_rule_for_every_arrangement_tool_keeps_the_chosen_one_lit() {
    let mut h = arrangement_app();
    let mut settings = suhina_ui::settings::Settings::default();
    settings.view.arrangement_toolbar = true;
    let ctx = h.ctx.clone();
    h.state_mut().set_settings(&ctx, settings);
    h.run();
    // The toolbar's tools: select, split, erase, draw, mute and range.
    let tools = 6;
    let fills = |h: &mut Harness<'static, App>, css: &str| {
        suhina_ui::kit::native_css::set_user_sheet(Some(css.to_owned().into()));
        for _ in 0..4 {
            h.step();
        }
        let shown = (count(h, odd(31)), count(h, odd(32)));
        suhina_ui::kit::native_css::set_user_sheet(None);
        shown
    };
    let general = format!("arrangement-tool {{ background-color: {}; }}", hex(odd(31)));
    assert_eq!(fills(&mut h, &general), (tools - 1, 0), "the chosen tool keeps its native fill");
    let chosen = format!("{general} arrangement-tool[checked] {{ background-color: {}; }}", hex(odd(32)));
    assert_eq!(fills(&mut h, &chosen), (tools - 1, 1), "a rule for `[checked]` recolours it");
}
