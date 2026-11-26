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
    }
    // The reading agrees with the command's answer.
    let read = selection(&mut session);
    assert_eq!(read["device"]["device_id"], picked["device"]["device_id"], "{read}");
    assert_eq!(read["track"], track.0);
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
