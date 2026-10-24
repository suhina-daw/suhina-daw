// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::{Value, json};
use suhina_model::*;
use suhina_session::{CommandKind, Session, commands};

use crate::support::run;
use suhina_test_support::ProjectBuilder;

fn project() -> Project {
    suhina_test_support::with_ids(project_built())
}

fn project_built() -> Project {
    ProjectBuilder::new()
        .midi_track(1, "Bass")
        .midi_track(2, "Lead")
        .notes(1, "Riff", 4.0, vec![Note::new(0.0, 1.0, 36, 1.0)])
        .clip(1, Clip::new(ContentId(1), 0.0, 4.0))
        .clip(1, Clip::new(ContentId(1), 8.0, 4.0))
        .build_without_ids()
}

#[test]
fn every_command_is_listed_with_a_unique_id() {
    let mut session = Session::new(project());
    let listed = run(&mut session, "command.list", json!({})).unwrap();
    let ids: Vec<&str> = listed.as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), commands().len());
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len());
    assert!(ids.iter().all(|id| id.contains('.')));
}

#[test]
fn reads_project_tracks_and_clips() {
    let mut session = Session::new(project());
    let info = run(&mut session, "project.info", json!({})).unwrap();
    assert_eq!(info["tracks"], 2);
    assert_eq!(info["arrangement_clips"], 2);
    assert_eq!(info["can_undo"], false);

    let tracks = run(&mut session, "project.tracks", json!({})).unwrap();
    assert_eq!(tracks[0]["name"], "Bass");
    assert_eq!(tracks[0]["volume_db"], 0.0);

    let clips = run(&mut session, "project.clips", json!({ "track": "bass" })).unwrap();
    assert_eq!(clips.as_array().unwrap().len(), 2);
    assert_eq!(clips[1]["start"], 8.0);
    assert_eq!(clips[1]["notes"], 1);
}

#[test]
fn edits_record_one_undo_step_each() {
    let mut session = Session::new(project());
    run(&mut session, "transport.setTempo", json!({ "bpm": 128 })).unwrap();
    run(&mut session, "track.setVolume", json!({ "tracks": ["Lead"], "db": -6 })).unwrap();
    run(&mut session, "track.setPan", json!({ "tracks": [2], "pan": -1 })).unwrap();
    run(&mut session, "track.setMute", json!({ "tracks": ["Bass"], "mute": true })).unwrap();
    run(&mut session, "clip.move", json!({ "track": "Bass", "index": 1, "start": 4 })).unwrap();

    let p = session.project();
    assert_eq!(p.transport.tempo_bpm, 128.0);
    assert!((p.channels[1].volume - 0.501).abs() < 1e-3);
    assert_eq!(p.channels[1].pan, -1.0);
    assert!(p.channels[0].mute);
    assert_eq!(p.arrangement.lanes[0].clips[1].start, 4.0);

    for _ in 0..5 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert_eq!(session.project(), &project());
    assert!(run(&mut session, "edit.undo", json!({})).is_err());

    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(session.project().transport.tempo_bpm, 128.0);
}

#[test]
fn a_new_edit_clears_redo_and_no_op_edits_record_nothing() {
    let mut session = Session::new(project());
    run(&mut session, "transport.setTempo", json!({ "bpm": 100 })).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "transport.setTempo", json!({ "bpm": 90 })).unwrap();
    assert!(run(&mut session, "edit.redo", json!({})).is_err());

    // Setting the same value again changes nothing, so there is nothing new to undo.
    run(&mut session, "transport.setTempo", json!({ "bpm": 90 })).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().transport.tempo_bpm, 120.0);
}

/// Pans `track` as a drag does: the first value without `continue`, the rest with it.
fn drag_pan(session: &mut Session, track: Value, values: &[f64]) {
    for (i, pan) in values.iter().enumerate() {
        let params = json!({ "tracks": [track], "pan": pan, "continue": i > 0 });
        let result = run(session, "track.setPan", params);
        assert!(result.is_ok(), "{result:?}");
    }
}

#[test]
fn the_calls_of_one_drag_make_one_undo_step() {
    let mut session = Session::new(project());
    drag_pan(&mut session, json!(1), &[0.1, 0.2, 0.3, -0.4]);
    assert_eq!(session.project().channels[0].pan, -0.4);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &project());
    assert!(!session.can_undo());
    // Redo brings back where the drag ended.
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(session.project().channels[0].pan, -0.4);

    // A track named rather than numbered is the same track.
    let mut session = Session::new(project());
    run(&mut session, "track.setVolume", json!({ "tracks": [1], "db": -1 })).unwrap();
    run(&mut session, "track.setVolume", json!({ "tracks": ["Bass"], "db": -2, "continue": true })).unwrap();
    run(&mut session, "track.setVolume", json!({ "tracks": ["bass"], "db": "-inf", "continue": true })).unwrap();
    assert_eq!(session.project().channels[0].volume, 0.0);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &project());
}

#[test]
fn a_second_drag_or_anything_between_starts_a_new_step() {
    let mut session = Session::new(project());
    drag_pan(&mut session, json!(1), &[0.1, 0.2]);
    drag_pan(&mut session, json!(1), &[0.5, 0.6]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().channels[0].pan, 0.2);

    // Another track, another command, or another edit between: `continue` starts a new step.
    let mut session = Session::new(project());
    drag_pan(&mut session, json!(1), &[0.1]);
    run(&mut session, "track.setPan", json!({ "tracks": [2], "pan": 0.7, "continue": true })).unwrap();
    run(&mut session, "track.setVolume", json!({ "tracks": [2], "db": -3, "continue": true })).unwrap();
    run(&mut session, "track.setMute", json!({ "tracks": [1], "mute": true })).unwrap();
    run(&mut session, "track.setPan", json!({ "tracks": [1], "pan": 0.9, "continue": true })).unwrap();
    for pan in [0.1, 0.1, 0.1, 0.1, 0.0] {
        run(&mut session, "edit.undo", json!({})).unwrap();
        assert_eq!(session.project().channels[0].pan, pan);
    }
    assert_eq!(session.project(), &project());

    // Undo ends the step too: the next call starts its own.
    let mut session = Session::new(project());
    run(&mut session, "transport.setTempo", json!({ "bpm": 100 })).unwrap();
    drag_pan(&mut session, json!(1), &[0.1, 0.2]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "track.setPan", json!({ "tracks": [1], "pan": 0.3, "continue": true })).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().channels[0].pan, 0.0);
    assert_eq!(session.project().transport.tempo_bpm, 100.0);
}

#[test]
fn a_drag_that_starts_without_a_change_does_not_join_the_last_one() {
    // The first frame of a drag often sends the value as it is: no step is made, but the
    // drag before it is still over.
    let mut session = Session::new(project());
    drag_pan(&mut session, json!(1), &[0.1, 0.2]);
    drag_pan(&mut session, json!(1), &[0.2, 0.5]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().channels[0].pan, 0.2);
}

#[test]
fn volume_accepts_silence() {
    let mut session = Session::new(project());
    let result = run(&mut session, "track.setVolume", json!({ "tracks": ["Bass"], "db": "-inf" })).unwrap();
    assert_eq!(result["volume_db"], json!([Value::Null]));
    assert_eq!(session.project().channels[0].volume, 0.0);
}

/// Every editing command, given hostile parameters, returns an error and leaves the project as
/// it was.
#[test]
fn bad_parameters_never_change_the_project() {
    let hostile = [
        json!(null),
        json!({}),
        json!({ "track": "nope" }),
        json!({ "track": 99, "bpm": 120, "db": 0, "pan": 0, "mute": true, "index": 0, "start": 0 }),
        json!({ "track": -1 }),
        json!({ "track": 1.5 }),
        json!({ "track": "Bass", "bpm": "fast", "db": 99, "pan": 2, "mute": "yes", "index": 7, "start": -1 }),
        json!({ "track": "Bass", "bpm": 1e308, "db": f64::MAX, "pan": -2, "mute": 1, "index": -1, "start": f64::MAX }),
        json!({ "track": [1], "index": u64::MAX }),
    ];
    for spec in commands().iter().filter(|c| c.kind == CommandKind::Edit) {
        for params in &hostile {
            // Keep only this command's own keys, so the values themselves are what gets tested.
            let params = match params.as_object() {
                Some(object) => Value::Object(
                    object
                        .iter()
                        .filter(|(k, _)| spec.params.iter().any(|p| p.name == k.as_str()))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                ),
                None => params.clone(),
            };
            let mut session = Session::new(project());
            let result = session.execute(spec.id, &params);
            if let Err(e) = result {
                assert!(!e.message().to_owned().is_empty());
                assert_eq!(session.project(), &project(), "{} with {params} changed the project", spec.id);
            }
        }
    }
}

#[test]
fn unknown_commands_and_non_object_params_are_errors() {
    let mut session = Session::new(project());
    let err = run(&mut session, "track.setVolumee", json!({})).unwrap_err();
    assert!(err.contains("track.setVolume"), "{err}");
    assert!(run(&mut session, "nope", json!({})).is_err());
    assert!(run(&mut session, "project.info", json!([1, 2])).is_err());
    let err = run(&mut session, "transport.setTempo", json!({ "bpm": 100, "temp": 1 })).unwrap_err();
    assert!(err.contains("\"temp\""), "{err}");
    assert_eq!(session.project().transport.tempo_bpm, 120.0);
}

#[test]
fn a_clip_moves_to_another_track_that_takes_its_kind() {
    let mut session = Session::new(project());
    let moved =
        run(&mut session, "clip.move", json!({ "track": "Bass", "index": 1, "start": 2, "toTrack": "Lead" })).unwrap();
    assert_eq!(moved["track"], 2);
    let p = session.project();
    assert_eq!(p.arrangement.lanes[0].clips.len(), 1);
    let lead = p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(2))).unwrap();
    assert!(lead.clips.iter().any(|c| c.start == 2.0));
    assert_eq!(moved["index"], lead.clips.iter().position(|c| c.start == 2.0).unwrap());

    run(&mut session, "edit.undo", json!({})).unwrap();
    // Undo takes the edit back; the id counter stays where it got to (ADR 0025).
    assert_eq!(session.project(), &Project { next_id: session.project().next_id, ..project() });
    assert!(
        run(&mut session, "clip.move", json!({ "track": "Bass", "index": 0, "start": 0, "toTrack": "Nobody" }))
            .is_err()
    );
    assert!(run(&mut session, "clip.move", json!({ "track": "Bass", "index": 9, "start": 0, "toTrack": 2 })).is_err());
}

#[test]
fn solo_is_an_edit_and_can_be_exclusive() {
    let mut session = Session::new(project());
    run(&mut session, "track.setSolo", json!({ "tracks": ["Bass"], "solo": true })).unwrap();
    run(&mut session, "track.setSolo", json!({ "tracks": ["Lead"], "solo": true })).unwrap();
    let solos = |s: &Session| s.project().channels.iter().map(|c| c.solo).collect::<Vec<_>>();
    assert_eq!(solos(&session), [true, true]);

    run(&mut session, "track.setSolo", json!({ "tracks": ["Lead"], "solo": true, "exclusive": true })).unwrap();
    assert_eq!(solos(&session), [false, true]);

    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(solos(&session), [true, true]);
    assert!(run(&mut session, "track.setSolo", json!({ "tracks": ["Bass"], "solo": "yes" })).is_err());
}

#[test]
fn a_selection_moves_in_proportion_and_mutes_together() {
    let volume = |s: &Session| s.project().channels.iter().map(|c| c.volume).collect::<Vec<_>>();
    let mut session = Session::new(project());
    run(&mut session, "track.setVolume", json!({ "tracks": ["Bass"], "db": -6.0 })).unwrap();
    assert_eq!(
        run(&mut session, "track.setVolume", json!({ "tracks": ["Bass", "Lead"], "db": 0.0 })).unwrap()["volume_db"],
        json!([0.0, 0.0])
    );

    // Both are set to the same level, then moved by the same number of dB: what they had between
    // them stays.
    run(&mut session, "track.setVolume", json!({ "tracks": ["Bass"], "db": -6.0 })).unwrap();
    let moved = run(&mut session, "track.adjustVolume", json!({ "tracks": ["Bass", "Lead"], "db": 3.0 })).unwrap();
    assert_eq!(moved["volume_db"], json!([-3.0, 3.0]));
    assert_eq!(moved["tracks"], json!([1, 2]));
    assert!((volume(&session)[0] - 10f64.powf(-3.0 / 20.0)).abs() < 1e-9);

    // Pan moves the same way and stops at full left and right.
    run(&mut session, "track.setPan", json!({ "tracks": ["Bass"], "pan": -0.9 })).unwrap();
    run(&mut session, "track.setPan", json!({ "tracks": ["Lead"], "pan": 0.5 })).unwrap();
    let panned = run(&mut session, "track.adjustPan", json!({ "tracks": ["Bass", "Lead"], "pan": -0.5 })).unwrap();
    assert_eq!(panned["pan"], json!([-1.0, 0.0]));
    assert!(run(&mut session, "track.adjustPan", json!({ "tracks": ["Bass"], "pan": 2.0 })).is_err());
    assert!(run(&mut session, "track.adjustVolume", json!({ "tracks": ["Bass"], "db": "loud" })).is_err());

    // Mute and solo take the whole list, as one undo step.
    run(&mut session, "track.setMute", json!({ "tracks": ["Bass", "Lead"], "mute": true })).unwrap();
    assert!(session.project().channels.iter().all(|c| c.mute));
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(session.project().channels.iter().all(|c| !c.mute));
    let soloed = run(&mut session, "track.setSolo", json!({ "tracks": ["Bass", "Lead"], "solo": true })).unwrap();
    assert_eq!(soloed["tracks"], json!([1, 2]));
    assert!(session.project().channels.iter().all(|c| c.solo));

    // A list that names nothing, or a track with no mixer channel, is refused.
    assert!(run(&mut session, "track.setMute", json!({ "tracks": [], "mute": true })).is_err());
    assert!(run(&mut session, "track.setMute", json!({ "tracks": ["Nope"], "mute": true })).is_err());
}

#[test]
fn a_track_routes_into_a_group_and_back_to_main() {
    let mut p = project();
    p.tracks.push(Track { id: TrackId(3), name: "Group".into(), channel: Some(ChannelId(3)), ..p.tracks[0].clone() });
    p.channels.push(Channel {
        id: ChannelId(3),
        name: "Group".into(),
        role: ChannelRole::Submix,
        ..p.channels[0].clone()
    });
    let mut session = Session::new(p);

    assert_eq!(run(&mut session, "track.setOutput", json!({ "track": "Bass", "to": "Group" })).unwrap()["to"], "Group");
    assert_eq!(session.project().channels[0].output, Some(ChannelId(3)));
    // Not into a track that is not a group, and not into a loop.
    assert!(run(&mut session, "track.setOutput", json!({ "track": "Bass", "to": "Lead" })).is_err());
    assert!(run(&mut session, "track.setOutput", json!({ "track": "Group", "to": "Group" })).is_err());

    run(&mut session, "track.setOutput", json!({ "track": "Bass", "to": "main" })).unwrap();
    assert_eq!(session.project().channels[0].output, None);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project().channels[0].output, Some(ChannelId(3)));
}

/// One lane with three one-beat-long clips at 0, 4 and 8 and a looping eight-beat clip at 12.
fn lane_project() -> Project {
    // Ids on its lanes, clips and scenes, as a session gives them.
    let mut project = lane_project_built();
    assert_eq!(project.assign_ids(), Ok(()));
    project
}

fn lane_project_built() -> Project {
    let mut p = project_built();
    let clip = |start: f64, length: f64| Clip { start, length, ..p.arrangement.lanes[0].clips[0].clone() };
    let mut looping = clip(12.0, 8.0);
    looping.loop_range = Some(LoopRange { start: 0.0, end: 4.0 });
    looping.offset = 0.0;
    p.arrangement.lanes[0].clips = vec![clip(0.0, 4.0), clip(4.0, 4.0), clip(8.0, 4.0), looping];
    p
}

fn spans(session: &Session) -> Vec<(f64, f64, f64)> {
    session.project().arrangement.lanes[0].clips.iter().map(|c| (c.start, c.length, c.offset)).collect()
}

#[test]
fn a_moved_clip_replaces_what_it_covers() {
    let mut session = Session::new(lane_project());
    // The clip at 0 moves to 6: the clip at 4 is shortened, the one at 8 starts later.
    let moved = run(&mut session, "clip.move", json!({ "track": "Bass", "index": 0, "start": 6 })).unwrap();
    assert_eq!(spans(&session)[..3], [(4.0, 2.0, 0.0), (6.0, 4.0, 0.0), (10.0, 2.0, 2.0)]);
    assert_eq!(moved["index"], 1);

    // Into the middle of the looping clip: it splits, the second part keeps the loop's phase.
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "clip.move", json!({ "track": "Bass", "index": 0, "start": 13 })).unwrap();
    let after: Vec<_> = spans(&session).into_iter().filter(|s| s.0 >= 12.0).collect();
    assert_eq!(after, [(12.0, 1.0, 0.0), (13.0, 4.0, 0.0), (17.0, 3.0, 1.0)]);

    // A clip entirely covered disappears.
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "clip.move", json!({ "track": "Bass", "index": 3, "start": 4 })).unwrap();
    assert_eq!(spans(&session), [(0.0, 4.0, 0.0), (4.0, 8.0, 0.0)]);
}

#[test]
fn a_stacked_clip_leaves_the_others_alone() {
    let mut session = Session::new(lane_project());
    run(&mut session, "clip.move", json!({ "track": "Bass", "index": 0, "start": 6, "stack": true })).unwrap();
    assert_eq!(spans(&session).len(), 4);
    assert!(spans(&session).contains(&(4.0, 4.0, 0.0)));
    assert!(spans(&session).contains(&(6.0, 4.0, 0.0)));
    assert!(
        run(&mut session, "clip.move", json!({ "track": "Bass", "index": 0, "start": 1, "stack": "yes" })).is_err()
    );
}

#[test]
fn a_stacked_paste_or_duplicate_leaves_the_others_alone() {
    // Without stack, a pasted copy of the clip at 0 placed at 6 shortens the clip at 4.
    let mut session = Session::new(lane_project());
    run(&mut session, "clip.copy", json!({ "clips": [{ "track": "Bass", "index": 0 }] })).unwrap();
    run(&mut session, "clip.paste", json!({ "track": "Bass", "start": 6 })).unwrap();
    assert!(spans(&session).contains(&(4.0, 2.0, 0.0)), "{:?}", spans(&session));

    // With stack, every clip keeps its span and the copy lies over them.
    let mut session = Session::new(lane_project());
    let before = spans(&session);
    run(&mut session, "clip.copy", json!({ "clips": [{ "track": "Bass", "index": 0 }] })).unwrap();
    run(&mut session, "clip.paste", json!({ "track": "Bass", "start": 6, "stack": true })).unwrap();
    assert_eq!(spans(&session).len(), before.len() + 1);
    assert!(before.iter().all(|s| spans(&session).contains(s)), "{:?}", spans(&session));
    assert!(spans(&session).contains(&(6.0, 4.0, 0.0)));
    assert!(run(&mut session, "clip.paste", json!({ "track": "Bass", "start": 6, "stack": 1 })).is_err());

    // A duplicate replaces the clip after it unless stacked.
    let mut session = Session::new(lane_project());
    run(&mut session, "clip.duplicate", json!({ "clips": [{ "track": "Bass", "index": 0 }] })).unwrap();
    assert_eq!(spans(&session).len(), 4, "the copy replaces the clip at 4");
    let mut session = Session::new(lane_project());
    let before = spans(&session);
    let copy =
        run(&mut session, "clip.duplicate", json!({ "clips": [{ "track": "Bass", "index": 0 }], "stack": true }))
            .unwrap();
    assert_eq!(copy["clips"][0]["start"], 4.0);
    assert_eq!(spans(&session).len(), before.len() + 1);
    assert!(before.iter().all(|s| spans(&session).contains(s)), "{:?}", spans(&session));
    assert!(
        run(&mut session, "clip.duplicate", json!({ "clips": [{ "track": "Bass", "index": 0 }], "stack": "yes" }))
            .is_err()
    );
}

fn order(session: &Session) -> Vec<String> {
    session.project().tracks.iter().map(|t| t.name.clone()).collect()
}

#[test]
fn tracks_group_ungroup_and_move() {
    let mut session = Session::new(project());
    let group = run(&mut session, "track.group", json!({ "tracks": ["Lead", 1], "name": "Band" })).unwrap();
    let id = TrackId(u32::try_from(group["track"].as_u64().unwrap()).unwrap());
    assert_eq!(order(&session), ["Band", "Bass", "Lead"]);
    let p = session.project();
    let group_channel = p.tracks[0].channel.unwrap();
    assert!(p.tracks[1..].iter().all(|t| t.parent == Some(id)));
    assert!(p.channels.iter().filter(|c| c.id != group_channel).all(|c| c.output == Some(group_channel)));
    assert_eq!(p.validate(), Ok(()));

    // A group moves with its tracks.
    run(&mut session, "track.move", json!({ "track": "Lead", "to": 0 })).unwrap();
    assert_eq!(order(&session), ["Lead", "Band", "Bass"]);
    run(&mut session, "track.move", json!({ "track": "Band", "to": 9 })).unwrap();
    assert_eq!(order(&session), ["Lead", "Band", "Bass"]);

    run(&mut session, "track.ungroup", json!({ "track": "Band" })).unwrap();
    assert_eq!(order(&session), ["Lead", "Bass"]);
    assert!(session.project().tracks.iter().all(|t| t.parent.is_none()));
    assert!(session.project().channels.iter().all(|c| c.output.is_none()));
    assert!(run(&mut session, "track.ungroup", json!({ "track": "Bass" })).is_err());

    // The second move changed nothing, so three steps undo it all.
    for _ in 0..3 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert_eq!(session.project(), &project());
}

fn names(session: &Session, ids: Vec<TrackId>) -> Vec<String> {
    ids.iter().filter_map(|id| session.project().track(*id)).map(|t| t.name.clone()).collect()
}

fn parent_of(session: &Session, name: &str) -> Option<String> {
    let p = session.project();
    let parent = p.tracks.iter().find(|t| t.name == name)?.parent?;
    p.track(parent).map(|t| t.name.clone())
}

fn output_of(session: &Session, name: &str) -> Option<String> {
    let p = session.project();
    let channel = p.tracks.iter().find(|t| t.name == name)?.channel?;
    let output = p.channel(channel)?.output?;
    p.tracks.iter().find(|t| t.channel == Some(output)).map(|t| t.name.clone())
}

#[test]
fn groups_nest_and_tracks_move_between_them() {
    let mut session = Session::new(project());
    run(&mut session, "track.group", json!({ "tracks": ["Bass"], "name": "Low" })).unwrap();
    run(&mut session, "track.group", json!({ "tracks": ["Low"], "name": "Band" })).unwrap();
    assert_eq!(order(&session), ["Band", "Low", "Bass", "Lead"]);
    assert_eq!(parent_of(&session, "Low").as_deref(), Some("Band"));
    assert_eq!(output_of(&session, "Low").as_deref(), Some("Band"));
    assert_eq!(output_of(&session, "Bass").as_deref(), Some("Low"));
    let bass = session.project().tracks[2].id;
    assert_eq!(names(&session, session.project().ancestors(bass)), ["Low", "Band"]);

    // Into the inner group, at the end, then at its start.
    let moved = run(&mut session, "track.setGroup", json!({ "track": "Lead", "group": "Low" })).unwrap();
    assert_eq!(moved["position"], 3);
    assert_eq!(order(&session), ["Band", "Low", "Bass", "Lead"]);
    assert_eq!(parent_of(&session, "Lead").as_deref(), Some("Low"));
    assert_eq!(output_of(&session, "Lead").as_deref(), Some("Low"));
    run(&mut session, "track.setGroup", json!({ "track": "Lead", "group": "Low", "index": 0 })).unwrap();
    assert_eq!(order(&session), ["Band", "Low", "Lead", "Bass"]);

    // A group moves with everything inside it: "Low" out to the top level, after "Band".
    run(&mut session, "track.setGroup", json!({ "track": "Low", "group": "none" })).unwrap();
    assert_eq!(order(&session), ["Band", "Low", "Lead", "Bass"]);
    assert_eq!(parent_of(&session, "Low"), None);
    assert_eq!(output_of(&session, "Low"), None);
    assert_eq!(output_of(&session, "Bass").as_deref(), Some("Low"));
    // And back in, first at the top level, then into "Band" (empty now) by its id.
    run(&mut session, "track.setGroup", json!({ "track": "Low", "group": "none", "index": 0 })).unwrap();
    assert_eq!(order(&session), ["Low", "Lead", "Bass", "Band"]);
    let band = session.project().tracks[3].id.0;
    run(&mut session, "track.setGroup", json!({ "track": "Low", "group": band })).unwrap();
    assert_eq!(order(&session), ["Band", "Low", "Lead", "Bass"]);
    assert_eq!(session.project().validate(), Ok(()));

    // Every step undoes, back to the project it began with.
    for _ in 0..7 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert_eq!(session.project(), &project());
}

#[test]
fn moving_into_a_group_refuses_loops_non_groups_and_bad_params() {
    let mut session = Session::new(project());
    run(&mut session, "track.group", json!({ "tracks": ["Bass"], "name": "Low" })).unwrap();
    run(&mut session, "track.group", json!({ "tracks": ["Low"], "name": "Band" })).unwrap();
    let before = session.project().clone();
    for (params, why) in [
        (json!({ "track": "Band", "group": "Low" }), "inside the track"),
        (json!({ "track": "Band", "group": "Band" }), "inside the track"),
        (json!({ "track": "Lead", "group": "Bass" }), "not a group"),
        (json!({ "track": "Lead", "group": "Nowhere" }), "Nowhere"),
        (json!({ "track": "Lead", "group": 99 }), "does not exist"),
        (json!({ "track": "Lead", "group": -1 }), "whole number"),
        (json!({ "track": "Lead", "group": true }), ""),
        (json!({ "track": "Lead" }), "group must be"),
        (json!({ "track": "Lead", "group": "Low", "index": -1 }), "index"),
        (json!({ "track": "Lead", "group": "Low", "index": 1.5 }), "index"),
        (json!({ "track": "Lead", "group": "Low", "index": "first" }), "index"),
        (json!({ "group": "Low" }), ""),
    ] {
        let error = run(&mut session, "track.setGroup", params.clone()).unwrap_err();
        assert!(error.contains(why), "{params}: {error}");
        assert_eq!(session.project(), &before, "{params}");
    }
    // An index past the end is the end.
    run(&mut session, "track.setGroup", json!({ "track": "Lead", "group": "Band", "index": 99 })).unwrap();
    assert_eq!(order(&session), ["Band", "Low", "Bass", "Lead"]);
}

#[test]
fn groups_nest_up_to_the_cap() {
    let mut session = Session::new(project());
    for level in 0..MAX_GROUP_DEPTH {
        let inner = if level == 0 { "Bass".to_string() } else { format!("G{}", level - 1) };
        run(&mut session, "track.group", json!({ "tracks": [inner], "name": format!("G{level}") })).unwrap();
    }
    let bass = session.project().tracks.iter().find(|t| t.name == "Bass").unwrap().id;
    assert_eq!(session.project().group_depth(bass), Some(MAX_GROUP_DEPTH));
    let deeper = run(&mut session, "track.group", json!({ "tracks": ["Bass"] })).unwrap_err();
    assert!(deeper.contains("nest at most"), "{deeper}");
    // "Lead" fits into the innermost group, "G0", a group holding a track does not.
    let deepest = "G0";
    run(&mut session, "track.group", json!({ "tracks": ["Lead"], "name": "Pair" })).unwrap();
    let error = run(&mut session, "track.setGroup", json!({ "track": "Pair", "group": deepest })).unwrap_err();
    assert!(error.contains("nest at most"), "{error}");
    run(&mut session, "track.setGroup", json!({ "track": "Lead", "group": deepest })).unwrap();
    assert_eq!(session.project().validate(), Ok(()));
}

#[test]
fn a_return_track_stays_out_of_groups() {
    let mut p = project();
    p.channels[1].role = ChannelRole::Effect;
    let mut session = Session::new(p);
    run(&mut session, "track.group", json!({ "tracks": ["Bass"], "name": "Low" })).unwrap();
    let error = run(&mut session, "track.setGroup", json!({ "track": "Lead", "group": "Low" })).unwrap_err();
    assert!(error.contains("return"), "{error}");
}

#[test]
fn tracks_rename_colour_and_delete() {
    let mut session = Session::new(project());
    run(&mut session, "track.rename", json!({ "track": "Lead", "name": "Keys" })).unwrap();
    run(&mut session, "track.setColor", json!({ "track": "Keys", "color": "#D08F6E" })).unwrap();
    assert_eq!(session.project().tracks[1].color, Some(Rgb(0xD0, 0x8F, 0x6E)));
    assert_eq!(session.project().channels[1].name, "Keys");
    assert!(run(&mut session, "track.rename", json!({ "track": "Keys", "name": "  " })).is_err());
    assert!(run(&mut session, "track.setColor", json!({ "track": "Keys", "color": "red" })).is_err());

    run(&mut session, "track.delete", json!({ "track": "Bass" })).unwrap();
    assert_eq!(order(&session), ["Keys"]);
    assert!(session.project().arrangement.lanes.iter().all(|l| l.track != Some(TrackId(1))));
    assert_eq!(session.project().validate(), Ok(()));
}

#[test]
fn clips_delete_and_duplicate() {
    let mut session = Session::new(project());
    let copy = run(&mut session, "clip.duplicate", json!({ "clips": [{ "track": "Bass", "index": 0 }] })).unwrap();
    assert_eq!(copy["clips"][0]["start"], 4.0);
    let starts: Vec<f64> = session.project().arrangement.lanes[0].clips.iter().map(|c| c.start).collect();
    assert_eq!(starts, [0.0, 4.0, 8.0]);
    run(&mut session, "clip.delete", json!({ "clips": [{ "track": "Bass", "index": 1 }] })).unwrap();
    let starts: Vec<f64> = session.project().arrangement.lanes[0].clips.iter().map(|c| c.start).collect();
    assert_eq!(starts, [0.0, 8.0]);
    assert!(run(&mut session, "clip.delete", json!({ "clips": [{ "track": "Bass", "index": 5 }] })).is_err());
}

/// The clips named by `indexes` on Bass, as `clip.*` commands take them.
fn bass_clips(indexes: &[u64]) -> Value {
    json!({ "clips": indexes.iter().map(|index| json!({ "track": "Bass", "index": index })).collect::<Vec<Value>>() })
}

/// The start and the on/off state of every clip on Bass.
fn bass_spans(session: &Session) -> Vec<(f64, bool)> {
    session.project().arrangement.lanes[0].clips.iter().map(|c| (c.start, c.enabled)).collect()
}

#[test]
fn several_clips_mute_and_delete_as_one_undo_step() {
    let mut session = Session::new(project());
    assert_eq!(bass_spans(&session), [(0.0, true), (8.0, true)]);

    // Both clips off together: one step, and one undo brings both back.
    let mut params = bass_clips(&[0, 1]);
    params["enabled"] = json!(false);
    run(&mut session, "clip.setEnabled", params).unwrap();
    assert_eq!(bass_spans(&session), [(0.0, false), (8.0, false)]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(bass_spans(&session), [(0.0, true), (8.0, true)]);

    // Both gone together, the indices still meaning the clips the project listed.
    run(&mut session, "clip.delete", bass_clips(&[0, 1])).unwrap();
    assert!(bass_spans(&session).is_empty());
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(bass_spans(&session).len(), 2);

    // A list that names nothing, a clip without an index, an unknown track and an index past the
    // end are all refused, and so is a list given with a single clip beside it.
    for bad in [
        json!({ "clips": [] }),
        json!({ "clips": [{ "track": "Bass" }] }),
        json!({ "clips": [{ "track": "Nope", "index": 0 }] }),
        json!({ "clips": [{ "track": "Bass", "index": 9 }] }),
        json!({ "clips": "Bass" }),
        json!({ "clips": [{ "track": "Bass", "index": 0 }], "track": "Bass", "index": 1 }),
    ] {
        assert!(run(&mut session, "clip.delete", bad.clone()).is_err(), "{bad}");
    }
    assert_eq!(bass_spans(&session).len(), 2, "a refused list changes nothing");

    // One clip by track and index still works, as the Session view's form by scene does.
    run(&mut session, "clip.delete", json!({ "track": "Bass", "index": 0 })).unwrap();
    assert_eq!(bass_spans(&session), [(8.0, true)]);
}

#[test]
fn copied_clips_paste_together_and_keep_their_places() {
    let mut session = Session::new(project());
    let copied = run(&mut session, "clip.copy", bass_clips(&[0, 1])).unwrap();
    assert_eq!(copied["copied"], json!(["Riff", "Riff"]));
    let pasted = run(&mut session, "clip.paste", json!({ "track": "Bass", "start": 16, "stack": true })).unwrap();
    assert_eq!(pasted["clips"].as_array().map(Vec::len), Some(2));
    let starts: Vec<f64> = session.project().arrangement.lanes[0].clips.iter().map(|c| c.start).collect();
    assert_eq!(
        starts,
        [0.0, 8.0, 16.0, 24.0],
        "the earliest copy lands on the start, the others keep their distance from it"
    );
    assert_eq!(session.project().validate(), Ok(()));
}

/// The test project with its second Bass clip playing an audio content.
fn audio_project() -> Project {
    // Ids on its lanes, clips and scenes, as a session gives them.
    let mut project = audio_project_built();
    assert_eq!(project.assign_ids(), Ok(()));
    project
}

fn audio_project_built() -> Project {
    let mut p = project_built();
    p.contents.push(ClipContent {
        id: ContentId(9),
        name: "Loop".into(),
        length: 4.0,
        lanes: vec![ContentLane { target: None, data: LaneData::Audio(AudioRegion::new("loop.wav")) }],
        extensions: Extensions::default(),
        scale: None,
    });
    p.arrangement.lanes[0].clips[1].content = ContentId(9);
    p
}

#[test]
fn an_audio_clip_takes_a_gain_and_a_midi_clip_does_not() {
    let mut session = Session::new(audio_project());
    run(&mut session, "clip.setGain", json!({ "track": "Bass", "index": 1, "db": -6 })).unwrap();
    let LaneData::Audio(region) = &session.project().content(ContentId(9)).unwrap().lanes[0].data else { panic!() };
    assert!((region.gain - 0.501).abs() < 1e-3);
    assert!(run(&mut session, "clip.setGain", json!({ "track": "Bass", "index": 0, "db": 0 })).is_err());
    assert!(run(&mut session, "clip.setGain", json!({ "track": "Bass", "index": 1, "db": 30 })).is_err());
}

#[test]
fn an_audio_clip_takes_fades_that_undo() {
    let mut session = Session::new(audio_project());
    let set = |session: &mut Session, params: Value| run(session, "clip.setFades", params);
    let fades = |session: &Session| {
        let clip = &session.project().arrangement.lanes[0].clips[1];
        (clip.fade_in, clip.fade_out)
    };
    let done = set(&mut session, json!({ "track": "Bass", "index": 1, "in": 1.5 })).unwrap();
    assert_eq!(
        done,
        json!({ "fade_in": 1.5, "fade_out": 0.0, "fade_in_shape": "linear", "fade_in_curve": 0.0,
                "fade_out_shape": "linear", "fade_out_curve": 0.0 })
    );
    set(&mut session, json!({ "track": "Bass", "index": 1, "out": 0.25 })).unwrap();
    assert_eq!(fades(&session), (1.5, 0.25), "out alone keeps the fade-in");
    set(&mut session, json!({ "track": "Bass", "index": 1, "in": 2, "out": 2 })).unwrap();
    assert_eq!(fades(&session), (2.0, 2.0), "together exactly the clip's 4 beats");
    let listed = run(&mut session, "project.clips", json!({ "track": "Bass" })).unwrap();
    assert_eq!((listed[1]["fade_in"].clone(), listed[1]["fade_out"].clone()), (json!(2.0), json!(2.0)));

    // Each is one undo step.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(fades(&session), (1.5, 0.25));
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &audio_project());
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(fades(&session), (1.5, 0.0));
}

/// Shapes and curves are set alone or with the lengths, listed by project.clips, and undone
/// like any edit (#197).
#[test]
fn fade_shapes_and_curves_are_set_and_undone() {
    let mut session = Session::new(audio_project());
    let set = |session: &mut Session, params: Value| run(session, "clip.setFades", params);
    let curves = |session: &Session| {
        let clip = &session.project().arrangement.lanes[0].clips[1];
        (clip.fade_in_curve, clip.fade_out_curve)
    };
    set(&mut session, json!({ "track": "Bass", "index": 1, "in": 1, "inShape": "equal_power" })).unwrap();
    set(&mut session, json!({ "track": "Bass", "index": 1, "outShape": "s_curve", "outCurve": -0.5 })).unwrap();
    set(&mut session, json!({ "track": "Bass", "index": 1, "inCurve": 1 })).unwrap();
    assert_eq!(
        curves(&session),
        (FadeCurve { shape: FadeShape::EqualPower, curve: 1.0 }, FadeCurve { shape: FadeShape::SCurve, curve: -0.5 })
    );
    let listed = run(&mut session, "project.clips", json!({ "track": "Bass" })).unwrap();
    assert_eq!(
        [
            &listed[1]["fade_in_shape"],
            &listed[1]["fade_in_curve"],
            &listed[1]["fade_out_shape"],
            &listed[1]["fade_out_curve"]
        ],
        [&json!("equal_power"), &json!(1.0), &json!("s_curve"), &json!(-0.5)]
    );
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(curves(&session).0.curve, 0.0, "one undo step per call");
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &audio_project());
}

#[test]
fn bad_fades_are_errors_that_change_nothing() {
    let mut session = Session::new(audio_project());
    let before = session.project().clone();
    for params in [
        json!({ "track": "Bass", "index": 1 }),
        json!({ "track": "Bass", "index": 1, "in": -0.5 }),
        json!({ "track": "Bass", "index": 1, "out": "long" }),
        json!({ "track": "Bass", "index": 1, "in": 3, "out": 1.5 }),
        json!({ "track": "Bass", "index": 1, "in": 5 }),
        json!({ "track": "Bass", "index": 7, "in": 1 }),
        json!({ "track": "Bass", "index": 0, "in": 1 }),
        json!({ "track": "Bass", "index": 1, "inShape": "wobbly" }),
        json!({ "track": "Bass", "index": 1, "outShape": 2 }),
        json!({ "track": "Bass", "index": 1, "inCurve": 1.5 }),
        json!({ "track": "Bass", "index": 1, "outCurve": -2 }),
        json!({ "track": "Bass", "index": 0, "inShape": "fast" }),
    ] {
        assert!(run(&mut session, "clip.setFades", params.clone()).is_err(), "{params}");
    }
    assert_eq!(session.project(), &before);
    assert!(run(&mut session, "edit.undo", json!({})).is_err(), "nothing was recorded");
}

/// The audio project with launcher slots on Bass: scene 1 the audio loop, scene 2 the notes
/// clip, scene 3 empty.
fn slot_project() -> Project {
    // Ids on its lanes, clips and scenes, as a session gives them.
    let mut project = slot_project_built();
    assert_eq!(project.assign_ids(), Ok(()));
    project
}

fn slot_project_built() -> Project {
    let mut p = audio_project_built();
    let lane = &p.arrangement.lanes[0];
    let (notes, audio) = (lane.clips[0].clone(), Clip { start: 0.0, ..lane.clips[1].clone() });
    p.launcher = Launcher {
        scenes: vec![Scene::default(); 3],
        columns: vec![LauncherColumn {
            track: TrackId(1),
            slots: vec![
                Slot { clip: Some(audio), has_stop: true },
                Slot { clip: Some(notes), has_stop: true },
                Slot { clip: None, has_stop: true },
            ],
        }],
        mappings: Default::default(),
    };
    p
}

/// A launcher clip's fades are set by its scene, leave the arrangement as it is, and undo (#104).
#[test]
fn a_launcher_clip_takes_fades_by_scene_that_undo() {
    let mut session = Session::new(slot_project());
    let slot_clip = |session: &Session| session.project().launcher.columns[0].slots[0].clip.clone().unwrap();
    let done = run(
        &mut session,
        "clip.setFades",
        json!({ "track": "Bass", "scene": 1, "in": 1, "out": 0.5, "inShape": "fast", "outCurve": 0.5 }),
    )
    .unwrap();
    assert_eq!(
        done,
        json!({ "fade_in": 1.0, "fade_out": 0.5, "fade_in_shape": "fast", "fade_in_curve": 0.0,
                "fade_out_shape": "linear", "fade_out_curve": 0.5 })
    );
    let clip = slot_clip(&session);
    assert_eq!((clip.fade_in, clip.fade_out), (1.0, 0.5));
    assert_eq!(clip.fade_in_curve.shape, FadeShape::Fast);
    assert_eq!(session.project().arrangement, slot_project().arrangement, "the arrangement is untouched");

    run(&mut session, "clip.setFades", json!({ "track": "Bass", "scene": 1, "in": 0 })).unwrap();
    assert_eq!((slot_clip(&session).fade_in, slot_clip(&session).fade_out), (0.0, 0.5));
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(slot_clip(&session).fade_in, 1.0);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &slot_project());
}

#[test]
fn bad_launcher_clip_fades_are_errors_that_change_nothing() {
    let mut session = Session::new(slot_project());
    for params in [
        // Both an arrangement index and a scene.
        json!({ "track": "Bass", "index": 1, "scene": 1, "in": 1 }),
        // Neither.
        json!({ "track": "Bass", "in": 1 }),
        // The notes clip, an empty slot, scenes that do not exist, a track without slots.
        json!({ "track": "Bass", "scene": 2, "in": 1 }),
        json!({ "track": "Bass", "scene": 3, "in": 1 }),
        json!({ "track": "Bass", "scene": 0, "in": 1 }),
        json!({ "track": "Bass", "scene": 4, "in": 1 }),
        json!({ "track": "Bass", "scene": -1, "in": 1 }),
        json!({ "track": "Bass", "scene": "one", "in": 1 }),
        json!({ "track": "Lead", "scene": 1, "in": 1 }),
        // Longer than the 4-beat clip, negative, no change asked.
        json!({ "track": "Bass", "scene": 1, "in": 3, "out": 2 }),
        json!({ "track": "Bass", "scene": 1, "out": -1 }),
        json!({ "track": "Bass", "scene": 1 }),
    ] {
        assert!(run(&mut session, "clip.setFades", params.clone()).is_err(), "{params}");
    }
    assert_eq!(session.project(), &slot_project());
    assert!(run(&mut session, "edit.undo", json!({})).is_err(), "nothing was recorded");
}

#[test]
fn clips_shortened_or_split_by_a_move_keep_fades_that_fit() {
    let mut p = lane_project();
    for clip in &mut p.arrangement.lanes[0].clips {
        (clip.fade_in, clip.fade_out) = (1.0, 1.0);
    }
    p.arrangement.lanes[0].clips[3].fade_in = 3.0;
    let mut session = Session::new(p);
    // The clip at 4 moves to 9 and covers 9..13: the clip at 8 is shortened to one beat, so its
    // 1 + 1 beats of fades become 0.5 + 0.5.
    run(&mut session, "clip.move", json!({ "track": "Bass", "index": 1, "start": 9 })).unwrap();
    let clips = &session.project().arrangement.lanes[0].clips;
    let fades: Vec<(f64, f64, f64, f64)> = clips.iter().map(|c| (c.start, c.length, c.fade_in, c.fade_out)).collect();
    assert!(fades.contains(&(8.0, 1.0, 0.5, 0.5)), "{fades:?}");
    assert_eq!(session.project().validate(), Ok(()));

    // Into the middle of the looping clip (12..20): the first part keeps the fade-in, the
    // second the fade-out.
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "clip.move", json!({ "track": "Bass", "index": 0, "start": 14 })).unwrap();
    let clips = &session.project().arrangement.lanes[0].clips;
    let fades: Vec<(f64, f64, f64, f64)> = clips.iter().map(|c| (c.start, c.length, c.fade_in, c.fade_out)).collect();
    assert!(fades.contains(&(12.0, 2.0, 2.0, 0.0)), "{fades:?}");
    assert!(fades.contains(&(18.0, 2.0, 0.0, 1.0)), "{fades:?}");
    assert_eq!(session.project().validate(), Ok(()));
}

#[test]
fn clips_copy_cut_and_paste_with_their_own_content() {
    let mut session = Session::new(project());
    assert!(run(&mut session, "clip.paste", json!({ "track": "Bass", "start": 0 })).is_err(), "nothing copied yet");
    run(&mut session, "clip.copy", json!({ "clips": [{ "track": "Bass", "index": 0 }] })).unwrap();
    let pasted = run(&mut session, "clip.paste", json!({ "track": "Lead", "start": 16 })).unwrap();
    let p = session.project();
    let lead = p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(2))).unwrap();
    let copy = &lead.clips[usize::try_from(pasted["clips"][0]["index"].as_u64().unwrap()).unwrap()];
    assert_eq!(copy.start, 16.0);
    assert_ne!(copy.content, ContentId(1), "the copy has its own content");
    assert_eq!(p.validate(), Ok(()));

    run(&mut session, "clip.cut", json!({ "clips": [{ "track": "Bass", "index": 1 }] })).unwrap();
    assert_eq!(session.project().arrangement.lanes[0].clips.len(), 1);
    run(&mut session, "clip.paste", json!({ "track": "Bass", "start": 20 })).unwrap();
    assert_eq!(session.project().arrangement.lanes[0].clips.len(), 2);
}

#[test]
fn markers_are_added_moved_renamed_and_deleted_in_beat_order() {
    let mut session = Session::new(project());
    let names = |s: &Session| -> Vec<(f64, String)> {
        s.project().arrangement.markers.iter().map(|m| (m.beat, m.name.clone())).collect()
    };
    let added = run(&mut session, "marker.add", json!({ "beat": 16, "name": " Chorus " })).unwrap();
    assert_eq!((added["index"].as_u64(), added["name"].as_str()), (Some(0), Some("Chorus")));
    let added = run(&mut session, "marker.add", json!({ "beat": 4 })).unwrap();
    assert_eq!((added["index"].as_u64(), added["name"].as_str()), (Some(0), Some("Marker 2")));
    let added = run(&mut session, "marker.add", json!({ "beat": 16 })).unwrap();
    assert_eq!((added["index"].as_u64(), added["name"].as_str()), (Some(2), Some("Marker 3")), "after Chorus");
    assert_eq!(run(&mut session, "project.info", json!({})).unwrap()["markers"], 3);

    // Moving keeps the order and says where the marker went.
    let moved = run(&mut session, "marker.move", json!({ "index": 0, "beat": 32 })).unwrap();
    assert_eq!(moved["index"], 2);
    run(&mut session, "marker.rename", json!({ "index": 0, "name": "Hook" })).unwrap();
    assert_eq!(
        names(&session),
        [(16.0, "Hook".to_string()), (16.0, "Marker 3".to_string()), (32.0, "Marker 2".to_string())]
    );
    let listed = run(&mut session, "project.markers", json!({})).unwrap();
    assert_eq!(listed[2], json!({ "index": 2, "beat": 32.0, "name": "Marker 2" }));

    // A default name skips one in use.
    run(&mut session, "marker.delete", json!({ "index": 1 })).unwrap();
    let added = run(&mut session, "marker.add", json!({ "beat": 0 })).unwrap();
    assert_eq!(added["name"], "Marker 3");

    // Each edit is one undo step.
    for _ in 0..7 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert_eq!(session.project(), &project());
}

#[test]
fn bad_marker_edits_are_errors_and_change_nothing() {
    let mut session = Session::new(project());
    run(&mut session, "marker.add", json!({ "beat": 4, "name": "Verse" })).unwrap();
    let before = session.project().clone();
    for (id, params) in [
        ("marker.add", json!({ "beat": -1 })),
        ("marker.add", json!({ "beat": 120_000.5 })),
        ("marker.add", json!({ "beat": "4" })),
        ("marker.add", json!({})),
        ("marker.add", json!({ "beat": 1, "name": "  " })),
        ("marker.add", json!({ "beat": 1, "name": 3 })),
        ("marker.move", json!({ "index": 1, "beat": 0 })),
        ("marker.move", json!({ "index": 0, "beat": -0.5 })),
        ("marker.move", json!({ "index": -1, "beat": 0 })),
        ("marker.move", json!({ "index": 0 })),
        ("marker.rename", json!({ "index": 0, "name": "" })),
        ("marker.rename", json!({ "index": 9, "name": "Nine" })),
        ("marker.delete", json!({ "index": 1 })),
        ("marker.delete", json!({ "index": 18_446_744_073_709_551_615u64 })),
        ("marker.delete", json!({})),
    ] {
        assert!(run(&mut session, id, params.clone()).is_err(), "{id} {params}");
    }
    assert_eq!(session.project(), &before);
}

#[test]
fn the_loop_region_is_set_and_turned_on_and_off_with_undo() {
    let mut session = Session::new(project());
    let looped = |s: &Session| (s.project().transport.loop_range, s.project().transport.loop_enabled);
    assert_eq!(run(&mut session, "project.info", json!({})).unwrap()["loop"], Value::Null);

    // Setting the region leaves looping as it is unless `enabled` says otherwise.
    let set = run(&mut session, "transport.setLoop", json!({ "start": 4, "end": 12 })).unwrap();
    assert_eq!(set, json!({ "start": 4.0, "end": 12.0, "enabled": false }));
    run(&mut session, "transport.setLoop", json!({ "start": 8, "end": 9.5, "enabled": true })).unwrap();
    assert_eq!(looped(&session), (Some(LoopRange { start: 8.0, end: 9.5 }), true));
    assert_eq!(run(&mut session, "project.info", json!({})).unwrap()["loop"]["end"], 9.5);

    // Without `enabled` it toggles; off keeps the region for the next time it is on.
    run(&mut session, "transport.setLoopEnabled", json!({})).unwrap();
    assert_eq!(looped(&session), (Some(LoopRange { start: 8.0, end: 9.5 }), false));
    run(&mut session, "transport.setLoopEnabled", json!({ "enabled": true })).unwrap();
    assert!(session.project().transport.loop_enabled);

    for _ in 0..4 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert_eq!(session.project(), &project());

    // On without a region makes one over the first 4 bars, in the project's time signature.
    let mut waltz = project();
    waltz.transport.time_signature = TimeSignature { numerator: 3, denominator: 4 };
    let mut session = Session::new(waltz);
    let on = run(&mut session, "transport.setLoopEnabled", json!({ "enabled": true })).unwrap();
    assert_eq!(on, json!({ "start": 0.0, "end": 12.0, "enabled": true }));
}

#[test]
fn bad_loop_edits_are_errors_and_change_nothing() {
    let mut session = Session::new(project());
    run(&mut session, "transport.setLoop", json!({ "start": 4, "end": 8 })).unwrap();
    let before = session.project().clone();
    for (id, params) in [
        ("transport.setLoop", json!({ "start": 8, "end": 8 })),
        ("transport.setLoop", json!({ "start": 8, "end": 4 })),
        ("transport.setLoop", json!({ "start": -1, "end": 4 })),
        ("transport.setLoop", json!({ "start": 0, "end": 120_000.5 })),
        ("transport.setLoop", json!({ "start": 0 })),
        ("transport.setLoop", json!({ "start": "0", "end": 4 })),
        ("transport.setLoop", json!({ "start": 0, "end": 4, "enabled": "yes" })),
        ("transport.setLoopEnabled", json!({ "enabled": 1 })),
    ] {
        assert!(run(&mut session, id, params.clone()).is_err(), "{id} {params}");
    }
    assert_eq!(session.project(), &before);
}

/// The audio test project with an audio track Drums (3), and clip slots in two scenes for
/// Bass and Drums; Lead has none. Drums holds a clip in scene 1; the audio clip on the
/// arrangement has fades, a loop, a gain, warping and an offset.
fn launcher_project() -> Project {
    // Ids on its lanes, clips and scenes, as a session gives them.
    let mut project = launcher_project_built();
    assert_eq!(project.assign_ids(), Ok(()));
    project
}

fn launcher_project_built() -> Project {
    let mut p = audio_project_built();
    p.tracks.push(Track {
        content: ContentKinds { notes: false, audio: true },
        channel: Some(ChannelId(3)),
        ..Track::new(TrackId(3), "Drums")
    });
    let mut channel = p.channels[0].clone();
    (channel.id, channel.name) = (ChannelId(3), "C3".into());
    p.channels.push(channel);
    if let LaneData::Audio(region) = &mut p.contents[1].lanes[0].data {
        region.gain = 0.5;
        region.warped = true;
        region.stretch = StretchMode::Tonal;
    }
    let audio = &mut p.arrangement.lanes[0].clips[1];
    (audio.fade_in, audio.fade_out, audio.offset) = (1.0, 0.5, 0.25);
    audio.fade_out_curve = FadeCurve { shape: FadeShape::EqualPower, curve: 0.5 };
    audio.loop_range = Some(LoopRange { start: 0.0, end: 2.0 });
    let mut held = p.arrangement.lanes[0].clips[1].clone();
    held.start = 0.0;
    p.launcher = Launcher {
        scenes: vec![Scene::default(), Scene::default()],
        columns: vec![
            LauncherColumn { track: TrackId(1), slots: vec![Slot::default(), Slot::default()] },
            LauncherColumn {
                track: TrackId(3),
                slots: vec![Slot { clip: Some(held), has_stop: true }, Slot::default()],
            },
        ],
        mappings: Default::default(),
    };
    p
}

#[test]
fn an_arrangement_clip_pastes_into_a_slot_with_everything_it_carries() {
    let mut session = Session::new(launcher_project());
    let arranged = session.project().arrangement.lanes[0].clips[1].clone();
    run(&mut session, "clip.copy", json!({ "clips": [{ "track": "Bass", "index": 1 }] })).unwrap();
    let pasted = run(&mut session, "clip.paste", json!({ "track": "Drums", "scene": 2 })).unwrap();
    let placed = session.project().launcher.columns[1].slots[1].clip.as_ref().map(|c| c.id.0);
    assert_eq!(pasted, json!({ "clip": placed, "track": 3, "scene": 2, "replaced": false }));
    assert_ne!(placed, Some(arranged.id.0), "the pasted clip is a new clip");
    let p = session.project();
    let placed = p.launcher.columns[1].slots[1].clip.clone().unwrap();
    assert_ne!(placed.content, arranged.content, "the slot clip plays its own content");
    assert_eq!(Clip { id: arranged.id, content: arranged.content, start: arranged.start, ..placed.clone() }, arranged);
    assert_eq!(placed.start, 0.0);
    let (copy, original) = (p.content(placed.content).unwrap(), p.content(arranged.content).unwrap());
    assert_eq!(copy.lanes, original.lanes, "gain and warping travel with the content");
    assert_eq!(p.arrangement.lanes[0].clips.len(), 2, "the arrangement is left as it was");
    assert_eq!(p.validate(), Ok(()));

    // One undo step takes it out again, content and all.
    let contents = p.contents.len();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(session.project().launcher.columns[1].slots[1].clip.is_none());
    assert_eq!(session.project().contents.len(), contents - 1);
}

#[test]
fn a_paste_into_a_held_slot_replaces_its_clip() {
    let mut session = Session::new(launcher_project());
    let before = session.project().launcher.columns[1].slots[0].clone();
    run(&mut session, "clip.copy", json!({ "clips": [{ "track": "Bass", "index": 1 }] })).unwrap();
    let pasted = run(&mut session, "clip.paste", json!({ "track": 3, "scene": 1 })).unwrap();
    assert_eq!(pasted["replaced"], true);
    let after = &session.project().launcher.columns[1].slots[0];
    assert_ne!(after.clip.as_ref().map(|c| c.content), before.clip.as_ref().map(|c| c.content));
    assert!(after.has_stop, "the slot keeps its stop button");
}

#[test]
fn a_slot_clip_copies_onto_the_arrangement() {
    let mut session = Session::new(launcher_project());
    run(&mut session, "clip.copy", json!({ "track": "Drums", "scene": 1 })).unwrap();
    let pasted = run(&mut session, "clip.paste", json!({ "track": "Drums", "start": 16 })).unwrap();
    assert_eq!((pasted["clips"][0]["track"].as_u64(), pasted["clips"][0]["start"].as_f64()), (Some(3), Some(16.0)));
    let p = session.project();
    let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(TrackId(3))).unwrap();
    let slot = p.launcher.columns[1].slots[0].clip.clone().unwrap();
    assert_eq!(lane.clips.len(), 1);
    assert_eq!(Clip { id: slot.id, content: slot.content, start: 0.0, ..lane.clips[0].clone() }, slot);
    assert_ne!(lane.clips[0].id, slot.id, "the copy is a clip of its own");
    assert_eq!(p.validate(), Ok(()));
    // A MIDI clip from a slot goes to a MIDI track's slot too.
    run(&mut session, "clip.copy", json!({ "clips": [{ "track": "Bass", "index": 0 }] })).unwrap();
    run(&mut session, "clip.paste", json!({ "track": "Bass", "scene": 1 })).unwrap();
    run(&mut session, "clip.copy", json!({ "track": "Bass", "scene": 1 })).unwrap();
    run(&mut session, "clip.paste", json!({ "track": "Bass", "scene": 2 })).unwrap();
    assert!(session.project().launcher.columns[0].slots.iter().all(|s| s.clip.is_some()));
}

#[test]
fn malformed_copies_and_pastes_into_slots_are_errors_that_record_nothing() {
    let mut session = Session::new(launcher_project());
    let copy = |session: &mut Session, params: Value| run(session, "clip.copy", params);
    for params in [
        json!({ "track": "Bass" }),
        json!({ "track": "Bass", "index": 0, "scene": 1 }),
        json!({ "track": "Bass", "scene": 1 }),
        json!({ "track": "Drums", "scene": 3 }),
        json!({ "track": "Drums", "scene": 0 }),
        json!({ "track": "Drums", "scene": -1 }),
        json!({ "track": "Drums", "scene": "one" }),
        json!({ "track": "Lead", "scene": 1 }),
    ] {
        assert!(copy(&mut session, params.clone()).is_err(), "{params}");
    }
    assert!(run(&mut session, "clip.paste", json!({ "track": "Drums", "scene": 2 })).is_err(), "nothing copied");
    assert!(
        run(&mut session, "clip.cut", json!({ "track": "Drums", "scene": 1 })).is_err(),
        "cut is for the arrangement"
    );

    run(&mut session, "clip.copy", json!({ "clips": [{ "track": "Bass", "index": 1 }] })).unwrap();
    let before = session.project().clone();
    for params in [
        json!({ "track": "Drums" }),
        json!({ "track": "Drums", "start": 4, "scene": 2 }),
        json!({ "track": "Drums", "scene": 2, "stack": true }),
        json!({ "track": "Drums", "scene": 3 }),
        json!({ "track": "Drums", "scene": 0 }),
        json!({ "track": "Drums", "scene": 1.5 }),
        json!({ "track": "Drums", "scene": null }),
        json!({ "track": "Lead", "scene": 1 }),
        json!({ "track": "Nobody", "scene": 1 }),
        json!({ "track": "Drums", "start": -1 }),
        json!({ "track": "Drums", "start": 1e12 }),
    ] {
        assert!(run(&mut session, "clip.paste", params.clone()).is_err(), "{params}");
    }
    // Audio onto a MIDI track's slot, and MIDI onto an audio track's.
    assert!(run(&mut session, "clip.paste", json!({ "track": "Bass", "scene": 2 })).is_err());
    run(&mut session, "clip.copy", json!({ "clips": [{ "track": "Bass", "index": 0 }] })).unwrap();
    assert!(run(&mut session, "clip.paste", json!({ "track": "Drums", "scene": 2 })).is_err());
    assert_eq!(session.project(), &before);
    assert!(run(&mut session, "edit.undo", json!({})).is_err(), "nothing was recorded");
}

/// Bass and Lead play notes, Drums audio; each has two clip slots, Lead's first one full.
fn midi_slot_project() -> Project {
    // Ids on its lanes, clips and scenes, as a session gives them.
    let mut project = midi_slot_project_built();
    assert_eq!(project.assign_ids(), Ok(()));
    project
}

fn midi_slot_project_built() -> Project {
    let mut p = project_built();
    let mut drums = p.tracks[0].clone();
    drums.id = TrackId(3);
    drums.name = "Drums".into();
    drums.channel = None;
    drums.content = ContentKinds { notes: false, audio: true };
    p.tracks.push(drums);
    let column = |track: u32| LauncherColumn {
        track: TrackId(track),
        slots: vec![Slot { clip: None, has_stop: true }, Slot { clip: None, has_stop: true }],
    };
    p.launcher = Launcher {
        scenes: vec![Scene::default(), Scene::default()],
        columns: vec![column(1), column(2), column(3)],
        mappings: Default::default(),
    };
    let full = Clip { start: 0.0, ..p.arrangement.lanes[0].clips[0].clone() };
    p.launcher.columns[1].slots[0].clip = Some(full);
    p
}

fn content_of<'a>(session: &'a Session, clip: &Clip) -> Option<&'a ClipContent> {
    session.project().contents.iter().find(|c| c.id == clip.content)
}

#[test]
fn an_empty_midi_clip_is_made_in_a_slot_one_bar_long_and_looping() {
    let mut session = Session::new(midi_slot_project());
    let made = run(&mut session, "clip.create", json!({ "track": "Bass", "scene": 2 })).unwrap();
    assert_eq!((made["track"].as_u64(), made["scene"].as_u64()), (Some(1), Some(2)));
    let clip = session.project().launcher.columns[0].slots[1].clip.clone().unwrap();
    assert_eq!(clip.length, 4.0, "one bar of 4/4");
    assert_eq!(clip.loop_range, Some(LoopRange { start: 0.0, end: 4.0 }));
    let content = content_of(&session, &clip).unwrap();
    assert_eq!(content.name, "MIDI clip");
    assert!(matches!(content.lanes.as_slice(), [ContentLane { data: LaneData::Notes(notes), .. }] if notes.is_empty()));
    // One undo step takes it back.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(session.project().launcher.columns[0].slots[1].clip.is_none());
    // Undo takes the edit back; the id counter stays where it got to (ADR 0025).
    assert_eq!(session.project(), &Project { next_id: session.project().next_id, ..midi_slot_project() });
}

#[test]
fn an_empty_audio_clip_is_made_on_an_audio_track_no_file_yet() {
    let mut session = Session::new(midi_slot_project());
    let made = run(&mut session, "clip.create", json!({ "track": "Drums", "scene": 2 })).unwrap();
    assert_eq!((made["track"].as_u64(), made["scene"].as_u64()), (Some(3), Some(2)));
    let clip = session.project().launcher.columns[2].slots[1].clip.clone().unwrap();
    assert_eq!(clip.length, 4.0, "one bar of 4/4");
    assert_eq!(clip.loop_range, Some(LoopRange { start: 0.0, end: 4.0 }), "it loops, as the MIDI one does");
    let content = content_of(&session, &clip).unwrap();
    assert_eq!(content.name, "Audio clip");
    assert!(
        matches!(content.lanes.as_slice(), [ContentLane { data: LaneData::Audio(region), .. }]
            if region.file.is_empty() && region.duration_seconds.is_none()),
        "no file yet, so nothing is played"
    );
    // One undo step takes it back.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(session.project().launcher.columns[2].slots[1].clip.is_none());
    // Undo takes the edit back; the id counter stays where it got to (ADR 0025).
    assert_eq!(session.project(), &Project { next_id: session.project().next_id, ..midi_slot_project() });

    // In the arrangement it starts where it is told, same shapes as the MIDI one; a lane is made.
    let at = run(&mut session, "clip.create", json!({ "track": "Drums", "start": 4, "length": 8 })).unwrap();
    assert_eq!((at["start"].as_f64(), at["index"].as_u64()), (Some(4.0), Some(0)));
    let lane = session.project().arrangement.lanes.iter().find(|l| l.name == "Drums").unwrap();
    let clip = lane.clips[0].clone();
    assert!(matches!(content_of(&session, &clip).map(|c| c.lanes.as_slice()),
            Some([ContentLane { data: LaneData::Audio(region), .. }]) if region.file.is_empty()));
    run(&mut session, "edit.undo", json!({})).unwrap();
    // Undo takes the edit back; the id counter stays where it got to (ADR 0025).
    assert_eq!(session.project(), &Project { next_id: session.project().next_id, ..midi_slot_project() });
}

#[test]
fn an_empty_midi_clip_in_the_arrangement_replaces_what_it_covers_unless_stacked() {
    let mut session = Session::new(lane_project());
    // A bar in 3/4 is three beats.
    let mut three = lane_project();
    three.transport.time_signature = TimeSignature { numerator: 3, denominator: 4 };
    let mut waltz = Session::new(three);
    let made = run(&mut waltz, "clip.create", json!({ "track": "Lead", "start": 0 })).unwrap();
    assert_eq!(made["index"], 0);
    let lead = waltz.project().arrangement.lanes.iter().find(|l| l.track == Some(TrackId(2))).unwrap();
    assert_eq!((lead.clips[0].start, lead.clips[0].length, lead.clips[0].loop_range), (0.0, 3.0, None));

    let made = run(&mut session, "clip.create", json!({ "track": "Bass", "start": 2, "length": 4 })).unwrap();
    assert_eq!((made["start"].as_f64(), made["index"].as_u64()), (Some(2.0), Some(1)));
    assert_eq!(spans(&session)[..3], [(0.0, 2.0, 0.0), (2.0, 4.0, 0.0), (6.0, 2.0, 2.0)]);
    let new = session.project().arrangement.lanes[0].clips[1].clone();
    assert_eq!(content_of(&session, &new).unwrap().name, "MIDI clip");

    run(&mut session, "edit.undo", json!({})).unwrap();
    let made = run(&mut session, "clip.create", json!({ "track": "Bass", "start": 2, "stack": true })).unwrap();
    assert_eq!(made["index"], 1);
    assert_eq!(spans(&session).len(), 5, "nothing shortened");
    assert_eq!(spans(&session)[..3], [(0.0, 4.0, 0.0), (2.0, 4.0, 0.0), (4.0, 4.0, 0.0)]);
}

#[test]
fn an_empty_midi_clip_is_refused_where_it_cannot_go_and_changes_nothing() {
    let mut session = Session::new(midi_slot_project());
    for (params, why) in [
        (json!({ "track": "Lead", "scene": 1 }), "already has a clip"),
        (json!({ "track": "Bass", "scene": 3 }), "from 1 to 2"),
        (json!({ "track": "Bass", "scene": 0 }), "from 1"),
        (json!({ "track": "Bass", "scene": -1 }), "from 1"),
        (json!({ "track": "Bass", "scene": "one" }), "scene must be a whole number"),
        (json!({ "track": "Bass" }), "either start"),
        (json!({ "track": "Bass", "scene": 1, "start": 0 }), "either start"),
        (json!({ "track": "Bass", "start": -1 }), "start must be"),
        (json!({ "track": "Bass", "start": 1e300 }), "start must be"),
        (json!({ "track": "Bass", "start": "0" }), "start must be"),
        (json!({ "track": "Bass", "start": 0, "length": 0 }), "length must be"),
        (json!({ "track": "Bass", "start": 0, "length": -4 }), "length must be"),
        (json!({ "track": "Bass", "start": 0, "length": 1e300 }), "length must be"),
        (json!({ "track": "Bass", "start": 0, "stack": "yes" }), "stack"),
        (json!({ "track": "Nobody", "start": 0 }), "no track"),
        (json!({ "scene": 1 }), "track"),
    ] {
        let error = run(&mut session, "clip.create", params.clone()).unwrap_err();
        assert!(error.contains(why), "{params}: {error}");
    }
    // A track that holds no clips (a group's or a return's content) is refused.
    let mut kept = midi_slot_project();
    let mut silent = kept.tracks[0].clone();
    silent.id = TrackId(4);
    silent.name = "Silent".into();
    silent.channel = None;
    silent.content = ContentKinds::default();
    kept.tracks.push(silent);
    let mut refused = Session::new(kept);
    let error = run(&mut refused, "clip.create", json!({ "track": "Silent", "scene": 1 })).unwrap_err();
    assert!(error.contains("holds no clips"), "{error}");
    assert_eq!(session.project(), &midi_slot_project());
    assert!(!session.can_undo(), "nothing was recorded");
}

#[test]
fn an_empty_midi_clip_follows_odd_time_signatures_splits_a_clip_it_falls_inside_and_makes_a_lane() {
    // One bar of 7/8 is three and a half beats, in a slot; a given length wins.
    let mut odd = midi_slot_project();
    odd.transport.time_signature = TimeSignature { numerator: 7, denominator: 8 };
    let mut session = Session::new(odd);
    let made = run(&mut session, "clip.create", json!({ "track": "Bass", "scene": 1 })).unwrap();
    let slot = session.project().launcher.columns[0].slots[0].clip.clone().unwrap();
    assert_eq!((made["scene"].as_u64(), slot.length), (Some(1), 3.5), "one bar of 7/8");
    run(&mut session, "clip.create", json!({ "track": "Bass", "scene": 2, "length": 16 })).unwrap();
    let slot = session.project().launcher.columns[0].slots[1].clip.clone().unwrap();
    assert_eq!(slot.loop_range, Some(LoopRange { start: 0.0, end: 16.0 }));

    // Inside the looping clip at 12 to 20: it is split around the new one.
    let mut session = Session::new(lane_project());
    let made = run(&mut session, "clip.create", json!({ "track": "Bass", "start": 14, "length": 2 })).unwrap();
    assert_eq!(made["index"], 4);
    let after: Vec<(f64, f64)> = spans(&session).into_iter().filter(|s| s.0 >= 12.0).map(|s| (s.0, s.1)).collect();
    assert_eq!(after, [(12.0, 2.0), (14.0, 2.0), (16.0, 4.0)], "split around it");

    // Lead has no lane yet: one is made, named after the track.
    run(&mut session, "clip.create", json!({ "track": "Lead", "start": 0 })).unwrap();
    let lead = session.project().arrangement.lanes.iter().find(|l| l.track == Some(TrackId(2))).unwrap();
    assert_eq!((lead.name.as_str(), lead.clips.len()), ("Lead", 1));
    assert!(session.project().validate().is_ok());
}

/// `lane_project` with its clips playing audio, and a second audio track with one clip.
fn audio_lane_project() -> Project {
    let mut p = lane_project();
    for t in &mut p.tracks {
        t.content = ContentKinds { notes: false, audio: true };
    }
    p.contents[0].lanes = vec![ContentLane { target: None, data: LaneData::Audio(AudioRegion::new("vocal.wav")) }];
    let clip = Clip::new(ContentId(1), 0.0, 4.0);
    p.arrangement.lanes.push(ArrangementLane {
        track: Some(TrackId(2)),
        clips: vec![clip],
        ..ArrangementLane::default()
    });
    p
}

#[test]
fn audio_overlaps_follow_stacking_and_undo() {
    let mut session = Session::new(audio_lane_project());
    let none = run(&mut session, "arrangement.audioOverlaps", json!({})).unwrap();
    assert_eq!(none, json!({ "overlaps": [], "truncated": false }));

    // Stacked at 6, the clip from 0 sounds with the clips at 4 and 8.
    run(&mut session, "clip.move", json!({ "track": "Bass", "index": 0, "start": 6, "stack": true })).unwrap();
    let found = run(&mut session, "arrangement.audioOverlaps", json!({})).unwrap();
    let spans: Vec<_> = found["overlaps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| (o["track_name"].clone(), o["start"].clone(), o["end"].clone(), o["kind"].clone()))
        .collect();
    assert_eq!(
        spans,
        [
            (json!("Bass"), json!(6.0), json!(8.0), json!("partial")),
            (json!("Bass"), json!(8.0), json!(10.0), json!("partial")),
        ]
    );
    let clips = &found["overlaps"][0]["clips"];
    assert_eq!(clips[0]["name"], "Riff");
    for c in clips.as_array().unwrap() {
        assert_eq!(c["track"], 1);
        let index = c["index"].as_u64().unwrap() as usize;
        let clip = &session.project().arrangement.lanes[0].clips[index];
        assert_eq!(c["id"], json!(clip.id.0), "the id clip.move takes");
    }

    // The track filter, by name and by id.
    let lead = run(&mut session, "arrangement.audioOverlaps", json!({ "track": "Lead" })).unwrap();
    assert_eq!(lead["overlaps"], json!([]));
    let bass = run(&mut session, "arrangement.audioOverlaps", json!({ "track": 1 })).unwrap();
    assert_eq!(bass["overlaps"].as_array().map(Vec::len), Some(2));
    assert!(run(&mut session, "arrangement.audioOverlaps", json!({ "track": "Nobody" })).is_err());

    // Undo takes the overlap away; a move without stack replaces instead of overlapping.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(run(&mut session, "arrangement.audioOverlaps", json!({})).unwrap()["overlaps"], json!([]));
    run(&mut session, "clip.move", json!({ "track": "Bass", "index": 0, "start": 6 })).unwrap();
    assert_eq!(run(&mut session, "arrangement.audioOverlaps", json!({})).unwrap()["overlaps"], json!([]));
}

/// Command ids and parameters follow one convention (#764 › 13): a namespace per kind of object,
/// singular, and parameters in camelCase like the ids.
#[test]
fn command_ids_and_parameters_follow_one_convention() {
    let all = suhina_session::all_commands();
    let mut wrong = Vec::new();
    for command in all.as_array().unwrap() {
        let id = command["id"].as_str().unwrap();
        let namespace = id.split('.').next().unwrap();
        if namespace.ends_with('s') {
            wrong.push(format!("{id}: the namespace is plural"));
        }
        for name in command["params"]["properties"].as_object().unwrap().keys() {
            if name.contains('_') || name.starts_with(|c: char| c.is_uppercase()) {
                wrong.push(format!("{id}: {name} is not camelCase"));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}
