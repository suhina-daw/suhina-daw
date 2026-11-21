// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Model and Advanced pages of the struck, bar and tine bodies (revision 3, and 1 read as 3):
//! the body tabs, the sound drawn across the page (`Widget::Strands`, from the Instrument's live
//! values), the large dial at its center, the body's name and one even row of dials, and the
//! Advanced page's shaping dials and engine choices (docs/spec/instrument-editor.md).

use super::editor::{ADVANCED, BODY, KEYS_OVER_BODY};
use super::{Canvas, add, any_of, both, line, not, page_is, spaced, text, times, value_is};
use suhina_device_sdk::ui::{Draw, PAGE_ID, TextAlign, TextFormat, UiExpression, UiGesture, UiValue, Widget};

/// The revisions whose pages these are.
const BODY_REVISIONS: &[u16] = &[1, 3];
/// Body values: struck bars, the tine, the abstract body.
const STRUCK_BARS: &[u16] = &[1, 2, 3, 4];
const KALIMBA: &[u16] = &[5];
const STRUCK: &[u16] = &[0];

/// The page's own colors over the ivory artwork, the same with every theme: espresso ink,
/// fainter text, a hairline, the dial's groove and ticks, and brass with its glow.
const INK: [u8; 4] = super::INK;
const MUTED: [u8; 4] = [107, 85, 67, 255];
const HAIRLINE: [u8; 4] = [52, 38, 29, 56];
const GROOVE: [u8; 4] = [52, 38, 29, 34];
const TICK: [u8; 4] = [52, 38, 29, 96];
const TICK_MINOR: [u8; 4] = [52, 38, 29, 46];
const ACCENT: [u8; 4] = [184, 116, 47, 255];
const GLOW: [u8; 4] = [214, 160, 86, 70];

/// The bodies in Body's order: their name and a line on each.
const BODIES: [(u16, &str, &str); 6] = [
    (0, "Struck", "An abstract struck resonance"),
    (1, "Marimba", "Rosewood bar · tuned 1 : 4 : 10 · tube resonator"),
    (2, "Vibraphone", "Aluminium bar · tuned 1 : 4 · long ring"),
    (3, "Xylophone", "Rosewood bar · tuned 1 : 3 : 6 · short and bright"),
    (4, "Glockenspiel", "Steel bar · untuned overtones · no resonator"),
    (5, "Kalimba", "Steel tine · plucked · wooden box"),
];

/// The large dial's turn: from 150° left of the top to 150° right of it, in the SDK's radians
/// (clockwise from the right).
const TURN_START: f32 = 2.094_395;
const TURN_SPAN: f32 = 5.235_988;

/// The large dial for `parameter`: a machined ivory knob on an espresso rim (the canvas's
/// `knob-large`), a scale of ticks, a brass arc in a groove with a glow, a brass pointer at the
/// dome's edge, and `caption` and the value inside the dome. The control is 196 points square
/// times `k`.
fn large_dial(parameter: &str, caption: &str, k: f32) -> Vec<Draw> {
    let c = 98.0 * k;
    let turn = add(TURN_START.into(), times(UiValue::normalized(parameter, false), TURN_SPAN));
    let at = |radius: f32| {
        [
            add(c.into(), times(UiValue::Expression(Box::new(UiExpression::Cos { value: turn.clone() })), radius)),
            add(c.into(), times(UiValue::Expression(Box::new(UiExpression::Sin { value: turn.clone() })), radius)),
        ]
    };
    let mut drawing = vec![Draw::image("knob-large", [20.0 * k, 24.0 * k, 156.0 * k, 156.0 * k])];
    for n in 0..=60 {
        let angle = TURN_START + n as f32 * TURN_SPAN / 60.0;
        let major = n % 6 == 0;
        let (inner, outer) = (if major { 86.0 * k } else { 89.0 * k }, 93.0 * k);
        drawing.push(line(
            [c + inner * angle.cos(), c + inner * angle.sin()],
            [c + outer * angle.cos(), c + outer * angle.sin()],
            if major { TICK } else { TICK_MINOR },
            if major { 1.6 } else { 1.0 },
        ));
    }
    let arc = |end: UiValue, color: [u8; 4], width: f32| Draw::Arc {
        center: [c.into(), c.into()],
        radius: (78.0 * k).into(),
        start: TURN_START.into(),
        end,
        color,
        width,
    };
    drawing.push(arc((TURN_START + TURN_SPAN).into(), GROOVE, 5.0));
    drawing.push(arc(turn.clone(), GLOW, 11.0 * k));
    drawing.push(arc(turn.clone(), ACCENT, 3.5 * k));
    drawing.push(Draw::Line { from: at(44.0 * k), to: at(57.0 * k), color: ACCENT, width: 3.5 * k });
    drawing.push(text([c, c - 22.0], &spaced(caption), 8.5, TextAlign::Center, MUTED));
    drawing.push(Draw::Text {
        position: [c.into(), (c - 8.0).into()],
        text: String::new(),
        size: 24.0 * k.max(0.9),
        color: INK,
        parameter: Some(parameter.into()),
        decimals: 0,
        align: TextAlign::Center,
        format: TextFormat::Percent,
    });
    drawing
}

/// One even row of dials for the bodies `bodies`: `size` each, spread over `width` points from
/// `left`, their top at `y`.
fn dial_row(
    ui: &mut Canvas,
    bodies: &[u16],
    dials: &[(&str, &str, &str)],
    y: f32,
    size: [f32; 2],
    left: f32,
    width: f32,
) {
    let step = width / dials.len().max(1) as f32;
    for (n, (id, name, tooltip)) in dials.iter().enumerate() {
        let x = left + step * (n as f32 + 0.5) - size[0] / 2.0;
        ui.face_dial(id, name, [x, y, size[0], size[1]], tooltip, "knob-small");
        ui.only_revisions(BODY_REVISIONS, 1);
        ui.only_values("body", bodies, 1);
    }
}

/// A dot before a choice of `id`, on the page being built: brass and filled while it is `value`,
/// a ring otherwise. Two controls.
pub(super) fn radio_dot(ui: &mut Canvas, id: &str, value: u16, center: [f32; 2]) {
    let at = [center[0].into(), center[1].into()];
    ui.graphic(
        &format!("{id} {value} chosen"),
        vec![Draw::Circle { center: at.clone(), radius: 3.5.into(), color: ACCENT }],
    );
    ui.also_when(1, &value_is(id, value));
    ui.graphic(
        &format!("{id} {value} not chosen"),
        vec![Draw::Arc {
            center: at,
            radius: 3.5.into(),
            start: 0.0.into(),
            end: std::f32::consts::TAU.into(),
            color: MUTED,
            width: 1.2,
        }],
    );
    ui.also_when(1, &not(value_is(id, value)));
}

/// A hairline across the page at `y`.
fn hairline(ui: &mut Canvas, y: f32) {
    ui.add(
        "Divider",
        Widget::Graphic { drawing: vec![line([0.0, 0.5], [768.0, 0.5], HAIRLINE, 1.0)] },
        [56.0, y, 768.0, 8.0],
        None,
        UiGesture::Drag,
        "",
    );
    ui.only_revisions(BODY_REVISIONS, 1);
}

type DialSpec = (&'static str, &'static str, &'static str);

const DECAY: DialSpec = (
    "body_decay",
    "Decay",
    "Struck: fundamental T60. Bars and tines: scales their decays relative to 2 seconds. Next strike",
);
const BRIGHTNESS: DialSpec = ("brightness", "Brightness", "Energy in the higher resonances; affects the next strike");
const STRIKE: DialSpec = (
    "strike",
    "Strike",
    "Where the mallet or finger meets the body: a bar from its center to its end, a tine from its free end. Next strike",
);
const STRUCTURE: DialSpec = ("structure", "Structure", "Stretch the resonance frequencies; next strike");
const WIDTH: DialSpec = ("stereo_width", "Width", "Spread the bars from left (low) to right (high)");
const TUNING: DialSpec =
    ("tuning", "Tuning", "How far the body's tuned overtones apply; 0 % leaves the natural bar or tine");
const RING: DialSpec = ("ring", "Ring", "How long the upper modes ring beside the fundamental; 50 % is the body's own");
const TORSION: DialSpec = ("torsion", "Torsion", "How much the bar's twisting modes sound; 50 % is the body's own");

/// Where each body's tab stands, from the left, and how wide it is.
const TABS: [[f32; 2]; 6] =
    [[117.0, 77.0], [202.0, 88.0], [299.0, 115.0], [421.0, 109.0], [538.0, 132.0], [678.0, 85.0]];

/// The body layer for the bodies (page `BODY`; its tabs also over the keys) and their Advanced
/// page.
pub(super) fn body_page(ui: &mut Canvas) {
    ui.pages(&[BODY, KEYS_OVER_BODY]);
    for ((value, name, about), [x, w]) in BODIES.iter().zip(TABS) {
        ui.choice_labelled("body", *value, name, &name.to_uppercase(), [x, 101.0, w, 20.0], about);
        ui.only_revisions(BODY_REVISIONS, 1);
    }
    ui.page(BODY);

    // The sound across the page, behind the controls: the Instrument's live values move it, and
    // it steps back on the Advanced page.
    let presence = UiValue::Expression(Box::new(UiExpression::Multiply {
        a: add(0.7.into(), times(UiValue::normalized("resonator", false), 0.3)),
        b: add(1.0.into(), times(add(page_is(ADVANCED), page_is(KEYS_OVER_BODY)), -0.45)),
    }));
    ui.add(
        "Sound",
        Widget::Strands {
            modes: (0..5).map(|n| UiValue::parameter(format!("#r{n}"), false)).collect(),
            since: UiValue::parameter("#r5", false),
            velocity: UiValue::parameter("#r6", false),
            height: UiValue::normalized("stereo_width", false),
            thickness: UiValue::normalized("mallet_weight", false),
            speed: UiValue::normalized("mallet_hardness", false),
            presence,
            light: [255, 249, 237, 255],
            mid: [222, 189, 135, 255],
            shadow: [128, 87, 54, 255],
            thread: [199, 128, 51, 255],
        },
        [0.0, 112.0, 880.0, 176.0],
        None,
        UiGesture::Drag,
        "",
    );
    ui.show_when(both(any_of("model_revision", BODY_REVISIONS), any_of(PAGE_ID, &[BODY, ADVANCED, KEYS_OVER_BODY])));

    // The large dial at the center: Hardness for a struck bar, Touch for the tine, Brightness for
    // the abstract body, which has no mallet.
    for (bodies, parameter, name, tooltip) in [
        (
            STRUCK_BARS,
            "mallet_hardness",
            "Hardness",
            "How hard the mallet is: harder sounds brighter and the sound flows faster",
        ),
        (KALIMBA, "mallet_hardness", "Touch", "How hard the finger is: harder sounds brighter"),
        (STRUCK, "brightness", "Brightness", "Energy in the higher resonances; affects the next strike"),
    ] {
        let k = 0.8;
        let size = 196.0 * k;
        ui.drawn_dial(
            parameter,
            name,
            [440.0 - size / 2.0, 124.0, size, size],
            large_dial(parameter, name, k),
            tooltip,
        );
        ui.only_revisions(BODY_REVISIONS, 1);
        ui.only_values("body", bodies, 1);
    }

    // The body's name with a line on it beside it, the mallet model, and the way to the
    // Advanced page.
    for (value, name, about) in BODIES {
        let width = name.chars().count() as f32 * 14.6;
        ui.add(
            &format!("{name} name"),
            Widget::Graphic {
                drawing: vec![
                    text([0.0, 0.0], &spaced(name), 16.0, TextAlign::Left, INK),
                    text([width + 18.0, 5.0], about, 10.5, TextAlign::Left, MUTED),
                ],
            },
            [56.0, 288.0, 460.0, 26.0],
            None,
            UiGesture::Drag,
            "",
        );
        ui.only_revisions(BODY_REVISIONS, 1);
        ui.only_values("body", &[value], 1);
    }
    for (value, name, x, w, tooltip) in [
        (0_u16, "Contact", 539.0, 73.0, "Simulate the mallet against the moving bar"),
        (1, "Pulse", 653.0, 50.0, "A fixed force pulse, added linearly"),
    ] {
        ui.choice_labelled("mallet_model", value, name, &name.to_uppercase(), [x, 290.0, w, 20.0], tooltip);
        radio_dot(ui, "mallet_model", value, [x - 10.0, 299.0]);
        ui.only_revisions(BODY_REVISIONS, 3);
        ui.only_values("body", STRUCK_BARS, 3);
    }
    ui.text("Plucked", "PLUCKED", [539.0, 295.0, 120.0, 14.0], 9.0, TextAlign::Left, MUTED);
    ui.only_revisions(BODY_REVISIONS, 1);
    ui.only_values("body", KALIMBA, 1);
    ui.choice_labelled(
        PAGE_ID,
        ADVANCED,
        "Advanced page",
        "ADVANCED ›",
        [728.0, 290.0, 96.0, 20.0],
        "Tuning, Ring, Torsion and Modes per voice",
    );
    ui.only_revisions(BODY_REVISIONS, 1);
    hairline(ui, 320.0);

    // One even row of dials.
    let small = [96.0, 100.0];
    let weight = ("mallet_weight", "Weight", "The mallet head's mass, 12.5 to 50 g");
    let resonator = ("resonator", "Resonator", "The tube under the bar");
    dial_row(ui, STRUCK_BARS, &[DECAY, BRIGHTNESS, STRIKE, weight, resonator, WIDTH], 326.0, small, 56.0, 768.0);
    let finger = ("mallet_weight", "Finger", "How heavy the finger's touch is");
    let body = ("resonator", "Box", "The wooden box under the tine");
    dial_row(ui, KALIMBA, &[DECAY, BRIGHTNESS, STRIKE, finger, body, WIDTH], 326.0, small, 56.0, 768.0);
    dial_row(ui, STRUCK, &[DECAY, STRIKE, STRUCTURE], 326.0, small, 56.0, 768.0);

    advanced_page(ui);
}

/// The Advanced page: the body's shaping dials, larger, with a line on each, and the engine's
/// choices. The sound stays behind it, at about half strength.
fn advanced_page(ui: &mut Canvas) {
    ui.page(ADVANCED);
    ui.text("Advanced", &spaced("Advanced"), [0.0, 108.0, 880.0, 28.0], 20.0, TextAlign::Center, INK);
    ui.only_revisions(BODY_REVISIONS, 1);
    ui.text(
        "Advanced note",
        "Fine shaping of the chosen body. Every control is neutral at its default.",
        [0.0, 140.0, 880.0, 14.0],
        11.0,
        TextAlign::Center,
        MUTED,
    );
    ui.only_revisions(BODY_REVISIONS, 1);
    let large = [120.0, 124.0];
    let shaping: [(&[u16], Vec<DialSpec>, Vec<&str>); 3] = [
        (
            STRUCK_BARS,
            vec![TUNING, RING, TORSION],
            vec![
                "How far the tuned overtones apply",
                "Upper modes ring shorter or longer",
                "How much the twisting modes sound",
            ],
        ),
        (KALIMBA, vec![TUNING, RING], vec!["How far the tuned overtones apply", "Upper modes ring shorter or longer"]),
        (STRUCK, vec![STRUCTURE], vec!["Stretches the resonances away from harmonic"]),
    ];
    for (bodies, dials, hints) in shaping {
        let width = dials.len() as f32 * 180.0;
        let left = 440.0 - width / 2.0;
        dial_row(ui, bodies, &dials, 160.0, large, left, width);
        let step = width / hints.len().max(1) as f32;
        let drawing = hints
            .iter()
            .enumerate()
            .map(|(n, hint)| text([step * (n as f32 + 0.5), 0.0], hint, 10.5, TextAlign::Center, MUTED))
            .collect();
        ui.add("Advanced hints", Widget::Graphic { drawing }, [left, 290.0, width, 14.0], None, UiGesture::Drag, "");
        ui.only_revisions(BODY_REVISIONS, 1);
        ui.only_values("body", bodies, 1);
    }
    ui.choice_labelled(
        PAGE_ID,
        BODY,
        "Body page back",
        "‹  BODY",
        [56.0, 306.0, 120.0, 22.0],
        "Back to the body layer",
    );
    ui.only_revisions(BODY_REVISIONS, 1);
    hairline(ui, 336.0);

    ui.text("Mallet model caption", &spaced("Mallet model"), [56.0, 350.0, 300.0, 12.0], 9.0, TextAlign::Left, MUTED);
    ui.only_revisions(BODY_REVISIONS, 1);
    for (value, name) in [(0_u16, "Contact"), (1, "Pulse")] {
        ui.choice_labelled(
            "mallet_model",
            value,
            name,
            &name.to_uppercase(),
            [56.0 + f32::from(value) * 88.0, 370.0, 80.0, 22.0],
            if value == 0 {
                "Simulate the mallet against the moving bar"
            } else {
                "A fixed force pulse, added linearly"
            },
        );
        ui.only_revisions(BODY_REVISIONS, 1);
        ui.only_values("body", STRUCK_BARS, 1);
    }
    ui.text(
        "Advanced plucked",
        "Plucked: a tine has no mallet",
        [56.0, 375.0, 300.0, 14.0],
        10.5,
        TextAlign::Left,
        MUTED,
    );
    ui.only_revisions(BODY_REVISIONS, 1);
    ui.only_values("body", KALIMBA, 1);

    ui.text("Modes caption", &spaced("Modes per voice"), [484.0, 350.0, 340.0, 12.0], 9.0, TextAlign::Left, MUTED);
    ui.only_revisions(BODY_REVISIONS, 1);
    for (value, label) in [(0_u16, "8"), (1, "16")] {
        ui.choice_labelled(
            "quality",
            value,
            &format!("{label} modes"),
            label,
            [484.0 + f32::from(value) * 56.0, 370.0, 48.0, 22.0],
            "Resonances per modal voice: more sound richer and cost more processing",
        );
        ui.only_revisions(BODY_REVISIONS, 1);
    }
    ui.text(
        "Modes note",
        "More modes sound richer and cost more processing.",
        [484.0, 404.0, 340.0, 14.0],
        10.5,
        TextAlign::Left,
        MUTED,
    );
    ui.only_revisions(BODY_REVISIONS, 1);
}
