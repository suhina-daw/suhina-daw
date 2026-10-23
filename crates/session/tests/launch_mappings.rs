// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Keys and MIDI notes mapped to slots, scenes and stop buttons (`launcher.addMapping`,
//! `launcher.removeMapping`, `launcher.mappings`), and the commands their presses and releases
//! run, through a real renderer rendered offline (`docs/spec/clip-launching.md` › Launching from
//! keys and notes).
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
        lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![Note::new(0.0, 0.25, 60, 1.0)]) }],
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
            mappings: Vec::new(),
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
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
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
        "launcher.addMapping",
        json!({ "source": "key:Q", "target": "slot", "track": "Drums", "scene": 1 }),
    )
    .unwrap();
    assert_eq!(added["command"], "clip.launch");
    assert_eq!((&added["track"], &added["scene"]), (&json!(1), &json!(1)));
    run(&mut session, "launcher.addMapping", json!({ "source": "note:10:36", "target": "scene", "scene": 2 })).unwrap();
    run(&mut session, "launcher.addMapping", json!({ "source": "key:Space", "target": "stop", "track": 2 })).unwrap();
    let list = run(&mut session, "launcher.mappings", json!({})).unwrap();
    assert_eq!(
        list["mappings"],
        json!([
            { "source": "key:Q", "label": "Q", "target": "slot", "command": "clip.launch", "track": 1, "scene": 1, "broken": false },
            { "source": "note:10:36", "label": "Ch 10 Note 36", "target": "scene", "command": "scene.launch", "scene": 2, "broken": false },
            { "source": "key:Space", "label": "Space", "target": "stop", "command": "track.stop", "track": 2, "broken": false },
        ])
    );

    // A source mapped again moves to its new target; it stays one mapping.
    let moved =
        run(&mut session, "launcher.addMapping", json!({ "source": "key:Q", "target": "scene", "scene": 1 })).unwrap();
    assert_eq!(moved["replaced"], "slot");
    assert_eq!(session.project().launcher.mappings.len(), 3);
    assert_eq!(session.project().launcher.mappings[0].target, MappingTarget::Scene { scene: 0 });

    // Removing by source, and every mapping of a target.
    run(&mut session, "launcher.addMapping", json!({ "source": "key:W", "target": "stop", "track": 2 })).unwrap();
    let removed = run(&mut session, "launcher.removeMapping", json!({ "target": "stop", "track": "Bass" })).unwrap();
    assert_eq!(removed["removed"].as_array().unwrap().len(), 2);
    run(&mut session, "launcher.removeMapping", json!({ "source": "note:10:36" })).unwrap();
    assert_eq!(session.project().launcher.mappings.len(), 1);

    // One undo step each.
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().launcher.mappings.len(), 4);
    for _ in 0..5 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert!(session.project().launcher.mappings.is_empty());
    assert!(!session.can_undo());
}

#[test]
fn malformed_mappings_are_errors_and_change_nothing() {
    let mut session = Session::new(project());
    run(&mut session, "launcher.addMapping", json!({ "source": "key:Q", "target": "scene", "scene": 1 })).unwrap();
    let before = session.project().clone();
    let bad = [
        ("launcher.addMapping", json!({ "source": "Q", "target": "scene", "scene": 1 })),
        ("launcher.addMapping", json!({ "source": "key:", "target": "scene", "scene": 1 })),
        ("launcher.addMapping", json!({ "source": "key:two words", "target": "scene", "scene": 1 })),
        ("launcher.addMapping", json!({ "source": "note:0:60", "target": "scene", "scene": 1 })),
        ("launcher.addMapping", json!({ "source": "note:1:128", "target": "scene", "scene": 1 })),
        ("launcher.addMapping", json!({ "source": 5, "target": "scene", "scene": 1 })),
        ("launcher.addMapping", json!({ "source": "key:W", "target": "pad", "scene": 1 })),
        ("launcher.addMapping", json!({ "source": "key:W" })),
        ("launcher.addMapping", json!({ "source": "key:W", "target": "scene" })),
        ("launcher.addMapping", json!({ "source": "key:W", "target": "scene", "scene": 3 })),
        ("launcher.addMapping", json!({ "source": "key:W", "target": "scene", "scene": 0 })),
        ("launcher.addMapping", json!({ "source": "key:W", "target": "scene", "scene": u64::MAX })),
        ("launcher.addMapping", json!({ "source": "key:W", "target": "slot", "scene": 1 })),
        ("launcher.addMapping", json!({ "source": "key:W", "target": "slot", "track": "Nope", "scene": 1 })),
        ("launcher.addMapping", json!({ "source": "key:W", "target": "stop" })),
        ("launcher.addMapping", json!({ "source": "key:W", "target": "stop", "track": 1, "extra": 1 })),
        ("launcher.removeMapping", json!({})),
        ("launcher.removeMapping", json!({ "source": "key:W" })),
        ("launcher.removeMapping", json!({ "source": "key:Q", "target": "scene", "scene": 1 })),
        ("launcher.removeMapping", json!({ "target": "scene", "scene": 2 })),
        ("launcher.removeMapping", json!({ "source": "bad" })),
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
    project.launcher.mappings = (0..MAX_MAPPINGS)
        .map(|i| LaunchMapping {
            source: MappingSource::note(1 + (i / 128) as u8, (i % 128) as u8).unwrap(),
            target: MappingTarget::Scene { scene: 0 },
        })
        .collect();
    session = Session::new(project);
    let err = run(&mut session, "launcher.addMapping", json!({ "source": "key:Q", "target": "scene", "scene": 1 }))
        .unwrap_err();
    assert!(err.contains("at most"), "{err}");
    // A source already mapped still moves.
    run(&mut session, "launcher.addMapping", json!({ "source": "note:1:0", "target": "scene", "scene": 2 })).unwrap();
}

#[test]
fn presses_and_releases_run_the_commands_the_mouse_runs() {
    let mut session = Session::new(project());
    let add = |session: &mut Session, params: Value| run(session, "launcher.addMapping", params).unwrap();
    add(&mut session, json!({ "source": "key:Q", "target": "slot", "track": 1, "scene": 1 }));
    add(&mut session, json!({ "source": "key:1", "target": "scene", "scene": 2 }));
    add(&mut session, json!({ "source": "note:1:36", "target": "stop", "track": 2 }));
    let p = session.project();
    assert_eq!(mapped_command(p, &key("Q"), true), Some(("clip.launch", json!({ "track": 1, "scene": 1 }))));
    // A trigger clip does not listen for its release.
    assert_eq!(mapped_command(p, &key("Q"), false), None);
    assert_eq!(mapped_command(p, &key("1"), true), Some(("scene.launch", json!({ "scene": 2 }))));
    assert_eq!(mapped_command(p, &key("1"), false), None);
    let note = MappingSource::note(1, 36).unwrap();
    assert_eq!(mapped_command(p, &note, true), Some(("track.stop", json!({ "track": 2 }))));
    assert_eq!(mapped_command(p, &note, false), None);
    // Another channel, another key: not mapped.
    assert_eq!(mapped_command(p, &MappingSource::note(2, 36).unwrap(), true), None);
    assert_eq!(mapped_command(p, &key("W"), true), None);

    run(&mut session, "clip.setLaunchMode", json!({ "track": 1, "scene": 1, "mode": "repeat" })).unwrap();
    assert_eq!(
        mapped_command(session.project(), &key("Q"), false),
        Some(("clip.release", json!({ "track": 1, "scene": 1 })))
    );
}

#[test]
fn a_mapping_whose_track_is_gone_is_kept_unused_and_undo_brings_it_back() {
    let mut session = Session::new(project());
    run(&mut session, "launcher.addMapping", json!({ "source": "key:Q", "target": "slot", "track": 2, "scene": 2 }))
        .unwrap();
    run(&mut session, "track.delete", json!({ "track": "Bass" })).unwrap();
    assert_eq!(session.project().launcher.mappings.len(), 1);
    assert_eq!(mapped_command(session.project(), &key("Q"), true), None);
    let list = run(&mut session, "launcher.mappings", json!({})).unwrap();
    assert_eq!(list["mappings"][0]["broken"], true);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(mapped_command(session.project(), &key("Q"), true).map(|(id, _)| id), Some("clip.launch"));
}

#[test]
fn a_held_key_plays_a_gate_clip_and_its_release_stops_it() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "clip.setLaunchMode", json!({ "track": "Drums", "scene": 1, "mode": "gate" })).unwrap();
    run(&mut session, "clip.setQuantization", json!({ "track": "Drums", "scene": 1, "value": "none" })).unwrap();
    run(&mut session, "launcher.addMapping", json!({ "source": "key:Q", "target": "slot", "track": 1, "scene": 1 }))
        .unwrap();
    run(&mut session, "launcher.addMapping", json!({ "source": "note:1:60", "target": "scene", "scene": 2 })).unwrap();
    run(&mut session, "launcher.addMapping", json!({ "source": "key:S", "target": "stop", "track": 2 })).unwrap();

    assert_eq!(press(&mut session, &key("Q"), true), Some("clip.launch"));
    advance(&mut renderer, BEAT);
    assert_eq!(state(&mut session)["tracks"][0]["playing"]["scene"], 1);
    assert_eq!(press(&mut session, &key("Q"), false), Some("clip.release"));
    advance(&mut renderer, 1);
    assert_eq!(state(&mut session)["tracks"][0]["playing"], Value::Null);

    // A note launches scene 2 (Bass plays its loop there), a key stops Bass.
    run(&mut session, "launcher.setQuantization", json!({ "value": "none" })).unwrap();
    assert_eq!(press(&mut session, &MappingSource::note(1, 60).unwrap(), true), Some("scene.launch"));
    advance(&mut renderer, BEAT);
    assert_eq!(state(&mut session)["tracks"][1]["playing"]["scene"], 2);
    assert_eq!(press(&mut session, &key("S"), true), Some("track.stop"));
    advance(&mut renderer, 1);
    assert_eq!(state(&mut session)["tracks"][1]["playing"], Value::Null);
}
