// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A note's own curves through the commands: `note.setExpression` sets one, one undo step, and
//! refuses what the model refuses; every other `note.*` command and the clip edits that change a
//! note's time or length move, crop, stretch, split or join its curves as the spec's table says
//! (`docs/spec/mpe.md` › Note curves and editing, ADR 0039 › 6).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use serde_json::{Value, json};
use suhina_model::*;
use suhina_session::Session;

use crate::support::run;

/// The clip every test edits: track 1's first arrangement clip.
fn at() -> Value {
    json!({ "track": 1, "index": 0 })
}

/// `at()` with `more`.
fn with(more: Value) -> Value {
    let mut params = at();
    for (key, value) in more.as_object().unwrap() {
        params[key] = value.clone();
    }
    params
}

fn note(time: f64, duration: f64, key: u8) -> Note {
    Note::new(time, duration, key, 100.0 / 127.0)
}

fn points(list: &[(f64, f64)]) -> Vec<ExpressionPoint> {
    list.iter().map(|&(offset, value)| ExpressionPoint::new(offset, value)).collect()
}

/// `note` with `pitch` as its pitch curve.
fn bent(mut note: Note, pitch: &[(f64, f64)]) -> Note {
    note.expression = Some(Shared::new(NoteExpression { pitch: points(pitch), ..NoteExpression::default() }));
    note
}

/// One MIDI track with one arrangement clip, 16 beats long, playing `notes`.
fn session(notes: Vec<Note>) -> Session {
    let content = ClipContent {
        linked: false,
        id: ContentId(1),
        name: "C1".into(),
        length: 16.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Notes(notes.into()) }].into(),
        extensions: Extensions::default(),
        scale: None,
    };
    Session::new(suhina_test_support::with_ids(Project {
        tracks: vec![Track { content: ContentKinds { notes: true, audio: false }, ..Track::new(TrackId(1), "Keys") }],
        contents: vec![content],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "Keys".into(),
                track: Some(TrackId(1)),
                clips: vec![Clip::new(ContentId(1), 0.0, 16.0)],
            }],
            markers: Default::default(),
            takes: Vec::new(),
        },
        ..Project::default()
    }))
}

fn notes(session: &Session) -> Vec<Note> {
    let clip = &session.project().arrangement.lanes[0].clips[0];
    let content = session.project().content(clip.content).expect("the content");
    content
        .lanes
        .iter()
        .find_map(|l| match &l.data {
            LaneData::Notes(n) => Some(n.to_vec()),
            LaneData::Audio(_) => None,
        })
        .unwrap_or_default()
}

/// The `kind` curve of note `index`, as (offset, value) pairs; empty when it has none.
fn curve(session: &Session, index: usize, kind: ExpressionKind) -> Vec<(f64, f64)> {
    let notes = notes(session);
    notes[index].expression().map_or_else(Vec::new, |e| e.curve(kind).iter().map(|p| (p.offset, p.value)).collect())
}

fn pitch(session: &Session, index: usize) -> Vec<(f64, f64)> {
    curve(session, index, ExpressionKind::Pitch)
}

#[track_caller]
fn assert_curve(actual: &[(f64, f64)], expected: &[(f64, f64)]) {
    let close = actual.len() == expected.len()
        && actual.iter().zip(expected).all(|(a, e)| (a.0 - e.0).abs() < 1e-9 && (a.1 - e.1).abs() < 1e-9);
    assert!(close, "curve {actual:?}, expected {expected:?}");
}

fn undo(session: &mut Session) {
    run(session, "edit.undo", json!({})).unwrap();
}

/// A note 2 beats long from beat 1 that bends up two semitones and back.
fn arch() -> Note {
    bent(note(1.0, 2.0, 60), &[(0.0, 0.0), (1.0, 2.0), (2.0, 0.0)])
}

#[test]
fn set_expression_replaces_a_curve_and_one_undo_takes_it_back() {
    let mut session = session(vec![note(0.0, 2.0, 60), note(2.0, 1.0, 62)]);
    let set = |session: &mut Session, note: Value, kind: &str, points: Value| {
        run(session, "note.setExpression", with(json!({ "note": note, "kind": kind, "points": points })))
    };

    let out = set(&mut session, json!(0), "pitch", json!([[0, 0], [0.5, 2], [2, -1.5]])).unwrap();
    assert_eq!(out, json!({ "note": 0, "kind": "pitch", "points": 3 }));
    assert_curve(&pitch(&session, 0), &[(0.0, 0.0), (0.5, 2.0), (2.0, -1.5)]);
    assert!(notes(&session)[1].expression.is_none(), "the other note is untouched");

    // By id; a second kind beside the first; the whole curve is replaced, not merged.
    let id = run(&mut session, "note.list", at()).unwrap()[0]["id"].clone();
    set(&mut session, json!({ "id": id }), "pressure", json!([[0, 0.25], [1, 1]])).unwrap();
    set(&mut session, json!(0), "pitch", json!([[1, 12]])).unwrap();
    assert_curve(&pitch(&session, 0), &[(1.0, 12.0)]);
    assert_curve(&curve(&session, 0, ExpressionKind::Pressure), &[(0.0, 0.25), (1.0, 1.0)]);

    // [] removes one curve; the last one gone, the note has no expression at all.
    set(&mut session, json!(0), "pitch", json!([])).unwrap();
    assert!(pitch(&session, 0).is_empty());
    let out = set(&mut session, json!(0), "pressure", json!([])).unwrap();
    assert_eq!(out["points"], 0);
    assert!(notes(&session)[0].expression.is_none(), "an emptied note has None");

    // Timbre on the other note, the full range allowed.
    set(&mut session, json!(1), "timbre", json!([[0, 0], [1, 1]])).unwrap();
    assert_curve(&curve(&session, 1, ExpressionKind::Timbre), &[(0.0, 0.0), (1.0, 1.0)]);

    // One undo per command, back to the start.
    undo(&mut session);
    assert!(notes(&session)[1].expression.is_none());
    undo(&mut session);
    assert_curve(&curve(&session, 0, ExpressionKind::Pressure), &[(0.0, 0.25), (1.0, 1.0)]);
    undo(&mut session);
    assert_curve(&pitch(&session, 0), &[(1.0, 12.0)]);
    undo(&mut session);
    assert_curve(&pitch(&session, 0), &[(0.0, 0.0), (0.5, 2.0), (2.0, -1.5)]);
    undo(&mut session);
    assert!(curve(&session, 0, ExpressionKind::Pressure).is_empty());
    undo(&mut session);
    assert!(notes(&session)[0].expression.is_none());
    assert!(!session.can_undo());

    // Redo puts the first curve back.
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_curve(&pitch(&session, 0), &[(0.0, 0.0), (0.5, 2.0), (2.0, -1.5)]);
}

#[test]
fn set_expression_refuses_what_the_model_refuses_and_changes_nothing() {
    let mut session = session(vec![note(0.0, 2.0, 60)]);
    let before = session.project().clone();
    let cases: [(Value, &str); 14] = [
        (json!({ "note": 0, "kind": "volume", "points": [] }), "kind"),
        (json!({ "note": 0, "points": [] }), "kind"),
        (json!({ "note": 0, "kind": "pitch" }), "points"),
        (json!({ "kind": "pitch", "points": [] }), "note"),
        (json!({ "note": 5, "kind": "pitch", "points": [] }), "note 5 does not exist"),
        (json!({ "note": { "id": 999_999 }, "kind": "pitch", "points": [] }), "no note with id"),
        (json!({ "note": 0, "kind": "pitch", "points": [[0, 0], [2.5, 1]] }), "outside the note"),
        (json!({ "note": 0, "kind": "pitch", "points": [[-0.5, 0]] }), "outside the note"),
        (json!({ "note": 0, "kind": "pitch", "points": [[1, 0], [0.5, 1]] }), "increasing"),
        (json!({ "note": 0, "kind": "pitch", "points": [[1, 0], [1, 1]] }), "increasing"),
        (json!({ "note": 0, "kind": "pitch", "points": [[0, 300]] }), "outside -256 to 256"),
        (json!({ "note": 0, "kind": "pressure", "points": [[0, 1.5]] }), "outside 0 to 1"),
        (json!({ "note": 0, "kind": "timbre", "points": [[0]] }), "[offset, value]"),
        (json!({ "note": 0, "kind": "timbre", "points": [["a", 1]] }), "[offset, value]"),
    ];
    for (params, says) in cases {
        let error = run(&mut session, "note.setExpression", with(params.clone())).unwrap_err();
        assert!(error.contains(says), "{params}: {error}");
        assert_eq!(session.project(), &before, "{params} changed the project");
    }
    // A clip that is not there has no notes to bend.
    let error = run(
        &mut session,
        "note.setExpression",
        json!({ "track": 1, "index": 3, "note": 0, "kind": "pitch", "points": [] }),
    )
    .unwrap_err();
    assert!(error.contains("clip index 3 does not exist"), "{error}");

    // More points than one curve may have.
    let many: Vec<Value> = (0..=MAX_CURVE_POINTS).map(|i| json!([i as f64 / 4096.0, 0])).collect();
    let error = run(&mut session, "note.setExpression", with(json!({ "note": 0, "kind": "pitch", "points": many })))
        .unwrap_err();
    assert!(error.contains("at most 4096 points"), "{error}");
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo(), "no refusal marks an undo step");
}

#[test]
fn set_expression_keeps_a_clip_content_within_its_expression_points() {
    // 64 full curves: exactly the content's limit.
    let full: Vec<(f64, f64)> = (0..MAX_CURVE_POINTS).map(|i| (i as f64 / 1024.0, 0.5)).collect();
    let full = points(&full);
    let mut all = Vec::new();
    for n in 0..21 {
        let mut held = note(f64::from(n) * 5.0, 5.0, 60);
        held.expression =
            Some(Shared::new(NoteExpression { pitch: full.clone(), pressure: full.clone(), timbre: full.clone() }));
        all.push(held);
    }
    let mut last = note(105.0, 5.0, 60);
    last.expression = Some(Shared::new(NoteExpression { pitch: full.clone(), ..NoteExpression::default() }));
    all.push(last);
    all.push(note(110.0, 5.0, 62));
    assert_eq!(21 * 3 + 1, MAX_CONTENT_EXPRESSION_POINTS / MAX_CURVE_POINTS);
    let mut session = session(all);

    let error =
        run(&mut session, "note.setExpression", with(json!({ "note": 22, "kind": "pitch", "points": [[0, 1]] })))
            .unwrap_err();
    assert!(error.contains("at most 262144 expression points"), "{error}");
    assert!(!session.can_undo());

    // Replacing a full curve with another full one keeps the content at the limit: allowed.
    let again: Vec<Value> = (0..MAX_CURVE_POINTS).map(|i| json!([i as f64 / 1024.0, 1])).collect();
    let out =
        run(&mut session, "note.setExpression", with(json!({ "note": 21, "kind": "pitch", "points": again }))).unwrap();
    assert_eq!(out["points"], 4096);
    // And taking a curve away makes room for one point elsewhere.
    run(&mut session, "note.setExpression", with(json!({ "note": 21, "kind": "pitch", "points": [] }))).unwrap();
    run(&mut session, "note.setExpression", with(json!({ "note": 22, "kind": "pitch", "points": [[0, 1]] }))).unwrap();
}

#[test]
fn moving_transposing_duplicating_and_pasting_keep_the_curves() {
    let mut session = session(vec![arch()]);
    let shape = [(0.0, 0.0), (1.0, 2.0), (2.0, 0.0)];
    run(&mut session, "note.move", with(json!({ "notes": [0], "beats": 2, "semitones": 5 }))).unwrap();
    assert_eq!((notes(&session)[0].time, notes(&session)[0].key), (3.0, 65));
    assert_curve(&pitch(&session, 0), &shape);

    run(&mut session, "note.quantize", with(json!({ "grid": "1 bar" }))).unwrap();
    assert_eq!(notes(&session)[0].time, 4.0);
    assert_curve(&pitch(&session, 0), &shape);

    run(&mut session, "note.duplicate", with(json!({ "notes": [0] }))).unwrap();
    assert_curve(&pitch(&session, 1), &shape);
    run(&mut session, "note.copy", with(json!({ "notes": [0] }))).unwrap();
    run(&mut session, "note.paste", with(json!({ "time": 12 }))).unwrap();
    assert_eq!(notes(&session)[2].time, 12.0);
    assert_curve(&pitch(&session, 2), &shape);
}

#[test]
fn the_right_edge_crops_the_curves_and_lengthening_holds_them() {
    let mut session = session(vec![arch()]);
    run(&mut session, "note.resize", with(json!({ "notes": [0], "length": 1.5 }))).unwrap();
    assert_curve(&pitch(&session, 0), &[(0.0, 0.0), (1.0, 2.0), (1.5, 1.0)]);
    undo(&mut session);
    run(&mut session, "note.resize", with(json!({ "notes": [0], "by": 1 }))).unwrap();
    assert_eq!(notes(&session)[0].duration, 3.0);
    assert_curve(&pitch(&session, 0), &[(0.0, 0.0), (1.0, 2.0), (2.0, 0.0)]);
    undo(&mut session);
    run(&mut session, "note.resize", with(json!({ "notes": [0], "lengths": [0.5] }))).unwrap();
    assert_curve(&pitch(&session, 0), &[(0.0, 0.0), (0.5, 1.0)]);
    undo(&mut session);

    // Legato shortens the note to the next one's start: a crop too.
    run(&mut session, "note.add", with(json!({ "notes": [{ "time": 2.5, "length": 1, "key": 64 }] }))).unwrap();
    run(&mut session, "note.legato", with(json!({ "notes": [0, 1] }))).unwrap();
    assert_eq!(notes(&session)[0].duration, 1.5);
    assert_curve(&pitch(&session, 0), &[(0.0, 0.0), (1.0, 2.0), (1.5, 1.0)]);
}

#[test]
fn the_left_edge_rebases_the_curves() {
    let mut session = session(vec![arch()]);
    run(&mut session, "note.resize", with(json!({ "notes": [0], "startBy": 0.5 }))).unwrap();
    let moved = &notes(&session)[0];
    assert_eq!((moved.time, moved.duration), (1.5, 1.5));
    assert_curve(&pitch(&session, 0), &[(0.0, 1.0), (0.5, 2.0), (1.5, 0.0)]);
    undo(&mut session);

    // Earlier: the old first value holds from the new start.
    run(&mut session, "note.resize", with(json!({ "notes": [0], "startBy": -0.5 }))).unwrap();
    let moved = &notes(&session)[0];
    assert_eq!((moved.time, moved.duration), (0.5, 2.5));
    assert_curve(&pitch(&session, 0), &[(0.5, 0.0), (1.5, 2.0), (2.5, 0.0)]);
    undo(&mut session);
    assert_curve(&pitch(&session, 0), &[(0.0, 0.0), (1.0, 2.0), (2.0, 0.0)]);
}

#[test]
fn stretching_fitting_and_quantising_ends_scale_the_curves() {
    let mut session = session(vec![arch()]);
    run(&mut session, "note.stretch", with(json!({ "notes": [0], "factor": 2 }))).unwrap();
    assert_curve(&pitch(&session, 0), &[(0.0, 0.0), (2.0, 2.0), (4.0, 0.0)]);
    undo(&mut session);

    run(&mut session, "note.fitToTime", with(json!({ "notes": [0], "start": 0, "end": 1 }))).unwrap();
    assert_curve(&pitch(&session, 0), &[(0.0, 0.0), (0.5, 2.0), (1.0, 0.0)]);
    undo(&mut session);

    // A note from 1 to 2.5 quantised with ends to beats ends at 3: 2 beats, 4/3 as long.
    let mut session = session_with_short();
    run(&mut session, "note.quantize", with(json!({ "grid": "1/4", "ends": true }))).unwrap();
    assert_eq!(notes(&session)[0].duration, 2.0);
    assert_curve(&pitch(&session, 0), &[(0.0, 0.0), (2.0, 1.0)]);
}

fn session_with_short() -> Session {
    session(vec![bent(note(1.0, 1.5, 60), &[(0.0, 0.0), (1.5, 1.0)])])
}

#[test]
fn splitting_keeps_the_value_at_each_cut_on_both_sides() {
    let mut session = session(vec![arch()]);
    run(&mut session, "note.split", with(json!({ "notes": [0], "at": 2 }))).unwrap();
    assert_curve(&pitch(&session, 0), &[(0.0, 0.0), (1.0, 2.0)]);
    assert_curve(&pitch(&session, 1), &[(0.0, 2.0), (1.0, 0.0)]);
    undo(&mut session);

    run(&mut session, "note.split", with(json!({ "notes": [0], "ats": [1.5, 2.5] }))).unwrap();
    assert_curve(&pitch(&session, 0), &[(0.0, 0.0), (0.5, 1.0)]);
    assert_curve(&pitch(&session, 1), &[(0.0, 1.0), (0.5, 2.0), (1.0, 1.0)]);
    assert_curve(&pitch(&session, 2), &[(0.0, 1.0), (0.5, 0.0)]);
    undo(&mut session);

    run(&mut session, "note.split", with(json!({ "notes": [0], "at": 1.5, "keepLonger": true }))).unwrap();
    assert_eq!(notes(&session).len(), 1);
    assert_curve(&pitch(&session, 0), &[(0.0, 1.0), (0.5, 2.0), (1.5, 0.0)]);
    undo(&mut session);

    // Cropping the clip cuts the note at the clip's edges the same way.
    run(&mut session, "clip.crop", with(json!({ "start": 1.5, "end": 2.5 }))).unwrap();
    assert_eq!(notes(&session)[0].duration, 1.0);
    assert_curve(&pitch(&session, 0), &[(0.0, 1.0), (0.5, 2.0), (1.0, 1.0)]);
}

#[test]
fn joining_concatenates_matching_curves_and_refuses_a_jump() {
    let first = bent(note(0.0, 1.0, 60), &[(0.0, 0.0), (1.0, 2.0)]);
    let second = bent(note(1.0, 1.0, 60), &[(0.0, 2.0), (1.0, 0.0)]);
    let mut session = session(vec![first.clone(), second]);
    run(&mut session, "note.join", with(json!({ "notes": [0, 1] }))).unwrap();
    assert_eq!(notes(&session).len(), 1);
    assert_eq!(notes(&session)[0].duration, 2.0);
    assert_curve(&pitch(&session, 0), &[(0.0, 0.0), (1.0, 2.0), (2.0, 0.0)]);

    // A curve that jumps at the join: refused, nothing changed.
    let jump = bent(note(1.0, 1.0, 60), &[(0.0, 1.0)]);
    let mut session = self::session(vec![first, jump]);
    let before = session.project().clone();
    let error = run(&mut session, "note.join", with(json!({ "notes": [0, 1] }))).unwrap_err();
    assert!(error.contains("differs across the join"), "{error}");
    assert_eq!(session.project(), &before);

    // A missing curve is its neutral value: a pressure that has fallen to 0 joins a note with none.
    let mut pressed = note(0.0, 1.0, 60);
    pressed.expression =
        Some(Shared::new(NoteExpression { pressure: points(&[(0.0, 0.5), (1.0, 0.0)]), ..NoteExpression::default() }));
    let mut session = self::session(vec![pressed, note(1.0, 1.0, 60)]);
    run(&mut session, "note.join", with(json!({ "notes": [0, 1] }))).unwrap();
    assert_curve(&curve(&session, 0, ExpressionKind::Pressure), &[(0.0, 0.5), (1.0, 0.0)]);
}

/// A note from beat 0, 5 beats long, whose pitch curve holds `count` points at 1/1024, 2/1024, …
fn dense(count: usize) -> Note {
    let list: Vec<(f64, f64)> = (1..=count).map(|i| (i as f64 / 1024.0, 0.0)).collect();
    bent(note(0.0, 5.0, 60), &list)
}

#[test]
fn edits_keep_a_full_curve_within_the_curve_limit() {
    // A cut before the first point: the right part's first value holds already, no point added.
    let mut session = session(vec![dense(MAX_CURVE_POINTS)]);
    run(&mut session, "note.split", with(json!({ "notes": [0], "at": 0.0005 }))).unwrap();
    assert_eq!(pitch(&session, 1).len(), MAX_CURVE_POINTS);
    undo(&mut session);
    // The left edge moved to before the first point: the same.
    run(&mut session, "note.resize", with(json!({ "notes": [0], "startBy": 0.0005 }))).unwrap();
    assert_eq!(pitch(&session, 0).len(), MAX_CURVE_POINTS);
}

#[test]
fn a_join_past_the_curve_limit_is_refused_and_changes_nothing() {
    let half = |count: usize, time: f64| {
        let list: Vec<(f64, f64)> = (0..count).map(|i| (i as f64 / count as f64, 0.0)).collect();
        bent(note(time, 1.0, 60), &list)
    };
    let mut session = session(vec![half(MAX_CURVE_POINTS / 2, 0.0), half(MAX_CURVE_POINTS / 2 + 1, 1.0)]);
    let before = session.project().clone();
    let error = run(&mut session, "note.join", with(json!({ "notes": [0, 1] }))).unwrap_err();
    assert!(error.contains("4097 points, more than 4096"), "{error}");
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());

    // One point fewer fits exactly.
    let mut session = self::session(vec![half(MAX_CURVE_POINTS / 2, 0.0), half(MAX_CURVE_POINTS / 2, 1.0)]);
    run(&mut session, "note.join", with(json!({ "notes": [0, 1] }))).unwrap();
    assert_eq!(pitch(&session, 0).len(), MAX_CURVE_POINTS);
}

#[test]
fn a_quantised_end_never_leaves_a_point_past_the_note() {
    // 2.921875 * (3 / 2.921875) is a hair above 3: the stretched last point is cropped back.
    let mut session = session(vec![bent(note(0.0, 2.921875, 60), &[(0.0, 0.0), (2.921875, 1.0)])]);
    run(&mut session, "note.quantize", with(json!({ "grid": "1/4", "ends": true }))).unwrap();
    let quantised = &notes(&session)[0];
    assert_eq!(quantised.duration, 3.0);
    assert_curve(&pitch(&session, 0), &[(0.0, 0.0), (3.0, 1.0)]);
    assert_eq!(quantised.expression().unwrap().validate(quantised.duration), Ok(()));
}

#[test]
fn an_edit_past_the_content_expression_limit_is_refused() {
    // 64 full curves on 64 notes: the content is at its limit, and a duplicate would pass it.
    let full: Vec<(f64, f64)> = (0..MAX_CURVE_POINTS).map(|i| (i as f64 / 1024.0, 0.5)).collect();
    let all = (0..MAX_CONTENT_EXPRESSION_POINTS / MAX_CURVE_POINTS)
        .map(|n| bent(note(n as f64 * 0.125, 4.0, 60 + (n % 8) as u8), &full))
        .collect();
    let mut session = session(all);
    let before = session.project().clone();
    let error = run(&mut session, "note.duplicate", with(json!({ "notes": [0] }))).unwrap_err();
    assert!(error.contains("TooManyExpressionPoints"), "{error}");
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
}
