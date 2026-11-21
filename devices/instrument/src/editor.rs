// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The expanded editor (880 × 500, docs/spec/instrument-editor.md): along the top the logo, the
//! two layers (each one's lamp turns it on or off, its name shows it) and the keys; under them
//! the layer shown, the body's or the sample's, or the keys and velocities; along the bottom on
//! every page Blend, Release and Gain. Which page shows stays in the host.

use super::{BRASS, Canvas, HAIRLINE, INK, add, any_of, line, not, page_is, spaced, text, times, value_is, value_text};
use suhina_device_sdk::ui::{DeviceUi, Draw, PAGE_ID, TextAlign, TextFormat, UiGesture, UiValue, Widget};

/// The editor's look on top of the shared sheet (docs/spec/instrument-editor.md › Appearance and
/// validation), with every theme: buttons as text with only the chosen one underlined, in brass.
const EDITOR_STYLE: &str = "button { border-color: transparent; border-width: 0 0 2px 0; }
button[checked] { border-color: #b8742f; }";
/// The revisions whose body page shows #1599's struck-body group: a profile or a fitted set.
const PROFILE_OR_FITTED: &[u16] = &[2, 4];
/// Fainter espresso text.
const MUTED: [u8; 4] = super::MUTED;
/// A lit lamp's brass and its glow; an unlit lamp's ring.
const LAMP: [u8; 4] = [184, 116, 47, 255];
const LAMP_GLOW: [u8; 4] = [214, 160, 86, 90];
const LAMP_OFF: [u8; 4] = [107, 85, 67, 200];
/// The rail of Blend and the face of its thumb.
const RAIL: [u8; 4] = [52, 38, 29, 70];
const THUMB: [u8; 4] = [251, 246, 236, 255];
/// The card a switched-off layer's note stands on.
const CARD: [u8; 4] = [239, 228, 210, 236];

/// The editor's pages (`#page`): the body layer, the sample layer, the body's Advanced page, and
/// the keys and velocities over the body layer or over the sample layer, so Done goes back to the
/// layer it came from.
pub(crate) const BODY: u16 = 0;
pub(crate) const SAMPLE: u16 = 1;
pub(crate) const ADVANCED: u16 = 2;
pub(crate) const KEYS_OVER_BODY: u16 = 3;
pub(crate) const KEYS_OVER_SAMPLE: u16 = 4;
/// The editor's last page.
pub(crate) const LAST_PAGE: u16 = KEYS_OVER_SAMPLE;

/// The small knob's turn, as the large dial's: from 150° left of the top to 150° right of it.
const TURN_START: f32 = 2.094_395;
const TURN_SPAN: f32 = 5.235_988;

pub(super) fn canvas() -> DeviceUi {
    let mut ui = Canvas::new([880.0, 500.0]);
    ui.artwork(super::EDITOR_LIGHT);
    ui.style(EDITOR_STYLE);
    ui.asset("logo", super::LOGO);
    ui.asset("knob-large", super::KNOB_LARGE);
    ui.asset("knob-small", super::KNOB_SMALL);
    // The logo in place of a title, and the model in small capitals under it.
    ui.add(
        "Suhina",
        Widget::Graphic { drawing: vec![Draw::image("logo", [0.0, 0.0, 108.0, 30.0])] },
        [386.0, 18.0, 108.0, 30.0],
        None,
        UiGesture::Drag,
        "",
    );
    for (revision, name) in [
        (1, "Struck modal · experimental"),
        (2, "Modal profile · experimental"),
        (3, "Modal bodies · experimental"),
        (4, "Fitted model · experimental"),
    ] {
        ui.text(name, &name.to_uppercase(), [190.0, 52.0, 500.0, 12.0], 8.5, TextAlign::Center, MUTED);
        ui.only_revision(revision);
    }
    layers(&mut ui);
    keys_button(&mut ui);

    // The body layer: the bodies (revisions 1 and 3) with their Advanced page, or a profile's or
    // fitted model's groups (revisions 2 and 4).
    super::editor_bodies::body_page(&mut ui);
    profile_page(&mut ui);
    // The sample layer: the file, its waveform, the envelope, the root and the analysis.
    super::editor_pages::sample_page(&mut ui);
    // The keys and velocities, over either layer.
    super::editor_pages::keys_page(&mut ui);
    layer_off(&mut ui, BODY, "body", 1);
    layer_off(&mut ui, SAMPLE, "sample", 0);
    strip(&mut ui);
    ui.finish()
}

/// The two layers in the header. A layer's lamp turns it on or off by setting Source (Modal plays
/// the body alone, Sample the sample alone, Hybrid both); the one layer left on cannot be turned
/// off, so its lamp is only drawn. A layer's name shows it, underlined while it shows (with the
/// body's Advanced page and the keys over a layer, by a drawn line under it).
fn layers(ui: &mut Canvas) {
    ui.every_page();
    // (name, the page that shows it, the lamp's left, the name's bounds, Source with the other
    // layer alone)
    for (name, page, lamp_x, bounds, other) in [
        ("Body", BODY, 318.0, [348.0, 72.0, 55.0, 24.0], 1_u16),
        ("Sample", SAMPLE, 451.0, [481.0, 72.0, 80.0, 24.0], 0),
    ] {
        let center = [lamp_x + 10.0, 84.0];
        let lit = vec![
            Draw::Circle { center: [center[0].into(), center[1].into()], radius: 8.0.into(), color: LAMP_GLOW },
            Draw::Circle { center: [center[0].into(), center[1].into()], radius: 5.0.into(), color: LAMP },
        ];
        let unlit = vec![Draw::Arc {
            center: [center[0].into(), center[1].into()],
            radius: 5.0.into(),
            start: 0.0.into(),
            end: std::f32::consts::TAU.into(),
            color: LAMP_OFF,
            width: 1.4,
        }];
        let layer = name.to_lowercase();
        ui.graphic(&format!("{name} lamp lit"), lit);
        ui.show_when(not(value_is("source", other)));
        ui.graphic(&format!("{name} lamp unlit"), unlit);
        ui.show_when(value_is("source", other));
        let lamp = [lamp_x, 74.0, 20.0, 20.0];
        // On with the other: turning it off leaves the other alone.
        ui.choice_labelled("source", other, &format!("Turn off the {layer} layer"), "", lamp, "Turn this layer off");
        ui.show_when(value_is("source", 2));
        // Off: turning it on plays both.
        ui.choice_labelled("source", 2, &format!("Turn on the {layer} layer"), "", lamp, "Turn this layer on");
        ui.show_when(value_is("source", other));
        ui.choice_labelled(
            PAGE_ID,
            page,
            &format!("Show the {layer} layer"),
            &spaced(name),
            bounds,
            "Show this layer's controls; its lamp turns it on or off",
        );
    }
    // The Advanced page and the keys are the body's, or the sample's, still.
    let underline = |x0: f32, x1: f32| vec![line([x0, 95.0], [x1, 95.0], [184, 116, 47, 255], 2.0)];
    ui.graphic("Body layer underline", underline(348.0, 403.0));
    ui.show_when(any_of(PAGE_ID, &[ADVANCED, KEYS_OVER_BODY]));
    ui.graphic("Sample layer underline", underline(481.0, 561.0));
    ui.show_when(page_is(KEYS_OVER_SAMPLE));
}

/// The keys at the top right: the playable range, which opens the keys and velocities over the
/// layer shown, and closes them again.
fn keys_button(ui: &mut Canvas) {
    ui.every_page();
    ui.text("Keys caption", &spaced("Keys"), [787.0, 16.0, 40.0, 10.0], 8.0, TextAlign::Left, MUTED);
    let bounds = [762.0, 30.0, 62.0, 22.0];
    let tooltip = "Playable keys, velocities and the soft pedal";
    for (from, to) in [
        (&[BODY, ADVANCED][..], KEYS_OVER_BODY),
        (&[SAMPLE][..], KEYS_OVER_SAMPLE),
        (&[KEYS_OVER_BODY][..], BODY),
        (&[KEYS_OVER_SAMPLE][..], SAMPLE),
    ] {
        ui.choice_labelled(PAGE_ID, to, "Keys and velocity", "", bounds, tooltip);
        ui.show_when(any_of(PAGE_ID, from));
    }
    let range = vec![
        value_text([773.0.into(), 34.0.into()], "", "key_low", TextFormat::Key, 10.5, TextAlign::Center, INK),
        text([791.0, 34.0], "–", 10.5, TextAlign::Center, INK),
        value_text([807.0.into(), 34.0.into()], "", "key_high", TextFormat::Key, 10.5, TextAlign::Center, INK),
        line([762.0, 52.5], [824.0, 52.5], HAIRLINE, 1.0),
    ];
    ui.graphic("Keys range", range);
    ui.graphic(
        "Keys closed",
        vec![line([816.0, 39.0], [819.5, 42.5], INK, 1.2), line([819.5, 42.5], [823.0, 39.0], INK, 1.2)],
    );
    ui.show_when(any_of(PAGE_ID, &[BODY, SAMPLE, ADVANCED]));
    ui.graphic(
        "Keys open",
        vec![line([816.0, 42.5], [819.5, 39.0], INK, 1.2), line([819.5, 39.0], [823.0, 42.5], INK, 1.2)],
    );
    ui.show_when(any_of(PAGE_ID, &[KEYS_OVER_BODY, KEYS_OVER_SAMPLE]));
}

/// A small knob for `parameter`, 34 points square: the canvas's `knob-small` face, a groove, a
/// brass arc to the value and a pointer, drawn whole, for a dial whose caption stands beside it.
pub(super) fn small_knob(parameter: &str) -> Vec<Draw> {
    let c = 17.0;
    let turn = add(TURN_START.into(), times(UiValue::normalized(parameter, false), TURN_SPAN));
    let at = |radius: f32| {
        [
            add(
                c.into(),
                times(
                    UiValue::Expression(Box::new(suhina_device_sdk::ui::UiExpression::Cos { value: turn.clone() })),
                    radius,
                ),
            ),
            add(
                c.into(),
                times(
                    UiValue::Expression(Box::new(suhina_device_sdk::ui::UiExpression::Sin { value: turn.clone() })),
                    radius,
                ),
            ),
        ]
    };
    let arc = |end: UiValue, color: [u8; 4], width: f32| Draw::Arc {
        center: [c.into(), c.into()],
        radius: 16.0.into(),
        start: TURN_START.into(),
        end,
        color,
        width,
    };
    vec![
        arc((TURN_START + TURN_SPAN).into(), [52, 38, 29, 40], 2.0),
        arc(turn.clone(), LAMP, 2.2),
        Draw::image("knob-small", [4.0, 4.0, 26.0, 26.0]),
        Draw::Line { from: at(3.0), to: at(10.0), color: [42, 28, 19, 255], width: 1.6 },
    ]
}

/// A small knob for `id` at `at` (its top left), its caption in spaced capitals and its value
/// beside it at the right.
pub(super) fn knob_beside(
    ui: &mut Canvas,
    (id, name, caption): (&str, &str, &str),
    at: [f32; 2],
    format: TextFormat,
    tooltip: &str,
) {
    ui.drawn_dial(id, name, [at[0], at[1], 34.0, 34.0], small_knob(id), tooltip);
    ui.add(
        &format!("{name} caption"),
        Widget::Graphic {
            drawing: vec![
                text([0.0, 0.0], &spaced(caption), 8.5, TextAlign::Left, MUTED),
                value_text([0.0.into(), 16.0.into()], "", id, format, 11.0, TextAlign::Left, INK),
            ],
        },
        [at[0] + 44.0, at[1] + 2.0, 120.0, 30.0],
        None,
        UiGesture::Drag,
        "",
    );
}

/// The strip along the bottom of every page: Blend from the body to the sample (only with both
/// layers on), Release and Gain, which act on both layers.
fn strip(ui: &mut Canvas) {
    ui.every_page();
    let rail = (150.0, 380.0, 467.0);
    let thumb_x = add(times(UiValue::normalized("blend", false), rail.1 - rail.0), (rail.0 - 5.0).into());
    ui.graphic(
        "Strip",
        vec![
            line([56.0, 432.5], [824.0, 432.5], HAIRLINE, 1.0),
            text([56.0, 447.0], &spaced("Blend"), 8.5, TextAlign::Left, MUTED),
            text([rail.0, 446.0], &spaced("Body"), 7.5, TextAlign::Left, MUTED),
            text([rail.1 - 54.0, 446.0], &spaced("Sample"), 7.5, TextAlign::Left, MUTED),
            line([rail.0, rail.2], [rail.1, rail.2], RAIL, 2.0),
            Draw::Rect {
                rect: [add(thumb_x.clone(), (-1.0).into()), 456.0.into(), 12.0.into(), 22.0.into()],
                color: INK,
                radius: 3.0,
            },
            Draw::Rect {
                rect: [add(thumb_x.clone(), 0.5.into()), 457.5.into(), 9.0.into(), 19.0.into()],
                color: THUMB,
                radius: 2.0,
            },
            Draw::Line {
                from: [add(thumb_x.clone(), 5.0.into()), 461.0.into()],
                to: [add(thumb_x, 5.0.into()), 473.0.into()],
                color: BRASS,
                width: 1.2,
            },
        ],
    );
    ui.graphic(
        "Blend value",
        vec![value_text([56.0.into(), 463.0.into()], "", "blend", TextFormat::Percent, 11.0, TextAlign::Left, INK)],
    );
    ui.show_when(value_is("source", 2));
    ui.graphic("Blend one layer", vec![text([56.0, 463.0], "one layer on", 11.0, TextAlign::Left, MUTED)]);
    ui.show_when(not(value_is("source", 2)));
    ui.slider(
        "blend",
        "Hybrid blend",
        [rail.0, 452.0, rail.1 - rail.0, 30.0],
        "With both layers on: from the body to the sample",
    );
    ui.dim("Blend dimmed", &[[rail.0 - 8.0, 444.0, rail.1 - rail.0 + 16.0, 40.0]], &not(value_is("source", 2)));
    knob_beside(
        ui,
        ("release", "Release", "Release"),
        [500.0, 446.0],
        TextFormat::Seconds,
        "After note-off: the damper for fitted models (caps every T60), the fade for samples and the original model",
    );
    knob_beside(ui, ("gain", "Output gain", "Gain"), [688.0, 446.0], TextFormat::Decibels, "Gain for both layers");
}

/// The note a switched-off layer's page shows over its picture: that the layer is off, and a way
/// to turn it on (both layers then play). Its controls stay, faded, so values can be prepared.
fn layer_off(ui: &mut Canvas, page: u16, layer: &str, alone_other: u16) {
    ui.page(page);
    let off = value_is("source", alone_other);
    let card = [226.0, 176.0, 428.0, 84.0];
    ui.graphic(
        &format!("{layer} layer off"),
        vec![
            Draw::rect(card, CARD),
            line([card[0], card[1] + 0.5], [card[0] + card[2], card[1] + 0.5], HAIRLINE, 1.0),
            line([card[0], card[1] + card[3] - 0.5], [card[0] + card[2], card[1] + card[3] - 0.5], HAIRLINE, 1.0),
            text([440.0, 200.0], &spaced(&format!("The {layer} layer is off")), 9.0, TextAlign::Center, INK),
        ],
    );
    ui.show_when(both_page(page, off.clone()));
    ui.choice_labelled(
        "source",
        2,
        &format!("Turn the {layer} layer on"),
        &spaced("Turn it on"),
        [380.0, 222.0, 120.0, 22.0],
        "Play both layers",
    );
    ui.show_when(both_page(page, off));
}

fn both_page(page: u16, condition: UiValue) -> UiValue {
    super::both(page_is(page), condition)
}

/// The body layer for a profile or a fitted model (revisions 2 and 4): the struck body's dials,
/// the fitted model's (revision 4), and Modes per voice.
fn profile_page(ui: &mut Canvas) {
    ui.page(BODY);
    ui.group("STRUCK BODY", [24.0, 104.0, 832.0, 132.0]);
    ui.only_revisions(PROFILE_OR_FITTED, 1);
    for (i, (id, name, tooltip)) in [
        (
            "body_decay",
            "Body decay",
            "Struck: fundamental T60. Profile, fitted model and bars: scales their decays relative to 2 seconds. Next strike only",
        ),
        ("brightness", "Brightness", "Energy in the higher resonances; affects the next strike"),
        ("structure", "Structure", "Stretch the original or accepted resonance frequencies; affects the next strike"),
    ]
    .into_iter()
    .enumerate()
    {
        ui.dial(id, name, [74.0 + i as f32 * 196.0, 124.0, 120.0, 104.0], tooltip);
        ui.only_revisions(PROFILE_OR_FITTED, 1);
    }
    for (revision, tooltip) in [
        (2, "Creative excitation weighting of accepted modes, not a measured impact position; next strike only"),
        (4, "Creative excitation weighting of fitted modes, not a measured impact position; next strike only"),
    ] {
        ui.dial("strike", "Strike color", [662.0, 124.0, 120.0, 104.0], tooltip);
        ui.only_revision(revision);
    }
    // Fitted models: several recordings, the hammer and the pedals.
    ui.group("FITTED MODEL", [24.0, 244.0, 832.0, 128.0]);
    ui.only_revision(4);
    for (i, (id, name, hint)) in [
        ("morph", "Source morph", "Weight between the model's sources (instruments); 50 % weighs every source equally"),
        (
            "velocity_brightness",
            "Velocity to brightness",
            "How much a harder or softer strike than the recording brightens or darkens it and changes its knock",
        ),
        ("damper_noise", "Damper noise", "The short noise of a damper landing on note-off"),
        ("pedal_resonance", "Pedal resonance", "Undamped strings ringing along while the sustain pedal is down"),
        (
            "model_tune",
            "Model tune",
            "Moves the model's pitch in semitones, for a model fitted at the wrong root or a few cents off",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        ui.dial(id, name, [54.0 + i as f32 * 160.0, 264.0, 110.0, 100.0], hint);
        ui.only_revision(4);
    }
    ui.label("Modes per voice", [40.0, 386.0, 196.0, 20.0]);
    ui.only_revisions(PROFILE_OR_FITTED, 1);
    ui.menu(
        "quality",
        "Modes per voice",
        [240.0, 382.0, 132.0, 30.0],
        "8 or 16 resonances per modal voice; fitted models: 64 or 128 sines",
    );
    ui.only_revisions(PROFILE_OR_FITTED, 1);
    ui.label(
        "An experimental resonator or a model fitted from recordings, not a calibrated instrument.",
        [400.0, 386.0, 456.0, 20.0],
    );
    ui.only_revisions(PROFILE_OR_FITTED, 1);
}
