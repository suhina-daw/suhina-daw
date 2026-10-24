// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Transport and launcher commands drive a real renderer through its controller, rendered
//! offline here instead of on an audio device.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use serde_json::{Value, json};
use suhina_engine::{AudioPool, Quantization, Renderer};
use suhina_model::*;
use suhina_session::{CommandKind, ParamType, Player, Session, commands, parse_quantization};

use crate::support::run;

const SR: u32 = 48_000;
const BEAT: usize = SR as usize / 2;

fn project() -> Project {
    suhina_test_support::with_ids(project_built())
}

fn project_built() -> Project {
    let track = |id: u32, name: &str| Track {
        id: TrackId(id),
        name: name.into(),
        color: None,
        content: ContentKinds { notes: true, audio: false },
        channel: Some(ChannelId(id)),
        parent: None,
        midi_input: Default::default(),
        audio_input: Default::default(),
        extensions: Extensions::default(),
    };
    let channel = |id: u32| Channel {
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
    };
    let content = |id: u32, name: &str, key: u8| ClipContent {
        id: ContentId(id),
        name: name.into(),
        length: 1.0,
        lanes: vec![ContentLane {
            target: None,
            data: LaneData::Notes(vec![Note {
                time: 0.0,
                duration: 0.25,
                key,
                velocity: 1.0,
                release_velocity: 0.5,
                channel: 0,
                muted: false,
            }]),
        }],
        extensions: Extensions::default(),
        scale: None,
    };
    let clip = |content: u32, enabled: bool| Clip {
        id: Default::default(),
        content: ContentId(content),
        start: 0.0,
        length: 1.0,
        offset: 0.0,
        loop_range: Some(LoopRange { start: 0.0, end: 1.0 }),
        enabled,
        fade_in: 0.0,
        fade_out: 0.0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        follow_action: Default::default(),
        launch: Default::default(),
        color: None,
        extensions: Extensions::default(),
    };
    let slot = |clip: Option<Clip>, has_stop: bool| Slot { clip, has_stop };
    Project {
        tracks: vec![track(1, "Drums"), track(2, "Bass"), track(3, "Pad")],
        channels: vec![channel(1), channel(2), channel(3)],
        contents: vec![content(1, "Beat", 36), content(2, "Line", 40), content(3, "Off", 50)],
        launcher: Launcher {
            mappings: Vec::new(),
            scenes: vec![
                Scene { name: "Intro".into(), ..Scene::default() },
                Scene { id: Default::default(), name: "Drop".into(), tempo_bpm: Some(128.0), time_signature: None },
            ],
            columns: vec![
                LauncherColumn {
                    track: TrackId(1),
                    slots: vec![slot(Some(clip(1, true)), true), slot(Some(clip(1, true)), true)],
                },
                LauncherColumn { track: TrackId(2), slots: vec![slot(None, true), slot(Some(clip(2, true)), true)] },
                // A deactivated clip, and an empty slot without a stop button.
                LauncherColumn { track: TrackId(3), slots: vec![slot(Some(clip(3, false)), true), slot(None, false)] },
            ],
        },
        ..Project::default()
    }
}

/// A session with an offline renderer attached as its player.
fn playing() -> (Session, Renderer) {
    let project = project();
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let mut session = Session::new(project);
    session.attach_player(Player::new(renderer.controller(), None));
    (session, renderer)
}

fn advance(renderer: &mut Renderer, frames: usize) {
    let mut buffer = vec![0.0f32; 2 * 256];
    let mut left = frames;
    while left > 0 {
        let n = left.min(256);
        renderer.process(&mut buffer[..2 * n]);
        left -= n;
    }
}

#[test]
fn transport_commands_need_a_player() {
    let mut session = Session::new(project());
    // The quantisation is kept by the session until a player is attached (tested below). Arming
    // is kept by the session and told to each new player, so it needs none either (ADR 0017);
    // nor do the count-in and what records (docs/spec/recording.md).
    let without = ["launcher.setQuantization", "track.setArm", "transport.setCountIn", "record.state"];
    for spec in commands().iter().filter(|c| c.kind == CommandKind::Transport && !without.contains(&c.id)) {
        let params = json!({ "track": "Drums", "scene": 1, "value": "1 bar", "on": true, "beats": 4.0, "key": 60 });
        let mut params = params
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, _)| spec.params.iter().any(|p| p.name == k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<serde_json::Map<_, _>>();
        // Any other parameter it needs, of its type, so that what refuses it is the missing engine.
        let missing: Vec<_> = spec.params.iter().filter(|p| p.required && !params.contains_key(p.name)).collect();
        for p in missing {
            let value = match p.ty {
                ParamType::Integer | ParamType::DevicePath => json!(0),
                ParamType::Number | ParamType::Decibels => json!(1.0),
                ParamType::Bool => json!(true),
                _ => json!("x"),
            };
            params.insert(p.name.into(), value);
        }
        let params = Value::from(params);
        let err = run(&mut session, spec.id, params).unwrap_err();
        assert!(err.contains("no audio engine"), "{}: {err}", spec.id);
    }
    // The launcher grid is part of the project and needs no player.
    let grid = run(&mut session, "project.launcher", json!({})).unwrap();
    assert_eq!(grid["scenes"][1]["name"], "Drop");
    assert_eq!(grid["scenes"][1]["tempo_bpm"], 128.0);
    assert_eq!(grid["tracks"][0]["slots"][0]["clip"], "Beat");
    assert_eq!(grid["tracks"][1]["slots"][0]["stop_button"], true);
}

#[test]
fn launching_a_scene_plays_its_row_and_changes_the_tempo() {
    let (mut session, mut renderer) = playing();
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["running"], false);

    run(&mut session, "scene.launch", json!({ "scene": 2 })).unwrap();
    advance(&mut renderer, 100);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["running"], true);
    assert_eq!(state["tempo_bpm"], 128.0);
    assert_eq!(state["tracks"][0]["playing"]["scene"], 2);
    assert_eq!(state["tracks"][1]["playing"]["clip"], "Line");
    // Scene 2's slot on track 3 is empty without a stop button: nothing changes there.
    assert_eq!(state["tracks"][2]["playing"], Value::Null);
    assert_eq!(state["tracks"][0]["plays_arrangement"], false);
    assert_eq!(state["tracks"][2]["plays_arrangement"], true);
}

#[test]
fn launches_and_stops_are_queued_until_the_next_bar() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "clip.launch", json!({ "track": "Drums", "scene": 1 })).unwrap();
    advance(&mut renderer, BEAT);
    let launched = run(&mut session, "clip.launch", json!({ "track": "Bass", "scene": 2 })).unwrap();
    assert_eq!(launched["queued"], "launch");
    run(&mut session, "track.stop", json!({ "track": 1 })).unwrap();
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["tracks"][0]["playing"]["scene"], 1);
    assert_eq!(state["tracks"][0]["queued"], "stop");
    assert_eq!(state["tracks"][1]["queued"]["clip"], "Line");
    assert_eq!(state["position"], "1.2");

    advance(&mut renderer, 3 * BEAT);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["position"], "2.1");
    assert_eq!(state["tracks"][0]["playing"], Value::Null);
    assert_eq!(state["tracks"][1]["playing"]["scene"], 2);
    assert_eq!(state["tracks"][1]["queued"], Value::Null);

    // An empty slot with a stop button stops its track.
    let stopped = run(&mut session, "clip.launch", json!({ "track": "Bass", "scene": 1 })).unwrap();
    assert_eq!(stopped["queued"], "stop");

    run(&mut session, "transport.stop", json!({})).unwrap();
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["running"], false);
    assert_eq!(state["tracks"][1]["playing"], Value::Null);
    assert_eq!(state["tracks"][1]["queued"], Value::Null);
}

#[test]
fn bad_launches_are_errors_and_change_nothing() {
    let (mut session, mut renderer) = playing();
    let before = session.project().clone();
    let bad = [
        ("clip.launch", json!({ "track": "Drums", "scene": 0 })),
        ("clip.launch", json!({ "track": "Drums", "scene": 3 })),
        ("clip.launch", json!({ "track": "Nope", "scene": 1 })),
        ("clip.launch", json!({ "track": "Drums", "scene": -1 })),
        ("clip.launch", json!({ "track": "Drums", "scene": "1" })),
        // A deactivated clip, and an empty slot without a stop button.
        ("clip.launch", json!({ "track": "Pad", "scene": 1 })),
        ("clip.launch", json!({ "track": "Pad", "scene": 2 })),
        ("scene.launch", json!({ "scene": u64::MAX })),
        ("track.stop", json!({ "track": 99 })),
        ("launcher.setQuantization", json!({ "value": "3 bars" })),
        ("launcher.setQuantization", json!({ "value": 4 })),
    ];
    for (id, params) in bad {
        assert!(run(&mut session, id, params.clone()).is_err(), "{id} {params}");
    }
    advance(&mut renderer, BEAT);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["running"], false);
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
}

#[test]
fn quantisation_values_parse_to_beats() {
    assert_eq!(parse_quantization("none").unwrap(), Quantization::None);
    assert_eq!(parse_quantization(" 2 Bars ").unwrap(), Quantization::Bars(2));
    assert_eq!(parse_quantization("1 bar").unwrap(), Quantization::Bars(1));
    assert_eq!(parse_quantization("1/4").unwrap(), Quantization::Beats(1.0));
    assert_eq!(parse_quantization("1/16").unwrap(), Quantization::Beats(0.25));
    assert!(matches!(parse_quantization("1/8T").unwrap(), Quantization::Beats(b) if (b - 1.0 / 3.0).abs() < 1e-12));
    for bad in ["", "1/3", "1/32T", "bar", "16 bars"] {
        assert!(parse_quantization(bad).is_err(), "{bad}");
    }
}

#[test]
fn the_quantisation_is_kept_without_a_player_and_handed_to_the_next() {
    let project = project();
    let mut session = Session::new(project.clone());
    assert_eq!(session.quantization(), Quantization::Bars(1));
    run(&mut session, "launcher.setQuantization", json!({ "value": "1/16" })).unwrap();
    assert_eq!(session.quantization(), Quantization::Beats(0.25));
    // A bad value changes nothing.
    assert!(run(&mut session, "launcher.setQuantization", json!({ "value": "1/3" })).is_err());
    assert_eq!(session.quantization(), Quantization::Beats(0.25));
    run(&mut session, "launcher.setQuantization", json!({ "value": "none" })).unwrap();

    // The player attached now launches at once, as `none` says, not on its own default bar.
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    session.attach_player(Player::new(renderer.controller(), None));
    run(&mut session, "clip.launch", json!({ "track": "Drums", "scene": 1 })).unwrap();
    advance(&mut renderer, BEAT / 3);
    run(&mut session, "clip.launch", json!({ "track": "Bass", "scene": 2 })).unwrap();
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["tracks"][1]["playing"]["scene"], 2);
}

#[test]
fn quantisation_none_launches_at_once() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "clip.launch", json!({ "track": "Drums", "scene": 1 })).unwrap();
    advance(&mut renderer, BEAT / 3);
    run(&mut session, "launcher.setQuantization", json!({ "value": "none" })).unwrap();
    run(&mut session, "clip.launch", json!({ "track": "Bass", "scene": 2 })).unwrap();
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["tracks"][1]["playing"]["scene"], 2);
}

/// Renders `frames` and returns the loudest sample of each beat.
fn beat_peaks(renderer: &mut Renderer, beats: usize) -> Vec<f32> {
    let mut buffer = vec![0.0f32; 2 * BEAT];
    (0..beats)
        .map(|_| {
            renderer.process(&mut buffer);
            buffer.iter().fold(0.0f32, |m, s| m.max(s.abs()))
        })
        .collect()
}

#[test]
fn edits_and_undo_are_heard_while_playing() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "clip.launch", json!({ "track": "Drums", "scene": 1 })).unwrap();
    let loud = beat_peaks(&mut renderer, 2)[1];
    assert!(loud > 0.05);

    // -12 dB is a quarter of the level, from the next note on.
    run(&mut session, "track.setVolume", json!({ "tracks": ["Drums"], "db": -12 })).unwrap();
    let quieter = beat_peaks(&mut renderer, 2)[1];
    assert!((quieter / loud - 0.25).abs() < 0.01, "{loud} -> {quieter}");
    // The clip kept playing: the launcher still reports it.
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["running"], true);

    run(&mut session, "edit.undo", json!({})).unwrap();
    let back = beat_peaks(&mut renderer, 2)[1];
    assert!((back / loud - 1.0).abs() < 0.01, "{loud} -> {back}");
}

#[test]
fn a_drag_that_outruns_the_engine_is_kept_and_heard_once_it_catches_up() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "clip.launch", json!({ "track": "Drums", "scene": 1 })).unwrap();
    let loud = beat_peaks(&mut renderer, 2)[1];
    // Edits while the audio thread takes none fill the engine's queue of new graphs (a mute takes
    // a new graph; a volume or a pan is a message to the playing one).
    let mut full = None;
    for i in 0..64 {
        let mute = i % 2 == 0;
        if let Err(e) = run(&mut session, "track.setMute", json!({ "tracks": ["Pad"], "mute": mute })) {
            full = Some(e);
            break;
        }
    }
    assert!(full.is_some_and(|e| e.contains("could not follow")));
    // A continued edit (a drag) is not an error then: the project has it, the engine catches up.
    run(&mut session, "track.setPan", json!({ "tracks": ["Drums"], "pan": 0.0, "continue": true })).unwrap();
    assert!(session.project().channels[0].pan.abs() < 1e-9);
    run(&mut session, "track.setVolume", json!({ "tracks": ["Drums"], "db": -12, "continue": true })).unwrap();
    assert!((session.project().channels[0].volume - 0.2512).abs() < 1e-3);
    beat_peaks(&mut renderer, 1);
    session.catch_up_player();
    let quieter = beat_peaks(&mut renderer, 2)[1];
    assert!((quieter / loud - 0.25).abs() < 0.01, "{loud} -> {quieter}");
}

#[test]
fn the_metronome_is_a_transport_command() {
    let (mut session, mut renderer) = playing();
    let set = run(&mut session, "transport.setMetronome", json!({ "on": true, "db": -12 })).unwrap();
    assert_eq!(set, json!({ "metronome": true, "db": -12.0, "sound": "beep" }));
    run(&mut session, "transport.play", json!({})).unwrap();
    let peaks = beat_peaks(&mut renderer, 2);
    // A click at -12 dB on each beat, and no undo step.
    assert!(peaks.iter().all(|p| (0.2..0.26).contains(p)), "{peaks:?}");
    assert_eq!(run(&mut session, "launcher.state", json!({})).unwrap()["metronome"], true);
    assert!(!session.can_undo());
    // Off and on again without a level keeps -12 dB.
    assert_eq!(
        run(&mut session, "transport.setMetronome", json!({ "on": false })).unwrap(),
        json!({ "metronome": false, "db": -12.0, "sound": "beep" })
    );
    run(&mut session, "transport.setMetronome", json!({ "on": true })).unwrap();
    let peaks = beat_peaks(&mut renderer, 2);
    assert!(peaks.iter().all(|p| (0.2..0.26).contains(p)), "{peaks:?}");
    assert!(run(&mut session, "transport.setMetronome", json!({ "on": true, "db": 7 })).is_err());
    // The sound is picked by name and kept until another is.
    let wood = run(&mut session, "transport.setMetronome", json!({ "on": true, "sound": "wood" })).unwrap();
    assert_eq!(wood["sound"], "wood");
    assert_eq!(run(&mut session, "transport.setMetronome", json!({ "on": true })).unwrap()["sound"], "wood");
    assert!(run(&mut session, "transport.setMetronome", json!({ "on": true, "sound": "cowbell" })).is_err());
    assert!(run(&mut session, "transport.setMetronome", json!({ "on": true, "sound": 5 })).is_err());
}

#[test]
fn devices_added_while_playing_are_heard() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "clip.launch", json!({ "track": "Drums", "scene": 1 })).unwrap();
    let sine = beat_peaks(&mut renderer, 2)[1];
    assert!(sine > 0.05);

    // The FM synth plays the track's notes instead of the sine voices; its own volume proves
    // it is the synth that sounds.
    run(&mut session, "device.add", json!({ "track": "Drums", "id": "fm-synth" })).unwrap();
    let full = beat_peaks(&mut renderer, 2)[1];
    assert!(full > 0.05, "{full}");
    run(&mut session, "device.setParam", json!({ "track": "Drums", "device": 0, "param": "volume", "value": -6.0206 }))
        .unwrap();
    let synth = beat_peaks(&mut renderer, 2)[1];
    assert!((synth / full - 0.5).abs() < 0.02, "{full} -> {synth}");

    // An effect after it, then its gain: a quarter of the level. The renderer takes one new
    // graph per block (ADR 0005) and these blocks are a beat long, so the second edit is heard
    // a beat after the first.
    run(&mut session, "device.add", json!({ "track": "Drums", "id": "utility" })).unwrap();
    run(&mut session, "device.setParam", json!({ "track": "Drums", "device": 1, "param": "gain", "value": -12 }))
        .unwrap();
    let quieter = beat_peaks(&mut renderer, 3)[2];
    assert!((quieter / synth - 0.25).abs() < 0.02, "{synth} -> {quieter}");

    // Undo four times: the sine voices again, once the four graphs have been taken.
    for _ in 0..4 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    let back = beat_peaks(&mut renderer, 5)[4];
    assert!((back / sine - 1.0).abs() < 0.01, "{sine} -> {back}");
}

#[test]
fn the_play_start_marker_moves_and_stop_returns_to_it() {
    let (mut session, mut renderer) = playing();
    let set = run(&mut session, "transport.setPosition", json!({ "beats": 6.0 })).unwrap();
    assert_eq!(set["start_beat"], 6.0);
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!((state["start_beat"].as_f64(), state["position"].as_str()), (Some(6.0), Some("2.3")));

    run(&mut session, "transport.play", json!({})).unwrap();
    advance(&mut renderer, 2 * BEAT);
    // The beat is published as a block starts.
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["position"], "3.1");

    // Stop goes back to the marker; stop again goes to the beginning.
    run(&mut session, "transport.stop", json!({})).unwrap();
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!((state["start_beat"].as_f64(), state["position"].as_str()), (Some(6.0), Some("2.3")));
    run(&mut session, "transport.stop", json!({ "return": "marker" })).unwrap();
    advance(&mut renderer, 1);
    assert_eq!(run(&mut session, "launcher.state", json!({})).unwrap()["start_beat"], 0.0);

    // "here" leaves the marker where the playhead stopped.
    run(&mut session, "transport.play", json!({})).unwrap();
    advance(&mut renderer, 3 * BEAT);
    run(&mut session, "transport.stop", json!({ "return": "here" })).unwrap();
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!((state["start_beat"].as_f64(), state["running"].as_bool()), (Some(3.0), Some(false)));

    // While playing, a new position is a jump.
    run(&mut session, "transport.play", json!({})).unwrap();
    advance(&mut renderer, BEAT);
    run(&mut session, "transport.setPosition", json!({ "beats": 0 })).unwrap();
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!((state["running"].as_bool(), state["position"].as_str()), (Some(true), Some("1.1")));
}

#[test]
fn jumping_to_a_marker_moves_the_play_start_marker_and_playback() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "marker.add", json!({ "beat": 6.0, "name": "Verse" })).unwrap();
    run(&mut session, "marker.add", json!({ "beat": 2.0, "name": "Intro" })).unwrap();
    let jump = run(&mut session, "marker.jump", json!({ "index": 1 })).unwrap();
    assert_eq!(jump["start_beat"], 6.0);
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!((state["start_beat"].as_f64(), state["position"].as_str()), (Some(6.0), Some("2.3")));

    // While playing, playback jumps there at once.
    run(&mut session, "transport.play", json!({})).unwrap();
    advance(&mut renderer, BEAT);
    run(&mut session, "marker.jump", json!({ "index": 0 })).unwrap();
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!((state["running"].as_bool(), state["position"].as_str()), (Some(true), Some("1.3")));

    // A jump is not an edit: undo takes back the second marker, not the jump.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().arrangement.markers.len(), 1);
    // A marker that is not there is an error.
    assert!(run(&mut session, "marker.jump", json!({ "index": 1 })).unwrap_err().contains("does not exist"));
    assert!(run(&mut session, "marker.jump", json!({})).is_err());
}

#[test]
fn bad_positions_and_stop_returns_are_errors() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "transport.setPosition", json!({ "beats": 2 })).unwrap();
    let bad = [
        ("transport.setPosition", json!({ "beats": -1 })),
        ("transport.setPosition", json!({ "beats": -0.001 })),
        ("transport.setPosition", json!({ "beats": 120_000.5 })),
        ("transport.setPosition", json!({ "beats": 1e300 })),
        ("transport.setPosition", json!({ "beats": "NaN" })),
        ("transport.setPosition", json!({ "beats": "4" })),
        ("transport.setPosition", json!({ "beats": null })),
        ("transport.setPosition", json!({})),
        ("transport.stop", json!({ "return": "start" })),
        ("transport.stop", json!({ "return": true })),
    ];
    for (id, params) in bad {
        assert!(run(&mut session, id, params.clone()).is_err(), "{id} {params}");
    }
    // JSON has no NaN; a non-finite number from code is refused the same way.
    assert!(run(&mut session, "transport.setPosition", json!({ "beats": f64::NAN })).is_err());
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!((state["start_beat"].as_f64(), state["running"].as_bool()), (Some(2.0), Some(false)));
    // The largest position is accepted.
    run(&mut session, "transport.setPosition", json!({ "beats": 120_000 })).unwrap();
}

/// [`playing`] with arrangement clips on Drums at beats 4 and 10, and on Bass at beat 2.
fn playing_with_clips() -> (Session, Renderer) {
    let mut project = project();
    let clip = |start: f64| Clip {
        id: Default::default(),
        content: ContentId(1),
        start,
        length: 1.0,
        offset: 0.0,
        loop_range: None,
        enabled: true,
        fade_in: 0.0,
        fade_out: 0.0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        follow_action: Default::default(),
        color: None,
        extensions: Extensions::default(),
        launch: Default::default(),
    };
    let lane = |track: u32, clips: Vec<Clip>| ArrangementLane {
        id: Default::default(),
        name: String::new(),
        track: Some(TrackId(track)),
        clips,
    };
    project.arrangement.lanes = vec![lane(1, vec![clip(4.0), clip(10.0)]), lane(2, vec![clip(2.0)])];
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let mut session = Session::new(project);
    session.attach_player(Player::new(renderer.controller(), None));
    advance(&mut renderer, 1);
    (session, renderer)
}

#[test]
fn play_from_a_clip_moves_the_marker_to_its_start() {
    let (mut session, mut renderer) = playing_with_clips();
    let played = run(&mut session, "transport.play", json!({ "from": "clip", "track": "Drums", "index": 1 })).unwrap();
    assert_eq!(played, json!({ "transport": "playing", "start_beat": 10.0 }));
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!((state["start_beat"].as_f64(), state["running"].as_bool()), (Some(10.0), Some(true)));
    assert_eq!(state["position"], "3.3");

    // Stop goes back to the clip's start, where the marker now is.
    advance(&mut renderer, 2 * BEAT);
    run(&mut session, "transport.stop", json!({})).unwrap();
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!((state["start_beat"].as_f64(), state["position"].as_str()), (Some(10.0), Some("3.3")));

    // While playing, another clip is a jump to its start.
    run(&mut session, "transport.play", json!({})).unwrap();
    advance(&mut renderer, BEAT);
    run(&mut session, "transport.play", json!({ "from": "clip", "track": 2, "index": 0 })).unwrap();
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!((state["start_beat"].as_f64(), state["running"].as_bool()), (Some(2.0), Some(true)));
    assert_eq!(state["position"], "1.3");

    // from: marker is the plain play: the marker stays.
    run(&mut session, "transport.stop", json!({})).unwrap();
    run(&mut session, "transport.play", json!({ "from": "marker" })).unwrap();
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!((state["start_beat"].as_f64(), state["running"].as_bool()), (Some(2.0), Some(true)));
}

#[test]
fn bad_play_sources_are_errors_and_leave_the_transport() {
    let (mut session, mut renderer) = playing_with_clips();
    let bad = [
        json!({ "from": "clip" }),
        json!({ "from": "clip", "track": "Drums" }),
        json!({ "from": "clip", "index": 0 }),
        json!({ "from": "clip", "track": "Drums", "index": 2 }),
        json!({ "from": "clip", "track": "Drums", "index": -1 }),
        json!({ "from": "clip", "track": "Drums", "index": "0" }),
        json!({ "from": "clip", "track": "Nobody", "index": 0 }),
        // Pad has no arrangement lane.
        json!({ "from": "clip", "track": "Pad", "index": 0 }),
        json!({ "from": "selection" }),
        json!({ "from": true }),
        json!({ "track": "Drums", "index": 0 }),
        json!({ "from": "marker", "index": 0 }),
    ];
    for params in bad {
        assert!(run(&mut session, "transport.play", params.clone()).is_err(), "{params}");
    }
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!((state["start_beat"].as_f64(), state["running"].as_bool()), (Some(0.0), Some(false)));
}

#[test]
fn a_key_column_note_plays_until_it_is_let_go() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "device.add", json!({ "track": "Drums", "id": "fm-synth" })).unwrap();
    advance(&mut renderer, 2);
    let before = session.project().clone();
    let on = run(&mut session, "preview.noteOn", json!({ "track": "Drums", "key": 60 })).unwrap();
    assert_eq!(on, json!({ "track": 1, "key": 60, "velocity": 100.0 }));
    // The note holds while the transport is stopped, taking no undo step.
    let held = beat_peaks(&mut renderer, 2)[1];
    assert!(held > 0.01, "the track's FM synth plays the key: {held}");
    assert!(session.player().unwrap().preview_monitor().playing());
    let later = beat_peaks(&mut renderer, 4)[3];
    assert!(later > held * 0.2, "{held} -> {later}");
    // Playing and letting go of a key is no edit: the only undo step is the device added above.
    assert_eq!(session.project(), &before);
    assert_eq!(run(&mut session, "edit.undo", json!({})).unwrap()["undone"], true);
    run(&mut session, "edit.redo", json!({})).unwrap();

    // Letting go releases it.
    assert_eq!(
        run(&mut session, "preview.noteOff", json!({ "track": "Drums", "key": 60 })).unwrap(),
        json!({ "key": 60 })
    );
    let tail = beat_peaks(&mut renderer, 2)[1];
    assert!(tail < held * 0.05, "released: {tail} of {held}");
    assert!(!session.player().unwrap().preview_monitor().playing());
    // Letting go again, of a key that is not held, is not an error.
    run(&mut session, "preview.noteOff", json!({ "track": "Drums", "key": 64 })).unwrap();
    run(&mut session, "preview.noteOff", json!({ "track": "Drums" })).unwrap();
    assert_eq!(session.project(), &before);
}

/// The piano roll's audition plays the notes a drag moved together: `preview.noteOn` with `keys`.
#[test]
fn a_chord_plays_until_it_is_let_go() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "device.add", json!({ "track": "Drums", "id": "fm-synth" })).unwrap();
    advance(&mut renderer, 2);
    let before = session.project().clone();
    let on = run(&mut session, "preview.noteOn", json!({ "track": "Drums", "keys": [64, 60, 60, 67], "velocity": 90 }))
        .unwrap();
    assert_eq!(on, json!({ "track": 1, "keys": [60, 64, 67], "velocity": 90.0 }));
    let held = beat_peaks(&mut renderer, 2)[1];
    assert!(held > 0.01, "the chord plays: {held}");
    run(&mut session, "preview.noteOff", json!({ "track": "Drums" })).unwrap();
    let tail = beat_peaks(&mut renderer, 2)[1];
    assert!(tail < held * 0.05, "released: {tail} of {held}");
    assert!(!session.player().unwrap().preview_monitor().playing());
    assert_eq!(session.project(), &before);
}

#[test]
fn bad_key_column_notes_are_refused_and_change_nothing() {
    let (mut session, mut renderer) = playing();
    let before = session.project().clone();
    let bad = [
        ("preview.noteOn", json!({ "track": "Drums" })),
        ("preview.noteOn", json!({ "track": "Drums", "key": 128 })),
        ("preview.noteOn", json!({ "track": "Drums", "key": -1 })),
        ("preview.noteOn", json!({ "track": "Drums", "key": 1.5 })),
        ("preview.noteOn", json!({ "track": "Drums", "key": 60, "velocity": 0 })),
        ("preview.noteOn", json!({ "track": "Drums", "key": 60, "velocity": 128 })),
        ("preview.noteOn", json!({ "track": "Nope", "key": 60 })),
        ("preview.noteOn", json!({ "track": "Drums", "key": 60, "keys": [60] })),
        ("preview.noteOn", json!({ "track": "Drums", "keys": [] })),
        ("preview.noteOn", json!({ "track": "Drums", "keys": [60, 128] })),
        ("preview.noteOn", json!({ "track": "Drums", "keys": [-1] })),
        ("preview.noteOn", json!({ "track": "Drums", "keys": 60 })),
        ("preview.noteOn", json!({ "track": "Drums", "keys": vec![60; 129] })),
        ("preview.noteOff", json!({ "track": "Drums", "key": 200 })),
        ("preview.noteOff", json!({ "track": "Nope" })),
    ];
    for (id, params) in bad {
        assert!(run(&mut session, id, params.clone()).is_err(), "{id} {params}");
    }
    advance(&mut renderer, 4);
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
    assert!(!session.player().unwrap().preview_monitor().playing(), "nothing was started");
}

#[test]
fn launcher_state_reports_the_cpu_load() {
    let (mut session, mut renderer) = playing();
    advance(&mut renderer, 256 * 4);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    let cpu = state["cpu"].as_f64().unwrap();
    assert!((0.0..=100.0).contains(&cpu), "{cpu}");
    // The monitor the player reads is the one the renderer writes.
    advance(&mut renderer, 2);
    assert_eq!(session.player().unwrap().cpu_monitor().load_milli(), renderer.cpu_monitor().load_milli());
}

#[test]
fn a_loop_turned_on_while_playing_repeats_its_span() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "transport.setLoop", json!({ "start": 1.0, "end": 2.0, "enabled": true })).unwrap();
    run(&mut session, "transport.play", json!({})).unwrap();
    // 3.5 beats in: past the loop end twice, so half a beat into the loop.
    advance(&mut renderer, 7 * BEAT / 2);
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!((state["running"].as_bool(), state["position"].as_str()), (Some(true), Some("1.2")));

    // Off: playback carries on past the loop end.
    run(&mut session, "transport.setLoopEnabled", json!({ "enabled": false })).unwrap();
    advance(&mut renderer, BEAT);
    advance(&mut renderer, 1);
    assert_eq!(run(&mut session, "launcher.state", json!({})).unwrap()["position"], "1.3");
}

#[test]
fn a_slot_clip_is_deleted_by_scene_and_undo_brings_it_back() {
    let mut session = Session::new(project());
    run(&mut session, "clip.create", json!({ "track": 3, "scene": 2 })).unwrap();
    // Deleted by scene: the slot is empty again.
    let deleted = run(&mut session, "clip.delete", json!({ "track": 3, "scene": 2 })).unwrap();
    assert_eq!(deleted["deleted"], json!(2));
    let column = session.project().launcher.columns.iter().find(|c| c.track == TrackId(3)).unwrap();
    assert!(column.slots[1].clip.is_none());
    // An empty slot is refused, and so is an index next to the scene.
    assert!(run(&mut session, "clip.delete", json!({ "track": 3, "scene": 2 })).is_err());
    assert!(run(&mut session, "clip.delete", json!({ "track": 3, "scene": 2, "index": 0 })).is_err());
    // One undo puts the clip back.
    run(&mut session, "edit.undo", json!({})).unwrap();
    let column = session.project().launcher.columns.iter().find(|c| c.track == TrackId(3)).unwrap();
    assert!(column.slots[1].clip.is_some());
}

#[test]
fn stopping_a_scene_stops_its_rows_clips_and_leaves_the_empty_slots_tracks() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "scene.launch", json!({ "scene": 2 })).unwrap();
    advance(&mut renderer, 100);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["tracks"][0]["playing"]["scene"], 2);
    assert_eq!(state["tracks"][1]["playing"]["clip"], "Line");

    // The stop queues on every track with a clip in the row, and fires at the next bar.
    let stopped = run(&mut session, "scene.stop", json!({ "scene": 2 })).unwrap();
    assert_eq!(stopped["queued"], "stop");
    // The command reaches the engine with the next audio block.
    advance(&mut renderer, 1);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["tracks"][0]["queued"], "stop");
    assert_eq!(state["tracks"][1]["queued"], "stop");
    advance(&mut renderer, 4 * BEAT);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["tracks"][0]["playing"], Value::Null);
    assert_eq!(state["tracks"][1]["playing"], Value::Null);
    // Scene 2's slot on track 3 is empty without a stop button: it never played and is not
    // touched now either.
    assert_eq!(state["tracks"][2]["playing"], Value::Null);
    assert_eq!(state["tracks"][2]["queued"], Value::Null);
    // A scene that does not exist is refused.
    assert!(run(&mut session, "scene.stop", json!({ "scene": 99 })).is_err());
}

/// Pressing play and stop quickly (#506) costs the session next to nothing: each press is a
/// message to the audio thread, with no undo step, no change to the project (so no backup or
/// save follows) and no new graph. Prints what a press took; the numbers in #506 come from
/// `cargo test -p suhina-session --release --test launcher quick_presses -- --nocapture`.
#[test]
fn quick_presses_of_play_and_stop_cost_the_session_next_to_nothing() {
    let (mut session, mut renderer) = playing();
    let before = session.project().clone();
    let mut took = Vec::with_capacity(1000);
    for n in 0..1000 {
        let id = if n % 2 == 0 { "transport.play" } else { "transport.stop" };
        let started = std::time::Instant::now();
        run(&mut session, id, json!({})).unwrap();
        took.push(started.elapsed());
        advance(&mut renderer, 256);
        assert_eq!(renderer.monitor().running(), n % 2 == 0, "press {n} reached the audio thread");
    }
    assert!(!session.can_undo(), "a press is not an undo step");
    assert!(session.project() == &before, "a press does not change the project");
    took.sort();
    println!("1000 presses of play and stop: median {:?}, 99 % {:?}, longest {:?}", took[500], took[990], took[999]);
}

#[test]
fn a_slot_clip_moves_to_another_slot_and_swaps_an_occupied_one() {
    let mut session = Session::new(project());
    // The fixture: track 1 scene 1 and track 2 scene 2 hold clips; track 2 scene 1 is empty.
    let clip = |session: &Session, track: u32, scene: usize| {
        session
            .project()
            .launcher
            .columns
            .iter()
            .find(|c| c.track == TrackId(track))
            .and_then(|c| c.slots.get(scene))
            .and_then(|s| s.clip.as_ref())
            .map(|c| c.content)
    };
    let (first, second) = (clip(&session, 1, 0).unwrap(), clip(&session, 2, 1).unwrap());

    // Moved to an occupied slot: the two swap, nothing lost.
    let moved =
        run(&mut session, "clip.moveSlot", json!({ "track": 1, "scene": 1, "toTrack": 2, "toScene": 2 })).unwrap();
    assert_eq!((moved["moved"].clone(), moved["swapped"].clone()), (json!(true), json!(true)));
    assert_eq!(clip(&session, 2, 1), Some(first), "the first's clip is in the target now");
    assert_eq!(clip(&session, 1, 0), Some(second), "the target's clip came back to the source's slot");
    // One undo step restores both.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(clip(&session, 1, 0), Some(first));
    assert_eq!(clip(&session, 2, 1), Some(second));

    // Moved to an empty slot: the source empties.
    let moved =
        run(&mut session, "clip.moveSlot", json!({ "track": 1, "scene": 1, "toTrack": 2, "toScene": 1 })).unwrap();
    assert_eq!((moved["moved"].clone(), moved["swapped"].clone()), (json!(true), json!(false)));
    assert_eq!(clip(&session, 1, 0), None, "the source slot is empty");
    assert_eq!(clip(&session, 2, 0), Some(first), "the clip is in the target now");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(clip(&session, 1, 0), Some(first));

    // The same slot: nothing.
    let moved = run(&mut session, "clip.moveSlot", json!({ "track": 1, "scene": 1, "toScene": 1 })).unwrap();
    assert_eq!(moved["moved"], json!(false));

    // An empty source is refused.
    assert!(run(&mut session, "clip.moveSlot", json!({ "track": 2, "scene": 1, "toScene": 2 })).is_err());

    // The target's kind must take the clip's: an audio track takes no MIDI clips.
    let mut p = project();
    p.tracks[0].content = ContentKinds { notes: false, audio: true };
    let mut session = Session::new(p);
    let moved = run(&mut session, "clip.moveSlot", json!({ "track": 2, "scene": 2, "toTrack": 1, "toScene": 2 }));
    assert!(moved.is_err(), "an audio track takes no MIDI clips");
}

#[test]
fn a_clip_is_renamed_by_scene_or_index_and_undo_brings_the_name_back() {
    let mut session = Session::new(project());
    // A slot's clip renamed by scene: its content's name is the clip's. The fixture's track 1
    // slots share one content, so the two slots' clips share the name, as the model has it.
    let renamed = run(&mut session, "clip.rename", json!({ "track": 1, "scene": 1, "name": "Drop the beat" })).unwrap();
    assert_eq!(renamed["name"], "Drop the beat");
    let p = session.project();
    let names = |p: &Project, track: u32| -> Vec<String> {
        p.launcher
            .columns
            .iter()
            .find(|c| c.track == TrackId(track))
            .map(|c| {
                c.slots
                    .iter()
                    .flat_map(|s| s.clip.as_ref())
                    .map(|c| p.content(c.content).map(|c| c.name.clone()).unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default()
    };
    assert_eq!(names(p, 1), vec!["Drop the beat".to_string(); 2], "the shared content's name is both slots'");
    // An arrangement clip renamed by index: the fixture's arrangement is empty, so a lane with
    // its own clip goes in first.
    let mut p = project();
    p.arrangement.lanes.push(ArrangementLane {
        name: "Drums".into(),
        track: Some(TrackId(1)),
        clips: vec![Clip { loop_range: Some(LoopRange { start: 0.0, end: 1.0 }), ..Clip::new(ContentId(1), 0.0, 1.0) }],
        ..ArrangementLane::default()
    });
    let mut session = Session::new(p);
    run(&mut session, "clip.rename", json!({ "track": 1, "index": 0, "name": "Intro take" })).unwrap();
    let p = session.project();
    let arranged = &p.arrangement.lanes[0].clips[0];
    assert_eq!(p.content(arranged.content).unwrap().name, "Intro take");

    // An empty name clears it; the clip named by its id this time.
    let id = session.project().arrangement.lanes[0].clips[0].id;
    run(&mut session, "clip.rename", json!({ "clip": id.0, "name": "" })).unwrap();
    let p = session.project();
    let arranged = &p.arrangement.lanes[0].clips[0];
    assert_eq!(p.content(arranged.content).unwrap().name, "");

    // One undo brings the cleared name back, and another, the slot's rename.
    run(&mut session, "edit.undo", json!({})).unwrap();
    let p = session.project();
    let arranged = &p.arrangement.lanes[0].clips[0];
    assert_eq!(p.content(arranged.content).unwrap().name, "Intro take");
    run(&mut session, "edit.undo", json!({})).unwrap();
    let p = session.project();
    let slot = p.launcher.columns.iter().find(|c| c.track == TrackId(1)).and_then(|c| c.slots.first()).unwrap();
    assert_eq!(
        p.content(slot.clip.as_ref().unwrap().content).unwrap().name,
        "Beat",
        "the undo walks back to the name the fixture's content had"
    );
}
