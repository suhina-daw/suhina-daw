// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An audio clip's own audio state (#742, #824, `docs/spec/clip-panel.md`): two clips of one file
//! keep their own gain, envelope and warp; `clip.setEnvelope`, `clip.setTempo` and
//! `clip.findTempo`. The offline render is the oracle for what a clip sounds like.
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use std::path::Path;

use serde_json::json;
use suhina_engine::RenderInputs;
use suhina_model::*;
use suhina_session::{Session, blank_project};
use suhina_test_support::{WavFormat, write_wav};

use crate::support::run;
use crate::temp_dir::TempDir;

const SR: u32 = 48_000;

/// A mono WAV file of `seconds` at a steady `level`.
fn wav(dir: &Path, name: &str, seconds: f64, level: f32) -> String {
    let path = dir.join(name);
    write_wav(&path, SR, 1, WavFormat::Float32, &vec![level; (seconds * f64::from(SR)) as usize]);
    path.display().to_string()
}

/// A session with `file` (two seconds, four beats at 120 BPM) placed on the audio track (track 3)
/// at beat 0, and duplicated: the copy at beat 4 plays the same content.
fn two_clips(file: &str) -> Session {
    let mut session = Session::new(blank_project());
    run(&mut session, "clip.addFiles", json!({ "paths": [file], "track": 3, "start": 0.0 })).unwrap();
    let first = clips(&session)[0].id.0;
    run(&mut session, "clip.duplicate", json!({ "clips": [{ "clip": first }] })).unwrap();
    session
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

#[test]
fn two_clips_of_one_file_have_their_own_gain() {
    let dir = TempDir::new("clip-audio-gain");
    let mut session = two_clips(&wav(&dir, "hit.wav", 2.0, 0.5));
    let [a, b] = clips(&session).try_into().unwrap();
    assert_eq!(a.content, b.content, "both play one content");
    run(&mut session, "clip.setGain", json!({ "clip": b.id.0, "db": -6.0 })).unwrap();
    let [a, b] = clips(&session).try_into().unwrap();
    assert_eq!(a.audio.gain, 1.0, "the other clip keeps its gain");
    assert!((b.audio.gain - 0.501).abs() < 1e-3);
    // Heard: the second clip plays at half the level of the first.
    let (first, second) = (level_at(&session, 1.0), level_at(&session, 5.0));
    assert!(first > 0.0 && (second / first - 0.501).abs() < 0.01, "{first} {second}");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(clips(&session)[1].audio.gain, 1.0);
}

#[test]
fn a_gain_envelope_shapes_one_clip_and_undoes_in_one_step() {
    let dir = TempDir::new("clip-audio-envelope");
    let mut session = two_clips(&wav(&dir, "pad.wav", 2.0, 0.5));
    let [a, b] = clips(&session).try_into().unwrap();
    // Given out of order: kept in beat order. 0 dB at the start, -20 dB from beat 2.
    let out =
        run(&mut session, "clip.setEnvelope", json!({ "clip": a.id.0, "points": [2.0, -20.0, 0.0, 0.0] })).unwrap();
    assert_eq!(out["points"], json!([0.0, 0.0, 2.0, -20.0]));
    let [a2, b2] = clips(&session).try_into().unwrap();
    assert_eq!(a2.audio.gain_envelope.len(), 2);
    assert!(b2.audio.gain_envelope.is_empty(), "the other clip has none");
    // Straight in dB: -10 dB half-way, -20 dB past the last point.
    // The other clip, at its own gain, is the reference.
    let full = level_at(&session, 5.0);
    let half = level_at(&session, 1.0);
    let late = level_at(&session, 3.0);
    assert!((half / full - 0.316).abs() < 0.01, "{full} {half}");
    assert!((late / full - 0.1).abs() < 0.005, "{full} {late}");
    assert!((level_at(&session, 7.0) / full - 1.0).abs() < 0.01, "the other clip is not shaped");
    // Refused: an odd list, a dB out of range, a negative beat, a MIDI clip.
    assert!(run(&mut session, "clip.setEnvelope", json!({ "clip": a.id.0, "points": [1.0] })).is_err());
    assert!(run(&mut session, "clip.setEnvelope", json!({ "clip": a.id.0, "points": [0.0, 30.0] })).is_err());
    assert!(run(&mut session, "clip.setEnvelope", json!({ "clip": a.id.0, "points": [-1.0, 0.0] })).is_err());
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(clips(&session)[0].audio.gain_envelope.is_empty(), "one undo step");
    assert_eq!(clips(&session)[1], b);
}

/// Warp turned off and on again: the clip's length follows its audio, and so do the envelope's
/// points, each staying on the sound it was put on, then back on its beat.
#[test]
fn envelope_points_follow_the_audio_when_warp_turns_off_and_on() {
    let dir = TempDir::new("clip-audio-warp-envelope");
    let mut session = two_clips(&wav(&dir, "pad.wav", 2.0, 0.5));
    let a = clips(&session)[0].id;
    run(&mut session, "clip.setTempo", json!({ "clip": a.0, "bpm": 100.0, "source": "manual" })).unwrap();
    run(&mut session, "clip.setEnvelope", json!({ "clip": a.0, "points": [1.0, 0.0, 2.0, -6.0] })).unwrap();
    let length = clips(&session)[0].length;
    run(&mut session, "clip.setAudio", json!({ "clip": a.0, "warp": false })).unwrap();
    let (clip, after) = (clips(&session)[0].clone(), clips(&session)[0].audio.gain_envelope.clone());
    // The point moved by as much as the clip's length did.
    let scale = clip.length / length;
    assert!((scale - 1.0).abs() > 0.01, "the length changed: {scale}");
    assert!((after[1].beat - 2.0 * scale).abs() < 1e-6, "{after:?} at {scale}");
    assert_eq!(after[1].db, -6.0);
    run(&mut session, "clip.setAudio", json!({ "clip": a.0, "warp": true })).unwrap();
    let back = clips(&session)[0].audio.gain_envelope.clone();
    assert!((back[1].beat - 2.0).abs() < 1e-6, "{back:?}");
}

#[test]
fn two_envelope_points_in_one_place_are_one_and_a_beat_holds_a_step() {
    let dir = TempDir::new("clip-audio-merge");
    let mut session = two_clips(&wav(&dir, "pad.wav", 2.0, 0.5));
    let a = clips(&session)[0].id;
    // A point dropped on another (same beat, same dB) is one point.
    let out = run(&mut session, "clip.setEnvelope", json!({ "clip": a.0, "points": [0.0, 0.0, 2.0, -6.0, 2.0, -6.0] }))
        .unwrap();
    assert_eq!(out["points"], json!([0.0, 0.0, 2.0, -6.0]));
    // Two on one beat at other levels are a step; a third there is where the step goes to.
    let out = run(
        &mut session,
        "clip.setEnvelope",
        json!({ "clip": a.0, "points": [2.0, 0.0, 2.0, -6.0, 2.0, -12.0, 4.0, -12.0] }),
    )
    .unwrap();
    assert_eq!(out["points"], json!([2.0, 0.0, 2.0, -12.0, 4.0, -12.0]));
}

#[test]
fn a_split_clip_keeps_its_part_of_the_envelope_and_a_loop_repeats_it() {
    let dir = TempDir::new("clip-audio-split");
    let mut session = Session::new(blank_project());
    run(&mut session, "clip.addFiles", json!({ "paths": [wav(&dir, "pad.wav", 2.0, 0.5)], "track": 3, "start": 0.0 }))
        .unwrap();
    let clip = clips(&session)[0].id.0;
    run(&mut session, "clip.setEnvelope", json!({ "clip": clip, "points": [0.0, -20.0, 2.0, -20.0, 2.0, 0.0] }))
        .unwrap();
    let quiet = level_at(&session, 1.0);
    let loud = level_at(&session, 3.0);
    assert!(quiet > 0.0 && (quiet / loud - 0.1).abs() < 0.005, "{quiet} {loud}");
    // Looped over its first two beats and made longer: the quiet part comes round again.
    run(&mut session, "clip.setLoop", json!({ "clip": clip, "start": 0.0, "end": 4.0 })).unwrap();
    run(&mut session, "clip.trim", json!({ "clip": clip, "end": 8.0 })).unwrap();
    assert!((level_at(&session, 5.0) / quiet - 1.0).abs() < 0.01, "the second pass starts quiet");
    assert!((level_at(&session, 7.0) / loud - 1.0).abs() < 0.01, "and gets loud at its beat 2");
}

#[test]
fn a_clips_tempo_is_set_halved_and_doubled_and_its_length_follows() {
    let dir = TempDir::new("clip-audio-tempo");
    // Four seconds with no tempo: unwarped, 8 beats at the project's 120 BPM.
    let mut session = Session::new(blank_project());
    run(&mut session, "clip.addFiles", json!({ "paths": [wav(&dir, "hit.wav", 4.0, 0.5)], "track": 3, "start": 2.0 }))
        .unwrap();
    let clip = clips(&session)[0].clone();
    assert!(!clip.audio.warped && clip.audio.tempo_source.is_none());
    assert_eq!(clip.length, 8.0);
    // At 60 BPM the four seconds hold four beats: it plays twice as fast, half as long.
    let out = run(&mut session, "clip.setTempo", json!({ "clip": clip.id.0, "bpm": 60.0 })).unwrap();
    assert_eq!((out["bpm"].as_f64(), out["source"].as_str()), (Some(60.0), Some("manual")));
    let set = clips(&session)[0].clone();
    assert!(set.audio.warped, "warp turned on");
    assert_eq!((set.start, set.length), (2.0, 4.0), "the start stays");
    assert_eq!(set.audio.tempo(), Some(60.0));
    assert_eq!(set.audio.tempo_source, Some(TempoSource::Manual));
    // Doubled: 120 again, 8 beats; halved from there: 60.
    run(&mut session, "clip.setTempo", json!({ "clip": clip.id.0, "bpm": 120.0, "source": "detected" })).unwrap();
    let doubled = clips(&session)[0].clone();
    assert_eq!((doubled.length, doubled.audio.tempo_source), (8.0, Some(TempoSource::Detected)));
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(clips(&session)[0].length, 4.0, "one undo step");
    // Refused: out of range, an unknown source.
    assert!(run(&mut session, "clip.setTempo", json!({ "clip": clip.id.0, "bpm": 5.0 })).is_err());
    assert!(run(&mut session, "clip.setTempo", json!({ "clip": clip.id.0, "bpm": 90.0, "source": "x" })).is_err());
    // A selector that names no clip says so, not "only an audio clip has a tempo".
    let err = run(&mut session, "clip.setTempo", json!({ "clip": 999_999, "bpm": 90.0 })).unwrap_err();
    assert!(!err.contains("only an audio clip"), "{err}");
}

#[test]
fn a_slot_clips_tempo_is_its_own() {
    let dir = TempDir::new("clip-audio-slot-tempo");
    let mut session = Session::new(blank_project());
    run(&mut session, "clip.addFiles", json!({ "paths": [wav(&dir, "hit.wav", 4.0, 0.5)], "track": 3, "scene": 0 }))
        .unwrap();
    run(&mut session, "clip.setTempo", json!({ "track": 3, "scene": 0, "bpm": 240.0 })).unwrap();
    let column = session.project().launcher.columns.iter().find(|c| c.track == TrackId(3)).unwrap();
    let clip = column.slots[0].clip.as_ref().unwrap();
    assert_eq!((clip.length, clip.audio.tempo()), (16.0, Some(240.0)));
}

#[test]
fn find_tempo_says_what_each_source_finds() {
    let dir = TempDir::new("clip-audio-find");
    let mut session = Session::new(blank_project());
    run(
        &mut session,
        "clip.addFiles",
        json!({ "paths": [wav(&dir, "drums 96bpm.wav", 5.0, 0.5)], "track": 3, "start": 0.0 }),
    )
    .unwrap();
    let clip = clips(&session)[0].clone();
    assert_eq!(clip.audio.tempo_source, Some(TempoSource::Name), "the drop took it from the name");
    let found = run(&mut session, "clip.findTempo", json!({ "clip": clip.id.0 })).unwrap();
    assert_eq!(found["name"], 96.0);
    assert_eq!(found["metadata"], serde_json::Value::Null);
    assert_eq!(found["found"], 96.0);
    assert_eq!((found["bpm"].as_f64().map(f64::round), found["source"].as_str()), (Some(96.0), Some("name")));
    // A MIDI clip has no tempo of its own.
    run(&mut session, "clip.create", json!({ "track": 1, "start": 0.0 })).unwrap();
    let midi = session.project().arrangement.lanes.iter().find(|l| l.track == Some(TrackId(1))).unwrap().clips[0].id;
    assert!(run(&mut session, "clip.findTempo", json!({ "clip": midi.0 })).is_err());
}

/// The calls of one drag (one `gesture`) make one undo step, as the clip panel's live controls
/// send them.
#[test]
fn a_dragged_gain_is_one_undo_step() {
    let dir = TempDir::new("clip-audio-continue");
    let mut session = two_clips(&wav(&dir, "hit.wav", 2.0, 0.5));
    let clip = clips(&session)[0].id.0;
    for db in [-1.0, -2.0, -3.0] {
        run(&mut session, "clip.setGain", json!({ "clip": clip, "db": db, "gesture": "drag" })).unwrap();
    }
    assert!((clips(&session)[0].audio.gain - 0.708).abs() < 1e-3);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(clips(&session)[0].audio.gain, 1.0, "one undo step for the whole drag");
}

/// A clip warped, then unwarped, keeps its markers; its tempo is set from how it plays now (its
/// seconds at the song's tempo), not from the markers it kept.
#[test]
fn an_unwarped_clips_tempo_is_set_from_how_it_plays() {
    let dir = TempDir::new("clip-audio-unwarped-tempo");
    let mut session = Session::new(blank_project());
    run(&mut session, "clip.addFiles", json!({ "paths": [wav(&dir, "hit.wav", 4.0, 0.5)], "track": 3, "start": 0.0 }))
        .unwrap();
    let clip = clips(&session)[0].id.0;
    run(&mut session, "clip.setTempo", json!({ "clip": clip, "bpm": 60.0 })).unwrap();
    run(&mut session, "clip.setAudio", json!({ "clip": clip, "warp": false })).unwrap();
    // Unwarped, 4 s at 120 BPM are 8 beats again.
    assert_eq!(clips(&session)[0].length, 8.0);
    // Set to 120, the tempo it plays at now: its length stays.
    run(&mut session, "clip.setTempo", json!({ "clip": clip, "bpm": 120.0 })).unwrap();
    let set = clips(&session)[0].clone();
    assert!(set.audio.warped);
    assert!((set.length - 8.0).abs() < 1e-9, "{}", set.length);
    assert_eq!(set.audio.tempo(), Some(120.0));
}

/// A tempo that makes a clip longer stops it at the next clip on its lane; the next clip stays.
#[test]
fn a_longer_tempo_never_removes_the_next_clip() {
    let dir = TempDir::new("clip-audio-tempo-room");
    // Two seconds: four beats at 120, at beats 0 and 4.
    let mut session = two_clips(&wav(&dir, "hit.wav", 2.0, 0.5));
    let first = clips(&session)[0].id.0;
    run(&mut session, "clip.setTempo", json!({ "clip": first, "bpm": 240.0 })).unwrap();
    let [a, b] = clips(&session).try_into().unwrap();
    assert_eq!((a.start, a.length), (0.0, 4.0), "stopped where the next clip starts");
    assert_eq!(b.start, 4.0, "the next clip is still there");
    assert_eq!(a.audio.tempo(), Some(240.0));
}

/// `clip.setEnvelope` takes each point's curve, as automation does: a bend or four numbers, null
/// for straight; a bad one is refused.
#[test]
fn an_envelope_takes_curves_between_its_points() {
    let dir = TempDir::new("clip-audio-curves");
    let mut session = two_clips(&wav(&dir, "hit.wav", 2.0, 0.5));
    let clip = clips(&session)[0].id.0;
    run(
        &mut session,
        "clip.setEnvelope",
        json!({ "clip": clip, "points": [0.0, 0.0, 4.0, -12.0], "curves": [0.5, null] }),
    )
    .unwrap();
    let points = clips(&session)[0].audio.gain_envelope.clone();
    assert_eq!(points[0].curve, suhina_engine::model::automation::Curve::bend(0.5));
    assert!(points[1].curve.is_straight());
    let bad = json!({ "clip": clip, "points": [0.0, 0.0, 4.0, -12.0], "curves": [7.0] });
    assert!(run(&mut session, "clip.setEnvelope", bad).is_err());
    let too_many = json!({ "clip": clip, "points": [0.0, 0.0], "curves": [0.5, 0.5] });
    assert!(run(&mut session, "clip.setEnvelope", too_many).is_err());
}
