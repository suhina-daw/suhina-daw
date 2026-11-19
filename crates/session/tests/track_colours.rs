// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::support::run;
use serde_json::json;
use suhina_model::{Rgb, TrackId};
use suhina_session::{Session, blank_project};

#[test]
fn several_tracks_change_atomically_and_round_trip_with_one_undo() {
    let mut session = Session::new(blank_project());
    let before = session.project().clone();
    run(&mut session, "track.setColor", json!({"tracks":[1,2,1],"color":"#23aBcD"})).unwrap();
    for id in [1, 2] {
        assert_eq!(session.project().track(TrackId(id)).unwrap().color, Some(Rgb(35, 171, 205)));
    }
    assert_eq!(session.undo_steps(), 1);
    let saved = suhina_project_file::to_json(session.project());
    assert_eq!(suhina_project_file::from_json(&saved).unwrap(), *session.project());
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &before);
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(session.project().track(TrackId(2)).unwrap().color, Some(Rgb(35, 171, 205)));
    run(&mut session, "track.setColor", json!({"tracks":[1,2],"color":"none"})).unwrap();
    assert!(session.project().track(TrackId(1)).unwrap().color.is_none());
    assert!(session.project().track(TrackId(2)).unwrap().color.is_none());
}

#[test]
fn malformed_colour_or_target_never_partially_changes_the_selection() {
    let mut session = Session::new(blank_project());
    let before = session.project().clone();
    for params in [
        json!({"tracks":[1,999999],"color":"#123456"}),
        json!({"tracks":[],"color":"#123456"}),
        json!({"tracks":[1],"track":2,"color":"#123456"}),
        json!({"tracks":[1,2],"color":"#12345"}),
        json!({"tracks":[1,2],"color":"#abé12"}),
        json!({"tracks":[1,2],"color":"#gg0000"}),
        json!({"tracks":[1,2],"color":null}),
        json!({"tracks":[1,2],"color":"#123456","gesture":""}),
    ] {
        assert!(run(&mut session, "track.setColor", params.clone()).is_err(), "{params}");
        assert_eq!(session.project(), &before);
        assert_eq!(session.undo_steps(), 0);
    }
}

#[test]
fn live_colour_gestures_merge_until_release_and_noops_add_nothing() {
    let mut session = Session::new(blank_project());
    let before = session.project().clone();
    for colour in ["#123456", "#654321", "#abcdef"] {
        run(&mut session, "track.setColor", json!({"tracks":[1,2],"color":colour,"gesture":"colour"})).unwrap();
        assert_eq!(session.undo_steps(), 1);
    }
    run(&mut session, "history.endGesture", json!({"gesture":"colour"})).unwrap();
    run(&mut session, "track.setColor", json!({"tracks":[1,2],"color":"#abcdef"})).unwrap();
    assert_eq!(session.undo_steps(), 1);
    run(&mut session, "track.setColor", json!({"track":1,"color":"#000000","gesture":"colour"})).unwrap();
    assert_eq!(session.undo_steps(), 2);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().track(TrackId(1)).unwrap().color, Some(Rgb(171, 205, 239)));
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &before);
}
