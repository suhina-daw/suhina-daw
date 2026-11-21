// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's Play and Sample pages
//! (docs/spec/instrument-editor.md): the amplitude envelope with its points, the playable keys and
//! velocities and one row of dials; the sample's waveform with its region and loop, its name line,
//! one row of dials and Playback. Controls with no effect in the current Source are dimmed.

use super::{
    BRASS, Canvas, HAIRLINE, INK, MUTED, add, blend_silent, envelope_silent, line, sample_silent, spaced, text, times,
    value_text,
};
use suhina_device_sdk::ui::{Draw, TextAlign, TextFormat, UiExpression, UiGesture, UiValue, Widget};

/// A faint line, the envelope's fill, its glow and its line.
const FAINT: [u8; 4] = [52, 38, 29, 36];
const FILL: [u8; 4] = [201, 138, 61, 44];
const GLOW: [u8; 4] = [201, 138, 61, 56];
const STROKE: [u8; 4] = [201, 138, 61, 255];
/// The keys: their ground, the playable ones, the black keys, the octaves, and the veil over the
/// keys outside the range.
const KEY_GROUND: [u8; 4] = [52, 38, 29, 14];
const KEY_LIT: [u8; 4] = [251, 246, 236, 255];
const KEY_BLACK: [u8; 4] = [58, 42, 31, 170];
const KEY_OUTSIDE: [u8; 4] = [239, 228, 210, 150];
/// The velocity wedge and its admitted part.
const WEDGE: [u8; 4] = [52, 38, 29, 18];
const WEDGE_LIT: [u8; 4] = [184, 116, 47, 150];

/// The page's one row of dials, as on the Model page: their top and size, across 768 points.
const ROW_Y: f32 = 394.0;
const DIAL: [f32; 2] = [96.0, 104.0];

/// The envelope: its left edge, top and base, each stage's shortest width, the width a stage's
/// whole range adds to it, and the held part's width.
const LEFT: f32 = 56.0;
const TOP: f32 = 100.0;
const BASE: f32 = 206.0;
const STAGE: f32 = 30.0;
const SPAN: f32 = 170.0;
const HOLD: f32 = 150.0;
/// The legend's captions and values under the envelope, and where each stage's stands.
const LEGEND_Y: f32 = 216.0;
const LEGEND_X: [f32; 4] = [56.0, 176.0, 296.0, 416.0];

/// The 128 keys, one column of `KEY` points each from `LEFT`: their top, their height and the
/// black keys' height; the range bar above them.
const KEY: f32 = 6.0;
const KEYS_TOP: f32 = 268.0;
const KEYS_HEIGHT: f32 = 36.0;
const BLACK_HEIGHT: f32 = 22.0;
const RANGE_Y: f32 = 263.0;
/// The velocity wedge's base and its rise across the page.
const WEDGE_BASE: f32 = 356.0;
const WEDGE_RISE: f32 = 18.0;
/// The right-hand readouts' centers: the lower value, the dash, the higher value.
const READ_LOW: f32 = 748.0;
const READ_DASH: f32 = 778.0;
const READ_HIGH: f32 = 806.0;

/// Where dial `n` of `count` stands in the row across the page.
fn dial_at(n: usize, count: usize) -> [f32; 4] {
    let step = 768.0 / count.max(1) as f32;
    [56.0 + step * (n as f32 + 0.5) - DIAL[0] / 2.0, ROW_Y, DIAL[0], DIAL[1]]
}

/// The rectangle from dial `first` to dial `last` (of `count`).
fn dials_from(first: usize, last: usize, count: usize) -> [f32; 4] {
    let [x, y, _, h] = dial_at(first, count);
    let [end, ..] = dial_at(last, count);
    [x, y, end + DIAL[0] - x, h]
}

fn plus(a: UiValue, b: f32) -> UiValue {
    add(a, b.into())
}

fn at(x: UiValue, y: f32) -> [UiValue; 2] {
    [x, y.into()]
}

fn max(a: UiValue, b: f32) -> UiValue {
    UiValue::Expression(Box::new(UiExpression::Max { a, b: b.into() }))
}

/// A filled convex shape through `points`.
fn shape(points: Vec<[UiValue; 2]>, fill: [u8; 4]) -> Draw {
    Draw::Path { points, color: [0, 0, 0, 0], width: 0.0, closed: true, fill: Some(fill) }
}

/// A caption in spaced capitals, as the Model page sets them.
fn caption(position: [f32; 2], name: &str) -> Draw {
    text(position, &spaced(name), 8.5, TextAlign::Left, INK)
}

/// The Play page (page 0): the amplitude envelope as the page's picture, its three points dragged
/// (Attack sideways, Decay sideways and Sustain up and down, Release sideways); the playable keys
/// and velocities, their ends dragged; one row of dials, Gain, Blend, Attack, Decay, Sustain and
/// Release.
pub(super) fn play_page(ui: &mut Canvas) {
    ui.page(0);
    // The row first, so the envelope's veil falls over its envelope dials as well.
    for (n, (id, name, label, tooltip)) in [
        ("gain", "Output gain", "Gain", "Gain for both sources"),
        ("blend", "Hybrid blend", "Blend", "Hybrid only: move from the modal body to the local sample"),
        (
            "attack",
            "Attack",
            "Attack",
            "Amplitude envelope rise for the sample and fitted models; 0 keeps a fitted recording's own attack",
        ),
        ("decay", "Decay", "Decay", "Amplitude envelope fall to Sustain, for the sample and fitted models"),
        (
            "sustain",
            "Sustain",
            "Sustain",
            "Amplitude envelope held level for the sample and fitted models; 100 % plays a recording as fitted",
        ),
        (
            "release",
            "Release",
            "Release",
            "After note-off: the damper for fitted models (caps every T60), the fade for samples and the original model",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        ui.face_dial(id, name, dial_at(n, 6), tooltip, "knob-small");
        ui.caption(label);
    }

    // The envelope: each stage as wide as its value, linear in seconds like its dial, Release by
    // its logarithmic scale; Sustain the held part's height.
    let attack = times(UiValue::normalized("attack", false), SPAN);
    let decay = times(UiValue::normalized("decay", false), SPAN);
    let release = times(UiValue::normalized("release", true), SPAN);
    let peak = plus(attack.clone(), LEFT + STAGE);
    let knee = plus(add(attack.clone(), decay.clone()), LEFT + 2.0 * STAGE);
    let held = plus(add(attack.clone(), decay.clone()), LEFT + 2.0 * STAGE + HOLD);
    let end = plus(add(add(attack.clone(), decay.clone()), release), LEFT + 3.0 * STAGE + HOLD);
    let level = plus(times(UiValue::normalized("sustain", false), TOP - BASE), BASE);
    let curve = vec![
        at(LEFT.into(), BASE),
        at(peak.clone(), TOP),
        [knee.clone(), level.clone()],
        [held.clone(), level.clone()],
        at(end.clone(), BASE),
    ];
    let mut drawing = vec![line([LEFT, BASE], [824.0, BASE], HAIRLINE, 1.0)];
    for x in [&peak, &knee, &held] {
        drawing.push(Draw::Line { from: at(x.clone(), TOP), to: at(x.clone(), BASE), color: FAINT, width: 1.0 });
    }
    drawing.extend([
        shape(vec![at(LEFT.into(), BASE), at(peak.clone(), TOP), at(peak.clone(), BASE)], FILL),
        shape(vec![at(peak.clone(), TOP), [knee.clone(), level.clone()], at(knee.clone(), BASE), at(peak, BASE)], FILL),
        Draw::Rect {
            rect: [knee.clone(), level.clone(), HOLD.into(), times(UiValue::normalized("sustain", false), BASE - TOP)],
            color: FILL,
            radius: 0.0,
        },
        shape(vec![[held.clone(), level], at(end.clone(), BASE), at(held, BASE)], FILL),
        Draw::Path { points: curve.clone(), color: GLOW, width: 9.0, closed: false, fill: None },
        Draw::Path { points: curve, color: STROKE, width: 2.4, closed: false, fill: None },
    ]);
    for (n, (name, id, format)) in [
        ("Attack", "attack", TextFormat::Seconds),
        ("Decay", "decay", TextFormat::Seconds),
        ("Sustain", "sustain", TextFormat::Percent),
    ]
    .into_iter()
    .enumerate()
    {
        drawing.push(caption([LEGEND_X[n], LEGEND_Y], name));
        drawing.push(value_text(
            [LEGEND_X[n].into(), (LEGEND_Y + 13.0).into()],
            "",
            id,
            format,
            10.5,
            TextAlign::Left,
            MUTED,
        ));
    }
    ui.graphic("Envelope", drawing);
    ui.handle(
        "Attack point",
        [LEFT + STAGE, TOP - 8.0, SPAN, 16.0],
        Some(("attack", false)),
        None,
        None,
        true,
        "Attack: drag sideways",
    );
    ui.handle(
        "Decay and sustain point",
        [LEFT + 2.0 * STAGE, TOP, SPAN, BASE - TOP],
        Some(("decay", false)),
        Some("sustain"),
        Some(attack.clone()),
        false,
        "Decay sideways, Sustain up and down",
    );
    // Faded with Modal, but for fitted models: the envelope and its dials.
    ui.dim("Envelope dimmed", &[[LEFT, 90.0, 768.0, 152.0], dials_from(2, 4, 6)], &envelope_silent());
    ui.handle(
        "Release point",
        [LEFT + 3.0 * STAGE + HOLD, BASE - 8.0, SPAN, 16.0],
        Some(("release", true)),
        None,
        Some(add(attack, decay)),
        true,
        "Release: drag sideways",
    );

    // The keys: a column for each of the 128, the playable ones lit, middle C named, the sample's
    // root marked while the sample plays; the velocities as a wedge, the admitted part lit.
    let low = UiValue::parameter("key_low", false);
    let high = UiValue::parameter("key_high", false);
    let key_x = |key: UiValue, edge: f32| plus(times(key, KEY), LEFT + edge);
    let mut drawing = vec![
        caption([LEGEND_X[3], LEGEND_Y], "Release"),
        value_text(
            [LEGEND_X[3].into(), (LEGEND_Y + 13.0).into()],
            "",
            "release",
            TextFormat::Seconds,
            10.5,
            TextAlign::Left,
            MUTED,
        ),
        caption([56.0, 244.0], "Key range"),
        value_text([READ_LOW.into(), 243.0.into()], "", "key_low", TextFormat::Key, 10.5, TextAlign::Center, INK),
        text([READ_DASH, 243.0], "–", 10.5, TextAlign::Center, INK),
        value_text([READ_HIGH.into(), 243.0.into()], "", "key_high", TextFormat::Key, 10.5, TextAlign::Center, INK),
        Draw::rect([LEFT, KEYS_TOP, 128.0 * KEY, KEYS_HEIGHT], KEY_GROUND),
        Draw::Rect {
            rect: [
                key_x(low.clone(), 0.0),
                KEYS_TOP.into(),
                max(times(plus(add(high.clone(), times(low.clone(), -1.0)), 1.0), KEY), 0.0),
                KEYS_HEIGHT.into(),
            ],
            color: KEY_LIT,
            radius: 0.0,
        },
    ];
    for key in 0..128_u16 {
        let x = LEFT + f32::from(key) * KEY;
        match key % 12 {
            0 => drawing.push(line([x + 0.5, KEYS_TOP], [x + 0.5, KEYS_TOP + KEYS_HEIGHT], HAIRLINE, 1.0)),
            1 | 3 | 6 | 8 | 10 => drawing.push(Draw::rect([x + 0.8, KEYS_TOP, KEY - 1.6, BLACK_HEIGHT], KEY_BLACK)),
            _ => {}
        }
    }
    drawing.extend([
        // The keys outside the range, under a veil of the surface.
        Draw::Rect {
            rect: [LEFT.into(), KEYS_TOP.into(), times(low.clone(), KEY), KEYS_HEIGHT.into()],
            color: KEY_OUTSIDE,
            radius: 0.0,
        },
        Draw::Rect {
            rect: [
                key_x(high.clone(), KEY),
                KEYS_TOP.into(),
                max(times(plus(times(high.clone(), -1.0), 127.0), KEY), 0.0),
                KEYS_HEIGHT.into(),
            ],
            color: KEY_OUTSIDE,
            radius: 0.0,
        },
        Draw::Line {
            from: at(key_x(low, 1.0), RANGE_Y),
            to: at(key_x(high, KEY - 1.0), RANGE_Y),
            color: BRASS,
            width: 2.0,
        },
        text([LEFT + 60.0 * KEY + KEY / 2.0, KEYS_TOP + KEYS_HEIGHT + 3.0], "C3", 9.0, TextAlign::Center, MUTED),
        // The root, moved off the canvas with Modal.
        Draw::Circle {
            center: [
                add(key_x(UiValue::parameter("root", false), KEY / 2.0), times(sample_silent(), 4096.0)),
                (KEYS_TOP + 30.0).into(),
            ],
            radius: 2.5.into(),
            color: BRASS,
        },
        caption([56.0, 318.0], "Velocity"),
        text([READ_DASH, 313.0], "–", 10.5, TextAlign::Center, INK),
        shape(
            vec![
                at(LEFT.into(), WEDGE_BASE),
                at((LEFT + 768.0).into(), WEDGE_BASE - WEDGE_RISE),
                at((LEFT + 768.0).into(), WEDGE_BASE),
            ],
            WEDGE,
        ),
    ]);
    let velocity_x = |id: &str| plus(times(UiValue::parameter(id, false), 768.0), LEFT);
    let velocity_y = |id: &str| plus(times(UiValue::parameter(id, false), -WEDGE_RISE), WEDGE_BASE);
    drawing.push(shape(
        vec![
            at(velocity_x("velocity_low"), WEDGE_BASE),
            [velocity_x("velocity_low"), velocity_y("velocity_low")],
            [velocity_x("velocity_high"), velocity_y("velocity_high")],
            at(velocity_x("velocity_high"), WEDGE_BASE),
        ],
        WEDGE_LIT,
    ));
    drawing.push(line([56.0, 386.5], [824.0, 386.5], HAIRLINE, 1.0));
    ui.graphic("Keys", drawing);
    // A key column's left edge for the lowest key, its right edge for the highest.
    ui.handle(
        "Lowest key marker",
        [LEFT, RANGE_Y - 8.0, 127.0 * KEY, 16.0],
        Some(("key_low", false)),
        None,
        None,
        true,
        "Lowest key: drag sideways",
    );
    ui.handle(
        "Highest key marker",
        [LEFT + KEY, RANGE_Y - 8.0, 127.0 * KEY, 16.0],
        Some(("key_high", false)),
        None,
        None,
        true,
        "Highest key: drag sideways",
    );
    for (id, name) in [("velocity_low", "Lowest velocity marker"), ("velocity_high", "Highest velocity marker")] {
        ui.handle(
            name,
            [LEFT, WEDGE_BASE - 24.0, 768.0, 28.0],
            Some((id, false)),
            None,
            None,
            false,
            "Drag sideways: notes softer or harder than the range do not play",
        );
    }
    ui.readout("velocity_low", "Lowest velocity", [READ_LOW - 22.0, 313.0, 44.0, 16.0], TextFormat::Percent, 0);
    ui.readout("velocity_high", "Highest velocity", [READ_HIGH - 22.0, 313.0, 44.0, 16.0], TextFormat::Percent, 0);
    ui.dim("Blend dimmed", &[dial_at(1, 6)], &blend_silent());
}

/// The Sample page (page 2): the waveform, the sample analysis under it, one row of dials and
/// Playback.
pub(super) fn sample_page(ui: &mut Canvas) {
    ui.page(2);
    // The waveform as the page's picture: Start and End, or the loop with Loop, are dragged on it.
    let wave = [56.0, 98.0, 768.0, 112.0];
    ui.waveform(wave);
    ui.graphic(
        "Sample captions",
        vec![
            caption([56.0, 80.0], "Start"),
            caption([730.0, 80.0], "End"),
            caption([56.0, 222.0], "Loop start"),
            caption([690.0, 222.0], "Loop end"),
            line([56.0, 386.5], [824.0, 386.5], HAIRLINE, 1.0),
        ],
    );
    for (id, name, bounds) in [
        ("start", "Source start", [104.0, 76.0, 56.0, 16.0]),
        ("end", "Source end", [768.0, 76.0, 56.0, 16.0]),
        ("loop_start", "Loop start", [150.0, 218.0, 56.0, 16.0]),
        ("loop_end", "Loop end", [768.0, 218.0, 56.0, 16.0]),
    ] {
        ui.readout(id, name, bounds, TextFormat::Percent, 1);
    }
    // The sample analysis on the page itself, under the waveform: it reads the same file, and
    // its proposals change the controls on this page.
    ui.add(
        "Sample analysis",
        Widget::SampleAnalysis,
        [56.0, 244.0, 768.0, 136.0],
        None,
        UiGesture::Drag,
        "Analyze the loaded sample on this computer: check the root it proposes, or fit a model from it; nothing changes until you accept",
    );
    for (n, (id, name, label, tooltip)) in [
        ("root", "Root key", "Root", "The key that plays the file at its own pitch"),
        ("attack", "Attack", "Attack", "Amplitude envelope rise, sample and fitted models"),
        ("decay", "Decay", "Decay", "Amplitude envelope fall to Sustain"),
        ("sustain", "Sustain", "Sustain", "Amplitude envelope held level"),
        ("release", "Release", "Release", "After note-off, both sources"),
    ]
    .into_iter()
    .enumerate()
    {
        ui.face_dial(id, name, dial_at(n, 5), tooltip, "knob-small");
        ui.caption(label);
    }
    // Playback at the bottom right, as the mallet model sits on the Model page.
    let playback: [(u16, &str, &str, [f32; 4], &str); 3] = [
        (0, "Gate", "GATE", [584.0, 518.0, 64.0, 22.0], "Plays while the key is held"),
        (1, "One shot", "ONE SHOT", [656.0, 518.0, 88.0, 22.0], "Plays to the end"),
        (2, "Loop", "LOOP", [752.0, 518.0, 72.0, 22.0], "Repeats the loop while the key is held"),
    ];
    for (value, name, label, bounds, tooltip) in playback {
        ui.choice_labelled("playback", value, name, label, bounds, tooltip);
    }
    // Faded with Modal: the sample's own controls; the envelope's, but for fitted models.
    ui.dim(
        "Sample dimmed",
        &[[56.0, 72.0, 768.0, 166.0], dial_at(0, 5), [584.0, 518.0, 240.0, 22.0]],
        &sample_silent(),
    );
    ui.dim("Envelope dimmed", &[dials_from(1, 3, 5)], &envelope_silent());
}
