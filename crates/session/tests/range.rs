// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Editing a time selection (`docs/spec/arrangement-editing.md` › Time selection): splitting,
//! deleting, copying and duplicating what plays inside a span of beats over some tracks, and
//! moving several clips at once.
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use serde_json::{Value, json};
use suhina_model::*;
use suhina_session::{Session, blank_project};

fn run(session: &mut Session, id: &str, params: Value) -> Result<Value, String> {
    session.execute(id, &params).map_err(|e| e.0)
}

/// The blank project with a 4-beat notes clip at beat 0 on tracks 1 and 2.
fn project() -> Project {
    let mut p = blank_project();
    for id in [1, 2] {
        p.contents.push(ClipContent {
            id: ContentId(id),
            name: format!("Riff {id}"),
            length: 4.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![Note::new(0.0, 4.0, 60, 0.8)]) }],
            extensions: Extensions::default(),
            scale: None,
        });
        let lane = p.arrangement.lanes.iter_mut().find(|l| l.track == Some(TrackId(id))).unwrap();
        lane.clips = vec![Clip::new(ContentId(id), 0.0, 4.0)];
    }
    p
}

/// Each clip of `track` as (start, length).
fn spans(session: &Session, track: u32) -> Vec<(f64, f64)> {
    let p = session.project();
    let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(track))).unwrap();
    lane.clips.iter().map(|c| (c.start, c.length)).collect()
}

const BOTH: [u32; 2] = [1, 2];

#[test]
fn deleting_a_range_keeps_the_parts_outside_it_and_the_gap() {
    let mut session = Session::new(project());
    run(&mut session, "range.delete", json!({ "tracks": BOTH, "start": 1.0, "end": 3.0 })).unwrap();
    for track in BOTH {
        assert_eq!(spans(&session, track), [(0.0, 1.0), (3.0, 1.0)], "track {track}");
    }
    // The second part plays on from where the clip was.
    let p = session.project();
    let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(1))).unwrap();
    assert!((lane.clips[1].offset - 3.0).abs() < 1e-9);
    // One undo step brings the whole clips back.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(spans(&session, 1), [(0.0, 4.0)]);
    assert_eq!(spans(&session, 2), [(0.0, 4.0)]);
}

#[test]
fn a_range_on_one_track_leaves_the_others() {
    let mut session = Session::new(project());
    run(&mut session, "range.delete", json!({ "tracks": [2], "start": 0.0, "end": 2.0 })).unwrap();
    assert_eq!(spans(&session, 1), [(0.0, 4.0)]);
    assert_eq!(spans(&session, 2), [(2.0, 2.0)]);
}

#[test]
fn splitting_a_range_makes_it_clips_of_its_own() {
    let mut session = Session::new(project());
    let out = run(&mut session, "range.split", json!({ "tracks": BOTH, "start": 1.0, "end": 3.0 })).unwrap();
    for track in BOTH {
        assert_eq!(spans(&session, track), [(0.0, 1.0), (1.0, 2.0), (3.0, 1.0)]);
    }
    assert_eq!(out["clips"], json!([{ "track": 1, "index": 1 }, { "track": 2, "index": 1 }]), "the middle parts");
}

#[test]
fn a_copied_range_pastes_back_on_its_own_tracks_from_the_start_given() {
    let mut session = Session::new(project());
    run(&mut session, "range.copy", json!({ "tracks": BOTH, "start": 1.0, "end": 3.0 })).unwrap();
    assert!(session.copied_from_tracks());
    run(&mut session, "clip.paste", json!({ "start": 8.0 })).unwrap();
    for track in BOTH {
        assert_eq!(spans(&session, track), [(0.0, 4.0), (8.0, 2.0)], "track {track}");
    }
    // A paste with a track still puts everything on that one.
    run(&mut session, "clip.paste", json!({ "track": 1, "start": 12.0 })).unwrap();
    // (both copied parts land there, the second replacing the first)
    assert_eq!(spans(&session, 1), [(0.0, 4.0), (8.0, 2.0), (12.0, 2.0)]);
}

#[test]
fn cutting_a_range_copies_then_deletes_it_in_one_step() {
    let mut session = Session::new(project());
    run(&mut session, "range.cut", json!({ "tracks": [1], "start": 2.0, "end": 4.0 })).unwrap();
    assert_eq!(spans(&session, 1), [(0.0, 2.0)]);
    run(&mut session, "clip.paste", json!({ "start": 6.0 })).unwrap();
    assert_eq!(spans(&session, 1), [(0.0, 2.0), (6.0, 2.0)]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(spans(&session, 1), [(0.0, 4.0)]);
}

#[test]
fn duplicating_a_range_puts_the_copy_right_after_it() {
    let mut session = Session::new(project());
    run(&mut session, "range.duplicate", json!({ "tracks": BOTH, "start": 0.0, "end": 2.0 })).unwrap();
    for track in BOTH {
        assert_eq!(spans(&session, track), [(0.0, 2.0), (2.0, 2.0)], "the copy replaced what it covered");
    }
}

#[test]
fn a_range_with_nothing_in_it_or_backwards_is_an_error() {
    let mut session = Session::new(project());
    assert!(run(&mut session, "range.copy", json!({ "tracks": BOTH, "start": 10.0, "end": 12.0 })).is_err());
    assert!(run(&mut session, "range.delete", json!({ "tracks": BOTH, "start": 3.0, "end": 1.0 })).is_err());
}

#[test]
fn consolidating_a_span_with_empty_time_makes_one_clip_over_all_of_it() {
    let mut session = Session::new(project());
    // The 4-beat clip and 4 empty beats after it become one 8-beat clip.
    run(&mut session, "clip.consolidate", json!({ "track": 1, "start": 0.0, "end": 8.0 })).unwrap();
    assert_eq!(spans(&session, 1), [(0.0, 8.0)]);
}

#[test]
fn several_clips_move_together_without_replacing_each_other() {
    let mut session = Session::new(project());
    // Two clips side by side on track 1: moving both right by their length must keep both.
    run(&mut session, "range.split", json!({ "tracks": [1], "start": 2.0, "end": 4.0 })).unwrap();
    assert_eq!(spans(&session, 1), [(0.0, 2.0), (2.0, 2.0)]);
    let clips = json!([{ "track": 1, "index": 0 }, { "track": 1, "index": 1 }, { "track": 2, "index": 0 }]);
    run(&mut session, "clip.moveMany", json!({ "clips": clips, "beats": 2.0 })).unwrap();
    assert_eq!(spans(&session, 1), [(2.0, 2.0), (4.0, 2.0)]);
    assert_eq!(spans(&session, 2), [(2.0, 4.0)]);
    // Clips of one track go to another together; across tracks they keep theirs.
    let one = json!([{ "track": 1, "index": 0 }, { "track": 1, "index": 1 }]);
    run(&mut session, "clip.moveMany", json!({ "clips": one, "beats": 0.0, "to_track": 2 })).unwrap();
    assert!(spans(&session, 1).is_empty());
    assert_eq!(spans(&session, 2), [(2.0, 2.0), (4.0, 2.0)], "the moved clips replaced what they covered");
    assert!(
        run(&mut session, "clip.moveMany", json!({ "clips": [{ "track": 2, "index": 0 }], "beats": -10.0 })).is_err()
    );
}
