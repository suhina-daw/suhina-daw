// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Model page of the struck, bar and tine bodies (revision 3, and 1 read as 3): the body
//! buttons, a side view of the chosen bar or tine with its mallet or thumb, which can be dragged
//! along it, and the body, mallet and space groups (`docs/spec/instrument-editor.md`).

use super::Canvas;
use std::f32::consts::PI;
use suhina_device_sdk::ui::{Draw, TextAlign, TextFormat, UiExpression, UiValue};

/// The revisions whose Model page shows the bodies.
const STRUCK_OR_BARS: &[u16] = &[1, 3];
/// Body values with a physical bar or tine.
const BARS: &[u16] = &[1, 2, 3, 4, 5];
/// Body values with a struck bar (the tine is plucked).
const STRUCK_BARS: &[u16] = &[1, 2, 3, 4];

/// The drawing's place on the canvas, and the bar's within the drawing.
const HERO: [f32; 4] = [24.0, 178.0, 832.0, 116.0];
const BAR_LEFT: f32 = 96.0;
const BAR_LENGTH: f32 = 640.0;
const BAR_TOP: f32 = 34.0;
/// The tine's clamp (its bridge) and its length to the free end.
const TINE_CLAMP: f32 = 200.0;
const TINE_LENGTH: f32 = 510.0;
const TINE_TOP: f32 = 44.0;

const INK: [u8; 4] = [74, 58, 46, 255];
const MUTED: [u8; 4] = [120, 98, 78, 255];
const ACCENT: [u8; 4] = [184, 116, 47, 255];
const SHADOW: [u8; 4] = [60, 40, 20, 46];
const SHINE: [u8; 4] = [255, 255, 255, 80];
const BRASS: [u8; 4] = [190, 150, 88, 255];
const STEEL: [u8; 4] = [204, 208, 214, 255];
const YARN: [u8; 4] = [92, 64, 102, 255];
const RATTAN: [u8; 4] = [196, 160, 112, 255];
const SKIN: [u8; 4] = [226, 186, 156, 255];

/// How a body's bar looks from the side.
struct BarLook {
    colour: [u8; 4],
    thickness: f32,
    /// How far the underside is cut away between the nodes.
    arch: f32,
    grain: bool,
    tube: bool,
    felt: bool,
}

const MARIMBA: BarLook =
    BarLook { colour: [138, 78, 50, 255], thickness: 28.0, arch: 12.0, grain: true, tube: true, felt: false };
const XYLOPHONE: BarLook =
    BarLook { colour: [180, 116, 70, 255], thickness: 26.0, arch: 7.0, grain: true, tube: true, felt: false };
const VIBRAPHONE: BarLook =
    BarLook { colour: [180, 186, 194, 255], thickness: 20.0, arch: 7.0, grain: false, tube: true, felt: false };
const GLOCKENSPIEL: BarLook =
    BarLook { colour: STEEL, thickness: 14.0, arch: 0.0, grain: false, tube: false, felt: true };
/// The frame rail under the bars.
const RAIL: f32 = 100.0;

fn n(value: f32) -> UiValue {
    value.into()
}

fn with_alpha(colour: [u8; 4], alpha: u8) -> [u8; 4] {
    [colour[0], colour[1], colour[2], alpha]
}

/// `a + b * parameter`, an expression for a drawing that follows a parameter.
fn follow(a: f32, b: f32, parameter: &str) -> UiValue {
    UiValue::Expression(Box::new(UiExpression::Add {
        a: a.into(),
        b: UiValue::Expression(Box::new(UiExpression::Multiply {
            a: b.into(),
            b: UiValue::parameter(parameter, false),
        })),
    }))
}

fn rect(x: f32, y: f32, width: f32, height: f32, color: [u8; 4], radius: f32) -> Draw {
    Draw::Rect { rect: [n(x), n(y), n(width), n(height)], color, radius }
}

fn circle(center: [UiValue; 2], radius: UiValue, color: [u8; 4]) -> Draw {
    Draw::Circle { center, radius, color }
}

fn line(from: [f32; 2], to: [f32; 2], color: [u8; 4], width: f32) -> Draw {
    Draw::Line { from: [n(from[0]), n(from[1])], to: [n(to[0]), n(to[1])], color, width }
}

fn text(position: [f32; 2], text: &str, align: TextAlign) -> Draw {
    Draw::Text {
        align,
        format: TextFormat::Number,
        position: [n(position[0]), n(position[1])],
        text: text.into(),
        color: MUTED,
        size: 11.0,
        parameter: None,
        decimals: 0,
    }
}

/// How many pieces draw the arch.
const ARCH_PIECES: usize = 16;

/// The underside's height at `t` of the length: the bar's bottom, cut away in an arch between
/// a fifth and four fifths of the length.
fn underside(look: &BarLook, t: f32) -> f32 {
    let bottom = BAR_TOP + look.thickness;
    if (0.2..=0.8).contains(&t) { bottom - look.arch * (PI * (t - 0.2) / 0.6).sin() } else { bottom }
}

/// The bar's side outline, for its edge (a fill takes only convex shapes, so the body is drawn in
/// pieces by `bar_body`).
fn bar_outline(look: &BarLook) -> Vec<[UiValue; 2]> {
    let mut points = vec![
        [n(BAR_LEFT), n(BAR_TOP)],
        [n(BAR_LEFT + BAR_LENGTH), n(BAR_TOP)],
        [n(BAR_LEFT + BAR_LENGTH), n(BAR_TOP + look.thickness)],
    ];
    for i in (0..=ARCH_PIECES).rev() {
        let t = 0.2 + 0.6 * i as f32 / ARCH_PIECES as f32;
        points.push([n(BAR_LEFT + BAR_LENGTH * t), n(underside(look, t))]);
    }
    points.push([n(BAR_LEFT), n(BAR_TOP + look.thickness)]);
    points
}

/// The bar's body: the slab above the arch, its two ends and the arch as convex pieces that
/// overlap by a hair, so no seam shows between them.
fn bar_body(look: &BarLook) -> Vec<Draw> {
    let slab = look.thickness - look.arch;
    let bottom = BAR_TOP + look.thickness;
    let mut pieces = vec![rect(BAR_LEFT, BAR_TOP, BAR_LENGTH, slab + 0.5, look.colour, 3.0)];
    if look.arch > 0.0 {
        let end = BAR_LENGTH * 0.2 + 0.5;
        pieces.push(rect(BAR_LEFT, BAR_TOP + slab, end, look.arch, look.colour, 2.0));
        pieces.push(rect(BAR_LEFT + BAR_LENGTH - end, BAR_TOP + slab, end, look.arch, look.colour, 2.0));
        let top = BAR_TOP + slab - 0.5;
        for i in 0..ARCH_PIECES {
            let (a, b) = (0.2 + 0.6 * i as f32 / ARCH_PIECES as f32, 0.2 + 0.6 * (i + 1) as f32 / ARCH_PIECES as f32);
            let (x0, x1) = (BAR_LEFT + BAR_LENGTH * a - 0.4, BAR_LEFT + BAR_LENGTH * b + 0.4);
            pieces.push(Draw::Path {
                points: vec![
                    [n(x0), n(top)],
                    [n(x1), n(top)],
                    [n(x1), n(underside(look, b).min(bottom))],
                    [n(x0), n(underside(look, a).min(bottom))],
                ],
                color: look.colour,
                width: 0.0,
                closed: true,
                fill: Some(look.colour),
            });
        }
    }
    pieces
}

/// Where the mallet meets the bar, from the left end: `0.5 * (0.98 - p) / 0.96` of its length
/// for Strike position `p` (`docs/spec/instrument.md`).
fn bar_contact() -> UiValue {
    follow(BAR_LEFT + BAR_LENGTH * 0.5 * 0.98 / 0.96, -BAR_LENGTH * 0.5 / 0.96, "strike")
}

/// A mallet whose head grows with Mallet weight, resting on `top` at `x`.
fn mallet(x: UiValue, top: f32) -> Vec<Draw> {
    // The head's radius, 7 to 12 points, and its centre just above the bar.
    let radius = follow(7.0, 5.0, "mallet_weight");
    let centre_y = follow(top, -5.0, "mallet_weight");
    let centre_y = UiValue::Expression(Box::new(UiExpression::Add { a: centre_y, b: n(-7.0) }));
    let stick_end = UiValue::Expression(Box::new(UiExpression::Add { a: x.clone(), b: n(72.0) }));
    let shine = UiValue::Expression(Box::new(UiExpression::Add { a: x.clone(), b: n(-2.5) }));
    let shine_y = UiValue::Expression(Box::new(UiExpression::Add { a: centre_y.clone(), b: n(-2.5) }));
    vec![
        Draw::Line { from: [x.clone(), centre_y.clone()], to: [stick_end, n(0.0)], color: RATTAN, width: 3.0 },
        circle([x, centre_y], radius, YARN),
        circle([shine, shine_y], n(2.5), SHINE),
    ]
}

/// A bar from the side: its supports at the nodes of its first mode, the resonator tube or felt
/// under it, its shadow and grain, the mirror of the strike point and the mallet.
fn bar_drawing(look: &BarLook) -> Vec<Draw> {
    let bottom = BAR_TOP + look.thickness;
    let nodes = [BAR_LEFT + BAR_LENGTH * 0.224, BAR_LEFT + BAR_LENGTH * 0.776];
    let centre = BAR_LEFT + BAR_LENGTH / 2.0;
    let mut drawing = Vec::new();
    // A rail along the frame and a post under each node.
    drawing.push(line([BAR_LEFT - 24.0, RAIL], [BAR_LEFT + BAR_LENGTH + 24.0, RAIL], with_alpha(INK, 70), 2.0));
    for x in nodes {
        drawing.push(line([x, bottom], [x, RAIL], with_alpha(INK, 90), 2.0));
    }
    if look.tube {
        let (x, top) = (centre - 22.0, bottom + 6.0);
        drawing.push(rect(x + 4.0, top + 4.0, 44.0, 114.0 - top, SHADOW, 7.0));
        drawing.push(rect(x, top, 44.0, 114.0 - top, BRASS, 7.0));
        drawing.push(rect(x + 8.0, top + 5.0, 6.0, 104.0 - top, SHINE, 3.0));
        drawing.push(rect(x + 32.0, top + 5.0, 5.0, 104.0 - top, [110, 80, 40, 60], 2.5));
        drawing.push(rect(x - 2.0, top - 3.0, 48.0, 7.0, with_alpha(INK, 210), 3.5));
    }
    if look.felt {
        for x in nodes {
            drawing.push(rect(x - 12.0, bottom, 24.0, 5.0, [150, 58, 58, 255], 2.0));
        }
    }
    drawing.push(rect(BAR_LEFT + 5.0, BAR_TOP + 6.0, BAR_LENGTH, look.thickness - look.arch * 0.6, SHADOW, 4.0));
    drawing.extend(bar_body(look));
    drawing.push(Draw::Path {
        points: bar_outline(look),
        color: with_alpha(INK, 110),
        width: 1.0,
        closed: true,
        fill: None,
    });
    drawing.push(rect(BAR_LEFT + 4.0, BAR_TOP + 2.0, BAR_LENGTH - 8.0, 3.0, SHINE, 1.5));
    if look.grain {
        let grain = [0, 0, 0, 34];
        let y = BAR_TOP + look.thickness * 0.45;
        drawing.push(line([BAR_LEFT + 30.0, y], [BAR_LEFT + 250.0, y + 1.0], grain, 1.0));
        drawing.push(line([BAR_LEFT + 330.0, y + 4.0], [BAR_LEFT + 600.0, y + 3.0], grain, 1.0));
        drawing.push(line([BAR_LEFT + 120.0, y - 6.0], [BAR_LEFT + 420.0, y - 6.0], grain, 1.0));
    }
    for x in nodes {
        drawing.push(circle([n(x), n(BAR_TOP + look.thickness / 2.0)], n(2.5), with_alpha(INK, 170)));
    }
    // The other half of a symmetric bar is struck the same: its strike point, faintly.
    let mirror = follow(BAR_LEFT + BAR_LENGTH * (1.0 - 0.5 * 0.98 / 0.96), BAR_LENGTH * 0.5 / 0.96, "strike");
    drawing.push(circle([mirror, n(BAR_TOP + 3.0)], n(4.0), with_alpha(ACCENT, 110)));
    drawing.extend(mallet(bar_contact(), BAR_TOP));
    drawing
}

/// A kalimba from the side: the box with its sound hole, the pressure bar and bridge, the tine
/// over them, and the thumb at the pluck point, `1 - 0.5 * p` of the length from the bridge.
fn tine_drawing() -> Vec<Draw> {
    let wood = [196, 146, 96, 255];
    let thumb_x = follow(TINE_CLAMP + TINE_LENGTH, -TINE_LENGTH * 0.5, "strike");
    let nail_x = follow(TINE_CLAMP + TINE_LENGTH - 4.0, -TINE_LENGTH * 0.5, "strike");
    vec![
        rect(BAR_LEFT + 4.0, 66.0, BAR_LENGTH, 40.0, SHADOW, 9.0),
        rect(BAR_LEFT, 62.0, BAR_LENGTH, 40.0, wood, 9.0),
        rect(BAR_LEFT + 6.0, 64.0, BAR_LENGTH - 12.0, 3.0, SHINE, 1.5),
        circle([n(600.0), n(84.0)], n(10.0), [62, 42, 30, 255]),
        rect(150.0, TINE_TOP - 2.0, 14.0, 22.0, [120, 124, 130, 255], 3.0),
        rect(TINE_CLAMP - 4.0, TINE_TOP + 4.0, 8.0, 14.0, INK, 2.0),
        // The neighbouring tines behind it, shorter and longer, as on a kalimba.
        rect(144.0, TINE_TOP + 1.0, TINE_CLAMP + TINE_LENGTH - 104.0, 4.0, [150, 154, 160, 150], 2.0),
        rect(144.0, TINE_TOP + 1.0, TINE_CLAMP + TINE_LENGTH - 250.0, 4.0, [150, 154, 160, 190], 2.0),
        rect(144.0, TINE_TOP, TINE_CLAMP + TINE_LENGTH - 144.0, 5.0, STEEL, 2.5),
        rect(146.0, TINE_TOP, TINE_CLAMP + TINE_LENGTH - 148.0, 1.5, SHINE, 0.75),
        circle([thumb_x, n(TINE_TOP - 12.0)], n(11.0), SKIN),
        circle([nail_x, n(TINE_TOP - 15.0)], n(4.0), [246, 222, 206, 255]),
    ]
}

/// Struck has no physical shape: rings for an abstract resonance.
fn struck_drawing() -> Vec<Draw> {
    let mut drawing: Vec<Draw> = [(46.0, 22), (34.0, 34), (22.0, 52), (11.0, 90)]
        .into_iter()
        .map(|(radius, alpha)| circle([n(416.0), n(50.0)], n(radius), with_alpha(ACCENT, alpha)))
        .collect();
    drawing.push(text(
        [416.0, 100.0],
        "An abstract struck body. Choose a bar or a tine for a physical model.",
        TextAlign::Center,
    ));
    drawing
}

/// The bodies' part of the Model page.
pub(super) fn body_page(ui: &mut Canvas) {
    for (i, label) in
        ["Struck", "Marimba", "Vibraphone", "Xylophone", "Glockenspiel", "Kalimba"].into_iter().enumerate()
    {
        ui.choice(
            "body",
            i as u16,
            label,
            [24.0 + i as f32 * 140.0, 140.0, 132.0, 30.0],
            "The body Modal plays: the abstract struck body, a struck bar or a plucked tine",
        );
        ui.only_revisions(STRUCK_OR_BARS, 1);
    }
    ui.graphic("Struck body", struck_drawing(), HERO);
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", &[0], 1);
    for (look, body) in [(&MARIMBA, 1), (&VIBRAPHONE, 2), (&XYLOPHONE, 3), (&GLOCKENSPIEL, 4)] {
        ui.graphic("Bar", bar_drawing(look), HERO);
        ui.only_revisions(STRUCK_OR_BARS, 1);
        ui.only_values("body", &[body], 1);
    }
    ui.handle(
        "strike",
        "Strike point",
        [HERO[0] + BAR_LEFT, HERO[1] + BAR_TOP - 6.0, BAR_LENGTH / 2.0, 34.0],
        [HERO[1] + BAR_TOP - 4.0, HERO[1] + BAR_TOP + 26.0],
        ACCENT,
        "Drag along the bar: from its centre (rounder) to its end (thinner)",
    );
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", STRUCK_BARS, 1);
    ui.graphic("Tine", tine_drawing(), HERO);
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", &[5], 1);
    let (near, far) = (TINE_CLAMP + TINE_LENGTH * 0.51, TINE_CLAMP + TINE_LENGTH * 0.99);
    ui.handle(
        "strike",
        "Pluck point",
        [HERO[0] + near, HERO[1] + TINE_TOP - 8.0, far - near, 22.0],
        [HERO[1] + TINE_TOP - 6.0, HERO[1] + TINE_TOP + 11.0],
        ACCENT,
        "Drag along the tine: from its free end (rounder) towards the bridge (brighter)",
    );
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", &[5], 1);

    ui.group("BODY", [24.0, 300.0, 448.0, 146.0]);
    ui.only_revisions(STRUCK_OR_BARS, 1);
    let dial = |i: usize| [32.0 + i as f32 * 73.0, 322.0, 70.0, 112.0];
    ui.dial(
        "body_decay",
        "Decay",
        dial(0),
        "Struck: fundamental T60. Bars and tines: scales their decays relative to 2 seconds. Next strike",
    );
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.dial("brightness", "Brightness", dial(1), "Energy in the higher resonances; affects the next strike");
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.dial(
        "strike",
        "Strike",
        dial(2),
        "Where the mallet or finger meets the body: a bar from its centre to its end, a tine from its free end. Next strike",
    );
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.dial("structure", "Structure", dial(3), "Struck only: stretch the resonance frequencies; next strike");
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", &[0], 1);
    ui.dial(
        "tuning",
        "Tuning",
        dial(3),
        "How far the body's tuned overtones apply; 0 % leaves the natural bar or tine",
    );
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", BARS, 1);
    ui.dial("ring", "Ring", dial(4), "How long the upper modes ring beside the fundamental; 50 % is the body's own");
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", BARS, 1);
    ui.dial("torsion", "Torsion", dial(5), "How much the bar's twisting modes sound; 50 % is the body's own");
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", STRUCK_BARS, 1);

    ui.group("MALLET", [480.0, 300.0, 208.0, 146.0]);
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", STRUCK_BARS, 1);
    // A tine is plucked: the same hardness and weight set the finger's touch.
    ui.group("FINGER", [480.0, 300.0, 208.0, 146.0]);
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", &[5], 1);
    ui.dial(
        "mallet_hardness",
        "Hardness",
        [488.0, 322.0, 66.0, 112.0],
        "A harder mallet or finger touches more briefly and sounds brighter",
    );
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", BARS, 1);
    ui.dial("mallet_weight", "Weight", [556.0, 322.0, 66.0, 112.0], "The mallet head's mass, 12.5 to 50 g");
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", BARS, 1);
    ui.choice("mallet_model", 0, "Contact", [624.0, 340.0, 58.0, 28.0], "Simulate the mallet against the moving bar");
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", STRUCK_BARS, 1);
    ui.choice("mallet_model", 1, "Pulse", [624.0, 376.0, 58.0, 28.0], "A fixed force pulse, added linearly");
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", STRUCK_BARS, 1);

    ui.group("SPACE", [696.0, 300.0, 160.0, 146.0]);
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", BARS, 1);
    ui.dial("resonator", "Resonator", [700.0, 322.0, 76.0, 112.0], "The tube under a bar or the box under a tine");
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", BARS, 1);
    ui.dial("stereo_width", "Width", [778.0, 322.0, 76.0, 112.0], "Spread the bars from left (low) to right (high)");
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.only_values("body", BARS, 1);
}
