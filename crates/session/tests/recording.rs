// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recording (`docs/spec/recording.md`): MIDI takes into a slot and into the arrangement, the
//! count-in, audio takes written to files, one undo step per take. A real renderer is driven
//! offline through its controller, and its recording thread runs as it does in the app.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use serde_json::{Value, json};
use suhina_engine::{AudioPool, LiveEvent, LiveSender, Renderer};
use suhina_model::*;
use suhina_session::{Player, Session};

const SR: u32 = 48_000;
const BEAT: usize = SR as usize / 2;
const BLOCK: usize = 240;

fn track(id: u32, name: &str, audio_input: InputChannels) -> Track {
    Track {
        id: TrackId(id),
        name: name.into(),
        color: None,
        content: ContentKinds { notes: audio_input == InputChannels::None, audio: audio_input != InputChannels::None },
        channel: Some(ChannelId(id)),
        parent: None,
        midi_input: Default::default(),
        audio_input: AudioInput { channels: audio_input, monitor: Monitor::Off },
        extensions: Extensions::default(),
    }
}

fn channel(id: u32) -> Channel {
    Channel {
        id: ChannelId(id),
        name: format!("C{id}"),
        role: ChannelRole::Regular,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        solo: false,
        cue: false,
        output: None,
        sends: Vec::new(),
        devices: Vec::new(),
        extensions: Extensions::default(),
        automation: Vec::new(),
    }
}

/// Keys (plays notes) and Mic (hears input channel 1), with two empty scenes.
fn project() -> Project {
    suhina_test_support::with_ids(project_built())
}

fn project_built() -> Project {
    let empty = || Slot { clip: None, has_stop: true };
    Project {
        tracks: vec![track(1, "Keys", InputChannels::None), track(2, "Mic", InputChannels::Mono(1))],
        channels: vec![channel(1), channel(2)],
        launcher: Launcher {
            mappings: Vec::new(),
            scenes: vec![Scene::default(), Scene::default()],
            columns: vec![
                LauncherColumn { track: TrackId(1), slots: vec![empty(), empty()] },
                LauncherColumn { track: TrackId(2), slots: vec![empty(), empty()] },
            ],
        },
        ..Project::default()
    }
}

struct Rig {
    session: Session,
    renderer: Renderer,
    live: LiveSender,
    input: rtrb::Producer<f32>,
    /// Frames rendered.
    position: usize,
}

fn rig(project: Project) -> Rig {
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let (input, consumer) = rtrb::RingBuffer::new(BLOCK * 64);
    renderer.set_audio_input(consumer, 1, Arc::new(AtomicU64::new(0)));
    let mut controller = renderer.controller();
    let live = controller.take_live_input().unwrap();
    let mut session = Session::new(project);
    session.attach_player(Player::new(controller, None));
    Rig { session, renderer, live, input, position: 0 }
}

impl Rig {
    fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        self.session.execute(id, &params).map_err(String::from)
    }

    /// Renders until frame `until`, feeding the input `level`.
    fn render_to(&mut self, until: usize, level: f32) {
        let mut block = vec![0.0f32; BLOCK * 2];
        while self.position < until {
            for _ in 0..BLOCK {
                let _ = self.input.push(level);
            }
            self.renderer.process(&mut block);
            self.position += BLOCK;
        }
    }

    /// Lets the recording thread and the settling time pass, then looks.
    fn settle(&mut self) -> Vec<String> {
        std::thread::sleep(Duration::from_millis(350));
        let mut notices = self.session.poll_recording();
        std::thread::sleep(Duration::from_millis(50));
        notices.extend(self.session.poll_recording());
        notices
    }

    fn slot_clip(&self, track: usize, scene: usize) -> Option<(&Clip, &ClipContent)> {
        let p = self.session.project();
        let clip = p.launcher.columns.get(track)?.slots.get(scene)?.clip.as_ref()?;
        Some((clip, p.content(clip.content)?))
    }
}

fn notes(content: &ClipContent) -> Vec<(f64, f64, u8)> {
    content
        .lanes
        .iter()
        .flat_map(|l| match &l.data {
            LaneData::Notes(notes) => notes.iter().map(|n| (n.time, n.duration, n.key)).collect(),
            LaneData::Audio(_) => Vec::new(),
        })
        .collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn a_slot_records_from_the_next_bar_and_plays_its_take_looped() {
    let mut rig = rig(project());
    rig.run("track.setArm", json!({ "track": 1, "armed": true })).unwrap();
    rig.run("transport.play", json!({})).unwrap();
    rig.render_to(BLOCK * 10, 0.0);
    let started = rig.run("clip.record", json!({ "track": 1, "scene": 1 })).unwrap();
    assert_eq!(started["start_beat"], json!(4.0), "the next bar");
    assert_eq!(rig.session.recording_slot(TrackId(1)), Some(0));
    // A note before the take's start is not kept.
    rig.live.send(LiveEvent::NoteOn { key: 50, velocity: 1.0 });
    rig.render_to(BEAT * 3, 0.0);
    rig.live.send(LiveEvent::NoteOff { key: 50 });
    rig.render_to(BEAT * 5, 0.0);
    rig.live.send(LiveEvent::NoteOn { key: 60, velocity: 0.5 });
    rig.render_to(BEAT * 6, 0.0);
    rig.live.send(LiveEvent::NoteOff { key: 60 });
    // A key still down when the take ends is cut there.
    rig.live.send(LiveEvent::NoteOn { key: 64, velocity: 1.0 });
    rig.render_to(BEAT * 7, 0.0);
    let ended = rig.run("clip.launch", json!({ "track": 1, "scene": 1 })).unwrap();
    assert_eq!(ended["end_beat"], json!(8.0));
    assert_eq!(rig.session.recording_slot(TrackId(1)), None, "ending");
    // Asked to end, the take still records to its boundary: auto-arm leaves the arm alone.
    assert!(rig.session.is_recording(), "the take records on to beat 8");
    rig.render_to(BEAT * 8 + BLOCK, 0.0);
    rig.settle();
    assert!(!rig.session.is_recording(), "the take is a clip");
    let (clip, content) = rig.slot_clip(0, 0).expect("the take is a clip");
    assert_eq!(clip.loop_range, Some(LoopRange { start: 0.0, end: 4.0 }), "four beats, looped");
    assert!(close(content.length, 4.0));
    let got = notes(content);
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(close(got[0].0, 1.0) && close(got[0].1, 1.0) && got[0].2 == 60, "{got:?}");
    assert!(close(got[1].0, 2.0) && close(got[1].1, 2.0) && got[1].2 == 64, "{got:?}");
    // The take plays from where it is: the slot is playing.
    rig.render_to(BEAT * 9, 0.0);
    rig.session.poll_recording();
    rig.render_to(BEAT * 9 + BLOCK * 2, 0.0);
    let state = rig.run("launcher.state", json!({})).unwrap();
    assert_eq!(state["tracks"][0]["playing"]["scene"], json!(1), "{state}");

    // One undo removes the take.
    rig.run("edit.undo", json!({})).unwrap();
    assert!(rig.slot_clip(0, 0).is_none());
}

#[test]
fn the_arrangement_records_after_the_count_in() {
    let mut rig = rig(project());
    rig.run("track.setArm", json!({ "track": 1, "armed": true })).unwrap();
    rig.run("transport.setCountIn", json!({ "bars": 1 })).unwrap();
    let started = rig.run("transport.record", json!({ "on": true })).unwrap();
    assert_eq!(started["count_in_bars"], json!(1));
    assert_eq!(started["start_beat"], json!(0.0));
    assert!(rig.session.records_arrangement());
    rig.render_to(BEAT, 0.0);
    let state = rig.run("record.state", json!({})).unwrap();
    assert!(state["count_in_left"].as_f64().unwrap() > 2.5, "{state}");
    assert_eq!(state["takes"][0]["into"], json!("arrangement"));
    // Played during the count-in: not kept (the transport has not started).
    rig.live.send(LiveEvent::NoteOn { key: 40, velocity: 1.0 });
    rig.render_to(BEAT * 2, 0.0);
    rig.live.send(LiveEvent::NoteOff { key: 40 });
    // The count-in is four beats; the transport starts after it, at beat 0.
    rig.render_to(BEAT * 5, 0.0);
    rig.live.send(LiveEvent::NoteOn { key: 62, velocity: 1.0 });
    rig.render_to(BEAT * 6, 0.0);
    rig.live.send(LiveEvent::NoteOff { key: 62 });
    rig.render_to(BEAT * 7, 0.0);
    rig.run("transport.record", json!({ "on": false })).unwrap();
    assert!(!rig.session.records_arrangement());
    rig.settle();
    let p = rig.session.project();
    let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(1))).expect("a lane");
    assert_eq!(lane.clips.len(), 1);
    let clip = &lane.clips[0];
    assert_ne!(clip.id.0, 0, "the take's clip has an id the clip commands take");
    // It ends where the playhead was when asked: within a block of beat 3.
    assert!(close(clip.start, 0.0) && (clip.length - 3.0).abs() < 0.02, "{clip:?}");
    let got = notes(p.content(clip.content).unwrap());
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(close(got[0].0, 1.0) && close(got[0].1, 1.0) && got[0].2 == 62);
    // Rendered offline, the take plays what was played: sound from beat 1, gone after beat 2.
    let out = suhina_engine::render_offline(rig.session.project(), SR, 256).unwrap();
    let first = out.iter().position(|s| s.abs() > 1e-4).map(|i| i / 2).unwrap();
    assert!((BEAT..BEAT + 200).contains(&first), "{first}");
    let held = &out[BEAT * 3..BEAT * 3 + 400];
    assert!(held.iter().any(|s| s.abs() > 0.01), "sounding at beat 1.5");
    // Released at beat 2: the render ends with the note's release, 50 ms later.
    let frames = out.len() / 2;
    assert!((BEAT * 2..BEAT * 2 + SR as usize / 10).contains(&frames), "{frames}");
    rig.run("edit.undo", json!({})).unwrap();
    assert!(rig.session.project().arrangement.lanes.iter().all(|l| l.clips.is_empty()));
}

#[test]
fn stopping_the_transport_ends_the_take_where_it_stopped() {
    let mut rig = rig(project());
    rig.run("track.setArm", json!({ "track": 1, "armed": true })).unwrap();
    rig.run("transport.record", json!({ "on": true })).unwrap();
    rig.live.send(LiveEvent::NoteOn { key: 62, velocity: 1.0 });
    rig.render_to(BEAT * 2, 0.0);
    rig.run("transport.stop", json!({})).unwrap();
    rig.render_to(BEAT * 2 + BLOCK, 0.0);
    rig.settle();
    let p = rig.session.project();
    let clip = &p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(1))).unwrap().clips[0];
    assert!((clip.length - 2.0).abs() < 0.02, "{clip:?}");
    let got = notes(p.content(clip.content).unwrap());
    assert!(close(got[0].0, 0.0) && close(got[0].1, clip.length), "held to the end: {got:?}");
    let state = rig.run("record.state", json!({})).unwrap();
    assert_eq!(state["takes"], json!([]));
}

#[test]
fn recording_refuses_what_it_cannot_do() {
    let mut rig = rig(project());
    let not_armed = rig.run("clip.record", json!({ "track": 1, "scene": 1 })).unwrap_err();
    assert!(not_armed.contains("arm"), "{not_armed}");
    assert!(rig.run("transport.record", json!({ "on": true })).unwrap_err().contains("arm"));
    for bars in [3, 8, -1] {
        assert!(rig.run("transport.setCountIn", json!({ "bars": bars })).is_err(), "{bars}");
    }
    assert!(rig.run("transport.setCountIn", json!({ "bars": "two" })).is_err());
    rig.run("track.setArm", json!({ "track": 2, "armed": true })).unwrap();
    // An audio take needs a saved project for its file.
    let unsaved = rig.run("transport.record", json!({ "on": true })).unwrap_err();
    assert!(unsaved.contains("save the project"), "{unsaved}");
    rig.run("track.setArm", json!({ "track": 1, "armed": true, "exclusive": true })).unwrap();
    rig.run("clip.record", json!({ "track": 1, "scene": 1 })).unwrap();
    let twice = rig.run("clip.record", json!({ "track": 1, "scene": 2 })).unwrap_err();
    assert!(twice.contains("recording already"), "{twice}");
    // Without an engine.
    let mut alone = Session::new(project());
    alone.execute("track.setArm", &json!({ "track": 1, "armed": true })).unwrap();
    assert!(alone.execute("clip.record", &json!({ "track": 1, "scene": 1 })).is_err());
    assert_eq!(alone.execute("transport.setCountIn", &json!({ "bars": 2 })).unwrap(), json!({ "count_in_bars": 2 }));
    assert_eq!(alone.count_in_bars(), 2);
}

#[test]
fn a_take_whose_slot_filled_meanwhile_goes_to_the_next_empty_one() {
    let mut rig = rig(project());
    rig.run("track.setArm", json!({ "track": 1, "armed": true })).unwrap();
    rig.run("clip.record", json!({ "track": 1, "scene": 1 })).unwrap();
    rig.render_to(BEAT, 0.0);
    rig.run("clip.create", json!({ "track": 1, "scene": 1 })).unwrap();
    rig.run("clip.launch", json!({ "track": 1, "scene": 1 })).unwrap();
    rig.render_to(BEAT * 4 + BLOCK, 0.0);
    rig.settle();
    assert!(rig.slot_clip(0, 1).is_some(), "the take went to scene 2");
}

#[test]
fn an_audio_take_is_written_to_a_file_and_becomes_a_clip() {
    let dir = std::env::temp_dir().join(format!("suhina-recording-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut rig = rig(project());
    rig.session.save(&dir.join("song.suhina"), false).unwrap();
    rig.run("track.setArm", json!({ "track": 2, "armed": true })).unwrap();
    rig.run("transport.play", json!({})).unwrap();
    rig.render_to(BEAT, 0.25);
    let started = rig.run("transport.record", json!({ "on": true })).unwrap();
    let start = started["start_beat"].as_f64().unwrap();
    rig.render_to(BEAT * 3, 0.25);
    let state = rig.run("record.state", json!({})).unwrap();
    assert_eq!(state["takes"][0]["kind"], json!("audio"));
    let file = std::path::PathBuf::from(state["takes"][0]["file"].as_str().unwrap());
    assert!(file.starts_with(dir.join("Recorded")) && file.file_name().unwrap() == "Mic bar 1.wav", "{file:?}");
    rig.run("transport.record", json!({ "on": false })).unwrap();
    rig.render_to(BEAT * 3 + BLOCK * 4, 0.25);
    let notices = rig.settle();
    assert!(notices.is_empty(), "{notices:?}");
    let p = rig.session.project();
    let clip = &p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(2))).expect("a lane").clips[0];
    assert!(close(clip.start, start));
    let reader = hound::WavReader::open(&file).unwrap();
    assert_eq!(reader.spec().channels, 1);
    let samples: Vec<f32> = reader.into_samples::<f32>().map(Result::unwrap).collect();
    // From the start to where it ended: about two beats, all of them the input's level.
    let beats = samples.len() as f64 / BEAT as f64;
    assert!((beats - (3.0 - start)).abs() < 0.05, "{beats} beats from {start}");
    assert!(close(clip.length, beats));
    assert!(samples.iter().all(|s| *s == 0.25));
    rig.run("edit.undo", json!({})).unwrap();
    assert!(rig.session.project().arrangement.lanes.iter().all(|l| l.clips.is_empty()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sampler_recording_replaces_source_without_creating_a_clip() {
    let dir = std::env::temp_dir().join(format!("suhina-sampler-recording-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut p = project();
    p.tracks[0].audio_input.channels = InputChannels::Mono(1);
    let mut rig = rig(p);
    rig.session.save(&dir.join("song.suhina"), false).unwrap();
    rig.run("device.add", json!({"track":1,"id":"sampler"})).unwrap();
    rig.run("track.setArm", json!({"track":1,"armed":true})).unwrap();
    rig.run("transport.play", json!({})).unwrap();
    rig.render_to(BEAT, 0.25);
    rig.run("device.recordSample", json!({"track":1,"device":0,"on":true})).unwrap();
    assert!(rig.session.recording_sampler(TrackId(1), 0));
    rig.render_to(BEAT * 3, 0.25);
    rig.run("device.recordSample", json!({"track":1,"device":0,"on":false})).unwrap();
    rig.render_to(BEAT * 3 + BLOCK * 4, 0.25);
    let notices = rig.settle();
    assert!(notices.is_empty(), "{notices:?}");
    let file = rig.session.project().channels[0].devices[0].sample_file().unwrap().to_string();
    assert!(std::path::Path::new(&file).is_file());
    assert!(rig.session.audio().files[&file].samples.iter().all(|s| *s == 0.25));
    assert!(rig.session.project().arrangement.lanes.iter().all(|l| l.clips.is_empty()));
    rig.run("edit.undo", json!({})).unwrap();
    assert!(rig.session.project().channels[0].devices[0].sample_file().is_none());
    assert!(std::path::Path::new(&file).is_file(), "undo preserves the recorded file");
    std::fs::remove_dir_all(&dir).unwrap();
}
