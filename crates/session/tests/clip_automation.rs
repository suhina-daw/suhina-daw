// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The automation under an arrangement clip goes with it: moved, copied and pasted, duplicated
//! (#916, `docs/spec/automation.md` › Automation under clips).
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use serde_json::json;
use suhina_model::automation::{Plain, value_at};
use suhina_model::*;
use suhina_session::Session;
use suhina_test_support::ProjectBuilder;

use crate::support::run;

/// "Keys" with a MIDI clip from beat 4 to 8 and a pan envelope that swings under it (1 at beat 5,
/// -1 at 7), and an empty "Bass".
fn session() -> Session {
    let mut project = ProjectBuilder::new()
        .midi_track(1, "Keys")
        .midi_track(2, "Bass")
        .notes(1, "Riff", 4.0, vec![Note::new(0.0, 1.0, 60, 1.0)])
        .clip(1, Clip::new(ContentId(1), 4.0, 4.0))
        .build();
    let channel = project.tracks[0].channel.unwrap();
    let mut lane = AutomationLane::new("pan");
    lane.points = vec![
        AutomationPoint::new(0.0, 0.0),
        AutomationPoint::new(5.0, 1.0),
        AutomationPoint::new(7.0, -1.0),
        AutomationPoint::new(12.0, 0.0),
    ];
    project.channels.iter_mut().find(|c| c.id == channel).unwrap().automation.push(lane);
    Session::new(project)
}

/// The pan envelope of track `track` (from 1) at `beat`.
fn pan(session: &Session, track: usize, beat: f64) -> f64 {
    let p = session.project();
    let channel = p.tracks[track - 1].channel.unwrap();
    let lanes = &p.channels.iter().find(|c| c.id == channel).unwrap().automation;
    let points = lanes.iter().find(|l| l.param == "pan").map_or(&[][..], |l| &l.points[..]);
    (value_at(points, beat, false, &Plain).unwrap_or(0.0) * 1e6).round() / 1e6
}

fn clip_id(session: &Session) -> u64 {
    u64::from(session.project().arrangement.lanes[0].clips[0].id.0)
}

#[test]
fn a_moved_clip_takes_its_automation_along() {
    let mut session = session();
    run(&mut session, "clip.move", json!({ "track": 1, "index": 0, "start": 12 })).unwrap();
    // The swing is under the clip at 12 to 16, as it was under it at 4 to 8.
    assert_eq!((pan(&session, 1, 13.0), pan(&session, 1, 15.0)), (1.0, -1.0));
    // Where the clip was, a straight line between the values at its edges (0.8 at 4, -0.8 at 8).
    assert_eq!((pan(&session, 1, 5.0), pan(&session, 1, 6.0)), (0.4, 0.0));
    // Before the old place it is as it was.
    assert_eq!(pan(&session, 1, 2.0), 0.4);
    // Moved back by several clips' command: the swing comes back.
    let id = clip_id(&session);
    run(&mut session, "clip.moveMany", json!({ "clips": [{ "clip": id }], "beats": -8 })).unwrap();
    assert_eq!((pan(&session, 1, 5.0), pan(&session, 1, 7.0)), (1.0, -1.0));
    // One undo takes the move back with its automation.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!((pan(&session, 1, 13.0), pan(&session, 1, 15.0)), (1.0, -1.0));
}

#[test]
fn a_copied_clip_pastes_with_its_automation_on_any_track() {
    let mut session = session();
    let id = clip_id(&session);
    run(&mut session, "clip.copy", json!({ "clips": [{ "clip": id }] })).unwrap();
    run(&mut session, "clip.paste", json!({ "track": 2, "start": 20 })).unwrap();
    assert_eq!((pan(&session, 2, 21.0), pan(&session, 2, 23.0)), (1.0, -1.0));
    // The original stays where it was.
    assert_eq!((pan(&session, 1, 5.0), pan(&session, 1, 7.0)), (1.0, -1.0));
}

#[test]
fn a_duplicated_clip_repeats_its_automation() {
    let mut session = session();
    let id = clip_id(&session);
    run(&mut session, "clip.duplicate", json!({ "clips": [{ "clip": id }] })).unwrap();
    assert_eq!((pan(&session, 1, 9.0), pan(&session, 1, 11.0)), (1.0, -1.0));
    assert_eq!((pan(&session, 1, 5.0), pan(&session, 1, 7.0)), (1.0, -1.0));
}

/// A clip with no point under it leaves the automation alone: the line under it is not cut.
#[test]
fn a_clip_over_a_plain_line_leaves_it() {
    let mut session = session();
    // Only the points at 0 and 12 left: a straight line under the clip.
    run(&mut session, "automation.deletePoints", json!({ "track": 1, "param": "pan", "points": [1, 2] })).unwrap();
    let before = session.project().channels.clone();
    let id = clip_id(&session);
    run(&mut session, "clip.moveMany", json!({ "clips": [{ "clip": id }], "beats": 4 })).unwrap();
    assert_eq!(session.project().channels, before);
}
