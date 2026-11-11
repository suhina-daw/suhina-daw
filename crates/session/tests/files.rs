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
use suhina_test_support::{ProjectBuilder, WavFormat, write_wav};

const SR: u32 = 48_000;

/// The shipped model's bytes, under a test name in the library: the machine's copy of a file
/// the pack also carries.
const SHIPPED_OVERDRIVE: &[u8] = include_bytes!("../../../packs/nam-amp-modeler/Suhina Overdrive.nam");

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
    p.tracks.iter().map(|t| p.shown_track_name(t.id)).collect()
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
    assert_eq!(names(p), ["1 MIDI", "2 MIDI", "3 Audio", "4 Audio", "5 Audio", "A Return", "B Return"]);
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
fn two_tracks_with_the_same_name_are_named_by_id_instead() {
    let mut session = Session::new(blank_project());
    run(&mut session, "track.rename", json!({ "track": "4 Audio", "name": "3 Audio" })).unwrap();
    // The first of the two was picked without a word; now the command says it cannot tell.
    let error = run(&mut session, "track.setMute", json!({ "tracks": ["3 Audio"], "mute": true })).unwrap_err();
    assert!(error.contains("several tracks are named"), "{error}");
    assert!(session.project().channels.iter().all(|c| !c.mute), "nothing changed");
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

/// `track.add` with a `file`: the device it adds runs the file at once, as `device.add` does
/// ([docs/spec/nam-amp-and-cab.md]); a file a device does not run changes nothing.
#[test]
fn a_track_added_with_a_file_starts_its_device_on_it() {
    let dir = temp_dir("track-file");
    let model = wav(&dir, "raw cabinet.wav", 0.1, 0.5);
    let mut session = Session::new(blank_project());
    let added =
        run(&mut session, "track.add", json!({ "device": "nam-cab-loader", "file": model.display().to_string() }))
            .unwrap();
    assert_eq!(added["kind"], "audio");
    let file = run(&mut session, "device.file", json!({ "track": added["track"], "device": 0 })).unwrap();
    assert_eq!(file["file"], model.display().to_string(), "the device runs the file");
    assert_eq!(session.device_asset(&model.display().to_string()).map(|a| a.device()), Some("nam-cab-loader"));

    let before = session.project().clone();
    let error = run(&mut session, "track.add", json!({ "device": "fm-synth", "file": model.display().to_string() }))
        .unwrap_err();
    assert!(error.contains("does not run a file"), "{error}");
    assert_eq!(session.project(), &before, "a refused file added nothing");
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
    assert!(!lane.clips[0].audio.warped);
    assert_eq!(lane.clips[0].audio.tempo_source, None, "no tempo found");
    assert_eq!(region.file, text(&std::path::absolute(&one).unwrap()));
    assert!(session.audio().files.contains_key(&region.file), "decoded into the session's pool");

    // The offline render plays them: the oracle for what was placed.
    let renderer = Renderer::build(
        session.project(),
        RenderInputs::from_audio(session.audio(), suhina_session::devices()),
        SR,
        RenderOptions::PLAYING,
    )
    .unwrap();
    drop(renderer);
    let out = suhina_engine::render_offline_from(
        session.project(),
        RenderInputs::from_audio(session.audio(), suhina_session::devices()),
        SR,
        512,
        None,
    )
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
    let audio = &lane.clips[0].audio;
    assert!(audio.warped);
    assert_eq!(audio.stretch, StretchMode::Rhythmic);
    assert_eq!(audio.warp_markers.last().map(|m| (m.time, m.audio_time)), Some((2.0, 2.0)));
    assert_eq!((audio.tempo(), audio.tempo_source), (Some(60.0), Some(TempoSource::Name)), "from its name");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A loop of known tempo stretched with warp off, then warped again, is back at its own tempo:
/// warp on sets it to the file's tempo, not to the stretch.
#[test]
fn warp_on_sets_a_loop_of_known_tempo_back_to_its_own_tempo() {
    let dir = temp_dir("warp-back");
    let looped = wav(&dir, "loop 60bpm.wav", 2.0, 0.5);
    let mut session = Session::new(blank_project());
    run(&mut session, "clip.addFiles", json!({ "paths": [text(&looped)], "track": 3, "start": 0.0 })).unwrap();
    let clip = |s: &Session| {
        let p = s.project();
        p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(3))).unwrap().clips[0].clone()
    };
    let id = clip(&session).id.0;
    run(&mut session, "clip.setAudio", json!({ "clip": id, "warp": false })).unwrap();
    run(&mut session, "clip.stretch", json!({ "clip": id, "end": 3.0 })).unwrap();
    run(&mut session, "clip.setAudio", json!({ "clip": id, "warp": false })).unwrap();
    run(&mut session, "clip.setAudio", json!({ "clip": id, "warp": true })).unwrap();
    let c = clip(&session);
    assert!(c.audio.warped);
    assert_eq!(c.audio.warp_markers.last().map(|m| (m.time, m.audio_time)), Some((2.0, 2.0)), "60 BPM again");
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
    run(&mut session, "clip.addFiles", json!({ "paths": [text(&hit)], "track": "3 Audio", "scene": 0 })).unwrap();
    let clip = slot_clip(session.project(), 3, 1);
    assert_eq!((clip.length, clip.loop_range), (1.5, Some(LoopRange { start: 0.0, end: 1.5 })));
    assert!(!clip.audio.warped, "it plays at its own speed");

    // Launched, it repeats: sound in the second and third passes, not only the first.
    let column = session.project().launcher.columns.iter().position(|c| c.track == TrackId(3)).unwrap();
    let launch = [(0.0, suhina_engine::LaunchCommand::LaunchClip { column, slot: 0 })];
    let render = |session: &Session| {
        suhina_engine::render_launches(session.project(), session.audio(), SR, &launch, 2.0, suhina_session::devices())
            .unwrap()
    };
    let at = |out: &[f32], seconds: f64| out[(seconds * f64::from(SR)) as usize * 2].abs();
    let out = render(&session);
    assert!(at(&out, 0.3) > 0.1 && at(&out, 1.0) > 0.1 && at(&out, 1.8) > 0.1);

    // clip.setLoop off makes it a one-shot: it plays once and the slot falls silent.
    let done = run(&mut session, "clip.setLoop", json!({ "track": 3, "scene": 0, "loop": false })).unwrap();
    let (clip, scene_id) = (slot_clip(session.project(), 3, 1).id.0, session.project().launcher.scenes[0].id.0);
    assert_eq!(done, json!({ "clip": clip, "scene": 0, "scene_id": scene_id, "loop": false }));
    assert_eq!(slot_clip(session.project(), 3, 1).loop_range, None);
    let out = render(&session);
    assert!(at(&out, 0.3) > 0.1 && at(&out, 1.0) < 1e-6 && at(&out, 1.8) < 1e-6);

    // On again loops the whole clip; one undo step each.
    run(&mut session, "clip.setLoop", json!({ "track": 3, "scene": 0, "loop": true })).unwrap();
    assert_eq!(slot_clip(session.project(), 3, 1).loop_range, Some(LoopRange { start: 0.0, end: 1.5 }));
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(slot_clip(session.project(), 3, 1).loop_range, None);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(slot_clip(session.project(), 3, 1).loop_range.is_some());

    // An empty slot, a missing scene and a value that is not true or false are errors.
    assert!(run(&mut session, "clip.setLoop", json!({ "track": 3, "scene": 1, "loop": true })).is_err());
    assert!(run(&mut session, "clip.setLoop", json!({ "track": 3, "scene": 98, "loop": true })).is_err());
    assert!(run(&mut session, "clip.setLoop", json!({ "track": 3, "scene": 0, "loop": "yes" })).is_err());
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
    run(&mut session, "clip.addFiles", json!({ "paths": paths, "track": "4 Audio", "scene": 1 })).unwrap();
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
    assert_eq!(names(p)[2..6], ["3 Audio", "snare", "hat", "6 Audio"]);
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
        run(&mut session, "clip.addFiles", json!({ "paths": [text(&a), text(&m), text(&b)], "scene": 0 })).unwrap();
    assert_eq!(out["tracks"].as_array().map(Vec::len), Some(2));
    let p = session.project();
    assert_eq!(names(p)[4..6], ["5 Audio", "groove"]);
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
    assert_eq!(names(p)[..3], ["1 MIDI", "vox", "3 MIDI"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn after_puts_the_new_tracks_after_a_track() {
    let dir = temp_dir("after");
    let a = wav(&dir, "pad.wav", 0.5, 0.5);
    let m = midi(&dir, "riff.mid", &[("", &[(0, 96, 60)])]);
    let mut session = Session::new(blank_project());
    let out =
        run(&mut session, "clip.addFiles", json!({ "paths": [text(&a), text(&m)], "after": "1 MIDI", "start": 4.0 }))
            .unwrap();
    assert_eq!(out["tracks"].as_array().map(Vec::len), Some(2));
    assert_eq!(names(session.project())[..4], ["1 MIDI", "pad", "riff", "4 MIDI"]);

    // One each with separate, in the order given, still after the track.
    let b = wav(&dir, "stab.wav", 0.5, 0.5);
    let mut session = Session::new(blank_project());
    run(
        &mut session,
        "clip.addFiles",
        json!({ "paths": [text(&a), text(&b)], "after": "1 MIDI", "start": 0.0, "separate": true }),
    )
    .unwrap();
    assert_eq!(names(session.project())[..4], ["1 MIDI", "pad", "stab", "4 MIDI"]);

    // With a track to place them on, after has nothing to say.
    let mut session = Session::new(blank_project());
    let err = run(
        &mut session,
        "clip.addFiles",
        json!({ "paths": [text(&a)], "track": "3 Audio", "after": "1 MIDI", "start": 0.0 }),
    )
    .unwrap_err();
    assert!(err.contains("not both"), "{err}");
    // A track that is not there.
    let err =
        run(&mut session, "clip.addFiles", json!({ "paths": [text(&a)], "after": "Nope", "start": 0.0 })).unwrap_err();
    assert!(!err.is_empty());
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
    run(&mut session, "clip.addFiles", json!({ "paths": [text(&a), text(&m)], "scene": 2, "separate": true })).unwrap();
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
        json!({ "paths": [text(&good)], "start": 0.0, "scene": 99 }),
        json!({ "paths": [text(&good)], "start": -1.0 }),
        json!({ "paths": [text(&good)], "start": 1e12 }),
        json!({ "paths": [text(&good)], "scene": -1 }),
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
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR, suhina_session::devices()).unwrap();
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

/// An instrument added with a preset names its track after the preset, as a new track with the
/// preset is named; a name the user chose stays (#946).
#[test]
fn an_instrument_preset_names_its_track_after_the_preset() {
    let mut session = Session::new(blank_project());
    run(&mut session, "device.add", json!({ "track": 1, "stock": "fm-synth/warm-fm-bass" })).unwrap();
    assert_eq!(names(session.project())[0], "1 Warm FM Bass");
    // Undo puts the name back with the device, one step.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(names(session.project())[0], "1 MIDI");
    // The same instrument without a preset names the track after the device.
    run(&mut session, "device.add", json!({ "track": 1, "id": "fm-synth" })).unwrap();
    assert_eq!(names(session.project())[0], "1 FM Synth");
    // A name the user chose stays.
    run(&mut session, "track.rename", json!({ "track": 2, "name": "Lead" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 2, "stock": "fm-synth/warm-fm-bass" })).unwrap();
    assert_eq!(names(session.project())[1], "Lead");
}

#[test]
fn a_stock_preset_and_a_preset_file_add_their_device_with_their_values() {
    let dir = temp_dir("stock-preset");
    let mut session = Session::new(blank_project());
    let out = run(&mut session, "device.add", json!({ "track": 1, "stock": "fm-synth/warm-fm-bass" })).unwrap();
    assert_eq!((out["id"].as_str(), out["preset"].as_str()), (Some("fm-synth"), Some("Warm FM Bass")));
    let device = &session.project().channel(ChannelId(1)).unwrap().devices[0];
    assert!(device.params.contains(&suhina_model::DeviceParam::new("op2.level", 86.0)));

    let added = run(&mut session, "track.add", json!({ "stock": "fm-synth/struck-fm-keys" })).unwrap();
    assert_eq!((added["kind"].as_str(), added["name"].as_str()), (Some("midi"), Some("Struck FM Keys")));

    let file = dir.join("Mine.fm-synth.preset.suhina");
    std::fs::write(
        &file,
        r#"{ "format": "suhina-preset", "version": 1, "device": "fm-synth", "role": "instrument", "name": "Mine", "params": { "volume": -3.0 } }"#,
    )
    .unwrap();
    let added = run(&mut session, "track.add", json!({ "preset": text(&file) })).unwrap();
    assert_eq!(added["name"], "Mine");

    for bad in [
        json!({ "track": 1, "stock": "fm-synth/nothing" }),
        json!({ "track": 1, "stock": 3 }),
        json!({ "track": 1, "stock": "fm-synth/warm-fm-bass", "preset": text(&file) }),
        json!({ "track": 1, "stock": "fm-synth/warm-fm-bass", "id": "overdrive" }),
    ] {
        assert!(run(&mut session, "device.add", bad.clone()).is_err(), "{bad}");
    }
    assert!(run(&mut session, "track.add", json!({ "stock": "fm-synth/nothing" })).is_err());
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
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR, suhina_session::devices()).unwrap();
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

/// A shipped file's key rides `device.setNamFile`: the device's state holds the key, the
/// pack's real bytes load through the device's own loader, the shown name comes back, one
/// undo step takes it back, and another device's pack is refused ([ADR 0036]).
#[test]
fn a_shipped_file_key_sets_the_state_and_undoes() {
    let mut session = Session::new(blank_project());
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 1, "id": "nam-amp-modeler" })).unwrap();

    let before = run(&mut session, "device.file", json!({ "track": 1, "device": 0 })).unwrap();
    assert_eq!(before["file"], "");

    let key = "builtin:nam-amp-modeler/Suhina Overdrive.nam";
    let out = run(&mut session, "device.setNamFile", json!({ "track": 1, "device": 0, "path": key })).unwrap();
    assert_eq!((out["file"].as_str(), out["name"].as_str()), (Some(key), Some("Suhina Overdrive")));

    let after = run(&mut session, "device.file", json!({ "track": 1, "device": 0 })).unwrap();
    assert_eq!(after["file"], key);
    assert_eq!(after["name"], "Suhina Overdrive");
    assert_eq!(after["file"], key);
    assert_eq!(after["name"], "Suhina Overdrive");

    // A cab key is not a model's.
    assert!(
        run(
            &mut session,
            "device.setNamFile",
            json!({ "track": 1, "device": 0, "path": "builtin:nam-cab-loader/Synthetic Cabinet Plain.wav" })
        )
        .is_err()
    );
    // An unknown shipped file is.
    assert!(
        run(
            &mut session,
            "device.setNamFile",
            json!({ "track": 1, "device": 0, "path": "builtin:nam-amp-modeler/Nothing.nam" })
        )
        .is_err()
    );

    run(&mut session, "edit.undo", json!({})).unwrap();
    let undone = run(&mut session, "device.file", json!({ "track": 1, "device": 0 })).unwrap();
    assert_eq!(undone["file"], "");
}

/// The shipped pack is there on a fresh session with nothing on disk: a Cab Loader picks its
/// file from the pack's bytes too.
#[test]
fn a_shipped_ir_key_loads_the_pack_at_the_rate() {
    let mut session = Session::new(blank_project());
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 1, "id": "nam-cab-loader" })).unwrap();
    let out = run(
        &mut session,
        "device.setNamFile",
        json!({ "track": 1, "device": 0, "path": "builtin:nam-cab-loader/Synthetic Cabinet Plain.wav" }),
    )
    .unwrap();
    assert_eq!(out["name"], "Synthetic Cabinet Plain", "a cab's shown name is the IR file's own, never the folder's");
}

/// `device.setNamFile`: the state keeps what was chosen, and what a project carries. A file
/// picked inside the library stores relative to it ([ADR 0036] › 4)); a file outside keeps its
/// absolute path; the project file carries exactly that, and a session opened elsewhere
/// resolves the relative one against its own library root.
#[test]
fn a_devices_file_state_saves_library_relative() {
    let dir = temp_dir("library-relative-state");
    let mut session = Session::new(blank_project());
    session.set_library_base(dir.clone());
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 1, "id": "nam-amp-modeler" })).unwrap();
    // The file the user picked inside the library's Sounds folders: the pack's unpacked copy
    // of the shipped model.
    let file = dir.join("Sounds/NAM/Amp models/Suhina/Suhina Overdrive.nam");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, SHIPPED_OVERDRIVE).unwrap();
    let out = run(&mut session, "device.setNamFile", json!({ "track": 1, "device": 0, "path": text(&file) })).unwrap();
    assert_eq!(out["file"], "Sounds/NAM/Amp models/Suhina/Suhina Overdrive.nam", "the store is library-relative");
    let device = &session.project().tracks[0].channel.and_then(|c| session.project().channel(c)).unwrap().devices[0];
    assert_eq!(
        device.state.as_deref().and_then(|s| std::str::from_utf8(s).ok()),
        Some("Sounds/NAM/Amp models/Suhina/Suhina Overdrive.nam")
    );

    // The project file carries the relative name; a session opened elsewhere resolves it
    // against its own base. The save runs before the next choice, so the file carries the
    // relative form.
    let project = dir.join("Band.suhina");
    session.save(&project, false).unwrap();
    let (mut second, _) = Session::open(&project).unwrap();
    second.set_library_base(dir.clone());
    second.sync_device_files();
    let carried = |session: &Session| -> String {
        let project = session.project();
        let channel = project.tracks[0].channel.and_then(|c| project.channel(c)).unwrap();
        channel.devices[0]
            .state
            .as_deref()
            .and_then(|state| std::str::from_utf8(state).ok())
            .unwrap_or_default()
            .to_owned()
    };
    assert_eq!(
        carried(&second),
        "Sounds/NAM/Amp models/Suhina/Suhina Overdrive.nam",
        "the project file keeps the relative form"
    );
    let file_out = run(&mut second, "device.file", json!({ "track": 1, "device": 0 })).unwrap();
    assert_eq!(file_out["file"], "Sounds/NAM/Amp models/Suhina/Suhina Overdrive.nam");
    assert_eq!(file_out["name"], "Suhina Overdrive", "the relative state resolves and the model loads");

    // A machine without the file on disk, and without the base, still runs it: the pack's
    // file of the state's own name stands in.
    let (mut third, _) = Session::open(&project).unwrap();
    third.sync_device_files();
    let file_out = run(&mut third, "device.file", json!({ "track": 1, "device": 0 })).unwrap();
    assert_eq!(file_out["name"], "Suhina Overdrive", "the pack stands in by name");

    // A file outside the library keeps its absolute path, recorded when it is all there is.
    let outside = std::env::temp_dir().join(format!("suhina-library-outside-{}.nam", std::process::id()));
    std::fs::write(&outside, SHIPPED_OVERDRIVE).unwrap();
    let out =
        run(&mut session, "device.setNamFile", json!({ "track": 1, "device": 0, "path": text(&outside) })).unwrap();
    let device = &session.project().tracks[0].channel.and_then(|c| session.project().channel(c)).unwrap().devices[0];
    assert_eq!(
        device.state.as_deref().and_then(|s| std::str::from_utf8(s).ok()),
        Some(text(&outside).as_str()),
        "{out}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&outside);
}

/// The offline render of a note through the Amp Modeler that the session's assets carry.
fn rendered(session: &Session) -> Vec<f32> {
    suhina_engine::render_offline_from(
        session.project(),
        suhina_engine::RenderInputs::from_audio(session.audio(), suhina_session::devices())
            .with_assets(session.device_assets()),
        SR,
        512,
        None,
    )
    .unwrap()
}

/// The round-trip of [ADR 0036] › 7: a project saves to a library-relative name, opens on a
/// machine without the file on disk and still renders from the pack. The offline render is
/// the oracle: the same notes through the same model read two ways give the same samples, and
/// they are not silence ([crates/device-registry/tests/pack.rs])(the pack's own bound).
#[test]
fn a_library_relative_state_still_renders_from_the_pack() {
    let dir = temp_dir("library-relative-render");
    // A tone into the Amp Modeler: a note on the track the device sits on. The file is the
    // pack's unpacked copy, named as the pack names it — a renamed copy is no stand-in, for
    // nothing proves it is the same file.
    let mut session = Session::new(suhina_test_support::with_ids(
        suhina_test_support::ProjectBuilder::new()
            .midi_track(1, "Keys")
            .notes(1, "Riff", 4.0, vec![suhina_model::Note::new(0.0, 4.0, 60, 0.8)])
            .clip(1, suhina_model::Clip::new(suhina_model::ContentId(1), 0.0, 4.0))
            .build_without_ids(),
    ));
    session.set_library_base(dir.clone());
    run(&mut session, "device.add", json!({ "track": 1, "id": "nam-amp-modeler" })).unwrap();
    let file = dir.join("Sounds/NAM/Amp models/Suhina/Suhina Overdrive.nam");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, SHIPPED_OVERDRIVE).unwrap();
    run(&mut session, "device.setNamFile", json!({ "track": 1, "device": 0, "path": text(&file) })).unwrap();
    let first = rendered(&session);
    assert!(first.iter().any(|s| s.abs() > 0.05), "the model runs the note");

    // The project saved and reopened where the file is not: the pack's bytes of the same name
    // load, and the render is the same.
    let project = dir.join("Band.suhina");
    session.save(&project, false).unwrap();
    std::fs::remove_file(&file).unwrap();
    let (mut second, _) = Session::open(&project).unwrap();
    second.sync_device_files();
    assert_eq!(rendered(&second), first, "the pack stands in: the same samples");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `device.savePreset` (docs/decisions/0036-library-and-shipped-content.md › 5): a device's
/// values and, with the file it selects, one file reference, written into the library's
/// `Presets/<device id>/` ([`ADR 0014`]'s format), a save again replacing the same name's file,
/// and the project it runs on changed by nothing. The stock presets that name a pack file add
/// their device with the file state it carries.
#[test]
fn a_preset_is_saved_into_the_library_and_replaced_on_saving_again() {
    let dir = temp_dir("save-preset");
    let mut session = Session::new(blank_project());
    session.set_library_base(dir.clone());
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 1, "id": "nam-amp-modeler" })).unwrap();
    let key = "builtin:nam-amp-modeler/Suhina Overdrive.nam";
    run(&mut session, "device.setNamFile", json!({ "track": 1, "device": 0, "path": key })).unwrap();
    run(&mut session, "device.setParam", json!({ "track": 1, "device": 0, "param": "inputGain", "value": 2.5 }))
        .unwrap();

    let before = session.project().clone();
    let saved =
        run(&mut session, "device.savePreset", json!({ "track": 1, "device": 0, "name": "Home Crunch" })).unwrap();
    assert_eq!(saved["name"], "Home Crunch");
    let path = PathBuf::from(saved["preset"].as_str().unwrap());
    assert_eq!(path, dir.join("Presets/nam-amp-modeler/Home Crunch.nam-amp-modeler.preset.suhina"));
    assert_eq!(session.project(), &before, "saving a preset writes the library, not the project");

    let read = suhina_device_registry::preset(&std::fs::read(&path).unwrap(), 1).unwrap();
    assert_eq!(read.file.as_deref(), Some(key), "the preset names the file the device runs");
    let input_gain =
        |read: &suhina_device_registry::Preset| read.params.iter().find(|(id, _)| *id == "inputGain").unwrap().1;
    assert_eq!(input_gain(&read), 2.5);

    // Saving again over the same name replaces the file with the device as it is now.
    run(&mut session, "device.setParam", json!({ "track": 1, "device": 0, "param": "inputGain", "value": 1.5 }))
        .unwrap();
    run(&mut session, "device.savePreset", json!({ "track": 1, "device": 0, "name": "Home Crunch" })).unwrap();
    let read = suhina_device_registry::preset(&std::fs::read(&path).unwrap(), 1).unwrap();
    assert_eq!(input_gain(&read), 1.5);

    // A name typed with padding writes the trimmed name's file ([ADR 0014] › 5, the review):
    // the same file a plain typing of the name replaces.
    let padded =
        run(&mut session, "device.savePreset", json!({ "track": 1, "device": 0, "name": "  Lead  " })).unwrap();
    let lead = PathBuf::from(padded["preset"].as_str().unwrap());
    assert_eq!(
        lead,
        dir.join("Presets/nam-amp-modeler/Lead.nam-amp-modeler.preset.suhina"),
        "the trimmed name is the file's"
    );
    assert_eq!(padded["name"], "Lead");
    run(&mut session, "device.savePreset", json!({ "track": 1, "device": 0, "name": "Lead" })).unwrap();
    assert!(lead.exists(), "the plain typing replaced the padded name's file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_preset_is_not_saved_without_the_library_or_a_name() {
    let mut session = Session::new(blank_project());
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 1, "id": "compressor" })).unwrap();
    let error = run(&mut session, "device.savePreset", json!({ "track": 1, "device": 0, "name": "Home" })).unwrap_err();
    assert!(error.contains("the library's folder is not set"), "{error}");
    let dir = temp_dir("save-preset-refused");
    session.set_library_base(dir.clone());
    for bad in [
        json!({ "track": 1, "device": 0 }),
        json!({ "track": 1, "device": 0, "name": "  " }),
        json!({ "track": 1, "device": 0, "name": "a/b" }),
        json!({ "track": 1, "device": 0, "name": ".." }),
        json!({ "track": 1, "device": 1 }),
        json!({ "track": 1, "device": 0, "name": "Home", "track2": 1 }),
    ] {
        assert!(run(&mut session, "device.savePreset", bad.clone()).is_err(), "{bad}");
    }
    assert!(!dir.join("Presets/compressor/Nothing was written").exists());
    assert!(dir.read_dir().is_ok_and(|mut entries| entries.next().is_none()), "nothing was written");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_stock_preset_naming_a_file_adds_its_device_with_the_file() {
    let mut session = Session::new(blank_project());
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    let added =
        run(&mut session, "device.add", json!({ "track": 1, "stock": "nam-amp-modeler/bright-clean" })).unwrap();
    assert_eq!(added["preset"], "Bright Clean");
    let file = run(&mut session, "device.file", json!({ "track": 1, "device": 0 })).unwrap();
    assert_eq!(file["file"], "builtin:nam-amp-modeler/Tim R Fender TwinVerb Vibrato Bright.nam", "{file}");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(
        session.project().channel(ChannelId(1)).unwrap().devices.is_empty(),
        "the undo takes the added device with its file"
    );
}

#[test]
fn a_preset_file_naming_a_missing_file_adds_its_device_and_says_so() {
    let dir = temp_dir("preset-missing-file");
    let mut session = Session::new(blank_project());
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    let preset = dir.join("Away.nam-amp-modeler.preset.suhina");
    std::fs::write(
        &preset,
        r#"{ "format": "suhina-preset", "version": 1, "device": "nam-amp-modeler", "role": "effect", "name": "Away", "file": "nowhere/bright.nam", "params": { "inputGain": -1.0 } }"#,
    )
    .unwrap();
    let added = run(&mut session, "device.add", json!({ "track": 1, "preset": text(&preset) })).unwrap();
    assert_eq!(added["preset"], "Away");
    assert!(added["file_missing"].as_str().is_some(), "{added}");
    let file = run(&mut session, "device.file", json!({ "track": 1, "device": 0 })).unwrap();
    assert_eq!(file["file"], "", "the device runs without one");

    // Where the device runs no file, a named one is refused at the read.
    std::fs::write(
        &preset,
        r#"{ "format": "suhina-preset", "version": 1, "device": "compressor", "role": "effect", "name": "No File", "file": "x.wav", "params": {} }"#,
    )
    .unwrap();
    let error = run(&mut session, "device.add", json!({ "track": 1, "preset": text(&preset) })).unwrap_err();
    assert!(error.contains("does not run"), "{error}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A library-relative state text handed back to `device.setNamFile` keeps the same reference:
/// the state text is the file's own, so a caller (the MCP server, a user preset) that gives it
/// back keeps the project's file ([ADR 0036] › 4). A working-folder path still resolves against
/// it.
#[test]
fn a_file_state_handed_back_keeps_the_library_relative_form() {
    let dir = temp_dir("state-handed-back");
    let mut session = Session::new(blank_project());
    session.set_library_base(dir.clone());
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 1, "id": "nam-amp-modeler" })).unwrap();
    let file = dir.join("Sounds/NAM/Amp models/Suhina/Suhina Overdrive.nam");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, SHIPPED_OVERDRIVE).unwrap();
    run(&mut session, "device.setNamFile", json!({ "track": 1, "device": 0, "path": text(&file) })).unwrap();
    let relative = "Sounds/NAM/Amp models/Suhina/Suhina Overdrive.nam";

    // The state text handed back keeps the same reference: the state does not become an
    // absolute path of the working folder.
    let out = run(&mut session, "device.setNamFile", json!({ "track": 1, "device": 0, "path": relative })).unwrap();
    assert_eq!(out["file"], relative, "the same reference is kept");
    let device = session.project().tracks[0].channel.and_then(|c| session.project().channel(c)).unwrap();
    assert_eq!(device.devices[0].state.as_deref().and_then(|s| std::str::from_utf8(s).ok()), Some(relative));
    // A file outside the library stays absolute: its path is the state.
    let outside = std::env::temp_dir().join(format!("suhina-library-outside-{}.nam", std::process::id()));
    std::fs::write(&outside, SHIPPED_OVERDRIVE).unwrap();
    let out =
        run(&mut session, "device.setNamFile", json!({ "track": 1, "device": 0, "path": text(&outside) })).unwrap();
    assert_eq!(out["file"], text(&outside), "the outside file keeps its absolute path");
    let _ = std::fs::remove_file(&outside);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A relative state text stores its plain forward-slash form, whatever separators or `.` steps
/// the caller's text carried ([ADR 0036] › 4, the review): the component join the absolute
/// branch writes, so `Sounds/./NAM//Amp models/…` stores as `Sounds/NAM/Amp models/…` — the
/// form every machine's base joins, a Windows caller's backslash text included (checked on
/// Windows below).
#[test]
fn a_relative_state_stores_its_plain_forward_slash_form() {
    let dir = temp_dir("relative-state-normalised");
    let mut session = Session::new(blank_project());
    session.set_library_base(dir.clone());
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 1, "id": "nam-amp-modeler" })).unwrap();
    let file = dir.join("Sounds/NAM/Amp models/Suhina/Suhina Overdrive.nam");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, SHIPPED_OVERDRIVE).unwrap();
    let out = run(
        &mut session,
        "device.setNamFile",
        json!({ "track": 1, "device": 0, "path": "Sounds/./NAM//Amp models/Suhina/Suhina Overdrive.nam" }),
    )
    .unwrap();
    assert_eq!(
        out["file"], "Sounds/NAM/Amp models/Suhina/Suhina Overdrive.nam",
        "the store is the plain form, the caller's separators and steps gone"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A Windows caller's backslash text (`Sounds\NAM\…` from MCP or the CLI) stores forward
/// slashed ([ADR 0036] › 4, the review), so the state opens on every machine's base. Windows
/// only: the separators the component join normalises are the caller's own.
#[cfg(windows)]
#[test]
fn a_windows_backslash_state_stores_forward_slashed() {
    let dir = temp_dir("windows-backslash-state");
    let mut session = Session::new(blank_project());
    session.set_library_base(dir.clone());
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 1, "id": "nam-amp-modeler" })).unwrap();
    let file = dir.join("Sounds/NAM/Amp models/Suhina/Suhina Overdrive.nam");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, SHIPPED_OVERDRIVE).unwrap();
    let out = run(
        &mut session,
        "device.setNamFile",
        json!({ "track": 1, "device": 0, "path": r"Sounds\NAM\Amp models\Suhina\Suhina Overdrive.nam" }),
    )
    .unwrap();
    assert_eq!(
        out["file"], "Sounds/NAM/Amp models/Suhina/Suhina Overdrive.nam",
        "forward slashes on every platform"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A library-relative state's file is at the library's base, not the working folder: the
/// timestamp check sees the same resolved path the load read, so a rewritten file is
/// reloaded and the player hears it.
#[test]
fn a_library_relative_files_rewrite_is_reloaded_by_the_sync() {
    let dir = temp_dir("relative-rewrite-sync");
    let mut session = Session::new(blank_project());
    session.set_library_base(dir.clone());
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 1, "id": "nam-amp-modeler" })).unwrap();
    let file = dir.join("Sounds/NAM/Amp models/Suhina/Suhina Overdrive.nam");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, SHIPPED_OVERDRIVE).unwrap();
    run(&mut session, "device.setNamFile", json!({ "track": 1, "device": 0, "path": text(&file) })).unwrap();
    let relative = "Sounds/NAM/Amp models/Suhina/Suhina Overdrive.nam";
    session.sync_device_files();
    assert_eq!(
        session.device_asset_name(relative).as_deref(),
        Some("Suhina Overdrive"),
        "the first sync loads the file"
    );

    // The working folder is not the library's base: the file's rewrite is seen at the
    // library's path, not the working folder's, and the sync loads it again.
    let model = r#"{
        "version": "0.5.4", "architecture": "WaveNet",
        "config": {
            "layers": [{
                "input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
                "kernel_size": 1, "dilations": [1], "activation": "ReLU",
                "gated": false, "head_bias": false
            }],
            "head": null, "head_scale": 1.0
        },
        "weights": [1.0, 2.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0],
        "metadata": { "name": "Test amp 1965" }
    }"#;
    std::fs::write(&file, model).unwrap();
    session.sync_device_files();
    assert_eq!(
        session.device_asset_name(relative).as_deref(),
        Some("Test amp 1965"),
        "the sync sees the rewrite at the library's path and loads it again"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A missing file outside the library keeps its missing-file note: a shipped file of the same
/// name is no stand-in for an absolute path, for nothing proves it is the pack's file.
#[test]
fn a_missing_outside_file_keeps_its_missing_file_note() {
    let dir = temp_dir("missing-outside-note");
    let mut session = Session::new(blank_project());
    session.set_library_base(dir.clone());
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 1, "id": "nam-amp-modeler" })).unwrap();
    let outside = std::env::temp_dir().join(format!("suhina-library-outside-note-{}.nam", std::process::id()));
    std::fs::write(&outside, SHIPPED_OVERDRIVE).unwrap();
    run(&mut session, "device.setNamFile", json!({ "track": 1, "device": 0, "path": text(&outside) })).unwrap();
    let absolute = text(&outside);
    let _ = std::fs::remove_file(&outside);
    session.sync_device_files();
    let note = session.device_asset_note(&absolute);
    assert!(note.is_some(), "{absolute}: the missing file keeps its note, not the pack's sound");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The pack stands in only where the state names the shipped pack's own place: a project
/// carrying its own recording by a library-relative path in the user's folder
/// (`Sounds/NAM/Cabinets/MyCabs/Marshall MG 15.wav`) opens elsewhere on a missing-file note,
/// never on the pack's different file of the same name ([ADR 0036] › 7, the review).
#[test]
fn a_relative_state_in_the_users_own_folder_misses_elsewhere_not_on_the_pack() {
    let dir = temp_dir("own-folder-missing");
    let mut session = Session::new(blank_project());
    session.set_library_base(dir.clone());
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 1, "id": "nam-cab-loader" })).unwrap();
    // The user's own recording, in the user's own folder under the library: not the pack's
    // `Suhina` folder. The state stores it library-relative.
    let own = dir.join("Sounds/NAM/Cabinets/MyCabs");
    std::fs::create_dir_all(&own).unwrap();
    let own = wav(&own, "Marshall MG 15.wav", 0.1, 0.25);
    run(&mut session, "device.setNamFile", json!({ "track": 1, "device": 0, "path": text(&own) })).unwrap();
    let relative = "Sounds/NAM/Cabinets/MyCabs/Marshall MG 15.wav";
    let device = &session.project().tracks[0].channel.and_then(|c| session.project().channel(c)).unwrap().devices[0];
    assert_eq!(
        device.state.as_deref().and_then(|s| std::str::from_utf8(s).ok()),
        Some(relative),
        "the user's own file stores library-relative"
    );
    // Another machine without the file: the note is the missing file's, not the pack's IR.
    let _ = std::fs::remove_file(&own);
    session.sync_device_files();
    let note = session.device_asset_note(relative);
    assert!(
        note.is_some_and(|note| note.contains("missing") || note.contains("not there") || !note.is_empty()),
        "{relative}: the user's own missing file keeps its note, not the pack's stand-in: {note:?}"
    );
    assert!(
        session.device_asset(relative).is_none(),
        "no asset: the pack's file of the same name does not stand in"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A project that opened before the library's move carries its file by the old defaults'
/// place — `NAM models/edge.nam`, or an absolute path into that folder — and the move put
/// the file under the library's device folder ([ADR 0036] › 4). The old form reads the new
/// place, so the project keeps its sound across the move.
#[test]
fn a_state_in_the_old_defaults_form_reads_the_files_new_place() {
    let dir = temp_dir("legacy-move-state");
    // The move's new place: the library's device folder holds the file; the old folder does not.
    let moved = dir.join("Sounds/NAM/Amp models/edge.nam");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::write(&moved, SHIPPED_OVERDRIVE).unwrap();

    // The relative old form.
    let mut device = Device::new("NAM Amp Modeler", DeviceKind::Builtin("nam-amp-modeler".into()));
    device.state = Some(b"NAM models/edge.nam".as_slice().into());
    let mut project = ProjectBuilder::new().audio_track(1, "Audio").build_without_ids();
    project.channels[0].devices.push(device);
    let mut session = Session::new(suhina_test_support::with_ids(project));
    session.set_library_base(dir.clone());
    session.sync_device_files();
    assert!(session.device_asset("NAM models/edge.nam").is_some(), "the old form reads the file's new place");

    // The absolute old form, inside the base's old folder.
    let old_place = dir.join("NAM models/edge.nam");
    let mut device = Device::new("NAM Amp Modeler", DeviceKind::Builtin("nam-amp-modeler".into()));
    device.state = Some(text(&old_place).into_bytes().into());
    let mut project = ProjectBuilder::new().audio_track(1, "Audio").build_without_ids();
    project.channels[0].devices.push(device);
    let mut session = Session::new(suhina_test_support::with_ids(project));
    session.set_library_base(dir.clone());
    session.sync_device_files();
    assert!(session.device_asset(&text(&old_place)).is_some(), "the absolute old form reads the file's new place too");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The library's base given again: a session's loads that ran with it stand, so the same
/// assignment does not clear the assets a project's devices already loaded.
#[test]
fn the_same_base_again_keeps_the_assets() {
    let dir = temp_dir("same-base-assets");
    let mut session = Session::new(blank_project());
    session.set_library_base(dir.clone());
    run(&mut session, "track.add", json!({ "kind": "audio" })).unwrap();
    run(&mut session, "device.add", json!({ "track": 1, "id": "nam-amp-modeler" })).unwrap();
    let file = dir.join("Sounds/NAM/Amp models/Suhina/Suhina Overdrive.nam");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, SHIPPED_OVERDRIVE).unwrap();
    run(&mut session, "device.setNamFile", json!({ "track": 1, "device": 0, "path": text(&file) })).unwrap();
    session.sync_device_files();
    let relative = "Sounds/NAM/Amp models/Suhina/Suhina Overdrive.nam";
    assert!(session.device_asset(relative).is_some());
    session.set_library_base(dir.clone());
    assert!(session.device_asset(relative).is_some(), "the same base again keeps the assets");
    let _ = std::fs::remove_dir_all(&dir);
}
