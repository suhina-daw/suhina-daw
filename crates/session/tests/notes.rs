// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `note.*` commands: editing a MIDI clip's notes, one undo step each, and refusing bad input
//! (`docs/spec/ui-clip-editor.md` › Commands).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use serde_json::{Value, json};
use suhina_model::*;
use suhina_session::{MAX_NOTES_PER_COMMAND, Session};

use crate::support::run;

fn note(time: f64, key: u8) -> Note {
    Note::new(time, 1.0, key, 100.0 / 127.0)
}

fn clip(content: u32, start: f64) -> Clip {
    Clip::new(ContentId(content), start, 4.0)
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
        content: ContentKinds { notes: true, audio: true },
        ..Track::new(TrackId(id), name)
    };
    let notes = |n: Vec<Note>| ContentLane { target: None, data: LaneData::Notes(n) };
    let audio = AudioRegion::new("a.wav");
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
fn notes_are_listed_with_midi_velocities() {
    let mut session = Session::new(project());
    let listed = run(&mut session, "note.list", json!({ "track": "Keys", "index": 0 })).unwrap();
    assert_eq!(
        listed[1],
        json!({ "index": 1, "time": 1.0, "length": 1.0, "key": 64, "velocity": 100, "muted": false })
    );
    assert_eq!(listed.as_array().map(Vec::len), Some(3));
    // An empty MIDI clip has none; a slot's clip is named by its scene.
    assert_eq!(run(&mut session, "note.list", json!({ "track": 1, "index": 1 })).unwrap(), json!([]));
    let slot = run(&mut session, "note.list", json!({ "track": 1, "scene": 1 })).unwrap();
    assert_eq!(slot[0]["key"], 48);
}

#[test]
fn adding_appends_notes_and_undoes() {
    let mut session = Session::new(project());
    let before = notes(&session, 1);
    let added = run(
        &mut session,
        "note.add",
        json!({ "track": 1, "index": 0, "notes": [
            { "time": 3.0, "length": 0.5, "key": 72 },
            { "time": 0.25, "length": 0.25, "key": 0, "velocity": 127 },
        ] }),
    )
    .unwrap();
    assert_eq!(added, json!({ "added": [3, 4], "count": 5 }));
    let now = notes(&session, 1);
    assert_eq!(&now[..3], &before[..]);
    assert_eq!((now[3].time, now[3].duration, now[3].key), (3.0, 0.5, 72));
    assert_eq!(suhina_session::midi_velocity(now[3].velocity), 100);
    assert_eq!(now[4].velocity, 1.0);
    undoes(&mut session, 1, &before);
}

#[test]
fn an_empty_clip_and_a_slot_clip_take_notes() {
    let mut session = Session::new(project());
    let one = json!([{ "time": 0.0, "length": 1.0, "key": 60 }]);
    run(&mut session, "note.add", json!({ "track": 1, "index": 1, "notes": one })).unwrap();
    assert_eq!(notes(&session, 2).len(), 1);
    run(&mut session, "note.add", json!({ "track": 1, "scene": 1, "notes": one })).unwrap();
    assert_eq!(notes(&session, 5).len(), 2);
}

#[test]
fn deleting_removes_the_notes_and_later_ones_move_up() {
    let mut session = Session::new(project());
    let before = notes(&session, 1);
    let result = run(&mut session, "note.delete", json!({ "track": 1, "index": 0, "notes": [2, 0, 2] })).unwrap();
    assert_eq!(result, json!({ "deleted": [0, 2], "count": 1 }));
    assert_eq!(notes(&session, 1), [before[1]]);
    undoes(&mut session, 1, &before);
}

#[test]
fn moving_shifts_time_and_pitch_together() {
    let mut session = Session::new(project());
    let before = notes(&session, 1);
    run(&mut session, "note.move", json!({ "track": 1, "index": 0, "notes": [1, 2], "beats": 0.5, "semitones": -12 }))
        .unwrap();
    let now = notes(&session, 1);
    assert_eq!(now[0], before[0]);
    assert_eq!((now[1].time, now[1].key), (1.5, 52));
    assert_eq!((now[2].time, now[2].key), (2.5, 55));
    undoes(&mut session, 1, &before);
    // Exactly to the start is allowed; past it, or out of the keys, the whole move fails.
    run(&mut session, "note.move", json!({ "track": 1, "index": 0, "notes": [1], "beats": -1.0 })).unwrap();
    assert_eq!(notes(&session, 1)[1].time, 0.0);
    let past = run(&mut session, "note.move", json!({ "track": 1, "index": 0, "notes": [0, 2], "beats": -1.0 }));
    assert!(past.unwrap_err().contains("note 0"));
    let high = run(&mut session, "note.move", json!({ "track": 1, "index": 0, "notes": [2], "semitones": 61 }));
    assert!(high.unwrap_err().contains("keys 0 to 127"));
    assert_eq!(notes(&session, 1)[2], before[2]);
}

#[test]
fn a_moved_note_with_replace_takes_the_place_of_the_one_it_lands_on() {
    let mut session = Session::new(project());
    let before = notes(&session, 1);
    // Note 1 (beat 1, key 64) onto note 2 (beat 2, key 67).
    let moved = json!({ "track": 1, "index": 0, "notes": [1], "beats": 1.0, "semitones": 3, "replace": true });
    assert_eq!(run(&mut session, "note.move", moved).unwrap(), json!({ "notes": [1], "replaced": [2] }));
    assert_eq!(notes(&session, 1).len(), before.len() - 1);
    undoes(&mut session, 1, &before);
    // Without replace they stack; a "move" by nothing then covers nothing.
    run(&mut session, "note.move", json!({ "track": 1, "index": 0, "notes": [1], "beats": 1.0, "semitones": 3 }))
        .unwrap();
    let stacked = notes(&session, 1);
    let still = json!({ "track": 1, "index": 0, "notes": [1], "beats": 0.0, "replace": true });
    assert_eq!(run(&mut session, "note.move", still).unwrap()["replaced"], json!([]));
    assert_eq!(notes(&session, 1), stacked);
}

#[test]
fn resizing_sets_or_changes_lengths() {
    let mut session = Session::new(project());
    let before = notes(&session, 1);
    run(&mut session, "note.resize", json!({ "track": 1, "index": 0, "notes": [0, 1], "length": 0.25 })).unwrap();
    assert_eq!(notes(&session, 1).iter().map(|n| n.duration).collect::<Vec<_>>(), [0.25, 0.25, 1.0]);
    undoes(&mut session, 1, &before);
    run(&mut session, "note.resize", json!({ "track": 1, "index": 0, "notes": [2], "by": 1.5 })).unwrap();
    assert_eq!(notes(&session, 1)[2].duration, 2.5);
    for bad in [json!({ "by": -1.0 }), json!({ "length": 0.0 }), json!({ "length": -2.0 }), json!({ "length": 1e9 })] {
        let mut params = json!({ "track": 1, "index": 0, "notes": [0] });
        params.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
        assert!(run(&mut session, "note.resize", params).is_err(), "{bad}");
    }
    let both = run(&mut session, "note.resize", json!({ "track": 1, "index": 0, "notes": [0], "length": 1, "by": 1 }));
    assert!(both.is_err());
}

#[test]
fn velocity_is_set_or_changed_within_1_to_127() {
    let mut session = Session::new(project());
    let before = notes(&session, 1);
    let velocity =
        |s: &Session| notes(s, 1).iter().map(|n| suhina_session::midi_velocity(n.velocity)).collect::<Vec<_>>();
    run(&mut session, "note.setVelocity", json!({ "track": 1, "index": 0, "notes": [0], "velocity": 20 })).unwrap();
    assert_eq!(velocity(&session), [20, 100, 100]);
    undoes(&mut session, 1, &before);
    run(&mut session, "note.setVelocity", json!({ "track": 1, "index": 0, "notes": [0, 1, 2], "by": 40 })).unwrap();
    assert_eq!(velocity(&session), [127, 127, 127]);
    run(&mut session, "note.setVelocity", json!({ "track": 1, "index": 0, "notes": [0], "by": -127 })).unwrap();
    assert_eq!(velocity(&session)[0], 1);
    for bad in [0.0, 128.0, -5.0] {
        let set =
            run(&mut session, "note.setVelocity", json!({ "track": 1, "index": 0, "notes": [0], "velocity": bad }));
        assert!(set.is_err(), "{bad}");
    }
}

#[test]
fn malformed_params_are_refused_and_change_nothing() {
    let mut session = Session::new(project());
    let before = session.project().clone();
    let one = json!([{ "time": 0.0, "length": 1.0, "key": 60 }]);
    let cases = [
        // Which clip.
        ("note.list", json!({ "track": 9, "index": 0 }), "does not exist"),
        ("note.list", json!({ "track": 1 }), "give index"),
        ("note.list", json!({ "track": 1, "index": 0, "scene": 1 }), "not both"),
        ("note.list", json!({ "track": 1, "index": 7 }), "clip index 7"),
        ("note.list", json!({ "track": 1, "index": -1 }), "whole number"),
        ("note.list", json!({ "track": 1, "scene": 2 }), "empty"),
        ("note.list", json!({ "track": 1, "scene": 0 }), "scene 0"),
        ("note.list", json!({ "track": 2, "scene": 1 }), "no clip slots"),
        ("note.list", json!({ "track": 1, "index": 2 }), "audio clip"),
        ("note.add", json!({ "track": 1, "index": 2, "notes": one }), "audio clip"),
        ("note.add", json!({ "track": 1, "index": 3, "notes": one }), "pattern"),
        // The new notes.
        ("note.add", json!({ "track": 1, "index": 0 }), "notes must be"),
        ("note.add", json!({ "track": 1, "index": 0, "notes": [] }), "empty"),
        ("note.add", json!({ "track": 1, "index": 0, "notes": [{ "time": 0, "length": 1, "key": 128 }] }), "key"),
        ("note.add", json!({ "track": 1, "index": 0, "notes": [{ "time": 0, "length": 1, "key": -1 }] }), "key"),
        ("note.add", json!({ "track": 1, "index": 0, "notes": [{ "time": -1, "length": 1, "key": 60 }] }), "time"),
        ("note.add", json!({ "track": 1, "index": 0, "notes": [{ "time": 1e12, "length": 1, "key": 60 }] }), "time"),
        ("note.add", json!({ "track": 1, "index": 0, "notes": [{ "time": 0, "length": -1, "key": 60 }] }), "length"),
        ("note.add", json!({ "track": 1, "index": 0, "notes": [{ "time": 0, "length": 0, "key": 60 }] }), "length"),
        (
            "note.add",
            json!({ "track": 1, "index": 0, "notes": [{ "time": 0, "length": 1, "key": 60, "velocity": 0 }] }),
            "velocity",
        ),
        (
            "note.add",
            json!({ "track": 1, "index": 0, "notes": [{ "time": 0, "length": 1, "key": 60, "pitch": 3 }] }),
            "unknown field",
        ),
        ("note.add", json!({ "track": 1, "index": 0, "notes": ["C3"] }), "object"),
        // Which notes.
        ("note.delete", json!({ "track": 1, "index": 0, "notes": [3] }), "note 3 does not exist"),
        ("note.delete", json!({ "track": 1, "index": 0, "notes": [-1] }), "whole number"),
        ("note.delete", json!({ "track": 1, "index": 0, "notes": "all" }), "list"),
        ("note.delete", json!({ "track": 1, "index": 0, "notes": [] }), "empty"),
        ("note.move", json!({ "track": 1, "index": 0, "notes": [0] }), "give beats"),
        ("note.move", json!({ "track": 1, "index": 0, "notes": [0], "semitones": 0.5 }), "semitones"),
        ("note.move", json!({ "track": 1, "index": 0, "notes": [0], "beats": "later" }), "beats"),
        ("note.move", json!({ "track": 1, "index": 0, "notes": [0], "beats": 1e300 }), "beats"),
        ("note.setVelocity", json!({ "track": 1, "index": 0, "notes": [0] }), "give velocity"),
        ("note.setVelocity", json!({ "track": 1, "index": 0, "notes": [0], "by": 1000 }), "by"),
    ];
    for (id, params, error) in cases {
        let got = run(&mut session, id, params.clone());
        assert!(got.as_ref().is_err_and(|e| e.contains(error)), "{id} {params}: {got:?}");
    }
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
}

#[test]
fn huge_inputs_are_capped() {
    let mut session = Session::new(project());
    let many: Vec<Value> = (0..=MAX_NOTES_PER_COMMAND).map(|i| json!({ "time": i, "length": 1, "key": 60 })).collect();
    let added = run(&mut session, "note.add", json!({ "track": 1, "index": 0, "notes": many }));
    assert!(added.unwrap_err().contains("at most"));
    let indices: Vec<usize> = (0..=MAX_NOTES_PER_COMMAND).collect();
    let deleted = run(&mut session, "note.delete", json!({ "track": 1, "index": 0, "notes": indices }));
    assert!(deleted.unwrap_err().contains("at most"));
    // A clip never holds more than its cap: ten full commands fill it, the eleventh fails.
    let full: Vec<Value> =
        (0..MAX_NOTES_PER_COMMAND).map(|i| json!({ "time": (i % 64) as f64, "length": 1, "key": 60 })).collect();
    for _ in 0..10 {
        run(&mut session, "note.add", json!({ "track": 1, "index": 1, "notes": full })).unwrap();
    }
    let over = run(&mut session, "note.add", json!({ "track": 1, "index": 1, "notes": [full[0]] }));
    assert!(over.unwrap_err().contains("at most"));
}

/// The content each clip on the Keys lane plays, in order.
fn arranged(session: &Session) -> Vec<u32> {
    session.project().arrangement.lanes[0].clips.iter().map(|c| c.content.0).collect()
}

#[test]
fn editing_a_duplicate_leaves_the_original_alone_and_undoes_both() {
    let mut session = Session::new(project());
    let original = notes(&session, 1);
    let made =
        run(&mut session, "clip.duplicate", json!({ "clips": [{ "track": 1, "index": 0 }], "stack": true })).unwrap();
    let copy = made["clips"][0]["index"].as_u64().unwrap() as usize;
    assert_eq!(arranged(&session)[copy], 1, "a duplicate plays the same content until it is edited");
    let before_edit = session.project().clone();

    run(&mut session, "note.move", json!({ "track": 1, "index": copy, "notes": [0], "semitones": 12 })).unwrap();
    // The duplicate now plays a content of its own with the edit; the original keeps its notes.
    let own = arranged(&session)[copy];
    assert_ne!(own, 1);
    assert_eq!(arranged(&session)[0], 1);
    assert_eq!(notes(&session, 1), original);
    assert_eq!(notes(&session, own)[0].key, 72);
    assert_eq!(&notes(&session, own)[1..], &original[1..]);
    assert_eq!(session.project().content(ContentId(own)).map(|c| c.name.as_str()), Some("C1"));

    // A second edit goes to the copy, which no other clip plays: no further copy.
    let contents = session.project().contents.len();
    run(&mut session, "note.delete", json!({ "track": 1, "index": copy, "notes": [2] })).unwrap();
    assert_eq!(session.project().contents.len(), contents);
    assert_eq!(notes(&session, own).len(), 2);
    assert_eq!(notes(&session, 1), original);

    // One undo per edit: the copy and the edit go back together.
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(*session.project(), before_edit);
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(arranged(&session)[copy], own);
    assert_eq!(notes(&session, 1), original);
}

#[test]
fn editing_the_original_of_a_duplicate_leaves_the_duplicate_alone() {
    let mut session = Session::new(project());
    let original = notes(&session, 1);
    let made =
        run(&mut session, "clip.duplicate", json!({ "clips": [{ "track": 1, "index": 0 }], "stack": true })).unwrap();
    let copy = made["clips"][0]["index"].as_u64().unwrap() as usize;
    run(&mut session, "note.setVelocity", json!({ "track": 1, "index": 0, "notes": [0, 1, 2], "velocity": 20 }))
        .unwrap();
    assert_eq!(arranged(&session)[copy], 1);
    assert_eq!(notes(&session, 1), original);
    let own = arranged(&session)[0];
    assert!(notes(&session, own).iter().all(|n| suhina_session::midi_velocity(n.velocity) == 20));
}

#[test]
fn quantising_a_duplicate_leaves_the_original_alone() {
    let mut session = Session::new(project());
    run(
        &mut session,
        "note.add",
        json!({ "track": 1, "index": 0, "notes": [{ "time": 0.3, "length": 1, "key": 70 }] }),
    )
    .unwrap();
    let original = notes(&session, 1);
    let made =
        run(&mut session, "clip.duplicate", json!({ "clips": [{ "track": 1, "index": 0 }], "stack": true })).unwrap();
    let copy = made["clips"][0]["index"].as_u64().unwrap() as usize;
    run(&mut session, "note.quantize", json!({ "track": 1, "index": copy, "grid": "1/4" })).unwrap();
    let own = arranged(&session)[copy];
    assert_ne!(own, 1);
    assert_eq!(notes(&session, 1), original);
    assert_eq!(notes(&session, own)[3].time, 0.0);
}

#[test]
fn a_launcher_clip_sharing_an_arrangement_clips_content_is_copied_when_edited() {
    let mut p = project();
    p.launcher.columns[0].slots[1].clip = Some(clip(1, 0.0));
    let mut session = Session::new(p);
    let original = notes(&session, 1);
    run(
        &mut session,
        "note.add",
        json!({ "track": 1, "scene": 2, "notes": [{ "time": 3.0, "length": 1.0, "key": 50 }] }),
    )
    .unwrap();
    assert_eq!(notes(&session, 1), original);
    let own = session.project().launcher.columns[0].slots[1].clip.as_ref().unwrap().content.0;
    assert_ne!(own, 1);
    assert_eq!(notes(&session, own).len(), 4);
    assert_eq!(arranged(&session)[0], 1);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().launcher.columns[0].slots[1].clip.as_ref().unwrap().content.0, 1);
}

#[test]
fn an_edit_that_changes_nothing_or_fails_makes_no_copy() {
    let mut session = Session::new(project());
    run(&mut session, "clip.duplicate", json!({ "clips": [{ "track": 1, "index": 0 }], "stack": true })).unwrap();
    let before = session.project().clone();
    // A move by nothing changes no note, and so adds no undo step.
    run(&mut session, "note.move", json!({ "track": 1, "index": 0, "notes": [0], "beats": 0.0 })).unwrap();
    assert_eq!(*session.project(), before);
    // A refused edit leaves the project as it was.
    assert!(run(&mut session, "note.move", json!({ "track": 1, "index": 0, "notes": [0], "semitones": 100 })).is_err());
    assert_eq!(*session.project(), before);
    // The one undo step there is takes back the duplicate.
    run(&mut session, "edit.undo", json!({})).unwrap();
    // Undo takes the edit back; the id counter stays where it got to (ADR 0025).
    assert_eq!(*session.project(), Project { next_id: session.project().next_id, ..project() });
}

#[test]
fn an_empty_duplicate_gets_its_own_content_when_notes_are_added() {
    let mut session = Session::new(project());
    let made =
        run(&mut session, "clip.duplicate", json!({ "clips": [{ "track": 1, "index": 1 }], "stack": true })).unwrap();
    let copy = made["clips"][0]["index"].as_u64().unwrap() as usize;
    run(
        &mut session,
        "note.add",
        json!({ "track": 1, "index": copy, "notes": [{ "time": 0.0, "length": 1.0, "key": 60 }] }),
    )
    .unwrap();
    assert!(session.project().content(ContentId(2)).unwrap().lanes.is_empty());
    assert_eq!(notes(&session, arranged(&session)[copy]).len(), 1);
}

#[test]
fn a_shared_pattern_stays_shared_when_edited() {
    // One pattern with a lane for each track, placed twice on the Keys lane.
    let mut p = project();
    p.contents.push(content(
        6,
        vec![
            ContentLane { target: Some(TrackId(1)), data: LaneData::Notes(vec![note(0.0, 60)]) },
            ContentLane { target: Some(TrackId(2)), data: LaneData::Notes(vec![note(1.0, 36)]) },
        ],
    ));
    p.arrangement.lanes[0].clips.extend([clip(6, 16.0), clip(6, 20.0)]);
    let mut session = Session::new(p);
    run(&mut session, "note.move", json!({ "track": 1, "index": 5, "notes": [0], "semitones": 2 })).unwrap();
    // Both placements still play the one pattern, now with the edit; the other track's lane is
    // untouched.
    assert_eq!(&arranged(&session)[4..], &[6, 6]);
    assert_eq!(session.project().contents.len(), 6);
    let pattern = session.project().content(ContentId(6)).unwrap();
    let keys: Vec<u8> = pattern
        .lanes
        .iter()
        .filter_map(|l| match &l.data {
            LaneData::Notes(n) => n.first().map(|n| n.key),
            LaneData::Audio(_) => None,
        })
        .collect();
    assert_eq!(keys, [62, 36]);
}

#[test]
fn pasting_into_a_duplicate_leaves_the_original_alone() {
    let mut session = Session::new(project());
    let original = notes(&session, 1);
    let made =
        run(&mut session, "clip.duplicate", json!({ "clips": [{ "track": 1, "index": 0 }], "stack": true })).unwrap();
    let copy = made["clips"][0]["index"].as_u64().unwrap() as usize;
    run(&mut session, "note.copy", json!({ "track": 1, "index": 0, "notes": [0] })).unwrap();
    run(&mut session, "note.paste", json!({ "track": 1, "index": copy })).unwrap();
    let own = arranged(&session)[copy];
    assert_ne!(own, 1);
    assert_eq!(notes(&session, 1), original);
    assert_eq!(notes(&session, own).len(), original.len() + 1);
}

#[test]
fn copied_notes_paste_with_their_spacing_after_the_copy_or_at_a_time() {
    let mut session = Session::new(project());
    let before = notes(&session, 1);
    // Copy changes nothing and is no undo step.
    let copied = run(&mut session, "note.copy", json!({ "track": 1, "index": 0, "notes": [2, 1] })).unwrap();
    assert_eq!(copied, json!({ "copied": 2, "span": 2.0 }));
    assert_eq!(session.copied_notes(), Some(2));
    assert!(!session.can_undo());
    // Without a time: right after the copied notes (they spanned beats 1 to 3).
    let pasted = run(&mut session, "note.paste", json!({ "track": 1, "index": 0 })).unwrap();
    assert_eq!(pasted, json!({ "added": [3, 4], "count": 5 }));
    let now = notes(&session, 1);
    assert_eq!((now[3].time, now[3].key), (3.0, 64));
    assert_eq!((now[4].time, now[4].key), (4.0, 67));
    assert_eq!(now[3].velocity, before[1].velocity);
    undoes(&mut session, 1, &before);
    // At a time, into another clip, and again (the clipboard stays).
    run(&mut session, "note.paste", json!({ "track": 1, "index": 1, "time": 0.5 })).unwrap();
    run(&mut session, "note.paste", json!({ "track": 1, "scene": 1, "time": 8 })).unwrap();
    let empty = notes(&session, 2);
    assert_eq!(empty.iter().map(|n| (n.time, n.key)).collect::<Vec<_>>(), [(0.5, 64), (1.5, 67)]);
    assert_eq!(notes(&session, 5).len(), 3);
}

#[test]
fn cutting_copies_and_deletes_in_one_undo_step() {
    let mut session = Session::new(project());
    let before = notes(&session, 1);
    let cut = run(&mut session, "note.cut", json!({ "track": 1, "index": 0, "notes": [0] })).unwrap();
    assert_eq!(cut, json!({ "deleted": [0], "count": 2 }));
    assert_eq!(notes(&session, 1), &before[1..]);
    undoes(&mut session, 1, &before);
    // The cut note stays copied after the undo.
    run(&mut session, "note.paste", json!({ "track": 1, "index": 0, "time": 3 })).unwrap();
    assert_eq!((notes(&session, 1)[3].time, notes(&session, 1)[3].key), (3.0, 60));
}

#[test]
fn duplicating_puts_the_copy_right_after_the_notes() {
    let mut session = Session::new(project());
    let before = notes(&session, 1);
    let result = run(&mut session, "note.duplicate", json!({ "track": 1, "index": 0, "notes": [0, 1] })).unwrap();
    assert_eq!(result, json!({ "added": [3, 4], "count": 5 }));
    let now = notes(&session, 1);
    assert_eq!(now.iter().skip(3).map(|n| (n.time, n.key)).collect::<Vec<_>>(), [(2.0, 60), (3.0, 64)]);
    // Duplicate leaves the clipboard alone.
    assert_eq!(session.copied_notes(), None);
    undoes(&mut session, 1, &before);
}

#[test]
fn malformed_clipboard_commands_are_refused_and_change_nothing() {
    let mut session = Session::new(project());
    let before = session.project().clone();
    let nothing = run(&mut session, "note.paste", json!({ "track": 1, "index": 0 }));
    assert!(nothing.unwrap_err().contains("no notes have been copied"));
    let cases = [
        ("note.copy", json!({ "track": 1, "index": 0 }), "notes must be"),
        ("note.copy", json!({ "track": 1, "index": 0, "notes": [] }), "empty"),
        ("note.copy", json!({ "track": 1, "index": 0, "notes": [9] }), "note 9 does not exist"),
        ("note.copy", json!({ "track": 1, "index": 2, "notes": [0] }), "audio clip"),
        ("note.cut", json!({ "track": 1, "index": 0, "notes": [-1] }), "whole number"),
        ("note.cut", json!({ "track": 1, "scene": 2, "notes": [0] }), "empty"),
        ("note.duplicate", json!({ "track": 1, "index": 0, "notes": "all" }), "list"),
        ("note.duplicate", json!({ "track": 1, "index": 1, "notes": [0] }), "does not exist"),
    ];
    for (id, params, error) in cases {
        let got = run(&mut session, id, params.clone());
        assert!(got.as_ref().is_err_and(|e| e.contains(error)), "{id} {params}: {got:?}");
    }
    assert_eq!(session.copied_notes(), None, "a failed copy or cut copies nothing");
    run(&mut session, "note.copy", json!({ "track": 1, "index": 0, "notes": [0, 2] })).unwrap();
    let pastes = [
        (json!({ "track": 1, "index": 0, "time": -1 }), "time must be"),
        (json!({ "track": 1, "index": 0, "time": "now" }), "time must be"),
        (json!({ "track": 1, "index": 0, "time": 1e12 }), "time must be"),
        // The first note fits, the last would start past the furthest beat.
        (json!({ "track": 1, "index": 0, "time": 119_999.0 }), "outside 0 to"),
        (json!({ "track": 1, "index": 2 }), "audio clip"),
        (json!({ "track": 1, "index": 3 }), "pattern"),
        (json!({ "track": 1, "index": 0, "notes": [0] }), "unknown parameter"),
    ];
    for (params, error) in pastes {
        let got = run(&mut session, "note.paste", params.clone());
        assert!(got.as_ref().is_err_and(|e| e.contains(error)), "{params}: {got:?}");
    }
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
}

#[test]
fn pasting_never_fills_a_clip_past_its_cap() {
    let mut session = Session::new(project());
    let full: Vec<Value> =
        (0..MAX_NOTES_PER_COMMAND).map(|i| json!({ "time": (i % 64) as f64, "length": 1, "key": 60 })).collect();
    for _ in 0..10 {
        run(&mut session, "note.add", json!({ "track": 1, "index": 1, "notes": full })).unwrap();
    }
    run(&mut session, "note.copy", json!({ "track": 1, "index": 0, "notes": [0] })).unwrap();
    let over = run(&mut session, "note.paste", json!({ "track": 1, "index": 1 }));
    assert!(over.unwrap_err().contains("at most"));
    let twice = run(&mut session, "note.duplicate", json!({ "track": 1, "index": 1, "notes": [0] }));
    assert!(twice.unwrap_err().contains("at most"));
}

#[test]
fn splitting_cuts_each_note_that_spans_the_beat_in_two() {
    let mut session = Session::new(project());
    // The clip's notes are at beats 0, 1 and 2, each a beat long; a note at 0.5 to 2.5 too.
    run(
        &mut session,
        "note.add",
        json!({ "track": 1, "index": 0, "notes": [{ "time": 0.5, "length": 2.0, "key": 48, "velocity": 60 }] }),
    )
    .unwrap();
    let before = session.project().clone();
    let kept = notes(&session, 1);
    let cut = run(&mut session, "note.split", json!({ "track": 1, "index": 0, "notes": [0, 3], "at": 1.5 })).unwrap();
    // Only the note that spans 1.5 (0.5 to 2.5) is cut: note 0 ends exactly at 1.0.
    assert_eq!(cut, json!({ "notes": [4], "count": 1 }));
    let now = notes(&session, 1);
    assert_eq!(now.len(), 5);
    assert_eq!((now[3].time, now[3].duration, now[3].key), (0.5, 1.0, 48));
    assert_eq!((now[4].time, now[4].duration, now[4].key), (1.5, 1.0, 48));
    // The halves keep the note's velocity and the notes the command did not touch are untouched.
    assert_eq!(now[4].velocity, now[3].velocity);
    assert_eq!(&now[..3], &kept[..3]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(*session.project(), before);
    // The undo put the note that spans 2.0 back: 0.5 to 2.5, one note again.
    let restored = notes(&session, 1);
    assert_eq!((restored.len(), restored[3].time, restored[3].duration), (4, 0.5, 2.0));

    // Splitting again at 2.0: the note that now spans it (0.5 to 2.5) is cut, and a note that
    // only touches the beat (1.0 to 2.0) is left whole even though it is named.
    run(&mut session, "note.split", json!({ "track": 1, "index": 0, "notes": [1, 3], "at": 2.0 })).unwrap();
    let now = notes(&session, 1);
    // One note spans 2.0 (0.5 to 2.5); note 1 (1.0 to 2.0) only touches it and is left whole.
    assert_eq!(now.len(), 5);
    assert_eq!((now[3].time, now[3].duration), (0.5, 1.5));
    assert_eq!((now[4].time, now[4].duration), (2.0, 0.5));
    assert_eq!(now[1], kept[1]);
}

#[test]
fn splitting_a_duplicate_leaves_the_original_alone() {
    let mut session = Session::new(project());
    run(
        &mut session,
        "note.add",
        json!({ "track": 1, "index": 0, "notes": [{ "time": 0.25, "length": 2.0, "key": 70 }] }),
    )
    .unwrap();
    let original = notes(&session, 1);
    let made =
        run(&mut session, "clip.duplicate", json!({ "clips": [{ "track": 1, "index": 0 }], "stack": true })).unwrap();
    let copy = made["clips"][0]["index"].as_u64().unwrap() as usize;
    run(&mut session, "note.split", json!({ "track": 1, "index": copy, "notes": [3], "at": 1.0 })).unwrap();
    let own = arranged(&session)[copy];
    assert_ne!(own, 1);
    assert_eq!(notes(&session, 1), original);
    assert_eq!(notes(&session, own).len(), 5);
    assert_eq!(notes(&session, own)[4].time, 1.0);
}

#[test]
fn malformed_splits_are_refused_and_change_nothing() {
    let mut session = Session::new(project());
    let before = session.project().clone();
    let cases = [
        ("note.split", json!({ "track": 1, "index": 0, "notes": [0] }), "at must be given"),
        ("note.split", json!({ "track": 1, "index": 0, "notes": [0], "at": -1.0 }), "at must be"),
        ("note.split", json!({ "track": 1, "index": 0, "notes": [0], "at": 1e12 }), "at must be"),
        ("note.split", json!({ "track": 1, "index": 0, "notes": [0], "at": "halfway" }), "at must be"),
        ("note.split", json!({ "track": 1, "index": 0, "at": 1.0 }), "notes must be"),
        ("note.split", json!({ "track": 1, "index": 0, "notes": [9], "at": 1.0 }), "note 9 does not exist"),
        ("note.split", json!({ "track": 1, "index": 2, "notes": [0], "at": 1.0 }), "audio clip"),
        ("note.split", json!({ "track": 1, "scene": 2, "notes": [0], "at": 1.0 }), "empty"),
    ];
    for (id, params, error) in cases {
        let got = run(&mut session, id, params.clone());
        assert!(got.as_ref().is_err_and(|e| e.contains(error)), "{id} {params}: {got:?}");
    }
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
}
