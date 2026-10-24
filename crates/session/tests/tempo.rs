// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tempo map's commands (`docs/spec/tempo-map.md` › Commands): tempo points and
//! time-signature changes, kept in order, undone in one step each, and malformed params that
//! change nothing.

use serde_json::json;
use suhina_model::*;
use suhina_session::Session;

use crate::support::run;

fn beats(session: &Session) -> Vec<(f64, f64, bool)> {
    session.project().transport.tempo_changes.iter().map(|p| (p.beat, p.bpm, p.ramp)).collect()
}

fn bars(session: &Session) -> Vec<(u32, u8, u8)> {
    let changes = &session.project().transport.time_signature_changes;
    changes.iter().map(|c| (c.bar, c.time_signature.numerator, c.time_signature.denominator)).collect()
}

#[test]
fn tempo_points_are_added_in_order_moved_and_deleted() {
    let mut session = Session::new(Project::default());
    run(&mut session, "tempo.addPoint", json!({ "beat": 16, "bpm": 140 })).unwrap();
    run(&mut session, "tempo.addPoint", json!({ "beat": 8, "bpm": 90, "ramp": true })).unwrap();
    assert_eq!(beats(&session), [(8.0, 90.0, true), (16.0, 140.0, false)]);
    // Point 2 is the one at beat 16; a point on its beat replaces it.
    let listed = run(&mut session, "tempo.addPoint", json!({ "beat": 16, "bpm": 150 })).unwrap();
    assert_eq!(listed["points"][1], json!({ "point": 2, "beat": 16.0, "bpm": 150.0, "ramp": false }));
    assert_eq!(listed["start_bpm"], json!(120.0));
    // Moved before the other, it is numbered 1.
    run(&mut session, "tempo.movePoint", json!({ "point": 2, "beat": 4, "ramp": true })).unwrap();
    assert_eq!(beats(&session), [(4.0, 150.0, true), (8.0, 90.0, true)]);
    run(&mut session, "tempo.movePoint", json!({ "point": 2, "bpm": 60 })).unwrap();
    assert_eq!(beats(&session), [(4.0, 150.0, true), (8.0, 60.0, true)]);
    run(&mut session, "tempo.deletePoint", json!({ "point": 1 })).unwrap();
    assert_eq!(beats(&session), [(8.0, 60.0, true)]);
    // Each edit is one undo step.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(beats(&session), [(4.0, 150.0, true), (8.0, 60.0, true)]);
    for _ in 0..4 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert_eq!(beats(&session), [(16.0, 140.0, false)]);
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(beats(&session).len(), 2);
}

#[test]
fn one_beat_holds_a_glide_and_a_jump() {
    let mut session = Session::new(Project::default());
    // The jump first, then the glide into it: the glide is listed first either way.
    run(&mut session, "tempo.addPoint", json!({ "beat": 16, "bpm": 100 })).unwrap();
    run(&mut session, "tempo.addPoint", json!({ "beat": 16, "bpm": 140, "ramp": true })).unwrap();
    assert_eq!(beats(&session), [(16.0, 140.0, true), (16.0, 100.0, false)]);
    // The tempo glides from 120 to 140 and jumps to 100 on beat 16.
    let t = &session.project().transport;
    let map = TempoMap::new(t.tempo_bpm, &t.tempo_changes, 1.0);
    assert!((map.bpm_at(15.999) - 140.0).abs() < 0.01, "{}", map.bpm_at(15.999));
    assert_eq!(map.bpm_at(16.0), 100.0);
    // A point of the same kind on that beat replaces only its own kind.
    run(&mut session, "tempo.addPoint", json!({ "beat": 16, "bpm": 90 })).unwrap();
    assert_eq!(beats(&session), [(16.0, 140.0, true), (16.0, 90.0, false)]);
    run(&mut session, "tempo.addPoint", json!({ "beat": 16, "bpm": 150, "ramp": true })).unwrap();
    assert_eq!(beats(&session), [(16.0, 150.0, true), (16.0, 90.0, false)]);
    // The glide made a jump replaces the jump; a third point never joins the beat.
    run(&mut session, "tempo.movePoint", json!({ "point": 1, "ramp": false })).unwrap();
    assert_eq!(beats(&session), [(16.0, 150.0, false)]);
    // A jump moved onto a beat with only a glide joins it after the glide.
    run(&mut session, "tempo.addPoint", json!({ "beat": 8, "bpm": 130, "ramp": true })).unwrap();
    run(&mut session, "tempo.movePoint", json!({ "point": 2, "beat": 8 })).unwrap();
    assert_eq!(beats(&session), [(8.0, 130.0, true), (8.0, 150.0, false)]);
    let text = suhina_project_file::to_json(session.project());
    assert_eq!(suhina_project_file::from_json(&text).as_ref(), Ok(session.project()));
}

#[test]
fn time_signature_changes_are_added_moved_and_deleted() {
    let mut session = Session::new(Project::default());
    let listed = run(&mut session, "timeSignature.add", json!({ "bar": 9, "numerator": 7, "denominator": 8 })).unwrap();
    assert_eq!(listed, json!({ "start": "4/4", "changes": [{ "bar": 9, "time_signature": "7/8" }] }));
    run(&mut session, "timeSignature.add", json!({ "bar": 5, "numerator": 3, "denominator": 4 })).unwrap();
    // Bars are shown from 1 and kept from 0.
    assert_eq!(bars(&session), [(4, 3, 4), (8, 7, 8)]);
    run(&mut session, "timeSignature.move", json!({ "bar": 9, "to": 3 })).unwrap();
    assert_eq!(bars(&session), [(2, 7, 8), (4, 3, 4)]);
    run(&mut session, "timeSignature.move", json!({ "bar": 5, "numerator": 6, "denominator": 8 })).unwrap();
    assert_eq!(bars(&session), [(2, 7, 8), (4, 6, 8)]);
    // Moved onto another change, it replaces it.
    run(&mut session, "timeSignature.move", json!({ "bar": 3, "to": 5 })).unwrap();
    assert_eq!(bars(&session), [(4, 7, 8)]);
    run(&mut session, "timeSignature.delete", json!({ "bar": 5 })).unwrap();
    assert!(bars(&session).is_empty());
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(bars(&session), [(4, 7, 8)]);
    let listed = run(&mut session, "tempo.points", json!({})).unwrap();
    assert_eq!(listed["time_signatures"]["changes"][0]["bar"], json!(5));
    assert_eq!(listed["tempo"]["points"], json!([]));
}

#[test]
fn malformed_params_are_errors_that_change_nothing() {
    let mut start = Project::default();
    start.transport.tempo_changes = vec![TempoPoint { beat: 8.0, bpm: 100.0, ramp: false }];
    start.transport.time_signature_changes =
        vec![TimeSignatureChange { bar: 4, time_signature: TimeSignature { numerator: 3, denominator: 4 } }];
    let bad = [
        ("tempo.addPoint", json!({ "beat": 0, "bpm": 100 })),
        ("tempo.addPoint", json!({ "beat": -1, "bpm": 100 })),
        ("tempo.addPoint", json!({ "beat": 1e9, "bpm": 100 })),
        ("tempo.addPoint", json!({ "beat": 4, "bpm": 10 })),
        ("tempo.addPoint", json!({ "beat": 4, "bpm": 1000 })),
        ("tempo.addPoint", json!({ "beat": "four", "bpm": 100 })),
        ("tempo.addPoint", json!({ "beat": 4 })),
        ("tempo.addPoint", json!({ "beat": 4, "bpm": 100, "ramp": "yes" })),
        ("tempo.addPoint", json!({ "beat": 4, "bpm": 100, "curve": 1 })),
        ("tempo.movePoint", json!({ "point": 0, "beat": 4 })),
        ("tempo.movePoint", json!({ "point": 2, "beat": 4 })),
        ("tempo.movePoint", json!({ "point": 1 })),
        ("tempo.movePoint", json!({ "point": 1, "beat": 0 })),
        ("tempo.movePoint", json!({ "point": 1, "bpm": f64::MAX })),
        ("tempo.movePoint", json!({ "point": -1, "bpm": 100 })),
        ("tempo.deletePoint", json!({ "point": 2 })),
        ("tempo.deletePoint", json!({ "point": 1.5 })),
        ("tempo.deletePoint", json!({})),
        ("timeSignature.add", json!({ "bar": 1, "numerator": 3, "denominator": 4 })),
        ("timeSignature.add", json!({ "bar": 0, "numerator": 3, "denominator": 4 })),
        ("timeSignature.add", json!({ "bar": u64::MAX, "numerator": 3, "denominator": 4 })),
        ("timeSignature.add", json!({ "bar": 3, "numerator": 0, "denominator": 4 })),
        ("timeSignature.add", json!({ "bar": 3, "numerator": 100, "denominator": 4 })),
        ("timeSignature.add", json!({ "bar": 3, "numerator": 3, "denominator": 3 })),
        ("timeSignature.add", json!({ "bar": 3, "numerator": 3, "denominator": 300 })),
        ("timeSignature.add", json!({ "bar": 3, "numerator": 3 })),
        ("timeSignature.move", json!({ "bar": 3, "to": 6 })),
        ("timeSignature.move", json!({ "bar": 5 })),
        ("timeSignature.move", json!({ "bar": 5, "to": 1 })),
        ("timeSignature.move", json!({ "bar": 5, "denominator": 5 })),
        ("timeSignature.delete", json!({ "bar": 6 })),
        ("timeSignature.delete", json!({ "bar": "five" })),
    ];
    for (id, params) in bad {
        let mut session = Session::new(start.clone());
        let result = run(&mut session, id, params.clone());
        assert!(result.as_ref().is_err_and(|e| !e.is_empty()), "{id} {params} gave {result:?}");
        assert_eq!(session.project(), &start, "{id} {params} changed the project");
        assert!(!session.can_undo());
    }
}

#[test]
fn a_saved_project_keeps_its_tempo_map() {
    let mut session = Session::new(Project::default());
    run(&mut session, "tempo.addPoint", json!({ "beat": 8, "bpm": 90, "ramp": true })).unwrap();
    run(&mut session, "timeSignature.add", json!({ "bar": 3, "numerator": 5, "denominator": 4 })).unwrap();
    let text = suhina_project_file::to_json(session.project());
    assert_eq!(suhina_project_file::from_json(&text).as_ref(), Ok(session.project()));
}
