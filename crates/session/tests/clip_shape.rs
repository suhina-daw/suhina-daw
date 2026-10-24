// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `clip.setLoop`, `clip.setLength` and `clip.setStart`: a clip's loop, length and start marker,
//! for an arrangement clip or a slot's clip, one undo step each, refusing bad input
//! (`docs/spec/ui-clip-editor.md` › Commands).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use serde_json::{Value, json};
use suhina_model::*;
use suhina_session::Session;

use crate::support::run;

fn clip(content: u32, start: f64) -> Clip {
    Clip {
        id: Default::default(),
        content: ContentId(content),
        start,
        length: 4.0,
        offset: 0.0,
        loop_range: None,
        enabled: true,
        fade_in: 0.0,
        fade_out: 0.0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        follow_action: Default::default(),
        color: None,
        extensions: Extensions::default(),
        launch: Default::default(),
    }
}

/// Track 1 "Keys": arrangement clips at beat 0 (index 0) and beat 8 (index 1), both 4 beats long
/// and not looping; a clip in the slot of scene 1, and an empty slot in scene 2.
fn project() -> Project {
    suhina_test_support::with_ids(project_built())
}

fn project_built() -> Project {
    let track = Track {
        id: TrackId(1),
        name: "Keys".into(),
        color: None,
        content: ContentKinds { notes: true, audio: true },
        channel: None,
        parent: None,
        extensions: Extensions::default(),
        audio_input: Default::default(),
        midi_input: Default::default(),
    };
    let note =
        Note { time: 0.0, duration: 1.0, key: 60, velocity: 0.8, release_velocity: 0.5, channel: 0, muted: false };
    let content = |id: u32| ClipContent {
        id: ContentId(id),
        name: format!("C{id}"),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![note]) }],
        extensions: Extensions::default(),
        scale: None,
    };
    Project {
        tracks: vec![track],
        contents: vec![content(1), content(2), content(3)],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "Keys".into(),
                track: Some(TrackId(1)),
                clips: vec![clip(1, 0.0), clip(2, 8.0)],
            }],
            markers: Vec::new(),
        },
        launcher: Launcher {
            scenes: vec![Scene::default(), Scene::default()],
            columns: vec![LauncherColumn {
                track: TrackId(1),
                slots: vec![Slot { clip: Some(clip(3, 0.0)), has_stop: true }, Slot::default()],
            }],
            mappings: Default::default(),
        },
        ..Project::default()
    }
}

fn arranged(session: &Session, index: usize) -> Clip {
    session.project().arrangement.lanes[0].clips[index].clone()
}

fn slot(session: &Session) -> Clip {
    session.project().launcher.columns[0].slots[0].clip.clone().unwrap()
}

fn undo(session: &mut Session) {
    run(session, "edit.undo", json!({})).unwrap();
}

#[test]
fn the_loop_of_a_slot_clip_switches_as_before() {
    let mut session = Session::new(project());
    let on = run(&mut session, "clip.setLoop", json!({ "track": 1, "scene": 1, "loop": true })).unwrap();
    assert_eq!(on["scene"], 1);
    assert_eq!(on["loop"], true);
    assert_eq!(slot(&session).loop_range, Some(LoopRange { start: 0.0, end: 4.0 }));
    run(&mut session, "clip.setLoop", json!({ "track": "Keys", "scene": 1, "loop": false })).unwrap();
    assert_eq!(slot(&session).loop_range, None);
    undo(&mut session);
    assert_eq!(slot(&session).loop_range, Some(LoopRange { start: 0.0, end: 4.0 }));
}

#[test]
fn an_arrangement_clips_loop_is_set_by_start_and_end_in_one_undo_step() {
    let mut session = Session::new(project());
    let before = arranged(&session, 0);
    let set = run(&mut session, "clip.setLoop", json!({ "track": 1, "index": 0, "start": 1.0, "end": 3.0 })).unwrap();
    let id = arranged(&session, 0).id.0;
    assert_eq!(set, json!({ "clip": id, "index": 0, "loop": true, "start": 1.0, "end": 3.0 }));
    assert_eq!(arranged(&session, 0).loop_range, Some(LoopRange { start: 1.0, end: 3.0 }));
    // Only one end, while it loops: the other stays.
    run(&mut session, "clip.setLoop", json!({ "track": 1, "index": 0, "end": 6.0 })).unwrap();
    assert_eq!(arranged(&session, 0).loop_range, Some(LoopRange { start: 1.0, end: 6.0 }));
    // Off: the clip plays once; each step undoes on its own.
    run(&mut session, "clip.setLoop", json!({ "track": 1, "index": 0, "loop": false })).unwrap();
    assert_eq!(arranged(&session, 0).loop_range, None);
    undo(&mut session);
    undo(&mut session);
    assert_eq!(arranged(&session, 0).loop_range, Some(LoopRange { start: 1.0, end: 3.0 }));
    undo(&mut session);
    assert_eq!(arranged(&session, 0), before);
    // A slot's clip takes start and end too.
    run(&mut session, "clip.setLoop", json!({ "track": 1, "scene": 1, "start": 0.5, "end": 2.0 })).unwrap();
    assert_eq!(slot(&session).loop_range, Some(LoopRange { start: 0.5, end: 2.0 }));
}

#[test]
fn set_loop_refuses_bad_input_and_changes_nothing() {
    let mut session = Session::new(project());
    let before = session.project().clone();
    for params in [
        json!({ "track": 1, "index": 0 }),
        json!({ "track": 1, "index": 0, "loop": "yes" }),
        json!({ "track": 1, "index": 0, "start": 2.0, "end": 2.0 }),
        json!({ "track": 1, "index": 0, "start": 3.0, "end": 1.0 }),
        json!({ "track": 1, "index": 0, "start": -1.0, "end": 1.0 }),
        json!({ "track": 1, "index": 0, "start": 0.0, "end": 1e9 }),
        json!({ "track": 1, "index": 0, "start": "0", "end": 1.0 }),
        json!({ "track": 1, "index": 0, "start": 0.0, "end": 2.0, "loop": false }),
        // Not looping yet: both ends are needed.
        json!({ "track": 1, "index": 0, "end": 2.0 }),
        json!({ "track": 1, "index": 9, "loop": true }),
        json!({ "track": 1, "index": -1, "loop": true }),
        json!({ "track": 1, "index": 0, "scene": 1, "loop": true }),
        json!({ "track": 1, "scene": 2, "loop": true }),
        json!({ "track": 1, "scene": 99, "loop": true }),
        json!({ "track": 7, "index": 0, "loop": true }),
        json!({ "track": 1, "loop": true }),
    ] {
        assert!(run(&mut session, "clip.setLoop", params.clone()).is_err(), "{params}");
        assert_eq!(session.project(), &before, "{params}");
    }
    // NaN and infinity cannot be written in JSON; serde turns them into null, which is "left out".
    assert!(run(&mut session, "clip.setLoop", json!({ "track": 1, "index": 0, "start": f64::NAN })).is_err());
    assert_eq!(session.project(), &before);
}

#[test]
fn the_length_changes_in_one_undo_step_and_never_runs_into_the_next_clip() {
    let mut session = Session::new(project());
    // By the clip's id; the result names it by id and by where it is.
    let id = arranged(&session, 0).id.0;
    let set = run(&mut session, "clip.setLength", json!({ "clip": id, "length": 6.0 })).unwrap();
    assert_eq!(set, json!({ "clip": id, "index": 0, "length": 6.0 }));
    assert_eq!(arranged(&session, 0).length, 6.0);
    assert_eq!(arranged(&session, 0).start, 0.0);
    // Up to the next clip, at beat 8, and no further.
    run(&mut session, "clip.setLength", json!({ "track": 1, "index": 0, "length": 8.0 })).unwrap();
    let before = session.project().clone();
    assert!(run(&mut session, "clip.setLength", json!({ "track": 1, "index": 0, "length": 8.5 })).is_err());
    assert_eq!(session.project(), &before);
    // The last clip on the lane, and a slot's clip, have no neighbour after them.
    run(&mut session, "clip.setLength", json!({ "track": 1, "index": 1, "length": 100.0 })).unwrap();
    run(&mut session, "clip.setLength", json!({ "track": 1, "scene": 1, "length": 0.5 })).unwrap();
    assert_eq!(slot(&session).length, 0.5);
    undo(&mut session);
    assert_eq!(slot(&session).length, 4.0);
    undo(&mut session);
    undo(&mut session);
    undo(&mut session);
    assert_eq!(session.project(), &project());
}

#[test]
fn a_shorter_clip_shortens_its_fades() {
    let mut p = project();
    p.arrangement.lanes[0].clips[0].fade_in = 2.0;
    p.arrangement.lanes[0].clips[0].fade_out = 2.0;
    let mut session = Session::new(p);
    run(&mut session, "clip.setLength", json!({ "track": 1, "index": 0, "length": 2.0 })).unwrap();
    let clip = arranged(&session, 0);
    assert!(clip.fades_valid());
    assert!((clip.fade_in - 1.0).abs() < 1e-9 && (clip.fade_out - 1.0).abs() < 1e-9);
}

#[test]
fn set_length_refuses_bad_input_and_changes_nothing() {
    let mut session = Session::new(project());
    let before = session.project().clone();
    for params in [
        json!({ "track": 1, "index": 0 }),
        json!({ "track": 1, "index": 0, "length": 0.0 }),
        json!({ "track": 1, "index": 0, "length": -4.0 }),
        json!({ "track": 1, "index": 0, "length": 120_001.0 }),
        json!({ "track": 1, "index": 0, "length": 1e300 }),
        json!({ "track": 1, "index": 0, "length": "4" }),
        json!({ "track": 1, "index": 0, "length": null }),
        json!({ "track": 1, "index": 5, "length": 2.0 }),
        json!({ "track": 1, "scene": 2, "length": 2.0 }),
        json!({ "track": 1, "length": 2.0 }),
        json!({ "track": "Nobody", "index": 0, "length": 2.0 }),
    ] {
        assert!(run(&mut session, "clip.setLength", params.clone()).is_err(), "{params}");
        assert_eq!(session.project(), &before, "{params}");
    }
}

#[test]
fn the_start_marker_keeps_the_end_of_a_clip_that_does_not_loop() {
    let mut session = Session::new(project());
    let set = run(&mut session, "clip.setStart", json!({ "track": 1, "index": 0, "offset": 1.0 })).unwrap();
    let id = arranged(&session, 0).id.0;
    assert_eq!(set, json!({ "clip": id, "index": 0, "offset": 1.0, "length": 3.0 }));
    let clip = arranged(&session, 0);
    assert_eq!((clip.start, clip.offset, clip.length), (0.0, 1.0, 3.0));
    undo(&mut session);
    assert_eq!(arranged(&session, 0), project().arrangement.lanes[0].clips[0]);
    // Earlier than 0 it cannot go; at or past the end neither.
    let before = session.project().clone();
    for offset in [-0.5, 4.0, 5.0, 120_001.0] {
        assert!(run(&mut session, "clip.setStart", json!({ "track": 1, "index": 0, "offset": offset })).is_err());
        assert_eq!(session.project(), &before, "{offset}");
    }
}

#[test]
fn the_start_marker_of_a_looping_clip_keeps_its_length_and_stays_before_the_loop_end() {
    let mut session = Session::new(project());
    run(&mut session, "clip.setLoop", json!({ "track": 1, "index": 0, "start": 0.0, "end": 2.0 })).unwrap();
    run(&mut session, "clip.setStart", json!({ "track": 1, "index": 0, "offset": 1.5 })).unwrap();
    let clip = arranged(&session, 0);
    assert_eq!((clip.offset, clip.length), (1.5, 4.0));
    let before = session.project().clone();
    assert!(run(&mut session, "clip.setStart", json!({ "track": 1, "index": 0, "offset": 2.0 })).is_err());
    assert_eq!(session.project(), &before);
}

#[test]
fn an_earlier_start_marker_may_not_make_the_clip_run_into_the_next() {
    let mut p = project();
    p.arrangement.lanes[0].clips[0].offset = 2.0;
    p.arrangement.lanes[0].clips[0].length = 7.0;
    let mut session = Session::new(p);
    let before = session.project().clone();
    // From 2 to 0 makes it 9 beats long: past the clip at beat 8.
    assert!(run(&mut session, "clip.setStart", json!({ "track": 1, "index": 0, "offset": 0.0 })).is_err());
    assert_eq!(session.project(), &before);
    run(&mut session, "clip.setStart", json!({ "track": 1, "index": 0, "offset": 1.0 })).unwrap();
    assert_eq!(arranged(&session, 0).length, 8.0);
    for params in [
        json!({ "track": 1, "index": 0 }),
        json!({ "track": 1, "index": 0, "offset": "1" }),
        json!({ "track": 1, "scene": 2, "offset": 0.0 }),
    ] {
        assert!(run(&mut session, "clip.setStart", params.clone()).is_err(), "{params}");
    }
}

#[test]
fn the_commands_are_listed() {
    for id in ["clip.setLoop", "clip.setLength", "clip.setStart"] {
        assert!(suhina_session::commands().iter().any(|c| c.id == id), "{id}");
    }
}

#[test]
fn several_clips_fades_move_by_as_much_each_from_its_own_in_one_step() {
    let mut project = project();
    // Only audio clips have fades.
    for content in &mut project.contents[..2] {
        content.lanes = vec![ContentLane { target: None, data: LaneData::Audio(AudioRegion::new("a.wav")) }];
    }
    project.arrangement.lanes[0].clips[1].fade_in = 1.0;
    project.arrangement.lanes[0].clips[1].fade_out = 2.5;
    let mut session = Session::new(project);
    let ids: Vec<Value> =
        session.project().arrangement.lanes[0].clips.iter().map(|c| json!({ "clip": c.id.0 })).collect();
    run(&mut session, "clip.shiftFades", json!({ "clips": ids, "edge": "in", "beats": 0.5 })).unwrap();
    assert_eq!(arranged(&session, 0).fade_in, 0.5, "from none");
    assert_eq!(arranged(&session, 1).fade_in, 1.5, "from its own, not reset");
    // Kept to what the other fade leaves (4 - 2.5).
    run(&mut session, "clip.shiftFades", json!({ "clips": ids, "edge": "in", "beats": 3.0 })).unwrap();
    assert_eq!(arranged(&session, 1).fade_in, 1.5);
    assert_eq!(arranged(&session, 0).fade_in, 3.5);
    undo(&mut session);
    undo(&mut session);
    assert_eq!((arranged(&session, 0).fade_in, arranged(&session, 1).fade_in), (0.0, 1.0), "one step each");
    assert!(run(&mut session, "clip.shiftFades", json!({ "clips": ids, "edge": "up", "beats": 1.0 })).is_err());
}

#[test]
fn shifting_fades_leaves_the_midi_clips_of_a_mixed_selection_alone() {
    let mut project = project();
    project.contents[0].lanes = vec![ContentLane { target: None, data: LaneData::Audio(AudioRegion::new("a.wav")) }];
    let mut session = Session::new(project);
    let ids: Vec<Value> =
        session.project().arrangement.lanes[0].clips.iter().map(|c| json!({ "clip": c.id.0 })).collect();
    let answer = run(&mut session, "clip.shiftFades", json!({ "clips": ids, "edge": "out", "beats": 1.0 })).unwrap();
    assert_eq!(answer["clips"], 1, "only the audio clip is counted");
    assert_eq!((arranged(&session, 0).fade_out, arranged(&session, 1).fade_out), (1.0, 0.0));
}
