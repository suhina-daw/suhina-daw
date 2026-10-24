// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Adding tracks and placing files (`docs/spec/browser.md` › Files, › Commands): `track.add`,
//! `clip.addFiles`, presets with `device.add`, and `preview.*`. The offline render is the oracle
//! for what a placed sample sounds like, and a player attached to the session shows that a file
//! added while playing reaches the engine (ADR 0011).
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use std::path::{Path, PathBuf};
use suhina_engine::{RenderInputs, RenderOptions};

use serde_json::{Value, json};
use suhina_engine::{AudioPool, Renderer};
use suhina_model::*;
use suhina_session::{Player, Session, blank_project};

use crate::support::run;
use suhina_test_support::{WavFormat, write_wav};

const SR: u32 = 48_000;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A mono WAV file of `seconds` at a steady `level`.
fn wav(dir: &Path, name: &str, seconds: f64, level: f32) -> PathBuf {
    let path = dir.join(name);
    write_wav(&path, SR, 1, WavFormat::Float32, &vec![level; (seconds * f64::from(SR)) as usize]);
    path
}

fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = id.to_vec();
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// A type 1 MIDI file at 96 ticks a beat with the given tracks of (name, notes as (tick, length,
/// key)).
/// A MIDI track for [`midi`]: its name and notes as (tick, length, key).
type TrackNotes<'a> = (&'a str, &'a [(u32, u32, u8)]);

fn midi(dir: &Path, name: &str, tracks: &[TrackNotes<'_>]) -> PathBuf {
    let mut header = 1u16.to_be_bytes().to_vec();
    header.extend_from_slice(&(tracks.len() as u16).to_be_bytes());
    header.extend_from_slice(&96u16.to_be_bytes());
    let mut file = chunk(b"MThd", &header);
    for (track_name, notes) in tracks {
        let mut events: Vec<(u32, Vec<u8>)> = Vec::new();
        for (tick, length, key) in notes.iter() {
            events.push((*tick, vec![0x90, *key, 100]));
            events.push((tick + length, vec![0x80, *key, 64]));
        }
        events.sort_by_key(|e| e.0);
        let mut body = vec![0x00, 0xFF, 0x03, track_name.len() as u8];
        body.extend_from_slice(track_name.as_bytes());
        let mut now = 0;
        for (tick, bytes) in events {
            let delta = tick - now;
            now = tick;
            // Deltas here stay under 2^14.
            if delta >= 0x80 {
                body.push(0x80 | (delta >> 7) as u8);
            }
            body.push((delta & 0x7F) as u8);
            body.extend(bytes);
        }
        body.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
        file.extend(chunk(b"MTrk", &body));
    }
    let path = dir.join(name);
    std::fs::write(&path, file).unwrap();
    path
}

fn names(p: &Project) -> Vec<String> {
    p.tracks.iter().map(|t| t.name.clone()).collect()
}

fn text(path: &Path) -> String {
    path.display().to_string()
}

#[test]
fn sd_roles_choose_the_new_tracks_kind() {
    use suhina_device_sdk::{Connection, DeviceGraph, DeviceRole, Node, NodeKind};
    let dir = temp_dir("sd-track-roles");
    for (role, input, output, kind) in [
        (DeviceRole::Effect, NodeKind::AudioInput, NodeKind::AudioOutput, "audio"),
        (DeviceRole::Instrument, NodeKind::MidiGate, NodeKind::AudioOutput, "midi"),
        (DeviceRole::MidiEffect, NodeKind::MidiInput, NodeKind::MidiOutput, "midi"),
    ] {
        let mut graph = DeviceGraph::new("test.track-role", "Track Role", role);
        graph.nodes = vec![Node::new("in", input), Node::new("out", output)];
        graph.connections = vec![Connection::new("in", "out")];
        let path = dir.join(format!("{kind}-{role:?}.sd"));
        std::fs::write(&path, graph.to_sd().unwrap()).unwrap();
        let mut session = Session::new(blank_project());
        let before = session.project().tracks.len();
        let added = run(&mut session, "track.add", json!({"sd": text(&path)})).unwrap();
        assert_eq!(added["kind"], kind);
        assert_eq!(session.project().tracks.len(), before + 1);
        assert!(!added["device"].is_null());
        run(&mut session, "edit.undo", json!({})).unwrap();
        assert_eq!(session.project().tracks.len(), before);
        if kind == "midi" {
            assert!(run(&mut session, "track.add", json!({"sd": text(&path), "kind": "audio"})).is_err());
            assert_eq!(session.project().tracks.len(), before);
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_track_is_added_before_the_returns_with_its_channel_slots_and_lane() {
    let mut session = Session::new(blank_project());
    let added = run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    let id = TrackId(added["track"].as_u64().unwrap() as u32);
    let p = session.project();
    assert_eq!(names(p), ["1 MIDI", "2 MIDI", "3 Audio", "4 Audio", "Audio 3", "A Return", "B Return"]);
    let track = p.track(id).unwrap();
    assert_eq!(track.content, ContentKinds { notes: false, audio: true });
    let channel = p.channels.iter().find(|c| Some(c.id) == track.channel).unwrap();
    assert_eq!((channel.role, channel.output), (ChannelRole::Regular, Some(ChannelId(100))));
    let column = p.launcher.columns.iter().find(|c| c.track == id).unwrap();
    assert_eq!(column.slots.len(), p.launcher.scenes.len());
    assert!(p.arrangement.lanes.iter().any(|l| l.track == Some(id)));
    assert_eq!(p.validate(), Ok(()));

    // After a named track, with a name.
    run(&mut session, "track.add", json!({ "kind": "midi", "after": "1 MIDI", "name": "Keys" })).unwrap();
    assert_eq!(names(session.project())[1], "Keys");
    // One undo step each.
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    // Undo takes the edit back; the id counter stays where it got to (ADR 0025).
    assert_eq!(session.project(), &Project { next_id: session.project().next_id, ..blank_project() });
}

#[test]
fn a_track_added_with_a_device_takes_its_kind_and_name() {
    let mut session = Session::new(blank_project());
    let synth = run(&mut session, "track.add", json!({ "device": "fm-synth" })).unwrap();
    assert_eq!((synth["kind"].as_str(), synth["name"].as_str()), (Some("midi"), Some("FM Synth")));
    let effect = run(&mut session, "track.add", json!({ "device": "overdrive" })).unwrap();
    assert_eq!(effect["kind"], "audio");
    let p = session.project();
    let devices_of = |id: &Value| {
        let track = p.tracks.iter().find(|t| u64::from(t.id.0) == id.as_u64().unwrap()).unwrap();
        let channel = p.channels.iter().find(|c| Some(c.id) == track.channel).unwrap();
        channel.devices.iter().map(|d| d.name.clone()).collect::<Vec<_>>()
    };
    assert_eq!(devices_of(&synth["track"]), ["FM Synth"]);
    assert_eq!(devices_of(&effect["track"]), ["Overdrive"]);

    let before = session.project().clone();
    for bad in [
        json!({}),
        json!({ "kind": "video" }),
        json!({ "kind": "audio", "device": "fm-synth" }),
        json!({ "device": "no-such-device" }),
        json!({ "kind": "midi", "after": "Nobody" }),
        json!({ "kind": "midi", "name": 7 }),
    ] {
        assert!(run(&mut session, "track.add", bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(session.project(), &before, "failed adds change nothing");
}

#[test]
fn samples_go_one_after_another_on_a_track_at_their_length() {
    let dir = temp_dir("sequence");
    let one = wav(&dir, "hit one.wav", 0.5, 0.5);
    let two = wav(&dir, "hit two.wav", 1.0, 0.25);
    let mut session = Session::new(blank_project());
    let out = run(
        &mut session,
        "clip.addFiles",
        json!({ "paths": [text(&one), text(&two)], "track": "3 Audio", "start": 4.0 }),
    )
    .unwrap();
    assert_eq!(out["tracks"], json!([]));
    let p = session.project();
    let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(3))).unwrap();
    // At 120 BPM half a second is a beat; neither file has a tempo, so neither is warped.
    let placed: Vec<(f64, f64)> = lane.clips.iter().map(|c| (c.start, c.length)).collect();
    assert_eq!(placed, [(4.0, 1.0), (5.0, 2.0)]);
    let content = p.content(lane.clips[0].content).unwrap();
    assert_eq!(content.name, "hit one");
    let LaneData::Audio(region) = &content.lanes[0].data else { panic!("not audio") };
    assert!(!region.warped);
    assert_eq!(region.file, text(&std::path::absolute(&one).unwrap()));
    assert!(session.audio().files.contains_key(&region.file), "decoded into the session's pool");

    // The offline render plays them: the oracle for what was placed.
    let renderer =
        Renderer::build(session.project(), RenderInputs::from_audio(session.audio()), SR, RenderOptions::PLAYING)
            .unwrap();
    drop(renderer);
    let out =
        suhina_engine::render_offline_from(session.project(), RenderInputs::from_audio(session.audio()), SR, 512, None)
            .unwrap();
    let at = |beat: f64| out[((beat * 0.5 + 0.05) * f64::from(SR)) as usize * 2].abs();
    assert!(at(4.0) > 0.1 && at(5.0) > 0.05, "{} {}", at(4.0), at(5.0));
    assert!(at(2.0) < 1e-6, "nothing before the clips");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sample_with_a_tempo_in_its_name_is_warped_to_the_project() {
    let dir = temp_dir("warp");
    // Two seconds at 60 BPM is two beats.
    let looped = wav(&dir, "loop 60bpm.wav", 2.0, 0.5);
    let mut session = Session::new(blank_project());
    run(&mut session, "clip.addFiles", json!({ "paths": [text(&looped)], "track": 3, "start": 0.0 })).unwrap();
    let p = session.project();
    let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(3))).unwrap();
    assert_eq!(lane.clips[0].length, 2.0);
    let LaneData::Audio(region) = &p.content(lane.clips[0].content).unwrap().lanes[0].data else { panic!() };
    assert!(region.warped);
    assert_eq!(region.stretch, StretchMode::Rhythmic);
    assert_eq!(region.warp_markers.last().map(|m| (m.time, m.audio_time)), Some((2.0, 2.0)));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The slot clip in `scene` (from 1) of `track`.
fn slot_clip(p: &Project, track: u32, scene: usize) -> &Clip {
    let column = p.launcher.columns.iter().find(|c| c.track == TrackId(track)).unwrap();
    column.slots[scene - 1].clip.as_ref().unwrap()
}

#[test]
fn a_sample_with_no_known_tempo_loops_in_its_slot() {
    // The tester's case (#316): a short WAV with no tempo, dropped in a slot and launched.
    let dir = temp_dir("slot-loop");
    // 0.75 s at 120 BPM is a beat and a half: not a whole bar, and no tempo to find in it.
    let hit = wav(&dir, "hit.wav", 0.75, 0.5);
    let mut session = Session::new(blank_project());
    run(&mut session, "clip.addFiles", json!({ "paths": [text(&hit)], "track": "3 Audio", "scene": 1 })).unwrap();
    let clip = slot_clip(session.project(), 3, 1);
    assert_eq!((clip.length, clip.loop_range), (1.5, Some(LoopRange { start: 0.0, end: 1.5 })));
    let LaneData::Audio(region) = &session.project().content(clip.content).unwrap().lanes[0].data else {
        panic!("not audio")
    };
    assert!(!region.warped, "it plays at its own speed");

    // Launched, it repeats: sound in the second and third passes, not only the first.
    let column = session.project().launcher.columns.iter().position(|c| c.track == TrackId(3)).unwrap();
    let launch = [(0.0, suhina_engine::LaunchCommand::LaunchClip { column, slot: 0 })];
    let render = |session: &Session| {
        suhina_engine::render_launches(session.project(), session.audio(), SR, &launch, 2.0).unwrap()
    };
    let at = |out: &[f32], seconds: f64| out[(seconds * f64::from(SR)) as usize * 2].abs();
    let out = render(&session);
    assert!(at(&out, 0.3) > 0.1 && at(&out, 1.0) > 0.1 && at(&out, 1.8) > 0.1);

    // clip.setLoop off makes it a one-shot: it plays once and the slot falls silent.
    let done = run(&mut session, "clip.setLoop", json!({ "track": 3, "scene": 1, "loop": false })).unwrap();
    assert_eq!(done, json!({ "clip": slot_clip(session.project(), 3, 1).id.0, "scene": 1, "loop": false }));
    assert_eq!(slot_clip(session.project(), 3, 1).loop_range, None);
    let out = render(&session);
    assert!(at(&out, 0.3) > 0.1 && at(&out, 1.0) < 1e-6 && at(&out, 1.8) < 1e-6);

    // On again loops the whole clip; one undo step each.
    run(&mut session, "clip.setLoop", json!({ "track": 3, "scene": 1, "loop": true })).unwrap();
    assert_eq!(slot_clip(session.project(), 3, 1).loop_range, Some(LoopRange { start: 0.0, end: 1.5 }));
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(slot_clip(session.project(), 3, 1).loop_range, None);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(slot_clip(session.project(), 3, 1).loop_range.is_some());

    // An empty slot, a missing scene and a value that is not true or false are errors.
    assert!(run(&mut session, "clip.setLoop", json!({ "track": 3, "scene": 2, "loop": true })).is_err());
    assert!(run(&mut session, "clip.setLoop", json!({ "track": 3, "scene": 99, "loop": true })).is_err());
    assert!(run(&mut session, "clip.setLoop", json!({ "track": 3, "scene": 1, "loop": "yes" })).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sample_with_no_known_tempo_does_not_loop_in_the_arrangement() {
    let dir = temp_dir("arrangement-once");
    let hit = wav(&dir, "hit.wav", 0.75, 0.5);
    let mut session = Session::new(blank_project());
    run(&mut session, "clip.addFiles", json!({ "paths": [text(&hit)], "track": 3, "start": 0.0 })).unwrap();
    let lane = session.project().arrangement.lanes.iter().find(|l| l.track == Some(TrackId(3))).unwrap();
    assert_eq!(lane.clips[0].loop_range, None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn files_go_down_the_slots_from_a_scene_and_add_scenes_when_needed() {
    let dir = temp_dir("slots");
    let paths: Vec<String> = (0..3).map(|i| text(&wav(&dir, &format!("s{i}.wav"), 0.5, 0.5))).collect();
    let mut project = blank_project();
    project.launcher.scenes.truncate(2);
    for column in &mut project.launcher.columns {
        column.slots.truncate(2);
    }
    let mut session = Session::new(project);
    run(&mut session, "clip.addFiles", json!({ "paths": paths, "track": "4 Audio", "scene": 2 })).unwrap();
    let p = session.project();
    assert_eq!(p.launcher.scenes.len(), 4, "two scenes added");
    assert!(p.launcher.columns.iter().all(|c| c.slots.len() == 4));
    let column = p.launcher.columns.iter().find(|c| c.track == TrackId(4)).unwrap();
    let filled: Vec<bool> = column.slots.iter().map(|s| s.clip.is_some()).collect();
    assert_eq!(filled, [false, true, true, true]);
    assert_eq!(p.validate(), Ok(()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn separate_puts_each_file_on_a_track_of_its_own() {
    let dir = temp_dir("separate");
    let paths: Vec<String> =
        ["kick.wav", "snare.wav", "hat.wav"].iter().map(|n| text(&wav(&dir, n, 0.5, 0.5))).collect();
    let mut session = Session::new(blank_project());
    let out = run(
        &mut session,
        "clip.addFiles",
        json!({ "paths": paths, "track": "3 Audio", "start": 8.0, "separate": true }),
    )
    .unwrap();
    assert_eq!(out["tracks"].as_array().map(Vec::len), Some(2));
    let p = session.project();
    assert_eq!(names(p)[2..6], ["3 Audio", "snare", "hat", "4 Audio"]);
    for lane in p.arrangement.lanes.iter().filter(|l| l.clips.len() == 1) {
        assert_eq!(lane.clips[0].start, 8.0, "side by side, at the same beat");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mixed_files_on_empty_space_make_an_audio_and_a_midi_track() {
    let dir = temp_dir("mixed");
    let a = wav(&dir, "pad.wav", 0.5, 0.5);
    let b = wav(&dir, "stab.wav", 0.5, 0.5);
    let m = midi(&dir, "groove.mid", &[("", &[(0, 96, 60), (192, 96, 64)])]);
    let mut session = Session::new(blank_project());
    let out =
        run(&mut session, "clip.addFiles", json!({ "paths": [text(&a), text(&m), text(&b)], "scene": 1 })).unwrap();
    assert_eq!(out["tracks"].as_array().map(Vec::len), Some(2));
    let p = session.project();
    assert_eq!(names(p)[4..6], ["Audio 3", "groove"]);
    let midi_track = p.tracks.iter().find(|t| t.name == "groove").unwrap();
    assert_eq!(midi_track.content, ContentKinds { notes: true, audio: false });
    // The MIDI clip is a bar long (its notes end at beat 3) and loops in its slot.
    let column = p.launcher.columns.iter().find(|c| c.track == midi_track.id).unwrap();
    let clip = column.slots[0].clip.as_ref().unwrap();
    assert_eq!((clip.length, clip.loop_range.map(|r| r.end)), (4.0, Some(4.0)));

    // With separate, one track per file.
    let mut session = Session::new(blank_project());
    let out = run(
        &mut session,
        "clip.addFiles",
        json!({ "paths": [text(&a), text(&m), text(&b)], "start": 0.0, "separate": true }),
    )
    .unwrap();
    assert_eq!(out["tracks"].as_array().map(Vec::len), Some(3));
    assert_eq!(names(session.project())[4..7], ["pad", "groove", "stab"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn files_a_track_cannot_hold_go_on_a_new_track_after_it() {
    let dir = temp_dir("mismatch");
    let a = wav(&dir, "vox.wav", 0.5, 0.5);
    let m = midi(&dir, "bass.mid", &[("", &[(0, 96, 40)])]);
    let mut session = Session::new(blank_project());
    run(&mut session, "clip.addFiles", json!({ "paths": [text(&m), text(&a)], "track": "1 MIDI", "start": 0.0 }))
        .unwrap();
    let p = session.project();
    assert_eq!(names(p)[..3], ["1 MIDI", "vox", "2 MIDI"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_type_1_midi_file_gives_a_clip_per_track_with_notes() {
    let dir = temp_dir("midi");
    let m =
        midi(&dir, "song.mid", &[("Drums", &[(0, 48, 36), (96, 48, 38)]), ("Empty", &[]), ("Bass", &[(0, 480, 33)])]);
    let mut session = Session::new(blank_project());
    run(&mut session, "clip.addFiles", json!({ "paths": [text(&m)], "track": "2 MIDI", "start": 0.0 })).unwrap();
    let p = session.project();
    let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(2))).unwrap();
    let clips: Vec<(f64, f64, String)> =
        lane.clips.iter().map(|c| (c.start, c.length, p.content(c.content).unwrap().name.clone())).collect();
    assert_eq!(clips, [(0.0, 4.0, "song – Drums".to_string()), (4.0, 8.0, "song – Bass".to_string())]);
    let LaneData::Notes(notes) = &p.content(lane.clips[0].content).unwrap().lanes[0].data else { panic!() };
    assert_eq!(notes.iter().map(|n| (n.time, n.key)).collect::<Vec<_>>(), [(0.0, 36), (1.0, 38)]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_drop_is_one_undo_step() {
    let dir = temp_dir("undo");
    let a = wav(&dir, "a.wav", 0.5, 0.5);
    let m = midi(&dir, "b.mid", &[("", &[(0, 96, 60)])]);
    let mut session = Session::new(blank_project());
    run(&mut session, "clip.addFiles", json!({ "paths": [text(&a), text(&m)], "scene": 3, "separate": true })).unwrap();
    assert_eq!(session.project().tracks.len(), 8);
    run(&mut session, "edit.undo", json!({})).unwrap();
    // Undo takes the edit back; the id counter stays where it got to (ADR 0025).
    assert_eq!(session.project(), &Project { next_id: session.project().next_id, ..blank_project() });
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unreadable_files_are_skipped_and_reported() {
    let dir = temp_dir("hostile");
    let good = wav(&dir, "good.wav", 0.5, 0.5);
    let broken = dir.join("broken.wav");
    std::fs::write(&broken, b"RIFF\x24\x00\x00\x00WAVEfmt nonsense").unwrap();
    let text_file = dir.join("notes.txt");
    std::fs::write(&text_file, b"hello").unwrap();
    let bad_midi = dir.join("bad.mid");
    std::fs::write(&bad_midi, b"MThd\x00\x00\x00\x06\x00\x01\x00\x01\x00\x00").unwrap();
    let missing = dir.join("missing.wav");
    let mut session = Session::new(blank_project());
    let out = run(
        &mut session,
        "clip.addFiles",
        json!({ "paths": [text(&broken), text(&good), text(&text_file), text(&bad_midi), text(&missing)], "track": 3, "start": 0.0 }),
    )
    .unwrap();
    assert_eq!(out["clips"].as_array().map(Vec::len), Some(1));
    assert_eq!(out["skipped"].as_array().map(Vec::len), Some(4), "{}", out["skipped"]);

    // When nothing can be placed, nothing changes.
    let before = session.project().clone();
    let err = run(&mut session, "clip.addFiles", json!({ "paths": [text(&broken), text(&missing)], "start": 0.0 }))
        .unwrap_err();
    assert!(err.contains("nothing could be placed"), "{err}");
    assert_eq!(session.project(), &before);
    for bad in [
        json!({ "paths": [] , "start": 0.0 }),
        json!({ "paths": "a.wav", "start": 0.0 }),
        json!({ "paths": [text(&good)] }),
        json!({ "paths": [text(&good)], "start": 0.0, "scene": 1 }),
        json!({ "paths": [text(&good)], "start": -1.0 }),
        json!({ "paths": [text(&good)], "start": 1e12 }),
        json!({ "paths": [text(&good)], "scene": 0 }),
        json!({ "paths": [text(&good)], "scene": 1_000_000 }),
        json!({ "paths": [text(&good)], "start": 0.0, "track": "A Return" }),
        json!({ "paths": [text(&good)], "start": 0.0, "warp": "elastic" }),
        json!({ "paths": [1, 2], "start": 0.0 }),
    ] {
        assert!(run(&mut session, "clip.addFiles", bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(session.project(), &before);

    // More files than the limit: the rest are skipped, not read.
    let many: Vec<String> = (0..300).map(|_| text(&good)).collect();
    let out = run(&mut session, "clip.addFiles", json!({ "paths": many, "start": 0.0, "stack": true })).unwrap();
    assert_eq!(out["clips"].as_array().map(Vec::len), Some(256));
    assert_eq!(out["skipped"].as_array().map(Vec::len), Some(44));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_sample_added_while_playing_reaches_the_engine() {
    let dir = temp_dir("playing");
    let sample = wav(&dir, "late.wav", 2.0, 0.5);
    let project = blank_project();
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let mut session = Session::new(project);
    session.attach_player(Player::new(renderer.controller(), None));
    run(&mut session, "clip.addFiles", json!({ "paths": [text(&sample)], "track": 3, "start": 0.0 })).unwrap();
    run(&mut session, "transport.play", json!({})).unwrap();
    let mut out = vec![0.0f32; 4096];
    let mut heard = 0.0f32;
    for _ in 0..20 {
        renderer.process(&mut out);
        heard = heard.max(out.iter().fold(0.0, |m, s| m.max(s.abs())));
    }
    assert!(heard > 0.1, "the new graph played the new file: {heard}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn audio_provided_ahead_is_used_instead_of_decoding_again() {
    let dir = temp_dir("provided");
    let sample = wav(&dir, "pre.wav", 0.5, 0.5);
    let mut session = Session::new(blank_project());
    // What the window decoded on its own thread: a different level, so it shows which was used.
    let data =
        std::sync::Arc::new(suhina_engine::AudioData { sample_rate: SR, channels: 1, samples: vec![0.125; 24_000] });
    session.provide_audio(&sample, std::sync::Arc::clone(&data));
    run(&mut session, "clip.addFiles", json!({ "paths": [text(&sample)], "track": 3, "start": 0.0 })).unwrap();
    let key = text(&std::path::absolute(&sample).unwrap());
    assert!(std::sync::Arc::ptr_eq(&session.audio().files[&key], &data));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_preset_adds_the_fm_synth_with_its_voice() {
    use suhina_device_fm_synth::patch::{Patch, write_bank};
    let dir = temp_dir("preset");
    let mut patch = Patch::default();
    patch.set_name("TUBULAR");
    patch.set(134, 4);
    let bank = dir.join("bank.syx");
    std::fs::write(&bank, write_bank(&vec![patch; 32])).unwrap();
    let mut session = Session::new(blank_project());
    let out = run(&mut session, "device.add", json!({ "track": 1, "preset": text(&bank), "voice": 32 })).unwrap();
    assert_eq!(out["preset"], "TUBULAR");
    let channel = session.project().channel(ChannelId(1)).unwrap();
    let device = &channel.devices[0];
    assert_eq!(device.kind, DeviceKind::Builtin("fm-synth".into()));
    assert!(!device.params.is_empty());

    // As a new track.
    let out = run(&mut session, "track.add", json!({ "preset": text(&bank) })).unwrap();
    assert_eq!((out["kind"].as_str(), out["name"].as_str()), (Some("midi"), Some("bank")));

    for bad in [
        json!({ "track": 2, "preset": text(&bank), "voice": 33 }),
        json!({ "track": 2, "preset": text(&bank), "voice": 0 }),
        json!({ "track": 2, "preset": text(&bank), "id": "overdrive" }),
        json!({ "track": 2, "preset": text(&dir.join("none.syx")) }),
        json!({ "track": 3, "preset": text(&bank) }),
        json!({ "track": 3, "id": "fm-synth" }),
        json!({ "track": 2 }),
    ] {
        assert!(run(&mut session, "device.add", bad.clone()).is_err(), "{bad}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn previews_need_a_player_and_show_in_the_launcher_state() {
    let dir = temp_dir("preview");
    let sample = wav(&dir, "listen.wav", 1.0, 0.5);
    let m = midi(&dir, "tune.mid", &[("", &[(0, 96, 60)])]);
    let project = blank_project();
    let mut session = Session::new(project.clone());
    assert!(run(&mut session, "preview.play", json!({ "path": text(&sample) })).is_err(), "no engine");
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    session.attach_player(Player::new(renderer.controller(), None));
    let out = run(&mut session, "preview.play", json!({ "path": text(&sample), "gain": 0.0 })).unwrap();
    assert_eq!(out["seconds"], 1.0);
    let mut block = vec![0.0f32; 1024];
    renderer.process(&mut block);
    assert!(block[600].abs() > 0.4, "{}", block[600]);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["preview"]["playing"], true);
    assert!(session.audio().files.is_empty(), "a preview does not join the project's audio");

    // A new level for the preview playing: it keeps playing, quieter.
    run(&mut session, "preview.setGain", json!({ "gain": -12.0 })).unwrap();
    for _ in 0..8 {
        renderer.process(&mut block);
    }
    assert!(block[600].abs() < 0.2 && block[600].abs() > 0.05, "{}", block[600]);
    assert_eq!(run(&mut session, "launcher.state", json!({})).unwrap()["preview"]["playing"], true);
    for bad in [json!({}), json!({ "gain": 100.0 }), json!({ "gain": "loud" })] {
        assert!(run(&mut session, "preview.setGain", bad.clone()).is_err(), "{bad}");
    }

    run(&mut session, "preview.play", json!({ "path": text(&m), "track": 1 })).unwrap();
    run(&mut session, "preview.stop", json!({})).unwrap();
    renderer.process(&mut block);
    assert_eq!(run(&mut session, "launcher.state", json!({})).unwrap()["preview"]["playing"], false);
    for bad in [
        json!({}),
        json!({ "path": text(&dir.join("nothing.wav")) }),
        json!({ "path": text(&dir) }),
        json!({ "path": text(&sample), "from": -1.0 }),
        json!({ "path": text(&sample), "gain": 100.0 }),
        json!({ "path": text(&sample), "track": "Nobody" }),
    ] {
        assert!(run(&mut session, "preview.play", bad.clone()).is_err(), "{bad}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
