// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Editing a time selection (`docs/spec/arrangement-editing.md` › Time selection): splitting,
//! deleting, copying and duplicating what plays inside a span of beats over some tracks, and
//! moving several clips at once.
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use serde_json::json;
use suhina_model::*;
use suhina_session::{Session, blank_project};

use crate::support::run;

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
    // The middle parts, each with its id (ADR 0025).
    let parts: Vec<(u64, u64)> = out["clips"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            assert!(c["clip"].as_u64().is_some(), "{c}");
            (c["track"].as_u64().unwrap(), c["index"].as_u64().unwrap())
        })
        .collect();
    assert_eq!(parts, [(1, 1), (2, 1)], "the middle parts");
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
    run(&mut session, "clip.moveMany", json!({ "clips": one, "beats": 0.0, "toTrack": 2 })).unwrap();
    assert!(spans(&session, 1).is_empty());
    assert_eq!(spans(&session, 2), [(2.0, 2.0), (4.0, 2.0)], "the moved clips replaced what they covered");
    assert!(
        run(&mut session, "clip.moveMany", json!({ "clips": [{ "track": 2, "index": 0 }], "beats": -10.0 })).is_err()
    );
}

#[test]
fn copying_clips_by_drag_leaves_them_and_places_copies_of_their_own() {
    let mut session = Session::new(project());
    let clips = json!([{ "track": 1, "index": 0 }, { "track": 2, "index": 0 }]);
    run(&mut session, "clip.moveMany", json!({ "clips": clips, "beats": 8.0, "copy": true })).unwrap();
    for track in BOTH {
        assert_eq!(spans(&session, track), [(0.0, 4.0), (8.0, 4.0)], "the original stays, the copy goes");
    }
    let p = session.project();
    let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(1))).unwrap();
    assert_ne!(lane.clips[0].content, lane.clips[1].content, "the copy plays a content of its own");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(spans(&session, 1), [(0.0, 4.0)], "one undo step");
}

#[test]
fn adding_a_section_inserts_empty_time_and_moves_everything_after_it() {
    let mut p = project();
    p.arrangement.lanes.iter_mut().find(|l| l.track == Some(TrackId(2))).unwrap().clips[0].start = 8.0;
    p.arrangement.markers.push(Marker::new(8.0, "Chorus"));
    p.transport.tempo_changes.push(TempoPoint { beat: 8.0, bpm: 90.0, ramp: false });
    p.transport
        .time_signature_changes
        .push(TimeSignatureChange { bar: 2, time_signature: TimeSignature { numerator: 3, denominator: 4 } });
    p.transport.loop_range = Some(LoopRange { start: 8.0, end: 12.0 });
    let main = p.channels.iter().position(|c| c.role == ChannelRole::Master).unwrap();
    p.channels[main].automation.push(AutomationLane {
        param: "volume".into(),
        points: vec![AutomationPoint::new(1.0, 1.0), AutomationPoint::new(8.0, 0.5)],
        enabled: true,
    });
    let mut session = Session::new(p);
    let before = session.project().clone();
    // One bar at beat 2, inside track 1's clip (0..4): it is split there, its rest moves.
    run(&mut session, "range.insertTime", json!({ "start": 2.0, "end": 6.0 })).unwrap();
    assert_eq!(spans(&session, 1), [(0.0, 2.0), (6.0, 2.0)]);
    assert_eq!(spans(&session, 2), [(12.0, 4.0)]);
    let p = session.project();
    assert_eq!(p.arrangement.markers[0].beat, 12.0);
    assert_eq!(p.transport.tempo_changes[0].beat, 12.0);
    assert_eq!(p.transport.time_signature_changes[0].bar, 3, "a whole bar moves the change a bar");
    assert_eq!(p.transport.loop_range, Some(LoopRange { start: 12.0, end: 16.0 }));
    let points: Vec<f64> = p.channels[main].automation[0].points.iter().map(|pt| pt.beat).collect();
    assert_eq!(points, [1.0, 12.0]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    // One undo step; the id counter stays where it went (ADR 0025).
    assert_eq!(session.project(), &Project { next_id: session.project().next_id, ..before });
    assert!(run(&mut session, "range.insertTime", json!({ "start": 4.0, "end": 4.0 })).is_err());
}

#[test]
fn adding_a_section_refuses_to_push_any_beat_past_the_end_of_the_song() {
    let last = suhina_engine::MAX_START_BEAT;
    let main = |p: &Project| p.channels.iter().position(|c| c.role == ChannelRole::Master).unwrap();
    type Put = fn(&mut Project, f64, usize);
    let cases: [(&str, Put); 3] = [
        ("the loop's end", |p, last, _| p.transport.loop_range = Some(LoopRange { start: 1.0, end: last - 1000.0 })),
        ("a tempo change", |p, last, _| {
            p.transport.tempo_changes.push(TempoPoint { beat: last - 1000.0, bpm: 90.0, ramp: false });
        }),
        ("an automation point", |p, last, main| {
            p.channels[main].automation.push(AutomationLane {
                param: "volume".into(),
                points: vec![AutomationPoint::new(last - 1000.0, 0.5)],
                enabled: true,
            });
        }),
    ];
    for (what, put) in cases {
        let mut p = project();
        let at = main(&p);
        put(&mut p, last, at);
        let mut session = Session::new(p);
        let before = session.project().clone();
        let refused = run(&mut session, "range.insertTime", json!({ "start": 2.0, "end": 2002.0 }));
        assert!(refused.is_err(), "{what} would pass the last beat");
        assert_eq!(session.project(), &before, "{what}: nothing changed");
        // A shorter one fits.
        run(&mut session, "range.insertTime", json!({ "start": 2.0, "end": 502.0 })).unwrap();
    }
}

/// A selection on several tracks moves by as many tracks, each from its own; a drop under the
/// tracks makes a track of the clips' kind for them, in the same undo step.
#[test]
fn clips_move_by_tracks_or_to_a_new_track() {
    let mut session = Session::new(project());
    let added = run(&mut session, "track.add", json!({ "kind": "midi", "after": 2 })).unwrap();
    let third = added["track"].as_u64().unwrap() as u32;
    let clips = json!([{ "track": 1, "index": 0 }, { "track": 2, "index": 0 }]);
    run(&mut session, "clip.moveMany", json!({ "clips": clips, "beats": 0.0, "tracksBy": 1 })).unwrap();
    assert!(spans(&session, 1).is_empty());
    assert_eq!(spans(&session, 2), [(0.0, 4.0)]);
    assert_eq!(spans(&session, third), [(0.0, 4.0)]);
    // Onto a track that cannot hold them (an audio track under the MIDI ones): refused.
    let down = json!([{ "track": 2, "index": 0 }, { "track": third, "index": 0 }]);
    assert!(run(&mut session, "clip.moveMany", json!({ "clips": down, "beats": 0.0, "tracksBy": 1 })).is_err());
    // Under the tracks: a new MIDI track holds the copy.
    let tracks = session.project().tracks.len();
    let one = json!([{ "track": third, "index": 0 }]);
    let moved =
        run(&mut session, "clip.moveMany", json!({ "clips": one, "beats": 4.0, "copy": true, "toNewTrack": true }))
            .unwrap();
    assert_eq!(session.project().tracks.len(), tracks + 1);
    let new_id = moved["clips"][0]["track"].as_u64().unwrap() as u32;
    let new = session.project().tracks.iter().find(|t| t.id.0 == new_id).unwrap();
    assert!(new.content.notes, "a MIDI track for a MIDI clip");
    assert_eq!(spans(&session, new.id.0), [(4.0, 4.0)]);
    assert_eq!(spans(&session, third), [(0.0, 4.0)], "copied, the clip stays");
    // One undo takes the copy and its track away together.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().tracks.len(), tracks);
}

/// `ara_edits` (ADR 0028 step 6): keep carries the edits of clips a plug-in edits; clips no
/// plug-in edits move as ever and carry nothing; anything but keep or new is refused.
#[test]
fn keeping_ara_edits_moves_plain_clips_as_ever() {
    let mut session = Session::new(project());
    let one = json!([{ "track": 1, "index": 0 }]);
    let moved =
        run(&mut session, "clip.moveMany", json!({ "clips": one, "beats": 0.0, "toTrack": 2, "araEdits": "keep" }))
            .unwrap();
    assert_eq!(moved["edits_carried"], 0, "no plug-in edits these clips");
    assert!(spans(&session, 1).is_empty());
    assert!(!session.carrying_ara_edits());
    let back = json!([{ "track": 2, "index": 0 }]);
    assert!(
        run(&mut session, "clip.moveMany", json!({ "clips": back, "beats": 0.0, "toTrack": 1, "araEdits": "maybe" }))
            .is_err()
    );
}
