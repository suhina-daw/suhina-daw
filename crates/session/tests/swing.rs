// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Swing: `transport.setSwing` and `clip.setSwing`, one undo step each, refusing bad input and
//! changing nothing then; a MIDI clip follows the project's swing until it gets its own, and keeps
//! its own while it follows the project's again (`docs/spec/swing.md`, #1221).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use serde_json::json;
use suhina_model::*;
use suhina_session::Session;

use crate::support::run;

/// Track 1 "Keys" with a MIDI clip (index 0) and an audio clip (index 1) on the arrangement.
fn project() -> Project {
    let content = |id: u32, lanes: Vec<ContentLane>| ClipContent {
        id: ContentId(id),
        name: format!("C{id}"),
        length: 4.0,
        lanes: lanes.into(),
        extensions: Extensions::default(),
        scale: None,
    };
    let notes = vec![Note::new(0.0, 0.25, 60, 0.8), Note::new(0.25, 0.25, 62, 0.8)];
    suhina_test_support::with_ids(Project {
        tracks: vec![Track { content: ContentKinds { notes: true, audio: true }, ..Track::new(TrackId(1), "Keys") }],
        contents: vec![
            content(1, vec![ContentLane { target: None, data: LaneData::Notes(notes) }]),
            content(2, vec![ContentLane { target: None, data: LaneData::Audio(AudioRegion::new("a.wav")) }]),
        ],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "Keys".into(),
                track: Some(TrackId(1)),
                clips: vec![Clip::new(ContentId(1), 0.0, 4.0), Clip::new(ContentId(2), 4.0, 4.0)],
            }],
            markers: Vec::new(),
            takes: Vec::new(),
        },
        ..Project::default()
    })
}

fn swing_of(session: &Session) -> ClipSwing {
    session.project().arrangement.lanes[0].clips[0].swing
}

#[test]
fn the_projects_swing_is_set_in_percent_and_undone_in_one_step() {
    let mut session = Session::new(project());
    let result = run(&mut session, "transport.setSwing", json!({ "amount": 30 })).unwrap();
    assert_eq!(result, json!({ "amount": 30.0 }));
    assert!((session.project().transport.swing - 0.3).abs() < 1e-12);
    for bad in [json!({ "amount": 101 }), json!({ "amount": -1 }), json!({})] {
        assert!(run(&mut session, "transport.setSwing", bad.clone()).is_err(), "{bad}");
    }
    assert!((session.project().transport.swing - 0.3).abs() < 1e-12, "a refused call changes nothing");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().transport.swing, 0.0);
    assert!(!session.can_undo());
}

#[test]
fn a_clip_follows_the_projects_swing_until_it_has_its_own_and_keeps_it() {
    let mut session = Session::new(project());
    run(&mut session, "transport.setSwing", json!({ "amount": 30 })).unwrap();
    assert_eq!(swing_of(&session), ClipSwing::default(), "a clip follows the project's at first");
    // Its own amount: the clip plays it.
    let result = run(&mut session, "clip.setSwing", json!({ "track": 1, "index": 0, "amount": 55 })).unwrap();
    assert_eq!(result, json!({ "amount": 55.0, "global": false, "plays": 55.0 }));
    // Back to the project's: it plays 30 and keeps 55 for later.
    let result = run(&mut session, "clip.setSwing", json!({ "track": 1, "index": 0, "global": true })).unwrap();
    assert_eq!(result, json!({ "amount": 55.0, "global": true, "plays": 30.0 }));
    assert!((swing_of(&session).amount - 0.55).abs() < 1e-12);
    // The project's swing changes what it plays; its own amount stays.
    run(&mut session, "transport.setSwing", json!({ "amount": 70 })).unwrap();
    assert!((swing_of(&session).effective(session.project().transport.swing) - 0.7).abs() < 1e-12);
    // Its own again: 55 is back.
    let result = run(&mut session, "clip.setSwing", json!({ "track": 1, "index": 0, "global": false })).unwrap();
    assert_eq!(result["plays"], json!(55.0));
    // One undo step each.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(!swing_of(&session).own);
}

#[test]
fn clip_swing_refuses_bad_input_and_audio_clips() {
    let mut session = Session::new(project());
    for bad in [
        json!({ "track": 1, "index": 0 }),
        json!({ "track": 1, "index": 0, "amount": 150 }),
        json!({ "track": 1, "index": 0, "amount": 20, "global": true }),
        json!({ "track": 1, "index": 0, "global": "yes" }),
        json!({ "track": 1, "index": 1, "amount": 20 }),
    ] {
        assert!(run(&mut session, "clip.setSwing", bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(*session.project(), project(), "a refused call changes nothing");
    assert!(!session.can_undo());
}
