// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Clip scales: `clip.setScale`, `clip.scale` and `project.setDefaultScale`, one undo step each,
//! refusing bad input and changing nothing then; new MIDI clips take the default scale, and
//! `note.fitToScale` fits to the clip's own scale (`docs/spec/ui-clip-editor.md` › Scale and key).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use serde_json::json;
use suhina_model::*;
use suhina_session::Session;

use crate::support::run;

fn note(time: f64, key: u8) -> Note {
    Note { time, duration: 1.0, key, velocity: 100.0 / 127.0, release_velocity: 0.5, channel: 0, muted: false }
}

fn clip(content: u32) -> Clip {
    Clip {
        id: Default::default(),
        content: ContentId(content),
        start: 0.0,
        length: 4.0,
        offset: 0.0,
        loop_range: None,
        enabled: true,
        fade_in: 0.0,
        fade_out: 0.0,
        fade_in_curve: Default::default(),
        fade_out_curve: Default::default(),
        follow_action: Default::default(),
        launch: Default::default(),
        color: None,
        extensions: Extensions::default(),
    }
}

/// Track 1 "Keys": arrangement clips 0 and 1 both play content 1 (a duplicate), clip 2 is audio;
/// scene 1's slot plays content 1 too, scene 2's slot is empty.
fn project() -> Project {
    suhina_test_support::with_ids(project_built())
}

fn project_built() -> Project {
    let content = |id: u32, data: LaneData| ClipContent {
        id: ContentId(id),
        name: format!("C{id}"),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data }],
        extensions: Extensions::default(),
        scale: None,
    };
    let audio = AudioRegion {
        file: "a.wav".into(),
        absolute_file: None,
        sample_rate: None,
        channels: None,
        duration_seconds: None,
        warped: false,
        stretch: StretchMode::default(),
        warp_markers: Vec::new(),
        gain: 1.0,
        transpose: 0.0,
        stream: None,
    };
    Project {
        tracks: vec![Track {
            id: TrackId(1),
            name: "Keys".into(),
            color: None,
            content: ContentKinds { notes: true, audio: true },
            channel: None,
            parent: None,
            midi_input: Default::default(),
            audio_input: Default::default(),
            extensions: Extensions::default(),
        }],
        contents: vec![
            content(1, LaneData::Notes(vec![note(0.0, 60), note(1.0, 61), note(2.0, 66)])),
            content(2, LaneData::Audio(audio)),
        ],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "Keys".into(),
                track: Some(TrackId(1)),
                clips: vec![clip(1), Clip { start: 4.0, ..clip(1) }, Clip { start: 8.0, ..clip(2) }],
            }],
            markers: Vec::new(),
        },
        launcher: Launcher {
            scenes: vec![Scene::default(), Scene::default()],
            columns: vec![LauncherColumn {
                track: TrackId(1),
                slots: vec![Slot { clip: Some(clip(1)), has_stop: true }, Slot { clip: None, has_stop: true }],
            }],
            mappings: Vec::new(),
        },
        ..Project::default()
    }
}

/// The scale of the content arrangement clip `index` plays.
fn scale_at(session: &Session, index: usize) -> Option<Scale> {
    let clip = &session.project().arrangement.lanes[0].clips[index];
    session.project().content(clip.content).and_then(|c| c.scale)
}

fn scale(root: u8, mode: ScaleMode) -> Option<Scale> {
    Scale::new(root, mode)
}

#[test]
fn set_scale_sets_the_root_and_mode_and_undo_takes_it_back() {
    let mut session = Session::new(project());
    let done =
        run(&mut session, "clip.setScale", json!({ "track": "Keys", "index": 0, "root": "F#", "mode": "minor" }));
    assert_eq!(done, Ok(json!({ "scale": { "root": "F#", "mode": "minor" }, "changed": true })));
    assert_eq!(scale_at(&session, 0), scale(6, ScaleMode::Minor));
    // The content was shared: this clip got its own copy, the duplicate and the slot keep theirs.
    assert_ne!(session.project().arrangement.lanes[0].clips[0].content, ContentId(1));
    assert_eq!(scale_at(&session, 1), None);
    assert_eq!(session.project().content(ContentId(1)).and_then(|c| c.scale), None);

    // Only the root, then only the mode: the other part is kept.
    run(&mut session, "clip.setScale", json!({ "track": 1, "index": 0, "root": 2 })).unwrap();
    assert_eq!(scale_at(&session, 0), scale(2, ScaleMode::Minor));
    run(&mut session, "clip.setScale", json!({ "track": 1, "index": 0, "mode": "dorian" })).unwrap();
    assert_eq!(scale_at(&session, 0), scale(2, ScaleMode::Dorian));
    assert_eq!(
        run(&mut session, "clip.scale", json!({ "track": 1, "index": 0 })),
        Ok(json!({ "scale": { "root": "D", "mode": "dorian" }, "default_scale": null }))
    );

    // Each call is one undo step; the first undo of all three brings back the shared content.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(scale_at(&session, 0), scale(2, ScaleMode::Minor));
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(scale_at(&session, 0), None);
    assert_eq!(session.project().arrangement.lanes[0].clips[0].content, ContentId(1));
    assert_eq!(session.project().contents.len(), 2);
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(scale_at(&session, 0), scale(6, ScaleMode::Minor));
}

#[test]
fn off_clears_the_scale_and_a_call_that_changes_nothing_is_no_undo_step() {
    let mut session = Session::new(project());
    run(&mut session, "clip.setScale", json!({ "track": 1, "scene": 1, "root": "C", "mode": "major" })).unwrap();
    let slot = |s: &Session| {
        let clip = s.project().launcher.columns[0].slots[0].clip.as_ref().unwrap();
        s.project().content(clip.content).and_then(|c| c.scale)
    };
    assert_eq!(slot(&session), scale(0, ScaleMode::Major));
    // The same scale again changes nothing and leaves the undo history as it was.
    let steps = |s: &mut Session| run(s, "project.info", json!({})).unwrap()["can_undo"].clone();
    let done = run(&mut session, "clip.setScale", json!({ "track": 1, "scene": 1, "root": 0, "mode": "major" }));
    assert_eq!(done.unwrap()["changed"], json!(false));
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(slot(&session), None, "one undo took back the first call");
    assert_eq!(steps(&mut session), json!(false));

    run(&mut session, "edit.redo", json!({})).unwrap();
    let done = run(&mut session, "clip.setScale", json!({ "track": 1, "scene": 1, "off": true }));
    assert_eq!(done, Ok(json!({ "scale": null, "changed": true })));
    assert_eq!(slot(&session), None);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(slot(&session), scale(0, ScaleMode::Major));
}

#[test]
fn bad_input_is_refused_and_changes_nothing() {
    let mut session = Session::new(project());
    let before = session.project().clone();
    for (params, says) in [
        (json!({ "track": 1, "index": 0 }), "give root"),
        (json!({ "track": 1, "index": 0, "root": "H" }), "unknown root"),
        (json!({ "track": 1, "index": 0, "root": 12 }), "0 to 11"),
        (json!({ "track": 1, "index": 0, "root": -1 }), "0 to 11"),
        (json!({ "track": 1, "index": 0, "root": "" }), "key name"),
        (json!({ "track": 1, "index": 0, "root": true }), "root must be a name or a number"),
        (json!({ "track": 1, "index": 0, "mode": "Major" }), "unknown mode"),
        (json!({ "track": 1, "index": 0, "mode": 3 }), "mode must be a text"),
        (json!({ "track": 1, "index": 0, "off": "yes" }), "off must be true or false"),
        (json!({ "track": 1, "index": 0, "off": true, "root": "C" }), "not both"),
        (json!({ "track": 1, "index": 2, "root": "C" }), "audio clip"),
        (json!({ "track": 1, "index": 9, "root": "C" }), "does not exist"),
        (json!({ "track": 1, "scene": 2, "root": "C" }), "empty"),
        (json!({ "track": 1, "index": 0, "scene": 1, "root": "C" }), "not both"),
        (json!({ "track": 7, "index": 0, "root": "C" }), "track"),
        (json!({ "track": 1, "index": 0, "root": "C", "colour": "red" }), "unknown parameter"),
    ] {
        match run(&mut session, "clip.setScale", params.clone()) {
            Err(e) => assert!(e.contains(says), "{params}: {e}"),
            Ok(v) => panic!("{params} was taken: {v}"),
        }
    }
    for (params, says) in [
        (json!({}), "give root"),
        (json!({ "root": "X#" }), "unknown root"),
        (json!({ "mode": "lydianDominant" }), "unknown mode"),
        (json!({ "off": true, "mode": "minor" }), "not both"),
        (json!({ "off": 1 }), "off must be true or false"),
    ] {
        match run(&mut session, "project.setDefaultScale", params.clone()) {
            Err(e) => assert!(e.contains(says), "{params}: {e}"),
            Ok(v) => panic!("{params} was taken: {v}"),
        }
    }
    assert!(run(&mut session, "clip.scale", json!({ "track": 1, "index": 2 })).is_err());
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
}

#[test]
fn new_midi_clips_take_the_default_scale_and_existing_ones_keep_theirs() {
    let mut session = Session::new(project());
    let done = run(&mut session, "project.setDefaultScale", json!({ "root": "A", "mode": "minor" }));
    assert_eq!(done, Ok(json!({ "default_scale": { "root": "A", "mode": "minor" }, "changed": true })));
    assert_eq!(session.project().default_scale, scale(9, ScaleMode::Minor));
    assert_eq!(
        run(&mut session, "project.info", json!({})).unwrap()["default_scale"],
        json!({ "root": "A", "mode": "minor" })
    );
    assert_eq!(scale_at(&session, 0), None, "a clip that exists keeps no scale");

    // clip.create in an empty slot and in the arrangement: both take A minor.
    run(&mut session, "clip.create", json!({ "track": 1, "scene": 2 })).unwrap();
    let made = session.project().launcher.columns[0].slots[1].clip.as_ref().unwrap().content;
    assert_eq!(session.project().content(made).and_then(|c| c.scale), scale(9, ScaleMode::Minor));
    let at = run(&mut session, "clip.create", json!({ "track": 1, "start": 16 })).unwrap();
    let index = at["index"].as_u64().unwrap() as usize;
    assert_eq!(scale_at(&session, index), scale(9, ScaleMode::Minor));
    let shown = run(&mut session, "clip.scale", json!({ "track": 1, "index": index })).unwrap();
    assert_eq!(shown["scale"], json!({ "root": "A", "mode": "minor" }));

    // Off: the next clip has none; undo brings the default back, one step.
    run(&mut session, "project.setDefaultScale", json!({ "off": true })).unwrap();
    assert_eq!(session.project().default_scale, None);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().default_scale, scale(9, ScaleMode::Minor));
    run(&mut session, "project.setDefaultScale", json!({ "off": true })).unwrap();
    let at = run(&mut session, "clip.create", json!({ "track": 1, "start": 32 })).unwrap();
    assert_eq!(scale_at(&session, at["index"].as_u64().unwrap() as usize), None);
}

#[test]
fn fit_to_scale_fits_to_the_clips_scale_when_none_is_named() {
    let mut session = Session::new(project());
    // D major holds D E F# G A B C#: 60 (C) goes to 59 (B, the lower of the tie), 61 stays, 66
    // stays.
    run(&mut session, "clip.setScale", json!({ "track": 1, "index": 0, "root": "D", "mode": "major" })).unwrap();
    let done = run(&mut session, "note.fitToScale", json!({ "track": 1, "index": 0 })).unwrap();
    assert_eq!(done["moved"], json!(1));
    let content = session.project().arrangement.lanes[0].clips[0].content;
    let keys: Vec<u8> = match &session.project().content(content).unwrap().lanes[0].data {
        LaneData::Notes(notes) => notes.iter().map(|n| n.key).collect(),
        LaneData::Audio(_) => Vec::new(),
    };
    assert_eq!(keys, [59, 61, 66]);
    // A named scale still wins over the clip's: C major moves 61 and 66 down.
    let done = run(&mut session, "note.fitToScale", json!({ "track": 1, "index": 0, "root": "C" })).unwrap();
    assert_eq!(done["moved"], json!(2));
}

#[test]
fn a_scale_is_saved_with_the_project() {
    let mut session = Session::new(project());
    run(&mut session, "clip.setScale", json!({ "track": 1, "index": 0, "root": "Bb", "mode": "blues" })).unwrap();
    run(&mut session, "project.setDefaultScale", json!({ "root": 4, "mode": "phrygian" })).unwrap();
    let read = suhina_project_file::from_json(&suhina_project_file::to_json(session.project())).unwrap();
    assert_eq!(&read, session.project());
    assert_eq!(read.default_scale, scale(4, ScaleMode::Phrygian));
}

#[test]
fn consolidated_midi_clips_keep_the_first_clips_scale() {
    let mut session = Session::new(project());
    run(&mut session, "clip.setScale", json!({ "track": 1, "index": 0, "root": "G", "mode": "mixolydian" })).unwrap();
    run(&mut session, "clip.consolidate", json!({ "track": 1, "start": 0.0, "end": 8.0 })).unwrap();
    assert_eq!(session.project().arrangement.lanes[0].clips[0].length, 8.0);
    assert_eq!(scale_at(&session, 0), scale(7, ScaleMode::Mixolydian));
}
