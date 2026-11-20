// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An audio clip's warp markers edited by commands (`docs/spec/ui-audio-editor.md` › Warp
//! markers): `clip.addWarpMarker`, `clip.moveWarpMarker`, `clip.removeWarpMarker` and
//! `clip.resetWarp`. The offline render is the oracle for what the markers do to the sound.
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use std::path::Path;

use serde_json::{Value, json};
use suhina_engine::RenderInputs;
use suhina_model::*;
use suhina_session::{Session, blank_project};
use suhina_test_support::{WavFormat, write_wav};

use crate::support::run;
use crate::temp_dir::TempDir;

const SR: u32 = 48_000;

fn wav(dir: &Path, name: &str, samples: &[f32]) -> String {
    let path = dir.join(name);
    write_wav(&path, SR, 1, WavFormat::Float32, samples);
    path.display().to_string()
}

fn clips(session: &Session) -> Vec<Clip> {
    let lane = session.project().arrangement.lanes.iter().find(|l| l.track == Some(TrackId(3))).unwrap();
    let mut clips = lane.clips.clone();
    clips.sort_by(|a, b| a.start.total_cmp(&b.start));
    clips
}

/// The rendered left channel's level at `beat` (at 120 BPM, 24 000 frames a beat).
fn level_at(session: &Session, beat: f64) -> f32 {
    let out = suhina_engine::render_offline_from(
        session.project(),
        RenderInputs::from_audio(session.audio(), suhina_session::devices()),
        SR,
        512,
        None,
    )
    .unwrap();
    out.get((beat * 24_000.0) as usize * 2).copied().unwrap_or(0.0).abs()
}

/// A session with `file` on the audio track, warped on two markers at 120 BPM, and the clip's id.
fn warped(file: &str) -> (Session, u32) {
    let mut session = Session::new(blank_project());
    run(&mut session, "clip.addFiles", json!({ "paths": [file], "track": 3, "start": 0.0 })).unwrap();
    let id = clips(&session)[0].id.0;
    run(&mut session, "clip.setTempo", json!({ "clip": id, "bpm": 120.0 })).unwrap();
    (session, id)
}

fn tone(dir: &Path) -> (Session, u32) {
    warped(&wav(dir, "hit.wav", &vec![0.5; 2 * SR as usize]))
}

fn markers_of(session: &Session) -> Vec<(f64, f64)> {
    clips(session)[0].audio.warp_markers.iter().map(|m| (m.time, m.audio_time)).collect()
}

#[test]
fn a_marker_is_added_where_the_sound_is_then_moved_and_removed_one_undo_step_each() {
    let dir = TempDir::new("warp-markers");
    let (mut session, id) = tone(&dir);
    assert_eq!(markers_of(&session), [(0.0, 0.0), (4.0, 2.0)]);
    // Added at beat 2: the sound there is second 1, so nothing moves yet.
    run(&mut session, "clip.addWarpMarker", json!({ "clip": id, "beat": 2.0 })).unwrap();
    assert_eq!(markers_of(&session), [(0.0, 0.0), (2.0, 1.0), (4.0, 2.0)]);
    // Moved to beat 2.5: the second of sound goes with it, later in the beat grid.
    let moved = run(&mut session, "clip.moveWarpMarker", json!({ "clip": id, "from": 2.0, "beat": 2.5 })).unwrap();
    assert_eq!(markers_of(&session), [(0.0, 0.0), (2.5, 1.0), (4.0, 2.0)]);
    assert_eq!(moved["markers"], json!([[0.0, 0.0], [2.5, 1.0], [4.0, 2.0]]));
    run(&mut session, "clip.removeWarpMarker", json!({ "clip": id, "beat": 2.5 })).unwrap();
    assert_eq!(markers_of(&session), [(0.0, 0.0), (4.0, 2.0)]);
    // Each is one undo step.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(markers_of(&session), [(0.0, 0.0), (2.5, 1.0), (4.0, 2.0)]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(markers_of(&session), [(0.0, 0.0), (2.0, 1.0), (4.0, 2.0)]);
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(markers_of(&session), [(0.0, 0.0), (2.5, 1.0), (4.0, 2.0)]);
}

/// Two seconds with a decaying click every half second (at 0.5, 1 and 1.5 s), a little hiss under
/// them.
fn clicks() -> Vec<f32> {
    let period = SR as usize / 2;
    let mut noise = 12_345u32;
    (0..2 * SR as usize)
        .map(|i| {
            noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let hiss = (noise >> 9) as f32 / (1u32 << 23) as f32 - 0.5;
            let t = (i % period) as f32 / SR as f32;
            (t * 2_000.0).sin() * (-t * 40.0).exp() * 0.8 + hiss * 0.001
        })
        .collect()
}

/// Markers placed at the transients pin each click where it plays, so nothing moves; placed
/// again they add nothing, and the whole placing is one undo step (#1185).
#[test]
fn warp_markers_are_placed_on_the_transients_where_the_sound_is() {
    let dir = TempDir::new("warp-transients");
    let (mut session, id) = warped(&wav(&dir, "clicks.wav", &clicks()));
    assert_eq!(markers_of(&session), [(0.0, 0.0), (4.0, 2.0)]);
    let placed = run(&mut session, "clip.addWarpMarkersAtTransients", json!({ "clip": id })).unwrap();
    assert_eq!(placed["added"], json!(3), "{placed}");
    let markers = markers_of(&session);
    assert_eq!(markers.len(), 5, "{markers:?}");
    for (k, (beat, seconds)) in markers.iter().enumerate() {
        let want = k as f64 * 0.5;
        assert!((seconds - want).abs() < 0.002, "marker {k} at {seconds} s, want {want}");
        assert!((beat - seconds * 2.0).abs() < 1e-9, "marker {k}: the sound stays at its beat");
    }
    let again = run(&mut session, "clip.addWarpMarkersAtTransients", json!({ "clip": id })).unwrap();
    assert_eq!(again["added"], json!(0));
    assert_eq!(markers_of(&session).len(), 5);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(markers_of(&session), [(0.0, 0.0), (4.0, 2.0)]);
    // Warp off: no markers to place.
    run(&mut session, "clip.setAudio", json!({ "clip": id, "warp": false })).unwrap();
    assert!(run(&mut session, "clip.addWarpMarkersAtTransients", json!({ "clip": id })).is_err());
}

#[test]
fn a_marker_on_a_beat_that_has_one_changes_nothing() {
    let dir = TempDir::new("warp-markers-twice");
    let (mut session, id) = tone(&dir);
    run(&mut session, "clip.addWarpMarker", json!({ "clip": id, "beat": 2.0 })).unwrap();
    run(&mut session, "clip.addWarpMarker", json!({ "clip": id, "beat": 2.0 })).unwrap();
    assert_eq!(markers_of(&session).len(), 3);
}

#[test]
fn a_moved_marker_moves_the_sound_pinned_to_it() {
    let dir = TempDir::new("warp-markers-heard");
    // A click at second 1 of otherwise silent audio.
    let mut samples = vec![0.0_f32; 2 * SR as usize];
    for s in samples.iter_mut().skip(SR as usize).take(480) {
        *s = 0.8;
    }
    let (mut session, id) = warped(&wav(&dir, "click.wav", &samples));
    run(&mut session, "clip.addWarpMarker", json!({ "clip": id, "beat": 2.0 })).unwrap();
    // The click sits on beat 2 (second 1 at 120 BPM).
    assert!(level_at(&session, 2.005) > 0.1, "the click is on beat 2");
    run(&mut session, "clip.moveWarpMarker", json!({ "clip": id, "from": 2.0, "beat": 2.5 })).unwrap();
    // Now it is on beat 2.5, pulled there by the marker.
    assert!(level_at(&session, 2.005) < 0.01, "the click left beat 2");
    assert!(level_at(&session, 2.505) > 0.1, "and is on beat 2.5");
}

#[test]
fn warp_marker_commands_refuse_what_they_cannot_do() {
    let dir = TempDir::new("warp-markers-errors");
    let (mut session, id) = tone(&dir);
    run(&mut session, "clip.addWarpMarker", json!({ "clip": id, "beat": 2.0 })).unwrap();
    let fails = |session: &mut Session, command: &str, params: Value| run(session, command, params).unwrap_err();
    // A marker stays between its neighbours.
    fails(&mut session, "clip.moveWarpMarker", json!({ "clip": id, "from": 2.0, "beat": 4.0 }));
    fails(&mut session, "clip.moveWarpMarker", json!({ "clip": id, "from": 2.0, "beat": 5.0 }));
    fails(&mut session, "clip.moveWarpMarker", json!({ "clip": id, "from": 2.0, "beat": 0.0 }));
    // No marker there to move or remove.
    fails(&mut session, "clip.moveWarpMarker", json!({ "clip": id, "from": 3.0, "beat": 3.5 }));
    fails(&mut session, "clip.removeWarpMarker", json!({ "clip": id, "beat": 3.0 }));
    // Numbers that are not beats.
    fails(&mut session, "clip.addWarpMarker", json!({ "clip": id, "beat": "2" }));
    fails(&mut session, "clip.addWarpMarker", json!({ "clip": id, "beat": 1e300 }));
    fails(&mut session, "clip.addWarpMarker", json!({ "clip": id }));
    fails(&mut session, "clip.moveWarpMarker", json!({ "clip": id, "from": 2.0 }));
    // Before the start of the audio there is no sound to pin.
    fails(&mut session, "clip.addWarpMarker", json!({ "clip": id, "beat": -1.0 }));
    // A clip that is not there.
    fails(&mut session, "clip.addWarpMarker", json!({ "clip": 9999, "beat": 1.0 }));
    assert_eq!(markers_of(&session).len(), 3, "none of them changed anything");
    // Two markers stay.
    run(&mut session, "clip.removeWarpMarker", json!({ "clip": id, "beat": 2.0 })).unwrap();
    fails(&mut session, "clip.removeWarpMarker", json!({ "clip": id, "beat": 4.0 }));
    assert_eq!(markers_of(&session).len(), 2);
    // With warp off the markers are not edited.
    run(&mut session, "clip.setAudio", json!({ "clip": id, "warp": false })).unwrap();
    fails(&mut session, "clip.addWarpMarker", json!({ "clip": id, "beat": 1.0 }));
}

#[test]
fn resetting_the_warp_runs_the_audio_evenly_again_at_the_clips_tempo() {
    let dir = TempDir::new("warp-reset");
    let (mut session, id) = tone(&dir);
    run(&mut session, "clip.addWarpMarker", json!({ "clip": id, "beat": 2.0 })).unwrap();
    run(&mut session, "clip.moveWarpMarker", json!({ "clip": id, "from": 2.0, "beat": 2.5 })).unwrap();
    let before = clips(&session)[0].clone();
    let reset = run(&mut session, "clip.resetWarp", json!({ "clip": id })).unwrap();
    // The first and last markers are the clip's tempo: 4 beats in 2 seconds is 120.
    assert_eq!(markers_of(&session), [(0.0, 0.0), (4.0, 2.0)]);
    assert_eq!(reset["bpm"], json!(120.0));
    let after = clips(&session)[0].clone();
    assert_eq!((after.start, after.length, after.offset), (before.start, before.length, before.offset));
    // One undo step brings the markers back.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(markers_of(&session), [(0.0, 0.0), (2.5, 1.0), (4.0, 2.0)]);
}

#[test]
fn resetting_with_detection_needs_a_tempo_to_find_and_takes_a_named_one() {
    let dir = TempDir::new("warp-reset-detect");
    let (mut session, id) = tone(&dir);
    // A steady tone has no tempo in its name, its metadata or its sound.
    assert!(run(&mut session, "clip.resetWarp", json!({ "clip": id, "detect": true })).is_err());
    assert_eq!(markers_of(&session), [(0.0, 0.0), (4.0, 2.0)], "a failed reset changes nothing");
    // Named with its tempo, the file says 96 BPM: two seconds are 3.2 beats.
    let (mut session, id) = warped(&wav(&dir, "loop_96bpm.wav", &vec![0.5; 2 * SR as usize]));
    let reset = run(&mut session, "clip.resetWarp", json!({ "clip": id, "detect": true })).unwrap();
    assert_eq!(reset["bpm"], json!(96.0));
    assert_eq!(reset["source"], json!("name"));
    let markers = markers_of(&session);
    assert!((markers[1].0 - 3.2).abs() < 1e-9 && markers[1].1 == 2.0, "{markers:?}");
    assert!(run(&mut session, "clip.resetWarp", json!({ "clip": id, "detect": "yes" })).is_err());
}

/// The warp modes' settings are the clip's own: set with `clip.setAudio`, one undo step, kept
/// when the mode changes, and refused out of range.
#[test]
fn a_clip_keeps_its_own_warp_mode_settings() {
    let dir = TempDir::new("warp-settings");
    let (mut session, id) = tone(&dir);
    let set = json!({ "clip": id, "mode": "texture", "grain": 120.0, "flux": 70.0 });
    let answer = run(&mut session, "clip.setAudio", set).unwrap();
    assert_eq!(answer["settings"]["grain"], json!(120.0));
    let warp = clips(&session)[0].audio.warp;
    assert_eq!((warp.grain, warp.flux), (120.0, 70.0));
    run(&mut session, "clip.setAudio", json!({ "clip": id, "mode": "rhythmic", "slice": "1/16", "tail": "off" }))
        .unwrap();
    let audio = clips(&session)[0].audio.clone();
    assert_eq!(audio.stretch, StretchMode::Rhythmic);
    assert_eq!((audio.warp.slice, audio.warp.tail), (SliceAt::Sixteenth, SliceTail::Off));
    assert_eq!(audio.warp.grain, 120.0, "the grain is kept for when texture is picked again");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(clips(&session)[0].audio.warp.slice, SliceAt::Transients);
    for bad in [
        json!({ "clip": id, "slice": "1/7" }),
        json!({ "clip": id, "tail": 3 }),
        json!({ "clip": id, "envelope": 101.0 }),
        json!({ "clip": id, "grain": 5.0 }),
        json!({ "clip": id, "flux": "lots" }),
        json!({ "clip": id }),
    ] {
        assert!(run(&mut session, "clip.setAudio", bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(clips(&session)[0].audio.warp.grain, 120.0, "nothing refused changed anything");
}

/// The last marker dragged later stretches the audio past the clip's end: the clip keeps its
/// length, and its right edge can then be drawn out over all of the audio.
#[test]
fn the_end_of_a_stretched_audio_is_where_its_clip_can_be_drawn_out_to() {
    let dir = TempDir::new("warp-end");
    let (mut session, id) = tone(&dir);
    let content_length = |session: &Session| {
        let clip = clips(session)[0].clone();
        session.project().content(clip.content).unwrap().length
    };
    assert_eq!((clips(&session)[0].length, content_length(&session)), (4.0, 4.0));
    run(&mut session, "clip.moveWarpMarker", json!({ "clip": id, "from": 4.0, "beat": 12.0 })).unwrap();
    assert_eq!(clips(&session)[0].length, 4.0, "the clip keeps its length");
    assert_eq!(content_length(&session), 12.0, "its audio ends at beat 12 now");
    let trimmed = run(&mut session, "clip.trim", json!({ "clip": id, "end": 20.0 })).unwrap();
    assert_eq!(trimmed["end"], json!(12.0), "drawn out to the end of the audio, no further");
    // Squeezed back, the audio ends earlier again; undo puts the longer end back.
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "clip.moveWarpMarker", json!({ "clip": id, "from": 12.0, "beat": 6.0 })).unwrap();
    assert_eq!(content_length(&session), 6.0);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(content_length(&session), 12.0);
    // A reset to the clip's tempo (now three times slower) keeps that end.
    run(&mut session, "clip.resetWarp", json!({ "clip": id })).unwrap();
    assert_eq!(content_length(&session), 12.0);
}

/// Warp turned off reads the file at the project's tempo again, so the end follows the audio
/// back: a reset after it and a trim never draw the clip out past where the audio ends.
#[test]
fn warp_turned_off_brings_the_end_back_to_the_audio() {
    let dir = TempDir::new("warp-end-off");
    let (mut session, id) = tone(&dir);
    let content_length = |session: &Session| {
        let clip = clips(session)[0].clone();
        session.project().content(clip.content).unwrap().length
    };
    run(&mut session, "clip.moveWarpMarker", json!({ "clip": id, "from": 4.0, "beat": 12.0 })).unwrap();
    assert_eq!(content_length(&session), 12.0);
    run(&mut session, "clip.setAudio", json!({ "clip": id, "warp": false })).unwrap();
    assert_eq!(content_length(&session), 4.0, "two seconds at 120 BPM end at beat 4 unwarped");
    run(&mut session, "clip.resetWarp", json!({ "clip": id })).unwrap();
    assert_eq!(content_length(&session), 4.0);
    let trimmed = run(&mut session, "clip.trim", json!({ "clip": id, "end": 20.0 })).unwrap();
    assert_eq!(trimmed["end"], json!(4.0), "no further than the audio");
    // Undo goes back through the reset and the switch, each one step, to the stretched end.
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(content_length(&session), 12.0);
}

/// Another clip of the same audio keeps its own end: the edited clip gets a content of its own.
#[test]
fn a_shared_audio_keeps_its_end_for_the_other_clips() {
    let dir = TempDir::new("warp-end-shared");
    let (mut session, id) = tone(&dir);
    run(&mut session, "clip.duplicate", json!({ "clips": [{ "clip": id }] })).unwrap();
    let both = clips(&session);
    assert_eq!(both.len(), 2);
    assert_eq!(both[0].content, both[1].content, "the copy plays the same content");
    run(&mut session, "clip.moveWarpMarker", json!({ "clip": id, "from": 4.0, "beat": 8.0 })).unwrap();
    let both = clips(&session);
    let length = |c: &Clip| session.project().content(c.content).unwrap().length;
    assert_ne!(both[0].content, both[1].content);
    assert_eq!((length(&both[0]), length(&both[1])), (8.0, 4.0));
}
