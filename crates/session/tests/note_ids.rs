// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Notes named by id (#1182): a `note.*` command made from an earlier read names the notes it
//! read, after other edits have added, deleted or moved notes; an edit that changes or moves a
//! note keeps its id, and a note an edit makes is a new one.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use serde_json::{Value, json};
use suhina_model::*;
use suhina_session::Session;

use crate::support::run;

fn note(time: f64, key: u8) -> Note {
    Note::new(time, 1.0, key, 100.0 / 127.0)
}

fn content(id: u32, notes: Vec<Note>) -> ClipContent {
    ClipContent {
        id: ContentId(id),
        name: format!("C{id}"),
        length: 8.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Notes(notes.into()) }].into(),
        extensions: Extensions::default(),
        scale: None,
    }
}

/// Track 1 has two arrangement clips: clip 0 plays content 1, C D E F a beat apart, and clip 1
/// plays content 2, one low C.
fn session() -> Session {
    let project = Project {
        tracks: vec![Track::new(TrackId(1), "Keys")],
        contents: vec![
            content(1, vec![note(0.0, 60), note(1.0, 62), note(2.0, 64), note(3.0, 65)]),
            content(2, vec![note(0.0, 48)]),
        ],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "Keys".into(),
                track: Some(TrackId(1)),
                clips: vec![Clip::new(ContentId(1), 0.0, 8.0), Clip::new(ContentId(2), 8.0, 8.0)],
            }],
            markers: Default::default(),
            takes: Vec::new(),
        },
        ..Project::default()
    };
    Session::new(suhina_test_support::with_ids(project))
}

/// The params that name clip 0 of track 1, with `more`.
fn at(more: Value) -> Value {
    let mut params = json!({ "track": 1, "index": 0 });
    if let (Some(params), Some(more)) = (params.as_object_mut(), more.as_object()) {
        params.extend(more.clone());
    }
    params
}

/// Content `id`'s notes.
fn notes_of(session: &Session, id: u32) -> Notes {
    let content = session.project().content(ContentId(id)).expect("the content");
    let notes = match &content.lanes[0].data {
        LaneData::Notes(notes) => Some(notes.clone()),
        LaneData::Audio(_) => None,
    };
    notes.expect("a MIDI content")
}

/// The ids of content `id`'s notes, in their order.
fn ids(session: &Session, id: u32) -> Vec<u64> {
    notes_of(session, id).ids().iter().map(|id| id.0).collect()
}

/// The ids `note.list` gives for clip 0, by index.
fn listed(session: &mut Session) -> Vec<u64> {
    let listed = run(session, "note.list", at(json!({}))).unwrap();
    listed.as_array().unwrap().iter().map(|n| n["id"].as_u64().unwrap()).collect()
}

fn key_of(session: &Session, id: u64) -> u8 {
    let notes = notes_of(session, 1);
    notes[notes.position(NoteId(id)).expect("the note is still there")].key
}

#[test]
fn every_note_is_listed_with_its_id_and_no_id_twice() {
    let mut session = session();
    let listed = listed(&mut session);
    assert_eq!(listed, ids(&session, 1));
    let mut unique = listed.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 4);
    assert!(listed.iter().all(|id| *id != 0), "0 is never an id: {listed:?}");
}

#[test]
fn a_command_from_a_stale_read_names_the_notes_it_read() {
    let mut session = session();
    // An agent reads the notes and means E (index 2) and F (index 3).
    let read = listed(&mut session);
    let (e, f) = (read[2], read[3]);
    // Before it sends its command, another writer deletes C and adds a note at the end.
    run(&mut session, "note.delete", at(json!({ "notes": [0] }))).unwrap();
    run(&mut session, "note.add", at(json!({ "notes": [{ "time": 4.0, "length": 1.0, "key": 72 }] }))).unwrap();
    // By index, [2, 3] would now be F and the new note; by id it is still E and F.
    run(&mut session, "note.setVelocity", at(json!({ "notes": [{ "id": e }, { "id": f }], "velocity": 20 }))).unwrap();
    let notes = notes_of(&session, 1);
    let quiet: Vec<u8> =
        notes.iter().filter(|n| suhina_session::midi_velocity(n.velocity) == 20).map(|n| n.key).collect();
    assert_eq!(quiet, [64, 65]);
    // And a delete by id deletes the note named, wherever it is now.
    let result = run(&mut session, "note.delete", at(json!({ "notes": [{ "id": e }] }))).unwrap();
    assert_eq!(result["deleted"], json!([1]));
    assert_eq!(result["ids"], json!([e]));
    assert_eq!(notes_of(&session, 1).iter().map(|n| n.key).collect::<Vec<_>>(), [62, 65, 72]);
}

#[test]
fn indices_and_ids_can_be_mixed_and_name_each_note_once() {
    let mut session = session();
    let read = listed(&mut session);
    let result = run(
        &mut session,
        "note.setMuted",
        at(json!({ "notes": [0, { "id": read[0] }, { "id": read[3] }], "muted": true })),
    )
    .unwrap();
    assert_eq!(result["notes"], json!([0, 3]), "{result}");
    let muted: Vec<bool> = notes_of(&session, 1).iter().map(|n| n.muted).collect();
    assert_eq!(muted, [true, false, false, true]);
}

#[test]
fn moved_and_changed_notes_keep_their_ids() {
    let mut session = session();
    let before = ids(&session, 1);
    let first = json!([{ "id": before[0] }, { "id": before[1] }]);
    let edits = [
        ("note.move", at(json!({ "notes": first, "beats": 0.5, "semitones": 2 }))),
        ("note.resize", at(json!({ "notes": first, "by": 0.25 }))),
        ("note.resize", at(json!({ "notes": first, "startBy": -0.25 }))),
        ("note.setVelocity", at(json!({ "notes": first, "velocities": [30, 40] }))),
        ("note.setMuted", at(json!({ "notes": first }))),
        ("note.quantize", at(json!({ "grid": "1/4", "ends": true }))),
        ("note.humanize", at(json!({ "seed": 7 }))),
        ("note.fitToScale", at(json!({ "root": "C", "scale": "minor" }))),
        ("note.invert", at(json!({}))),
        ("note.reverse", at(json!({}))),
        ("note.legato", at(json!({}))),
        ("note.stretch", at(json!({ "factor": 2 }))),
        ("note.randomizeVelocity", at(json!({ "seed": 3 }))),
        ("note.rampVelocity", at(json!({ "from": 10, "to": 120 }))),
    ];
    for (id, params) in edits {
        let edited = notes_of(&session, 1);
        run(&mut session, id, params.clone()).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
        assert_ne!(notes_of(&session, 1), edited, "{id} {params} changes the notes");
        assert_eq!(ids(&session, 1), before, "{id} {params} keeps every id");
    }
}

#[test]
fn a_move_reports_the_ids_of_the_moved_notes_and_keeps_them_as_others_are_replaced() {
    let mut session = session();
    let before = ids(&session, 1);
    // D (beat 1, key 62) onto E (beat 2, key 64): E goes, D stays D.
    let moved = run(
        &mut session,
        "note.move",
        at(json!({ "notes": [{ "id": before[1] }], "beats": 1.0, "semitones": 2, "replace": true })),
    )
    .unwrap();
    assert_eq!(moved["ids"], json!([before[1]]));
    assert_eq!(moved["replaced"], json!([2]));
    assert_eq!(ids(&session, 1), [before[0], before[1], before[3]]);
    assert_eq!(key_of(&session, before[1]), 64);
}

#[test]
fn notes_an_edit_makes_are_new_and_no_id_is_twice_in_a_content() {
    let mut session = session();
    let before = ids(&session, 1);
    let mut seen = before.clone();
    let mut check_new = |session: &Session, result: &Value, what: &str| {
        let made: Vec<u64> = result["ids"].as_array().unwrap().iter().map(|id| id.as_u64().unwrap()).collect();
        assert!(!made.is_empty(), "{what}: {result}");
        let at = result.get("added").or_else(|| result.get("notes")).unwrap();
        let now = ids(session, 1);
        let at_ids: Vec<u64> = at.as_array().unwrap().iter().map(|i| now[i.as_u64().unwrap() as usize]).collect();
        assert_eq!(made, at_ids, "{what}: the ids are those of the notes at the indices");
        for id in &made {
            assert!(!seen.contains(id), "{what} made note {id}, an id handed out before");
            seen.push(*id);
        }
        let mut unique = now.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), now.len(), "{what}: no id twice in the content: {now:?}");
    };
    let added =
        run(&mut session, "note.add", at(json!({ "notes": [{ "time": 5.0, "length": 1.0, "key": 70 }] }))).unwrap();
    check_new(&session, &added, "note.add");
    let first = json!([{ "id": before[0] }, { "id": before[1] }]);
    let duplicated = run(&mut session, "note.duplicate", at(json!({ "notes": first }))).unwrap();
    check_new(&session, &duplicated, "note.duplicate");
    let dragged =
        run(&mut session, "note.duplicate", at(json!({ "notes": first, "beats": 4, "semitones": 1 }))).unwrap();
    check_new(&session, &dragged, "a duplicate dragged away");
    run(&mut session, "note.copy", at(json!({ "notes": first }))).unwrap();
    let pasted = run(&mut session, "note.paste", at(json!({ "time": 6.0 }))).unwrap();
    check_new(&session, &pasted, "note.paste");
    let pasted_again = run(&mut session, "note.paste", at(json!({ "time": 6.0 }))).unwrap();
    check_new(&session, &pasted_again, "the same notes pasted again");
    // A split's first half keeps its note's id; the second half is a new note.
    let split = run(&mut session, "note.split", at(json!({ "notes": [{ "id": before[2] }], "at": 2.5 }))).unwrap();
    check_new(&session, &split, "note.split");
    let now = notes_of(&session, 1);
    let kept = now.position(NoteId(before[2])).expect("the first half keeps the id");
    assert_eq!((now[kept].time, now[kept].duration), (2.0, 0.5));
    // The originals are where they were, with their ids.
    assert_eq!(&ids(&session, 1)[..4], &before[..]);
}

#[test]
fn an_id_given_back_by_undo_is_not_handed_out_again() {
    let mut session = session();
    let new = json!({ "notes": [{ "time": 5.0, "length": 1.0, "key": 70 }] });
    let first = run(&mut session, "note.add", at(new.clone())).unwrap()["ids"][0].as_u64().unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(!ids(&session, 1).contains(&first));
    let second = run(&mut session, "note.add", at(new)).unwrap()["ids"][0].as_u64().unwrap();
    assert_ne!(first, second);
    // Undo brings back a deleted note with the id it had.
    let before = ids(&session, 1);
    run(&mut session, "note.delete", at(json!({ "notes": [{ "id": before[1] }] }))).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(ids(&session, 1), before);
}

#[test]
fn an_edit_leaves_another_contents_notes_shared() {
    let mut session = session();
    let other = session.project().content(ContentId(2)).unwrap().lanes.clone();
    run(&mut session, "note.move", at(json!({ "notes": [0], "beats": 1.0 }))).unwrap();
    run(&mut session, "note.add", at(json!({ "notes": [{ "time": 5.0, "length": 1.0, "key": 70 }] }))).unwrap();
    let after = &session.project().content(ContentId(2)).unwrap().lanes;
    assert!(Shared::ptr_eq(&other, after), "content 2's notes are the same list as before");
}

#[test]
fn a_clip_given_its_own_content_keeps_its_note_ids() {
    // Two clips play content 1; editing one gives it its own copy (copy on write), and the copy's
    // notes keep their ids, since a note is always named with its clip.
    let mut session = session();
    let clip = session.project().arrangement.lanes[0].clips[0].id.0;
    run(&mut session, "clip.duplicate", json!({ "clips": [{ "clip": clip }] })).unwrap();
    let shared = session.project().arrangement.lanes[0].clips.iter().filter(|c| c.content == ContentId(1)).count();
    assert_eq!(shared, 2, "the duplicate plays the same content");
    let before = listed(&mut session);
    run(&mut session, "note.setVelocity", at(json!({ "notes": [{ "id": before[0] }], "velocity": 5 }))).unwrap();
    let own = session.project().arrangement.lanes[0].clips[0].content;
    assert_ne!(own, ContentId(1), "the edited clip has its own content now");
    assert_eq!(listed(&mut session), before);
    assert_eq!(ids(&session, 1), before, "the other clip's content keeps them too");
}

#[test]
fn a_note_named_by_an_id_it_does_not_have_is_refused_and_changes_nothing() {
    let mut session = session();
    let before = session.project().clone();
    let other = ids(&session, 2)[0];
    let cases = [
        json!([{ "id": u64::MAX }]),
        json!([{ "id": other }]),
        json!([{ "id": "1" }]),
        json!([{ "id": -1 }]),
        json!([{ "key": 1 }]),
    ];
    for notes in cases {
        let got = run(&mut session, "note.delete", at(json!({ "notes": notes })));
        assert!(got.is_err(), "{notes}: {got:?}");
    }
    let got = run(&mut session, "note.delete", at(json!({ "notes": [{ "id": other }] })));
    assert!(got.as_ref().is_err_and(|e| e.contains(&format!("no note with id {other}"))), "{got:?}");
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
}
