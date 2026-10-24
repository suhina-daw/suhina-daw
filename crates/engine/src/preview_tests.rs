// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The preview voice (ADR 0011): samples and notes previewed through the renderer, with the
//! transport stopped or running, and audio added to the graphs after the renderer was built.

use crate::{RenderInputs, RenderOptions};
use std::sync::Arc;

use suhina_model::*;

use crate::chain_tests::{BEAT, SR, blocks, device, empty_project, note, peak, project};
use crate::{AudioData, AudioPool, LaunchCommand, Renderer, preview_notes};

/// One second of a steady 0.5 on both channels at 24 kHz, so the preview steps through it at
/// half a file frame per engine frame.
fn steady() -> Arc<AudioData> {
    Arc::new(AudioData { sample_rate: 24_000, channels: 2, samples: vec![0.5; 24_000 * 2] })
}

fn stopped(project: &Project) -> Renderer {
    Renderer::stopped(project, &AudioPool::new(), SR).unwrap()
}

#[test]
fn a_sample_previews_while_the_transport_is_stopped_and_ends_with_the_file() {
    let mut renderer = stopped(&empty_project());
    let mut controller = renderer.controller();
    let data = steady();
    controller.preview_sample(Arc::clone(&data), 0.0, 1.0).unwrap();
    // A second of file at 48 kHz is 48 000 frames: 47 blocks of 1024 play it, the 48th ends it.
    let out = blocks(&mut renderer, 46, 1024);
    assert!(controller.preview_monitor().playing());
    // After the click guard the level is the file's.
    assert!((out[2000] - 0.5).abs() < 1e-6, "{}", out[2000]);
    let seconds = controller.preview_monitor().seconds();
    assert!((seconds - 46.0 * 1024.0 / 48_000.0).abs() < 1e-3, "{seconds}");
    let tail = blocks(&mut renderer, 4, 1024);
    assert!(!controller.preview_monitor().playing());
    assert!(peak(&tail[2048..]) == 0.0, "silent after the file ends");
    // The voice gave its data back; collecting frees it on this thread.
    controller.collect();
    assert_eq!(Arc::strong_count(&data), 1);
}

#[test]
fn a_preview_starts_where_asked_and_stops_on_command() {
    let mut renderer = stopped(&empty_project());
    let mut controller = renderer.controller();
    let ramp: Vec<f32> = (0..24_000).flat_map(|i| [i as f32 / 24_000.0; 2]).collect();
    let data = Arc::new(AudioData { sample_rate: 24_000, channels: 2, samples: ramp });
    controller.preview_sample(data, 0.5, 1.0).unwrap();
    let out = blocks(&mut renderer, 1, 1024);
    // From the middle of the ramp (past the click guard).
    assert!((out[400] - 0.5).abs() < 0.01, "{}", out[400]);
    controller.preview_stop().unwrap();
    let out = blocks(&mut renderer, 2, 1024);
    assert_eq!(peak(&out), 0.0);
    assert!(!controller.preview_monitor().playing());
}

#[test]
fn a_new_preview_replaces_the_one_playing() {
    let mut renderer = stopped(&empty_project());
    let mut controller = renderer.controller();
    let first = steady();
    controller.preview_sample(Arc::clone(&first), 0.0, 1.0).unwrap();
    blocks(&mut renderer, 2, 512);
    let quiet = Arc::new(AudioData { sample_rate: SR, channels: 1, samples: vec![0.25; SR as usize] });
    controller.preview_sample(quiet, 0.0, 1.0).unwrap();
    let out = blocks(&mut renderer, 2, 512);
    assert!((out[1000] - 0.25).abs() < 1e-6, "mono plays on both sides: {}", out[1000]);
    assert!((out[1001] - 0.25).abs() < 1e-6);
    controller.collect();
    assert_eq!(Arc::strong_count(&first), 1, "the replaced sample went back");
}

#[test]
fn the_level_of_a_playing_preview_changes_smoothly_and_it_keeps_playing() {
    let mut renderer = stopped(&empty_project());
    let mut controller = renderer.controller();
    controller.preview_sample(steady(), 0.0, 1.0).unwrap();
    let before = blocks(&mut renderer, 2, 1024);
    assert!((before[3000] - 0.5).abs() < 1e-6);
    controller.preview_gain(0.5).unwrap();
    let after = blocks(&mut renderer, 4, 1024);
    assert!(controller.preview_monitor().playing(), "a new level does not restart or stop it");
    // No jump: the first frame is still near the old level, and it glides down.
    assert!((after[0] - 0.5).abs() < 0.01, "{}", after[0]);
    let steps: Vec<f32> = after.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
    assert!(steps.iter().all(|d| *d < 0.01), "no click");
    // About 80 ms later it is there.
    assert!((after[8000] - 0.25).abs() < 1e-3, "{}", after[8000]);
    // The next preview starts at the level it is given.
    controller.preview_sample(steady(), 0.0, 1.0).unwrap();
    let next = blocks(&mut renderer, 1, 1024);
    assert!((next[1500] - 0.5).abs() < 1e-6, "{}", next[1500]);
    // Hostile values are made safe.
    controller.preview_gain(f32::NAN).unwrap();
    let silent = blocks(&mut renderer, 4, 1024);
    assert!(silent[8000].abs() < 1e-3);
}

#[test]
fn the_preview_is_heard_with_the_transport_running_and_a_track_muted() {
    let mut muted = project(vec![note(0.0, 4.0, 60)], 4.0, Vec::new());
    muted.channels[0].mute = true;
    let mut renderer =
        Renderer::build(&muted, RenderInputs::from_audio(&AudioPool::new()), SR, RenderOptions::PLAYING).unwrap();
    let mut controller = renderer.controller();
    controller.preview_sample(steady(), 0.0, 0.5).unwrap();
    let out = blocks(&mut renderer, 2, 1024);
    assert!((out[1500] - 0.25).abs() < 1e-6, "at the preview's gain: {}", out[1500]);
}

#[test]
fn notes_preview_through_the_tracks_instrument() {
    let synth = project(Vec::new(), 1.0, vec![device("fm-synth", &[])]);
    let mut renderer = stopped(&synth);
    let mut controller = renderer.controller();
    let notes = preview_notes([(0.0, 0.5, 60, 1.0)], 120.0, SR, Some(TrackId(1)), None);
    controller.preview_notes(notes, 1.0).unwrap();
    let out = blocks(&mut renderer, (BEAT / 1024) + 1, 1024);
    assert!(peak(&out) > 0.01, "the track's FM synth plays the note");
    assert!(controller.preview_monitor().playing());
    // Half a beat later the note is released and the preview ends after its tail.
    let tail = blocks(&mut renderer, 60, 1024);
    assert!(!controller.preview_monitor().playing());
    assert!(peak(&tail[tail.len() - 2048..]) < 1e-3, "released");
}

#[test]
fn notes_preview_through_the_fallback_when_the_track_has_no_instrument() {
    let mut renderer = stopped(&empty_project());
    let mut controller = renderer.controller();
    let fallback = controller.preview_instrument();
    assert!(fallback.is_some());
    let notes = preview_notes([(0.0, 1.0, 64, 0.8), (0.5, 1.0, 67, 0.8)], 120.0, SR, Some(TrackId(1)), fallback);
    controller.preview_notes(notes, 1.0).unwrap();
    let out = blocks(&mut renderer, 20, 1024);
    assert!(peak(&out) > 0.01);
    // Stopping releases the fallback's notes at once.
    controller.preview_stop().unwrap();
    blocks(&mut renderer, 1, 1024);
    assert!(!controller.preview_monitor().playing());
    let after = blocks(&mut renderer, 30, 1024);
    assert!(peak(&after) < 1e-4, "{}", peak(&after));
}

#[test]
fn stopping_a_preview_releases_the_tracks_notes_it_holds() {
    let synth = project(Vec::new(), 1.0, vec![device("fm-synth", &[])]);
    let mut renderer = stopped(&synth);
    let mut controller = renderer.controller();
    let notes = preview_notes([(0.0, 64.0, 60, 1.0)], 120.0, SR, Some(TrackId(1)), None);
    controller.preview_notes(notes, 1.0).unwrap();
    let held = peak(&blocks(&mut renderer, 10, 1024));
    // Without a stop the note holds for 64 beats at the same level.
    let mut kept = stopped(&synth);
    let mut kept_controller = kept.controller();
    let notes = preview_notes([(0.0, 64.0, 60, 1.0)], 120.0, SR, Some(TrackId(1)), None);
    kept_controller.preview_notes(notes, 1.0).unwrap();
    let kept_late = peak(&blocks(&mut kept, 200, 1024)[190 * 2048..]);
    assert!(kept_late > held * 0.3, "{kept_late} {held}");

    controller.preview_stop().unwrap();
    let after = blocks(&mut renderer, 200, 1024);
    let late = peak(&after[190 * 2048..]);
    assert!(late < held * 0.05, "the held note was released: {late} of {held}");
}

#[test]
fn audio_set_on_the_controller_is_heard_from_the_next_graph() {
    let region = AudioRegion {
        sample_rate: Some(SR),
        channels: Some(1),
        duration_seconds: Some(1.0),
        ..AudioRegion::new("/samples/added.wav")
    };
    let mut audio_project = project(Vec::new(), 2.0, Vec::new());
    audio_project.tracks[0].content = ContentKinds { notes: false, audio: true };
    audio_project.contents[0].lanes = vec![ContentLane { target: None, data: LaneData::Audio(region) }];
    let mut renderer =
        Renderer::build(&audio_project, RenderInputs::from_audio(&AudioPool::new()), SR, RenderOptions::PLAYING)
            .unwrap();
    let mut controller = renderer.controller();
    assert_eq!(peak(&blocks(&mut renderer, 4, 1024)), 0.0, "the file is not in the pool yet");

    let mut pool = AudioPool::new();
    pool.files.insert(
        "/samples/added.wav".into(),
        Arc::new(AudioData { sample_rate: SR, channels: 1, samples: vec![0.5; SR as usize] }),
    );
    controller.set_audio(Arc::new(pool));
    controller.send(LaunchCommand::SetPosition { beat: 0.0 }).unwrap();
    controller.update(&audio_project).unwrap();
    let out = blocks(&mut renderer, 8, 1024);
    assert!(peak(&out) > 0.1, "{}", peak(&out));
}

#[test]
fn a_held_note_plays_until_it_is_let_go() {
    let synth = project(Vec::new(), 1.0, vec![device("fm-synth", &[])]);
    let mut renderer = stopped(&synth);
    let mut controller = renderer.controller();
    // The key column's note: no end of its own.
    let notes = preview_notes([(0.0, 0.0, 60, 1.0)], 120.0, SR, Some(TrackId(1)), None);
    controller.preview_note_on(notes, 1.0).unwrap();
    let held = peak(&blocks(&mut renderer, 40, 1024));
    assert!(held > 0.01, "the track's FM synth plays the held key");
    assert!(controller.preview_monitor().playing());
    // It is still sounding much later: nothing in the note releases it.
    let late = peak(&blocks(&mut renderer, 200, 1024));
    assert!(late > held * 0.2, "{late} of {held}");
    controller.preview_note_off(Some(60)).unwrap();
    let after = blocks(&mut renderer, 200, 1024);
    assert!(peak(&after[190 * 2048..]) < held * 0.05, "letting go released the note");
    assert!(!controller.preview_monitor().playing());
}

#[test]
fn a_held_note_through_the_fallback_ends_and_a_key_off_without_one_changes_nothing() {
    let mut renderer = stopped(&empty_project());
    let mut controller = renderer.controller();
    let fallback = controller.preview_instrument();
    let notes = preview_notes([(0.0, 0.0, 64, 0.8)], 120.0, SR, None, fallback);
    controller.preview_note_on(notes, 1.0).unwrap();
    assert!(peak(&blocks(&mut renderer, 20, 1024)) > 0.01, "the fallback plays it");
    // Every held note off, without naming a key; the tail of the release is silent.
    controller.preview_note_off(None).unwrap();
    let after = blocks(&mut renderer, 30, 1024);
    let tail = &after[after.len() - 4096..];
    assert!(peak(tail) < 1e-4, "{}", peak(tail));
    // Letting go of a key that is not held does nothing, and neither does letting go twice.
    controller.preview_note_off(Some(64)).unwrap();
    controller.preview_note_off(None).unwrap();
    assert!(blocks(&mut renderer, 4, 1024).iter().all(|s| s.is_finite()));
    assert!(!controller.preview_monitor().playing());
}

#[test]
fn hostile_preview_values_are_made_safe() {
    let mut renderer = stopped(&empty_project());
    let mut controller = renderer.controller();
    controller.preview_sample(steady(), f64::NAN, f32::INFINITY).unwrap();
    let out = blocks(&mut renderer, 2, 1024);
    assert!(out.iter().all(|s| s.is_finite()));
    controller.preview_sample(steady(), 1e12, 1.0).unwrap();
    assert_eq!(peak(&blocks(&mut renderer, 2, 1024)), 0.0, "past the end plays nothing");
    let empty = Arc::new(AudioData { sample_rate: 0, channels: 0, samples: Vec::new() });
    controller.preview_sample(empty, 0.0, 1.0).unwrap();
    assert_eq!(peak(&blocks(&mut renderer, 2, 1024)), 0.0);
    let notes = preview_notes([(f64::NAN, -1.0, 200, f32::NAN)], f64::NAN, SR, None, None);
    assert_eq!(notes.notes[0].key, 127);
    controller.preview_notes(notes, 1.0).unwrap();
    assert!(blocks(&mut renderer, 4, 1024).iter().all(|s| s.is_finite()));
}
