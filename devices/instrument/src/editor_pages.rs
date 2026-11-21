// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's sample layer and its keys page (docs/spec/instrument-editor.md): the loaded file,
//! Playback, the waveform with its region and loop, the envelope, the root and the analysis; the
//! playable keys and velocities and the soft pedal, over either layer.

use super::editor::{KEYS_OVER_BODY, KEYS_OVER_SAMPLE, SAMPLE, knob_beside};
use super::editor_bodies::radio_dot;
use super::{
    BRASS, Canvas, HAIRLINE, INK, MUTED, VALUE, add, envelope_silent, line, sample_silent, spaced, text, times,
    value_text,
};
use suhina_device_sdk::ui::{Draw, PAGE_ID, TextAlign, TextFormat, UiExpression, UiGesture, UiValue, Widget};

/// The keys: their ground, the playable ones, the black keys, the octaves, and the veil over the
/// keys outside the range.
const KEY_GROUND: [u8; 4] = [52, 38, 29, 14];
const KEY_LIT: [u8; 4] = [251, 246, 236, 255];
const KEY_BLACK: [u8; 4] = [58, 42, 31, 170];
const KEY_OUTSIDE: [u8; 4] = [239, 228, 210, 150];
/// The velocity wedge and its admitted part.
const WEDGE: [u8; 4] = [52, 38, 29, 18];
const WEDGE_LIT: [u8; 4] = [184, 116, 47, 150];
/// The keys page's ground over the body layer's sound.
const GROUND: [u8; 4] = [239, 228, 210, 222];

/// The keys page: the 128 keys, one column of `KEY` points each from `LEFT`, their top, their
/// height and the black keys' height, the range bar above them; the velocity wedge's base and
/// its rise; the right-hand readouts' centers (the lower value, the dash, the higher value).
const LEFT: f32 = 56.0;
const KEY: f32 = 6.0;
const KEYS_TOP: f32 = 254.0;
const KEYS_HEIGHT: f32 = 36.0;
const BLACK_HEIGHT: f32 = 22.0;
const RANGE_Y: f32 = 248.0;
const WEDGE_BASE: f32 = 380.0;
const WEDGE_RISE: f32 = 22.0;
const READ_LOW: f32 = 760.0;
const READ_DASH: f32 = 782.0;
const READ_HIGH: f32 = 804.0;

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

/// A caption in spaced capitals.
fn caption(position: [f32; 2], name: &str) -> Draw {
    text(position, &spaced(name), 8.5, TextAlign::Left, MUTED)
}

/// The sample layer (page `SAMPLE`): the file and Playback at the top, the waveform with its loop
/// above it and its region under it, then the envelope, the root key and the analysis.
pub(super) fn sample_page(ui: &mut Canvas) {
    ui.page(SAMPLE);
    // The loaded file's name and its facts, or that none is loaded.
    ui.add("Sample file", Widget::SampleFile, [56.0, 106.0, 260.0, 40.0], None, UiGesture::Drag, "");
    // Playback, as dots before its three choices.
    ui.graphic("Playback caption", vec![caption([471.0, 114.0], "Playback")]);
    for (value, name, label, x, w, tooltip) in [
        (0_u16, "Gate", "GATE", 589.0, 40.0, "Plays while the key is held"),
        (1, "One shot", "ONE SHOT", 667.0, 78.0, "Plays to the end once struck"),
        (2, "Loop", "LOOP", 783.0, 41.0, "Repeats the loop while the key is held"),
    ] {
        ui.choice_labelled("playback", value, name, label, [x, 108.0, w, 20.0], tooltip);
        radio_dot(ui, "playback", value, [x - 10.0, 117.0]);
    }
    // The loop above the waveform: its two ends, dragged too.
    ui.graphic(
        "Loop caption",
        vec![
            text([306.0, 126.0], &spaced("Loop"), 9.0, TextAlign::Left, VALUE),
            text([401.0, 126.0], "–", 10.5, TextAlign::Center, VALUE),
        ],
    );
    ui.readout("loop_start", "Loop start", [352.0, 122.0, 44.0, 16.0], TextFormat::Percent, 1);
    ui.readout("loop_end", "Loop end", [406.0, 122.0, 44.0, 16.0], TextFormat::Percent, 1);
    // The waveform: Start and End, or the loop with Loop, are dragged on it.
    ui.waveform([56.0, 144.0, 768.0, 142.0]);
    ui.graphic("Region captions", vec![caption([56.0, 296.0], "Start"), caption([640.0, 296.0], "End")]);
    ui.readout("start", "Source start", [100.0, 292.0, 52.0, 16.0], TextFormat::Percent, 1);
    ui.readout("end", "Source end", [674.0, 292.0, 52.0, 16.0], TextFormat::Percent, 1);
    ui.graphic(
        "Sample dividers",
        vec![
            line([56.0, 320.5], [824.0, 320.5], HAIRLINE, 1.0),
            line([526.5, 334.0], [526.5, 420.0], HAIRLINE, 1.0),
            caption([56.0, 336.0], "Envelope"),
            caption([552.0, 336.0], "Root key"),
            line([552.0, 396.5], [632.0, 396.5], HAIRLINE, 1.0),
        ],
    );
    // The amplitude envelope, shared by the sample and fitted models.
    for (n, (id, label, format, tooltip)) in [
        (
            "attack",
            "Attack",
            TextFormat::Seconds,
            "Amplitude envelope rise for the sample and fitted models; 0 keeps a fitted recording's own attack",
        ),
        ("decay", "Decay", TextFormat::Seconds, "Amplitude envelope fall to Sustain, for the sample and fitted models"),
        (
            "sustain",
            "Sustain",
            TextFormat::Percent,
            "Amplitude envelope held level for the sample and fitted models; 100 % plays a recording as fitted",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        knob_beside(ui, (id, label, label), [56.0 + n as f32 * 150.0, 360.0], format, tooltip);
    }
    // The root key, large, dragged up and down.
    ui.drawn_dial(
        "root",
        "Root key",
        [552.0, 352.0, 80.0, 44.0],
        vec![value_text([0.0.into(), 2.0.into()], "", "root", TextFormat::Key, 28.0, TextAlign::Left, INK)],
        "The key that plays the file at its own pitch; drag up or down",
    );
    // Faded with Modal: the sample's own controls; the envelope's, but for fitted models.
    ui.dim("Sample dimmed", &[[56.0, 104.0, 768.0, 210.0], [540.0, 330.0, 100.0, 94.0]], &sample_silent());
    ui.dim("Envelope dimmed", &[[56.0, 352.0, 460.0, 50.0]], &envelope_silent());
    // Last, over the veils: the local analysis: the root it suggests, Accept and Again, and fitting a model from the
    // file. It draws at the right of its bounds; a fitted model waiting for acceptance covers the
    // envelope and the root with its result.
    ui.add(
        "Sample analysis",
        Widget::SampleAnalysis,
        [56.0, 330.0, 768.0, 94.0],
        None,
        UiGesture::Drag,
        "Analyze the loaded sample on this computer: check the root it suggests, or fit a model from it; nothing changes until you accept",
    );
}

/// The keys page, over the body layer or the sample layer: the 128 keys with the playable ones
/// lit and their ends dragged, the velocities as a wedge, and the soft pedal. Done goes back to
/// the layer.
pub(super) fn keys_page(ui: &mut Canvas) {
    ui.pages(&[KEYS_OVER_BODY, KEYS_OVER_SAMPLE]);
    ui.graphic(
        "Keys ground",
        vec![Draw::rect([0.0, 160.0, 880.0, 272.0], GROUND), line([0.0, 160.5], [880.0, 160.5], HAIRLINE, 1.0)],
    );
    ui.text("Keys title", &spaced("Keys and velocity"), [56.0, 190.0, 400.0, 20.0], 15.0, TextAlign::Left, INK);
    for (page, back) in [(KEYS_OVER_BODY, super::editor::BODY), (KEYS_OVER_SAMPLE, SAMPLE)] {
        ui.choice_labelled(PAGE_ID, back, "Done", "DONE", [776.0, 190.0, 48.0, 20.0], "Back to the layer");
        ui.show_when(super::page_is(page));
    }

    // The keys: a column for each of the 128, the playable ones lit, middle C named, the sample's
    // root marked while the sample plays; the velocities as a wedge, the admitted part lit.
    let low = UiValue::parameter("key_low", false);
    let high = UiValue::parameter("key_high", false);
    let key_x = |key: UiValue, edge: f32| plus(times(key, KEY), LEFT + edge);
    let mut drawing = vec![
        caption([56.0, 228.0], "Key range"),
        value_text([READ_LOW.into(), 226.0.into()], "", "key_low", TextFormat::Key, 11.0, TextAlign::Center, INK),
        text([READ_DASH, 226.0], "–", 11.0, TextAlign::Center, INK),
        value_text([READ_HIGH.into(), 226.0.into()], "", "key_high", TextFormat::Key, 11.0, TextAlign::Center, INK),
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
        text([READ_DASH, 315.0], "–", 11.0, TextAlign::Center, INK),
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
    drawing.push(line([56.0, 396.5], [824.0, 396.5], HAIRLINE, 1.0));
    drawing.push(text([482.0, 409.0], &spaced("Glide time"), 8.5, TextAlign::Left, MUTED));
    drawing.push(text([660.0, 409.0], "C3 = 60  ·  pitch wheel ±2 semitones", 10.0, TextAlign::Left, MUTED));
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
            [LEFT, WEDGE_BASE - 28.0, 768.0, 32.0],
            Some((id, false)),
            None,
            None,
            false,
            "Drag sideways: notes softer or harder than the range do not play",
        );
    }
    ui.readout("velocity_low", "Lowest velocity", [READ_LOW - 22.0, 314.0, 44.0, 16.0], TextFormat::Percent, 0);
    ui.readout("velocity_high", "Highest velocity", [READ_HIGH - 22.0, 314.0, 44.0, 16.0], TextFormat::Percent, 0);
    ui.toggle(
        "soft_pedal",
        "Soft pedal",
        [56.0, 405.0, 120.0, 20.0],
        "Una corda for fitted models: quieter, darker strikes without beating, as with a soft pedal (CC 67) held down. Keys: C3 = 60; the pitch wheel bends ±2 semitones; the sustain pedal holds keys",
    );
    // Any fitted sound can glide: a note played while another key is held slides from its pitch.
    ui.toggle(
        "glide",
        "Legato glide",
        [196.0, 405.0, 140.0, 20.0],
        "Fitted models: a note played while another key is held glides from that key's pitch, with the recording's own glide shape when it has one",
    );
    ui.readout("glide_time", "Glide time", [572.0, 405.0, 56.0, 16.0], TextFormat::Seconds, 2);
}
