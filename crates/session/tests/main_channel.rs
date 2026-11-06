// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Devices and automation on the main channel, named "main" where a track goes (#687).

use serde_json::json;
use suhina_model::*;
use suhina_session::Session;

use crate::support::run;

fn main_channel(session: &Session) -> Channel {
    session
        .project()
        .channels
        .iter()
        .find(|c| c.role == ChannelRole::Master)
        .cloned()
        .unwrap_or_else(|| Channel::new(ChannelId(0), "none"))
}

#[test]
fn devices_and_their_automation_go_on_main() {
    let mut session = Session::new(suhina_session::blank_project());
    run(&mut session, "device.add", json!({ "track": "main", "id": "utility" })).unwrap();
    assert_eq!(main_channel(&session).devices.len(), 1);
    run(&mut session, "device.setParam", json!({ "track": "main", "device": 0, "param": "gain", "value": -6.0 }))
        .unwrap();
    run(
        &mut session,
        "automation.addPoint",
        json!({ "track": "main", "device": 0, "param": "gain", "beat": 0, "value": -12.0 }),
    )
    .unwrap();
    assert_eq!(main_channel(&session).devices[0].automation.len(), 1);
    // Main's own volume lane too.
    run(&mut session, "automation.addPoint", json!({ "track": "main", "param": "volume", "beat": 4, "value": -3.0 }))
        .unwrap();
    assert!(main_channel(&session).automated("volume"));
    // Its number is the reserved id.
    run(&mut session, "device.remove", json!({ "track": TrackId::MAIN.0, "device": 0 })).unwrap();
    assert!(main_channel(&session).devices.is_empty());
}

#[test]
fn main_takes_no_instrument_and_is_not_a_track() {
    let mut session = Session::new(suhina_session::blank_project());
    assert!(run(&mut session, "device.add", json!({ "track": "main", "id": "fm-synth" })).is_err());
    // Track commands do not reach the main channel.
    assert!(run(&mut session, "track.delete", json!({ "track": "main" })).is_err());
    assert!(run(&mut session, "track.setOutput", json!({ "track": "main", "to": "main" })).is_err());
    assert!(session.project().channels.iter().any(|c| c.role == ChannelRole::Master));
}
