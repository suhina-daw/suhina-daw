// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The identity of clips, arrangement lanes, scenes and devices
//! ([ADR 0025](../../docs/decisions/0025-object-identity.md)).
//!
//! The four kinds take their ids from one counter in the project, [`Project::next_id`], so a
//! value names at most one object of any of them, and a value once handed out is never handed
//! out again: deleting an object does not return its id. Id 0 is no id: an object built in code
//! starts with it, and [`Project::assign_ids`] gives every such object the next ids in document
//! order (lanes, each followed by its clips, then the scenes, then the launcher's clips column by
//! column, then the channels' devices, a group before the devices in it), which is also how a
//! file written before ids existed gets them.
//!
//! Tracks and channels have ids of their own ([`TrackId`], [`ChannelId`]);
//! [`Project::track`] and [`Project::channel`] find one by it.

use std::fmt;

use crate::{Channel, ChannelId, Clip, Device, Project, Track, TrackId};

/// Gives the devices among `devices` without an id, or with one `seen` already, the next ids.
fn fill_device_ids(
    devices: &mut [Device],
    counter: &mut Counter,
    seen: &mut std::collections::HashSet<DeviceId>,
    renumbered: &mut Vec<(DeviceId, DeviceId)>,
) -> Result<(), IdsExhausted> {
    for device in devices {
        let old = device.id;
        if !old.is_some() || !seen.insert(old) {
            device.id = DeviceId(counter.take()?);
            seen.insert(device.id);
            if old.is_some() {
                renumbered.push((old, device.id));
            }
        }
        if let Some(group) = device.as_group_mut() {
            // The ids given inside the group, so its macro mappings follow them.
            let mut inner = Vec::new();
            for branch in &mut group.branches {
                fill_device_ids(&mut branch.devices, counter, seen, &mut inner)?;
            }
            group.resolve_mappings(&inner);
            renumbered.extend(inner);
        }
    }
    Ok(())
}

/// The automation lanes of `devices` and the devices in their groups, depth first: a group's own
/// lanes before those of the devices in it.
fn collect_device_lanes<'a>(devices: &'a mut [Device], lanes: &mut Vec<&'a mut crate::AutomationLane>) {
    for device in devices {
        let Device { automation, kind, .. } = device;
        lanes.extend(automation.iter_mut());
        if let crate::DeviceKind::Group(group) = kind {
            for branch in &mut group.branches {
                collect_device_lanes(&mut branch.devices, lanes);
            }
        }
    }
}

/// Identifies a [`Clip`], on the arrangement or in a launcher slot. Stays the same
/// when the clip is moved, trimmed or edited; a copy gets a new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct ClipId(pub u32);

/// Identifies an [`ArrangementLane`](crate::ArrangementLane).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct LaneId(pub u32);

/// Identifies a [`Scene`](crate::Scene).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct SceneId(pub u32);

/// Identifies a [`Device`] on a channel, inside a device group too (#763 › 1).
/// Stays the same when the device moves, to another track or into a group; a copy gets a new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct DeviceId(pub u32);

macro_rules! object_id {
    ($($id:ident),*) => {$(
        impl $id {
            /// No id yet: [`Project::assign_ids`] gives the object one.
            pub const NONE: Self = Self(0);

            /// Whether this is an id, not [`Self::NONE`].
            pub fn is_some(self) -> bool {
                self.0 != 0
            }
        }

        impl fmt::Display for $id {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    )*};
}

/// Identifies an arrangement [`Marker`](crate::Marker) (#763 › 1). Stays the same when the
/// marker moves or is renamed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct MarkerId(pub u32);

/// Identifies an [`AutomationPoint`](crate::AutomationPoint) on a lane (#763 › 1). Stays the same
/// when the point is dragged; a point a range tool draws anew gets a new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct PointId(pub u32);

/// Identifies a [`TempoPoint`](crate::TempoPoint), a change in the tempo map (#763 › 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct TempoPointId(pub u32);

object_id!(ClipId, LaneId, SceneId, DeviceId, MarkerId, PointId, TempoPointId);

/// The counter has reached the largest id there is: the project can name no more objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdsExhausted;

impl fmt::Display for IdsExhausted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the project has used every object id there is ({})", u32::MAX - 1)
    }
}

impl std::error::Error for IdsExhausted {}

/// The next id to hand out while [`Project::assign_ids`] walks the project.
struct Counter(u32);

impl Counter {
    fn take(&mut self) -> Result<u32, IdsExhausted> {
        let id = self.0.max(1);
        // `u32::MAX` itself is never handed out: the counter must be able to name what follows.
        self.0 = id.checked_add(1).ok_or(IdsExhausted)?;
        Ok(id)
    }
}

/// Where a clip is: on an arrangement lane, or in a launcher column's slot. Indices into the
/// project as it is now; an edit that adds, moves or deletes a clip makes them stale, the id
/// stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipPlace {
    Lane { lane: usize, index: usize },
    Slot { column: usize, slot: usize },
}

impl Project {
    /// Hands out the next object id, or `None` when the counter is exhausted. `u32::MAX` is never
    /// handed out, so the counter can always say what comes next.
    fn next_object_id(&mut self) -> Option<u32> {
        let id = self.next_id.max(1);
        self.next_id = id.checked_add(1)?;
        Some(id)
    }

    /// A new clip id, never used in this project before.
    pub fn new_clip_id(&mut self) -> Result<ClipId, IdsExhausted> {
        self.next_object_id().map(ClipId).ok_or(IdsExhausted)
    }

    /// A new arrangement lane id, never used in this project before.
    pub fn new_lane_id(&mut self) -> Result<LaneId, IdsExhausted> {
        self.next_object_id().map(LaneId).ok_or(IdsExhausted)
    }

    /// A new scene id, never used in this project before.
    pub fn new_scene_id(&mut self) -> Result<SceneId, IdsExhausted> {
        self.next_object_id().map(SceneId).ok_or(IdsExhausted)
    }

    /// The largest object id in use, 0 when there is none.
    pub fn max_object_id(&self) -> u32 {
        let lanes = self.arrangement.lanes.iter().map(|l| l.id.0);
        let clips = self.all_clips().map(|c| c.id.0);
        let scenes = self.launcher.scenes.iter().map(|s| s.id.0);
        let devices = self.all_devices().map(|d| d.id.0);
        let markers = self.arrangement.markers.iter().map(|m| m.id.0);
        let points = self.all_automation_points().map(|p| p.id.0);
        let tempo = self.transport.tempo_changes.iter().map(|t| t.id.0);
        lanes.chain(clips).chain(scenes).chain(devices).chain(markers).chain(points).chain(tempo).max().unwrap_or(0)
    }

    /// Moves the counter past every id in use, then gives every lane, clip and scene that has no
    /// id ([`ClipId::NONE`]) the next ones, in document order. Objects that have an id keep it,
    /// so calling it again changes nothing. Fails, leaving the objects it did not reach without
    /// an id, when the counter runs out.
    pub fn assign_ids(&mut self) -> Result<(), IdsExhausted> {
        let used = self.max_object_id();
        if used == u32::MAX {
            return Err(IdsExhausted);
        }
        // Left at 0 while nothing has an id, so an empty project stays as it was.
        let mut counter = Counter(if used == 0 { self.next_id } else { self.next_id.max(used + 1) });
        let result = self.fill_ids(&mut counter);
        self.next_id = counter.0;
        result
    }

    fn fill_ids(&mut self, counter: &mut Counter) -> Result<(), IdsExhausted> {
        for lane in &mut self.arrangement.lanes {
            if !lane.id.is_some() {
                lane.id = LaneId(counter.take()?);
            }
            for clip in lane.clips.iter_mut().filter(|c| !c.id.is_some()) {
                clip.id = ClipId(counter.take()?);
            }
        }
        for scene in self.launcher.scenes.iter_mut().filter(|s| !s.id.is_some()) {
            scene.id = SceneId(counter.take()?);
        }
        let launched = self.launcher.columns.iter_mut().flat_map(|c| &mut c.slots).filter_map(|s| s.clip.as_mut());
        for clip in launched.filter(|c| !c.id.is_some()) {
            clip.id = ClipId(counter.take()?);
        }
        // A device that has an id an earlier device has (a copy that kept it) gets a new one too:
        // the first one, in document order, keeps it.
        let mut seen = std::collections::HashSet::new();
        for channel in &mut self.channels {
            fill_device_ids(&mut channel.devices, counter, &mut seen, &mut Vec::new())?;
        }
        // Markers, automation points and tempo points the same way: one that has no id, or one an
        // earlier one has (a copied lane's points), gets the next.
        let mut seen = std::collections::HashSet::new();
        for marker in &mut self.arrangement.markers {
            if !marker.id.is_some() || !seen.insert(marker.id) {
                marker.id = MarkerId(counter.take()?);
                seen.insert(marker.id);
            }
        }
        let mut seen = std::collections::HashSet::new();
        for point in self.all_automation_points_mut() {
            if !point.id.is_some() || !seen.insert(point.id) {
                point.id = PointId(counter.take()?);
                seen.insert(point.id);
            }
        }
        let mut seen = std::collections::HashSet::new();
        for point in &mut self.transport.tempo_changes {
            if !point.id.is_some() || !seen.insert(point.id) {
                point.id = TempoPointId(counter.take()?);
                seen.insert(point.id);
            }
        }
        Ok(())
    }

    /// Every automation point, the channels' lanes first (each channel's mixer lanes, then its
    /// devices' lanes depth first), in lane order.
    pub fn all_automation_points(&self) -> impl Iterator<Item = &crate::AutomationPoint> {
        self.channels.iter().flat_map(|c| {
            let devices = crate::walk_devices(&c.devices).into_iter().flat_map(|(_, d)| &d.automation);
            c.automation.iter().chain(devices).flat_map(|lane| &lane.points)
        })
    }

    /// Every automation point, to change, in the order of [`Project::all_automation_points`].
    fn all_automation_points_mut(&mut self) -> impl Iterator<Item = &mut crate::AutomationPoint> {
        let mut lanes: Vec<&mut crate::AutomationLane> = Vec::new();
        for channel in &mut self.channels {
            lanes.extend(channel.automation.iter_mut());
            collect_device_lanes(&mut channel.devices, &mut lanes);
        }
        lanes.into_iter().flat_map(|lane| lane.points.iter_mut())
    }

    /// The lane and place of the automation point `id`: where it is now.
    pub fn automation_point(&self, id: PointId) -> Option<&crate::AutomationPoint> {
        if !id.is_some() {
            return None;
        }
        self.all_automation_points().find(|p| p.id == id)
    }

    /// Every device on every channel, depth-first: a group before the devices in it.
    pub fn all_devices(&self) -> impl Iterator<Item = &crate::Device> {
        self.channels.iter().flat_map(|c| crate::walk_devices(&c.devices).into_iter().map(|(_, d)| d))
    }

    /// The channel and the path of the device `id`, or `None` when no device has it.
    pub fn device_place(&self, id: DeviceId) -> Option<(ChannelId, Vec<usize>)> {
        if !id.is_some() {
            return None;
        }
        self.channels.iter().find_map(|c| {
            crate::walk_devices(&c.devices).into_iter().find(|(_, d)| d.id == id).map(|(path, _)| (c.id, path))
        })
    }

    /// Every clip, the arrangement's lane by lane and then the launcher's column by column.
    pub fn all_clips(&self) -> impl Iterator<Item = &Clip> {
        let arranged = self.arrangement.lanes.iter().flat_map(|l| &l.clips);
        let launched = self.launcher.columns.iter().flat_map(|c| &c.slots).filter_map(|s| s.clip.as_ref());
        arranged.chain(launched)
    }

    /// Where the clip `id` is now, or `None` when no clip has it.
    pub fn clip_place(&self, id: ClipId) -> Option<ClipPlace> {
        if !id.is_some() {
            return None;
        }
        for (lane, l) in self.arrangement.lanes.iter().enumerate() {
            if let Some(index) = l.clips.iter().position(|c| c.id == id) {
                return Some(ClipPlace::Lane { lane, index });
            }
        }
        for (column, c) in self.launcher.columns.iter().enumerate() {
            if let Some(slot) = c.slots.iter().position(|s| s.clip.as_ref().is_some_and(|c| c.id == id)) {
                return Some(ClipPlace::Slot { column, slot });
            }
        }
        None
    }

    /// The clip at `place`.
    pub fn clip_at(&self, place: ClipPlace) -> Option<&Clip> {
        match place {
            ClipPlace::Lane { lane, index } => self.arrangement.lanes.get(lane)?.clips.get(index),
            ClipPlace::Slot { column, slot } => self.launcher.columns.get(column)?.slots.get(slot)?.clip.as_ref(),
        }
    }

    /// The clip at `place`, to change.
    pub fn clip_at_mut(&mut self, place: ClipPlace) -> Option<&mut Clip> {
        match place {
            ClipPlace::Lane { lane, index } => self.arrangement.lanes.get_mut(lane)?.clips.get_mut(index),
            ClipPlace::Slot { column, slot } => {
                self.launcher.columns.get_mut(column)?.slots.get_mut(slot)?.clip.as_mut()
            }
        }
    }

    /// The clip `id`, wherever it is.
    pub fn clip(&self, id: ClipId) -> Option<&Clip> {
        self.clip_at(self.clip_place(id)?)
    }

    /// The clip `id`, wherever it is, to change.
    pub fn clip_mut(&mut self, id: ClipId) -> Option<&mut Clip> {
        let place = self.clip_place(id)?;
        self.clip_at_mut(place)
    }
}

/// Finding a track or a channel by its id.
/// The most channels [`Project::output_chain`] gives, so a malformed project cannot make it
/// walk forever.
pub const MAX_OUTPUT_HOPS: usize = 64;

impl Project {
    /// The track with id `id`.
    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    /// The track with id `id`, to change.
    pub fn track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    /// The channel with id `id`.
    pub fn channel(&self, id: ChannelId) -> Option<&Channel> {
        self.channels.iter().find(|c| c.id == id)
    }

    /// The channel with id `id`, to change.
    pub fn channel_mut(&mut self, id: ChannelId) -> Option<&mut Channel> {
        self.channels.iter_mut().find(|c| c.id == id)
    }

    /// The main channel, the one every other plays into in the end.
    pub fn main_channel(&self) -> Option<&Channel> {
        self.channels.iter().find(|c| c.role == crate::ChannelRole::Master)
    }

    /// The mixer channel of track `track`, or the main channel for [`TrackId::MAIN`].
    pub fn channel_of_track(&self, track: TrackId) -> Option<&Channel> {
        if track == TrackId::MAIN {
            return self.main_channel();
        }
        self.track(track).and_then(|t| t.channel).and_then(|id| self.channel(id))
    }

    /// The mixer channel of track `track`, or the main channel for [`TrackId::MAIN`], to change.
    pub fn channel_of_track_mut(&mut self, track: TrackId) -> Option<&mut Channel> {
        let id = self.channel_of_track(track)?.id;
        self.channel_mut(id)
    }

    /// The channels the sound of channel `from` passes through after it, nearest first: its
    /// output, that channel's output, and so on (#764 › 7). A channel with no output ends the
    /// walk (it plays into the main output), and so does an output that names no channel, after
    /// it is given. A routing cycle, which a valid project does not have, ends it where the chain
    /// comes back on itself, and it never gives more than [`MAX_OUTPUT_HOPS`] channels.
    pub fn output_chain(&self, from: ChannelId) -> impl Iterator<Item = ChannelId> + '_ {
        let mut seen = vec![from];
        let mut next = self.channel(from).and_then(|c| c.output);
        std::iter::from_fn(move || {
            let id = next.take()?;
            if seen.len() > MAX_OUTPUT_HOPS || seen.contains(&id) {
                return None;
            }
            seen.push(id);
            next = self.channel(id).and_then(|c| c.output);
            Some(id)
        })
    }
}

#[cfg(test)]
#[path = "ids_tests.rs"]
mod tests;
