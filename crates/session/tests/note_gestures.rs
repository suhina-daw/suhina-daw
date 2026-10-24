// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The params the piano roll's gestures added to the `note.*` commands (#351): `note.resize` with
//! `startBy` (the left edge), `note.setVelocity` with `velocities` (the velocity line) and
//! `note.duplicate` with `beats` and `semitones` (Ctrl+drag copies); one undo step each, and bad
//! input refused (`docs/spec/ui-clip-editor.md` › Commands).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use serde_json::json;
use suhina_model::*;
use suhina_session::{MAX_NOTES_PER_COMMAND, Session};

use crate::support::run;

fn note(time: f64, key: u8) -> Note {
    Note { time, duration: 1.0, key, velocity: 100.0 / 127.0, release_velocity: 0.5, channel: 0, muted: false }
}

fn clip(content: u32, start: f64) -> Clip {
    Clip {
        id: Default::default(),
        content: ContentId(content),
        start,
        length: 4.0,
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
    }
}

fn content(id: u32, lanes: Vec<ContentLane>) -> ClipContent {
    ClipContent {
        id: ContentId(id),
        name: format!("C{id}"),
        length: 4.0,
        lanes,
        extensions: Extensions::default(),
        scale: None,
    }
}

/// Track 1 "Keys" has, on the arrangement, a MIDI clip with three notes (index 0), an empty MIDI
/// clip (index 1), an audio clip (index 2) and a pattern for another track (index 3); in scene 1
/// a slot with a MIDI clip and in scene 2 an empty slot.
fn project() -> Project {
    suhina_test_support::with_ids(project_built())
}

fn project_built() -> Project {
    let track = |id: u32, name: &str| Track {
        id: TrackId(id),
        name: name.into(),
        color: None,
        content: ContentKinds { notes: true, audio: true },
        channel: None,
        parent: None,
        extensions: Extensions::default(),
        audio_input: Default::default(),
        midi_input: Default::default(),
    };
    let notes = |n: Vec<Note>| ContentLane { target: None, data: LaneData::Notes(n) };
    let audio = AudioRegion {
        file: "a.wav".into(),
        absolute_file: None,
        sample_rate: None,
        channels: None,
        duration_seconds: None,
        warped: false,
        stretch: StretchMode::default(),
        warp_markers: Vec::new(),
        gain: 1.0,
        transpose: 0.0,
        stream: None,
    };
    Project {
        tracks: vec![track(1, "Keys"), track(2, "Drums")],
        contents: vec![
            content(1, vec![notes(vec![note(0.0, 60), note(1.0, 64), note(2.0, 67)])]),
            content(2, Vec::new()),
            content(3, vec![ContentLane { target: None, data: LaneData::Audio(audio) }]),
            content(4, vec![ContentLane { target: Some(TrackId(2)), data: LaneData::Notes(vec![note(0.0, 36)]) }]),
            content(5, vec![notes(vec![note(0.5, 48)])]),
        ],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "Keys".into(),
                track: Some(TrackId(1)),
                clips: vec![clip(1, 0.0), clip(2, 4.0), clip(3, 8.0), clip(4, 12.0)],
            }],
            markers: Vec::new(),
        },
        launcher: Launcher {
            scenes: vec![Scene::default(), Scene::default()],
            columns: vec![LauncherColumn {
                track: TrackId(1),
                slots: vec![Slot { clip: Some(clip(5, 0.0)), has_stop: true }, Slot::default()],
            }],
            mappings: Default::default(),
        },
        ..Project::default()
    }
}

fn notes(session: &Session, content: u32) -> Vec<Note> {
    let content = session.project().content(ContentId(content)).expect("the content");
    content
        .lanes
        .iter()
        .find_map(|l| match &l.data {
            LaneData::Notes(n) => Some(n.clone()),
            LaneData::Audio(_) => None,
        })
        .unwrap_or_default()
}

/// Each edit below is one undo step that puts the notes back as they were.
fn undoes(session: &mut Session, content: u32, before: &[Note]) {
    run(session, "edit.undo", json!({})).unwrap();
    assert_eq!(notes(session, content), before);
}

#[test]
fn start_by_moves_the_starts_and_keeps_the_ends() {
    let mut session = Session::new(project());
    let before = notes(&session, 1);
    run(&mut session, "note.resize", json!({ "track": 1, "index": 0, "notes": [1, 2], "startBy": 0.25 })).unwrap();
    let now = notes(&session, 1);
    assert_eq!(now.iter().map(|n| (n.time, n.duration)).collect::<Vec<_>>(), [(0.0, 1.0), (1.25, 0.75), (2.25, 0.75)]);
    undoes(&mut session, 1, &before);
    // Earlier: the notes grow at the front, their ends where they were.
    run(&mut session, "note.resize", json!({ "track": 1, "index": 0, "notes": [1], "startBy": -0.5 })).unwrap();
    assert_eq!((notes(&session, 1)[1].time, notes(&session, 1)[1].duration), (0.5, 1.5));
    undoes(&mut session, 1, &before);
    // A hair before 0 lands on 0.
    run(&mut session, "note.resize", json!({ "track": 1, "index": 0, "notes": [1], "startBy": -1.0000000001 }))
        .unwrap();
    assert_eq!(notes(&session, 1)[1].time, 0.0);
}

#[test]
fn malformed_start_by_is_refused_and_changes_nothing() {
    let mut session = Session::new(project());
    let before = session.project().clone();
    let cases = [
        (json!({ "startBy": -0.5 }), "outside 0"),
        (json!({ "startBy": 1.0 }), "beats long"),
        (json!({ "startBy": 2.0 }), "beats long"),
        (json!({ "startBy": 1e300 }), "startBy"),
        (json!({ "startBy": "earlier" }), "startBy"),
        (json!({ "startBy": 0.5, "by": 1.0 }), "one of them"),
        (json!({ "startBy": 0.5, "length": 1.0 }), "one of them"),
    ];
    for (extra, error) in cases {
        let mut params = json!({ "track": 1, "index": 0, "notes": [0] });
        params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let got = run(&mut session, "note.resize", params.clone());
        assert!(got.as_ref().is_err_and(|e| e.contains(error)), "{params}: {got:?}");
    }
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
}

#[test]
fn velocities_set_one_value_per_note_in_one_step() {
    let mut session = Session::new(project());
    let before = notes(&session, 1);
    let velocity =
        |s: &Session| notes(s, 1).iter().map(|n| suhina_session::midi_velocity(n.velocity)).collect::<Vec<_>>();
    // In the order the notes are given, not their index order.
    run(&mut session, "note.setVelocity", json!({ "track": 1, "index": 0, "notes": [2, 0], "velocities": [30, 90] }))
        .unwrap();
    assert_eq!(velocity(&session), [90, 100, 30]);
    undoes(&mut session, 1, &before);
    // A repeated index takes its last value.
    run(&mut session, "note.setVelocity", json!({ "track": 1, "index": 0, "notes": [1, 1], "velocities": [5, 6] }))
        .unwrap();
    assert_eq!(velocity(&session), [100, 6, 100]);
}

#[test]
fn malformed_velocities_are_refused_and_change_nothing() {
    let mut session = Session::new(project());
    let before = session.project().clone();
    let many: Vec<u32> = vec![64; MAX_NOTES_PER_COMMAND + 1];
    let cases = [
        (json!({ "velocities": [1, 2] }), "one value per note"),
        (json!({ "velocities": [] }), "one value per note"),
        (json!({ "velocities": [0] }), "1 to 127"),
        (json!({ "velocities": [128] }), "1 to 127"),
        (json!({ "velocities": ["loud"] }), "1 to 127"),
        (json!({ "velocities": 64 }), "list"),
        (json!({ "velocities": [64], "velocity": 64 }), "one of them"),
        (json!({ "velocities": [64], "by": 3 }), "one of them"),
        (json!({ "velocities": many }), "at most"),
    ];
    for (extra, error) in cases {
        let mut params = json!({ "track": 1, "index": 0, "notes": [0] });
        params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let got = run(&mut session, "note.setVelocity", params.clone());
        assert!(got.as_ref().is_err_and(|e| e.contains(error)), "{extra}: {got:?}");
    }
    let missing =
        run(&mut session, "note.setVelocity", json!({ "track": 1, "index": 0, "notes": [7], "velocities": [9] }));
    assert!(missing.unwrap_err().contains("does not exist"));
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
}

#[test]
fn a_duplicate_can_go_beats_and_semitones_away() {
    let mut session = Session::new(project());
    let before = notes(&session, 1);
    let result = run(
        &mut session,
        "note.duplicate",
        json!({ "track": 1, "index": 0, "notes": [0, 1], "beats": 4, "semitones": 2 }),
    )
    .unwrap();
    assert_eq!(result, json!({ "added": [3, 4], "count": 5 }));
    let now = notes(&session, 1);
    assert_eq!(&now[..3], &before[..], "the originals stay");
    assert_eq!(now.iter().skip(3).map(|n| (n.time, n.key)).collect::<Vec<_>>(), [(4.0, 62), (5.0, 66)]);
    undoes(&mut session, 1, &before);
    // A muted note's copy is muted too: the copy is the note, moved.
    run(&mut session, "note.setMuted", json!({ "track": 1, "index": 0, "notes": [2], "muted": true })).unwrap();
    run(&mut session, "note.duplicate", json!({ "track": 1, "index": 0, "notes": [2], "semitones": -12 })).unwrap();
    let now = notes(&session, 1);
    assert_eq!((now[3].time, now[3].key, now[3].muted), (2.0, 55, true));
}

#[test]
fn malformed_duplicate_offsets_are_refused_and_change_nothing() {
    let mut session = Session::new(project());
    let before = session.project().clone();
    let cases = [
        (json!({ "beats": -1 }), "outside 0"),
        (json!({ "beats": 1e300 }), "beats"),
        (json!({ "beats": "later" }), "beats"),
        (json!({ "semitones": 61 }), "keys 0 to 127"),
        (json!({ "semitones": -61 }), "keys 0 to 127"),
        (json!({ "semitones": 0.5 }), "semitones"),
        (json!({ "semitones": 500 }), "semitones"),
    ];
    for (extra, error) in cases {
        let mut params = json!({ "track": 1, "index": 0, "notes": [0, 2] });
        params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        let got = run(&mut session, "note.duplicate", params.clone());
        assert!(got.as_ref().is_err_and(|e| e.contains(error)), "{params}: {got:?}");
    }
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
}
