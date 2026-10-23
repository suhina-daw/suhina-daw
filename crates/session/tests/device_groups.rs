// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device groups through the commands (docs/spec/device-groups.md › Commands): device paths,
//! grouping, ungrouping, moving, duplicating, branches, the instrument rules and undo.

use serde_json::json;
use suhina_model::*;
use suhina_session::Session;

use crate::support::run;

fn project(notes: bool) -> Project {
    Project {
        tracks: vec![Track {
            content: ContentKinds { notes, audio: !notes },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Keys")
        }],
        channels: vec![Channel::new(ChannelId(1), "C1")],
        ..Project::default()
    }
}

fn add(session: &mut Session, id: &str) {
    let added = run(session, "device.add", json!({ "track": 1, "id": id }));
    assert!(added.is_ok(), "{added:?}");
}

fn names(devices: &[Device]) -> Vec<&str> {
    devices.iter().map(|d| d.name.as_str()).collect()
}

fn devices(session: &Session) -> &[Device] {
    &session.project().channels[0].devices
}

fn undo_all(session: &mut Session, steps: usize) {
    for _ in 0..steps {
        let undone = run(session, "edit.undo", json!({}));
        assert!(undone.is_ok(), "{undone:?}");
    }
}

#[test]
fn devices_are_grouped_and_ungrouped_with_undo() {
    let mut session = Session::new(project(false));
    for id in ["utility", "overdrive", "compressor", "limiter"] {
        add(&mut session, id);
    }
    let before = session.project().clone();
    let grouped =
        run(&mut session, "device.group", json!({ "track": 1, "device": 1, "count": 2, "name": "Crush" })).unwrap();
    assert_eq!(grouped["device"], 1);
    assert_eq!(names(devices(&session)), ["Utility", "Crush", "Limiter"]);
    let group = devices(&session)[1].as_group().cloned().unwrap();
    assert_eq!(group.branches.len(), 1);
    assert_eq!(names(&group.branches[0].devices), ["Overdrive", "Compressor"]);

    // Paths reach inside: switch the compressor off, set a parameter in the group.
    run(&mut session, "device.setEnabled", json!({ "track": 1, "device": "1/0/1", "enabled": false })).unwrap();
    run(&mut session, "device.setParam", json!({ "track": 1, "device": "1/0/0", "param": "drive", "value": 0.9 }))
        .unwrap();
    let inner = device_at(devices(&session), &[1, 0, 0]).unwrap();
    assert_eq!(inner.params, [DeviceParam::new("drive", 0.9)]);
    assert!(!device_at(devices(&session), &[1, 0, 1]).unwrap().enabled);
    // The group's own parameters.
    let mix =
        run(&mut session, "device.setParam", json!({ "track": 1, "device": 1, "param": "mix", "value": 2 })).unwrap();
    assert_eq!(mix["value"], 1.0, "limited to 0..1");
    assert!(
        run(&mut session, "device.setParam", json!({ "track": 1, "device": 1, "param": "width", "value": 1 })).is_err()
    );

    run(&mut session, "device.ungroup", json!({ "track": 1, "device": 1 })).unwrap();
    assert_eq!(names(devices(&session)), ["Utility", "Overdrive", "Compressor", "Limiter"]);
    undo_all(&mut session, 5);
    assert_eq!(session.project(), &before);
}

#[test]
fn track_devices_lists_branches_with_paths() {
    let mut session = Session::new(project(false));
    add(&mut session, "utility");
    add(&mut session, "overdrive");
    run(&mut session, "device.group", json!({ "track": 1, "device": 1 })).unwrap();
    run(&mut session, "branch.add", json!({ "track": 1, "device": 1, "name": "Dry" })).unwrap();
    let listed = run(&mut session, "track.devices", json!({ "track": 1 })).unwrap();
    let group = &listed[1];
    assert_eq!(group["kind"], "group");
    assert_eq!(group["path"], 1);
    assert_eq!(group["branches"][0]["devices"][0]["path"], "1/0/0");
    assert_eq!(group["branches"][0]["devices"][0]["name"], "Overdrive");
    assert_eq!(group["branches"][1]["name"], "Dry");
    assert_eq!(group["macros"].as_array().map(Vec::len), Some(8));
    let mix = group["params"].as_array().unwrap().iter().find(|p| p["id"] == "mix").unwrap();
    assert_eq!(mix["value"], 1.0);
}

#[test]
fn branches_are_added_set_duplicated_and_removed() {
    let mut session = Session::new(project(false));
    run(&mut session, "device.addGroup", json!({ "track": 1 })).unwrap();
    let before = session.project().clone();
    run(&mut session, "device.add", json!({ "track": 1, "id": "overdrive", "position": "0/0/0" })).unwrap();
    run(&mut session, "branch.add", json!({ "track": 1, "device": 0 })).unwrap();
    run(&mut session, "branch.setVolume", json!({ "track": 1, "device": 0, "branch": 1, "db": -6.0206 })).unwrap();
    run(&mut session, "branch.setPan", json!({ "track": 1, "device": 0, "branch": 1, "pan": -0.5 })).unwrap();
    run(&mut session, "branch.setMute", json!({ "track": 1, "device": 0, "branch": 0, "mute": true })).unwrap();
    run(&mut session, "branch.setSolo", json!({ "track": 1, "device": 0, "branch": 1, "solo": true })).unwrap();
    run(&mut session, "branch.rename", json!({ "track": 1, "device": 0, "branch": 1, "name": "Quiet" })).unwrap();
    run(&mut session, "branch.duplicate", json!({ "track": 1, "device": 0, "branch": 0 })).unwrap();
    let group = devices(&session)[0].as_group().cloned().unwrap();
    assert_eq!(group.branches.len(), 3);
    assert_eq!(names(&group.branches[1].devices), ["Overdrive"], "the copy holds its devices");
    let quiet = &group.branches[2];
    assert_eq!(quiet.name, "Quiet");
    assert!((quiet.volume - 0.5).abs() < 1e-4 && quiet.pan == -0.5 && quiet.solo);
    assert!(group.branches[0].mute);
    assert!(run(&mut session, "branch.setPan", json!({ "track": 1, "device": 0, "branch": 0, "pan": 2 })).is_err());
    assert!(
        run(&mut session, "branch.setMute", json!({ "track": 1, "device": 0, "branch": 9, "mute": true })).is_err()
    );
    run(&mut session, "branch.remove", json!({ "track": 1, "device": 0, "branch": 0 })).unwrap();
    run(&mut session, "branch.remove", json!({ "track": 1, "device": 0, "branch": 0 })).unwrap();
    let last = run(&mut session, "branch.remove", json!({ "track": 1, "device": 0, "branch": 0 }));
    assert!(last.unwrap_err().contains("at least one branch"));
    undo_all(&mut session, 10);
    assert_eq!(session.project(), &before);
}

#[test]
fn devices_move_into_out_of_and_between_branches() {
    let mut session = Session::new(project(false));
    for id in ["utility", "overdrive", "limiter"] {
        add(&mut session, id);
    }
    run(&mut session, "device.addGroup", json!({ "track": 1, "position": 1 })).unwrap();
    run(&mut session, "branch.add", json!({ "track": 1, "device": 1 })).unwrap();
    // [Utility, Group, Overdrive, Limiter]: the overdrive goes into branch 1, the limiter too.
    let moved = run(&mut session, "device.move", json!({ "track": 1, "device": 2, "to": "1/1/0" })).unwrap();
    assert_eq!(moved["device"], "1/1/0");
    run(&mut session, "device.move", json!({ "track": 1, "device": 2, "to": "1/1/1" })).unwrap();
    assert_eq!(names(devices(&session)), ["Utility", "Group"]);
    // Between branches, then out of the group to the front.
    run(&mut session, "device.move", json!({ "track": 1, "device": "1/1/1", "to": "1/0/0" })).unwrap();
    let group = devices(&session)[1].as_group().cloned().unwrap();
    assert_eq!(names(&group.branches[0].devices), ["Limiter"]);
    assert_eq!(names(&group.branches[1].devices), ["Overdrive"]);
    let out = run(&mut session, "device.move", json!({ "track": 1, "device": "1/1/0", "to": 0 })).unwrap();
    assert_eq!(out["device"], 0);
    assert_eq!(names(devices(&session)), ["Overdrive", "Utility", "Group"]);
    // Along the chain, after itself.
    run(&mut session, "device.move", json!({ "track": 1, "device": 0, "to": 2 })).unwrap();
    assert_eq!(names(devices(&session)), ["Utility", "Overdrive", "Group"]);
    // A group never goes into itself.
    let refused = run(&mut session, "device.move", json!({ "track": 1, "device": 2, "to": "2/0/0" }));
    assert!(refused.unwrap_err().contains("into itself"));
}

#[test]
fn a_mapping_follows_its_device_and_goes_when_it_leaves_the_group() {
    let mut p = project(false);
    let branch = Branch {
        devices: vec![
            Device::new("Overdrive", DeviceKind::Builtin("overdrive".into())),
            Device::new("Limiter", DeviceKind::Builtin("limiter".into())),
        ],
        ..Branch::new("A")
    };
    let mut group = DeviceGroup::with_branches(vec![branch]);
    group.macros[0].mappings.push(MacroMapping { device: vec![0, 1], param: "ceiling".into(), min: 0.0, max: 1.0 });
    group.macros[1].mappings.push(MacroMapping { device: vec![0, 0], param: "drive".into(), min: 0.0, max: 1.0 });
    p.channels[0].devices = vec![Device::group("G", group)];
    let mut session = Session::new(p);
    // The limiter moves to the front of its branch: its mapping follows, the overdrive's too.
    run(&mut session, "device.move", json!({ "track": 1, "device": "0/0/1", "to": "0/0/0" })).unwrap();
    let g = devices(&session)[0].as_group().cloned().unwrap();
    assert_eq!(g.macros[0].mappings[0].device, vec![0, 0]);
    assert_eq!(g.macros[1].mappings[0].device, vec![0, 1]);
    // The overdrive leaves the group: its mapping goes.
    run(&mut session, "device.move", json!({ "track": 1, "device": "0/0/1", "to": 1 })).unwrap();
    let g = devices(&session)[0].as_group().cloned().unwrap();
    assert_eq!(g.macros[0].mappings.len(), 1);
    assert!(g.macros[1].mappings.is_empty());
}

#[test]
fn a_device_is_duplicated_with_everything_in_it() {
    let mut session = Session::new(project(false));
    add(&mut session, "overdrive");
    run(&mut session, "device.group", json!({ "track": 1, "device": 0 })).unwrap();
    run(&mut session, "device.duplicate", json!({ "track": 1, "device": 0 })).unwrap();
    assert_eq!(devices(&session).len(), 2);
    assert_eq!(devices(&session)[0], devices(&session)[1]);
    run(&mut session, "device.duplicate", json!({ "track": 1, "device": "1/0/0" })).unwrap();
    assert_eq!(names(&devices(&session)[1].as_group().unwrap().branches[0].devices), ["Overdrive", "Overdrive"]);
}

#[test]
fn several_devices_are_duplicated_only_from_one_chain_or_branch() {
    let mut session = Session::new(project(false));
    for id in ["utility", "overdrive", "compressor"] {
        add(&mut session, id);
    }
    run(&mut session, "device.group", json!({ "track": 1, "device": 1, "count": 2 })).unwrap();
    // The chain is now Utility and a group; the top-level device and one inside the group are
    // in different places, and neither is copied.
    let before = session.project().clone();
    let mixed = run(&mut session, "device.duplicateMany", json!({ "track": 1, "devices": [0, "1/0/1"] }));
    assert!(mixed.is_err(), "{mixed:?}");
    assert_eq!(session.project(), &before, "nothing changed");
    // Two devices of the same branch are copied into it.
    let made = run(&mut session, "device.duplicateMany", json!({ "track": 1, "devices": ["1/0/0", "1/0/1"] })).unwrap();
    assert_eq!(made["devices"], json!(["1/0/2", "1/0/3"]));
    assert_eq!(devices(&session).len(), 2);
    assert_eq!(
        names(&devices(&session)[1].as_group().unwrap().branches[0].devices),
        ["Overdrive", "Compressor", "Overdrive", "Compressor"]
    );
}

#[test]
fn instruments_in_groups_follow_the_chain_rules() {
    let mut session = Session::new(project(true));
    add(&mut session, "fm-synth");
    add(&mut session, "overdrive");
    // The instrument and its effect make an instrument group, first in the chain.
    run(&mut session, "device.group", json!({ "track": 1, "device": 0, "count": 2 })).unwrap();
    run(&mut session, "branch.add", json!({ "track": 1, "device": 0 })).unwrap();
    // Layering: a second instrument goes first in the new branch.
    run(&mut session, "device.add", json!({ "track": 1, "id": "fm-synth", "position": "0/1/0" })).unwrap();
    // An effect cannot start a branch of an instrument group, and an instrument cannot follow
    // an effect in a branch.
    assert!(run(&mut session, "branch.add", json!({ "track": 1, "device": 0 })).is_ok());
    let effect_first = run(&mut session, "device.add", json!({ "track": 1, "id": "utility", "position": "0/2/0" }));
    assert!(effect_first.unwrap_err().contains("instrument first"));
    let late = run(&mut session, "device.add", json!({ "track": 1, "id": "fm-synth", "position": "0/0/2" }));
    assert!(late.unwrap_err().contains("goes first in its branch"));
    // A second instrument on the track, outside the group, is refused.
    let second = run(&mut session, "device.add", json!({ "track": 1, "id": "fm-synth", "position": 1 }));
    assert!(second.is_err());
    // An instrument group stays first: moving an effect in front of it is refused.
    add(&mut session, "utility");
    let in_front = run(&mut session, "device.move", json!({ "track": 1, "device": 1, "to": 0 }));
    assert!(in_front.unwrap_err().contains("goes first"));
}

#[test]
fn groups_nest_four_deep_and_a_track_holds_sixty_four_devices() {
    let mut session = Session::new(project(false));
    run(&mut session, "device.addGroup", json!({ "track": 1 })).unwrap();
    run(&mut session, "device.addGroup", json!({ "track": 1, "position": "0/0/0" })).unwrap();
    run(&mut session, "device.addGroup", json!({ "track": 1, "position": "0/0/0/0/0" })).unwrap();
    run(&mut session, "device.addGroup", json!({ "track": 1, "position": "0/0/0/0/0/0/0" })).unwrap();
    let fifth = run(&mut session, "device.addGroup", json!({ "track": 1, "position": "0/0/0/0/0/0/0/0/0" }));
    assert!(fifth.is_err(), "{fifth:?}");
    for _ in 0..60 {
        run(&mut session, "device.add", json!({ "track": 1, "id": "utility", "position": "0/0/0" })).unwrap();
    }
    assert_eq!(count_devices(devices(&session)), 64);
    assert!(run(&mut session, "device.add", json!({ "track": 1, "id": "utility" })).is_err());
    assert!(run(&mut session, "device.duplicate", json!({ "track": 1, "device": "0/0/0" })).is_err());
}

#[test]
fn bad_paths_are_refused_and_change_nothing() {
    let mut session = Session::new(project(false));
    add(&mut session, "utility");
    run(&mut session, "device.group", json!({ "track": 1, "device": 0 })).unwrap();
    let before = session.project().clone();
    for (id, params) in [
        ("device.setEnabled", json!({ "track": 1, "device": "0/1/0", "enabled": false })),
        ("device.setEnabled", json!({ "track": 1, "device": "0/0", "enabled": false })),
        ("device.setEnabled", json!({ "track": 1, "device": "x", "enabled": false })),
        ("device.remove", json!({ "track": 1, "device": "0/0/5" })),
        ("device.ungroup", json!({ "track": 1, "device": "0/0/0" })),
        ("branch.add", json!({ "track": 1, "device": "0/0/0" })),
        ("device.move", json!({ "track": 1, "device": 0, "to": "0/3/0" })),
        ("device.addGroup", json!({ "track": 1, "position": "0/0/9" })),
        ("device.group", json!({ "track": 1, "device": 0, "count": 3 })),
    ] {
        assert!(run(&mut session, id, params.clone()).is_err(), "{id} {params}");
        assert_eq!(session.project(), &before, "{id} {params}");
    }
    // A group of several branches is not ungrouped.
    run(&mut session, "branch.add", json!({ "track": 1, "device": 0 })).unwrap();
    assert!(run(&mut session, "device.ungroup", json!({ "track": 1, "device": 0 })).is_err());
}

#[test]
fn devices_in_groups_are_removed_copied_and_pasted_by_path() {
    let mut session = Session::new(project(false));
    for id in ["utility", "overdrive", "compressor"] {
        add(&mut session, id);
    }
    run(&mut session, "device.group", json!({ "track": 1, "device": 1, "count": 2 })).unwrap();
    run(&mut session, "device.copy", json!({ "track": 1, "devices": ["1/0/1"] })).unwrap();
    run(&mut session, "device.paste", json!({ "track": 1, "position": "1/0/0" })).unwrap();
    assert_eq!(
        names(&devices(&session)[1].as_group().unwrap().branches[0].devices),
        ["Compressor", "Overdrive", "Compressor"]
    );
    // A group and a device inside it: the group goes, once.
    run(&mut session, "device.removeMany", json!({ "track": 1, "devices": [1, "1/0/0"] })).unwrap();
    assert_eq!(names(devices(&session)), ["Utility"]);
}
