// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Built-in MIDI effects in the renderer: the pitch effect before an instrument changes the notes
//! it plays, from the arrangement and the launcher, with the clip's scale, in any block size,
//! while its parameters follow a lane.

use suhina_model::*;

use crate::chain_tests::{SR, device, max_diff, note, peak, project};
use crate::{LaunchCommand, render_launches, render_offline};

const SYNTH: &str = "fm-synth";

/// `p` with its first clip content in `scale`, or without one.
fn with_scale(mut p: Project, scale: Option<Scale>) -> Project {
    p.contents[0].scale = scale;
    p
}

/// `p` rendered offline in blocks of 512 frames.
fn render(p: &Project) -> Vec<f32> {
    render_offline(p, SR, 512).unwrap_or_else(|e| panic!("{e}"))
}

/// The instrument after the pitch effect sounds as if the transposed keys had been written in
/// the clip, in any block size.
#[test]
fn pitch_before_an_instrument_transposes_the_notes_it_plays() {
    let notes = vec![note(0.0, 1.0, 60), note(1.0, 1.0, 64), note(1.5, 0.5, 120)];
    let pitched = project(notes, 2.0, vec![device("pitch", &[("pitch", 12.0)]), device(SYNTH, &[])]);
    let moved =
        project(vec![note(0.0, 1.0, 72), note(1.0, 1.0, 76), note(1.5, 0.5, 127)], 2.0, vec![device(SYNTH, &[])]);
    let reference = render(&moved);
    let out = render(&pitched);
    assert!(peak(&out) > 0.05);
    assert!(max_diff(&out, &reference) < 1e-6, "{}", max_diff(&out, &reference));
    for size in [1, 64, 333, 4_096] {
        let other = render_offline(&pitched, SR, size).unwrap_or_else(|e| panic!("{e}"));
        assert!(max_diff(&other, &reference) < 1e-6, "block {size}: {}", max_diff(&other, &reference));
    }
}

/// On a track with a MIDI effect and no instrument, the changed notes play as the track's sine
/// tones, ahead of its audio effects.
#[test]
fn without_an_instrument_the_changed_notes_play_as_the_tracks_sine_tones() {
    let notes = vec![note(0.0, 1.0, 60), note(0.5, 1.0, 64), note(1.5, 0.5, 120)];
    let moved = vec![note(0.0, 1.0, 72), note(0.5, 1.0, 76), note(1.5, 0.5, 127)];
    let reference = render(&project(moved.clone(), 2.0, vec![]));
    assert!(peak(&reference) > 0.05);
    for size in [1, 64, 333, 4_096] {
        let pitched = project(notes.clone(), 2.0, vec![device("pitch", &[("pitch", 12.0)])]);
        let out = render_offline(&pitched, SR, size).unwrap_or_else(|e| panic!("{e}"));
        assert!(max_diff(&out, &reference) < 1e-5, "block {size}: {}", max_diff(&out, &reference));
    }
    // The tones come before the audio effects, as an instrument would.
    let gain = || device("utility", &[("gain", -6.0)]);
    let out = render(&project(notes, 2.0, vec![device("pitch", &[("pitch", 12.0)]), gain()]));
    let reference = render(&project(moved, 2.0, vec![gain()]));
    assert!(max_diff(&out, &reference) < 1e-5, "{}", max_diff(&out, &reference));
}

/// A device imported as a placeholder (a MIDI effect or an instrument Suhina has not built) is
/// left out of the chain: the notes reach the next device unchanged, and with no instrument left
/// they play as the track's sine tones (`docs/spec/device-import.md`).
#[test]
fn placeholders_around_a_midi_effect_pass_the_notes_on() {
    let placeholder = |id: &str| Device::new(id, DeviceKind::Foreign { format: "als".into(), id: id.into() });
    let notes = vec![note(0.0, 1.0, 60), note(1.0, 1.0, 64)];
    let pitch = || device("pitch", &[("pitch", 12.0)]);

    let with = project(notes.clone(), 2.0, vec![placeholder("MidiChord"), pitch(), device(SYNTH, &[])]);
    let without = project(notes.clone(), 2.0, vec![pitch(), device(SYNTH, &[])]);
    let reference = render(&without);
    assert!(peak(&reference) > 0.05);
    assert!(max_diff(&render(&with), &reference) < 1e-6);

    let unbuilt = project(notes.clone(), 2.0, vec![placeholder("MidiChord"), pitch(), placeholder("OriginalSimpler")]);
    let tones = render(&project(notes, 2.0, vec![pitch()]));
    assert!(peak(&tones) > 0.05);
    assert!(max_diff(&render(&unbuilt), &tones) < 1e-6);
}

/// A note outside the pitch effect's window in Block mode makes no sound.
#[test]
fn notes_the_window_blocks_are_never_played() {
    let notes = vec![note(0.0, 1.0, 40), note(1.0, 1.0, 60)];
    let window = [("lowest", 60.0), ("range", 0.0)];
    let out = render(&project(notes, 2.0, vec![device("pitch", &window), device(SYNTH, &[])]));
    let only = render(&project(vec![note(1.0, 1.0, 60)], 2.0, vec![device(SYNTH, &[])]));
    assert!(max_diff(&out, &only) < 1e-6, "{}", max_diff(&out, &only));
}

/// Scale degrees step along the scale of the clip that plays, and count semitones when the clip
/// has none.
#[test]
fn pitch_follows_the_scale_of_the_clip_playing() {
    let notes = vec![note(0.0, 1.0, 60), note(1.0, 1.0, 64), note(2.0, 1.0, 71)];
    let pitch = || device("pitch", &[("followScale", 1.0), ("pitchDegrees", 1.0)]);
    let major = Scale::new(0, ScaleMode::Major);
    let out = render(&with_scale(project(notes.clone(), 3.0, vec![pitch(), device(SYNTH, &[])]), major));
    let steps = render(&project(
        vec![note(0.0, 1.0, 62), note(1.0, 1.0, 65), note(2.0, 1.0, 72)],
        3.0,
        vec![device(SYNTH, &[])],
    ));
    assert!(max_diff(&out, &steps) < 1e-6, "{}", max_diff(&out, &steps));
    // Without a scale on the clip, degrees count semitones.
    let out = render(&project(notes, 3.0, vec![pitch(), device(SYNTH, &[])]));
    let semitones = render(&project(
        vec![note(0.0, 1.0, 61), note(1.0, 1.0, 65), note(2.0, 1.0, 72)],
        3.0,
        vec![device(SYNTH, &[])],
    ));
    assert!(max_diff(&out, &semitones) < 1e-6, "{}", max_diff(&out, &semitones));
}

/// A clip launched from a slot reaches the pitch effect with its scale, as an arrangement clip
/// does.
#[test]
fn launched_clips_go_through_pitch_with_their_scale() {
    let commands = [
        (0.0, LaunchCommand::SetQuantization(crate::Quantization::None)),
        (0.0, LaunchCommand::LaunchClip { column: 0, slot: 0 }),
        (2.0, LaunchCommand::StopColumn { column: 0 }),
    ];
    let launch = |p: &Project| {
        render_launches(p, &crate::AudioPool::new(), SR, &commands, 3.0).unwrap_or_else(|e| panic!("{e}"))
    };
    let pitch = device("pitch", &[("followScale", 1.0), ("pitchDegrees", -1.0)]);
    let minor = Scale::new(9, ScaleMode::Minor);
    let out = launch(&with_scale(project(vec![note(0.0, 0.5, 69)], 1.0, vec![pitch, device(SYNTH, &[])]), minor));
    let reference = launch(&project(vec![note(0.0, 0.5, 67)], 1.0, vec![device(SYNTH, &[])]));
    assert!(peak(&out) > 0.05);
    assert!(max_diff(&out, &reference) < 1e-6, "{}", max_diff(&out, &reference));
}

/// A note held while an automation lane moves the transposition still ends: its note-off reaches
/// the key its note-on sent.
#[test]
fn a_held_note_ends_at_its_key_when_pitch_moves_under_it() {
    let lane = AutomationLane {
        param: "pitch".into(),
        points: vec![AutomationPoint::new(0.0, 0.0), AutomationPoint::new(2.0, 0.0), AutomationPoint::new(2.0, 12.0)],
        enabled: true,
    };
    let mut pitch = device("pitch", &[]);
    pitch.automation = vec![lane];
    let held = vec![note(0.0, 4.0, 60), note(3.0, 1.0, 69)];
    let out = render(&project(held, 4.0, vec![pitch, device(SYNTH, &[])]));
    let reference = render(&project(vec![note(0.0, 4.0, 60), note(3.0, 1.0, 81)], 4.0, vec![device(SYNTH, &[])]));
    assert!(max_diff(&out, &reference) < 1e-6, "{}", max_diff(&out, &reference));
    // Nothing hangs: the tail after the notes dies away as the plain render's does.
    let tail = out.len() - SR as usize / 2;
    assert!(peak(&out[tail..]) < 1e-4, "{}", peak(&out[tail..]));
}
