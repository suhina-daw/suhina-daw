// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The selection (#1679, `docs/spec/selection.md`): the selected track and the selected device
//! of its chain — what controllers, mappings and agents point at.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use serde_json::{Value, json};
use suhina_session::{Session, blank_project};

use crate::support::run;

/// A blank project with two FM Synths on its first track: a chain to step through.
fn with_devices() -> (Session, suhina_model::TrackId) {
    let mut session = Session::new(blank_project());
    let track = session.project().tracks[0].id;
    run(&mut session, "device.add", json!({ "track": track.0, "id": "fm-synth" })).unwrap();
    run(&mut session, "device.add", json!({ "track": track.0, "id": "utility" })).unwrap();
    (session, track)
}

fn selection(session: &mut Session) -> Value {
    run(session, "project.selection", json!({})).unwrap()
}

/// Selecting a track selects its first device, and the answer carries the device's parameters
/// with the name, range and unit a controller's knob and ring need (#1679).
#[test]
fn selecting_a_track_selects_its_first_device_with_its_parameters() {
    let (mut session, track) = with_devices();
    let picked = run(&mut session, "track.select", json!({ "track": track.0 })).unwrap();
    assert_eq!(picked["track"], track.0);
    assert_eq!(picked["device"]["index"], 0, "{picked}");
    let params = picked["device"]["params"].as_array().unwrap();
    assert!(!params.is_empty(), "{picked}");
    for param in params {
        assert!(param["name"].is_string(), "{param}");
        assert!(param["min"].is_number() && param["max"].is_number(), "{param}");
        assert!(param["unit"].is_string(), "{param}");
        assert!(param["kind"].is_string(), "a controller tells its dials from its switches: {param}");
        let normalized = param["normalized"].as_f64().unwrap_or_else(|| panic!("the dial's position: {param}"));
        assert!((0.0..=1.0).contains(&normalized), "0 to 1: {param}");
    }
    // The dials a controller's knobs take, in the panel's order.
    let dials = picked["device"]["dials"].as_array().unwrap_or_else(|| panic!("the dials: {picked}"));
    assert!(!dials.is_empty(), "{picked}");
    for id in dials {
        let id = id.as_str().unwrap_or_default();
        let param = params.iter().find(|p| p["id"] == id).unwrap_or_else(|| panic!("dial {id} in {picked}"));
        assert!(matches!(param["kind"].as_str(), Some("continuous" | "integer")), "a dial, not a switch: {param}");
    }
    // The reading agrees with the command's answer, and one device reads the same directly.
    let read = selection(&mut session);
    assert_eq!(read["device"]["device_id"], picked["device"]["device_id"], "{read}");
    assert_eq!(read["track"], track.0);
    let one = run(&mut session, "device.read", json!({ "track": track.0, "device": 0 })).unwrap();
    assert_eq!(one, picked["device"], "device.read answers the selection's entry");
    // A device or track that is gone reads as nothing, so a live lock can release.
    let gone = run(&mut session, "device.read", json!({ "track": track.0, "device": 99 })).unwrap();
    assert_eq!(gone, Value::Null);
    let gone = run(&mut session, "device.read", json!({ "track": 999, "device": 0 })).unwrap();
    assert_eq!(gone, Value::Null);
}

/// A name several tracks share is still an ambiguity, not a device that is gone: only a name
/// no track has reads as nothing, so a controller's lock releases for a real removal.
#[test]
fn device_read_keeps_an_ambiguous_track_name_as_an_error() {
    let mut session = Session::new(blank_project());
    let [first, second] = [session.project().tracks[0].id, session.project().tracks[1].id];
    for track in [first, second] {
        run(&mut session, "device.add", json!({ "track": track.0, "id": "utility" })).unwrap();
    }
    let error = run(&mut session, "device.read", json!({ "track": "# MIDI", "device": 0 })).unwrap_err();
    assert!(error.contains("several tracks are named"), "{error}");
    let gone = run(&mut session, "device.read", json!({ "track": "No such track", "device": 0 })).unwrap();
    assert_eq!(gone, Value::Null);
}

/// A dial's normalized position is the panel's, not a linear share of the value: half of the FM
/// Synth's logarithmic attack is its geometric middle, and the reading carries the position back
/// so a controller's ring matches the dial (#1718).
#[test]
fn a_normalized_position_turns_a_logarithmic_time_smoothly() {
    let (mut session, track) = with_devices();
    run(&mut session, "track.select", json!({ "track": track.0 })).unwrap();
    let set = run(
        &mut session,
        "device.setParam",
        json!({ "track": track.0, "device": 0, "param": "op1.env.attack", "normalized": 0.5 }),
    )
    .unwrap();
    let value = set["value"].as_f64().unwrap_or_default();
    let middle = (0.0005f64 * 60.0).sqrt();
    assert!((value - middle).abs() < 1e-9, "the log dial's middle: {set}");
    assert!(value < 0.5, "not the linear middle of 0.0005 to 60: {set}");
    let read = selection(&mut session);
    let attack = read["device"]["params"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "op1.env.attack")
        .unwrap_or_else(|| panic!("the attack: {read}"));
    assert!((attack["normalized"].as_f64().unwrap_or_default() - 0.5).abs() < 1e-9, "{read}");
    assert!((attack["value"].as_f64().unwrap_or_default() - middle).abs() < 1e-9, "{read}");
    // One of the two, not both, and a finite number.
    let both = run(
        &mut session,
        "device.setParam",
        json!({ "track": track.0, "device": 0, "param": "op1.env.attack", "value": 1.0, "normalized": 0.5 }),
    );
    assert!(both.is_err(), "{both:?}");
    let neither =
        run(&mut session, "device.setParam", json!({ "track": track.0, "device": 0, "param": "op1.env.attack" }));
    assert!(neither.is_err(), "{neither:?}");
}

/// Stepping: the project's track order for tracks (nothing at either end; the first or last
/// with nothing selected), and the chain for devices (nothing at its end).
#[test]
fn stepping_follows_the_project_order_and_the_chain() {
    let (mut session, track) = with_devices();
    let one = session.project().tracks[0].id;
    let two = session.project().tracks[1].id;
    assert_eq!(one, track);
    // Nothing selected: the first track, and its first device.
    let first = run(&mut session, "track.selectNext", json!({})).unwrap();
    assert_eq!(first["track"], one.0);
    assert_eq!(first["device"]["index"], 0, "{first}");
    // The chain: next, then its end holds, then back.
    let second = run(&mut session, "device.selectNext", json!({})).unwrap();
    assert_eq!(second["device"]["index"], 1, "{second}");
    let end = run(&mut session, "device.selectNext", json!({})).unwrap();
    assert_eq!(end["device"]["index"], 1, "the end does nothing");
    let back = run(&mut session, "device.selectPrevious", json!({})).unwrap();
    assert_eq!(back["device"]["index"], 0, "{back}");
    // Tracks: next moves to the second track, whose chain is empty, so no device.
    let moved = run(&mut session, "track.selectNext", json!({})).unwrap();
    assert_eq!(moved["track"], two.0);
    assert_eq!(moved["device"], Value::Null, "{moved}");
    // The last track's end holds.
    for _ in 0..10 {
        run(&mut session, "track.selectNext", json!({})).unwrap();
    }
    assert_eq!(selection(&mut session)["track"], session.project().tracks.last().unwrap().id.0);
    // The already-selected track changes nothing: pick a device, select the track again.
    run(&mut session, "track.select", json!({ "track": one.0 })).unwrap();
    run(&mut session, "device.selectNext", json!({})).unwrap();
    let kept = run(&mut session, "track.select", json!({ "track": one.0 })).unwrap();
    assert_eq!(kept["device"]["index"], 1, "{kept}");
}

/// A removed device or track leaves nothing selected, and undo never restores the selection —
/// it is not an edit.
#[test]
fn a_removed_device_or_track_leaves_nothing_selected() {
    let (mut session, track) = with_devices();
    run(&mut session, "track.select", json!({ "track": track.0 })).unwrap();
    run(&mut session, "device.remove", json!({ "track": track.0, "device": 0 })).unwrap();
    let after = selection(&mut session);
    assert_eq!(after["track"], track.0, "the track stays selected");
    assert_eq!(after["device"], Value::Null, "{after}");
    // Undo brings the device back; the selection stays as it was.
    run(&mut session, "edit.undo", json!({})).unwrap();
    assert_eq!(selection(&mut session)["device"], Value::Null, "undo never restores the selection");
    // The track going clears all of it.
    run(&mut session, "track.delete", json!({ "track": track.0 })).unwrap();
    let gone = selection(&mut session);
    assert_eq!(gone["track"], Value::Null, "{gone}");
    assert_eq!(gone["device"], Value::Null, "{gone}");
}

/// Naming a device that is not there is refused with its reason, as every other device command
/// is; with no track selected and none named, `device.select` says so.
#[test]
fn selecting_a_missing_device_is_refused_with_its_reason() {
    let (mut session, track) = with_devices();
    let err = run(&mut session, "device.select", json!({ "device": 0 })).unwrap_err();
    assert!(err.contains("no track is selected"), "{err}");
    run(&mut session, "track.select", json!({ "track": track.0 })).unwrap();
    let err = run(&mut session, "device.select", json!({ "device": 9 })).unwrap_err();
    assert!(err.contains("device"), "{err}");
    let err = run(&mut session, "device.select", json!({ "device": { "id": 99999 } })).unwrap_err();
    assert!(err.contains("99999"), "{err}");
}

/// The main channel is selectable, addressed as the device commands address it; stepping is
/// over the project's tracks, so it moves off it.
#[test]
fn the_main_channel_is_selectable() {
    let mut session = Session::new(blank_project());
    let picked = run(&mut session, "track.select", json!({ "track": "main" })).unwrap();
    assert_eq!(picked["track"], suhina_model::TrackId::MAIN.0);
    assert_eq!(picked["track_name"], "Main");
    let moved = run(&mut session, "track.selectNext", json!({})).unwrap();
    assert_eq!(moved["track"], session.project().tracks[0].id.0, "{moved}");
}

/// The controller's dials follow the page the panel shows: a page change in the window changes
/// the reading's `dials`, and a controller following the reading takes the new page (#1718).
#[test]
fn a_page_change_moves_the_controllers_dials() {
    let (mut session, track) = with_devices();
    run(&mut session, "track.select", json!({ "track": track.0 })).unwrap();
    let before = selection(&mut session);
    let before_dials = before["device"]["dials"].as_array().unwrap().clone();
    assert_eq!(before_dials[0], "op1.env.attack", "{before}");
    run(&mut session, "device.setParam", json!({ "track": track.0, "device": 0, "param": "panel.page", "value": 8 }))
        .unwrap();
    let after = selection(&mut session);
    let after_dials = after["device"]["dials"].as_array().unwrap();
    assert_ne!(after_dials, &before_dials, "{after}");
    assert_eq!(after_dials[0], "random.amount", "the Random page's own dial: {after}");
}
