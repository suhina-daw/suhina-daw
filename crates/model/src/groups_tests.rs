// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::{Channel, DeviceParam, Project};

fn builtin(id: &str) -> Device {
    Device::new(id, DeviceKind::Builtin(id.into()))
}

fn branch(devices: Vec<Device>) -> Branch {
    Branch { devices, ..Branch::new("B") }
}

fn group(branches: Vec<Branch>) -> Device {
    Device::group("Group", DeviceGroup::with_branches(branches))
}

fn project(devices: Vec<Device>) -> Project {
    let mut channel = Channel::new(ChannelId(1), "A");
    channel.devices = devices;
    Project { channels: vec![channel], ..Project::default() }
}

fn errors(devices: Vec<Device>) -> Vec<ValidationError> {
    project(devices).validate().err().unwrap_or_default()
}

#[test]
fn a_default_group_is_one_empty_branch_and_eight_macros() {
    let group = DeviceGroup::default();
    assert_eq!(group.branches.len(), 1);
    assert!(group.branches[0].devices.is_empty());
    assert_eq!(group.macros.len(), MACROS);
    assert_eq!(group.macros[7].name, "Macro 8");
    assert_eq!(errors(vec![Device::group("G", group)]), vec![]);
}

#[test]
fn paths_find_devices_at_every_depth() {
    let inner = group(vec![branch(vec![builtin("utility")]), branch(vec![builtin("limiter")])]);
    let outer = group(vec![branch(vec![]), branch(vec![builtin("compressor"), inner])]);
    let devices = vec![builtin("dj-eq"), outer];
    assert_eq!(device_at(&devices, &[0]).map(|d| d.name.as_str()), Some("dj-eq"));
    assert_eq!(device_at(&devices, &[1, 1, 0]).map(|d| d.name.as_str()), Some("compressor"));
    assert_eq!(device_at(&devices, &[1, 1, 1, 1, 0]).map(|d| d.name.as_str()), Some("limiter"));
    assert!(device_at(&devices, &[]).is_none());
    assert!(device_at(&devices, &[1, 1]).is_none(), "a path that ends at a branch");
    assert!(device_at(&devices, &[0, 0, 0]).is_none(), "stepping into a device that is not a group");
    assert!(device_at(&devices, &[1, 5, 0]).is_none());
    let mut devices = devices;
    if let Some(d) = device_at_mut(&mut devices, &[1, 1, 1, 0, 0]) {
        d.enabled = false;
    }
    assert_eq!(device_at(&devices, &[1, 1, 1, 0, 0]).map(|d| d.enabled), Some(false));
    let paths: Vec<Vec<usize>> = walk_devices(&devices).into_iter().map(|(p, _)| p).collect();
    assert_eq!(
        paths,
        vec![vec![0], vec![1], vec![1, 1, 0], vec![1, 1, 1], vec![1, 1, 1, 0, 0], vec![1, 1, 1, 1, 0]],
        "depth-first, a group before what it holds"
    );
    assert_eq!(count_devices(&devices), 6);
    assert_eq!(group_depth(&devices), 2);
    let g = devices[1].as_group().map(|g| g.device_at(&[1, 1, 1, 0]).map(|d| d.name.clone()));
    assert_eq!(g, Some(Some("limiter".into())));
}

#[test]
fn branch_counts_outside_one_to_sixteen_are_refused() {
    let at = vec![0];
    let refused = ValidationError::GroupBranches { channel: ChannelId(1), device: at };
    assert_eq!(errors(vec![group(vec![])]), vec![refused.clone()]);
    assert_eq!(errors(vec![group((0..17).map(|_| branch(vec![])).collect())]), vec![refused]);
    assert_eq!(errors(vec![group((0..16).map(|_| branch(vec![])).collect())]), vec![]);
}

#[test]
fn a_group_without_eight_macros_is_refused() {
    let mut device = group(vec![branch(vec![])]);
    if let Some(g) = device.as_group_mut() {
        g.macros.pop();
    }
    assert_eq!(errors(vec![device]), vec![ValidationError::GroupMacros { channel: ChannelId(1), device: vec![0] }]);
    let mut device = group(vec![branch(vec![builtin("utility")])]);
    if let Some(g) = device.as_group_mut() {
        g.macros[0].mappings = (0..=MAX_MACRO_MAPPINGS)
            .map(|i| MacroMapping { device: vec![0, 0], param: format!("p{i}"), min: 0.0, max: 1.0 })
            .collect();
    }
    assert_eq!(errors(vec![device]), vec![ValidationError::GroupMacros { channel: ChannelId(1), device: vec![0] }]);
}

#[test]
fn groups_nest_four_deep_and_no_deeper() {
    let nest = |levels: usize| {
        let mut device = builtin("utility");
        for _ in 0..levels {
            device = group(vec![branch(vec![device])]);
        }
        device
    };
    assert_eq!(errors(vec![nest(MAX_DEVICE_GROUP_DEPTH)]), vec![]);
    let deepest = vec![0, 0, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(
        errors(vec![nest(MAX_DEVICE_GROUP_DEPTH + 1)]),
        vec![ValidationError::DeviceGroupTooDeep { channel: ChannelId(1), device: deepest }]
    );
}

#[test]
fn a_channel_holds_sixty_four_devices_counting_every_depth() {
    let full = group(vec![branch((0..MAX_DEVICES - 1).map(|_| builtin("utility")).collect())]);
    assert_eq!(errors(vec![full.clone()]), vec![]);
    assert_eq!(errors(vec![full, builtin("utility")]), vec![ValidationError::TooManyDevices(ChannelId(1))]);
}

#[test]
fn branch_volumes_and_pans_out_of_range_are_refused() {
    let bad = ValidationError::BadBranch { channel: ChannelId(1), device: vec![0] };
    for (volume, pan) in [(-0.1, 0.0), (f64::NAN, 0.0), (f64::INFINITY, 0.0), (1.0, 1.5), (1.0, f64::NAN)] {
        let device = group(vec![Branch { volume, pan, ..Branch::new("B") }]);
        assert_eq!(errors(vec![device]), vec![bad.clone()], "{volume} {pan}");
    }
    let fine = group(vec![Branch { volume: 0.0, pan: -1.0, ..Branch::new("B") }]);
    assert_eq!(errors(vec![fine]), vec![]);
}

#[test]
fn mappings_must_resolve_and_stay_on_the_travel() {
    let bad = vec![ValidationError::BadMacroMapping { channel: ChannelId(1), device: vec![0] }];
    let with = |device: Vec<u32>, min: f64, max: f64| {
        let mut g = group(vec![branch(vec![builtin("utility")])]);
        if let Some(group) = g.as_group_mut() {
            group.macros[0].mappings.push(MacroMapping { device, param: "gain".into(), min, max });
        }
        errors(vec![g])
    };
    assert_eq!(with(vec![0, 0], 0.8, 0.2), vec![], "an inverted mapping is fine");
    assert_eq!(with(vec![0, 1], 0.0, 1.0), bad, "no device there");
    assert_eq!(with(vec![1, 0], 0.0, 1.0), bad, "no branch there");
    assert_eq!(with(vec![0], 0.0, 1.0), bad, "a path to a branch");
    assert_eq!(with(vec![], 0.0, 1.0), bad, "an empty path");
    assert_eq!(with(vec![0, 0], -0.1, 1.0), bad);
    assert_eq!(with(vec![0, 0], 0.0, f64::NAN), bad);
}

#[test]
fn a_parameter_is_mapped_by_one_macro_only() {
    let twice = |outer_too: bool| {
        let mut inner = group(vec![branch(vec![builtin("utility")])]);
        if let Some(g) = inner.as_group_mut() {
            g.macros[0].mappings.push(MacroMapping { device: vec![0, 0], param: "gain".into(), min: 0.0, max: 1.0 });
        }
        let mut outer = group(vec![branch(vec![inner])]);
        if let Some(g) = outer.as_group_mut() {
            let param = if outer_too { "gain" } else { "width" };
            g.macros[2].mappings.push(MacroMapping {
                device: vec![0, 0, 0, 0],
                param: param.into(),
                min: 0.0,
                max: 1.0,
            });
            g.macros[3].mappings.push(MacroMapping { device: vec![0, 0], param: "macro.1".into(), min: 0.0, max: 1.0 });
        }
        errors(vec![outer])
    };
    assert_eq!(twice(false), vec![], "different parameters, and a nested group's macro");
    // The outer group is walked first, so the inner one's mapping is the second.
    let inner = vec![0, 0, 0];
    assert_eq!(twice(true), vec![ValidationError::MappedTwice { channel: ChannelId(1), device: inner }]);
}

#[test]
fn group_parameters_are_ordinary_parameters() {
    let mut device = group(vec![branch(vec![])]);
    device.params = vec![DeviceParam::new("mix", 0.5), DeviceParam::new("macro.1", 0.4)];
    assert_eq!(errors(vec![device]), vec![]);
}
