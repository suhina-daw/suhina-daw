// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Keys and MIDI notes mapped to slots, scenes and stop buttons, the project's controller
//! mappings (`mapping.add`, `mapping.remove`, `mapping.list`), and the commands their presses and
//! releases run, through a real renderer rendered offline (`docs/spec/midi-mapping.md` › Launch
//! keys, `docs/spec/clip-launching.md` › Launching from keys and notes).
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use serde_json::{Value, json};
use suhina_engine::{AudioPool, Renderer};
use suhina_model::*;
use suhina_session::{MAX_MAPPINGS, Player, Session, mapped_command};

use crate::support::run;

const SR: u32 = 48_000;
const BEAT: usize = SR as usize / 2;
/// Two tracks: Drums with a one-beat loop in scene 1, and an empty slot in scene 2; Bass with
/// an empty slot without a stop button in scene 1 and a loop in scene 2.
fn project() -> Project {
    suhina_test_support::with_ids(project_built())
}

fn project_built() -> Project {
    let track = |id: u32, name: &str| Track {
        content: ContentKinds { notes: true, audio: false },
        channel: Some(ChannelId(id)),
        ..Track::new(TrackId(id), name)
    };
    let channel = |id: u32| Channel::new(ChannelId(id), format!("C{id}"));
    let content = |id: u32| ClipContent {
        id: ContentId(id),
        name: format!("Loop {id}"),
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
    Project {
        tracks: vec![track(1, "Drums"), track(2, "Bass")],
        channels: vec![channel(1), channel(2)],
        contents: vec![content(1), content(2)],
        launcher: Launcher {
            scenes: vec![Scene::default(), Scene::default()],
            columns: vec![
                LauncherColumn {
                    track: TrackId(1),
                    slots: vec![Slot { clip: Some(clip(1)), has_stop: true }, Slot { clip: None, has_stop: true }],
                },
                LauncherColumn {
                    track: TrackId(2),
                    slots: vec![Slot { clip: None, has_stop: false }, Slot { clip: Some(clip(2)), has_stop: true }],
                },
            ],
        },
        ..Project::default()
    }
}

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

fn state(session: &mut Session) -> Value {
    run(session, "launcher.state", json!({})).unwrap()
}

fn key(name: &str) -> MappingSource {
    MappingSource::key(name).unwrap()
}

/// Presses or releases `source` the way the window does: runs the command it maps to.
fn press(session: &mut Session, source: &MappingSource, pressed: bool) -> Option<&'static str> {
    let (id, params) = mapped_command(session.project(), source, pressed)?;
    run(session, id, params).unwrap();
    Some(id)
}

#[test]
fn mappings_are_edits_with_undo_and_are_listed() {
    let mut session = Session::new(project());
    let added = run(
        &mut session,
        "mapping.add",
        json!({ "source": "key:Q", "target": { "command": "clip.launch", "track": "Drums", "scene": 0 } }),
    )
    .unwrap();
    assert_eq!(added["target"], json!({ "command": "clip.launch", "track": 1, "scene": 0 }));
    assert_eq!((&added["index"], &added["release"]), (&json!(0), &json!("clip.release")));
    // A scene by its id, as every command takes one (#1156).
    let second = session.project().launcher.scenes[1].id.0;
    run(
        &mut session,
        "mapping.add",
        json!({ "source": "note:10:36", "target": { "command": "scene.launch", "scene": { "id": second } } }),
    )
    .unwrap();
    run(
        &mut session,
        "mapping.add",
        json!({ "source": "key:Space", "target": { "command": "track.stop", "track": 2 } }),
    )
    .unwrap();
    let list = run(&mut session, "mapping.list", json!({})).unwrap();
    let scene_id = |i: usize| session.project().launcher.scenes[i].id.0;
    assert_eq!(
        list["mappings"],
        json!([
            { "index": 0, "source": "key:Q", "label": "Q", "target": { "command": "clip.launch", "track": 1, "scene": 0 }, "scene_id": scene_id(0), "release": "clip.release", "broken": false },
            { "index": 1, "source": "note:10:36", "label": "Ch 10 Note 36", "target": { "command": "scene.launch", "scene": 1 }, "scene_id": scene_id(1), "broken": false },
            { "index": 2, "source": "key:Space", "label": "Space", "target": { "command": "track.stop", "track": 2 }, "broken": false },
        ])
    );
    // What the list gives as a target is what mapping.add and mapping.remove take.
    let target = list["mappings"][1]["target"].clone();
    assert!(run(&mut session, "mapping.add", json!({ "source": "key:T", "target": target })).is_ok());
    run(&mut session, "edit.undo", json!({})).unwrap();

    // A source mapped again moves to its new target; it stays one mapping.
    let moved = run(
        &mut session,
        "mapping.add",
        json!({ "source": "key:Q", "target": { "command": "scene.launch", "scene": 0 } }),
    )
    .unwrap();
    assert_eq!(moved["replaced"], json!({ "command": "clip.launch", "track": 1, "scene": 0 }));
    assert_eq!(moved["index"], 0);
    assert_eq!(session.project().controller_mappings.len(), 3);
    assert_eq!(session.project().controller_mappings[0].target, MappingTarget::Scene { scene: 0 });

    // Removing every mapping of a target, by source, and by index.
    run(&mut session, "mapping.add", json!({ "source": "key:W", "target": { "command": "track.stop", "track": 2 } }))
        .unwrap();
    let removed =
        run(&mut session, "mapping.remove", json!({ "target": { "command": "track.stop", "track": "Bass" } })).unwrap();
    let indices: Vec<&Value> = removed["removed"].as_array().unwrap().iter().map(|m| &m["index"]).collect();
    assert_eq!(indices, [&json!(2), &json!(3)]);
    run(&mut session, "mapping.remove", json!({ "source": "note:10:36" })).unwrap();
    assert_eq!(session.project().controller_mappings.len(), 1);
    let removed = run(&mut session, "mapping.remove", json!({ "index": 0 })).unwrap();
    assert_eq!(removed["removed"][0]["source"], "key:Q");
    assert!(session.project().controller_mappings.is_empty());

    // One undo step each.
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().controller_mappings.len(), 4);
    for _ in 0..5 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert!(session.project().controller_mappings.is_empty());
    assert!(!session.can_undo());
}

#[test]
fn the_launcher_mapping_commands_are_gone() {
    let mut session = Session::new(project());
    for id in ["launcher.addMapping", "launcher.removeMapping", "launcher.mappings"] {
        assert!(suhina_session::commands().iter().all(|c| c.id != id), "{id}");
        assert!(run(&mut session, id, json!({})).is_err(), "{id}");
    }
}

#[test]
fn malformed_mappings_are_errors_and_change_nothing() {
    let mut session = Session::new(project());
    run(&mut session, "mapping.add", json!({ "source": "key:Q", "target": { "command": "scene.launch", "scene": 0 } }))
        .unwrap();
    let before = session.project().clone();
    let scene = |scene: Value| json!({ "command": "scene.launch", "scene": scene });
    let bad = [
        ("mapping.add", json!({ "source": "Q", "target": scene(json!(1)) })),
        ("mapping.add", json!({ "source": "key:", "target": scene(json!(1)) })),
        ("mapping.add", json!({ "source": "key:two words", "target": scene(json!(1)) })),
        ("mapping.add", json!({ "source": "note:0:60", "target": scene(json!(1)) })),
        ("mapping.add", json!({ "source": "note:1:128", "target": scene(json!(1)) })),
        ("mapping.add", json!({ "source": "cc:1:20", "target": scene(json!(1)) })),
        ("mapping.add", json!({ "source": 5, "target": scene(json!(1)) })),
        ("mapping.add", json!({ "source": "key:W" })),
        ("mapping.add", json!({ "target": scene(json!(1)) })),
        // The target: not an object, an old kind's name, no command, a command that is not a
        // launch command, a parameter missing, one the command does not take.
        ("mapping.add", json!({ "source": "key:W", "target": "scene", "scene": 1 })),
        ("mapping.add", json!({ "source": "key:W", "target": "scene" })),
        ("mapping.add", json!({ "source": "key:W", "target": 3 })),
        ("mapping.add", json!({ "source": "key:W", "target": {} })),
        ("mapping.add", json!({ "source": "key:W", "target": { "command": 5, "scene": 1 } })),
        ("mapping.add", json!({ "source": "key:W", "target": { "command": "slot", "track": 1, "scene": 1 } })),
        ("mapping.add", json!({ "source": "key:W", "target": { "command": "track.setVolume", "track": 1 } })),
        ("mapping.add", json!({ "source": "key:W", "target": { "command": "scene.launch" } })),
        ("mapping.add", json!({ "source": "key:W", "target": { "command": "scene.launch", "scene": null } })),
        ("mapping.add", json!({ "source": "key:W", "target": { "command": "scene.launch", "scene": 1, "track": 1 } })),
        ("mapping.add", json!({ "source": "key:W", "target": { "command": "clip.launch", "scene": 1 } })),
        ("mapping.add", json!({ "source": "key:W", "target": { "command": "clip.launch", "track": 1 } })),
        ("mapping.add", json!({ "source": "key:W", "target": { "command": "track.stop" } })),
        ("mapping.add", json!({ "source": "key:W", "target": { "command": "track.stop", "track": 1, "extra": 1 } })),
        ("mapping.add", json!({ "source": "key:W", "target": scene(json!(1)), "extra": 1 })),
        // What the target names does not exist.
        ("mapping.add", json!({ "source": "key:W", "target": scene(json!(2)) })),
        ("mapping.add", json!({ "source": "key:W", "target": scene(json!(u64::MAX)) })),
        ("mapping.add", json!({ "source": "key:W", "target": scene(json!(-1)) })),
        ("mapping.add", json!({ "source": "key:W", "target": scene(json!({ "id": 999 })) })),
        ("mapping.add", json!({ "source": "key:W", "target": scene(json!("1")) })),
        (
            "mapping.add",
            json!({ "source": "key:W", "target": { "command": "clip.launch", "track": "Nope", "scene": 0 } }),
        ),
        ("mapping.add", json!({ "source": "key:W", "target": { "command": "track.stop", "track": 99 } })),
        ("mapping.remove", json!({})),
        ("mapping.remove", json!({ "source": "key:W" })),
        ("mapping.remove", json!({ "source": "bad" })),
        ("mapping.remove", json!({ "source": "key:Q", "target": scene(json!(0)) })),
        ("mapping.remove", json!({ "source": "key:Q", "index": 0 })),
        ("mapping.remove", json!({ "target": scene(json!(1)) })),
        ("mapping.remove", json!({ "target": scene(json!(99)) })),
        ("mapping.remove", json!({ "index": 1 })),
        ("mapping.remove", json!({ "index": -1 })),
        ("mapping.remove", json!({ "index": "0" })),
    ];
    for (id, params) in bad {
        assert!(run(&mut session, id, params.clone()).is_err(), "{id} {params}");
    }
    assert_eq!(session.project(), &before);
    assert_eq!(MAX_MAPPINGS, 1024);
}

#[test]
fn a_project_keeps_at_most_max_mappings() {
    let mut session = Session::new(project());
    let mut project = session.project().clone();
    project.controller_mappings = (0..MAX_MAPPINGS)
        .map(|i| ControllerMapping {
            source: MappingSource::note(1 + (i / 128) as u8, (i % 128) as u8).unwrap(),
            target: MappingTarget::Scene { scene: 0 },
        })
        .collect();
    session = Session::new(project);
    let scene = |n: usize| json!({ "command": "scene.launch", "scene": n });
    let err = run(&mut session, "mapping.add", json!({ "source": "key:Q", "target": scene(0) })).unwrap_err();
    assert!(err.contains("at most"), "{err}");
    // A source already mapped still moves.
    run(&mut session, "mapping.add", json!({ "source": "note:1:0", "target": scene(1) })).unwrap();
}

#[test]
fn presses_and_releases_run_the_commands_the_mouse_runs() {
    let mut session = Session::new(project());
    let add = |session: &mut Session, params: Value| run(session, "mapping.add", params).unwrap();
    add(&mut session, json!({ "source": "key:Q", "target": { "command": "clip.launch", "track": 1, "scene": 0 } }));
    add(&mut session, json!({ "source": "key:1", "target": { "command": "scene.launch", "scene": 1 } }));
    add(&mut session, json!({ "source": "note:1:36", "target": { "command": "track.stop", "track": 2 } }));
    let p = session.project();
    assert_eq!(mapped_command(p, &key("Q"), true), Some(("clip.launch", json!({ "track": 1, "scene": 0 }))));
    // A trigger clip does not listen for its release.
    assert_eq!(mapped_command(p, &key("Q"), false), None);
    assert_eq!(mapped_command(p, &key("1"), true), Some(("scene.launch", json!({ "scene": 1 }))));
    assert_eq!(mapped_command(p, &key("1"), false), None);
    let note = MappingSource::note(1, 36).unwrap();
    assert_eq!(mapped_command(p, &note, true), Some(("track.stop", json!({ "track": 2 }))));
    assert_eq!(mapped_command(p, &note, false), None);
    // Another channel, another key: not mapped.
    assert_eq!(mapped_command(p, &MappingSource::note(2, 36).unwrap(), true), None);
    assert_eq!(mapped_command(p, &key("W"), true), None);

    run(&mut session, "clip.setLaunchMode", json!({ "track": 1, "scene": 0, "mode": "repeat" })).unwrap();
    assert_eq!(
        mapped_command(session.project(), &key("Q"), false),
        Some(("clip.release", json!({ "track": 1, "scene": 0 })))
    );
}

#[test]
fn a_mapping_whose_track_is_gone_is_kept_unused_and_undo_brings_it_back() {
    let mut session = Session::new(project());
    run(
        &mut session,
        "mapping.add",
        json!({ "source": "key:Q", "target": { "command": "clip.launch", "track": 2, "scene": 1 } }),
    )
    .unwrap();
    run(&mut session, "track.delete", json!({ "track": "Bass" })).unwrap();
    assert_eq!(session.project().controller_mappings.len(), 1);
    assert_eq!(mapped_command(session.project(), &key("Q"), true), None);
    let list = run(&mut session, "mapping.list", json!({})).unwrap();
    assert_eq!(list["mappings"][0]["broken"], true);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(mapped_command(session.project(), &key("Q"), true).map(|(id, _)| id), Some("clip.launch"));
}

/// A mapping stays on its scene when scenes are inserted, duplicated or deleted before it; one on
/// a deleted scene goes with it, and undo brings it back.
#[test]
fn a_mapping_follows_its_scene_when_scenes_are_inserted_or_deleted() {
    let mut session = Session::new(project());
    let add = |session: &mut Session, source: &str, target: Value| {
        run(session, "mapping.add", json!({ "source": source, "target": target })).unwrap();
    };
    add(&mut session, "key:Q", json!({ "command": "clip.launch", "track": 2, "scene": 1 }));
    add(&mut session, "key:W", json!({ "command": "scene.launch", "scene": 0 }));
    add(&mut session, "key:E", json!({ "command": "track.stop", "track": 1 }));
    let at = |session: &Session, source: &str| {
        let mappings = &session.project().controller_mappings;
        mappings.iter().find(|m| m.source.name() == source).and_then(|m| m.target.scene())
    };
    run(&mut session, "scene.insert", json!({ "scene": 0 })).unwrap();
    assert_eq!((at(&session, "key:Q"), at(&session, "key:W"), at(&session, "key:E")), (Some(2), Some(1), None));
    run(&mut session, "scene.duplicate", json!({ "scene": 1 })).unwrap();
    assert_eq!((at(&session, "key:Q"), at(&session, "key:W")), (Some(3), Some(1)));
    run(&mut session, "scene.delete", json!({ "scene": 0 })).unwrap();
    assert_eq!((at(&session, "key:Q"), at(&session, "key:W")), (Some(2), Some(0)));
    run(&mut session, "scene.delete", json!({ "scene": 0 })).unwrap();
    assert_eq!(at(&session, "key:Q"), Some(1));
    assert_eq!(at(&session, "key:W"), None, "a mapping on a deleted scene goes with it");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(at(&session, "key:W"), Some(0));
}

#[test]
fn a_held_key_plays_a_gate_clip_and_its_release_stops_it() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "clip.setLaunchMode", json!({ "track": "Drums", "scene": 0, "mode": "gate" })).unwrap();
    run(&mut session, "clip.setQuantization", json!({ "track": "Drums", "scene": 0, "value": "none" })).unwrap();
    run(
        &mut session,
        "mapping.add",
        json!({ "source": "key:Q", "target": { "command": "clip.launch", "track": 1, "scene": 0 } }),
    )
    .unwrap();
    run(
        &mut session,
        "mapping.add",
        json!({ "source": "note:1:60", "target": { "command": "scene.launch", "scene": 1 } }),
    )
    .unwrap();
    run(&mut session, "mapping.add", json!({ "source": "key:S", "target": { "command": "track.stop", "track": 2 } }))
        .unwrap();

    assert_eq!(press(&mut session, &key("Q"), true), Some("clip.launch"));
    advance(&mut renderer, BEAT);
    assert_eq!(state(&mut session)["tracks"][0]["playing"]["scene"], 0);
    assert_eq!(press(&mut session, &key("Q"), false), Some("clip.release"));
    advance(&mut renderer, 1);
    assert_eq!(state(&mut session)["tracks"][0]["playing"], Value::Null);

    // A note launches scene 2 (Bass plays its loop there), a key stops Bass.
    run(&mut session, "launcher.setQuantization", json!({ "value": "none" })).unwrap();
    assert_eq!(press(&mut session, &MappingSource::note(1, 60).unwrap(), true), Some("scene.launch"));
    advance(&mut renderer, BEAT);
    assert_eq!(state(&mut session)["tracks"][1]["playing"]["scene"], 1);
    assert_eq!(press(&mut session, &key("S"), true), Some("track.stop"));
    advance(&mut renderer, 1);
    assert_eq!(state(&mut session)["tracks"][1]["playing"], Value::Null);
}
