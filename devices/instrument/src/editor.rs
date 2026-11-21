// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Expanded pages expose the existing engine honestly; page selection stays in the host.

use super::Canvas;
use suhina_device_sdk::ui::{DeviceUi, Draw, TextAlign, UiGesture, Widget};

/// The editor's look (docs/spec/instrument-editor.md › Appearance and validation). Over the light
/// artwork: the logo in place of the title, no boxes (a section is a hairline over its heading),
/// buttons as text with the chosen one underlined in brass, quiet drop-downs, and espresso ink
/// with brass for the drawings' palette tokens and the dials' arcs. A dark theme hides the
/// artwork and the logo, shows the title and keeps its own look.
const EDITOR_STYLE: &str = "@media (prefers-color-scheme: dark) { #logo-light { display: none; } }
@media (prefers-color-scheme: light) {
  #title-dark { display: none; }
  device-section { background-color: transparent; color: #6b5543; border-color: rgba(52, 38, 29, 0.22);
    border-width: 1px 0 0 0; radius: 0; box-shadow: none; }
  button { image: none; background-color: transparent; color: #6b5543; border-color: transparent;
    border-width: 0 0 2px 0; radius: 0; box-shadow: none; accent-color: #b8742f; }
  button:hover { color: #34261d; }
  button[checked] { background-color: transparent; color: #34261d; border-color: #b8742f; }
  dropdown { background-color: transparent; color: #34261d; border-color: rgba(52, 38, 29, 0.35);
    border-width: 0 0 1px 0; radius: 0; }
  knob { color: #34261d; accent-color: #b8742f; }
  knob-ring { background-color: rgba(52, 38, 29, 0.14); }
  knob-pointer { color: #2a1c13; }
  label { color: #6b5543; }
  device-control { color: #34261d; accent-color: #b8742f; }
}";
/// The revisions whose Model page shows #1599's struck-body group: a profile or a fitted set.
const PROFILE_OR_FITTED: &[u16] = &[2, 4];
/// The theme's fainter text, a palette token.
const MUTED: [u8; 4] = [3, 1, 1, 170];

pub(super) fn canvas() -> DeviceUi {
    let mut ui = Canvas::new([880.0, 560.0]);
    ui.artwork(super::EDITOR_LIGHT);
    ui.style(EDITOR_STYLE);
    ui.asset("logo", super::LOGO);
    ui.asset("knob-large", super::KNOB_LARGE);
    ui.asset("knob-small", super::KNOB_SMALL);
    // The title shows with a dark theme, the logo over the light artwork.
    ui.title("Instrument", [24.0, 16.0, 400.0, 34.0], 28.0);
    ui.set_id("title-dark");
    ui.add(
        "Suhina",
        Widget::Graphic { drawing: vec![Draw::image("logo", [0.0, 0.0, 108.0, 30.0])] },
        [386.0, 18.0, 108.0, 30.0],
        None,
        UiGesture::Drag,
        "",
    );
    ui.set_id("logo-light");
    for (revision, name) in [
        (1, "Experimental struck modal · revision 1"),
        (2, "Experimental modal profile · revision 2"),
        (3, "Experimental struck and bar bodies · revision 3"),
        (4, "Experimental fitted model · revision 4"),
    ] {
        ui.text(name, &name.to_uppercase(), [190.0, 56.0, 500.0, 12.0], 8.5, TextAlign::Center, MUTED);
        ui.only_revision(revision);
    }
    ui.label("SOURCE", [724.0, 18.0, 100.0, 14.0]);
    ui.menu(
        "source",
        "Editor source",
        [724.0, 36.0, 100.0, 26.0],
        "Choose the modal body, local sample, or their Hybrid mix",
    );
    // The pages, at the bottom left, as text.
    for (i, name) in ["Play", "Model", "Sample"].into_iter().enumerate() {
        ui.navigate(name, [56.0 + i as f32 * 84.0, 518.0, 76.0, 22.0], i as u16, false);
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
        [24.0, 476.0, 640.0, 24.0],
    );
    ui.toggle(
        "soft_pedal",
        "Soft pedal",
        [704.0, 92.0, 152.0, 32.0],
        "Una corda for fitted models: quieter, darker strikes without beating, as with a soft pedal (CC 67) held down",
    );

    ui.page(1);
    ui.group("STRUCK BODY", [24.0, 144.0, 832.0, 180.0]);
    ui.only_revisions(PROFILE_OR_FITTED, 1);
    ui.dial(
        "body_decay",
        "Body decay",
        [54.0, 170.0, 150.0, 140.0],
        "Struck: fundamental T60. Profile, fitted model and bars: scales their decays relative to 2 seconds. Next strike only",
    );
    ui.only_revisions(PROFILE_OR_FITTED, 1);
    ui.dial(
        "brightness",
        "Brightness",
        [248.0, 170.0, 150.0, 140.0],
        "Energy in the higher resonances; affects the next strike",
    );
    ui.only_revisions(PROFILE_OR_FITTED, 1);
    ui.dial(
        "structure",
        "Structure",
        [446.0, 170.0, 150.0, 140.0],
        "Stretch the original or accepted resonance frequencies; affects the next strike",
    );
    ui.only_revisions(PROFILE_OR_FITTED, 1);
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
    ui.only_revisions(PROFILE_OR_FITTED, 1);
    ui.menu(
        "quality",
        "Modes per voice",
        [240.0, 452.0, 132.0, 30.0],
        "8 or 16 resonances per modal voice; fitted models: 64 or 128 sines",
    );
    ui.only_revisions(PROFILE_OR_FITTED, 1);
    ui.label(
        "An experimental resonator or a model fitted from recordings, not a calibrated instrument.",
        [400.0, 456.0, 456.0, 20.0],
    );
    ui.only_revisions(PROFILE_OR_FITTED, 1);
    // Struck and the bar and tine bodies (revision 3, and 1 read as 3), and their Advanced page.
    super::editor_bodies::body_page(&mut ui);

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
        [300.0, 386.0, 548.0, 120.0],
        None,
        UiGesture::Drag,
        "Analyze the loaded sample locally, check its root proposal, then accept one undoable change",
    );
    ui.finish()
}
