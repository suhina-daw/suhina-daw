// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Clips named by id (ADR 0025): a command finds its clip wherever earlier edits moved it, new
//! clips get new ids, moved and edited clips keep theirs, and the positional `index` still works
//! during its deprecation window.

use serde_json::{Value, json};
use suhina_model::*;
use suhina_session::Session;

use crate::support::run;

fn content(id: u32, name: &str, key: u8) -> ClipContent {
    ClipContent {
        id: ContentId(id),
        name: name.into(),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![Note::new(0.0, 1.0, key, 1.0)]) }],
        extensions: Extensions::default(),
        scale: None,
    }
}

/// Bass with three arrangement clips (Riff at 0, Riff at 8, Hook at 16) and Hook in its slot of
/// scene 1; Lead with nothing. No object has an id: the session gives them.
fn project() -> Project {
    let track = |id: u32, name: &str| Track {
        content: ContentKinds { notes: true, audio: false },
        channel: Some(ChannelId(id)),
        ..Track::new(TrackId(id), name)
    };
    let clip = |content: u32, start: f64| Clip::new(ContentId(content), start, 4.0);
    let column = |track: u32, clip: Option<Clip>| LauncherColumn {
        track: TrackId(track),
        slots: vec![Slot { clip, has_stop: true }, Slot { clip: None, has_stop: true }],
    };
    Project {
        tracks: vec![track(1, "Bass"), track(2, "Lead")],
        channels: vec![Channel::new(ChannelId(1), "C1"), Channel::new(ChannelId(2), "C2")],
        contents: vec![content(1, "Riff", 36), content(2, "Hook", 48)],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                name: "Bass".into(),
                track: Some(TrackId(1)),
                clips: vec![clip(1, 0.0), clip(1, 8.0), clip(2, 16.0)],
                ..ArrangementLane::default()
            }],
            markers: Vec::new(),
        },
        launcher: Launcher {
            scenes: vec![Scene::default(), Scene::default()],
            columns: vec![column(1, Some(clip(2, 0.0))), column(2, None)],
            mappings: Vec::new(),
        },
        ..Project::default()
    }
}

/// The ids of Bass's arrangement clips, in lane order.
fn bass(session: &Session) -> Vec<ClipId> {
    session.project().arrangement.lanes[0].clips.iter().map(|c| c.id).collect()
}

#[allow(clippy::unwrap_used)] // a test helper outside #[test] functions
fn clip(session: &Session, id: ClipId) -> Clip {
    session.project().clip(id).cloned().unwrap()
}

#[test]
fn a_session_gives_every_lane_clip_and_scene_an_id_and_reports_them() {
    let mut session = Session::new(project());
    let p = session.project();
    assert_eq!(p.validate(), Ok(()));
    assert!(p.arrangement.lanes.iter().all(|l| l.id.is_some()));
    assert!(p.all_clips().all(|c| c.id.is_some()));
    assert!(p.launcher.scenes.iter().all(|s| s.id.is_some()));
    let ids = bass(&session);

    let clips = run(&mut session, "project.clips", json!({})).unwrap();
    let listed: Vec<u64> = clips.as_array().unwrap().iter().map(|c| c["id"].as_u64().unwrap()).collect();
    assert_eq!(listed, ids.iter().map(|id| u64::from(id.0)).collect::<Vec<_>>());
    assert_eq!(clips[1]["index"], 1, "the index is still reported while it is accepted");

    let grid = run(&mut session, "project.launcher", json!({})).unwrap();
    let slot = session.project().launcher.columns[0].slots[0].clip.as_ref().unwrap().id;
    assert_eq!(grid["tracks"][0]["slots"][0]["id"], slot.0);
    assert_eq!(grid["scenes"][1]["id"], session.project().launcher.scenes[1].id.0);
}

/// The case a position gets wrong: once an earlier clip moved past it, "index 1" is another
/// clip, and the id is still the one meant.
#[test]
fn a_command_by_id_hits_its_clip_after_the_clips_before_it_moved() {
    let mut session = Session::new(project());
    let [first, second, third] = bass(&session)[..] else { panic!("three clips") };

    // The first clip moves to the end: the second clip is now at index 0, the third at 1.
    let moved = run(&mut session, "clip.move", json!({ "clip": first.0, "start": 24 })).unwrap();
    assert_eq!(moved["clip"], first.0, "a moved clip keeps its id");
    assert_eq!(bass(&session), [second, third, first]);

    // By id, the second clip is the one that changes.
    run(&mut session, "clip.setEnabled", json!({ "clips": [{ "clip": second.0 }], "enabled": false })).unwrap();
    assert!(!clip(&session, second).enabled);
    assert!(clip(&session, third).enabled);
    // By its old index, the same call would have hit the third clip: what ids are for.
    run(&mut session, "clip.setEnabled", json!({ "clips": [{ "track": "Bass", "index": 1 }], "enabled": false }))
        .unwrap();
    assert!(!clip(&session, third).enabled, "index 1 is the third clip now");

    // A deletion before a clip does not move what its id names either.
    run(&mut session, "clip.delete", json!({ "clip": second.0 })).unwrap();
    assert_eq!(bass(&session), [third, first]);
    run(&mut session, "clip.move", json!({ "clip": first.0, "start": 30 })).unwrap();
    assert_eq!(clip(&session, first).start, 30.0);
    assert_eq!(clip(&session, third).start, 16.0);
}

#[test]
fn the_seven_clip_commands_take_an_id() {
    let mut session = Session::new(project());
    let [first, second, third] = bass(&session)[..] else { panic!("three clips") };

    run(&mut session, "clip.setFades", json!({ "clip": second.0, "in": 0.5 })).unwrap_err();
    // Fades and gain are for audio clips; a MIDI clip refuses them by id as by index.
    let by_index = run(&mut session, "clip.setGain", json!({ "track": 1, "index": 1, "db": -6 })).unwrap_err();
    let by_id = run(&mut session, "clip.setGain", json!({ "clip": second.0, "db": -6 })).unwrap_err();
    assert_eq!(by_index, by_id);

    run(&mut session, "clip.copy", json!({ "clip": third.0 })).unwrap();
    run(&mut session, "clip.paste", json!({ "track": "Lead", "start": 0 })).unwrap();
    let pasted = session.project().arrangement.lanes[1].clips[0].id;
    assert!(![first, second, third].contains(&pasted), "a pasted clip is a new clip");

    let duplicated =
        run(&mut session, "clip.duplicate", json!({ "clips": [{ "clip": first.0 }], "stack": true })).unwrap();
    let copy = ClipId(u32::try_from(duplicated["clips"][0]["clip"].as_u64().unwrap()).unwrap());
    assert!(![first, second, third, pasted].contains(&copy), "a duplicate is a new clip");
    assert_eq!(clip(&session, copy).start, 4.0);

    run(&mut session, "clip.cut", json!({ "clips": [{ "clip": copy.0 }] })).unwrap();
    assert!(session.project().clip(copy).is_none());

    run(&mut session, "clip.delete", json!({ "clips": [{ "clip": first.0 }, { "clip": third.0 }] })).unwrap();
    assert_eq!(bass(&session), [second]);
    run(&mut session, "clip.move", json!({ "clip": second.0, "start": 2, "toTrack": "Lead" })).unwrap();
    assert_eq!(
        session.project().clip_place(second),
        Some(ClipPlace::Lane { lane: 1, index: 1 }),
        "after the pasted clip"
    );
    // Each of them, undone, brings back the clips with the ids they had.
    for _ in 0..5 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert_eq!(bass(&session), [first, second, third]);
}

#[test]
fn split_duplicate_and_paste_give_new_ids_and_the_neighbours_keep_theirs() {
    let mut session = Session::new(project());
    let [first, second, third] = bass(&session)[..] else { panic!("three clips") };
    let split = run(&mut session, "clip.split", json!({ "clip": second.0, "at": 10 })).unwrap();
    let new = ClipId(u32::try_from(split["second_clip"].as_u64().unwrap()).unwrap());
    assert_eq!(split["first_clip"], second.0, "the first part is the clip, shortened");
    assert_eq!(bass(&session), [first, second, new, third]);
    assert!(new.0 > third.0);

    // A clip placed over the middle of another splits it: the left part keeps the id.
    run(&mut session, "clip.copy", json!({ "clip": first.0 })).unwrap();
    run(&mut session, "clip.paste", json!({ "track": "Bass", "start": 17 })).unwrap();
    let ids = bass(&session);
    assert_eq!(ids[..3], [first, second, new]);
    assert_eq!(ids[3], third, "the part before the paste is the clip");
    assert_eq!(clip(&session, third).length, 1.0);
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len(), "{ids:?}");
    assert_eq!(session.project().validate(), Ok(()));
}

#[test]
fn moving_one_clip_changes_only_that_clips_record() {
    let mut session = Session::new(project());
    let before = session.project().clone();
    let [_, second, _] = bass(&session)[..] else { panic!("three clips") };
    run(&mut session, "clip.move", json!({ "clip": second.0, "start": 30 })).unwrap();
    let changed: Vec<ClipId> =
        before.all_clips().filter(|old| session.project().clip(old.id) != Some(old)).map(|c| c.id).collect();
    assert_eq!(changed, [second]);
}

#[test]
fn an_id_freed_by_undo_is_not_handed_out_again() {
    let mut session = Session::new(project());
    let first = bass(&session)[0];
    run(&mut session, "clip.copy", json!({ "clip": first.0 })).unwrap();
    let pasted = run(&mut session, "clip.paste", json!({ "track": "Lead", "start": 0 })).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    let again = run(&mut session, "clip.paste", json!({ "track": "Lead", "start": 4 })).unwrap();
    assert_ne!(pasted["clips"][0]["clip"], again["clips"][0]["clip"]);
    assert!(again["clips"][0]["clip"].as_u64() > pasted["clips"][0]["clip"].as_u64());
}

/// A duplicate is a clip of its own (#352): a note edit through it, named by its id, leaves the
/// original's notes as they were.
#[test]
fn a_duplicate_named_by_its_id_is_edited_alone() {
    let mut session = Session::new(project());
    let [first, ..] = bass(&session)[..] else { panic!("three clips") };
    let made = run(&mut session, "clip.duplicate", json!({ "clips": [{ "clip": first.0 }], "stack": true })).unwrap();
    let copy = made["clips"][0]["clip"].clone();
    run(&mut session, "note.add", json!({ "clip": copy, "notes": [{ "time": 2, "length": 1, "key": 60 }] })).unwrap();
    let notes = |session: &mut Session, clip: Value| {
        run(session, "note.list", json!({ "clip": clip })).unwrap().as_array().map_or(0, Vec::len)
    };
    assert_eq!(notes(&mut session, copy), 2);
    assert_eq!(notes(&mut session, json!(first.0)), 1);
}

#[test]
fn a_slot_clip_is_named_by_id_as_well() {
    let mut session = Session::new(project());
    let slot = session.project().launcher.columns[0].slots[0].clip.as_ref().unwrap().id;
    let set = run(&mut session, "clip.setLength", json!({ "clip": slot.0, "length": 2 })).unwrap();
    assert_eq!(set, json!({ "clip": slot.0, "scene": 1, "length": 2.0 }));
    run(&mut session, "clip.delete", json!({ "clip": slot.0 })).unwrap();
    assert!(session.project().launcher.columns[0].slots[0].clip.is_none());
}

#[test]
fn a_clip_is_named_by_a_name_only_it_has() {
    let mut session = Session::new(project());
    // Hook is on the arrangement and in a slot: two clips, so its name is not enough.
    let err = run(&mut session, "clip.setLength", json!({ "clip": "Hook", "length": 2 })).unwrap_err();
    assert!(err.contains("several clips are named \"Hook\""), "{err}");
    let slot = session.project().launcher.columns[0].slots[0].clip.as_ref().unwrap().id;
    run(&mut session, "clip.delete", json!({ "clip": slot.0 })).unwrap();
    let third = bass(&session)[2];
    run(&mut session, "clip.setLength", json!({ "clip": "hook", "length": 2 })).unwrap();
    assert_eq!(clip(&session, third).length, 2.0);
}

#[test]
fn a_clip_named_wrongly_is_an_error_that_changes_nothing() {
    let mut session = Session::new(project());
    let [first, ..] = bass(&session)[..] else { panic!("three clips") };
    let slot = session.project().launcher.columns[0].slots[0].clip.as_ref().unwrap().id;
    let lane = session.project().arrangement.lanes[0].id;
    let before = session.project().clone();
    for (command, params, says) in [
        ("clip.move", json!({ "clip": 999, "start": 0 }), "clip 999 does not exist"),
        ("clip.move", json!({ "clip": lane.0, "start": 0 }), "does not exist"),
        ("clip.move", json!({ "clip": 0, "start": 0 }), "does not exist"),
        ("clip.move", json!({ "clip": -1, "start": 0 }), "whole number"),
        ("clip.move", json!({ "clip": [1], "start": 0 }), "clip id or name"),
        ("clip.move", json!({ "clip": "Nothing", "start": 0 }), "no clip is named"),
        ("clip.move", json!({ "clip": first.0, "index": 0, "start": 0 }), "not both"),
        ("clip.move", json!({ "clip": first.0, "track": "Lead", "start": 0 }), "not on that track"),
        ("clip.move", json!({ "clip": slot.0, "start": 0 }), "in a clip slot"),
        ("clip.setEnabled", json!({ "clips": [{ "clip": slot.0 }], "enabled": false }), "in a clip slot"),
        ("clip.setEnabled", json!({ "clips": [{ "clip": first.0, "index": 0 }], "enabled": false }), "not both"),
        ("note.list", json!({ "clip": first.0, "scene": 1 }), "not both"),
    ] {
        let err = run(&mut session, command, params.clone()).unwrap_err();
        assert!(err.contains(says), "{command} {params}: {err} should say {says:?}");
    }
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
}

#[test]
fn every_command_that_takes_a_clip_index_takes_a_clip_id_and_says_the_index_is_deprecated() {
    for spec in suhina_session::commands() {
        let index = spec.params.iter().find(|p| p.name == "index" && p.doc.contains("project.clips"));
        let Some(index) = index else { continue };
        assert!(spec.params.iter().any(|p| p.name == "clip"), "{} takes index but not clip", spec.id);
        assert!(index.doc.contains("deprecated, use clip"), "{}: {}", spec.id, index.doc);
        assert!(!index.required, "{}: index is optional beside clip", spec.id);
        let schema = spec.input_schema();
        assert_eq!(schema["properties"]["clip"]["type"], json!(["integer", "string"]), "{}", spec.id);
        let required = schema["required"].as_array().unwrap();
        assert!(!required.contains(&json!("track")), "{}: track is not needed with clip", spec.id);
    }
    for id in ["clip.delete", "clip.setGain", "clip.setFades", "clip.copy", "clip.cut", "clip.duplicate", "clip.move"] {
        let spec = suhina_session::commands().iter().find(|c| c.id == id).unwrap();
        let takes_ids = spec.params.iter().any(|p| p.name == "clip")
            || spec.params.iter().any(|p| p.name == "clips" && p.doc.contains("\"clip\": id"));
        assert!(takes_ids, "{id} takes clip ids");
    }
}
