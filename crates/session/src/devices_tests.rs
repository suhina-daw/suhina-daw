// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_model::{Channel, ChannelId, Device, DeviceKind, Project, Track, TrackId};

use super::*;

/// A project with one MIDI track whose channel runs a VST3 plug-in device.
fn project() -> Project {
    let mut project = Project::default();
    let mut track = Track::new(TrackId(1), "Synth");
    track.content.notes = true;
    let mut channel = Channel::new(ChannelId(1), "Synth");
    channel.devices.push(Device::new("Serum", DeviceKind::Vst3 { id: "AABBCCDDEEFF0011AABBCCDDEEFF0011".into() }));
    project.tracks.push(track);
    project.channels.push(channel);
    project
}

#[test]
fn a_reported_editor_edit_becomes_the_device_s_value() {
    let mut project = project();
    let key = suhina_engine::plugin_devices(&project)[0].key;
    // The first edit the project has no stored parameter for yet: it gets one.
    assert!(take_in_param_edits(&mut project, &[(key, 0, 0.125)]));
    let device = &project.channels[0].devices[0];
    assert_eq!(
        device.params.iter().find(|p| param_id(&p.id) == Some(0)).map(|p| p.value),
        Some(0.125),
        "a parameter the device had not stored yet gets one"
    );
    // The next edit of the same parameter replaces the stored value.
    assert!(take_in_param_edits(&mut project, &[(key, 0, 0.5)]));
    assert_eq!(project.channels[0].devices[0].params[0].value, 0.5);
    // An edit that says what the device already keeps changes nothing.
    assert!(!take_in_param_edits(&mut project, &[(key, 0, 0.5)]));
    assert_eq!(project.channels[0].devices[0].params.len(), 1);
}

#[test]
fn edits_for_devices_the_project_no_longer_holds_are_left_out() {
    let mut project = project();
    let key = suhina_engine::plugin_devices(&project)[0].key;
    let other = key ^ 1;
    assert!(!take_in_param_edits(&mut project, &[(other, 0, 0.125)]));
    assert!(project.channels[0].devices[0].params.is_empty());
}
