// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `note.quantize`: moving notes' starts (and ends) towards a grid, one undo step, and refusing
//! bad input (`docs/spec/ui-clip-editor.md` › Quantise).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use serde_json::{Value, json};
use suhina_model::*;
use suhina_session::Session;

fn note(time: f64, duration: f64) -> Note {
    Note::new(time, duration, 60, 100.0 / 127.0)
}

fn clip(content: u32) -> Clip {
    Clip::new(ContentId(content), 0.0, 8.0)
}

/// Track 1 has, on the arrangement, a MIDI clip with `notes` (index 0) and an empty MIDI clip
/// (index 1), and in scene 1 a slot with the same notes.
fn project(notes: Vec<Note>) -> Project {
    suhina_test_support::with_ids(project_built(notes))
}

fn project_built(notes: Vec<Note>) -> Project {
    let content = |id: u32, lanes: Vec<ContentLane>| ClipContent {
        id: ContentId(id),
        name: format!("C{id}"),
        length: 8.0,
        lanes,
        extensions: Extensions::default(),
        scale: None,
    };
    let lane = |n: Vec<Note>| vec![ContentLane { target: None, data: LaneData::Notes(n) }];
    Project {
        tracks: vec![Track { content: ContentKinds { notes: true, audio: false }, ..Track::new(TrackId(1), "Keys") }],
        contents: vec![content(1, lane(notes.clone())), content(2, Vec::new()), content(3, lane(notes))],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "Keys".into(),
                track: Some(TrackId(1)),
                clips: vec![clip(1), clip(2)],
            }],
            markers: Vec::new(),
        },
        launcher: Launcher {
            scenes: vec![Scene::default()],
            columns: vec![LauncherColumn {
                track: TrackId(1),
                slots: vec![Slot { clip: Some(clip(3)), has_stop: true }],
            }],
            mappings: Default::default(),
        },
        ..Project::default()
    }
}

fn run(session: &mut Session, params: Value) -> Result<Value, String> {
    session.execute("note.quantize", &params).map_err(String::from)
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

fn starts(session: &Session) -> Vec<f64> {
    notes(session, 1).iter().map(|n| n.time).collect()
}

fn lengths(session: &Session) -> Vec<f64> {
    notes(session, 1).iter().map(|n| n.duration).collect()
}

fn close(got: &[f64], want: &[f64]) {
    assert_eq!(got.len(), want.len(), "{got:?} {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() < 1e-9, "{got:?} is not {want:?}");
    }
}

#[test]
fn a_straight_grid_moves_every_start_to_the_nearest_line_and_keeps_lengths() {
    let mut session = Session::new(project(vec![note(0.1, 0.5), note(0.9, 0.3), note(1.6, 1.0), note(2.37, 0.2)]));
    let before = notes(&session, 1);
    let result = run(&mut session, json!({ "track": 1, "index": 0, "grid": "1/4" })).unwrap();
    assert_eq!(result, json!({ "notes": [0, 1, 2, 3], "moved": 4 }));
    close(&starts(&session), &[0.0, 1.0, 2.0, 2.0]);
    close(&lengths(&session), &[0.5, 0.3, 1.0, 0.2]);
    // One undo step puts every note back.
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(notes(&session, 1), before);
    assert!(!session.can_undo());
    // A finer grid: the sixteenth nearest to each.
    run(&mut session, json!({ "track": 1, "index": 0, "grid": "1/16" })).unwrap();
    close(&starts(&session), &[0.0, 1.0, 1.5, 2.25]);
}

#[test]
fn a_triplet_grid_uses_thirds_of_the_note_value() {
    // An eighth triplet is a third of a beat.
    let mut session = Session::new(project(vec![note(0.3, 0.25), note(0.7, 0.25), note(1.2, 0.25)]));
    run(&mut session, json!({ "track": 1, "index": 0, "grid": "1/8T" })).unwrap();
    close(&starts(&session), &[1.0 / 3.0, 2.0 / 3.0, 4.0 / 3.0]);
    // Bars follow the time signature: in 3/4 a bar is three beats.
    let mut waltz = project(vec![note(1.4, 1.0), note(4.6, 1.0)]);
    waltz.transport.time_signature = TimeSignature { numerator: 3, denominator: 4 };
    let mut session = Session::new(waltz);
    run(&mut session, json!({ "track": 1, "index": 0, "grid": "1 bar" })).unwrap();
    close(&starts(&session), &[0.0, 6.0]);
}

#[test]
fn an_amount_moves_part_of_the_way() {
    let mut session = Session::new(project(vec![note(0.2, 1.0), note(1.9, 1.0)]));
    run(&mut session, json!({ "track": 1, "index": 0, "grid": "1/4", "amount": 50 })).unwrap();
    close(&starts(&session), &[0.1, 1.95]);
    // 0 % changes nothing and adds no undo step.
    let mut session = Session::new(project(vec![note(0.2, 1.0)]));
    let result = run(&mut session, json!({ "track": 1, "index": 0, "grid": "1/4", "amount": 0 })).unwrap();
    assert_eq!(result["moved"], 0);
    assert!(!session.can_undo());
}

#[test]
fn ends_move_to_the_nearest_line_too_and_never_onto_the_start() {
    let mut session = Session::new(project(vec![note(0.1, 0.8), note(1.02, 0.05), note(2.2, 1.7)]));
    run(&mut session, json!({ "track": 1, "index": 0, "grid": "1/16", "ends": true })).unwrap();
    close(&starts(&session), &[0.0, 1.0, 2.25]);
    // 0.9 ends on 1.0; 1.07 would end on its start, so it ends a line later; 3.9 on 4.0.
    close(&lengths(&session), &[1.0, 0.25, 1.75]);
    // Half way: both edges move half the distance.
    let mut session = Session::new(project(vec![note(0.1, 0.8)]));
    run(&mut session, json!({ "track": 1, "index": 0, "grid": "1/16", "ends": true, "amount": 50 })).unwrap();
    close(&starts(&session), &[0.05]);
    close(&lengths(&session), &[0.9]);
}

#[test]
fn notes_on_the_grid_stay_exactly_where_they_are() {
    let on = vec![note(0.0, 0.5), note(1.0 / 3.0, 1.0 / 3.0), note(2.5, 0.25)];
    let mut session = Session::new(project(on.clone()));
    let result = run(&mut session, json!({ "track": 1, "index": 0, "grid": "1/8T" })).unwrap();
    assert_eq!(result["moved"], 1, "only 2.5 is off an eighth-triplet line");
    let mut session = Session::new(project(on.clone()));
    let result = run(&mut session, json!({ "track": 1, "index": 0, "grid": "1/8", "ends": false })).unwrap();
    // 1/3 is off the eighths; 0 and 2.5 are on them and keep their exact values.
    assert_eq!(result["moved"], 1);
    assert_eq!(notes(&session, 1)[0], on[0]);
    assert_eq!(notes(&session, 1)[2], on[2]);
}

#[test]
fn only_the_named_notes_move_and_a_slot_clip_quantises_too() {
    let mut session = Session::new(project(vec![note(0.1, 1.0), note(1.1, 1.0), note(2.1, 1.0)]));
    let result = run(&mut session, json!({ "track": 1, "index": 0, "notes": [2, 0, 2], "grid": "1/4" })).unwrap();
    assert_eq!(result, json!({ "notes": [0, 2], "moved": 2 }));
    close(&starts(&session), &[0.0, 1.1, 2.0]);
    run(&mut session, json!({ "track": "Keys", "scene": 1, "grid": "1/4" })).unwrap();
    close(&notes(&session, 3).iter().map(|n| n.time).collect::<Vec<_>>(), &[0.0, 1.0, 2.0]);
    // An empty clip has nothing to move, and stays as it was.
    let before = session.project().clone();
    assert_eq!(run(&mut session, json!({ "track": 1, "index": 1, "grid": "1/4" })).unwrap()["moved"], 0);
    assert_eq!(session.project(), &before);
}

#[test]
fn malformed_params_are_refused_and_change_nothing() {
    let mut session = Session::new(project(vec![note(0.1, 1.0)]));
    let before = session.project().clone();
    let cases = [
        (json!({ "track": 1, "index": 0 }), "grid must be"),
        (json!({ "track": 1, "index": 0, "grid": 16 }), "grid must be"),
        (json!({ "track": 1, "index": 0, "grid": "1/5" }), "unknown quantisation"),
        (json!({ "track": 1, "index": 0, "grid": "none" }), "no lines"),
        (json!({ "track": 1, "index": 0, "grid": "1/4", "amount": 101 }), "amount"),
        (json!({ "track": 1, "index": 0, "grid": "1/4", "amount": -1 }), "amount"),
        (json!({ "track": 1, "index": 0, "grid": "1/4", "amount": "all" }), "amount"),
        (json!({ "track": 1, "index": 0, "grid": "1/4", "ends": "yes" }), "ends"),
        (json!({ "track": 1, "index": 0, "grid": "1/4", "notes": [1] }), "note 1 does not exist"),
        (json!({ "track": 1, "index": 0, "grid": "1/4", "notes": [] }), "empty"),
        (json!({ "track": 1, "index": 0, "grid": "1/4", "notes": "all" }), "list"),
        (json!({ "track": 1, "index": 5, "grid": "1/4" }), "clip index 5"),
        (json!({ "track": 1, "grid": "1/4" }), "give index"),
        (json!({ "track": 1, "index": 0, "grid": "1/4", "swing": 10 }), "unknown parameter"),
    ];
    for (params, error) in cases {
        let got = run(&mut session, params.clone());
        assert!(got.as_ref().is_err_and(|e| e.contains(error)), "{params}: {got:?}");
    }
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
}
