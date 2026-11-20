// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Expanded pages expose the existing engine honestly; page selection stays in the host.

use super::Canvas;
use suhina_device_sdk::ui::{DeviceUi, UiGesture, Widget};

pub(super) fn canvas() -> DeviceUi {
    let mut ui = Canvas::new([880.0, 520.0]);
    ui.title("Instrument", [24.0, 16.0, 400.0, 34.0], 28.0);
    ui.label("Experimental struck modal · revision 1", [24.0, 54.0, 450.0, 20.0]);
    ui.only_revision(1);
    ui.label("Experimental modal profile · revision 2", [24.0, 54.0, 450.0, 20.0]);
    ui.only_revision(2);
    ui.label("Experimental fitted model · revision 4", [24.0, 54.0, 450.0, 20.0]);
    ui.only_revision(4);
    ui.label("SOURCE", [632.0, 16.0, 220.0, 18.0]);
    ui.menu(
        "source",
        "Editor source",
        [632.0, 40.0, 220.0, 30.0],
        "Choose the modal body, local sample, or their Hybrid mix",
    );
    for (i, name) in ["Play", "Model", "Sample"].into_iter().enumerate() {
        ui.navigate(name, [24.0 + i as f32 * 144.0, 92.0, 132.0, 32.0], i as u16, false);
    }

    ui.page(0);
    ui.group("PERFORMANCE", [24.0, 144.0, 832.0, 192.0]);
    ui.dial("gain", "Output gain", [40.0, 192.0, 120.0, 116.0], "Gain for both sources");
    ui.dial(
        "release",
        "Release",
        [174.0, 192.0, 120.0, 116.0],
        "After note-off: the damper for fitted models (caps every T60), the fade for samples and the original model",
    );
    ui.dial("blend", "Hybrid blend", [308.0, 192.0, 120.0, 116.0], "Hybrid only: move from modal body to local sample");
    ui.dial(
        "attack",
        "Attack",
        [442.0, 192.0, 120.0, 116.0],
        "Amplitude envelope rise for the sample and fitted models; 0 keeps a fitted recording's own attack",
    );
    ui.dial(
        "decay",
        "Decay",
        [576.0, 192.0, 120.0, 116.0],
        "Amplitude envelope fall to Sustain, for the sample and fitted models",
    );
    ui.dial(
        "sustain",
        "Sustain",
        [710.0, 192.0, 120.0, 116.0],
        "Amplitude envelope held level for the sample and fitted models; 100 % plays a recording as fitted",
    );
    ui.group("PLAYABLE RANGE", [24.0, 352.0, 832.0, 112.0]);
    for (i, (id, label)) in [
        ("key_low", "Lowest key"),
        ("key_high", "Highest key"),
        ("velocity_low", "Lowest velocity"),
        ("velocity_high", "Highest velocity"),
    ]
    .into_iter()
    .enumerate()
    {
        ui.number(id, label, 44.0 + i as f32 * 206.0, 390.0, 174.0);
    }
    ui.label(
        "Keys: C3 = 60. Pitch wheel ±2 semitones. Sustain pedal holds keys; CC 67 is the soft pedal.",
        [24.0, 486.0, 640.0, 24.0],
    );
    ui.toggle(
        "soft_pedal",
        "Soft pedal",
        [704.0, 480.0, 152.0, 32.0],
        "Una corda for fitted models: quieter, darker strikes without beating, as with a soft pedal (CC 67) held down",
    );

    ui.page(1);
    ui.group("STRUCK BODY", [24.0, 144.0, 832.0, 180.0]);
    ui.dial(
        "body_decay",
        "Body decay",
        [54.0, 170.0, 150.0, 140.0],
        "Original model: fundamental T60. Profile: scales stored decays relative to 2 seconds. Next strike only",
    );
    ui.dial(
        "brightness",
        "Brightness",
        [248.0, 170.0, 150.0, 140.0],
        "Energy in the higher resonances; affects the next strike",
    );
    ui.dial(
        "structure",
        "Structure",
        [446.0, 170.0, 150.0, 140.0],
        "Stretch the original or accepted resonance frequencies; affects the next strike",
    );
    ui.dial(
        "strike",
        "Strike position",
        [646.0, 170.0, 150.0, 140.0],
        "Excitation position along the resonant body; affects the next strike",
    );
    ui.only_revision(1);
    ui.dial(
        "strike",
        "Strike colour",
        [646.0, 170.0, 150.0, 140.0],
        "Creative excitation weighting of accepted modes, not a measured impact position; next strike only",
    );
    ui.only_revision(2);
    ui.dial(
        "strike",
        "Strike colour",
        [646.0, 170.0, 150.0, 140.0],
        "Creative excitation weighting of fitted modes, not a measured impact position; next strike only",
    );
    ui.only_revision(4);
    // Fitted models: several recordings, the hammer and the pedals.
    ui.group("FITTED MODEL", [24.0, 334.0, 832.0, 112.0]);
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
    ]
    .into_iter()
    .enumerate()
    {
        ui.dial(id, name, [54.0 + i as f32 * 196.0, 350.0, 110.0, 92.0], hint);
        ui.only_revision(4);
    }
    ui.label("Modes per voice", [40.0, 456.0, 196.0, 20.0]);
    ui.menu(
        "quality",
        "Modes per voice",
        [240.0, 452.0, 132.0, 30.0],
        "8 or 16 resonances per modal voice; fitted models: 32 or 64 sines",
    );
    ui.label("Modal and Hybrid sources · up to 16 modal voices", [400.0, 456.0, 440.0, 20.0]);
    ui.label(
        "An experimental resonator or a model fitted from recordings, not a calibrated instrument. Changes shape the next strike.",
        [24.0, 490.0, 832.0, 24.0],
    );

    ui.page(2);
    ui.waveform([24.0, 144.0, 832.0, 140.0]);
    ui.number("root", "Root key", 24.0, 304.0, 130.0);
    ui.label("Playback", [182.0, 304.0, 148.0, 18.0]);
    ui.menu(
        "playback",
        "Sample playback",
        [182.0, 326.0, 148.0, 28.0],
        "Gate follows the key, One shot runs to the end, Loop repeats the loop region",
    );
    for (i, (id, label)) in
        [("start", "Source start"), ("end", "Source end"), ("loop_start", "Loop start"), ("loop_end", "Loop end")]
            .into_iter()
            .enumerate()
    {
        ui.number(id, label, 354.0 + i as f32 * 128.0, 304.0, 116.0);
    }
    ui.dial("attack", "Attack", [24.0, 386.0, 84.0, 104.0], "Amplitude envelope rise, sample and fitted models");
    ui.dial("decay", "Decay", [112.0, 386.0, 84.0, 104.0], "Amplitude envelope fall to Sustain");
    ui.dial("sustain", "Sustain", [200.0, 386.0, 92.0, 104.0], "Amplitude envelope held level");
    ui.add(
        "Sample analysis",
        Widget::SampleAnalysis,
        [300.0, 386.0, 548.0, 124.0],
        None,
        UiGesture::Drag,
        "Analyze the loaded sample locally, check its root proposal, then accept one undoable change",
    );
    ui.finish()
}
