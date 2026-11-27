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
    let mut session = crate::support::toned(project());
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
    let mut session = crate::support::toned(project());
    run(&mut session, "track.setSendMode", json!({ "track": "Lead", "return": "B Return", "mode": "PRE" })).unwrap();
    let s = sends(&session, 2);
    assert_eq!(s, vec![Send { target: ChannelId(4), level: 0.0, pre_fader: true }]);
}

#[test]
fn sends_are_listed_with_the_tracks() {
    let mut session = crate::support::toned(project());
    run(&mut session, "track.setSend", json!({ "track": "Bass", "return": "B Return", "level": 0 })).unwrap();
    let tracks = run(&mut session, "project.tracks", json!({})).unwrap();
    assert_eq!(tracks[0]["sends"], json!([{ "return": 4, "name": "B Return", "level_db": 0.0, "mode": "post" }]));
    assert_eq!(tracks[1]["sends"], json!([]));
}

#[test]
fn returns_send_to_other_returns_but_never_in_a_loop() {
    let mut session = crate::support::toned(project());
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
    let mut session = crate::support::toned(p);
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
        ("track.setSend", json!({ "track": "Bass", "return": "A Return", "level": 0.5 })),
        ("track.setSend", json!({ "track": "Bass", "return": "A Return", "level": "loud" })),
        ("track.setSend", json!({ "track": "Bass", "return": "A Return" })),
        ("track.setSend", json!({ "track": 99, "return": "A Return", "level": 0 })),
        ("track.setSend", json!({ "track": "Bass", "return": -1, "level": 0 })),
        ("track.setSendMode", json!({ "track": "Bass", "return": "A Return", "mode": "middle" })),
        ("track.setSendMode", json!({ "track": "Bass", "return": "A Return", "mode": 1 })),
        ("track.setSendMode", json!({ "track": "Bass", "return": "Lead", "mode": "pre" })),
        ("return.add", json!({ "name": "  " })),
        ("return.add", json!({ "name": 3 })),
        ("return.remove", json!({ "return": "Bass" })),
        ("return.remove", json!({})),
        ("return.rename", json!({ "return": "Group", "name": "X" })),
        ("return.rename", json!({ "return": "A Return", "name": "" })),
        ("return.rename", json!({ "return": "A Return" })),
    ];
    for (id, params) in cases {
        let mut session = crate::support::toned(project());
        let result = session.execute(id, &params);
        assert!(result.is_err(), "{id} with {params} succeeded");
        assert_eq!(session.project(), &project(), "{id} with {params} changed the project");
    }
}

#[test]
fn returns_are_added_renamed_and_removed_with_their_sends() {
    let mut session = crate::support::toned(project());
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
    let peak = |p: &Project| {
        suhina_engine::render_offline(p, 48_000, 512, suhina_session::devices())
            .unwrap()
            .iter()
            .fold(0.0f32, |m, s| m.max(s.abs()))
    };
    let mut session = crate::support::toned(project());
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
    let mut session = crate::support::toned(project_built());
    let before = session.project().clone();
    for level in [-30.0, -20.0, -10.0, -6.0] {
        run(
            &mut session,
            "track.setSend",
            json!({ "track": "Bass", "return": "A Return", "level": level, "gesture": "send" }),
        )
        .unwrap();
    }
    assert!(sends(&session, 1).iter().any(|s| (20.0 * s.level.log10() + 6.0).abs() < 0.01));
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &before, "the drag is undone at once");
}

#[test]
fn several_tracks_take_their_own_values_in_one_undo_step() {
    let mut session = crate::support::toned(project());
    let send = |session: &Session, channel: u32| sends(session, channel).first().map(|s| s.level);
    for values in [[-6.0, -12.0], [-3.0, -200.0]] {
        run(
            &mut session,
            "track.setMixerValues",
            json!({ "tracks": ["Bass", "Lead"], "control": "send", "return": "A Return", "values": values, "gesture": "sends" }),
        )
        .unwrap();
    }
    assert!((send(&session, 1).unwrap() - 10f64.powf(-3.0 / 20.0)).abs() < 1e-9);
    assert_eq!(send(&session, 2), Some(0.0), "below -150 is silence");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(send(&session, 1), None, "the drag's two calls were one step");
    assert_eq!(send(&session, 2), None);

    let channel = |session: &Session, id: u32| session.project().channel(ChannelId(id)).cloned().unwrap();
    run(
        &mut session,
        "track.setMixerValues",
        json!({ "tracks": ["Bass", "Lead"], "control": "volume", "values": [-6.0, 0.0] }),
    )
    .unwrap();
    assert!((channel(&session, 1).volume - 10f64.powf(-6.0 / 20.0)).abs() < 1e-9);
    assert_eq!(channel(&session, 2).volume, 1.0);
    run(
        &mut session,
        "track.setMixerValues",
        json!({ "tracks": ["Bass", "Lead"], "control": "pan", "values": [-0.5, 0.25] }),
    )
    .unwrap();
    assert_eq!((channel(&session, 1).pan, channel(&session, 2).pan), (-0.5, 0.25));

    for bad in [
        json!({ "tracks": ["Bass", "Lead"], "control": "pan", "values": [0.0] }),
        json!({ "tracks": ["Bass"], "control": "width", "values": [0.0] }),
        json!({ "tracks": ["Bass"], "control": "send", "values": [0.0] }),
        json!({ "tracks": ["Bass"], "control": "volume", "values": [12.0] }),
        json!({ "tracks": ["Bass"], "control": "send", "return": "A Return", "values": [0.5] }),
    ] {
        assert!(run(&mut session, "track.setMixerValues", bad.clone()).is_err(), "{bad}");
    }
}

/// A send turned up from silence is not in the playing graph: its first move builds one, and the
/// rest of the drag reaches the engine as messages (#763 › 4).
#[test]
fn a_send_turned_up_from_silence_takes_one_graph_then_messages() {
    let project = project();
    let mut renderer =
        suhina_engine::Renderer::stopped(&project, &suhina_engine::AudioPool::new(), 48_000, suhina_session::devices())
            .unwrap();
    let mut session = crate::support::toned(project);
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    // No gesture: a move that needed a new graph would fail here, not catch up later.
    let level = |db: f64| json!({ "track": "Bass", "return": "A Return", "level": db });
    run(&mut session, "track.setSend", level(-30.0)).unwrap();
    // Every later graph waits: the engine's queue of new graphs fills.
    let mut full = false;
    for i in 0..64 {
        if run(&mut session, "track.setMute", json!({ "tracks": ["Lead"], "mute": i % 2 == 0 })).is_err() {
            full = true;
            break;
        }
    }
    assert!(full, "the queue of new graphs is full");
    for db in [-24.0, -18.0, -12.0] {
        run(&mut session, "track.setSend", level(db)).unwrap();
    }
    let mut out = vec![0.0f32; 960];
    renderer.process(&mut out);
}

#[test]
fn a_shared_group_accepts_sends_but_is_not_removed_as_a_return() {
    let mut session = crate::support::toned(project());
    run(&mut session, "track.setSend", json!({"track": "Bass", "return": "Group", "level": -6})).unwrap();
    assert_eq!(sends(&session, 1)[0].target, ChannelId(5));
    run(&mut session, "track.setSendMode", json!({"track": "Bass", "return": "Group", "mode": "pre"})).unwrap();
    assert!(sends(&session, 1)[0].pre_fader);
    assert!(run(&mut session, "return.remove", json!({"return": "Group"})).is_err());
    assert!(session.project().track(TrackId(5)).is_some());
    assert!(run(&mut session, "track.setSend", json!({"track": "Group", "return": "Group", "level": 0})).is_err());
}

#[test]
fn turning_a_send_off_and_on_updates_the_soloed_groups_source_audibility() {
    let mut p = project();
    p.channel_mut(ChannelId(5)).unwrap().solo = true;
    p.channel_mut(ChannelId(1)).unwrap().sends.push(Send { target: ChannelId(5), level: 1.0, pre_fader: false });
    let mut renderer =
        suhina_engine::Renderer::stopped(&p, &suhina_engine::AudioPool::new(), 48_000, suhina_session::devices())
            .unwrap();
    let mut session = crate::support::toned(p);
    session.attach_player(suhina_session::Player::new(renderer.controller(), None));
    run(&mut session, "transport.play", json!({})).unwrap();
    let mut out = vec![0.0f32; 960];
    for _ in 0..4 {
        renderer.process(&mut out);
    }
    assert!(out.iter().any(|s| s.abs() > 0.01));
    run(&mut session, "track.setSend", json!({"track": "Bass", "return": "Group", "level": "-inf"})).unwrap();
    for _ in 0..4 {
        renderer.process(&mut out);
    }
    assert!(out.iter().all(|s| s.abs() < 1e-6), "direct output must also leave the solo graph");
    run(&mut session, "track.setSend", json!({"track": "Bass", "return": "Group", "level": 0})).unwrap();
    run(&mut session, "transport.stop", json!({})).unwrap();
    renderer.process(&mut out);
    run(&mut session, "transport.play", json!({})).unwrap();
    for _ in 0..4 {
        renderer.process(&mut out);
    }
    assert!(out.iter().any(|s| s.abs() > 0.01));
}

/// The keys (sidechains) of the devices on `channel`, at their paths.
fn keys(session: &Session, channel: u32) -> Vec<DeviceInput> {
    session
        .project()
        .channel(ChannelId(channel))
        .map(|c| c.devices.iter().flat_map(|d| d.inputs.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn a_compressor_is_keyed_by_another_track_and_a_loop_or_a_device_without_a_sidechain_is_refused() {
    let mut session = crate::support::toned(project());
    run(&mut session, "device.add", json!({ "track": "Lead", "id": "compressor" })).unwrap();
    run(&mut session, "device.add", json!({ "track": "Lead", "id": "utility" })).unwrap();
    let out = run(
        &mut session,
        "device.setKey",
        json!({ "track": "Lead", "device": 0, "source": "Bass", "tap": "postFader", "gain": -6 }),
    )
    .unwrap();
    assert_eq!(out["key"]["tap"], "postFader");
    let key = &keys(&session, 2)[0];
    assert_eq!((key.source.channel(), key.source.tap(), key.port.as_str()), (ChannelId(1), TapPoint::PostFader, "key"));
    assert!((key.gain - 0.501).abs() < 1e-3);
    let listed = run(&mut session, "track.devices", json!({ "track": "Lead" })).unwrap();
    assert!(listed.to_string().contains(r#""key":{"channel":1,"#), "{listed}");
    // Lead plays into Group: a key from Group closes a loop; so does Lead itself after its devices.
    let looped = run(&mut session, "device.setKey", json!({ "track": "Lead", "device": 0, "source": "Group" }));
    assert!(looped.unwrap_err().contains("loop"));
    assert!(run(&mut session, "device.setKey", json!({ "track": "Lead", "device": 0, "source": "Lead" })).is_err());
    // Lead before its devices is no loop.
    run(&mut session, "device.setKey", json!({ "track": "Lead", "device": 0, "source": "Lead", "tap": "preFx" }))
        .unwrap();
    // A utility has no sidechain input; a bad tap names itself.
    let utility = run(&mut session, "device.setKey", json!({ "track": "Lead", "device": 1, "source": "Bass" }));
    assert!(utility.unwrap_err().contains("no sidechain input"));
    let tap = run(&mut session, "device.setKey", json!({ "track": "Lead", "device": 0, "source": "Bass", "tap": "x" }));
    assert!(tap.unwrap_err().contains("tap"));
    // Inside a device group a key is not read yet: refused rather than kept unheard.
    run(&mut session, "device.add", json!({ "track": "Lead", "id": "compressor" })).unwrap();
    run(&mut session, "device.group", json!({ "track": "Lead", "device": 2 })).unwrap();
    let grouped = run(&mut session, "device.setKey", json!({ "track": "Lead", "device": "2/0/0", "source": "Bass" }));
    assert!(grouped.unwrap_err().contains("device group"));
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    // Undo takes back one key change; clear and "none" take it away.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(keys(&session, 2)[0].source.channel(), ChannelId(1));
    run(&mut session, "device.setKey", json!({ "track": "Lead", "device": 0, "source": "none" })).unwrap();
    assert!(keys(&session, 2).is_empty());
    run(&mut session, "edit.undo", json!({})).unwrap();
    // Deleting the source track takes the key with it.
    run(&mut session, "track.delete", json!({ "track": "Bass" })).unwrap();
    assert!(keys(&session, 2).is_empty());
    assert!(session.project().validate().is_ok());
}

/// `device.setKeyEnabled` switches a key off and on and keeps its source, tap and gain; each
/// switch is one undo step; a device without a key is refused; choosing a source again switches it
/// on; `track.devices` says whether it is read.
#[test]
fn a_key_switched_off_keeps_its_source_and_switches_back_on() {
    let mut session = crate::support::toned(project());
    run(&mut session, "device.add", json!({ "track": "Lead", "id": "compressor" })).unwrap();
    let none = run(&mut session, "device.setKeyEnabled", json!({ "track": "Lead", "device": 0, "enabled": false }));
    assert!(none.unwrap_err().contains("reads no key"));
    run(
        &mut session,
        "device.setKey",
        json!({ "track": "Lead", "device": 0, "source": "Bass", "tap": "preFx", "gain": -6 }),
    )
    .unwrap();
    assert!(keys(&session, 2)[0].enabled, "a key chosen is read");
    let off =
        run(&mut session, "device.setKeyEnabled", json!({ "track": "Lead", "device": 0, "enabled": false })).unwrap();
    assert_eq!((off["enabled"].as_bool(), off["changed"].as_bool()), (Some(false), Some(true)));
    let key = &keys(&session, 2)[0];
    assert!(!key.enabled);
    assert_eq!((key.source.channel(), key.source.tap()), (ChannelId(1), TapPoint::PreFx), "the source stays");
    assert!((key.gain - 0.501).abs() < 1e-3, "the gain stays");
    let listed = run(&mut session, "track.devices", json!({ "track": "Lead" })).unwrap();
    assert!(listed.to_string().contains(r#""enabled":false"#), "{listed}");
    // Undo brings the key back on; switching it off when it is off changes nothing.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(keys(&session, 2)[0].enabled, "undone, the key is read again");
    run(&mut session, "device.setKeyEnabled", json!({ "track": "Lead", "device": 0, "enabled": false })).unwrap();
    let again =
        run(&mut session, "device.setKeyEnabled", json!({ "track": "Lead", "device": 0, "enabled": false })).unwrap();
    assert_eq!(again["changed"].as_bool(), Some(false));
    // Off, then a source chosen again: on.
    run(&mut session, "device.setKeyEnabled", json!({ "track": "Lead", "device": 0, "enabled": false })).unwrap();
    run(&mut session, "device.setKey", json!({ "track": "Lead", "device": 0, "source": "Bass" })).unwrap();
    assert!(keys(&session, 2)[0].enabled);
    let bad = run(&mut session, "device.setKeyEnabled", json!({ "track": "Lead", "device": 0, "enabled": "yes" }));
    assert!(bad.is_err());
    assert!(session.project().validate().is_ok());
}
