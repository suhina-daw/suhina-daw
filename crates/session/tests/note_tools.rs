// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The MIDI tools: `note.setMuted`, `note.humanize`, `note.fitToScale`, `note.invert`,
//! `note.reverse`, `note.legato`, `note.stretch`, `note.randomizeVelocity` and
//! `note.rampVelocity` — editing a MIDI clip's notes, one undo step each, and refusing bad input
//! (`docs/spec/ui-clip-editor.md` › Muting, › Other note tools (MIDI tools column)).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use serde_json::{Value, json};
use suhina_model::*;
use suhina_session::{MAX_NOTES_PER_COMMAND, Session, midi_velocity};

use crate::support::run;

fn note(time: f64, duration: f64, key: u8) -> Note {
    Note::new(time, duration, key, 100.0 / 127.0)
}

fn clip(content: u32) -> Clip {
    Clip::new(ContentId(content), 0.0, 8.0)
}

/// Track 1 "Keys" has, on the arrangement, a MIDI clip with `notes` (index 0), an empty MIDI clip
/// (index 1) and an audio clip (index 2).
fn project(notes: Vec<Note>) -> Project {
    suhina_test_support::with_ids(project_built(notes))
}

fn project_built(notes: Vec<Note>) -> Project {
    let content = |id: u32, lanes: Vec<ContentLane>| ClipContent {
        id: ContentId(id),
        name: format!("C{id}"),
        length: 8.0,
        lanes,
        extensions: Extensions::default(),
        scale: None,
    };
    let lane = |n: Vec<Note>| vec![ContentLane { target: None, data: LaneData::Notes(n) }];
    let audio = AudioRegion::new("a.wav");
    Project {
        tracks: vec![Track { content: ContentKinds { notes: true, audio: true }, ..Track::new(TrackId(1), "Keys") }],
        contents: vec![
            content(1, lane(notes)),
            content(2, Vec::new()),
            content(3, vec![ContentLane { target: None, data: LaneData::Audio(audio) }]),
        ],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "Keys".into(),
                track: Some(TrackId(1)),
                clips: vec![clip(1), clip(2), clip(3)],
            }],
            markers: Vec::new(),
        },
        ..Project::default()
    }
}

/// The four notes most tests start from: C4, E4, G4 and a short B4, one beat apart.
fn scale_notes() -> Vec<Note> {
    vec![note(0.0, 1.0, 60), note(1.0, 1.0, 64), note(2.0, 1.0, 67), note(3.0, 0.5, 71)]
}

fn notes_of(session: &Session, content: u32) -> Vec<Note> {
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

fn times(session: &Session) -> Vec<f64> {
    notes_of(session, 1).iter().map(|n| n.time).collect()
}

fn lengths(session: &Session) -> Vec<f64> {
    notes_of(session, 1).iter().map(|n| n.duration).collect()
}

fn keys(session: &Session) -> Vec<u8> {
    notes_of(session, 1).iter().map(|n| n.key).collect()
}

fn velocities(session: &Session) -> Vec<u8> {
    notes_of(session, 1).iter().map(|n| midi_velocity(n.velocity)).collect()
}

fn mutes(session: &Session) -> Vec<bool> {
    notes_of(session, 1).iter().map(|n| n.muted).collect()
}

/// The content each clip on the Keys lane plays, in order.
fn arranged(session: &Session) -> Vec<u32> {
    session.project().arrangement.lanes[0].clips.iter().map(|c| c.content.0).collect()
}

fn close(got: &[f64], want: &[f64]) {
    assert_eq!(got.len(), want.len(), "{got:?} {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() < 1e-9, "{got:?} is not {want:?}");
    }
}

#[test]
fn muting_sets_flips_and_undoes_one_step_at_a_time() {
    let mut session = Session::new(project(scale_notes()));
    let before = notes_of(&session, 1);
    let muted =
        run(&mut session, "note.setMuted", json!({ "track": 1, "index": 0, "notes": [3, 1, 1], "muted": true }))
            .unwrap();
    assert_eq!(muted, json!({ "notes": [1, 3], "muted": [true, true] }));
    assert_eq!(mutes(&session), [false, true, false, true]);
    run(&mut session, "note.setMuted", json!({ "track": 1, "index": 0, "notes": [1], "muted": false })).unwrap();
    assert_eq!(mutes(&session), [false, false, false, true]);

    // With muted left out each named note flips, and the unnamed ones stay as they are.
    let flipped = run(&mut session, "note.setMuted", json!({ "track": 1, "index": 0, "notes": [0, 3] })).unwrap();
    assert_eq!(flipped, json!({ "notes": [0, 3], "muted": [true, false] }));
    assert_eq!(mutes(&session), [true, false, false, false]);

    // Three commands, three undo steps, then nothing left to undo.
    for _ in 0..3 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert_eq!(notes_of(&session, 1), before);
    assert!(!session.can_undo());

    // Muting what is already muted changes nothing and marks no undo step.
    run(&mut session, "note.setMuted", json!({ "track": 1, "index": 0, "notes": [2], "muted": true })).unwrap();
    let once = session.project().clone();
    run(&mut session, "note.setMuted", json!({ "track": 1, "index": 0, "notes": [2], "muted": true })).unwrap();
    assert_eq!(*session.project(), once);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(!session.can_undo(), "the second call marked no undo step");
}

#[test]
fn humanize_moves_starts_and_velocities_within_its_amounts() {
    let mut session = Session::new(project(scale_notes()));
    let before = notes_of(&session, 1);
    let result = run(
        &mut session,
        "note.humanize",
        json!({ "track": 1, "index": 0, "notes": [0, 3], "time": 0.1, "velocity": 20, "seed": 7 }),
    )
    .unwrap();
    assert_eq!(result, json!({ "notes": [0, 3], "seed": 7 }));
    let after = notes_of(&session, 1);
    assert_eq!(after[1], before[1]);
    assert_eq!(after[2], before[2]);
    for index in [0, 3] {
        let moved = (after[index].time - before[index].time).abs();
        assert!(moved <= 0.1 + 1e-12, "note {index} moved {moved}");
        let by = i32::from(midi_velocity(after[index].velocity)) - i32::from(midi_velocity(before[index].velocity));
        assert!(by.abs() <= 20, "note {index} moved {by} MIDI steps");
    }
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(notes_of(&session, 1), before);
    assert!(!session.can_undo());

    // The whole clip, and a fresh seed returned for the next call.
    let result = run(&mut session, "note.humanize", json!({ "track": 1, "index": 0 })).unwrap();
    assert_eq!(result["notes"], json!([0, 1, 2, 3]));
    assert!(result["seed"].as_u64().is_some_and(|seed| seed > 0), "{result}");
}

#[test]
fn humanize_repeats_with_a_seed_and_differs_without_one() {
    let with_seed = |seed: u64| {
        let mut session = Session::new(project(scale_notes()));
        run(&mut session, "note.humanize", json!({ "track": 1, "index": 0, "seed": seed })).unwrap();
        notes_of(&session, 1)
    };
    assert_eq!(with_seed(7), with_seed(7), "the same seed gives the same notes");
    assert_ne!(with_seed(7), with_seed(8), "another seed gives other notes");
    assert_ne!(with_seed(7), scale_notes());
    // The draw follows the notes' order, so naming one note draws the same value for it, and a
    // different number of notes changes the rest of the result.
    let mut session = Session::new(project(scale_notes()));
    run(&mut session, "note.humanize", json!({ "track": 1, "index": 0, "notes": [0], "seed": 7 })).unwrap();
    assert_eq!(notes_of(&session, 1)[0], with_seed(7)[0]);
    assert_ne!(notes_of(&session, 1), with_seed(7));
    assert_ne!(notes_of(&session, 1)[1], with_seed(7)[1], "the notes left out keep their place");
    for note in notes_of(&session, 1) {
        assert!((0.0..=120_000.0).contains(&note.time));
        assert!((1..=127).contains(&midi_velocity(note.velocity)));
    }

    // Nothing to move: no change, no undo step.
    let mut quiet = Session::new(project(scale_notes()));
    let result = run(&mut quiet, "note.humanize", json!({ "track": 1, "index": 0, "time": 0, "velocity": 0 })).unwrap();
    assert_eq!(result["notes"], json!([0, 1, 2, 3]));
    assert_eq!(*quiet.project(), project(scale_notes()));
    assert!(!quiet.can_undo());
}

#[test]
fn fit_to_scale_moves_to_the_nearest_key_and_takes_the_lower_on_a_tie() {
    // C#4, D#4 and F#4 against C major: 61 ties between 60 and 62 and takes 60, 63 moves to 62,
    // 66 ties between 65 and 67 and takes 65.
    let mut session = Session::new(project(vec![note(0.0, 1.0, 61), note(1.0, 1.0, 63), note(2.0, 1.0, 66)]));
    let before = notes_of(&session, 1);
    let result = run(&mut session, "note.fitToScale", json!({ "track": 1, "index": 0, "notes": [0, 2] })).unwrap();
    assert_eq!(result, json!({ "notes": [0, 2], "moved": 2 }));
    assert_eq!(keys(&session), [60, 63, 65]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(notes_of(&session, 1), before);
    assert!(!session.can_undo());

    // The whole clip, over another root and scale: D major pentatonic is D, E, F#, A and B.
    run(&mut session, "note.fitToScale", json!({ "track": 1, "index": 0, "root": "D", "scale": "majorPentatonic" }))
        .unwrap();
    assert_eq!(keys(&session), [62, 62, 66]);
    // A numeric root, and a call that moves nothing.
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "note.fitToScale", json!({ "track": 1, "index": 0, "root": 0, "scale": "major" })).unwrap();
    assert_eq!(keys(&session), [60, 62, 65]);
    // Every note already in the scale: no change, no undo step.
    let mut quiet = Session::new(project(vec![note(0.0, 1.0, 60), note(1.0, 1.0, 64)]));
    let already = run(&mut quiet, "note.fitToScale", json!({ "track": 1, "index": 0 })).unwrap();
    assert_eq!(already, json!({ "notes": [0, 1], "moved": 0 }));
    assert!(!quiet.can_undo());
}

#[test]
fn inverting_mirrors_the_keys_around_the_middle_of_the_range() {
    let mut session = Session::new(project(scale_notes()));
    let before = notes_of(&session, 1);
    // The named notes 0 and 1 span 60 to 64, so the pivot is 62.
    let result = run(&mut session, "note.invert", json!({ "track": 1, "index": 0, "notes": [0, 1] })).unwrap();
    assert_eq!(result, json!({ "notes": [0, 1] }));
    assert_eq!(keys(&session), [64, 60, 67, 71]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(notes_of(&session, 1), before);
    assert!(!session.can_undo());

    // The whole clip spans 60 to 71: the pivot is 65, and 62 (2 * 65 - 68) stays inside the keys.
    run(&mut session, "note.invert", json!({ "track": 1, "index": 0 })).unwrap();
    assert_eq!(keys(&session), [70, 66, 63, 59]);
    // A single note is its own pivot and does not move; no undo step is marked.
    run(&mut session, "edit.undo", json!({})).unwrap();
    let once = session.project().clone();
    run(&mut session, "note.invert", json!({ "track": 1, "index": 0, "notes": [0] })).unwrap();
    assert_eq!(*session.project(), once);
    assert!(!session.can_undo());
}

#[test]
fn inverting_keeps_the_keys_within_0_to_127() {
    let mut session = Session::new(project(vec![note(0.0, 1.0, 0), note(1.0, 1.0, 1)]));
    run(&mut session, "note.invert", json!({ "track": 1, "index": 0 })).unwrap();
    // The pivot is 0: note 1 would land on -1 and is kept at 0.
    assert_eq!(keys(&session), [0, 0]);
}

#[test]
fn reversing_turns_the_notes_round_inside_their_own_span() {
    let mut session = Session::new(project(scale_notes()));
    let before = notes_of(&session, 1);
    let result = run(&mut session, "note.reverse", json!({ "track": 1, "index": 0 })).unwrap();
    assert_eq!(result, json!({ "notes": [0, 1, 2, 3] }));
    // The span is beats 0 to 3.5; each start becomes 3.5 minus its end, and lengths stay.
    close(&times(&session), &[2.5, 1.5, 0.5, 0.0]);
    close(&lengths(&session), &[1.0, 1.0, 1.0, 0.5]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(notes_of(&session, 1), before);

    // Named notes reverse inside their own span (beats 0 to 3.5), leaving the others where they
    // are: note 0 starts at 3.5 - 1 and note 3 at 3.5 - 3.5.
    run(&mut session, "note.reverse", json!({ "track": 1, "index": 0, "notes": [0, 3] })).unwrap();
    close(&times(&session), &[2.5, 1.0, 2.0, 0.0]);
    close(&lengths(&session), &[1.0, 1.0, 1.0, 0.5]);
}

#[test]
fn legato_lasts_until_the_next_note_of_the_same_key() {
    // Keys 60, 60 and 64: the first 60 lasts until the second one starts; both last notes keep
    // their length.
    let mut session = Session::new(project(vec![note(0.0, 1.0, 60), note(1.5, 1.0, 60), note(0.5, 1.0, 64)]));
    let before = notes_of(&session, 1);
    let result = run(&mut session, "note.legato", json!({ "track": 1, "index": 0 })).unwrap();
    assert_eq!(result, json!({ "notes": [0, 1, 2], "changed": 1 }));
    close(&lengths(&session), &[1.5, 1.0, 1.0]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(notes_of(&session, 1), before);

    // Never shorter than a hundredth of a beat: two notes of a key at the same beat.
    let mut together = Session::new(project(vec![note(1.0, 1.0, 60), note(1.0, 2.0, 60)]));
    run(&mut together, "note.legato", json!({ "track": 1, "index": 0 })).unwrap();
    close(&lengths(&together), &[0.01, 2.0]);

    // A named note whose length is already right changes nothing.
    let mut quiet = Session::new(project(scale_notes()));
    let result = run(&mut quiet, "note.legato", json!({ "track": 1, "index": 0, "notes": [3] })).unwrap();
    assert_eq!(result, json!({ "notes": [3], "changed": 0 }));
    assert!(!quiet.can_undo());

    // A note that would have to last past the furthest beat is refused.
    let mut far = Session::new(project(vec![note(0.0, 1.0, 60), note(200_000.0, 1.0, 60)]));
    let before = notes_of(&far, 1);
    let refused = run(&mut far, "note.legato", json!({ "track": 1, "index": 0 }));
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(notes_of(&far, 1), before);
}

#[test]
fn stretching_scales_starts_and_lengths_about_the_earliest_start() {
    let mut session = Session::new(project(scale_notes()));
    let before = notes_of(&session, 1);
    let result = run(&mut session, "note.stretch", json!({ "track": 1, "index": 0, "factor": 2 })).unwrap();
    assert_eq!(result, json!({ "notes": [0, 1, 2, 3] }));
    close(&times(&session), &[0.0, 2.0, 4.0, 6.0]);
    close(&lengths(&session), &[2.0, 2.0, 2.0, 1.0]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(notes_of(&session, 1), before);
    assert!(!session.can_undo());

    // Named notes stretch about their own earliest start; the others stay.
    run(&mut session, "note.stretch", json!({ "track": 1, "index": 0, "notes": [1, 2], "factor": 0.5 })).unwrap();
    close(&times(&session), &[0.0, 1.0, 1.5, 3.0]);
    close(&lengths(&session), &[1.0, 0.5, 0.5, 0.5]);
    // A factor of 1 leaves everything where it is, with no undo step.
    let once = session.project().clone();
    run(&mut session, "note.stretch", json!({ "track": 1, "index": 0, "factor": 1 })).unwrap();
    assert_eq!(*session.project(), once);
}

#[test]
fn randomizing_velocities_repeats_with_a_seed() {
    let with_seed = |seed: u64, notes: Value| {
        let mut session = Session::new(project(scale_notes()));
        let result = run(
            &mut session,
            "note.randomizeVelocity",
            json!({ "track": 1, "index": 0, "notes": notes, "amount": 16, "seed": seed }),
        )
        .unwrap();
        (result, velocities(&session))
    };
    let (result, got) = with_seed(7, json!([0, 1, 2, 3]));
    assert_eq!(result, json!({ "notes": [0, 1, 2, 3], "seed": 7 }));
    assert_eq!(got, with_seed(7, json!([0, 1, 2, 3])).1, "the same seed gives the same velocities");
    assert_ne!(got, with_seed(8, json!([0, 1, 2, 3])).1);
    for (before, after) in [100u8; 4].iter().zip(&got) {
        assert!(i32::from(*after).abs_diff(i32::from(*before)) <= 16, "{got:?} is more than 16 steps from 100");
    }
    // A different set of notes draws differently.
    assert_ne!(got, with_seed(7, json!([0])).1);

    // An amount of 0 changes nothing and marks no undo step.
    let mut quiet = Session::new(project(scale_notes()));
    run(&mut quiet, "note.randomizeVelocity", json!({ "track": 1, "index": 0, "amount": 0 })).unwrap();
    assert_eq!(*quiet.project(), project(scale_notes()));
    assert!(!quiet.can_undo());

    // The velocities stay within 1 to 127 however hard they are pushed.
    let mut loud = Session::new(project(scale_notes()));
    run(&mut loud, "note.randomizeVelocity", json!({ "track": 1, "index": 0, "amount": 127, "seed": 99 })).unwrap();
    assert!(velocities(&loud).iter().all(|velocity| (1..=127).contains(velocity)));
}

#[test]
fn ramping_spreads_the_velocities_evenly_in_time_order() {
    let mut session = Session::new(project(scale_notes()));
    let before = notes_of(&session, 1);
    let result =
        run(&mut session, "note.rampVelocity", json!({ "track": 1, "index": 0, "from": 1, "to": 127 })).unwrap();
    assert_eq!(result, json!({ "notes": [0, 1, 2, 3], "count": 4 }));
    assert_eq!(velocities(&session), [1, 43, 85, 127]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(notes_of(&session, 1), before);
    assert!(!session.can_undo());

    // Named notes ramp in time order, whatever order they are named in.
    run(&mut session, "note.rampVelocity", json!({ "track": 1, "index": 0, "notes": [2, 0], "from": 10, "to": 20 }))
        .unwrap();
    assert_eq!(velocities(&session), [10, 100, 20, 100]);
    // One note takes the last value.
    run(&mut session, "note.rampVelocity", json!({ "track": 1, "index": 0, "notes": [1], "from": 10, "to": 20 }))
        .unwrap();
    assert_eq!(velocities(&session)[1], 20);
    // Already there: no change, no undo step.
    let once = session.project().clone();
    run(&mut session, "note.rampVelocity", json!({ "track": 1, "index": 0, "notes": [1], "from": 20, "to": 20 }))
        .unwrap();
    assert_eq!(*session.project(), once);
}

#[test]
fn a_mute_rides_through_the_other_note_tools() {
    let mut session = Session::new(project(scale_notes()));
    run(&mut session, "note.setMuted", json!({ "track": 1, "index": 0, "notes": [1], "muted": true })).unwrap();
    for (id, params) in [
        ("note.invert", json!({ "track": 1, "index": 0 })),
        ("note.reverse", json!({ "track": 1, "index": 0 })),
        ("note.stretch", json!({ "track": 1, "index": 0, "factor": 2 })),
        ("note.legato", json!({ "track": 1, "index": 0 })),
        ("note.fitToScale", json!({ "track": 1, "index": 0 })),
        ("note.humanize", json!({ "track": 1, "index": 0, "seed": 5 })),
        ("note.randomizeVelocity", json!({ "track": 1, "index": 0, "seed": 5 })),
        ("note.rampVelocity", json!({ "track": 1, "index": 0, "from": 1, "to": 127 })),
    ] {
        run(&mut session, id, params).unwrap();
        assert_eq!(mutes(&session), [false, true, false, false], "after {id}");
    }
    // Copying and pasting keeps the mute too.
    run(&mut session, "note.copy", json!({ "track": 1, "index": 0, "notes": [1] })).unwrap();
    run(&mut session, "note.paste", json!({ "track": 1, "index": 1 })).unwrap();
    assert_eq!(notes_of(&session, 2).iter().map(|n| n.muted).collect::<Vec<_>>(), [true]);
}

#[test]
fn every_tool_marks_one_undo_step() {
    let cases = [
        ("note.setMuted", json!({ "track": 1, "index": 0, "notes": [0], "muted": true })),
        ("note.humanize", json!({ "track": 1, "index": 0, "seed": 1 })),
        ("note.fitToScale", json!({ "track": 1, "index": 0, "scale": "minor" })),
        ("note.invert", json!({ "track": 1, "index": 0 })),
        ("note.reverse", json!({ "track": 1, "index": 0 })),
        ("note.legato", json!({ "track": 1, "index": 0 })),
        ("note.stretch", json!({ "track": 1, "index": 0, "factor": 2 })),
        ("note.randomizeVelocity", json!({ "track": 1, "index": 0, "seed": 2 })),
        ("note.rampVelocity", json!({ "track": 1, "index": 0, "from": 1, "to": 127 })),
    ];
    for (id, params) in cases {
        // A key of its own and a note to move, so every tool has something to change.
        let mut session = Session::new(project(vec![note(0.0, 1.0, 61), note(1.0, 0.5, 63), note(2.0, 1.0, 61)]));
        let before = session.project().clone();
        run(&mut session, id, params).unwrap();
        assert_ne!(*session.project(), before, "{id} changed nothing");
        run(&mut session, "edit.undo", json!({})).unwrap();
        assert_eq!(*session.project(), before, "{id} did not undo");
        assert!(!session.can_undo(), "{id} marked more than one step");
    }
}

#[test]
fn malformed_tool_params_are_refused_and_change_nothing() {
    let mut session = Session::new(project(scale_notes()));
    let before = session.project().clone();
    let cases = [
        ("note.setMuted", json!({ "track": 1, "index": 0, "notes": [0], "muted": "on" }), "muted must be"),
        ("note.setMuted", json!({ "track": 1, "index": 0, "notes": [0], "muted": 1 }), "muted must be"),
        ("note.setMuted", json!({ "track": 1, "index": 0 }), "notes must be"),
        ("note.setMuted", json!({ "track": 1, "index": 0, "notes": [] }), "empty"),
        ("note.setMuted", json!({ "track": 1, "index": 0, "notes": [9] }), "note 9 does not exist"),
        ("note.setMuted", json!({ "track": 1, "index": 0, "notes": [-1] }), "whole number"),
        ("note.setMuted", json!({ "track": 1, "index": 0, "notes": "all" }), "list"),
        ("note.setMuted", json!({ "track": 1, "index": 2, "notes": [0] }), "audio clip"),
        ("note.setMuted", json!({ "track": 1, "index": 9, "notes": [0] }), "clip index 9"),
        ("note.setMuted", json!({ "track": 1, "notes": [0] }), "give index"),
        ("note.setMuted", json!({ "track": 1, "index": 0, "notes": [0], "swap": true }), "unknown parameter"),
        ("note.humanize", json!({ "track": 1, "index": 0, "time": 2 }), "time must be"),
        ("note.humanize", json!({ "track": 1, "index": 0, "time": -0.1 }), "time must be"),
        ("note.humanize", json!({ "track": 1, "index": 0, "velocity": 128 }), "velocity must be"),
        ("note.humanize", json!({ "track": 1, "index": 0, "seed": -1 }), "seed must be"),
        ("note.humanize", json!({ "track": 1, "index": 0, "seed": 1.5 }), "seed must be"),
        ("note.humanize", json!({ "track": 1, "index": 0, "seed": "seven" }), "seed must be"),
        ("note.fitToScale", json!({ "track": 1, "index": 0, "root": "H" }), "root"),
        ("note.fitToScale", json!({ "track": 1, "index": 0, "root": 12 }), "root"),
        ("note.fitToScale", json!({ "track": 1, "index": 0, "root": "C$" }), "root"),
        ("note.fitToScale", json!({ "track": 1, "index": 0, "scale": "majorish" }), "unknown scale"),
        ("note.fitToScale", json!({ "track": 1, "index": 0, "scale": 5 }), "scale must be"),
        ("note.stretch", json!({ "track": 1, "index": 0, "factor": 0 }), "factor must be"),
        ("note.stretch", json!({ "track": 1, "index": 0, "factor": -1 }), "factor must be"),
        ("note.stretch", json!({ "track": 1, "index": 0, "factor": 200 }), "factor must be"),
        ("note.stretch", json!({ "track": 1, "index": 0 }), "factor must be given"),
        ("note.randomizeVelocity", json!({ "track": 1, "index": 0, "amount": 128 }), "amount must be"),
        ("note.randomizeVelocity", json!({ "track": 1, "index": 0, "amount": -1 }), "amount must be"),
        ("note.rampVelocity", json!({ "track": 1, "index": 0, "from": 0, "to": 127 }), "from must be"),
        ("note.rampVelocity", json!({ "track": 1, "index": 0, "from": 1, "to": 128 }), "to must be"),
        ("note.rampVelocity", json!({ "track": 1, "index": 0, "from": 1.5, "to": 127 }), "from must be"),
        ("note.rampVelocity", json!({ "track": 1, "index": 0, "from": 1 }), "to must be"),
        (
            "note.rampVelocity",
            json!({ "track": 1, "index": 0, "from": 1, "to": 127, "amount": 4 }),
            "unknown parameter",
        ),
        ("note.invert", json!({ "track": 1, "index": 0, "notes": [4] }), "note 4 does not exist"),
        ("note.reverse", json!({ "track": 1, "index": 1, "notes": [0] }), "does not exist"),
        ("note.legato", json!({ "track": 2, "index": 0 }), "track 2 does not exist"),
        ("note.invert", json!({ "track": 1, "index": 2 }), "audio clip"),
    ];
    for (id, params, error) in cases {
        let got = run(&mut session, id, params.clone());
        assert!(got.as_ref().is_err_and(|e| e.contains(error)), "{id} {params}: {got:?}");
    }
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
}

#[test]
fn a_tool_refuses_more_than_ten_thousand_notes_and_a_stretch_past_the_model() {
    let mut session = Session::new(project(scale_notes()));
    let too_many: Vec<Value> = (0..=MAX_NOTES_PER_COMMAND).map(|_| json!(0)).collect();
    let refused = run(&mut session, "note.invert", json!({ "track": 1, "index": 0, "notes": too_many }));
    assert!(refused.as_ref().is_err_and(|e| e.contains("at most")), "{refused:?}");

    // A clip with more notes than one command may name: naming every note is refused too.
    let full: Vec<Value> =
        (0..MAX_NOTES_PER_COMMAND).map(|i| json!({ "time": (i % 128) as f64, "length": 1, "key": 60 })).collect();
    run(&mut session, "note.add", json!({ "track": 1, "index": 1, "notes": full })).unwrap();
    run(&mut session, "note.add", json!({ "track": 1, "index": 1, "notes": [{ "time": 0, "length": 1, "key": 60 }] }))
        .unwrap();
    let refused = run(&mut session, "note.invert", json!({ "track": 1, "index": 1 }));
    assert!(refused.as_ref().is_err_and(|e| e.contains("at most")), "{refused:?}");

    // A stretch that would leave the model's range fails whole, leaving the notes as they are.
    let mut far = Session::new(project(vec![note(0.0, 1.0, 60), note(2_000.0, 1.0, 64)]));
    let before = notes_of(&far, 1);
    let refused = run(&mut far, "note.stretch", json!({ "track": 1, "index": 0, "factor": 100 }));
    assert!(refused.is_err(), "{refused:?}");
    assert_eq!(notes_of(&far, 1), before);
    assert!(!far.can_undo());
}

#[test]
fn a_tool_on_a_duplicate_gives_only_that_clip_its_own_content() {
    let mut session = Session::new(project(scale_notes()));
    let original = notes_of(&session, 1);
    let made =
        run(&mut session, "clip.duplicate", json!({ "clips": [{ "track": 1, "index": 0 }], "stack": true })).unwrap();
    let copy = made["clips"][0]["index"].as_u64().unwrap() as usize;
    assert_eq!(arranged(&session)[copy], 1, "a duplicate plays the same content until it is edited");
    let duplicated = session.project().clone();
    run(&mut session, "note.invert", json!({ "track": 1, "index": copy })).unwrap();
    let own = arranged(&session)[copy];
    assert_ne!(own, 1, "the edited duplicate got its own content");
    assert_eq!(notes_of(&session, 1), original, "the original is untouched");
    assert_ne!(notes_of(&session, own), original);
    // One undo takes back the edit and the copy it made, leaving the duplicate itself.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(*session.project(), duplicated);
    assert_eq!(arranged(&session)[copy], 1);
}

#[test]
fn muting_a_duplicate_gives_only_that_clip_its_own_content() {
    let mut session = Session::new(project(scale_notes()));
    let made =
        run(&mut session, "clip.duplicate", json!({ "clips": [{ "track": 1, "index": 0 }], "stack": true })).unwrap();
    let copy = made["clips"][0]["index"].as_u64().unwrap() as usize;
    run(&mut session, "note.setMuted", json!({ "track": 1, "index": copy, "notes": [0], "muted": true })).unwrap();
    let own = arranged(&session)[copy];
    assert_ne!(own, 1);
    assert_eq!(mutes(&session), [false, false, false, false], "the original stays unmuted");
    assert_eq!(notes_of(&session, own).iter().map(|n| n.muted).collect::<Vec<_>>(), [true, false, false, false]);
}
