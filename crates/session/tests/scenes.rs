// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Scene commands: insert, duplicate, delete, rename, tempo and time signature, with undo, and
//! what a playing engine does when the scenes change under it (rendered offline here).
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use serde_json::{Value, json};
use suhina_engine::{AudioPool, Renderer};
use suhina_model::*;
use suhina_session::{Player, Session};

use crate::support::run;

const SR: u32 = 48_000;
const BEAT: usize = SR as usize / 2;

/// Two tracks and two scenes; track 1 has a clip in both, track 2 only in scene 2.
fn project() -> Project {
    suhina_test_support::with_ids(project_built())
}

fn project_built() -> Project {
    let track = |id: u32| Track {
        content: ContentKinds { notes: true, audio: false },
        channel: Some(ChannelId(id)),
        ..Track::new(TrackId(id), format!("T{id}"))
    };
    let channel = |id: u32| Channel::new(ChannelId(id), format!("T{id}"));
    let content = |id: u32| ClipContent {
        id: ContentId(id),
        name: format!("Clip {id}"),
        length: 1.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![Note::new(0.0, 0.25, 60, 1.0)].into()) }]
            .into(),
        extensions: Extensions::default(),
        scale: None,
    };
    let clip = |content: u32| Clip {
        loop_range: Some(LoopRange { start: 0.0, end: 1.0 }),
        ..Clip::new(ContentId(content), 0.0, 1.0)
    };
    let slot = |content: Option<u32>| Slot { clip: content.map(clip), has_stop: true };
    Project {
        tracks: vec![track(1), track(2)],
        channels: vec![channel(1), channel(2)],
        contents: vec![content(1), content(2), content(3)],
        launcher: Launcher {
            scenes: vec![
                Scene { name: "Intro".into(), ..Scene::default() },
                Scene { name: "Drop".into(), ..Scene::default() },
            ],
            columns: vec![
                LauncherColumn { track: TrackId(1), slots: vec![slot(Some(1)), slot(Some(2))] },
                LauncherColumn { track: TrackId(2), slots: vec![slot(None), slot(Some(3))] },
            ],
        },
        ..Project::default()
    }
}

fn names(session: &Session) -> Vec<String> {
    session.project().launcher.scenes.iter().map(|s| s.name.clone()).collect()
}

/// Every column has one slot per scene.
fn assert_in_step(session: &Session) {
    let launcher = &session.project().launcher;
    assert!(launcher.columns.iter().all(|c| c.slots.len() == launcher.scenes.len()));
}

#[test]
fn inserting_a_scene_adds_a_row_of_empty_slots_and_undo_removes_it() {
    let mut session = Session::new(project());
    assert_eq!(run(&mut session, "scene.insert", json!({ "scene": 1, "name": " Break " })).unwrap()["scene"], 1);
    assert_eq!(names(&session), ["Intro", "Break", "Drop"]);
    assert_in_step(&session);
    let row: Vec<&Slot> = session.project().launcher.columns.iter().map(|c| &c.slots[1]).collect();
    assert!(row.iter().all(|s| s.clip.is_none() && s.has_stop));
    // Left out, the scene goes after the last one.
    assert_eq!(run(&mut session, "scene.insert", json!({})).unwrap()["scene"], 3);
    assert_eq!(names(&session), ["Intro", "Break", "Drop", ""]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    // Undo takes the edit back; the id counter stays where it got to (ADR 0025).
    assert_eq!(session.project(), &Project { next_id: session.project().next_id, ..project() });
}

#[test]
fn duplicating_a_scene_copies_its_clips_below_it() {
    let mut session = Session::new(project());
    run(&mut session, "scene.setTempo", json!({ "scene": 1, "bpm": 140 })).unwrap();
    assert_eq!(run(&mut session, "scene.duplicate", json!({ "scene": 1 })).unwrap()["scene"], 2);
    let p = session.project();
    assert_eq!(names(&session), ["Intro", "Drop", "Drop"]);
    assert_eq!(p.launcher.scenes[2].tempo_bpm, Some(140.0));
    assert_in_step(&session);
    // The copies play contents of their own, equal to the originals.
    for column in &p.launcher.columns {
        let (original, copy) = (column.slots[1].clip.as_ref().unwrap(), column.slots[2].clip.as_ref().unwrap());
        assert_ne!(original.content, copy.content);
        let lanes = |id| p.content(id).map(|c| c.lanes.clone());
        assert_eq!(lanes(original.content), lanes(copy.content));
    }
    assert_eq!(p.contents.len(), 5);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().launcher.scenes.len(), 2);
    assert_eq!(session.project().contents.len(), 3);
}

#[test]
fn deleting_a_scene_removes_its_row_but_never_the_last_scene() {
    let mut session = Session::new(project());
    assert_eq!(run(&mut session, "scene.delete", json!({ "scene": 0 })).unwrap()["deleted"], 0);
    assert_eq!(names(&session), ["Drop"]);
    assert_in_step(&session);
    assert_eq!(session.project().launcher.columns[1].slots[0].clip.as_ref().unwrap().content, ContentId(3));
    let err = run(&mut session, "scene.delete", json!({ "scene": 0 })).unwrap_err();
    assert!(err.contains("last scene"), "{err}");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &project());
}

#[test]
fn a_scene_is_renamed_and_given_a_tempo_and_time_signature() {
    let mut session = Session::new(project());
    run(&mut session, "scene.rename", json!({ "scene": 0, "name": "Verse" })).unwrap();
    run(&mut session, "scene.setTempo", json!({ "scene": 0, "bpm": 96.5 })).unwrap();
    let set = run(&mut session, "scene.setTimeSignature", json!({ "scene": 0, "numerator": 7, "denominator": 8 }));
    assert_eq!(set.unwrap()["time_signature"], "7/8");
    let grid = run(&mut session, "project.launcher", json!({})).unwrap();
    let id = session.project().launcher.scenes[0].id.0;
    assert_eq!(
        grid["scenes"][0],
        json!({ "scene": 0, "id": id, "scene_id": id, "name": "Verse", "tempo_bpm": 96.5, "time_signature": "7/8" })
    );
    // Leaving the values out clears them; an empty name clears the name.
    run(&mut session, "scene.setTempo", json!({ "scene": 0 })).unwrap();
    run(&mut session, "scene.setTimeSignature", json!({ "scene": 0, "numerator": null, "denominator": null })).unwrap();
    run(&mut session, "scene.rename", json!({ "scene": 0, "name": "  " })).unwrap();
    let id = session.project().launcher.scenes[0].id;
    assert_eq!(session.project().launcher.scenes[0], Scene { id, ..Scene::default() });
    for _ in 0..6 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert_eq!(session.project(), &project());
}

#[test]
fn malformed_scene_parameters_are_refused_and_change_nothing() {
    let mut session = Session::new(project());
    let bad = [
        ("scene.insert", json!({ "scene": 99 })),
        ("scene.insert", json!({ "scene": 3 })),
        ("scene.insert", json!({ "scene": "two" })),
        ("scene.insert", json!({ "name": 3 })),
        ("scene.duplicate", json!({})),
        ("scene.duplicate", json!({ "scene": 2 })),
        ("scene.delete", json!({ "scene": -1 })),
        ("scene.delete", json!({ "scene": 99.5 })),
        ("scene.rename", json!({ "scene": 99 })),
        ("scene.rename", json!({ "scene": 99, "name": false })),
        ("scene.rename", json!({ "scene": 8, "name": "X" })),
        ("scene.setTempo", json!({ "scene": 99, "bpm": 10 })),
        ("scene.setTempo", json!({ "scene": 99, "bpm": 1000 })),
        ("scene.setTempo", json!({ "scene": 99, "bpm": "fast" })),
        ("scene.setTimeSignature", json!({ "scene": 99, "numerator": 3 })),
        ("scene.setTimeSignature", json!({ "scene": 99, "numerator": 0, "denominator": 4 })),
        ("scene.setTimeSignature", json!({ "scene": 99, "numerator": 100, "denominator": 4 })),
        ("scene.setTimeSignature", json!({ "scene": 99, "numerator": 3, "denominator": 3 })),
        ("scene.setTimeSignature", json!({ "scene": 99, "numerator": 300, "denominator": 4 })),
        ("scene.setTimeSignature", json!({ "scene": 99, "numerator": "3", "denominator": 4 })),
        ("scene.setTempo", json!({ "scene": 99, "bpm": 120, "colour": "red" })),
    ];
    for (id, params) in bad {
        assert!(run(&mut session, id, params.clone()).is_err(), "{id} {params} was accepted");
    }
    assert_eq!(session.project(), &project());
    assert!(!session.can_undo());
}

/// The id of the scene named `name`.
fn scene_id_of(session: &Session, name: &str) -> u32 {
    session.project().launcher.scenes.iter().find(|s| s.name == name).unwrap().id.0
}

#[test]
fn a_scene_named_by_its_id_is_found_where_it_has_moved() {
    let mut session = Session::new(project());
    let drop = scene_id_of(&session, "Drop");
    // Scenes inserted before it move Drop down; its id still names it.
    let inserted = run(&mut session, "scene.insert", json!({ "scene": 0, "name": "Count" })).unwrap();
    assert_eq!(inserted["scene_id"], scene_id_of(&session, "Count"));
    let renamed = run(&mut session, "scene.rename", json!({ "scene": { "id": drop }, "name": "Chorus" })).unwrap();
    assert_eq!(renamed["scene"], 2);
    assert_eq!(renamed["scene_id"], drop);
    assert_eq!(names(&session), ["Count", "Intro", "Chorus"]);
    // Every command that takes a scene takes the id: a clip's slot by scene, a launch, a move.
    let tempo = run(&mut session, "scene.setTempo", json!({ "scene": { "id": drop }, "bpm": 128 })).unwrap();
    assert_eq!((tempo["scene"].clone(), tempo["scene_id"].clone()), (json!(2), json!(drop)));
    let notes = run(&mut session, "note.list", json!({ "track": 1, "scene": { "id": drop } })).unwrap();
    assert_eq!(notes.as_array().map(Vec::len), Some(1));
    let created = run(&mut session, "clip.create", json!({ "track": 2, "scene": 0 })).unwrap();
    assert_eq!(created["scene_id"], scene_id_of(&session, "Count"));
    let intro = scene_id_of(&session, "Intro");
    let moved =
        run(&mut session, "clip.moveSlot", json!({ "track": 2, "scene": 0, "toScene": { "id": intro } })).unwrap();
    assert_eq!((moved["scene"].clone(), moved["scene_id"].clone()), (json!(1), json!(intro)));
    // Inserting before a scene named by id puts the new one in its place.
    let before = run(&mut session, "scene.insert", json!({ "scene": { "id": drop }, "name": "Break" })).unwrap();
    assert_eq!(before["scene"], 2);
    assert_eq!(names(&session), ["Count", "Intro", "Break", "Chorus"]);
    // project.launcher gives each slot's scene id beside its number.
    let grid = run(&mut session, "project.launcher", json!({})).unwrap();
    assert_eq!(grid["tracks"][0]["slots"][3]["scene"], 3);
    assert_eq!(grid["tracks"][0]["slots"][3]["scene_id"], drop);
    assert_eq!(grid["scenes"][3]["scene_id"], drop);
    // A deleted scene's id names nothing.
    let deleted = run(&mut session, "scene.delete", json!({ "scene": { "id": drop } })).unwrap();
    assert_eq!((deleted["deleted"].clone(), deleted["scene_id"].clone()), (json!(3), json!(drop)));
    let err = run(&mut session, "scene.rename", json!({ "scene": { "id": drop }, "name": "X" })).unwrap_err();
    assert!(err.contains(&format!("no scene has id {drop}")), "{err}");
}

/// The clip commands that name a slot by scene take the scene's id as well as its number.
#[test]
fn the_clip_commands_take_a_scene_by_its_id() {
    let mut session = Session::new(project());
    let drop = scene_id_of(&session, "Drop");
    run(&mut session, "scene.insert", json!({ "scene": 0, "name": "Count" })).unwrap();
    let by_id = json!({ "id": drop });
    run(&mut session, "clip.copy", json!({ "track": 1, "scene": by_id })).unwrap();
    run(&mut session, "clip.paste", json!({ "track": 2, "scene": by_id })).unwrap();
    assert!(session.project().launcher.columns[1].slots[2].clip.is_some());
    // The scene is found; the clip there is a MIDI clip, which has no fades.
    let err = run(&mut session, "clip.setFades", json!({ "track": 1, "scene": by_id, "in": 0 })).unwrap_err();
    assert!(err.contains("only an audio clip"), "{err}");
    let deleted = run(&mut session, "clip.delete", json!({ "track": 2, "scene": by_id })).unwrap();
    assert_eq!(deleted["deleted"], 2);
    assert!(session.project().launcher.columns[1].slots[2].clip.is_none());
}

#[test]
fn malformed_scene_ids_are_refused_and_change_nothing() {
    let mut session = Session::new(project());
    let bad = [
        ("scene.rename", json!({ "scene": { "id": 999 }, "name": "X" })),
        ("scene.rename", json!({ "scene": { "id": 0 }, "name": "X" })),
        ("scene.rename", json!({ "scene": { "id": -1 }, "name": "X" })),
        ("scene.rename", json!({ "scene": { "id": "1" }, "name": "X" })),
        ("scene.rename", json!({ "scene": {}, "name": "X" })),
        ("scene.insert", json!({ "scene": { "id": 999 } })),
        ("scene.delete", json!({ "scene": { "id": 4_294_967_296_u64 } })),
        ("note.list", json!({ "track": 1, "scene": { "id": 999 } })),
        ("clip.moveSlot", json!({ "track": 1, "scene": 0, "toScene": { "id": 999 } })),
        ("clip.create", json!({ "track": 2, "scene": { "id": 999 } })),
    ];
    for (id, params) in bad {
        let err = run(&mut session, id, params.clone());
        assert!(err.is_err(), "{id} {params} was accepted");
    }
    let err = run(&mut session, "scene.rename", json!({ "scene": { "id": 999 }, "name": "X" })).unwrap_err();
    assert!(err.contains("no scene has id 999"), "{err}");
    assert_eq!(session.project(), &project());
    assert!(!session.can_undo());
}

/// A session with an offline renderer attached as its player.
fn playing() -> (Session, Renderer) {
    let project = project();
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR, suhina_session::devices()).unwrap();
    let mut session = Session::new(project);
    session.attach_player(Player::new(renderer.controller(), None));
    (session, renderer)
}

fn advance(renderer: &mut Renderer, frames: usize) {
    let mut buffer = vec![0.0f32; 2 * 256];
    let mut left = frames;
    while left > 0 {
        let n = left.min(256);
        renderer.process(&mut buffer[..2 * n]);
        left -= n;
    }
}

/// The scene each track plays, from `launcher.state`.
fn playing_scenes(session: &mut Session) -> Vec<Value> {
    let state = run(session, "launcher.state", json!({})).unwrap();
    state["tracks"].as_array().unwrap().iter().map(|t| t["playing"]["scene"].clone()).collect()
}

#[test]
fn playing_clips_stay_with_their_slots_when_scenes_are_inserted_or_deleted() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "scene.launch", json!({ "scene": 1 })).unwrap();
    advance(&mut renderer, BEAT);
    assert_eq!(playing_scenes(&mut session), [json!(1), json!(1)]);
    // A scene inserted above: the playing row moves down to scene 2 and keeps playing.
    run(&mut session, "scene.insert", json!({ "scene": 0 })).unwrap();
    advance(&mut renderer, BEAT);
    assert_eq!(playing_scenes(&mut session), [json!(2), json!(2)]);
    // Undo removes it again, and the row is scene 1 once more.
    run(&mut session, "edit.undo", json!({})).unwrap();
    advance(&mut renderer, BEAT);
    assert_eq!(playing_scenes(&mut session), [json!(1), json!(1)]);
    // Deleting the scene above moves the row up; deleting the playing scene stops its clips.
    run(&mut session, "scene.delete", json!({ "scene": 0 })).unwrap();
    advance(&mut renderer, BEAT);
    assert_eq!(playing_scenes(&mut session), [json!(0), json!(0)]);
    run(&mut session, "scene.insert", json!({})).unwrap();
    run(&mut session, "scene.delete", json!({ "scene": 0 })).unwrap();
    advance(&mut renderer, BEAT);
    assert_eq!(playing_scenes(&mut session), [Value::Null, Value::Null]);
}

#[test]
fn a_launched_scene_sets_the_time_signature_the_transport_counts_in() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "scene.setTimeSignature", json!({ "scene": 1, "numerator": 3, "denominator": 4 })).unwrap();
    run(&mut session, "scene.setTempo", json!({ "scene": 1, "bpm": 60 })).unwrap();
    advance(&mut renderer, 256);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["time_signature"], "4/4");
    run(&mut session, "scene.launch", json!({ "scene": 1 })).unwrap();
    // At 60 bpm a beat lasts a second: four seconds is beat 4, the second beat of bar 2 in 3/4.
    advance(&mut renderer, 4 * SR as usize + 256);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["time_signature"], "3/4");
    assert_eq!(state["tempo_bpm"], 60.0);
    assert_eq!(state["position"], "2.2");
}

#[test]
fn follow_tempo_map_takes_the_transport_back_from_a_scene() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "scene.setTimeSignature", json!({ "scene": 1, "numerator": 3, "denominator": 4 })).unwrap();
    run(&mut session, "scene.setTempo", json!({ "scene": 1, "bpm": 60 })).unwrap();
    advance(&mut renderer, 256);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["follows_tempo_map"], true);
    run(&mut session, "scene.launch", json!({ "scene": 1 })).unwrap();
    advance(&mut renderer, 2 * SR as usize);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!((state["follows_tempo_map"].clone(), state["tempo_bpm"].clone()), (json!(false), json!(60.0)));
    // Not an edit: nothing to undo.
    let undo = session.can_undo();
    run(&mut session, "transport.followTempoMap", json!({})).unwrap();
    advance(&mut renderer, 256);
    let state = run(&mut session, "launcher.state", json!({})).unwrap();
    assert_eq!(state["follows_tempo_map"], true);
    assert_eq!(state["tempo_bpm"], session.project().transport.tempo_bpm);
    assert_eq!(state["time_signature"], "4/4");
    assert_eq!(session.can_undo(), undo);
}
