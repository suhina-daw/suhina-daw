// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device groups on tracks (`docs/spec/device-groups.md` › Commands): device paths, grouping and
//! ungrouping, moving and duplicating devices through groups, and the branches' settings. Each
//! edit is one undo step.
//!
//! A *device path* names a device inside groups: a whole number is a device on the track (every
//! call from before groups keeps working), text such as `"2/1/0"` the device at index 2 on the
//! track (a group), its branch 1, device 0 there. A *position* path is the same with the last
//! number the place to insert before: `"2/1/3"` is branch 1 of the group at 2, before its device 3.

use serde_json::{Value, json};
use suhina_device_api::Role;
use suhina_model::{Branch, Device, DeviceGroup, DeviceKind, MAX_BRANCHES, device_at, device_at_mut};

use crate::commands::{device_channel, device_channel_read, number, track_or_main_id};
use crate::{CommandError, Session};

/// The longest path a command takes: a device inside four groups.
const MAX_PATH: usize = 1 + 2 * suhina_model::MAX_DEVICE_GROUP_DEPTH;

/// A path from a parameter's value: a whole number, or text of whole numbers between `/`.
pub(crate) fn parse_path(value: &Value) -> Option<Vec<usize>> {
    if let Some(n) = value.as_u64() {
        return usize::try_from(n).ok().map(|n| vec![n]);
    }
    let text = value.as_str()?.trim();
    let path: Option<Vec<usize>> = text.split('/').map(|part| part.trim().parse::<usize>().ok()).collect();
    path.filter(|p| !p.is_empty() && p.len() <= MAX_PATH)
}

/// A device path as the commands write it: `2/1/0`; a top-level device as its number.
pub(crate) fn path_value(path: &[usize]) -> Value {
    match path {
        [index] => json!(index),
        _ => json!(path.iter().map(ToString::to_string).collect::<Vec<_>>().join("/")),
    }
}

const PATH_HELP: &str =
    "a device index from 0, or a path such as \"2/1/0\" (device 2, its branch 1, device 0 there), as in track.devices";

/// The device path in `params[key]`, checked against `devices`.
pub(crate) fn device_path(devices: &[Device], params: &Value, key: &str) -> Result<Vec<usize>, CommandError> {
    let path = params.get(key).and_then(parse_path).ok_or_else(|| format!("{key} must be {PATH_HELP}"))?;
    if path.len() % 2 == 0 || device_at(devices, &path).is_none() {
        let shown = path.iter().map(ToString::to_string).collect::<Vec<_>>().join("/");
        return Err(format!("the track has no device {shown}; track.devices lists them").into());
    }
    Ok(path)
}

/// The list of devices a path's parent names: the track's for `[]`, a group's branch for
/// `[group…, branch]`.
pub(crate) fn container_mut<'a>(devices: &'a mut Vec<Device>, parent: &[usize]) -> Option<&'a mut Vec<Device>> {
    let Some((&branch, group)) = parent.split_last() else { return Some(devices) };
    let group = device_at_mut(devices, group)?.as_group_mut()?;
    group.branches.get_mut(branch).map(|b| &mut b.devices)
}

pub(crate) fn container<'a>(devices: &'a [Device], parent: &[usize]) -> Option<&'a [Device]> {
    let Some((&branch, group)) = parent.split_last() else { return Some(devices) };
    let group = device_at(devices, group)?.as_group()?;
    group.branches.get(branch).map(|b| b.devices.as_slice())
}

/// A position path in `params[key]`: where to insert, as (the parent, the index in it), the
/// index at most the parent's length. `None` when the parameter is left out.
pub(crate) fn position_path(
    devices: &[Device],
    params: &Value,
    key: &str,
) -> Result<Option<(Vec<usize>, usize)>, CommandError> {
    let Some(value) = params.get(key).filter(|v| !v.is_null()) else { return Ok(None) };
    let path =
        parse_path(value).ok_or_else(|| format!("{key} must be a position from 0, or a path such as \"2/1/3\""))?;
    let Some((&index, parent)) = path.split_last() else { return Err(format!("{key} is empty").into()) };
    let list = (parent.len() % 2 == 0).then(|| container(devices, parent)).flatten();
    match list {
        Some(list) if index <= list.len() => Ok(Some((parent.to_vec(), index))),
        Some(list) => Err(format!("{key} must be from 0 to {} there", list.len()).into()),
        None => Err(format!("{key} does not name a place on the track or in a group's branch").into()),
    }
}

/// The group at the path `params.device` names.
fn group_path(devices: &[Device], params: &Value) -> Result<Vec<usize>, CommandError> {
    let path = device_path(devices, params, "device")?;
    if device_at(devices, &path).and_then(Device::as_group).is_none() {
        return Err("the device is not a device group".into());
    }
    Ok(path)
}

/// The `branch` index parameter, checked against `group`.
fn branch_index(group: &DeviceGroup, params: &Value) -> Result<usize, CommandError> {
    let index = params
        .get("branch")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or("branch must be a branch index, from 0, as in track.devices")?;
    if index >= group.branches.len() {
        return Err(format!("the group has no branch {index}; it has {}", group.branches.len()).into());
    }
    Ok(index)
}

/// What a device is to the rules of a chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Instrument,
    MidiEffect,
    Effect,
}

/// What `device` is: an instrument (a group whose branches start with one counts), a MIDI
/// effect or an effect (everything else, unknown and missing devices included).
pub(crate) fn kind_of(session: &Session, device: &Device) -> Kind {
    match &device.kind {
        DeviceKind::Builtin(id) => match suhina_device_registry::info(id).map(|info| info.role) {
            Some(Role::Instrument) => Kind::Instrument,
            Some(Role::MidiEffect) => Kind::MidiEffect,
            _ => Kind::Effect,
        },
        DeviceKind::Sd => match crate::devices::sd_graph(device).map(|g| g.role) {
            Ok(suhina_device_sdk::DeviceRole::Instrument) => Kind::Instrument,
            Ok(suhina_device_sdk::DeviceRole::MidiEffect) => Kind::MidiEffect,
            _ => Kind::Effect,
        },
        DeviceKind::Clap { .. } | DeviceKind::Vst3 { .. } | DeviceKind::Vst2 { .. } => {
            let found = session.plugins.env().and_then(|env| crate::plugins::device_plugin(&env.catalog, device));
            if found.is_some_and(|(m, _)| crate::is_instrument(m.plugin)) { Kind::Instrument } else { Kind::Effect }
        }
        DeviceKind::Group(group) => {
            let starts = group.branches.iter().filter_map(|b| b.devices.first());
            if starts.into_iter().any(|d| kind_of(session, d) == Kind::Instrument) {
                Kind::Instrument
            } else {
                Kind::Effect
            }
        }
        DeviceKind::Foreign { .. } => Kind::Effect,
    }
}

/// Checks the rules of a chain (`docs/spec/device-groups.md` › Instruments in a group): on the
/// track, MIDI effects first, then at most one instrument (a device or an instrument group),
/// then effects; in a group, no MIDI effects, and either no instrument at all (an effect group)
/// or an instrument first in every branch that is not empty and none after it.
pub(crate) fn check_chain(session: &Session, devices: &[Device], plays_notes: bool) -> Result<(), CommandError> {
    let kinds: Vec<Kind> = devices.iter().map(|d| kind_of(session, d)).collect();
    let midi = kinds.iter().take_while(|k| **k == Kind::MidiEffect).count();
    if kinds.iter().skip(midi).any(|k| *k == Kind::MidiEffect) {
        return Err("MIDI effects go first in a track's chain".into());
    }
    if !plays_notes && midi > 0 {
        return Err("a MIDI effect goes on a MIDI track".into());
    }
    let instruments: Vec<usize> =
        kinds.iter().enumerate().filter(|(_, k)| **k == Kind::Instrument).map(|(i, _)| i).collect();
    if instruments.len() > 1 {
        return Err("a track has one instrument (or one instrument group)".into());
    }
    if let Some(&at) = instruments.first() {
        if !plays_notes {
            return Err("an instrument goes on a MIDI track".into());
        }
        if at != midi {
            return Err("the instrument (or instrument group) goes first in the chain, after the MIDI effects".into());
        }
    }
    for group in devices.iter().filter_map(Device::as_group) {
        check_group(session, group)?;
    }
    Ok(())
}

fn check_group(session: &Session, group: &DeviceGroup) -> Result<(), CommandError> {
    let mut instrument_group = false;
    for branch in &group.branches {
        for (index, device) in branch.devices.iter().enumerate() {
            match kind_of(session, device) {
                Kind::MidiEffect => return Err("a MIDI effect cannot go into a device group".into()),
                Kind::Instrument if index > 0 => {
                    return Err("in a device group an instrument goes first in its branch".into());
                }
                Kind::Instrument => instrument_group = true,
                Kind::Effect => {}
            }
        }
    }
    if instrument_group
        && group.branches.iter().filter_map(|b| b.devices.first()).any(|d| kind_of(session, d) != Kind::Instrument)
    {
        return Err("an instrument group has an instrument first in every branch that is not empty".into());
    }
    for inner in group.branches.iter().flat_map(|b| &b.devices).filter_map(Device::as_group) {
        check_group(session, inner)?;
    }
    Ok(())
}

/// Whether the track the parameters name plays notes.
fn plays_notes(session: &Session, params: &Value) -> Result<bool, CommandError> {
    let track = track_or_main_id(&session.project, params)?;
    Ok(session.project.track(track).is_some_and(|t| t.content.notes))
}

/// Applies `edit` to a copy of the track's devices, checks the chain's rules on the result and
/// keeps it only when they hold, so a refused edit changes nothing.
pub(crate) fn edit_chain<T>(
    session: &mut Session,
    params: &Value,
    edit: impl FnOnce(&mut Vec<Device>) -> Result<T, CommandError>,
) -> Result<T, CommandError> {
    let notes = plays_notes(session, params)?;
    let mut devices = device_channel_read(session, params)?.devices.clone();
    let out = edit(&mut devices)?;
    if suhina_model::count_devices(&devices) > suhina_model::MAX_DEVICES {
        return Err(
            format!("a track holds at most {} devices, counting those in groups", suhina_model::MAX_DEVICES).into()
        );
    }
    if suhina_model::device_group_depth(&devices) > suhina_model::MAX_DEVICE_GROUP_DEPTH {
        return Err(format!("device groups nest at most {} deep", suhina_model::MAX_DEVICE_GROUP_DEPTH).into());
    }
    check_chain(session, &devices, notes)?;
    device_channel(session, params)?.devices = devices;
    Ok(out)
}

/// Adds `device` at the `position` path when it is inside a group (`"2/1/3"`), with the group
/// rules checked; `None` when the position is on the track itself, which the adding command
/// handles as before.
pub(crate) fn insert_into_group(
    session: &mut Session,
    params: &Value,
    device: Device,
) -> Result<Option<Value>, CommandError> {
    if params.get("position").and_then(parse_path).is_none_or(|p| p.len() < 2) {
        return Ok(None);
    }
    edit_chain(session, params, |devices| {
        let (parent, index) = position_path(devices, params, "position")?.ok_or("position is missing")?;
        container_mut(devices, &parent).ok_or("the position is missing")?.insert(index, device);
        let mut path = parent;
        path.push(index);
        Ok(Some(path_value(&path)))
    })
}

/// `device.group {track, device, count?, name?}`: puts `count` consecutive devices from the
/// one at `device` into a new group of one branch, in the place of the first.
pub(crate) fn group(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let count = match params.get("count") {
        None | Some(Value::Null) => 1,
        Some(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n >= 1)
            .ok_or("count must be a whole number from 1")?,
    };
    let name = params.get("name").and_then(Value::as_str).map(str::to_owned);
    edit_chain(session, params, |devices| {
        let path = device_path(devices, params, "device")?;
        let (&first, parent) = path.split_last().ok_or("the device path is empty")?;
        let list = container_mut(devices, parent).ok_or("the device is missing")?;
        if first + count > list.len() {
            return Err(format!("there are only {} devices from there", list.len() - first).into());
        }
        let moved: Vec<Device> = list.drain(first..first + count).collect();
        let name = name.unwrap_or_else(|| {
            if count == 1 { moved.first().map_or_else(String::new, |d| d.name.clone()) } else { "Group".into() }
        });
        let branch = Branch { devices: moved, ..Branch::new("Branch 1") };
        list.insert(first, Device::group(name.clone(), DeviceGroup::with_branches(vec![branch])));
        Ok(json!({ "device": path_value(&path), "name": name, "devices": count }))
    })
}

/// `device.ungroup {track, device}`: puts the devices of a group's one branch in its place.
pub(crate) fn ungroup(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    edit_chain(session, params, |devices| {
        let path = group_path(devices, params)?;
        let (&index, parent) = path.split_last().ok_or("the device path is empty")?;
        let list = container_mut(devices, parent).ok_or("the device is missing")?;
        let group = list.get(index).and_then(Device::as_group).ok_or("the device is not a device group")?;
        if group.branches.len() > 1 {
            return Err("only a group of one branch can be ungrouped; its branches' sum is not a serial chain".into());
        }
        let removed = list.remove(index);
        let inner: Vec<Device> = match removed.kind {
            DeviceKind::Group(group) => group.branches.into_iter().flat_map(|b| b.devices).collect(),
            _ => Vec::new(),
        };
        let count = inner.len();
        for (offset, device) in inner.into_iter().enumerate() {
            list.insert(index + offset, device);
        }
        Ok(json!({ "device": path_value(&path), "devices": count }))
    })
}

/// `device.addGroup {track, position?, name?}`: an empty effect group with one empty branch.
pub(crate) fn add_group(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("Group").to_owned();
    edit_chain(session, params, |devices| {
        let (parent, index) = match position_path(devices, params, "position")? {
            Some(at) => at,
            None => (Vec::new(), devices.len()),
        };
        let list = container_mut(devices, &parent).ok_or("the position is missing")?;
        list.insert(index, Device::group(name.clone(), DeviceGroup::default()));
        let mut path = parent;
        path.push(index);
        Ok(json!({ "device": path_value(&path), "name": name }))
    })
}

/// `device.move {track, device, to}`: moves a device before the position `to`, along the chain,
/// between branches, into or out of a group. Macro mappings to it follow it while it stays in
/// its group; those whose device leaves the group go.
pub(crate) fn move_device(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    edit_chain(session, params, |devices| {
        let from = device_path(devices, params, "device")?;
        let (to_parent, to_index) =
            position_path(devices, params, "to")?.ok_or("to must be the position to move the device before")?;
        // Into itself: a group cannot go inside its own branches.
        if to_parent.len() >= from.len() && to_parent.starts_with(&from) {
            return Err("a group cannot move into itself".into());
        }
        let (&from_index, from_parent) = from.split_last().ok_or("the device path is empty")?;
        // Where `to` lands once the device is out of its list: a place after it in the same list
        // moves up by one, and so does a path that goes through a later device of that list.
        let mut to_parent = to_parent;
        let mut to_index = to_index;
        if to_parent.as_slice() == from_parent {
            if to_index > from_index {
                to_index -= 1;
            }
        } else if to_parent.len() > from_parent.len()
            && to_parent.starts_with(from_parent)
            && let Some(step) = to_parent.get_mut(from_parent.len())
            && *step > from_index
        {
            *step -= 1;
        }
        let mappings = absolute_mappings(devices);
        let device = container_mut(devices, from_parent).ok_or("the device is missing")?.remove(from_index);
        let list = container_mut(devices, &to_parent).ok_or("the position is missing")?;
        let to_index = to_index.min(list.len());
        list.insert(to_index, device);
        let mut path = to_parent.clone();
        path.push(to_index);
        let moved = Moved { from: &from, from_parent, from_index, to_parent: &to_parent, to_index, path: &path };
        place_mappings(devices, mappings, &moved);
        Ok(json!({ "device": path_value(&path) }))
    })
}

/// Every macro mapping on a track: the path of its group, its macro and the target's absolute
/// path.
fn absolute_mappings(devices: &[Device]) -> Vec<(Vec<usize>, usize, Vec<usize>, suhina_model::MacroMapping)> {
    let mut out = Vec::new();
    for (path, device) in suhina_model::walk_devices(devices) {
        let Some(group) = device.as_group() else { continue };
        for (m, macro_) in group.macros.iter().enumerate() {
            for mapping in &macro_.mappings {
                let mut target = path.clone();
                target.extend(mapping.device.iter().map(|&i| i as usize));
                out.push((path.clone(), m, target, mapping.clone()));
            }
        }
    }
    out
}

/// A device moved from `from` (in the list `from_parent`, at `from_index`) to `path` (in the list
/// `to_parent`, at `to_index`, counted once it was out of its place).
struct Moved<'a> {
    from: &'a [usize],
    from_parent: &'a [usize],
    from_index: usize,
    to_parent: &'a [usize],
    to_index: usize,
    path: &'a [usize],
}

impl Moved<'_> {
    /// Where the device at `old` (a path from before the move) is now.
    fn translate(&self, old: &[usize]) -> Vec<usize> {
        if let Some(rest) = old.strip_prefix(self.from) {
            let mut new = self.path.to_vec();
            new.extend_from_slice(rest);
            return new;
        }
        let mut new = old.to_vec();
        // Out of its list: what came after it moves up; into the new one: what comes after moves
        // down.
        if new.starts_with(self.from_parent)
            && let Some(i) = new.get_mut(self.from_parent.len())
            && *i > self.from_index
        {
            *i -= 1;
        }
        if new.starts_with(self.to_parent)
            && let Some(i) = new.get_mut(self.to_parent.len())
            && *i >= self.to_index
        {
            *i += 1;
        }
        new
    }
}

/// Puts the mappings `absolute_mappings` took before a move back, each at its group's new place
/// and pointing at its device's new place: a mapping follows its device while the device stays
/// inside the group, and goes when it leaves.
fn place_mappings(
    devices: &mut [Device],
    mappings: Vec<(Vec<usize>, usize, Vec<usize>, suhina_model::MacroMapping)>,
    moved: &Moved<'_>,
) {
    let groups: Vec<Vec<usize>> = suhina_model::walk_devices(devices)
        .into_iter()
        .filter(|(_, d)| d.as_group().is_some())
        .map(|(p, _)| p)
        .collect();
    for path in groups {
        if let Some(group) = device_at_mut(devices, &path).and_then(Device::as_group_mut) {
            for m in &mut group.macros {
                m.mappings.clear();
            }
        }
    }
    for (group_path, m, target, mapping) in mappings {
        let group_path = moved.translate(&group_path);
        let target = moved.translate(&target);
        let Some(relative) = target.strip_prefix(group_path.as_slice()).filter(|r| !r.is_empty()) else { continue };
        let device: Vec<u32> = relative.iter().filter_map(|&i| u32::try_from(i).ok()).collect();
        if let Some(group) = device_at_mut(devices, &group_path).and_then(Device::as_group_mut)
            && let Some(macro_) = group.macros.get_mut(m)
        {
            macro_.mappings.push(suhina_model::MacroMapping { device, ..mapping });
        }
    }
}

/// `device.duplicate {track, device}`: a copy of a device (a group with everything in it) next
/// to it. A plug-in's copy starts with the state its project device has.
pub(crate) fn duplicate(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_or_main_id(&session.project, params)?;
    // A running plug-in's state as it is now, as device.copy takes it.
    let (heard, _) = session.heard();
    let heard_devices = heard.channel_of_track(track).map(|c| c.devices.clone());
    edit_chain(session, params, |devices| {
        let path = device_path(devices, params, "device")?;
        let copy = heard_devices
            .as_deref()
            .and_then(|heard| device_at(heard, &path))
            .or_else(|| device_at(devices, &path))
            .cloned()
            .ok_or("the device is missing")?;
        let (&index, parent) = path.split_last().ok_or("the device path is empty")?;
        let list = container_mut(devices, parent).ok_or("the device is missing")?;
        list.insert(index + 1, copy);
        let mut at = parent.to_vec();
        at.push(index + 1);
        Ok(json!({ "device": path_value(&at) }))
    })
}

/// Runs `edit` on the group `params.device` names and its branch list.
fn edit_group<T>(
    session: &mut Session,
    params: &Value,
    edit: impl FnOnce(&mut DeviceGroup) -> Result<T, CommandError>,
) -> Result<T, CommandError> {
    edit_chain(session, params, |devices| {
        let path = group_path(devices, params)?;
        let group =
            device_at_mut(devices, &path).and_then(Device::as_group_mut).ok_or("the device is not a device group")?;
        edit(group)
    })
}

/// `branch.add {track, device, position?, name?}`: an empty branch.
pub(crate) fn branch_add(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let name = params.get("name").and_then(Value::as_str).map(str::to_owned);
    edit_group(session, params, |group| {
        if group.branches.len() >= MAX_BRANCHES {
            return Err(format!("a device group holds at most {MAX_BRANCHES} branches").into());
        }
        let position = match params.get("position") {
            None | Some(Value::Null) => group.branches.len(),
            Some(v) => v
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|&n| n <= group.branches.len())
                .ok_or_else(|| format!("position must be from 0 to {}", group.branches.len()))?,
        };
        let name = name.unwrap_or_else(|| format!("Branch {}", group.branches.len() + 1));
        group.branches.insert(position, Branch::new(name.clone()));
        shift_mappings(group, position, 1);
        Ok(json!({ "branch": position, "name": name }))
    })
}

/// Moves the macro mappings into branches from `from` on by `by` (1 after an insert, -1 after a
/// removal, whose own branch's mappings are gone first).
fn shift_mappings(group: &mut DeviceGroup, from: usize, by: isize) {
    for m in &mut group.macros {
        for mapping in &mut m.mappings {
            if let Some(branch) = mapping.device.first_mut()
                && *branch as usize >= from
            {
                *branch = branch.saturating_add_signed(by as i32);
            }
        }
    }
}

/// `branch.remove {track, device, branch}`: removes a branch with its devices and their
/// mappings; the last branch stays.
pub(crate) fn branch_remove(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    edit_group(session, params, |group| {
        let index = branch_index(group, params)?;
        if group.branches.len() == 1 {
            return Err("a device group keeps at least one branch".into());
        }
        let removed = group.branches.remove(index);
        let at = u32::try_from(index).unwrap_or(u32::MAX);
        for m in &mut group.macros {
            m.mappings.retain(|x| x.device.first() != Some(&at));
        }
        shift_mappings(group, index + 1, -1);
        Ok(json!({ "removed": removed.name, "branches": group.branches.len() }))
    })
}

/// `branch.duplicate {track, device, branch}`: a copy of a branch next to it.
pub(crate) fn branch_duplicate(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    edit_group(session, params, |group| {
        let index = branch_index(group, params)?;
        if group.branches.len() >= MAX_BRANCHES {
            return Err(format!("a device group holds at most {MAX_BRANCHES} branches").into());
        }
        let copy = group.branches.get(index).cloned().ok_or("the branch is missing")?;
        group.branches.insert(index + 1, copy);
        shift_mappings(group, index + 1, 1);
        Ok(json!({ "branch": index + 1 }))
    })
}

/// `branch.rename {track, device, branch, name}`.
pub(crate) fn branch_rename(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let name = params.get("name").and_then(Value::as_str).ok_or("name must be text")?.to_owned();
    edit_group(session, params, |group| {
        let index = branch_index(group, params)?;
        let branch = group.branches.get_mut(index).ok_or("the branch is missing")?;
        branch.name.clone_from(&name);
        Ok(json!({ "branch": index, "name": name }))
    })
}

/// `branch.setVolume {track, device, branch, db}`: as `track.setVolume`, in decibels.
pub(crate) fn branch_set_volume(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let db = crate::commands::number_or_neg_infinity(params, "db")?;
    if db > crate::commands::MAX_VOLUME_DB {
        return Err(format!("db must be at most +{}, got {db}", crate::commands::MAX_VOLUME_DB).into());
    }
    let volume = crate::commands::gain_of_db(db);
    edit_group(session, params, |group| {
        let index = branch_index(group, params)?;
        let branch = group.branches.get_mut(index).ok_or("the branch is missing")?;
        branch.volume = volume;
        Ok(json!({ "branch": index, "volume_db": crate::commands::to_db(volume) }))
    })
}

/// `branch.setPan {track, device, branch, pan}`.
pub(crate) fn branch_set_pan(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let pan = number(params, "pan")?;
    if !(-1.0..=1.0).contains(&pan) {
        return Err("pan must be from -1 to 1".into());
    }
    edit_group(session, params, |group| {
        let index = branch_index(group, params)?;
        let branch = group.branches.get_mut(index).ok_or("the branch is missing")?;
        branch.pan = pan;
        Ok(json!({ "branch": index, "pan": pan }))
    })
}

/// `branch.setMute {track, device, branch, mute}`: its devices keep running.
pub(crate) fn branch_set_mute(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let mute = params.get("mute").and_then(Value::as_bool).ok_or("mute must be true or false")?;
    edit_group(session, params, |group| {
        let index = branch_index(group, params)?;
        let branch = group.branches.get_mut(index).ok_or("the branch is missing")?;
        branch.mute = mute;
        Ok(json!({ "branch": index, "mute": mute }))
    })
}

/// `branch.setSolo {track, device, branch, solo}`: solo within its group.
pub(crate) fn branch_set_solo(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let solo = params.get("solo").and_then(Value::as_bool).ok_or("solo must be true or false")?;
    edit_group(session, params, |group| {
        let index = branch_index(group, params)?;
        let branch = group.branches.get_mut(index).ok_or("the branch is missing")?;
        branch.solo = solo;
        Ok(json!({ "branch": index, "solo": solo }))
    })
}

/// A device group as `track.devices` lists it: its parameters, branches (each with its devices,
/// listed by `device_json` with their paths) and macros.
pub(crate) fn group_json(
    group: &DeviceGroup,
    device: &Device,
    path: &[usize],
    device_json: &dyn Fn(&[usize], &Device) -> Value,
) -> Value {
    let values = suhina_engine::group_param_values(&device.params);
    let params: Vec<Value> = suhina_engine::GROUP_PARAMS
        .iter()
        .zip(&values)
        .map(|(&(id, min, max, default), value)| json!({ "id": id, "value": value, "min": min, "max": max, "default": default }))
        .collect();
    let branches: Vec<Value> = group
        .branches
        .iter()
        .enumerate()
        .map(|(b, branch)| {
            let devices: Vec<Value> = branch
                .devices
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    let mut inner = path.to_vec();
                    inner.extend([b, i]);
                    device_json(&inner, d)
                })
                .collect();
            json!({
                "index": b, "name": branch.name, "volume": branch.volume, "pan": branch.pan,
                "mute": branch.mute, "solo": branch.solo, "devices": devices,
            })
        })
        .collect();
    let macros: Vec<Value> = group
        .macros
        .iter()
        .enumerate()
        .map(|(m, macro_)| {
            let mappings: Vec<Value> = macro_
                .mappings
                .iter()
                .map(|x| json!({ "device": x.device.iter().map(ToString::to_string).collect::<Vec<_>>().join("/"), "param": x.param, "min": x.min, "max": x.max }))
                .collect();
            json!({ "macro": m + 1, "name": macro_.name, "value": values.get(m + 2), "mappings": mappings })
        })
        .collect();
    json!({ "params": params, "branches": branches, "macros": macros })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_parse_from_numbers_and_text() {
        assert_eq!(parse_path(&json!(3)), Some(vec![3]));
        assert_eq!(parse_path(&json!("2/1/0")), Some(vec![2, 1, 0]));
        assert_eq!(parse_path(&json!(" 4 ")), Some(vec![4]));
        assert_eq!(parse_path(&json!("2//0")), None);
        assert_eq!(parse_path(&json!("-1")), None);
        assert_eq!(parse_path(&json!("a/b")), None);
        assert_eq!(parse_path(&json!(-1)), None);
        assert_eq!(parse_path(&json!("0/0/0/0/0/0/0/0/0/0/0")), None, "deeper than four groups");
        assert_eq!(path_value(&[2]), json!(2));
        assert_eq!(path_value(&[2, 1, 0]), json!("2/1/0"));
    }
}
