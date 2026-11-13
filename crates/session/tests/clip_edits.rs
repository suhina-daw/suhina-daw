// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's clip edits: `note.join`, `note.fitToTime`, `clip.crop`, `clip.setOrigin` and
//! `clip.split` with several cuts, each one undo step, on an arrangement clip and a slot's clip,
//! and refusing what they cannot do (`docs/spec/ui-clip-editor.md` › Joining, fitting, cropping and
//! splitting).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use serde_json::json;
use suhina_model::*;
use suhina_session::Session;

use crate::support::run;

fn note(time: f64, duration: f64, key: u8) -> Note {
    Note::new(time, duration, key, 100.0 / 127.0)
}

/// Track 1 "Keys": on the arrangement a MIDI clip of `notes` at beat 0, 8 beats long (index 0),
/// an audio clip at beat 16 (index 1); in the launcher's first slot a clip of the same notes.
fn project(notes: Vec<Note>) -> Project {
    let content = |id: u32, lanes: Vec<ContentLane>| ClipContent {
        id: ContentId(id),
        name: format!("C{id}"),
        length: 8.0,
        lanes: lanes.into(),
        extensions: Extensions::default(),
        scale: None,
    };
    let lane = |n: Vec<Note>| vec![ContentLane { target: None, data: LaneData::Notes(n.into()) }];
    let audio = vec![ContentLane { target: None, data: LaneData::Audio(AudioRegion::new("a.wav")) }];
    suhina_test_support::with_ids(Project {
        tracks: vec![Track { content: ContentKinds { notes: true, audio: true }, ..Track::new(TrackId(1), "Keys") }],
        contents: vec![content(1, lane(notes.clone())), content(2, audio), content(3, lane(notes))],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "Keys".into(),
                track: Some(TrackId(1)),
                clips: vec![Clip::new(ContentId(1), 0.0, 8.0), Clip::new(ContentId(2), 16.0, 8.0)],
            }],
            markers: Default::default(),
            takes: Vec::new(),
        },
        launcher: Launcher {
            scenes: vec![Scene::default()],
            columns: vec![LauncherColumn {
                track: TrackId(1),
                slots: vec![Slot { clip: Some(Clip::new(ContentId(3), 0.0, 8.0)), has_stop: true }],
            }],
        },
        ..Project::default()
    })
}

fn lane_notes(session: &Session, content: u32) -> Vec<Note> {
    let content = session.project().content(ContentId(content)).expect("the content");
    content
        .lanes
        .iter()
        .find_map(|l| match &l.data {
            LaneData::Notes(n) => Some(n.to_vec()),
            LaneData::Audio(_) => None,
        })
        .unwrap_or_default()
}

fn notes(session: &Session, content: u32) -> Vec<(f64, f64, u8)> {
    lane_notes(session, content).iter().map(|n| (n.time, n.duration, n.key)).collect()
}

fn arranged(session: &Session) -> Clip {
    session.project().arrangement.lanes[0].clips[0].clone()
}

fn undo(session: &mut Session) {
    run(session, "edit.undo", json!({})).unwrap();
}

#[test]
fn joining_merges_touching_and_overlapping_notes_per_key() {
    // Key 60: three notes touching, overlapping, then one apart; key 64: one note alone.
    let mut first = note(0.0, 1.0, 60);
    first.velocity = 50.0 / 127.0;
    let mut session = Session::new(project(vec![
        first,
        note(1.0, 1.0, 60),
        note(1.5, 1.0, 60),
        note(4.0, 1.0, 60),
        note(0.0, 1.0, 64),
    ]));
    let before = notes(&session, 1);
    let result = run(&mut session, "note.join", json!({ "track": 1, "index": 0, "notes": [0, 1, 2, 3, 4] })).unwrap();
    assert_eq!(result["removed"], json!([1, 2]));
    assert_eq!(notes(&session, 1), vec![(0.0, 2.5, 60), (4.0, 1.0, 60), (0.0, 1.0, 64)]);
    assert_eq!(suhina_session::midi_velocity(lane_notes(&session, 1)[0].velocity), 50, "the first note's velocity");
    undo(&mut session);
    assert_eq!(notes(&session, 1), before, "one undo step");
    // Nothing that touches: refused. An empty selection too.
    assert!(run(&mut session, "note.join", json!({ "track": 1, "index": 0, "notes": [3, 4] })).is_err());
    assert!(run(&mut session, "note.join", json!({ "track": 1, "index": 0, "notes": [] })).is_err());
}

#[test]
fn fitting_fills_the_time_range_exactly() {
    let mut session = Session::new(project(vec![note(1.0, 1.0, 60), note(2.0, 2.0, 62), note(7.0, 1.0, 64)]));
    let fit = json!({ "track": 1, "index": 0, "notes": [0, 1], "start": 4.0, "end": 5.5 });
    run(&mut session, "note.fitToTime", fit).unwrap();
    assert_eq!(notes(&session, 1), vec![(4.0, 0.5, 60), (4.5, 1.0, 62), (7.0, 1.0, 64)]);
    undo(&mut session);
    let bad = json!({ "track": 1, "index": 0, "notes": [0], "start": 2.0, "end": 2.0 });
    assert!(run(&mut session, "note.fitToTime", bad).is_err(), "no range, no fit");
}

#[test]
fn cropping_keeps_the_stretch_and_cuts_notes_at_its_edges() {
    let mut session =
        Session::new(project(vec![note(0.0, 1.0, 60), note(1.5, 1.0, 62), note(3.0, 1.0, 64), note(5.5, 1.0, 65)]));
    run(&mut session, "clip.crop", json!({ "track": 1, "index": 0, "start": 2.0, "end": 6.0 })).unwrap();
    assert_eq!(notes(&session, 1), vec![(2.0, 0.5, 62), (3.0, 1.0, 64), (5.5, 0.5, 65)]);
    let clip = arranged(&session);
    assert_eq!((clip.start, clip.offset, clip.length), (2.0, 2.0, 4.0), "what is left plays where it played");
    undo(&mut session);
    assert_eq!(notes(&session, 1).len(), 4);
    assert_eq!(arranged(&session).length, 8.0);
}

#[test]
fn cropping_keeps_a_short_note_wholly_inside_and_drops_a_sliver_an_edge_cuts() {
    let mut session = Session::new(project(vec![note(3.0, 0.005, 60), note(1.0, 1.005, 62)]));
    run(&mut session, "clip.crop", json!({ "track": 1, "index": 0, "start": 2.0, "end": 6.0 })).unwrap();
    assert_eq!(notes(&session, 1), vec![(3.0, 0.005, 60)]);
}

#[test]
fn cropping_a_looping_clip_cuts_its_loop_and_keeps_its_end() {
    let mut session = Session::new(project(vec![note(0.0, 1.0, 60), note(3.0, 1.0, 64)]));
    run(&mut session, "clip.setLoop", json!({ "track": 1, "index": 0, "start": 0.0, "end": 4.0 })).unwrap();
    run(&mut session, "clip.crop", json!({ "track": 1, "index": 0, "start": 1.0, "end": 3.5 })).unwrap();
    let clip = arranged(&session);
    assert_eq!(clip.loop_range, Some(LoopRange { start: 1.0, end: 3.5 }));
    assert_eq!((clip.start, clip.start + clip.length), (1.0, 8.0));
    assert_eq!(notes(&session, 1), vec![(3.0, 0.5, 64)]);
}

#[test]
fn cropping_a_clip_with_an_offset_and_a_selection_outside_it() {
    let mut session = Session::new(project(vec![note(0.0, 1.0, 60), note(2.5, 1.0, 64)]));
    run(&mut session, "clip.setStart", json!({ "track": 1, "index": 0, "offset": 2.0 })).unwrap();
    // The clip plays content 2 to 8 from beat 0: a time selection before that is outside it.
    let outside = run(&mut session, "clip.crop", json!({ "track": 1, "index": 0, "start": 0.0, "end": 1.5 }));
    assert!(outside.is_err());
    run(&mut session, "clip.crop", json!({ "track": 1, "index": 0, "start": 2.5, "end": 4.0 })).unwrap();
    let clip = arranged(&session);
    assert_eq!((clip.start, clip.offset, clip.length), (0.5, 2.5, 1.5));
}

#[test]
fn cropping_a_slot_clip_and_refusing_audio() {
    let mut session = Session::new(project(vec![note(0.0, 1.0, 60), note(4.0, 1.0, 64)]));
    run(&mut session, "clip.crop", json!({ "track": 1, "scene": 0, "start": 0.0, "end": 2.0 })).unwrap();
    assert_eq!(notes(&session, 3), vec![(0.0, 1.0, 60)]);
    assert_eq!(notes(&session, 1).len(), 2, "the arrangement clip's own content is left alone");
    let clip = session.project().launcher.columns[0].slots[0].clip.clone().unwrap();
    assert_eq!(clip.length, 2.0);
    assert!(run(&mut session, "clip.crop", json!({ "track": 1, "index": 1, "start": 0.0, "end": 2.0 })).is_err());
    assert!(run(&mut session, "note.join", json!({ "track": 1, "index": 1, "notes": [0] })).is_err());
}

#[test]
fn setting_the_first_beat_moves_the_content_and_keeps_the_sound() {
    let mut session = Session::new(project(vec![note(2.0, 1.0, 60), note(3.0, 1.0, 64)]));
    run(&mut session, "clip.setStart", json!({ "track": 1, "index": 0, "offset": 2.0 })).unwrap();
    run(&mut session, "clip.setLoop", json!({ "track": 1, "index": 0, "start": 2.0, "end": 6.0 })).unwrap();
    run(&mut session, "clip.setOrigin", json!({ "track": 1, "index": 0, "beat": 2.0 })).unwrap();
    assert_eq!(notes(&session, 1), vec![(0.0, 1.0, 60), (1.0, 1.0, 64)]);
    let clip = arranged(&session);
    assert_eq!((clip.start, clip.offset), (0.0, 0.0));
    assert_eq!(clip.loop_range, Some(LoopRange { start: 0.0, end: 4.0 }));
    undo(&mut session);
    assert_eq!(arranged(&session).offset, 2.0, "one undo step");
    // A note before the beat would have nowhere to go.
    assert!(run(&mut session, "clip.setOrigin", json!({ "track": 1, "index": 0, "beat": 2.5 })).is_err());
}

#[test]
fn splitting_at_two_cuts_is_one_undo_step() {
    let mut session = Session::new(project(vec![note(0.0, 1.0, 60)]));
    let result = run(&mut session, "clip.split", json!({ "track": 1, "index": 0, "ats": [6.0, 2.0] })).unwrap();
    assert_eq!(result["clips"].as_array().map(Vec::len), Some(3));
    let parts: Vec<(f64, f64)> =
        session.project().arrangement.lanes[0].clips.iter().map(|c| (c.start, c.length)).collect();
    assert_eq!(parts, vec![(0.0, 2.0), (2.0, 4.0), (6.0, 2.0), (16.0, 8.0)]);
    undo(&mut session);
    assert_eq!(session.project().arrangement.lanes[0].clips.len(), 2);
    // A cut outside the clip makes none.
    assert!(run(&mut session, "clip.split", json!({ "track": 1, "index": 0, "ats": [2.0, 9.0] })).is_err());
    assert_eq!(session.project().arrangement.lanes[0].clips.len(), 2);
}
