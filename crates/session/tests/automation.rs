// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Automation commands (#395): points on mixer and device lanes, undo, turning a lane off by a
//! manual change and on again, and malformed parameters.

use serde_json::{Value, json};
use suhina_model::*;
use suhina_session::Session;

fn project() -> Project {
    // Ids on its lanes, clips and scenes, as a session gives them.
    let mut project = project_built();
    assert_eq!(project.assign_ids(), Ok(()));
    project
}

fn project_built() -> Project {
    Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Keys")
        }],
        channels: vec![Channel::new(ChannelId(1), "C1")],
        ..Project::default()
    }
}

fn run(session: &mut Session, id: &str, params: Value) -> Result<Value, String> {
    session.execute(id, &params).map_err(|e| e.0)
}

fn lanes(session: &Session) -> &[AutomationLane] {
    &session.project().channels[0].automation
}

fn sd_device() -> Device {
    let mut graph =
        suhina_device_sdk::DeviceGraph::new("test.gain", "Test Gain", suhina_device_sdk::DeviceRole::Effect);
    graph.parameters.push(suhina_device_sdk::Parameter::new("gain", "Gain", (0.0, 2.0), 0.5));
    graph.nodes = vec![
        suhina_device_sdk::Node::audio_input("in"),
        suhina_device_sdk::Node::gain("gain", "gain"),
        suhina_device_sdk::Node::audio_output("out"),
    ];
    graph.connections =
        vec![suhina_device_sdk::Connection::new("in", "gain"), suhina_device_sdk::Connection::new("gain", "out")];
    Device {
        name: "Test Gain".into(),
        kind: DeviceKind::Sd,
        enabled: true,
        state: graph.to_sd().ok().map(String::into_bytes),
        params: Vec::new(),
        automation: Vec::new(),
        extensions: Extensions::default(),
    }
}

#[test]
fn mixer_points_are_added_moved_and_deleted_with_undo() {
    let mut session = Session::new(project());
    let added =
        run(&mut session, "automation.addPoint", json!({ "track": "Keys", "param": "volume", "beat": 4, "value": 0 }))
            .unwrap();
    assert_eq!((added["point"].as_u64(), added["points"].as_u64()), (Some(0), Some(1)));
    // In beat order: a point before the first goes first.
    let added = run(
        &mut session,
        "automation.addPoint",
        json!({ "track": 1, "param": "volume", "beat": 0, "value": -150.5, "curve": 0.5 }),
    )
    .unwrap();
    assert_eq!(added["point"], 0);
    run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 2, "value": -1 })).unwrap();
    let volume = &lanes(&session)[0];
    assert_eq!(volume.param, "volume");
    assert_eq!(volume.points, [AutomationPoint { beat: 0.0, value: 0.0, curve: 0.5 }, AutomationPoint::new(4.0, 1.0)]);

    // Moving the first point past the second puts it second.
    let moved = run(
        &mut session,
        "automation.movePoint",
        json!({ "track": 1, "param": "volume", "point": 0, "beat": 8, "value": -6 }),
    )
    .unwrap();
    assert_eq!(moved["point"], 1);
    assert_eq!(moved["value"], -6.0);
    assert_eq!(lanes(&session)[0].points[1].beat, 8.0);
    assert_eq!(lanes(&session)[0].points[1].curve, 0.5, "the curve stays");

    let list = run(&mut session, "automation.list", json!({ "track": 1 })).unwrap();
    assert_eq!(list["mixer"][0]["points"][1]["value"], -6.0, "the volume in dB");
    assert_eq!(list["mixer"][1]["param"], "pan");
    assert_eq!(list["devices"], json!([]));

    // Deleting the last point removes the lane.
    run(&mut session, "automation.deletePoint", json!({ "track": 1, "param": "pan", "point": 0 })).unwrap();
    assert_eq!(lanes(&session).len(), 1);
    let cleared = run(&mut session, "automation.clear", json!({ "track": 1 })).unwrap();
    assert_eq!(cleared["cleared"], 1);
    assert!(lanes(&session).is_empty());

    // Every edit was one undo step.
    for _ in 0..6 {
        run(&mut session, "edit.undo", json!({})).unwrap();
    }
    assert_eq!(session.project(), &project());
}

#[test]
fn a_manual_change_turns_a_lane_off_until_it_is_on_again() {
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({ "track": 1, "id": "utility" })).unwrap();
    run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "volume", "beat": 0, "value": -12 }))
        .unwrap();
    run(
        &mut session,
        "automation.addPoint",
        json!({ "track": 1, "device": 0, "param": "gain", "beat": 0, "value": 99 }),
    )
    .unwrap();
    // Kept in the parameter's range.
    assert_eq!(session.project().channels[0].devices[0].automation[0].points[0].value, 35.0);

    let set = run(&mut session, "track.setVolume", json!({ "tracks": [1], "db": -3 })).unwrap();
    assert_eq!(set["automation"], "off");
    assert!(!lanes(&session)[0].enabled);
    // Once off, another change says nothing more.
    let set = run(&mut session, "track.setVolume", json!({ "tracks": [1], "db": -4 })).unwrap();
    assert!(set.get("automation").is_none());
    let set =
        run(&mut session, "device.setParam", json!({ "track": 1, "device": 0, "param": "gain", "value": -6 })).unwrap();
    assert_eq!(set["automation"], "off");
    assert!(!session.project().channels[0].devices[0].automation[0].enabled);
    // A parameter without a lane is not touched.
    let set = run(&mut session, "track.setPan", json!({ "tracks": [1], "pan": 0.5 })).unwrap();
    assert!(set.get("automation").is_none());

    let list = run(&mut session, "automation.list", json!({ "track": 1 })).unwrap();
    assert_eq!(list["mixer"][0]["enabled"], false);
    assert_eq!(list["devices"][0]["lanes"][0]["enabled"], false);
    run(&mut session, "automation.setEnabled", json!({ "track": 1, "param": "volume", "enabled": true })).unwrap();
    run(&mut session, "automation.setEnabled", json!({ "track": 1, "device": 0, "param": "gain", "enabled": true }))
        .unwrap();
    assert!(lanes(&session)[0].enabled);
    assert!(session.project().channels[0].devices[0].automation[0].enabled);
    // The value set by hand stays, under the lane.
    assert_eq!(session.project().channels[0].devices[0].params, [DeviceParam::new("gain", -6.0)]);
}

#[test]
fn device_set_automation_takes_built_in_devices() {
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({ "track": 1, "id": "utility" })).unwrap();
    let set = run(
        &mut session,
        "device.setAutomation",
        json!({ "track": 1, "device": 0, "param": "gain", "points": [4, -6, 0, -100] }),
    )
    .unwrap();
    assert_eq!((set["param"].as_str(), set["points"].as_u64()), (Some("gain"), Some(2)));
    let lane = &session.project().channels[0].devices[0].automation[0];
    // In beat order, limited to the range.
    assert_eq!(lane.points.iter().map(|p| (p.beat, p.value)).collect::<Vec<_>>(), [(0.0, -70.0), (4.0, -6.0)]);
    assert!(
        run(&mut session, "device.setAutomation", json!({ "track": 1, "device": 0, "param": "nope", "points": [] }))
            .is_err()
    );
}

#[test]
fn sd_automation_uses_stable_ids_and_the_parameter_range() {
    let mut p = project();
    p.channels[0].devices.push(sd_device());
    let mut session = Session::new(p);
    let set = run(
        &mut session,
        "device.setAutomation",
        json!({ "track": 1, "device": 0, "param": "gain", "points": [4, 3, 0, -1] }),
    )
    .unwrap();
    assert_eq!(set["param"], "gain");
    assert_eq!(
        session.project().channels[0].devices[0].automation[0]
            .points
            .iter()
            .map(|p| (p.beat, p.value))
            .collect::<Vec<_>>(),
        [(0.0, 0.0), (4.0, 2.0)]
    );
    run(
        &mut session,
        "automation.addPoint",
        json!({ "track": 1, "device": 0, "param": "gain", "beat": 2, "value": 1.25 }),
    )
    .unwrap();
    assert_eq!(
        run(&mut session, "automation.list", json!({ "track": 1 })).unwrap()["devices"][0]["lanes"][0]["param"],
        "gain"
    );
    let set =
        run(&mut session, "device.setParam", json!({ "track": 1, "device": 0, "param": "gain", "value": 1 })).unwrap();
    assert_eq!(set["automation"], "off");
    assert!(session.project().channels[0].devices[0].automation.iter().all(|lane| !lane.enabled));
}

#[test]
fn malformed_automation_edits_are_refused_and_change_nothing() {
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({ "track": 1, "id": "utility" })).unwrap();
    run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 0, "value": 0 })).unwrap();
    let before = session.project().clone();
    for (id, params) in [
        ("automation.addPoint", json!({ "track": 1, "param": "loudness", "beat": 0, "value": 0 })),
        ("automation.addPoint", json!({ "track": 9, "param": "pan", "beat": 0, "value": 0 })),
        ("automation.addPoint", json!({ "track": 1, "param": "pan", "beat": -1, "value": 0 })),
        ("automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 1e300, "value": 0 })),
        ("automation.addPoint", json!({ "track": 1, "param": "pan", "beat": "soon", "value": 0 })),
        ("automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 0, "value": 2 })),
        ("automation.addPoint", json!({ "track": 1, "param": "volume", "beat": 0, "value": 7 })),
        ("automation.addPoint", json!({ "track": 1, "param": "mute", "beat": 0, "value": 0.5, "curve": 2 })),
        ("automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 0, "value": 0, "curve": "round" })),
        ("automation.addPoint", json!({ "track": 1, "device": 1, "param": "gain", "beat": 0, "value": 0 })),
        ("automation.addPoint", json!({ "track": 1, "device": 0, "param": "volume", "beat": 0, "value": 0 })),
        ("automation.addPoint", json!({ "track": 1, "device": -1, "param": "gain", "beat": 0, "value": 0 })),
        ("automation.addPoint", json!({ "track": 1, "beat": 0, "value": 0 })),
        ("automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 0, "value": 0, "colour": 1 })),
        ("automation.movePoint", json!({ "track": 1, "param": "pan", "point": 1, "beat": 2 })),
        ("automation.movePoint", json!({ "track": 1, "param": "pan", "point": 0 })),
        ("automation.movePoint", json!({ "track": 1, "param": "volume", "point": 0, "beat": 2 })),
        ("automation.deletePoint", json!({ "track": 1, "param": "pan", "point": 5 })),
        ("automation.deletePoint", json!({ "track": 1, "param": "pan", "point": "first" })),
        ("automation.setEnabled", json!({ "track": 1, "param": "volume", "enabled": false })),
        ("automation.setEnabled", json!({ "track": 1, "param": "pan", "enabled": "no" })),
        ("automation.clear", json!({ "track": 1, "device": 4 })),
        ("automation.list", json!({ "track": "Nobody" })),
    ] {
        assert!(run(&mut session, id, params.clone()).is_err(), "{id} {params}");
        assert_eq!(session.project(), &before, "{id} {params}");
    }
    // One undo step for each good edit (the device and the point).
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert!(run(&mut session, "edit.undo", json!({})).is_err());
}

#[test]
fn a_lane_holds_a_limited_number_of_points() {
    let mut p = project();
    let mut lane = AutomationLane::new("pan");
    lane.points = (0..automation::MAX_POINTS).map(|i| AutomationPoint::new(i as f64 * 0.001, 0.0)).collect();
    p.channels[0].automation.push(lane);
    let mut session = Session::new(p);
    let full = run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 0, "value": 0 }));
    assert!(full.unwrap_err().contains("at most"));
}

#[test]
fn several_points_move_together_in_one_undo_step() {
    let mut session = Session::new(project());
    for (beat, value) in [(0, -12), (4, -6), (8, 0)] {
        run(
            &mut session,
            "automation.addPoint",
            json!({ "track": 1, "param": "volume", "beat": beat, "value": value }),
        )
        .unwrap();
    }
    let before = session.project().clone();
    // The first two move two beats later and 3 dB up; the first passes no other point.
    let moved = run(
        &mut session,
        "automation.movePoints",
        json!({ "track": 1, "param": "volume", "points": [0, 1], "beats": 2, "value": 3 }),
    )
    .unwrap();
    assert_eq!(moved["points"], json!([0, 1]));
    let beats: Vec<f64> = lanes(&session)[0].points.iter().map(|p| p.beat).collect();
    assert_eq!(beats, [2.0, 6.0, 8.0]);
    let db = |v: f64| (20.0 * v.log10() * 100.0).round() / 100.0;
    assert_eq!(db(lanes(&session)[0].points[1].value), -3.0);
    // The top of the fader holds a point that would go past it.
    run(&mut session, "automation.movePoints", json!({ "track": 1, "param": "volume", "points": [2], "value": 10 }))
        .unwrap();
    assert!((db(lanes(&session)[0].points[2].value) - 6.0).abs() < 0.01);
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(session.project(), &before);
}

#[test]
fn moved_points_keep_beat_order_and_report_their_new_places() {
    let mut session = Session::new(project());
    for (beat, value) in [(0.0, 0.0), (4.0, 0.5), (8.0, -0.5)] {
        run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": beat, "value": value }))
            .unwrap();
    }
    // The first point jumps past the others; values stay in the pan's range.
    let moved = run(
        &mut session,
        "automation.movePoints",
        json!({ "track": 1, "param": "pan", "points": [0], "beats": 10, "value": -2 }),
    )
    .unwrap();
    assert_eq!(moved["points"], json!([2]));
    let points: Vec<(f64, f64)> = lanes(&session)[0].points.iter().map(|p| (p.beat, p.value)).collect();
    assert_eq!(points, [(4.0, 0.5), (8.0, -0.5), (10.0, -1.0)]);
}

/// A point dragged onto the beat of the next one stays before it, so the lane jumps from the
/// dragged point's value to the other's, as drawn while dragging, not the other way round; a
/// point dragged back onto the beat of the one before stays after it, and changing only the
/// value of the first of two points at one beat keeps their order.
#[test]
fn a_point_moved_onto_another_points_beat_keeps_its_side() {
    let pans =
        |session: &Session| -> Vec<(f64, f64)> { lanes(session)[0].points.iter().map(|p| (p.beat, p.value)).collect() };
    let mut session = Session::new(project());
    for (beat, value) in [(0.0, -0.5), (4.0, 0.5)] {
        run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": beat, "value": value }))
            .unwrap();
    }
    let moved = run(&mut session, "automation.movePoint", json!({ "track": 1, "param": "pan", "point": 0, "beat": 4 }))
        .unwrap();
    assert_eq!(moved["point"], json!(0));
    assert_eq!(pans(&session), [(4.0, -0.5), (4.0, 0.5)]);
    // Only the value of the first: the order stays.
    run(&mut session, "automation.movePoint", json!({ "track": 1, "param": "pan", "point": 0, "value": -0.25 }))
        .unwrap();
    assert_eq!(pans(&session), [(4.0, -0.25), (4.0, 0.5)]);
    // A point that comes from later goes after the ones at its beat.
    run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 8, "value": 1 })).unwrap();
    run(&mut session, "automation.movePoint", json!({ "track": 1, "param": "pan", "point": 2, "beat": 4 })).unwrap();
    assert_eq!(pans(&session), [(4.0, -0.25), (4.0, 0.5), (4.0, 1.0)]);
    // A selection moved later goes before the points at its beat, and a value change of a
    // selection keeps the order.
    let mut session = Session::new(project());
    for (beat, value) in [(0.0, -0.5), (4.0, 0.5)] {
        run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": beat, "value": value }))
            .unwrap();
    }
    run(&mut session, "automation.movePoints", json!({ "track": 1, "param": "pan", "points": [0], "beats": 4 }))
        .unwrap();
    assert_eq!(pans(&session), [(4.0, -0.5), (4.0, 0.5)]);
    let moved =
        run(&mut session, "automation.movePoints", json!({ "track": 1, "param": "pan", "points": [0], "value": 0.25 }))
            .unwrap();
    assert_eq!(moved["points"], json!([0]));
    assert_eq!(pans(&session), [(4.0, -0.25), (4.0, 0.5)]);
}

/// Two selected points at one beat (a jump) moved together onto a third point's beat keep their
/// order, whatever order the request names them in, and the places reported follow the request.
#[test]
fn points_at_one_beat_moved_together_keep_their_jump() {
    let pans =
        |session: &Session| -> Vec<(f64, f64)> { lanes(session)[0].points.iter().map(|p| (p.beat, p.value)).collect() };
    let mut session = Session::new(project());
    for (beat, value) in [(0.0, -0.5), (2.0, 0.5), (4.0, 1.0)] {
        run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": beat, "value": value }))
            .unwrap();
    }
    // A jump at beat 0, from -0.5 to 0.5.
    run(&mut session, "automation.movePoint", json!({ "track": 1, "param": "pan", "point": 1, "beat": 0 })).unwrap();
    assert_eq!(pans(&session), [(0.0, -0.5), (0.0, 0.5), (4.0, 1.0)]);
    // Moved later onto the third point's beat, named last first.
    let moved =
        run(&mut session, "automation.movePoints", json!({ "track": 1, "param": "pan", "points": [1, 0], "beats": 4 }))
            .unwrap();
    assert_eq!(moved["points"], json!([1, 0]));
    assert_eq!(pans(&session), [(4.0, -0.5), (4.0, 0.5), (4.0, 1.0)]);
    // And back earlier onto a point at beat 0: after it, still in their order.
    run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 0, "value": 0 })).unwrap();
    assert_eq!(pans(&session)[0], (0.0, 0.0));
    let moved = run(
        &mut session,
        "automation.movePoints",
        json!({ "track": 1, "param": "pan", "points": [2, 1], "beats": -4 }),
    )
    .unwrap();
    assert_eq!(moved["points"], json!([2, 1]));
    assert_eq!(pans(&session), [(0.0, 0.0), (0.0, -0.5), (0.0, 0.5), (4.0, 1.0)]);
}

#[test]
fn several_points_are_deleted_in_one_undo_step_and_the_lane_goes_with_the_last() {
    let mut session = Session::new(project());
    for beat in [0, 2, 4] {
        run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "mute", "beat": beat, "value": 1 }))
            .unwrap();
    }
    run(&mut session, "automation.deletePoints", json!({ "track": 1, "param": "mute", "points": [2, 0] })).unwrap();
    assert_eq!(lanes(&session)[0].points.len(), 1);
    assert_eq!(lanes(&session)[0].points[0].beat, 2.0);
    run(&mut session, "automation.deletePoints", json!({ "track": 1, "param": "mute", "points": [0] })).unwrap();
    assert!(lanes(&session).is_empty());
    run(&mut session, "edit.undo", json!({})).unwrap();
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(lanes(&session)[0].points.len(), 3);
}

#[test]
fn malformed_point_lists_change_nothing() {
    let mut session = Session::new(project());
    for beat in [0, 2] {
        run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": beat, "value": 0 }))
            .unwrap();
    }
    let before = session.project().clone();
    for (id, params) in [
        ("automation.movePoints", json!({ "track": 1, "param": "pan", "points": [0], "beats": -1 })),
        ("automation.movePoints", json!({ "track": 1, "param": "pan", "points": [0, 0], "beats": 1 })),
        ("automation.movePoints", json!({ "track": 1, "param": "pan", "points": [5], "beats": 1 })),
        ("automation.movePoints", json!({ "track": 1, "param": "pan", "points": [0] })),
        ("automation.movePoints", json!({ "track": 1, "param": "pan", "points": [], "beats": 1 })),
        ("automation.movePoints", json!({ "track": 1, "param": "pan", "points": [-1], "beats": 1 })),
        ("automation.movePoints", json!({ "track": 1, "param": "pan", "points": [0], "curve": 2 })),
        ("automation.deletePoints", json!({ "track": 1, "param": "pan", "points": [2] })),
        ("automation.deletePoints", json!({ "track": 1, "param": "pan", "points": "0" })),
        ("automation.deletePoints", json!({ "track": 1, "param": "volume", "points": [0] })),
    ] {
        assert!(run(&mut session, id, params.clone()).is_err(), "{id} {params}");
    }
    assert_eq!(session.project(), &before);
}
