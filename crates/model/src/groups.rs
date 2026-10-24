// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device groups: a device on a channel's chain that holds other devices in parallel branches,
//! with a mix, an output gain and eight macros (`docs/spec/device-groups.md`).
//!
//! A device inside groups is found by a *device path*: its index on the channel, then for each
//! group on the way the branch and the index in it (`[2, 1, 0]` is the first device of branch 1
//! of the group at index 2). A macro mapping names its target the same way, relative to its
//! group (`[branch, index, …]`).

use crate::{ChannelId, Device, DeviceKind, Extensions, ValidationError};

/// Most branches one device group holds.
pub const MAX_BRANCHES: usize = 16;
/// How many macros every device group has.
pub const MACROS: usize = 8;
/// Most mappings one macro holds.
pub const MAX_MACRO_MAPPINGS: usize = 32;
/// How deep device groups nest: a group inside a group inside a group inside a group.
pub const MAX_DEVICE_GROUP_DEPTH: usize = 4;
/// Most devices a channel holds, counting every device at every depth, groups included.
pub const MAX_DEVICES: usize = 64;

/// The devices of a device group: its branches and macros. Its own values (`mix`, `gain`,
/// `macro.1` … `macro.8`) are ordinary parameters of the [`Device`] that holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceGroup {
    /// At least one, at most [`MAX_BRANCHES`].
    pub branches: Vec<Branch>,
    /// Always [`MACROS`]; a macro without mappings is still there (its name and value are kept).
    pub macros: Vec<Macro>,
}

/// One parallel path inside a device group: a serial chain of devices with its own volume, pan,
/// mute and solo.
#[derive(Debug, Clone, PartialEq)]
pub struct Branch {
    pub name: String,
    /// Linear gain; 1.0 is unity.
    pub volume: f64,
    /// -1.0 hard left … 1.0 hard right.
    pub pan: f64,
    pub mute: bool,
    pub solo: bool,
    pub devices: Vec<Device>,
    pub extensions: Extensions,
}

/// A knob of a device group that moves parameters of devices inside it.
#[derive(Debug, Clone, PartialEq)]
pub struct Macro {
    pub name: String,
    pub mappings: Vec<MacroMapping>,
}

/// A parameter a macro moves, and over which part of its travel.
#[derive(Debug, Clone, PartialEq)]
pub struct MacroMapping {
    /// The target device, as a path relative to the group: branch, index, then branch, index
    /// again for each nested group (`[2, 0]` is branch 2's first device).
    pub device: Vec<u32>,
    /// The parameter's stable id: a built-in parameter id (ADR 0004), a plug-in parameter id as
    /// text, or `macro.N` of a nested group.
    pub param: String,
    /// Positions on the parameter's 0..1 travel; `min > max` inverts the mapping.
    pub min: f64,
    pub max: f64,
}

impl Default for DeviceGroup {
    /// One empty branch (a clean passthrough) and eight unmapped macros.
    fn default() -> Self {
        Self::with_branches(vec![Branch::new("Branch 1")])
    }
}

impl DeviceGroup {
    /// A group of `branches`, with eight unmapped macros.
    pub fn with_branches(branches: Vec<Branch>) -> Self {
        Self { branches, macros: (1..=MACROS).map(Macro::new).collect() }
    }

    /// The device at `path` relative to this group (`[branch, index, branch, index, …]`).
    pub fn device_at(&self, path: &[u32]) -> Option<&Device> {
        fn step(branches: &[Branch], branch: u32, index: u32) -> Option<&Device> {
            branches.get(usize::try_from(branch).ok()?)?.devices.get(usize::try_from(index).ok()?)
        }
        let [branch, index, rest @ ..] = path else { return None };
        let mut device = step(&self.branches, *branch, *index)?;
        let mut rest = rest;
        while let [branch, index, more @ ..] = rest {
            device = step(&device.as_group()?.branches, *branch, *index)?;
            rest = more;
        }
        rest.is_empty().then_some(device)
    }
}

impl Branch {
    /// An empty branch at unity, centred.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            devices: Vec::new(),
            extensions: Extensions::default(),
        }
    }
}

impl Macro {
    /// Macro `number` (1 … 8) with its default name and no mappings.
    pub fn new(number: usize) -> Self {
        Self { name: format!("Macro {number}"), mappings: Vec::new() }
    }
}

impl Device {
    /// A device group named `name`, switched on, with its parameters at their defaults.
    pub fn group(name: impl Into<String>, group: DeviceGroup) -> Self {
        Self::new(name, DeviceKind::Group(Box::new(group)))
    }

    /// The group this device is, if it is one.
    pub fn as_group(&self) -> Option<&DeviceGroup> {
        match &self.kind {
            DeviceKind::Group(group) => Some(group),
            _ => None,
        }
    }

    /// The group this device is, if it is one, to change.
    pub fn as_group_mut(&mut self) -> Option<&mut DeviceGroup> {
        match &mut self.kind {
            DeviceKind::Group(group) => Some(group),
            _ => None,
        }
    }
}

/// How many devices `devices` hold, counting every device at every depth, groups included.
pub fn count_devices(devices: &[Device]) -> usize {
    devices
        .iter()
        .map(|d| 1 + d.as_group().map_or(0, |g| g.branches.iter().map(|b| count_devices(&b.devices)).sum()))
        .sum()
}

/// How many levels of device groups `devices` hold: 0 with none, 1 with groups of plain devices.
pub fn group_depth(devices: &[Device]) -> usize {
    devices
        .iter()
        .filter_map(Device::as_group)
        .map(|g| 1 + g.branches.iter().map(|b| group_depth(&b.devices)).max().unwrap_or(0))
        .max()
        .unwrap_or(0)
}

/// Whether any of `devices`, at any depth, is a device group.
pub fn has_groups(devices: &[Device]) -> bool {
    devices.iter().any(|d| d.as_group().is_some())
}

/// The device at `path` among `devices` (a channel's): its index there, then branch and index
/// for each group on the way. `None` for an empty path, one of even length (it ends at a branch)
/// or one that leaves the tree.
pub fn device_at<'a>(devices: &'a [Device], path: &[usize]) -> Option<&'a Device> {
    let (&first, mut rest) = path.split_first()?;
    let mut device = devices.get(first)?;
    while let [branch, index, more @ ..] = rest {
        device = device.as_group()?.branches.get(*branch)?.devices.get(*index)?;
        rest = more;
    }
    rest.is_empty().then_some(device)
}

/// The device at `path` among `devices`, to change; see [`device_at`].
pub fn device_at_mut<'a>(devices: &'a mut [Device], path: &[usize]) -> Option<&'a mut Device> {
    let (&first, mut rest) = path.split_first()?;
    let mut device = devices.get_mut(first)?;
    while let [branch, index, more @ ..] = rest {
        device = device.as_group_mut()?.branches.get_mut(*branch)?.devices.get_mut(*index)?;
        rest = more;
    }
    rest.is_empty().then_some(device)
}

/// Every device among `devices` with its path, depth-first: a group comes before the devices
/// in it, a branch's devices before the next branch's.
pub fn walk_devices(devices: &[Device]) -> Vec<(Vec<usize>, &Device)> {
    fn walk<'a>(devices: &'a [Device], prefix: &[usize], out: &mut Vec<(Vec<usize>, &'a Device)>) {
        for (index, device) in devices.iter().enumerate() {
            let mut path = prefix.to_vec();
            path.push(index);
            out.push((path.clone(), device));
            if let Some(group) = device.as_group() {
                for (b, branch) in group.branches.iter().enumerate() {
                    let mut inner = path.clone();
                    inner.push(b);
                    walk(&branch.devices, &inner, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(devices, &[], &mut out);
    out
}

/// Every problem with the device groups among `channel`'s `devices`, as validation errors.
pub(crate) fn validate_devices(channel: ChannelId, devices: &[Device], errors: &mut Vec<ValidationError>) {
    if count_devices(devices) > MAX_DEVICES {
        errors.push(ValidationError::TooManyDevices(channel));
    }
    // Every mapped parameter of the channel by its absolute path, so a parameter mapped by an
    // outer and an inner group's macro both is caught too.
    let mut mapped: Vec<(Vec<usize>, &str)> = Vec::new();
    for (path, device) in walk_devices(devices) {
        let Some(group) = device.as_group() else { continue };
        let at = |error: fn(ChannelId, Vec<usize>) -> ValidationError| error(channel, path.clone());
        // A group's own depth: one for itself and one for each branch it sits in on the way.
        if path.len() / 2 + 1 > MAX_DEVICE_GROUP_DEPTH {
            errors.push(at(|c, d| ValidationError::DeviceGroupTooDeep { channel: c, device: d }));
        }
        if group.branches.is_empty() || group.branches.len() > MAX_BRANCHES {
            errors.push(at(|c, d| ValidationError::GroupBranches { channel: c, device: d }));
        }
        let bad_branch = |b: &Branch| {
            !b.volume.is_finite() || b.volume < 0.0 || !b.pan.is_finite() || !(-1.0..=1.0).contains(&b.pan)
        };
        if group.branches.iter().any(bad_branch) {
            errors.push(at(|c, d| ValidationError::BadBranch { channel: c, device: d }));
        }
        if group.macros.len() != MACROS || group.macros.iter().any(|m| m.mappings.len() > MAX_MACRO_MAPPINGS) {
            errors.push(at(|c, d| ValidationError::GroupMacros { channel: c, device: d }));
        }
        let travel = |v: f64| v.is_finite() && (0.0..=1.0).contains(&v);
        let mut bad_mapping = false;
        let mut twice = false;
        for mapping in group.macros.iter().flat_map(|m| &m.mappings) {
            let resolves = mapping.device.len() % 2 == 0 && group.device_at(&mapping.device).is_some();
            if !resolves || !travel(mapping.min) || !travel(mapping.max) {
                bad_mapping = true;
                continue;
            }
            let mut target = path.clone();
            target.extend(mapping.device.iter().map(|&i| i as usize));
            if mapped.iter().any(|(p, param)| *p == target && *param == mapping.param) {
                twice = true;
            } else {
                mapped.push((target, &mapping.param));
            }
        }
        if bad_mapping {
            errors.push(at(|c, d| ValidationError::BadMacroMapping { channel: c, device: d }));
        }
        if twice {
            errors.push(at(|c, d| ValidationError::MappedTwice { channel: c, device: d }));
        }
    }
}

#[cfg(test)]
#[path = "groups_tests.rs"]
mod tests;
