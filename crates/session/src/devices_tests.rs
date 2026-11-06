// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_model::{AutomationLane, AutomationPoint, Channel, ChannelId, Device, DeviceKind, Project, Track, TrackId};

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
    assert!(take_in_param_edits(&mut project, &[(key, 0, 0.125)], None).0);
    let device = &project.channels[0].devices[0];
    assert_eq!(
        device.params.iter().find(|p| param_id(&p.id) == Some(0)).map(|p| p.value),
        Some(0.125),
        "a parameter the device had not stored yet gets one"
    );
    // The next edit of the same parameter replaces the stored value.
    assert!(take_in_param_edits(&mut project, &[(key, 0, 0.5)], None).0);
    assert_eq!(project.channels[0].devices[0].params[0].value, 0.5);
    // An edit that says what the device already keeps changes nothing.
    assert!(!take_in_param_edits(&mut project, &[(key, 0, 0.5)], None).0);
    assert_eq!(project.channels[0].devices[0].params.len(), 1);
}

#[test]
fn edits_for_devices_the_project_no_longer_holds_are_left_out() {
    let mut project = project();
    let key = suhina_engine::plugin_devices(&project)[0].key;
    let other = key ^ 1;
    assert!(!take_in_param_edits(&mut project, &[(other, 0, 0.125)], None).0);
    assert!(project.channels[0].devices[0].params.is_empty());
}

/// A plug-in project whose device has an enabled lane on parameter 0: 0 at beat 0 up to 1 at beat 8.
fn with_lane() -> Project {
    let mut project = project();
    let mut lane = AutomationLane::new("0");
    lane.points = vec![AutomationPoint::new(0.0, 0.0), AutomationPoint::new(8.0, 1.0)];
    project.channels[0].devices[0].automation.push(lane);
    project
}

/// A move in the plug-in's own editor of a parameter a lane plays turns the lane off, as a change
/// in the parameter list does; a value the lane sent there and the plug-in reported back does not,
/// stopped or playing (#1205).
#[test]
fn an_editor_edit_turns_its_lane_off_but_an_echo_does_not() {
    let enabled = |p: &Project| p.channels[0].devices[0].automation[0].enabled;
    let mut project = with_lane();
    let key = suhina_engine::plugin_devices(&project)[0].key;
    // Stopped at beat 4 the lane plays 0.5: the plug-in reporting 0.5 is the lane's own value.
    assert_eq!(take_in_param_edits(&mut project, &[(key, 0, 0.5)], Some((false, 4.0))), (true, false));
    assert!(enabled(&project));
    // Playing at beat 4, a report of what it played a beat ago is an echo too.
    assert!(!take_in_param_edits(&mut project, &[(key, 0, 0.375)], Some((true, 4.0))).1);
    assert!(enabled(&project));
    // A value the lane does not play around there is a move by hand: the lane goes off.
    assert_eq!(take_in_param_edits(&mut project, &[(key, 0, 0.95)], Some((false, 4.0))), (true, true));
    assert!(!enabled(&project));
    assert_eq!(project.channels[0].devices[0].params[0].value, 0.95, "the value set by hand holds");
    // Without a player nothing sends: any edit is by hand.
    let mut project = with_lane();
    assert!(take_in_param_edits(&mut project, &[(key, 0, 0.5)], None).1);
    // Another parameter leaves the lane alone.
    let mut project = with_lane();
    assert!(!take_in_param_edits(&mut project, &[(key, 1, 0.95)], Some((false, 4.0))).1);
    assert!(enabled(&project));
}
