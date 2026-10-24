// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sends and returns: `track.setSend`, `track.setSendMode`, `return.add`, `return.remove`,
//! `return.rename`.

use serde_json::json;
use suhina_model::*;
use suhina_session::Session;

use crate::support::run;
use suhina_test_support::ProjectBuilder;

/// Bass (notes, one clip) and Lead, Group (a group Lead plays into) and two returns, all into
/// Main.
fn project() -> Project {
    suhina_test_support::with_ids(project_built())
}

fn project_built() -> Project {
    ProjectBuilder::new()
        .midi_track(1, "Bass")
        .bus_track(5, "Group", ChannelRole::Submix)
        .midi_track(2, "Lead")
        .bus_track(3, "A Return", ChannelRole::Effect)
        .bus_track(4, "B Return", ChannelRole::Effect)
        .channel(Channel { role: ChannelRole::Master, ..Channel::new(ChannelId(100), "C100") })
        .parent(2, 5)
        .output(1, 100)
        .output(2, 5)
        .output(5, 100)
        .output(3, 100)
        .output(4, 100)
        .notes(1, "Riff", 1.0, vec![Note::new(0.0, 1.0, 69, 1.0)])
        .clip(1, Clip::new(ContentId(1), 0.0, 1.0))
        .build_without_ids()
}

fn sends(session: &Session, channel: u32) -> Vec<Send> {
    session.project().channel(ChannelId(channel)).map(|c| c.sends.clone()).unwrap_or_default()
}

#[test]
fn a_send_is_made_set_and_undone() {
    let mut session = Session::new(project());
    let out =
        run(&mut session, "track.setSend", json!({ "track": "Bass", "return": "A Return", "level": -6 })).unwrap();
    assert_eq!(out["level_db"], -6.0);
    let s = sends(&session, 1);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].target, ChannelId(3));
    assert!((s[0].level - 0.501).abs() < 1e-3);
    assert!(!s[0].pre_fader);

    run(&mut session, "track.setSendMode", json!({ "track": 1, "return": 3, "mode": "pre" })).unwrap();
    assert!(sends(&session, 1)[0].pre_fader);
    // The same send again: no second one.
    run(&mut session, "track.setSend", json!({ "track": "Bass", "return": "A Return", "level": "-inf" })).unwrap();
    assert_eq!(sends(&session, 1).len(), 1);
    assert_eq!(sends(&session, 1)[0].level, 0.0);
    assert!(sends(&session, 1)[0].pre_fader);

    for _ in 0..3 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert_eq!(session.project(), &project());
}

#[test]
fn a_send_mode_without_a_send_makes_a_silent_one() {
    let mut session = Session::new(project());
    run(&mut session, "track.setSendMode", json!({ "track": "Lead", "return": "B Return", "mode": "PRE" })).unwrap();
    let s = sends(&session, 2);
    assert_eq!(s, vec![Send { target: ChannelId(4), level: 0.0, pre_fader: true }]);
}

#[test]
fn sends_are_listed_with_the_tracks() {
    let mut session = Session::new(project());
    run(&mut session, "track.setSend", json!({ "track": "Bass", "return": "B Return", "level": 0 })).unwrap();
    let tracks = run(&mut session, "project.tracks", json!({})).unwrap();
    assert_eq!(tracks[0]["sends"], json!([{ "return": 4, "name": "B Return", "level_db": 0.0, "mode": "post" }]));
    assert_eq!(tracks[1]["sends"], json!([]));
}

#[test]
fn returns_send_to_other_returns_but_never_in_a_loop() {
    let mut session = Session::new(project());
    run(&mut session, "track.setSend", json!({ "track": "A Return", "return": "B Return", "level": 0 })).unwrap();
    let err = run(&mut session, "track.setSend", json!({ "track": "B Return", "return": "A Return", "level": 0 }))
        .unwrap_err();
    assert!(err.contains("loop"), "{err}");
    let err = run(&mut session, "track.setSend", json!({ "track": "A Return", "return": "A Return", "level": 0 }))
        .unwrap_err();
    assert!(err.contains("itself"), "{err}");

    // A return that plays into a group cannot be fed by that group.
    let mut p = project();
    p.channels[3].output = Some(ChannelId(5));
    let mut session = Session::new(p);
    let err =
        run(&mut session, "track.setSend", json!({ "track": "Group", "return": "A Return", "level": 0 })).unwrap_err();
    assert!(err.contains("loop"), "{err}");
    // A track inside the group can: the group does not send.
    run(&mut session, "track.setSend", json!({ "track": "Lead", "return": "A Return", "level": 0 })).unwrap();
}

#[test]
fn bad_send_parameters_change_nothing() {
    let cases = [
        ("track.setSend", json!({ "track": "Bass", "return": "Lead", "level": 0 })),
        ("track.setSend", json!({ "track": "Bass", "return": "nope", "level": 0 })),
        ("track.setSend", json!({ "track": "Bass", "return": "A Return", "level": 7 })),
        ("track.setSend", json!({ "track": "Bass", "return": "A Return", "level": "loud" })),
        ("track.setSend", json!({ "track": "Bass", "return": "A Return" })),
        ("track.setSend", json!({ "track": 99, "return": "A Return", "level": 0 })),
        ("track.setSend", json!({ "track": "Bass", "return": -1, "level": 0 })),
        ("track.setSendMode", json!({ "track": "Bass", "return": "A Return", "mode": "middle" })),
        ("track.setSendMode", json!({ "track": "Bass", "return": "A Return", "mode": 1 })),
        ("track.setSendMode", json!({ "track": "Bass", "return": "Group", "mode": "pre" })),
        ("return.add", json!({ "name": "  " })),
        ("return.add", json!({ "name": 3 })),
        ("return.remove", json!({ "return": "Bass" })),
        ("return.remove", json!({})),
        ("return.rename", json!({ "return": "Group", "name": "X" })),
        ("return.rename", json!({ "return": "A Return", "name": "" })),
        ("return.rename", json!({ "return": "A Return" })),
    ];
    for (id, params) in cases {
        let mut session = Session::new(project());
        let result = session.execute(id, &params);
        assert!(result.is_err(), "{id} with {params} succeeded");
        assert_eq!(session.project(), &project(), "{id} with {params} changed the project");
    }
}

#[test]
fn returns_are_added_renamed_and_removed_with_their_sends() {
    let mut session = Session::new(project());
    let added = run(&mut session, "return.add", json!({})).unwrap();
    assert_eq!(added["name"], "C Return");
    let id = added["track"].as_u64().unwrap();
    let p = session.project();
    let track = p.tracks.last().unwrap();
    assert_eq!(track.id.0 as u64, id);
    let channel = p.channels.iter().find(|c| Some(c.id) == track.channel).unwrap();
    assert_eq!(channel.role, ChannelRole::Effect);
    assert_eq!(channel.output, Some(ChannelId(100)));
    // A return has no clip slots or arrangement lane.
    assert!(p.launcher.columns.iter().all(|c| c.track != track.id));
    assert!(p.arrangement.lanes.iter().all(|l| l.track != Some(track.id)));

    run(&mut session, "return.rename", json!({ "return": "C Return", "name": "Echo" })).unwrap();
    run(&mut session, "track.setSend", json!({ "track": "Bass", "return": "Echo", "level": 0 })).unwrap();
    run(&mut session, "track.setSend", json!({ "track": "Bass", "return": "A Return", "level": 0 })).unwrap();
    run(&mut session, "return.remove", json!({ "return": "Echo" })).unwrap();
    assert!(session.project().tracks.iter().all(|t| t.name != "Echo"));
    assert_eq!(sends(&session, 1).len(), 1);
    assert_eq!(sends(&session, 1)[0].target, ChannelId(3));

    let named = run(&mut session, "return.add", json!({ "name": " Hall " })).unwrap();
    assert_eq!(named["name"], "Hall");
    for _ in 0..6 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert_eq!(session.project(), &project());
}

#[test]
fn a_send_set_by_command_is_heard_through_the_return() {
    let peak =
        |p: &Project| suhina_engine::render_offline(p, 48_000, 512).unwrap().iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let mut session = Session::new(project());
    run(&mut session, "track.setVolume", json!({ "tracks": ["Bass"], "db": "-inf" })).unwrap();
    assert!(peak(session.project()) < 1e-6);
    run(&mut session, "track.setSend", json!({ "track": "Bass", "return": "A Return", "level": 0 })).unwrap();
    // After the fader, a send follows the track's silent volume.
    assert!(peak(session.project()) < 1e-6);
    run(&mut session, "track.setSendMode", json!({ "track": "Bass", "return": "A Return", "mode": "pre" })).unwrap();
    assert!(peak(session.project()) > 0.1);
    run(&mut session, "track.setMute", json!({ "tracks": ["A Return"], "mute": true })).unwrap();
    assert!(peak(session.project()) < 1e-6);
}

#[test]
fn a_send_dragged_is_one_undo_step() {
    let mut session = Session::new(project_built());
    let before = session.project().clone();
    for (level, continued) in [(-30.0, false), (-20.0, true), (-10.0, true), (-6.0, true)] {
        run(
            &mut session,
            "track.setSend",
            json!({ "track": "Bass", "return": "A Return", "level": level, "continue": continued }),
        )
        .unwrap();
    }
    assert!(sends(&session, 1).iter().any(|s| (20.0 * s.level.log10() + 6.0).abs() < 0.01));
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &before, "the drag is undone at once");
}
