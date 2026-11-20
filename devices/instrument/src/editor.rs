// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Expanded pages expose the existing engine honestly; page selection stays in the host.

use super::Canvas;
use suhina_device_sdk::ui::{DeviceUi, UiGesture, Widget};

/// The revisions whose Model page shows Strike position and the bar controls.
const STRUCK_OR_BARS: &[u16] = &[1, 3];

pub(super) fn canvas() -> DeviceUi {
    let mut ui = Canvas::new([880.0, 520.0]);
    ui.title("Instrument", [24.0, 16.0, 400.0, 34.0], 28.0);
    ui.label("Experimental struck modal · revision 1", [24.0, 54.0, 450.0, 20.0]);
    ui.only_revision(1);
    ui.label("Experimental modal profile · revision 2", [24.0, 54.0, 450.0, 20.0]);
    ui.only_revision(2);
    ui.label("Experimental struck and bar bodies · revision 3", [24.0, 54.0, 450.0, 20.0]);
    ui.only_revision(3);
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
    ui.dial("gain", "Output gain", [48.0, 180.0, 144.0, 136.0], "Gain for both sources");
    ui.dial(
        "release",
        "Release time",
        [214.0, 180.0, 144.0, 136.0],
        "Release damping for modal voices and gated/looped samples",
    );
    ui.dial("blend", "Hybrid blend", [376.0, 192.0, 108.0, 116.0], "Hybrid only: move from modal body to local sample");
    ui.dial("attack", "Sample attack", [496.0, 192.0, 108.0, 116.0], "Sample layer only: onset fade time");
    ui.dial("sustain", "Sample sustain", [616.0, 192.0, 108.0, 116.0], "Sample layer only: held amplitude level");
    ui.dial(
        "stereo_width",
        "Stereo width",
        [736.0, 192.0, 108.0, 116.0],
        "Bar bodies: spread the bars from left (low) to right (high) like a row of bars",
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
        "Key numbers: C4 = 60. Pitch wheel: ±2 semitones. Sustain pedal holds released keys.",
        [24.0, 486.0, 832.0, 24.0],
    );

    ui.page(1);
    ui.group("STRUCK BODY", [24.0, 144.0, 832.0, 244.0]);
    ui.dial(
        "body_decay",
        "Body decay",
        [54.0, 188.0, 170.0, 164.0],
        "Struck: fundamental T60. Profile and bars: scales their decays relative to 2 seconds. Next strike only",
    );
    ui.dial(
        "brightness",
        "Brightness",
        [248.0, 188.0, 170.0, 164.0],
        "Energy in the higher resonances; affects the next strike",
    );
    ui.dial(
        "structure",
        "Structure",
        [446.0, 208.0, 150.0, 138.0],
        "Stretch the original or accepted resonance frequencies; affects the next strike",
    );
    ui.dial(
        "strike",
        "Strike position",
        [646.0, 208.0, 150.0, 138.0],
        "Excitation position along the resonant body; on a bar, from its centre (lowest) to its end. Next strike",
    );
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.dial(
        "strike",
        "Strike colour",
        [646.0, 208.0, 150.0, 138.0],
        "Creative excitation weighting of accepted modes, not a measured impact position; next strike only",
    );
    ui.only_revision(2);
    ui.dial(
        "strike",
        "Strike colour",
        [646.0, 208.0, 150.0, 138.0],
        "Creative excitation weighting of fitted modes, not a measured impact position; next strike only",
    );
    ui.only_revision(4);
    ui.label("Modes per voice", [40.0, 416.0, 196.0, 20.0]);
    ui.menu("quality", "Modes per voice", [240.0, 412.0, 132.0, 30.0], "8 or 16 resonances per modal voice");
    ui.label("Modal and Hybrid sources · up to 16 modal voices", [400.0, 416.0, 440.0, 20.0]);
    ui.only_revision(2);
    // Bar bodies and their mallet (revision 3, and 1 read as 3); a profile or fitted set ignores them.
    ui.label("Body", [392.0, 394.0, 120.0, 18.0]);
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.menu("body", "Body", [392.0, 416.0, 120.0, 28.0], "Struck, or an experimental Marimba or Vibraphone bar");
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.label("Mallet", [520.0, 394.0, 104.0, 18.0]);
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.menu(
        "mallet_model",
        "Mallet model",
        [520.0, 416.0, 104.0, 28.0],
        "Bars: Contact simulates the mallet against the moving bar; Pulse adds a fixed force pulse",
    );
    ui.only_revisions(STRUCK_OR_BARS, 1);
    ui.number("mallet_hardness", "Hardness", 632.0, 394.0, 104.0);
    ui.only_revisions(STRUCK_OR_BARS, 2);
    ui.number("resonator", "Resonator", 744.0, 394.0, 104.0);
    ui.only_revisions(STRUCK_OR_BARS, 2);
    ui.label(
        "An original experimental resonator, not a calibrated acoustic instrument. Changes shape the next strike.",
        [24.0, 486.0, 832.0, 24.0],
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
    ui.dial("attack", "Sample fade in", [24.0, 386.0, 104.0, 104.0], "Sample layer attack time");
    ui.dial("sustain", "Held sample level", [148.0, 386.0, 120.0, 104.0], "Sample layer held amplitude");
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
