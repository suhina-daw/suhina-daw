// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Output routing: `track.setOutput` sends a track's audio to the main output or into a group
//! track, as one undo step, refuses loops and targets that are not groups, and the route it sets
//! is what the engine plays.

#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use serde_json::json;
use suhina_model::*;
use suhina_session::Session;

use crate::support::run;

/// Bass (notes, one clip) into Main; two groups, Group and Outer, both into Main; and a return.
fn project() -> Project {
    suhina_test_support::with_ids(project_built())
}

fn project_built() -> Project {
    let track = |id: u32, name: &str, notes: bool| Track {
        id: TrackId(id),
        name: name.into(),
        color: None,
        content: ContentKinds { notes, audio: false },
        channel: Some(ChannelId(id)),
        parent: None,
        extensions: Extensions::default(),
        audio_input: Default::default(),
        midi_input: Default::default(),
    };
    let channel = |id: u32, role: ChannelRole| Channel {
        id: ChannelId(id),
        name: format!("C{id}"),
        role,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        solo: false,
        cue: false,
        output: None,
        sends: Vec::new(),
        devices: Vec::new(),
        automation: Vec::new(),
        extensions: Extensions::default(),
    };
    Project {
        tracks: vec![
            track(1, "Bass", true),
            track(2, "Group", false),
            track(3, "Outer", false),
            track(4, "A Return", false),
        ],
        channels: vec![
            channel(1, ChannelRole::Regular),
            channel(2, ChannelRole::Submix),
            channel(3, ChannelRole::Submix),
            channel(4, ChannelRole::Effect),
        ],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "Riff".into(),
            length: 1.0,
            lanes: vec![ContentLane {
                target: None,
                data: LaneData::Notes(vec![Note {
                    time: 0.0,
                    duration: 1.0,
                    key: 69,
                    velocity: 1.0,
                    release_velocity: 0.5,
                    channel: 0,
                    muted: false,
                }]),
            }],
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "Bass".into(),
                track: Some(TrackId(1)),
                clips: vec![Clip {
                    id: Default::default(),
                    content: ContentId(1),
                    start: 0.0,
                    length: 1.0,
                    offset: 0.0,
                    loop_range: None,
                    enabled: true,
                    fade_in: 0.0,
                    fade_out: 0.0,
                    fade_in_curve: Default::default(),
                    fade_out_curve: Default::default(),
                    follow_action: Default::default(),
                    launch: Default::default(),
                    color: None,
                    extensions: Extensions::default(),
                }],
            }],
            markers: Vec::new(),
        },
        ..Project::default()
    }
}

fn output(session: &Session, channel: u32) -> Option<ChannelId> {
    session.project().channel(ChannelId(channel)).and_then(|c| c.output)
}

/// The loudest sample of the left and of the right channel (both the same here: nothing is panned).
fn peaks(project: &Project) -> (f32, f32) {
    let samples = suhina_engine::render_offline(project, 48_000, 512).unwrap();
    samples.as_chunks::<2>().0.iter().fold((0.0f32, 0.0f32), |(l, r), [a, b]| (l.max(a.abs()), r.max(b.abs())))
}

#[test]
fn each_route_is_one_undo_step() {
    let mut session = Session::new(project());
    assert_eq!(run(&mut session, "track.setOutput", json!({ "track": "Bass", "to": "Group" })).unwrap()["to"], "Group");
    assert_eq!(output(&session, 1), Some(ChannelId(2)));
    // A group goes by its id too.
    assert_eq!(run(&mut session, "track.setOutput", json!({ "track": 1, "to": 3 })).unwrap()["to"], "Outer");
    assert_eq!(output(&session, 1), Some(ChannelId(3)));
    assert_eq!(run(&mut session, "track.setOutput", json!({ "track": "Bass", "to": "Main" })).unwrap()["to"], "main");
    assert_eq!(output(&session, 1), None);

    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(output(&session, 1), Some(ChannelId(3)));
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(output(&session, 1), Some(ChannelId(2)));
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &project());
    assert!(!session.can_undo());
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(output(&session, 1), Some(ChannelId(2)));
}

#[test]
fn loops_unknown_targets_and_tracks_that_are_not_groups_are_refused() {
    let mut session = Session::new(project());
    run(&mut session, "track.setOutput", json!({ "track": "Group", "to": "Outer" })).unwrap();
    let before = session.project().clone();
    for (params, why) in [
        (json!({ "track": "Group", "to": "Group" }), "into itself"),
        (json!({ "track": "Outer", "to": "Group" }), "a loop through another group"),
        (json!({ "track": "Bass", "to": "Nope" }), "a name no track has"),
        (json!({ "track": "Bass", "to": 99 }), "an id no track has"),
        (json!({ "track": "Bass", "to": "A Return" }), "a return track"),
        (json!({ "track": "Bass", "to": "Bass" }), "a track that is not a group"),
        (json!({ "track": "Bass" }), "no target"),
        (json!({ "track": "Bass", "to": true }), "not a track"),
        (json!({ "track": "Nope", "to": "main" }), "an unknown track"),
    ] {
        assert!(run(&mut session, "track.setOutput", params).is_err(), "{why} is refused");
        assert_eq!(session.project(), &before, "{why} changes nothing");
    }
    // Only the one route made above is on the undo history.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(!session.can_undo());
}

#[test]
fn a_track_routed_into_a_group_is_heard_through_it() {
    let mut session = Session::new(project());
    let (alone, _) = peaks(session.project());
    assert!(alone > 0.1, "Bass plays: {alone}");

    // The group's fader now applies: at -6 dB, half as loud.
    run(&mut session, "track.setVolume", json!({ "tracks": ["Group"], "db": -6.0206 })).unwrap();
    assert!((peaks(session.project()).0 - alone).abs() < 1e-4, "into Main, the group's fader does nothing");
    run(&mut session, "track.setOutput", json!({ "track": "Bass", "to": "Group" })).unwrap();
    let (left, right) = peaks(session.project());
    assert!(
        (left - alone * 0.5).abs() < 1e-3 && (right - alone * 0.5).abs() < 1e-3,
        "through the group: {left} {right}"
    );

    // A muted group silences what plays into it; back to Main it is heard again.
    run(&mut session, "track.setMute", json!({ "tracks": ["Group"], "mute": true })).unwrap();
    assert_eq!(peaks(session.project()), (0.0, 0.0));
    run(&mut session, "track.setOutput", json!({ "track": "Bass", "to": "main" })).unwrap();
    assert!((peaks(session.project()).0 - alone).abs() < 1e-4);
}
