// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `clip.setFollowAction`: a slot clip's follow action, with undo, malformed parameters, and
//! playback through an offline renderer (`docs/spec/clip-launching.md` › Follow actions).
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use serde_json::json;
use suhina_engine::{AudioPool, Renderer};
use suhina_model::*;
use suhina_session::{Player, Session};

use crate::support::run;

const SR: u32 = 48_000;
const BEAT: usize = SR as usize / 2;

/// One track, "Drums", with a one-beat looping clip in scenes 1 and 2 and an empty scene 3.
fn project() -> Project {
    suhina_test_support::with_ids(project_built())
}

fn project_built() -> Project {
    let clip = |content: u32| Clip {
        loop_range: Some(LoopRange { start: 0.0, end: 1.0 }),
        ..Clip::new(ContentId(content), 0.0, 1.0)
    };
    let content = |id: u32, key: u8| ClipContent {
        id: ContentId(id),
        name: format!("Beat {id}"),
        length: 1.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![Note::new(0.0, 0.25, key, 1.0)]) }],
        extensions: Extensions::default(),
        scale: None,
    };
    Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Drums")
        }],
        channels: vec![Channel::new(ChannelId(1), "C1")],
        contents: vec![content(1, 36), content(2, 40)],
        launcher: Launcher {
            scenes: vec![Scene::default(); 3],
            columns: vec![LauncherColumn {
                track: TrackId(1),
                slots: vec![
                    Slot { clip: Some(clip(1)), has_stop: true },
                    Slot { clip: Some(clip(2)), has_stop: true },
                    Slot { clip: None, has_stop: true },
                ],
            }],
            mappings: Default::default(),
        },
        ..Project::default()
    }
}

fn follow_of(session: &Session, scene: usize) -> FollowAction {
    session.project().launcher.columns[0].slots[scene].clip.as_ref().unwrap().follow_action
}

#[test]
fn set_follow_action_changes_what_is_given_and_undoes_in_one_step() {
    let mut session = Session::new(project());
    let result = run(
        &mut session,
        "clip.setFollowAction",
        json!({ "track": "Drums", "scene": 1, "enabled": true, "actionA": "Next", "actionB": "jump 2", "chanceB": 25 }),
    )
    .unwrap();
    assert_eq!(result["scene"], 1);
    assert_eq!(result["follow_action"]["action_b"], "jump 2");
    let expected = FollowAction {
        enabled: true,
        action_a: FollowKind::Next,
        action_b: FollowKind::Jump(1),
        chance_b: 25,
        ..FollowAction::default()
    };
    assert_eq!(follow_of(&session, 0), expected);
    assert_eq!(follow_of(&session, 1), FollowAction::default(), "other slots are left alone");

    // Unlinked with a time of its own; the rest stays.
    run(&mut session, "clip.setFollowAction", json!({ "track": 1, "scene": 1, "linked": false, "time": 6.5 })).unwrap();
    assert_eq!(follow_of(&session, 0), FollowAction { linked: false, time: 6.5, ..expected });

    // The launcher listing shows it.
    let grid = run(&mut session, "project.launcher", json!({})).unwrap();
    let shown = &grid["tracks"][0]["slots"][0]["follow_action"];
    assert_eq!(shown["enabled"], true);
    assert_eq!(shown["time"], 6.5);
    assert_eq!(shown["action_a"], "next");
    assert_eq!(shown["chance_a"], 100);

    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(follow_of(&session, 0), expected);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(follow_of(&session, 0), FollowAction::default());
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(follow_of(&session, 0), expected);
}

#[test]
fn malformed_parameters_change_nothing() {
    let mut session = Session::new(project());
    let cases = [
        (json!({ "track": "Drums", "scene": 1 }), "give enabled"),
        (json!({ "track": "Drums", "scene": 3, "enabled": true }), "that slot has no clip"),
        (json!({ "track": "Drums", "scene": 4, "enabled": true }), "scene 4 does not exist"),
        (json!({ "track": "Drums", "scene": 0, "enabled": true }), "scene 0 does not exist"),
        (json!({ "track": "Nobody", "scene": 1, "enabled": true }), "Nobody"),
        (json!({ "track": "Drums", "scene": 1, "enabled": "yes" }), "enabled must be true or false"),
        (json!({ "track": "Drums", "scene": 1, "linked": 1 }), "linked must be true or false"),
        (json!({ "track": "Drums", "scene": 1, "time": 0 }), "time must be more than 0"),
        (json!({ "track": "Drums", "scene": 1, "time": -2 }), "time must be more than 0"),
        (json!({ "track": "Drums", "scene": 1, "time": 1e9 }), "at most 100000 beats"),
        (json!({ "track": "Drums", "scene": 1, "time": "4 bars" }), "time must be a number"),
        (json!({ "track": "Drums", "scene": 1, "actionA": "sideways" }), "actionA must be nothing, stop"),
        (json!({ "track": "Drums", "scene": 1, "actionB": 3 }), "actionB must be a text"),
        (json!({ "track": "Drums", "scene": 1, "actionA": "jump 4" }), "scene 4 does not exist"),
        (json!({ "track": "Drums", "scene": 1, "actionA": "jump 0" }), "actionA must be"),
        (json!({ "track": "Drums", "scene": 1, "actionA": "jump x" }), "actionA must be"),
        (json!({ "track": "Drums", "scene": 1, "chanceA": 101 }), "chanceA must be a whole number from 0 to 100"),
        (json!({ "track": "Drums", "scene": 1, "chanceB": -1 }), "chanceB must be a whole number"),
        (json!({ "track": "Drums", "scene": 1, "chanceB": 2.5 }), "chanceB must be a whole number"),
    ];
    for (params, expected) in cases {
        let err = run(&mut session, "clip.setFollowAction", params.clone()).unwrap_err();
        assert!(err.contains(expected), "{params}: {err}");
    }
    assert_eq!(session.project(), &project());
    assert!(run(&mut session, "edit.undo", json!({})).is_err(), "no undo step was recorded");
}

#[test]
fn a_follow_action_set_while_playing_takes_the_column_to_the_next_clip() {
    let project = project();
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let mut session = Session::new(project);
    session.attach_player(Player::new(renderer.controller(), None));
    run(&mut session, "clip.setFollowAction", json!({ "track": "Drums", "scene": 1, "enabled": true })).unwrap();
    run(&mut session, "clip.launch", json!({ "track": "Drums", "scene": 1 })).unwrap();
    let mut buffer = vec![0.0f32; 2 * 256];
    let mut advance = |frames: usize| {
        for _ in 0..frames / 256 {
            renderer.process(&mut buffer);
        }
    };
    advance(256);
    let monitor = session.player().unwrap().monitor();
    assert_eq!(monitor.column(0).unwrap().playing, Some(0));
    // One beat later scene 2 plays; it has no follow action, so it stays.
    advance(BEAT);
    assert_eq!(monitor.column(0).unwrap().playing, Some(1));
    advance(4 * BEAT);
    assert_eq!(monitor.column(0).unwrap().playing, Some(1));
}
