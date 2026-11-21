// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Expanded pages expose the existing engine honestly; page selection stays in the host.

use super::Canvas;
use suhina_device_sdk::ui::{DeviceUi, Draw, TextAlign, UiGesture, Widget};

/// The editor's look on top of the shared sheet (docs/spec/instrument-editor.md › Appearance and
/// validation), with every theme: buttons as text with only the chosen one underlined, in brass.
const EDITOR_STYLE: &str = "button { border-color: transparent; border-width: 0 0 2px 0; }
button[checked] { border-color: #b8742f; }";
/// The revisions whose Model page shows #1599's struck-body group: a profile or a fitted set.
const PROFILE_OR_FITTED: &[u16] = &[2, 4];
/// Fainter espresso text.
const MUTED: [u8; 4] = super::MUTED;
/// The editor's last page: 0 Play, 1 Model, 2 Sample, 3 Advanced, 4 Analysis.
pub(super) const LAST_PAGE: u16 = 3;

pub(super) fn canvas() -> DeviceUi {
    let mut ui = Canvas::new([880.0, 560.0]);
    ui.artwork(super::EDITOR_LIGHT);
    ui.style(EDITOR_STYLE);
    ui.asset("logo", super::LOGO);
    ui.asset("knob-large", super::KNOB_LARGE);
    ui.asset("knob-small", super::KNOB_SMALL);
    // The logo in place of a title.
    ui.add(
        "Suhina",
        Widget::Graphic { drawing: vec![Draw::image("logo", [0.0, 0.0, 108.0, 30.0])] },
        [386.0, 18.0, 108.0, 30.0],
        None,
        UiGesture::Drag,
        "",
    );
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
        ui.navigate(name, [56.0 + i as f32 * 84.0, 518.0, 76.0, 22.0], i as u16);
    }

    // Beside the page buttons on every page, as the Source menu is.
    ui.toggle(
        "soft_pedal",
        "Soft pedal",
        [316.0, 518.0, 96.0, 22.0],
        "Una corda for fitted models: quieter, darker strikes without beating, as with a soft pedal (CC 67) held          down. Keys: C3 = 60; the pitch wheel bends ±2 semitones; the sustain pedal holds keys",
    );

    // The amplitude envelope, the playable keys and velocities, one row of dials.
    super::editor_pages::play_page(&mut ui);

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
        "Strike color",
        [646.0, 170.0, 150.0, 140.0],
        "Creative excitation weighting of accepted modes, not a measured impact position; next strike only",
    );
    ui.only_revision(2);
    ui.dial(
        "strike",
        "Strike color",
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
        (
            "model_tune",
            "Model tune",
            "Moves the model's pitch in semitones, for a model fitted at the wrong root or a few cents off",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        ui.dial(id, name, [54.0 + i as f32 * 160.0, 350.0, 110.0, 92.0], hint);
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

    // The waveform, its region and loop, the sample analysis, one row of dials and Playback.
    super::editor_pages::sample_page(&mut ui);
    ui.finish()
}
