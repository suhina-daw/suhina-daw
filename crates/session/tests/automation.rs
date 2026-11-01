// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Automation commands (#395): points on mixer and device lanes, undo, turning a lane off by a
//! manual change and on again, and malformed parameters.

use serde_json::json;
use suhina_model::automation::Curve;
use suhina_model::*;
use suhina_session::Session;

use crate::support::run;
use suhina_test_support::ProjectBuilder;

fn project() -> Project {
    suhina_test_support::with_ids(project_built())
}

fn project_built() -> Project {
    ProjectBuilder::new().midi_track(1, "Keys").build_without_ids()
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
        clip_host: false,
        extensions: Extensions::default(),
    }
}

/// A curve is given as two control points; a point added without a value goes on the envelope
/// and splits a bent segment so that it plays as before.
#[test]
fn a_point_added_on_a_bent_segment_splits_it_exactly() {
    let mut session = Session::new(project());
    let lopsided = json!([0.1, 0.7, 0.825, 0.525]);
    for (beat, value) in [(0, -24), (8, 0)] {
        let point = json!({ "track": 1, "param": "volume", "beat": beat, "value": value, "curve": lopsided });
        run(&mut session, "automation.addPoint", point).unwrap();
    }
    let before = lanes(&session)[0].clone();
    run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "volume", "beat": 3 })).unwrap();
    let after = &lanes(&session)[0];
    assert_eq!(after.points.len(), 3);
    for beat in [1.0, 3.0, 5.5] {
        let (old, new) = (
            before.value_at(beat, false, &automation::VolumeTaper),
            after.value_at(beat, false, &automation::VolumeTaper),
        );
        assert!((old.unwrap() - new.unwrap()).abs() < 1e-9, "{beat}");
    }
    let list = run(&mut session, "automation.list", json!({ "track": 1 })).unwrap();
    assert_eq!(list["mixer"][0]["points"][2]["curve"], lopsided);
}

/// An S whose points run back in time, split by a point on the envelope, plays as before: its
/// halves lie outside the unit square, unclamped, and are listed and taken back as they are.
#[test]
fn a_split_s_curve_plays_as_before_with_halves_outside_the_square() {
    let mut session = Session::new(project());
    for (beat, value) in [(0.0, -1.0), (8.0, 1.0)] {
        let point = json!({ "track": 1, "param": "pan", "beat": beat, "value": value, "curve": [1, 0, 0, 1] });
        run(&mut session, "automation.addPoint", point).unwrap();
    }
    let before = lanes(&session)[0].clone();
    run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 6 })).unwrap();
    run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 1 })).unwrap();
    let after = lanes(&session)[0].clone();
    assert_eq!(after.points.len(), 4);
    // The 0.75 cut's first half had x1 = 1.1958; cut again at beat 1, its second half keeps x1
    // above 1.
    assert!(after.points[1].curve.x1 > 1.1, "{:?}", after.points[1]);
    for q in 0..=640 {
        let beat = q as f64 / 80.0;
        let (old, new) =
            (before.value_at(beat, false, &automation::Plain), after.value_at(beat, false, &automation::Plain));
        // Beat 4 is the S's upright middle, where rounding alone moves the value by about 1e-5.
        let tolerance = if (beat - 4.0).abs() < 1e-9 { 1e-4 } else { 1e-9 };
        assert!((old.unwrap() - new.unwrap()).abs() < tolerance, "{beat}: {old:?} against {new:?}");
    }
    let list = run(&mut session, "automation.list", json!({ "track": 1 })).unwrap();
    let listed = list["mixer"][0]["points"][1]["curve"].clone();
    assert_eq!(listed, json!(after.points[1].curve.to_array()));
    let back = json!({ "track": 1, "param": "pan", "point": 0, "curve": listed });
    run(&mut session, "automation.movePoint", back).unwrap();
    assert_eq!(lanes(&session)[0].points[0].curve, after.points[1].curve);
}

/// A point added without a value keeps a level or straight segment exactly, holds the value
/// outside the points and on a step, and at a point's beat takes that point's curve; a lane
/// without points needs a value.
#[test]
fn a_point_added_on_the_envelope_keeps_it_as_it_was() {
    let mut session = Session::new(project());
    let add = |session: &mut Session, param: &str, beat: f64| {
        run(session, "automation.addPoint", json!({ "track": 1, "param": param, "beat": beat })).map(|_| ())
    };
    assert!(add(&mut session, "pan", 2.0).unwrap_err().to_string().contains("give a value"));
    for (param, beat, value, curve) in [
        ("pan", 0, -1.0, json!(0.6)),
        ("pan", 8, 1.0, json!(0)),
        ("mute", 0, 0.0, json!(0)),
        ("mute", 8, 1.0, json!(0)),
    ] {
        let point = json!({ "track": 1, "param": param, "beat": beat, "value": value, "curve": curve });
        run(&mut session, "automation.addPoint", point).unwrap();
    }
    let before = lanes(&session)[0].clone();
    for beat in [0.0, 12.0] {
        add(&mut session, "pan", beat).unwrap();
    }
    add(&mut session, "mute", 4.0).unwrap();
    let (pan, mute) = (&lanes(&session)[0], &lanes(&session)[1]);
    assert_eq!(
        pan.points.iter().map(|p| (p.beat, p.value)).collect::<Vec<_>>(),
        [(0.0, -1.0), (0.0, -1.0), (8.0, 1.0), (12.0, 1.0)]
    );
    assert_eq!(pan.points[1].curve, Curve::bend(0.6), "the bend stays with the segment");
    for beat in [1.0, 4.0, 10.0] {
        assert_eq!(pan.value_at(beat, false, &automation::Plain), before.value_at(beat, false, &automation::Plain));
    }
    assert_eq!(mute.points[1], AutomationPoint::new(4.0, 0.0));

    // On a level or straight segment the value is the envelope's and the lane plays as before.
    let mut session = Session::new(project());
    for (param, value) in [("pan", 0.5), ("volume", -24.0)] {
        for beat in [0, 8] {
            let value = if beat == 8 && param == "volume" { 0.0 } else { value };
            let point = json!({ "track": 1, "param": param, "beat": beat, "value": value });
            run(&mut session, "automation.addPoint", point).unwrap();
        }
    }
    let before = lanes(&session).to_vec();
    add(&mut session, "pan", 3.0).unwrap();
    add(&mut session, "volume", 2.0).unwrap();
    assert_eq!(lanes(&session)[0].points[1], AutomationPoint::new(3.0, 0.5));
    for beat in [1.0, 2.0, 5.0] {
        let (old, new) = (
            before[1].value_at(beat, false, &automation::VolumeTaper),
            lanes(&session)[1].value_at(beat, false, &automation::VolumeTaper),
        );
        assert!((old.unwrap() - new.unwrap()).abs() < 1e-12, "{beat}");
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
    assert_eq!(
        volume.points,
        [AutomationPoint { beat: 0.0, value: 0.0, curve: Curve::bend(0.5) }, AutomationPoint::new(4.0, 1.0)]
    );

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
    assert_eq!(lanes(&session)[0].points[1].curve, Curve::bend(0.5), "the curve stays");

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

/// A whole-number parameter's lane, placed as the engine places it: a value between two whole
/// numbers sits where it falls, unrounded.
struct WholeNumbers(suhina_device_api::ParamInfo);

impl automation::Taper for WholeNumbers {
    fn position(&self, value: f64) -> f64 {
        self.0.to_normalized(value)
    }
    fn value(&self, position: f64) -> f64 {
        self.0.min + position * (self.0.max - self.0.min)
    }
}

/// A point added without a value on a whole-number lane keeps the envelope: it stores the value
/// between two numbers (the engine cuts the fraction off only as it plays), and the segments on
/// either side move toward it, not toward a rounded one.
#[test]
fn a_point_added_on_a_whole_number_lane_keeps_the_envelope() {
    let mut session = Session::new(project());
    run(&mut session, "device.add", json!({ "track": 1, "id": "sampler" })).unwrap();
    let poly = suhina_device_registry::info("sampler")
        .and_then(|info| info.params.iter().find(|p| p.id == "poly").copied())
        .unwrap();
    assert!(matches!(poly.kind, suhina_device_api::ParamKind::Integer));
    let taper = WholeNumbers(poly);
    let point = |beat: f64, value: Option<f64>| {
        let mut params = json!({ "track": 1, "device": 0, "param": "poly", "beat": beat });
        if let Some(value) = value {
            params["value"] = json!(value);
        }
        params
    };
    for (beat, value) in [(0.0, 1.0), (8.0, 4.0)] {
        run(&mut session, "automation.addPoint", point(beat, Some(value))).unwrap();
    }
    let lane = |session: &Session| session.project().channels[0].devices[0].automation[0].clone();
    let before = lane(&session);
    run(&mut session, "automation.addPoint", point(4.0, None)).unwrap();
    let after = lane(&session);
    assert_eq!(after.points.iter().map(|p| p.value).collect::<Vec<_>>(), [1.0, 2.5, 4.0]);
    for beat in [1.0, 2.0, 4.0, 6.0, 7.5] {
        let (old, new) = (before.value_at(beat, false, &taper), after.value_at(beat, false, &taper));
        assert!((old.unwrap() - new.unwrap()).abs() < 1e-9, "{beat}: {old:?} against {new:?}");
    }
    assert!((after.value_at(2.0, false, &taper).unwrap() - 1.75).abs() < 1e-9);
    let list = run(&mut session, "automation.list", json!({ "track": 1 })).unwrap();
    assert_eq!(list["devices"][0]["lanes"][0]["points"][1]["value"], json!(2.5), "{list}");
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
        (
            "automation.addPoint",
            json!({ "track": 1, "param": "pan", "beat": 0, "value": 0, "curve": [0, 1.6, 0.5, 0.5] }),
        ),
        (
            "automation.addPoint",
            json!({ "track": 1, "param": "pan", "beat": 0, "value": 0, "curve": [-0.6, 0, 0.5, 0.5] }),
        ),
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

fn pan_points(session: &Session) -> Vec<(f64, f64)> {
    lanes(session).iter().find(|l| l.param == "pan").map_or_else(Vec::new, |l| {
        l.points.iter().map(|p| ((p.beat * 1e6).round() / 1e6, (p.value * 1e6).round() / 1e6)).collect()
    })
}

/// A range of a lane copies with its edge values and pastes anywhere, onto another parameter at
/// the same heights; Ctrl+D's duplicate goes right after the range; a deleted range keeps the
/// envelope outside it (#916).
#[test]
fn an_automation_range_copies_pastes_duplicates_and_deletes() {
    let mut session = Session::new(project());
    for (beat, value) in [(0.0, -1.0), (4.0, 1.0)] {
        run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": beat, "value": value }))
            .unwrap();
    }
    run(&mut session, "automation.copy", json!({ "track": 1, "param": "pan", "start": 0, "end": 4 })).unwrap();
    // Pasted at beat 8: the rise again, the lane holding its last value up to there.
    let done = run(&mut session, "automation.paste", json!({ "track": 1, "param": "pan", "start": 8 })).unwrap();
    assert_eq!((done["start"].as_f64(), done["end"].as_f64()), (Some(8.0), Some(12.0)));
    assert_eq!(pan_points(&session), [(0.0, -1.0), (4.0, 1.0), (8.0, 1.0), (8.0, -1.0), (12.0, 1.0)]);
    // Onto the mute lane: the bottom is off, the top on.
    run(&mut session, "automation.paste", json!({ "track": 1, "param": "mute", "start": 0 })).unwrap();
    let mute = lanes(&session).iter().find(|l| l.param == "mute").unwrap();
    assert_eq!(mute.points.first().map(|p| p.value), Some(0.0));
    assert_eq!(mute.points.last().map(|p| p.value), Some(1.0));
    // Duplicated right after itself, answering the copy's range.
    let done = run(&mut session, "automation.duplicate", json!({ "track": 1, "param": "pan", "start": 8, "end": 12 }))
        .unwrap();
    assert_eq!((done["start"].as_f64(), done["end"].as_f64()), (Some(12.0), Some(16.0)));
    let points = pan_points(&session);
    assert!(points.contains(&(16.0, 1.0)) && points.contains(&(12.0, -1.0)), "{points:?}");
    // Deleting 1 to 3 keeps the values at 1 and 3, so the line outside is as it was.
    run(&mut session, "automation.deleteRange", json!({ "track": 1, "param": "pan", "start": 1, "end": 3 })).unwrap();
    assert_eq!(pan_points(&session)[..4], [(0.0, -1.0), (1.0, -0.5), (3.0, 0.5), (4.0, 1.0)]);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(pan_points(&session)[..2], [(0.0, -1.0), (4.0, 1.0)]);
}

/// A shape fills a range over the parameter's whole travel, a repeating one as many times as
/// asked; the envelope outside the range stays (#916).
#[test]
fn a_shape_fills_an_automation_range() {
    let mut session = Session::new(project());
    run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 0, "value": 0 })).unwrap();
    run(
        &mut session,
        "automation.insertShape",
        json!({ "track": 1, "param": "pan", "start": 4, "end": 8, "shape": "square", "cycles": 2 }),
    )
    .unwrap();
    assert_eq!(
        pan_points(&session),
        [
            (0.0, 0.0),
            (4.0, 0.0),
            (4.0, 1.0),
            (5.0, 1.0),
            (5.0, -1.0),
            (6.0, -1.0),
            (6.0, 1.0),
            (7.0, 1.0),
            (7.0, -1.0),
            (8.0, -1.0),
            (8.0, 0.0)
        ]
    );
    // A shape on a lane with no automation makes the lane.
    run(
        &mut session,
        "automation.insertShape",
        json!({ "track": 1, "param": "volume", "start": 0, "end": 4, "shape": "ramp_up" }),
    )
    .unwrap();
    let volume = lanes(&session).iter().find(|l| l.param == "volume").unwrap();
    assert_eq!(volume.points.first().map(|p| p.value), Some(0.0), "the bottom is silence");
    assert!(volume.points.last().is_some_and(|p| (p.value - suhina_model::automation::MAX_VOLUME).abs() < 1e-9));
    assert!(
        run(
            &mut session,
            "automation.insertShape",
            json!({ "track": 1, "param": "pan", "start": 0, "end": 4, "shape": "spiral" })
        )
        .is_err()
    );
    assert!(run(&mut session, "automation.copy", json!({ "track": 1, "param": "pan", "start": 4, "end": 4 })).is_err());
}

/// With `point`, a shape fills the segment from the point before it, between the two points'
/// heights; `automation.addPoint` with `shape` gives the new point's segment that shape (#916).
#[test]
fn a_shape_fills_a_points_segment_and_a_new_points_segment() {
    let mut session = Session::new(project());
    for (beat, value) in [(0.0, 0.0), (4.0, 1.0)] {
        run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": beat, "value": value }))
            .unwrap();
    }
    run(&mut session, "automation.insertShape", json!({ "track": 1, "param": "pan", "point": 1, "shape": "square" }))
        .unwrap();
    let points = pan_points(&session);
    assert_eq!(points.first(), Some(&(0.0, 0.0)));
    assert_eq!(points.last(), Some(&(4.0, 1.0)));
    assert!(points.iter().all(|&(b, v)| (0.0..=4.0).contains(&b) && (0.0..=1.0).contains(&v)), "{points:?}");
    assert!(points.len() > 6, "a square once a beat: {points:?}");
    // A new point at 8 with a falling curve: its segment from 4 takes the bent line.
    run(
        &mut session,
        "automation.addPoint",
        json!({ "track": 1, "param": "pan", "beat": 8, "value": -1, "shape": "curve_down" }),
    )
    .unwrap();
    let after: Vec<(f64, f64)> = pan_points(&session).into_iter().filter(|(b, _)| *b >= 4.0).collect();
    assert_eq!(after.last(), Some(&(8.0, -1.0)));
    assert!(
        lanes(&session)
            .iter()
            .find(|l| l.param == "pan")
            .is_some_and(|l| l.points.iter().any(|p| p.curve.bend_of().is_some_and(|b| b > 0.5)))
    );
    assert!(
        run(
            &mut session,
            "automation.insertShape",
            json!({ "track": 1, "param": "pan", "point": 99, "shape": "sine" })
        )
        .is_err()
    );
}

/// Less depth swings a shape around the lane's value where the range starts; a sine is a few bent
/// segments a cycle; Smooth points thins dense points and keeps the line; Stretch scales the
/// points around their middle (#916).
#[test]
fn shapes_take_a_depth_and_ranges_smooth_and_stretch() {
    let mut session = Session::new(project());
    run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 0, "value": 0 })).unwrap();
    run(
        &mut session,
        "automation.insertShape",
        json!({ "track": 1, "param": "pan", "start": 0, "end": 4, "shape": "square", "depth": 0.5 }),
    )
    .unwrap();
    let values: Vec<f64> = pan_points(&session).iter().map(|p| p.1).collect();
    assert!(values.iter().all(|v| (-0.5..=0.5).contains(v)), "half the travel around the middle: {values:?}");
    assert!(values.contains(&0.5) && values.contains(&-0.5), "{values:?}");
    // A sine over 4 beats, once a beat: five points a cycle at most, bent.
    run(
        &mut session,
        "automation.insertShape",
        json!({ "track": 1, "param": "pan", "start": 8, "end": 12, "shape": "sine", "cycles": 4 }),
    )
    .unwrap();
    let sine: Vec<(f64, f64)> = pan_points(&session).into_iter().filter(|(b, _)| (8.0..=12.0).contains(b)).collect();
    assert!(sine.len() <= 18, "{sine:?}");
    // Dense points along a straight rise thin to its two ends.
    for i in 0..=20 {
        let beat = 16.0 + f64::from(i) * 0.2;
        let value = -1.0 + f64::from(i) * 0.1;
        run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": beat, "value": value }))
            .unwrap();
    }
    let done = run(&mut session, "automation.simplify", json!({ "track": 1, "param": "pan", "start": 16, "end": 20 }))
        .unwrap();
    assert_eq!(done["removed"], json!(19));
    let line: Vec<(f64, f64)> = pan_points(&session).into_iter().filter(|(b, _)| (16.0..=20.0).contains(b)).collect();
    assert_eq!(line, [(16.0, -1.0), (20.0, 1.0)]);
    // Stretch at a half: the square's swing halves around its middle.
    run(
        &mut session,
        "automation.scaleRange",
        json!({ "track": 1, "param": "pan", "start": 0, "end": 4, "factor": 0.5 }),
    )
    .unwrap();
    let values: Vec<f64> = pan_points(&session).iter().filter(|p| p.0 <= 4.0).map(|p| p.1).collect();
    assert!(values.iter().all(|v| (-0.25 - 1e-9..=0.25 + 1e-9).contains(v)), "{values:?}");
    assert!(
        run(
            &mut session,
            "automation.scaleRange",
            json!({ "track": 1, "param": "pan", "start": 0, "end": 4, "factor": 9 })
        )
        .is_err()
    );
}

/// Stretch on picked points moves only them, around the middle of the line over their span: the
/// troughs of a wave picked alone deepen and its peaks stay (#916).
#[test]
fn stretching_picked_points_moves_only_them() {
    let mut session = Session::new(project());
    for (beat, value) in [(0.0, 0.5), (1.0, -0.5), (2.0, 0.5), (3.0, -0.5), (4.0, 0.5)] {
        run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": beat, "value": value }))
            .unwrap();
    }
    run(&mut session, "automation.scaleRange", json!({ "track": 1, "param": "pan", "points": [1, 3], "factor": 2 }))
        .unwrap();
    let values: Vec<f64> = pan_points(&session).iter().map(|p| p.1).collect();
    assert_eq!(values, [0.5, -1.0, 0.5, -1.0, 0.5]);
}

/// Scale around the bottom of the lane takes a share of each place; a scaling that replaces the
/// one before starts from the line as it was, so 200 % and back to 100 % puts back peaks the lane's
/// edge had cut (#916).
#[test]
fn scaling_around_the_bottom_and_back_restores_the_line() {
    let mut session = Session::new(project());
    for (beat, value) in [(0.0, 0.0), (1.0, 0.8), (2.0, 0.0)] {
        run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": beat, "value": value }))
            .unwrap();
    }
    let scale = |factor: f64, again: bool| {
        json!({ "track": 1, "param": "pan", "start": 0, "end": 2, "factor": factor, "around": "middle",
                "continue": again, "replace": again })
    };
    run(&mut session, "automation.scaleRange", scale(2.0, false)).unwrap();
    let values: Vec<f64> = pan_points(&session).iter().map(|p| p.1).collect();
    assert_eq!(values[1], 1.0, "the peak is cut at the lane's top: {values:?}");
    run(&mut session, "automation.scaleRange", scale(1.0, true)).unwrap();
    let values: Vec<f64> = pan_points(&session).iter().map(|p| (p.1 * 1e9).round() / 1e9).collect();
    assert_eq!(values, [0.0, 0.8, 0.0]);
    // Around the bottom (full left), a half: each place halves toward it.
    let bottom = json!({ "track": 1, "param": "pan", "start": 0, "end": 2, "factor": 0.5, "around": "bottom" });
    run(&mut session, "automation.scaleRange", bottom).unwrap();
    let values: Vec<f64> = pan_points(&session).iter().map(|p| (p.1 * 1e9).round() / 1e9).collect();
    assert_eq!(values, [-0.5, -0.1, -0.5]);
}

/// Smooth points keeps a sine a sine: the line after it stays within a few hundredths of the
/// line before it everywhere, curves and all, with fewer points (#916).
#[test]
fn smoothing_a_sine_keeps_its_shape() {
    use suhina_model::automation::{Plain, value_at};
    let mut session = Session::new(project());
    // A sine of 16 straight points a beat, over 4 beats.
    for i in 0..=64 {
        let beat = f64::from(i) / 16.0;
        let value = (std::f64::consts::TAU * beat).sin() * 0.8;
        run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": beat, "value": value }))
            .unwrap();
    }
    let lane = |s: &Session| lanes(s).iter().find(|l| l.param == "pan").unwrap().points.clone();
    let before = lane(&session);
    run(&mut session, "automation.simplify", json!({ "track": 1, "param": "pan", "start": 0, "end": 4 })).unwrap();
    let after = lane(&session);
    assert!(after.len() < before.len() / 2, "{} -> {}", before.len(), after.len());
    for i in 0..=400 {
        let beat = f64::from(i) / 100.0;
        let (old, new) =
            (value_at(&before, beat, false, &Plain).unwrap(), value_at(&after, beat, false, &Plain).unwrap());
        // 2 % of the pan's travel (-1 to 1) is 0.04; a little more for the old straight pieces.
        assert!((old - new).abs() < 0.06, "at beat {beat}: {old} -> {new}");
    }
}

/// Smooth rounds a square's jumps into eased steps that pass the middle at the jump's beat, and a
/// triangle's tip into a curve that stays under it; undo brings the sharp line back (#916).
#[test]
fn smooth_rounds_jumps_and_corners() {
    use suhina_model::automation::{Plain, value_at};
    let mut session = Session::new(project());
    let lane = |s: &Session| lanes(s).iter().find(|l| l.param == "pan").unwrap().points.clone();
    run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 0, "value": 0 })).unwrap();
    run(
        &mut session,
        "automation.insertShape",
        json!({ "track": 1, "param": "pan", "start": 0, "end": 4, "shape": "square", "cycles": 2 }),
    )
    .unwrap();
    let square = lane(&session);
    let jumps = |points: &[suhina_model::AutomationPoint]| {
        points
            .windows(2)
            .filter(|w| w[0].beat > 0.01 && w[0].beat < 3.99 && (w[1].beat - w[0].beat).abs() < 1e-9)
            .count()
    };
    assert!(jumps(&square) > 0, "{square:?}");
    let done = run(
        &mut session,
        "automation.smooth",
        json!({ "track": 1, "param": "pan", "start": 0, "end": 4, "amount": 1 }),
    )
    .unwrap();
    assert!(done["rounded"].as_u64().unwrap() > 0, "{done}");
    let smooth = lane(&session);
    assert_eq!(jumps(&smooth), 0, "{smooth:?}");
    // At a jump's beat the eased step is halfway; just after it, it has not reached the top yet.
    let jump = square.windows(2).find(|w| w[0].beat > 0.01 && (w[1].beat - w[0].beat).abs() < 1e-9).unwrap();
    let (from, to, at) = (jump[0].value, jump[1].value, jump[0].beat);
    let middle = value_at(&smooth, at, false, &Plain).unwrap();
    assert!((middle - (from + to) / 2.0).abs() < 1e-6, "{middle} between {from} and {to}");
    let soon = value_at(&smooth, at + 0.05, false, &Plain).unwrap();
    assert!((soon - to).abs() > 1e-3 && (soon - from).abs() > (to - from).abs() / 2.0, "{soon}");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(lane(&session), square);

    // A triangle's tip: the corner stays, the line on either side bends into it.
    let mut session = Session::new(project());
    for (beat, value) in [(0.0, -1.0), (1.0, 1.0), (2.0, -1.0)] {
        run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": beat, "value": value }))
            .unwrap();
    }
    run(&mut session, "automation.smooth", json!({ "track": 1, "param": "pan", "start": 0, "end": 2, "amount": 0.5 }))
        .unwrap();
    let round = lane(&session);
    assert_eq!(round.len(), 5, "{round:?}");
    assert!(round.iter().any(|p| p.beat == 1.0 && p.value == 1.0), "{round:?}");
    let near = value_at(&round, 0.9, false, &Plain).unwrap();
    assert!(near > 0.6 && near < 1.0, "the straight line is at 0.6 there, the rounded one above it: {near}");
}

/// Scale steps that continue one another are one undo step, as a slider's drag is (#916).
#[test]
fn continued_scaling_is_one_undo_step() {
    let mut session = Session::new(project());
    for (beat, value) in [(0.0, 0.5), (1.0, -0.5), (2.0, 0.5)] {
        run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": beat, "value": value }))
            .unwrap();
    }
    let before = pan_points(&session);
    let scale = |continued: bool| json!({ "track": 1, "param": "pan", "start": 0, "end": 2, "factor": 1.2, "continue": continued });
    run(&mut session, "automation.scaleRange", scale(false)).unwrap();
    run(&mut session, "automation.scaleRange", scale(true)).unwrap();
    run(&mut session, "automation.scaleRange", scale(true)).unwrap();
    assert_ne!(pan_points(&session), before);
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(pan_points(&session), before);
}

/// A slider's live values replace one another (`continue` with `replace`): each runs on the line
/// as the step found it, a value that changes nothing (Smooth on a sine) leaves the line and the
/// history as they were, and one undo takes the whole drag back (#916).
#[test]
fn replacing_live_values_starts_from_the_line_as_it_was() {
    let mut session = Session::new(project());
    let lane = |s: &Session| lanes(s).iter().find(|l| l.param == "pan").unwrap().points.clone();
    run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 0, "value": 0 })).unwrap();
    run(
        &mut session,
        "automation.insertShape",
        json!({ "track": 1, "param": "pan", "start": 0, "end": 4, "shape": "sine", "cycles": 2 }),
    )
    .unwrap();
    let sine = lane(&session);
    let smooth = |amount: f64, continued: bool| {
        json!({ "track": 1, "param": "pan", "start": 0, "end": 4, "amount": amount,
                "continue": continued, "replace": continued })
    };
    // A sine's segments are bent already: Smooth leaves it, however often it is asked.
    for (i, amount) in [0.2, 0.5, 0.9].into_iter().enumerate() {
        run(&mut session, "automation.smooth", smooth(amount, i > 0)).unwrap();
        assert_eq!(lane(&session), sine);
    }
    // Undo takes the sine back out, as nothing was smoothed in between.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_ne!(lane(&session), sine);
    run(&mut session, "edit.redo", json!({})).unwrap();
    assert_eq!(lane(&session), sine);

    // Shapes put in again with replace: each from the line before the first, one undo for all.
    let mut session = Session::new(project());
    run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 0, "value": 0 })).unwrap();
    let line = lane(&session);
    let shape = |depth: f64, continued: bool| {
        json!({ "track": 1, "param": "pan", "start": 0, "end": 4, "shape": "square", "depth": depth,
                "center": 0.5, "continue": continued, "replace": continued })
    };
    run(&mut session, "automation.insertShape", shape(1.0, false)).unwrap();
    run(&mut session, "automation.insertShape", shape(0.5, true)).unwrap();
    run(&mut session, "automation.insertShape", shape(0.25, true)).unwrap();
    let mut once = Session::new(project());
    run(&mut once, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 0, "value": 0 })).unwrap();
    run(&mut once, "automation.insertShape", shape(0.25, false)).unwrap();
    assert_eq!(lane(&session), lane(&once), "the last value, from the line as it was");
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(lane(&session), line);
}

/// Volume points picked at the bottom of the lane (silent) rise with the rest when moved up
/// together, from -70 dB; moved back down to the bottom they are silent again (#916).
#[test]
fn silent_volume_points_move_up_with_the_rest() {
    let mut session = Session::new(project());
    for (beat, value) in [(0.0, json!("-inf")), (1.0, json!(-6)), (2.0, json!("-inf"))] {
        run(
            &mut session,
            "automation.addPoint",
            json!({ "track": 1, "param": "volume", "beat": beat, "value": value }),
        )
        .unwrap();
    }
    let volume = |s: &Session| lanes(s).iter().find(|l| l.param == "volume").unwrap().points.clone();
    run(
        &mut session,
        "automation.movePoints",
        json!({ "track": 1, "param": "volume", "points": [0, 1, 2], "value": 40 }),
    )
    .unwrap();
    let db: Vec<f64> = volume(&session).iter().map(|p| (20.0 * p.value.log10() * 100.0).round() / 100.0).collect();
    assert_eq!(db, [-30.0, 6.0, -30.0]);
    run(
        &mut session,
        "automation.movePoints",
        json!({ "track": 1, "param": "volume", "points": [0, 2], "value": -40 }),
    )
    .unwrap();
    let points = volume(&session);
    assert_eq!((points[0].value, points[2].value), (0.0, 0.0));
}

/// A curve whose points lie beyond the box can overshoot its own points; two halves could not
/// play as it does there, so a point is not added on it without a value.
#[test]
fn a_point_is_not_added_on_a_segment_that_overshoots_its_points() {
    let mut session = Session::new(project());
    let overshoot = json!([0.3, 1.5, 0.7, 1.5]);
    for (beat, value) in [(0, -1), (8, 1)] {
        let point = json!({ "track": 1, "param": "pan", "beat": beat, "value": value, "curve": overshoot });
        run(&mut session, "automation.addPoint", point).unwrap();
    }
    let before = lanes(&session)[0].clone();
    let refused = run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 4 }));
    assert!(refused.is_err());
    assert_eq!(lanes(&session)[0], before);
    // A cut before the overshoot is refused too when a half's points would need clamping.
    let refused = run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 1.925 }));
    assert!(refused.is_err());
    assert_eq!(lanes(&session)[0], before);
    // A value given, or the beat of a point, still works.
    run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 4, "value": 0 })).unwrap();
}

/// A half that cannot move is level only if its control points are: a curve that rises before
/// reaching the cut value of zero is not split into a straight first half.
#[test]
fn a_point_is_not_added_where_a_half_would_lose_its_rise() {
    let mut session = Session::new(project());
    let dips = json!([0.25, 1.0 / 6.0, 0.75, -0.5]);
    for (beat, value) in [(0, -1), (8, 1)] {
        let point = json!({ "track": 1, "param": "pan", "beat": beat, "value": value, "curve": dips });
        run(&mut session, "automation.addPoint", point).unwrap();
    }
    let before = lanes(&session)[0].clone();
    let refused = run(&mut session, "automation.addPoint", json!({ "track": 1, "param": "pan", "beat": 4 }));
    assert!(refused.is_err());
    assert_eq!(lanes(&session)[0], before);
}
