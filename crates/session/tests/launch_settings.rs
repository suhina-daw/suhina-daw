// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A slot clip's launch settings (`clip.setLaunchMode`, `clip.setLegato`, `clip.setQuantization`)
//! and the release of its launch button (`clip.release`), through a real renderer rendered
//! offline (`docs/spec/clip-launching.md` › Launch settings).
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use serde_json::{Value, json};
use suhina_engine::{AudioPool, Renderer};
use suhina_model::*;
use suhina_session::{Player, Session};

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

fn drums(session: &Session) -> LaunchSettings {
    session.project().launcher.columns[0].slots[0].clip.as_ref().unwrap().launch
}

fn state(session: &mut Session) -> Value {
    run(session, "launcher.state", json!({})).unwrap()
}

#[test]
fn launch_settings_are_edits_with_undo_and_show_in_the_grid() {
    let mut session = Session::new(project());
    let slot = |extra: Value| {
        let mut params = json!({ "track": "Drums", "scene": 1 });
        params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        params
    };
    let set = run(&mut session, "clip.setLaunchMode", slot(json!({ "mode": "gate" }))).unwrap();
    assert_eq!(set["mode"], "gate");
    let set = run(&mut session, "clip.setLegato", slot(json!({ "on": true }))).unwrap();
    assert_eq!(set["legato"], true);
    let set = run(&mut session, "clip.setQuantization", slot(json!({ "value": "1/8T" }))).unwrap();
    assert_eq!(set["quantization"], "1/8T");
    assert_eq!(
        drums(&session),
        LaunchSettings { mode: LaunchMode::Gate, legato: true, quantization: LaunchQuantization::Triplet(8) }
    );
    let grid = run(&mut session, "project.launcher", json!({})).unwrap();
    let shown = &grid["tracks"][0]["slots"][0];
    assert_eq!(
        (&shown["launch_mode"], &shown["legato"], &shown["quantization"]),
        (&json!("gate"), &json!(true), &json!("1/8T"))
    );
    assert_eq!(grid["tracks"][1]["slots"][1]["launch_mode"], "trigger");
    assert_eq!(grid["tracks"][1]["slots"][1]["quantization"], "global");

    // One undo step each.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(drums(&session).quantization, LaunchQuantization::Global);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(!drums(&session).legato);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(drums(&session), LaunchSettings::default());
    assert!(!session.can_undo());
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(drums(&session).mode, LaunchMode::Gate);
}

#[test]
fn malformed_launch_settings_are_errors_and_change_nothing() {
    let mut session = Session::new(project());
    let before = session.project().clone();
    let bad = [
        ("clip.setLaunchMode", json!({ "track": "Drums", "scene": 1, "mode": "hold" })),
        ("clip.setLaunchMode", json!({ "track": "Drums", "scene": 1, "mode": 2 })),
        ("clip.setLaunchMode", json!({ "track": "Drums", "scene": 1 })),
        ("clip.setLaunchMode", json!({ "track": "Drums", "scene": 1, "mode": "Gate" })),
        // An empty slot, a scene and a track that do not exist.
        ("clip.setLaunchMode", json!({ "track": "Drums", "scene": 2, "mode": "gate" })),
        ("clip.setLaunchMode", json!({ "track": "Drums", "scene": 0, "mode": "gate" })),
        ("clip.setLaunchMode", json!({ "track": "Drums", "scene": u64::MAX, "mode": "gate" })),
        ("clip.setLaunchMode", json!({ "track": "Nope", "scene": 1, "mode": "gate" })),
        ("clip.setLegato", json!({ "track": "Drums", "scene": 1, "on": "yes" })),
        ("clip.setLegato", json!({ "track": "Drums", "scene": 1, "on": 1 })),
        ("clip.setLegato", json!({ "track": "Drums", "scene": 1 })),
        ("clip.setLegato", json!({ "track": "Bass", "scene": 1, "on": true })),
        ("clip.setQuantization", json!({ "track": "Drums", "scene": 1, "value": "3 bars" })),
        ("clip.setQuantization", json!({ "track": "Drums", "scene": 1, "value": 4 })),
        ("clip.setQuantization", json!({ "track": "Drums", "scene": 1, "value": "" })),
        ("clip.setQuantization", json!({ "track": "Drums", "scene": 1, "value": "1/32T" })),
        ("clip.setQuantization", json!({ "track": "Drums", "scene": 1, "value": "1 bar", "extra": 1 })),
        ("clip.setQuantization", json!({ "track": "Drums", "scene": -1, "value": "1 bar" })),
    ];
    for (id, params) in bad {
        assert!(run(&mut session, id, params.clone()).is_err(), "{id} {params}");
    }
    assert_eq!(session.project(), &before);
    assert!(!session.can_undo());
    // Without a player the release is an error like any transport command.
    let err = run(&mut session, "clip.release", json!({ "track": "Drums", "scene": 1 })).unwrap_err();
    assert!(err.contains("no audio engine"), "{err}");
}

#[test]
fn a_gate_clip_plays_while_held() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "clip.setLaunchMode", json!({ "track": "Drums", "scene": 1, "mode": "gate" })).unwrap();
    run(&mut session, "clip.setQuantization", json!({ "track": "Drums", "scene": 1, "value": "1/4" })).unwrap();
    run(&mut session, "clip.launch", json!({ "track": "Drums", "scene": 1 })).unwrap();
    advance(&mut renderer, BEAT + BEAT / 2);
    assert_eq!(state(&mut session)["tracks"][0]["playing"]["scene"], 1);
    let released = run(&mut session, "clip.release", json!({ "track": "Drums", "scene": 1 })).unwrap();
    assert_eq!(released["released"], true);
    advance(&mut renderer, 1);
    assert_eq!(state(&mut session)["tracks"][0]["queued"], "stop");
    // The stop waits for the clip's own quarter-note grid, not the global bar.
    advance(&mut renderer, BEAT / 2);
    assert_eq!(state(&mut session)["tracks"][0]["playing"], Value::Null);
}

#[test]
fn a_toggle_clip_stops_on_the_second_launch() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "clip.setLaunchMode", json!({ "track": "Drums", "scene": 1, "mode": "toggle" })).unwrap();
    run(&mut session, "clip.setQuantization", json!({ "track": "Drums", "scene": 1, "value": "none" })).unwrap();
    run(&mut session, "clip.launch", json!({ "track": "Drums", "scene": 1 })).unwrap();
    advance(&mut renderer, BEAT);
    assert_eq!(state(&mut session)["tracks"][0]["playing"]["scene"], 1);
    run(&mut session, "clip.launch", json!({ "track": "Drums", "scene": 1 })).unwrap();
    advance(&mut renderer, 1);
    assert_eq!(state(&mut session)["tracks"][0]["playing"], Value::Null);
}

#[test]
fn a_mode_changed_while_held_is_heard_and_a_trigger_ignores_the_release() {
    let (mut session, mut renderer) = playing();
    run(&mut session, "launcher.setQuantization", json!({ "value": "none" })).unwrap();
    run(&mut session, "clip.launch", json!({ "track": "Drums", "scene": 1 })).unwrap();
    advance(&mut renderer, BEAT);
    // A trigger clip was never held: the release does nothing.
    run(&mut session, "clip.release", json!({ "track": "Drums", "scene": 1 })).unwrap();
    advance(&mut renderer, 1);
    assert_eq!(state(&mut session)["tracks"][0]["playing"]["scene"], 1);
    // Pressed as a gate clip, then edited while held (a new graph): the release still stops it.
    run(&mut session, "clip.setLaunchMode", json!({ "track": "Drums", "scene": 1, "mode": "gate" })).unwrap();
    run(&mut session, "clip.launch", json!({ "track": "Drums", "scene": 1 })).unwrap();
    advance(&mut renderer, BEAT);
    run(&mut session, "clip.setLegato", json!({ "track": "Drums", "scene": 1, "on": true })).unwrap();
    advance(&mut renderer, BEAT);
    run(&mut session, "clip.release", json!({ "track": "Drums", "scene": 1 })).unwrap();
    advance(&mut renderer, 1);
    assert_eq!(state(&mut session)["tracks"][0]["playing"], Value::Null);
}
