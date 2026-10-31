// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recording (`docs/spec/recording.md`): MIDI takes into a slot and into the arrangement, the
//! count-in, audio takes written to files, one undo step per take. A real renderer is driven
//! offline through its controller, and its recording thread runs as it does in the app.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use std::sync::Arc;
use std::sync::atomic::AtomicU64;

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
    /// What the session said while rendering, for `settle` to give back.
    notices: Vec<String>,
}

fn rig(project: Project) -> Rig {
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let (input, consumer) = rtrb::RingBuffer::new(BLOCK * 64);
    renderer.set_audio_input(consumer, 1, Arc::new(AtomicU64::new(0)));
    let mut controller = renderer.controller();
    let live = controller.take_live_input().unwrap();
    let mut session = Session::new(project);
    session.attach_player(Player::new(controller, None));
    Rig { session, renderer, live, input, position: 0, notices: Vec::new() }
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
            // As an audio device's blocks come, a little apart: the recording thread drains the
            // input between them, or a take of a few beats overflows its ring.
            let notices = self.session.sync_recording(false);
            self.notices.extend(notices);
        }
    }

    /// Takes in everything rendered, with no more audio to come, and looks.
    fn settle(&mut self) -> Vec<String> {
        let mut notices = std::mem::take(&mut self.notices);
        notices.extend(self.session.sync_recording(true));
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
    rig.session.set_track_type_colours(Rgb(200, 40, 90), Rgb(30, 120, 220));
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
    let spans = rig.session.arrangement_recording_spans();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].0, TrackId(1));
    assert_eq!(spans[0].1, 0.0);
    assert!(spans[0].2 > 2.9, "live recording grows before committing: {spans:?}");
    rig.run("transport.record", json!({ "on": false })).unwrap();
    assert!(!rig.session.records_arrangement());
    rig.settle();
    let p = rig.session.project();
    let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(1))).expect("a lane");
    assert_eq!(lane.clips.len(), 1);
    let clip = &lane.clips[0];
    assert_ne!(clip.id.0, 0, "the take's clip has an id the clip commands take");
    assert_eq!(clip.color, Some(Rgb(200, 40, 90)));
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

fn lane_clips(session: &Session, track: u32) -> Vec<Clip> {
    let p = session.project();
    p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(track))).map(|l| l.clips.clone()).unwrap_or_default()
}

#[test]
fn moving_back_while_recording_keeps_each_pass_as_a_take_and_the_latest_plays() {
    let mut rig = rig(project());
    rig.run("track.setArm", json!({ "track": 1, "armed": true })).unwrap();
    rig.run("transport.record", json!({ "on": true })).unwrap();
    // The first pass: a note at beat 1, then a click on the ruler at beat 3 goes back to 0.
    rig.render_to(BEAT, 0.0);
    rig.live.send(LiveEvent::NoteOn { key: 60, velocity: 1.0 });
    rig.render_to(BEAT * 2, 0.0);
    rig.live.send(LiveEvent::NoteOff { key: 60 });
    rig.render_to(BEAT * 3, 0.0);
    // The window polls every frame; the notes come through the recording thread.
    rig.session.sync_recording(false);
    let live = rig.session.live_takes();
    assert_eq!(live.len(), 1, "the take is drawn while it records");
    assert!(close(live[0].start, 0.0) && (live[0].end - 3.0).abs() < 0.02, "{live:?}");
    assert_eq!(live[0].notes.len(), 1, "its note so far: {live:?}");
    rig.run("transport.setPosition", json!({ "beats": 0.0 })).unwrap();
    rig.render_to(BEAT * 3 + BLOCK * 2, 0.0);
    rig.session.sync_recording(false);
    let live = rig.session.live_takes();
    // The pass the jump ended is drawn on, finished, with its note; the new one follows it.
    assert_eq!(live.len(), 2, "both passes are drawn: {live:?}");
    assert!(live[0].finished && live[0].notes.len() == 1, "the first pass keeps its note: {live:?}");
    assert!(close(live[1].start, 0.0) && live[1].end < 0.1, "a new pass from beat 0: {live:?}");
    assert!(!live[1].finished && live[1].notes.is_empty(), "the first pass's note is not in it: {live:?}");
    // The second pass: a note at its beat 0.5, ended at its beat 2.
    rig.render_to(BEAT * 3 + BEAT / 2, 0.0);
    rig.live.send(LiveEvent::NoteOn { key: 64, velocity: 1.0 });
    rig.render_to(BEAT * 4, 0.0);
    rig.live.send(LiveEvent::NoteOff { key: 64 });
    rig.render_to(BEAT * 5, 0.0);
    rig.run("transport.record", json!({ "on": false })).unwrap();
    rig.settle();
    assert!(!rig.session.is_recording());

    let p = rig.session.project();
    let takes: Vec<&TakeLane> = p.arrangement.takes.iter().filter(|t| t.track == TrackId(1)).collect();
    assert_eq!(takes.len(), 2, "both passes are kept: {takes:?}");
    assert_eq!((takes[0].name.as_str(), takes[1].name.as_str()), ("Take 1", "Take 2"));
    let first = takes[0].clips[0].clone();
    assert!(close(first.start, 0.0) && (first.length - 3.0).abs() < 0.02, "{first:?}");
    let got = notes(p.content(first.content).unwrap());
    assert!(got.len() == 1 && got[0].2 == 60 && (got[0].0 - 1.0).abs() < 0.02, "{got:?}");
    let second = takes[1].clips[0].clone();
    assert!(close(second.start, 0.0) && (second.length - 2.0).abs() < 0.03, "{second:?}");
    let got = notes(p.content(second.content).unwrap());
    assert!(got.len() == 1 && got[0].2 == 64 && (got[0].0 - 0.5).abs() < 0.02, "{got:?}");
    // On the track the latest pass plays from 0 to 2; the first pass's last beat is left after it.
    let clips = lane_clips(&rig.session, 1);
    assert_eq!(clips.len(), 2, "{clips:?}");
    assert_eq!(clips[0].content, second.content);
    assert_eq!(clips[1].content, first.content);
    assert!((clips[1].start - second.length).abs() < 1e-9, "{clips:?}");

    // Comping: the first pass's beats 0 to 2 go back onto the track; one undo takes them away.
    let comped = rig.run("take.comp", json!({ "track": 1, "take": 1, "start": 0.0, "end": 2.0 })).unwrap();
    assert_eq!(comped["take"], json!("Take 1"));
    let clips = lane_clips(&rig.session, 1);
    assert!(clips.iter().all(|c| c.content == first.content), "{clips:?}");
    assert!(clips.iter().all(|c| c.id.0 != 0), "comped clips have ids: {clips:?}");
    rig.run("edit.undo", json!({})).unwrap();
    assert_eq!(lane_clips(&rig.session, 1)[0].content, second.content);
    // A part comped with the take lane's colour shows it on the track (#953).
    rig.run("take.comp", json!({ "track": 1, "take": 1, "start": 0.0, "end": 1.0, "color": "#33aa55" })).unwrap();
    let coloured = lane_clips(&rig.session, 1);
    assert_eq!(coloured.iter().find(|c| close(c.start, 0.0)).and_then(|c| c.color), Some(Rgb(0x33, 0xaa, 0x55)));
    assert!(rig.run("take.comp", json!({ "track": 1, "take": 1, "color": "green" })).is_err());
    rig.run("edit.undo", json!({})).unwrap();
    // A take can be deleted; the track keeps its clips.
    rig.run("take.delete", json!({ "track": 1, "take": 2 })).unwrap();
    assert_eq!(rig.session.project().arrangement.takes.len(), 1);
    assert_eq!(lane_clips(&rig.session, 1).len(), 2);
    assert!(rig.run("take.comp", json!({ "track": 1, "take": 2 })).is_err(), "no take 2 any more");
    assert!(rig.run("take.comp", json!({ "track": 1, "take": 1, "start": 2.0, "end": 1.0 })).is_err());
}

#[test]
fn the_loop_wrapping_while_recording_audio_makes_a_take_of_each_pass_from_one_file() {
    let dir = std::env::temp_dir().join(format!("suhina-recording-loop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut project = project();
    project.transport.loop_range = Some(LoopRange { start: 0.0, end: 2.0 });
    project.transport.loop_enabled = true;
    let mut rig = rig(project);
    rig.session.save(&dir.join("song.suhina"), false).unwrap();
    rig.run("track.setArm", json!({ "track": 2, "armed": true })).unwrap();
    rig.run("transport.record", json!({ "on": true })).unwrap();
    // Two beats at one level, then the loop goes back and the next pass hears another.
    rig.render_to(BEAT * 2, 0.25);
    rig.render_to(BEAT * 3, 0.5);
    rig.run("transport.record", json!({ "on": false })).unwrap();
    rig.render_to(BEAT * 3 + BLOCK * 4, 0.5);
    let notices = rig.settle();
    assert!(notices.is_empty(), "{notices:?}");
    let p = rig.session.project();
    let takes: Vec<&TakeLane> = p.arrangement.takes.iter().filter(|t| t.track == TrackId(2)).collect();
    assert_eq!(takes.len(), 2, "{takes:?}");
    let (first, second) = (&takes[0].clips[0], &takes[1].clips[0]);
    assert_eq!(first.content, second.content, "one file");
    assert!(close(first.offset, 0.0) && (first.length - 2.0).abs() < 0.02, "{first:?}");
    assert!(
        (second.offset - first.length).abs() < 0.02,
        "the second pass reads on from where the first ended: {second:?}"
    );
    assert!(close(second.start, 0.0) && (second.length - 1.0).abs() < 0.03, "{second:?}");
    let clips = lane_clips(&rig.session, 2);
    assert_eq!(clips[0].offset, second.offset, "the latest pass plays first: {clips:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A take ends where Stop was pressed, not at the next grid line or bar (finding 4 of #916's
/// testing): a note played in the first beat, Stop after two and a half beats, and the clip is
/// two and a half beats long.
#[test]
fn a_take_stopped_mid_bar_is_as_long_as_what_was_recorded() {
    let mut rig = rig(project());
    rig.run("track.setArm", json!({ "track": 1, "armed": true })).unwrap();
    rig.run("transport.record", json!({ "on": true })).unwrap();
    rig.render_to(BLOCK * 4, 0.0);
    rig.live.send(LiveEvent::NoteOn { key: 60, velocity: 1.0 });
    rig.render_to(BEAT / 2, 0.0);
    rig.live.send(LiveEvent::NoteOff { key: 60 });
    rig.render_to(BEAT * 5 / 2, 0.0);
    rig.run("transport.stop", json!({ "return": "marker" })).unwrap();
    rig.render_to(BEAT * 5 / 2 + BLOCK * 4, 0.0);
    let notices = rig.settle();
    assert!(notices.is_empty(), "{notices:?}");
    let clips = lane_clips(&rig.session, 1);
    assert_eq!(clips.len(), 1, "{clips:?}");
    assert!((clips[0].length - 2.5).abs() < 0.02, "{:?}", clips[0]);
}
