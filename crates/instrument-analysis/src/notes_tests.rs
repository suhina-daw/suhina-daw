// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]
use super::*;

/// Frames of equal energy with these (MIDI note, periodicity) values, `None` unvoiced.
fn frames(values: &[Option<(f32, f32)>]) -> Vec<Frame> {
    values
        .iter()
        .enumerate()
        .map(|(i, v)| Frame {
            start: i * NOTE_HOP,
            midi: v.map(|(m, _)| m),
            periodicity: v.map_or(0.0, |(_, p)| p),
            energy: 1.0,
        })
        .collect()
}

fn held(midi: f32, periodicity: f32, count: usize) -> Vec<Option<(f32, f32)>> {
    (0..count).map(|i| Some((midi + if i % 2 == 0 { 0.1 } else { -0.1 }, periodicity))).collect()
}

#[test]
fn a_phrase_is_cut_into_its_held_notes() {
    // A3, a pause, C4, three quick passing notes, E4.
    let mut values = held(57.0, 0.97, 10);
    values.extend([None; 4]);
    values.extend(held(60.0, 0.97, 10));
    values.extend([Some((61.0, 0.9)), Some((62.0, 0.9)), Some((63.0, 0.9))]);
    values.extend(held(64.0, 0.97, 10));
    let notes = held_notes(&frames(&values));
    assert_eq!(notes.iter().map(|n| n.root).collect::<Vec<_>>(), vec![57, 60, 64], "{notes:?}");
    let total = notes.iter().map(|n| n.share).sum::<f32>();
    assert!((total - 30.0 / 33.0).abs() < 1e-4, "{total}");
    let first = notes.first().unwrap();
    assert_eq!(first.start_seconds, 0.0);
    assert!((first.end_seconds - (9 * NOTE_HOP + NOTE_FRAME) as f32 / RATE as f32).abs() < 1e-6);
    // A note played again after a pause stays a note of its own.
    let mut again = held(57.0, 0.97, 10);
    again.extend([None; 12]);
    again.extend(held(57.0, 0.97, 10));
    assert_eq!(held_notes(&frames(&again)).len(), 2);
}

#[test]
fn octave_errors_fold_and_an_ambiguous_octave_is_one_note() {
    // Two frames an octave up inside a held C3 are folded back.
    let mut values = held(48.0, 0.97, 20);
    for index in [5, 6] {
        values[index] = Some((60.0, 0.85));
    }
    let mut track = frames(&values);
    fold_octaves(&mut track);
    assert!(track.iter().all(|f| (f.midi.unwrap() - 48.0).abs() < 0.2));
    let notes = held_notes(&track);
    assert_eq!(notes.len(), 1);
    assert!((notes[0].share - 1.0).abs() < 1e-6);
    // A long stretch at each octave, the upper clearly less periodic: one C3.
    let mut values = held(48.0, 0.97, 10);
    values.extend(held(60.0, 0.87, 10));
    let mut track = frames(&values);
    fold_octaves(&mut track);
    let notes = held_notes(&track);
    assert_eq!(notes.iter().map(|n| n.root).collect::<Vec<_>>(), vec![48], "{notes:?}");
    assert!((notes[0].end_seconds - (19 * NOTE_HOP + NOTE_FRAME) as f32 / RATE as f32).abs() < 1e-6);
    // An octave leap played as clearly as the note before it is a second note.
    let mut values = held(48.0, 0.97, 10);
    values.extend(held(60.0, 0.96, 10));
    assert_eq!(held_notes(&frames(&values)).iter().map(|n| n.root).collect::<Vec<_>>(), vec![48, 60]);
}

#[test]
fn a_glide_into_a_note_is_found() {
    let mut values = vec![Some((45.0, 0.9)), Some((46.0, 0.9)), Some((47.0, 0.9))];
    values.extend(held(48.0, 0.97, 20));
    let track = frames(&values);
    let notes = held_notes(&track);
    assert_eq!(notes.iter().map(|n| n.root).collect::<Vec<_>>(), vec![48]);
    let found = glide(&track, &notes[0]).unwrap();
    assert_eq!(found.from_midi, 46.0);
    assert!((found.seconds - (3 * NOTE_HOP) as f32 / RATE as f32).abs() < 1e-6);
    // A note that starts on its pitch has no glide.
    let track = frames(&held(48.0, 0.97, 20));
    assert_eq!(glide(&track, &held_notes(&track)[0]), None);
}

#[test]
fn the_track_follows_a_tone_and_leaves_quiet_frames_unvoiced() {
    // 0.5 s of A3, then 0.5 s of it 40 dB down.
    let hz = 220.0;
    let mono: Vec<f32> = (0..RATE as usize)
        .map(|i| {
            let t = i as f64 / f64::from(RATE);
            let gain = if t < 0.5 { 0.5 } else { 0.005 };
            (gain * (std::f64::consts::TAU * hz * t).sin()) as f32
        })
        .collect();
    let mut calls = 0;
    let tracked = track(&mono, RATE, |advance| {
        calls += usize::from(advance);
        true
    })
    .unwrap();
    assert_eq!(tracked.len(), calls);
    assert_eq!(tracked.len(), (mono.len() - NOTE_FRAME) / NOTE_HOP + 1);
    let loud = tracked.iter().filter(|f| f.start + NOTE_FRAME <= RATE as usize / 2);
    assert!(loud.clone().count() > 10 && loud.clone().all(|f| (f.midi.unwrap() - 57.0).abs() < 0.1));
    assert!(tracked.iter().filter(|f| f.start >= RATE as usize / 2).all(|f| f.midi.is_none()));
    assert_eq!(track(&mono, RATE, |_| false), Err(Error::Cancelled));
}
