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
    assert!(take_in_param_edits(&mut project, &[(key, 0, 0.125)], None, &[]).0);
    let device = &project.channels[0].devices[0];
    assert_eq!(
        device.params.iter().find(|p| param_id(&p.id) == Some(0)).map(|p| p.value),
        Some(0.125),
        "a parameter the device had not stored yet gets one"
    );
    // The next edit of the same parameter replaces the stored value.
    assert!(take_in_param_edits(&mut project, &[(key, 0, 0.5)], None, &[]).0);
    assert_eq!(project.channels[0].devices[0].params[0].value, 0.5);
    // An edit that says what the device already keeps changes nothing.
    assert!(!take_in_param_edits(&mut project, &[(key, 0, 0.5)], None, &[]).0);
    assert_eq!(project.channels[0].devices[0].params.len(), 1);
}

#[test]
fn edits_for_devices_the_project_no_longer_holds_are_left_out() {
    let mut project = project();
    let key = suhina_engine::plugin_devices(&project)[0].key;
    let other = key ^ 1;
    assert!(!take_in_param_edits(&mut project, &[(other, 0, 0.125)], None, &[]).0);
    assert!(project.channels[0].devices[0].params.is_empty());
}

/// A plug-in project whose device has an enabled lane on parameter 0: 0 at beat 0 up to 1 at beat 8.
fn with_lane() -> Project {
    let mut project = project();
    let mut lane = AutomationLane::new("0");
    lane.points = vec![AutomationPoint::new(0.0, 0.0), AutomationPoint::new(8.0, 1.0)].into();
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
    assert_eq!(take_in_param_edits(&mut project, &[(key, 0, 0.5)], Some((false, 4.0)), &[]), (true, false));
    assert!(enabled(&project));
    // Playing at beat 4, a report of what it played a beat ago is an echo too.
    assert!(!take_in_param_edits(&mut project, &[(key, 0, 0.375)], Some((true, 4.0)), &[]).1);
    assert!(enabled(&project));
    // A value the lane does not play around there is a move by hand: the lane goes off.
    assert_eq!(take_in_param_edits(&mut project, &[(key, 0, 0.95)], Some((false, 4.0)), &[]), (true, true));
    assert!(!enabled(&project));
    assert_eq!(project.channels[0].devices[0].params[0].value, 0.95, "the value set by hand holds");
    // Without a player nothing sends: any edit is by hand.
    let mut project = with_lane();
    assert!(take_in_param_edits(&mut project, &[(key, 0, 0.5)], None, &[]).1);
    // Another parameter leaves the lane alone.
    let mut project = with_lane();
    assert!(!take_in_param_edits(&mut project, &[(key, 1, 0.95)], Some((false, 4.0)), &[]).1);
    assert!(enabled(&project));
}

/// A parameter that takes steps holds each value of its lane until the next point, as the engine
/// plays it: stopped half-way between the points, the plug-in reporting the first point's value
/// is the lane's own, and the value half-way is a move by hand (#1205).
#[test]
fn a_stepped_parameters_echo_is_the_value_its_lane_holds() {
    let enabled = |p: &Project| p.channels[0].devices[0].automation[0].enabled;
    let mut project = with_lane();
    let key = suhina_engine::plugin_devices(&project)[0].key;
    let stepped = [(key, 0)];
    assert_eq!(take_in_param_edits(&mut project, &[(key, 0, 0.0)], Some((false, 4.0)), &stepped), (true, false));
    assert!(enabled(&project));
    assert!(take_in_param_edits(&mut project, &[(key, 0, 0.5)], Some((false, 4.0)), &stepped).1);
    assert!(!enabled(&project));
}

/// A parameter moved in a plug-in's own editor is reported for its track's lanes to show, once;
/// a value its lane sent there is not (#1205).
#[test]
fn a_parameter_moved_in_an_editor_is_reported_for_its_lanes() {
    let mut project = with_lane();
    // The track plays through the plug-in's channel, so its lanes are the track's.
    project.tracks[0].channel = Some(ChannelId(1));
    let key = suhina_engine::plugin_devices(&project)[0].key;
    let edits = [(key, 0, 0.95), (key, 3, 0.2), (key, 3, 0.4)];
    assert_eq!(touched_by_hand(&project, &edits, Some((false, 4.0)), &[]), [(TrackId(1), 0, 0), (TrackId(1), 0, 3)]);
    // The lane's own value at the playhead, reported back.
    assert!(touched_by_hand(&project, &[(key, 0, 0.5)], Some((false, 4.0)), &[]).is_empty());
    // A device the project no longer holds.
    assert!(touched_by_hand(&project, &[(key ^ 1, 0, 0.5)], None, &[]).is_empty());
}

/// When a dragged point's preview ends, the plug-in goes back to what the project plays at the
/// playhead: the enabled lane's value there, held for a parameter that takes steps, else the value
/// set (#1205).
#[test]
fn what_the_project_plays_on_a_plug_in_parameter_now() {
    let mut project = with_lane();
    project.tracks[0].channel = Some(ChannelId(1));
    project.channels[0].devices[0].params.push(DeviceParam::new("3", 0.25));
    let session = crate::Session::new(project);
    assert_eq!(session.plugin_param_now(TrackId(1), 0, 0, 4.0, false), Some(0.5), "the lane at beat 4");
    assert_eq!(session.plugin_param_now(TrackId(1), 0, 0, 4.0, true), Some(0.0), "held until the next point");
    assert_eq!(session.plugin_param_now(TrackId(1), 0, 3, 4.0, false), Some(0.25), "no lane: the value set");
    assert_eq!(session.plugin_param_now(TrackId(1), 0, 7, 4.0, false), None);
    assert_eq!(session.plugin_param_now(TrackId(1), 5, 0, 4.0, false), None, "no such device");
}
