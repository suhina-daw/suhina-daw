// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The crossfader commands (`docs/spec/crossfader.md`): the value, the per-track assign, the
//! gesture and undo, and the malformed inputs.

use serde_json::json;
use suhina_model::CrossfadeAssign;
use suhina_session::{Session, blank_project};

use crate::support::run;

#[test]
fn the_crossfader_and_the_assign_are_project_state_with_undo() {
    let mut session = Session::new(blank_project());
    let track = session.project().tracks[0].id;
    let set = run(&mut session, "mixer.setCrossfader", json!({ "value": 0.25, "gesture": "crossfader" })).unwrap();
    assert_eq!(set["value"], 0.25);
    assert!((session.project().crossfader.0 - 0.25).abs() < 1e-9);
    let assign = run(&mut session, "track.setCrossfadeAssign", json!({ "tracks": [track.0], "assign": "b" })).unwrap();
    assert_eq!(assign["assign"], "b");
    assert_eq!(session.project().track(track).map(|t| t.crossfade_assign), Some(CrossfadeAssign::B));
    let listed = run(&mut session, "project.tracks", json!({})).unwrap();
    let entry = listed.as_array().unwrap().iter().find(|t| t["id"] == track.0).unwrap();
    assert_eq!(entry["crossfade_assign"], "b");
    // The command's answer names the track.
    assert_eq!(assign["tracks"], json!([track.0]));
    // One gesture is one undo step: the second value joins the first.
    run(&mut session, "mixer.setCrossfader", json!({ "value": 0.8, "gesture": "crossfader" })).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!((session.project().crossfader.0 - 0.25).abs() < 1e-9, "one drag back to where it began");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().track(track).map(|t| t.crossfade_assign), Some(CrossfadeAssign::Off));
}

#[test]
fn a_bad_crossfader_value_or_assign_is_refused() {
    let mut session = Session::new(blank_project());
    let track = session.project().tracks[0].id;
    for value in [-0.01, 1.01] {
        assert!(
            run(&mut session, "mixer.setCrossfader", json!({ "value": value })).is_err(),
            "value {value} is out of range"
        );
    }
    assert!(run(&mut session, "mixer.setCrossfader", json!({})).is_err(), "value is missing");
    for assign in ["c", "A", ""] {
        assert!(
            run(&mut session, "track.setCrossfadeAssign", json!({ "tracks": [track.0], "assign": assign })).is_err(),
            "assign {assign:?} is not a, b or off"
        );
    }
}
