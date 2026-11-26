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

/// A marker found by its beat at a floating-point edge: two markers within `SAME_MARKER` of the
/// beat asked for, the first a hair outside it; the second is the one moved.
#[test]
fn a_marker_just_inside_the_tolerance_is_found_past_one_just_outside() {
    // (beat asked for, a marker just outside the tolerance before it, one inside it after it).
    // In the second, the first marker is outside the tolerance by distance but not below
    // `beat - SAME_MARKER` as floats round it: a search that stopped at the first marker not
    // below that bound found it, failed the distance check and missed the second.
    for (beat, outside, inside) in [(1.0001, 1.0000985, 1.0001005), (0.0003, 0.000_298_999_999_999_999_95, 0.0003005)] {
        let dir = TempDir::new("warp-markers-edge");
        let (mut session, id) = tone(&dir);
        run(&mut session, "clip.addWarpMarker", json!({ "clip": id, "beat": outside })).unwrap();
        run(&mut session, "clip.addWarpMarker", json!({ "clip": id, "beat": inside })).unwrap();
        let before = markers_of(&session);
        run(&mut session, "clip.moveWarpMarkers", json!({ "clip": id, "beats": [beat], "by": 0.5 }))
            .unwrap_or_else(|e| panic!("beat {beat}: {e:?}"));
        let after = markers_of(&session);
        assert_eq!(after.len(), before.len(), "{after:?}");
        assert!(after.iter().any(|m| (m.0 - (inside + 0.5)).abs() < 1e-9), "the marker within reach moved: {after:?}");
        assert!(after.iter().any(|m| m.0 == outside), "the one outside stayed: {after:?}");
    }
}

#[test]
fn several_markers_move_and_are_removed_together_in_one_undo_step_each() {
    let dir = TempDir::new("warp-markers-many");
    let (mut session, id) = tone(&dir);
    for beat in [1.0, 2.0, 3.0] {
        run(&mut session, "clip.addWarpMarker", json!({ "clip": id, "beat": beat })).unwrap();
    }
    // 1 and 2 half a beat later, each with its sound; 3 stays.
    run(&mut session, "clip.moveWarpMarkers", json!({ "clip": id, "beats": [2.0, 1.0], "by": 0.5 })).unwrap();
    assert_eq!(markers_of(&session), [(0.0, 0.0), (1.5, 0.5), (2.5, 1.0), (3.0, 1.5), (4.0, 2.0)]);
    run(&mut session, "clip.removeWarpMarkers", json!({ "clip": id, "beats": [1.5, 3.0, 1.5] })).unwrap();
    assert_eq!(markers_of(&session), [(0.0, 0.0), (2.5, 1.0), (4.0, 2.0)]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(markers_of(&session), [(0.0, 0.0), (1.5, 0.5), (2.5, 1.0), (3.0, 1.5), (4.0, 2.0)]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(markers_of(&session), [(0.0, 0.0), (1.0, 0.5), (2.0, 1.0), (3.0, 1.5), (4.0, 2.0)]);
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(markers_of(&session).get(1), Some(&(1.5, 0.5)));

    let fails = |session: &mut Session, command: &str, params: Value| run(session, command, params).unwrap_err();
    // A moved marker stays between the ones not moved: 2.5 cannot pass 3.
    fails(&mut session, "clip.moveWarpMarkers", json!({ "clip": id, "beats": [1.5, 2.5], "by": 0.5 }));
    // No marker there, nothing named, not a list of numbers, no `by`.
    fails(&mut session, "clip.moveWarpMarkers", json!({ "clip": id, "beats": [1.7], "by": 0.1 }));
    fails(&mut session, "clip.moveWarpMarkers", json!({ "clip": id, "beats": [], "by": 0.1 }));
    fails(&mut session, "clip.moveWarpMarkers", json!({ "clip": id, "beats": ["1.5"], "by": 0.1 }));
    fails(&mut session, "clip.moveWarpMarkers", json!({ "clip": id, "beats": [1.5] }));
    fails(&mut session, "clip.moveWarpMarkers", json!({ "clip": id, "beats": [1.5], "by": 1e300 }));
    fails(&mut session, "clip.removeWarpMarkers", json!({ "clip": id, "beats": 1.5 }));
    // Two markers stay.
    fails(&mut session, "clip.removeWarpMarkers", json!({ "clip": id, "beats": [0.0, 1.5, 2.5, 3.0] }));
    assert_eq!(markers_of(&session).len(), 5, "none of them changed anything");
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

/// At 110 BPM the clicks (every half second) play off the beat grid; quantising puts a marker on
/// each and moves it onto the nearest beat, the sound with it, in one undo step.
#[test]
fn quantising_moves_the_transients_onto_the_grid() {
    let dir = TempDir::new("warp-quantise");
    let (mut session, id) = warped(&wav(&dir, "clicks.wav", &clicks()));
    run(&mut session, "clip.setTempo", json!({ "clip": id, "bpm": 110.0 })).unwrap();
    let before = markers_of(&session);
    let near = |got: &[(f64, f64)], want: &[(f64, f64)]| {
        got.len() == want.len()
            && got.iter().zip(want).all(|(g, w)| (g.0 - w.0).abs() < 1e-6 && (g.1 - w.1).abs() < 0.002)
    };
    let done = run(&mut session, "clip.quantizeWarpMarkers", json!({ "clip": id, "grid": "1/4" })).unwrap();
    let all = [(0.0, 0.0), (1.0, 0.5), (2.0, 1.0), (3.0, 1.5), (4.0, 2.0)];
    assert!(near(&markers_of(&session), &all), "{:?}", markers_of(&session));
    assert_eq!(done["moved"], json!(4), "{done}");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(markers_of(&session), before);

    // Only the range from beat 1.5 to 3, and half way.
    run(
        &mut session,
        "clip.quantizeWarpMarkers",
        json!({ "clip": id, "grid": "1/4", "start": 1.5, "end": 3.0, "amount": 50 }),
    )
    .unwrap();
    // Each new marker is on the transient found (a little after the click's first sample), so its
    // beat is worked out from that second, not from the click's.
    let beat = |seconds: f64| seconds * 110.0 / 60.0;
    let half = |b: f64| b + (b.round() - b) * 0.5;
    let got = markers_of(&session);
    let found = |near: f64| got.iter().map(|m| m.1).find(|s| (s - near).abs() < 0.002).unwrap_or(near);
    let (one, one_half) = (found(1.0), found(1.5));
    let want = [(0.0, 0.0), (half(beat(one)), one), (half(beat(one_half)), one_half), (before[1].0, before[1].1)];
    assert!(near(&got, &want), "{got:?}");

    let fails = |session: &mut Session, params: Value| run(session, "clip.quantizeWarpMarkers", params).unwrap_err();
    fails(&mut session, json!({ "clip": id }));
    fails(&mut session, json!({ "clip": id, "grid": "none" }));
    fails(&mut session, json!({ "clip": id, "grid": "1/4", "amount": 150 }));
    fails(&mut session, json!({ "clip": id, "grid": "1/4", "start": 3.0, "end": 1.0 }));
    fails(&mut session, json!({ "clip": 9999, "grid": "1/4" }));
}

/// Markers at beats 0.51 and 0.55 quantized half way onto beat 1 would land at 0.755 and 0.775,
/// past the marker at 0.60 left out of the range: both go back to their own beats, not only the
/// one next to it, and the command keeps the moves that still fit instead of refusing them all.
#[test]
fn every_marker_quantized_past_one_that_stays_goes_back() {
    let dir = TempDir::new("warp-quantize-crowded");
    let (mut session, id) = tone(&dir);
    for beat in [0.51, 0.55, 0.60] {
        run(&mut session, "clip.addWarpMarker", json!({ "clip": id, "beat": beat })).unwrap();
    }
    let before = markers_of(&session);
    let quantize =
        |start: f64| json!({ "clip": id, "grid": "1/4", "amount": 50, "start": start, "end": 0.55, "transients": [] });
    let done = run(&mut session, "clip.quantizeWarpMarkers", quantize(0.50)).unwrap();
    assert_eq!(done["moved"], json!(0), "{done}");
    assert_eq!(markers_of(&session), before);

    // A marker at 0.30 in the range moves half way to beat 0 (0.15), which still fits.
    run(&mut session, "clip.addWarpMarker", json!({ "clip": id, "beat": 0.30 })).unwrap();
    let before = markers_of(&session);
    let done = run(&mut session, "clip.quantizeWarpMarkers", quantize(0.25)).unwrap();
    assert_eq!(done["moved"], json!(1), "{done}");
    let got = markers_of(&session);
    assert_eq!(got.len(), before.len(), "{got:?}");
    for (g, b) in got.iter().zip(&before) {
        let want = if (b.0 - 0.30).abs() < 1e-9 { 0.15 } else { b.0 };
        assert!((g.0 - want).abs() < 1e-9 && g.1 == b.1, "{got:?}");
    }
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

/// A reset keeps where the clip's tempo came from only while it keeps that tempo: a clip whose
/// warp is off is reset at the project's tempo, which is no tempo of the file's but a manual one.
#[test]
fn a_reset_at_the_projects_tempo_is_a_manual_tempo() {
    let dir = TempDir::new("warp-reset-source");
    let (mut session, id) = warped(&wav(&dir, "loop_96bpm.wav", &vec![0.5; 2 * SR as usize]));
    let found = run(&mut session, "clip.resetWarp", json!({ "clip": id, "detect": true })).unwrap();
    assert_eq!(found["source"], json!("name"));
    // Warped, a reset keeps the clip's 96 BPM and that it came from the file's name.
    let kept = run(&mut session, "clip.resetWarp", json!({ "clip": id })).unwrap();
    assert!((kept["bpm"].as_f64().unwrap() - 96.0).abs() < 1e-9, "{kept}");
    assert_eq!(kept["source"], json!("name"));
    assert_eq!(clips(&session)[0].audio.tempo_source, Some(TempoSource::Name));
    // Warp off: the reset takes the project's 120 BPM, set by hand rather than read from the name.
    run(&mut session, "clip.setAudio", json!({ "clip": id, "warp": false })).unwrap();
    assert_eq!(clips(&session)[0].audio.tempo_source, Some(TempoSource::Name), "turning warp off keeps it");
    let reset = run(&mut session, "clip.resetWarp", json!({ "clip": id })).unwrap();
    assert_eq!(reset["bpm"], json!(120.0));
    assert_eq!(reset["source"], json!("manual"));
    assert_eq!(clips(&session)[0].audio.tempo_source, Some(TempoSource::Manual));
}

/// The warp modes' settings are the clip's own: set with `clip.setAudio`, one undo step, kept
/// when the mode changes, and refused out of range.
#[test]
fn a_clip_keeps_its_own_warp_mode_settings() {
    let dir = TempDir::new("warp-settings");
    let (mut session, id) = tone(&dir);
    let set = json!({ "clip": id, "mode": "texture", "grain": 120.0, "scatter": 70.0 });
    let answer = run(&mut session, "clip.setAudio", set).unwrap();
    assert_eq!(answer["settings"]["grain"], json!(120.0));
    let warp = clips(&session)[0].audio.warp;
    assert_eq!((warp.grain, warp.scatter), (120.0, 70.0));
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
        json!({ "clip": id, "scatter": "lots" }),
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

/// The content's end is read from the markers each time, not added to the length it had: held
/// at its shortest (a 64th) by one move, it comes back to the audio's end with the next.
#[test]
fn a_content_squeezed_to_its_shortest_comes_back_to_the_end_of_its_audio() {
    let dir = TempDir::new("warp-end-shortest");
    let (mut session, id) = tone(&dir);
    let content_length = |session: &Session| {
        let clip = clips(session)[0].clone();
        session.project().content(clip.content).unwrap().length
    };
    run(&mut session, "clip.moveWarpMarker", json!({ "clip": id, "from": 4.0, "beat": 1.0 / 128.0 })).unwrap();
    assert_eq!(content_length(&session), 1.0 / 64.0);
    run(&mut session, "clip.moveWarpMarker", json!({ "clip": id, "from": 1.0 / 128.0, "beat": 4.0 })).unwrap();
    assert!((content_length(&session) - 4.0).abs() < 1e-9, "{}", content_length(&session));
}

/// A linked copy whose warp moves the end of the audio gets a content of its own that is no
/// linked copy, and the content left behind stays linked only while two clips still play it.
#[test]
fn a_linked_copy_given_its_own_audio_end_leaves_the_link() {
    let dir = TempDir::new("warp-end-linked");
    let (mut session, id) = tone(&dir);
    let content = clips(&session)[0].content;
    let duplicate = json!({ "clips": [{ "clip": id }], "linked": true, "stack": true });
    run(&mut session, "clip.duplicate", duplicate.clone()).unwrap();
    run(&mut session, "clip.duplicate", duplicate).unwrap();
    let all = clips(&session);
    assert_eq!(all.len(), 3);
    assert!(all.iter().all(|c| c.content == content));
    assert!(session.project().content(content).unwrap().linked);
    let own = |session: &Session, clip: u32| clips(session).into_iter().find(|c| c.id.0 == clip).unwrap().content;
    run(&mut session, "clip.moveWarpMarker", json!({ "clip": id, "from": 4.0, "beat": 8.0 })).unwrap();
    let first = own(&session, id);
    assert_ne!(first, content);
    assert!(!session.project().content(first).unwrap().linked, "the clip's own content is no linked copy");
    assert!(session.project().content(content).unwrap().linked, "two clips still play the content left");
    // The second of the copies leaves too: one clip plays the content left, so it is no longer linked.
    let second = clips(&session).into_iter().find(|c| c.id.0 != id && c.content == content).unwrap().id.0;
    run(&mut session, "clip.moveWarpMarker", json!({ "clip": second, "from": 4.0, "beat": 6.0 })).unwrap();
    assert!(!session.project().content(own(&session, second)).unwrap().linked);
    assert!(!session.project().content(content).unwrap().linked, "one clip plays it now");
}

/// A clip holding more markers than a clip keeps (a damaged file) is refused by the marker
/// commands with a pointer to Reset warp, which replaces the list.
#[test]
fn a_clip_with_too_many_warp_markers_is_edited_again_after_a_reset() {
    let dir = TempDir::new("warp-too-many");
    let (session, id) = tone(&dir);
    let mut project = session.project().clone();
    let clip = project.arrangement.lanes.iter_mut().flat_map(|l| l.clips.iter_mut()).find(|c| c.id.0 == id).unwrap();
    clip.audio.warp_markers =
        (0..=100_000).map(|k| WarpMarker { time: k as f64 * 4e-5, audio_time: k as f64 * 2e-5 }).collect();
    let mut session = Session::new(project);
    for (command, params) in [
        ("clip.addWarpMarker", json!({ "clip": id, "beat": 1.0 })),
        ("clip.moveWarpMarker", json!({ "clip": id, "from": 4.0, "beat": 5.0 })),
        ("clip.removeWarpMarker", json!({ "clip": id, "beat": 4.0 })),
    ] {
        let error = run(&mut session, command, params).unwrap_err();
        assert!(error.contains("Reset warp"), "{command}: {error}");
    }
    assert_eq!(markers_of(&session).len(), 100_001, "nothing refused changed anything");
    run(&mut session, "clip.resetWarp", json!({ "clip": id })).unwrap();
    assert_eq!(markers_of(&session), [(0.0, 0.0), (4.0, 2.0)]);
    run(&mut session, "clip.addWarpMarker", json!({ "clip": id, "beat": 2.0 })).unwrap();
    assert_eq!(markers_of(&session).len(), 3);
}

/// Without `start` and `end`, quantizing takes the part of the content the clip plays: a clip
/// trimmed to beat 2 leaves the click after its end, and the marker at the audio's end, alone.
#[test]
fn quantizing_without_a_range_takes_what_the_clip_plays() {
    let dir = TempDir::new("warp-quantize-played");
    let (mut session, id) = warped(&wav(&dir, "clicks.wav", &clicks()));
    run(&mut session, "clip.setTempo", json!({ "clip": id, "bpm": 110.0 })).unwrap();
    let end = *markers_of(&session).last().unwrap();
    run(&mut session, "clip.trim", json!({ "clip": id, "end": 2.0 })).unwrap();
    run(&mut session, "clip.quantizeWarpMarkers", json!({ "clip": id, "grid": "1/4" })).unwrap();
    let got = markers_of(&session);
    let want = [(0.0, 0.0), (1.0, 0.5), (2.0, 1.0), end];
    let near = got.len() == want.len()
        && got.iter().zip(&want).all(|(g, w)| (g.0 - w.0).abs() < 1e-6 && (g.1 - w.1).abs() < 0.002);
    assert!(near, "{got:?}");
}

/// The stretch settings' names are read as every command's texts are: trimmed, null left out.
#[test]
fn warp_setting_names_are_read_trimmed() {
    let dir = TempDir::new("warp-settings-trimmed");
    let (mut session, id) = tone(&dir);
    run(&mut session, "clip.setAudio", json!({ "clip": id, "slice": " 1/16 ", "tail": null })).unwrap();
    assert_eq!(clips(&session)[0].audio.warp.slice, SliceAt::Sixteenth);
    assert!(run(&mut session, "clip.setAudio", json!({ "clip": id, "slice": "  " })).is_err());
    assert!(run(&mut session, "clip.setAudio", json!({ "clip": id, "tail": 1 })).is_err());
}

/// Note starts found already (the window finds them on a thread of its own) are used as given:
/// a silent file, where analysis finds none, gets markers on them, sorted, each once, and none
/// past the end of its audio; quantizing moves the ones given.
#[test]
fn given_note_starts_are_used_instead_of_analysing_the_file() {
    let dir = TempDir::new("warp-given-transients");
    let (mut session, id) = warped(&wav(&dir, "silence.wav", &vec![0.0; 2 * SR as usize]));
    let found = run(&mut session, "clip.addWarpMarkersAtTransients", json!({ "clip": id })).unwrap();
    assert_eq!(found["added"], json!(0), "silence has no note starts of its own: {found}");
    let given = json!({ "clip": id, "transients": [1.5, 0.5, 1.0, 1.0, 3.0] });
    let placed = run(&mut session, "clip.addWarpMarkersAtTransients", given).unwrap();
    assert_eq!(placed["added"], json!(3), "{placed}");
    assert_eq!(markers_of(&session), [(0.0, 0.0), (1.0, 0.5), (2.0, 1.0), (3.0, 1.5), (4.0, 2.0)]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(markers_of(&session), [(0.0, 0.0), (4.0, 2.0)]);
    // 0.55 s plays at beat 1.1 and is quantized onto beat 1.
    let given = json!({ "clip": id, "grid": "1/4", "transients": [0.55] });
    let done = run(&mut session, "clip.quantizeWarpMarkers", given).unwrap();
    assert_eq!(done["moved"], json!(1), "{done}");
    let got = markers_of(&session);
    let want = [(0.0, 0.0), (1.0, 0.55), (4.0, 2.0)];
    let near = got.len() == want.len()
        && got.iter().zip(&want).all(|(g, w)| (g.0 - w.0).abs() < 1e-9 && (g.1 - w.1).abs() < 1e-12);
    assert!(near, "{got:?}");
    // Reset warp takes the same list (its tempo is not read from note starts).
    run(&mut session, "clip.resetWarp", json!({ "clip": id, "transients": [0.5] })).unwrap();
    assert_eq!(markers_of(&session), [(0.0, 0.0), (4.0, 2.0)]);
}

/// A list of note starts is hostile input: not a list, a value that is not a number (NaN is
/// written as null), a negative second or more than a clip's markers are refused by each command
/// that takes one, and the markers stay as they were.
#[test]
fn malformed_note_starts_are_refused() {
    let dir = TempDir::new("warp-bad-transients");
    let (mut session, id) = warped(&wav(&dir, "silence.wav", &vec![0.0; 2 * SR as usize]));
    let too_many = vec![0.5; 100_001];
    let bad = [json!(0.5), json!("0.5"), json!(["0.5"]), json!([0.5, f64::NAN]), json!([0.5, -0.1]), json!(too_many)];
    for transients in bad {
        for (command, mut params) in [
            ("clip.addWarpMarkersAtTransients", json!({ "clip": id })),
            ("clip.quantizeWarpMarkers", json!({ "clip": id, "grid": "1/4" })),
            ("clip.resetWarp", json!({ "clip": id })),
        ] {
            params["transients"] = transients.clone();
            assert!(run(&mut session, command, params).is_err(), "{command} took {transients}");
        }
    }
    assert_eq!(markers_of(&session), [(0.0, 0.0), (4.0, 2.0)]);
}
