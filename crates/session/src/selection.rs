// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The selection (`docs/spec/selection.md`, #1679): the selected track and the selected device
//! of its chain — what controllers, mappings and agents point at. One per open project, and
//! the session's own state: not an edit, so undo never moves it, and it is not saved with the
//! project.

use suhina_model::{DeviceId, Project, TrackId, device_at, walk_devices};

/// What is selected: a track (the main channel among them, which the device commands address
/// as `"main"`) and, when one is, a device of that track's chain — the device held by its
/// stable id, so editing the chain around it keeps the selection on the device itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Selection {
    track: Option<TrackId>,
    device: Option<DeviceId>,
}

impl Selection {
    /// The selected track, when one is.
    pub fn track(&self) -> Option<TrackId> {
        self.track
    }

    /// Nothing is selected.
    pub fn clear(&mut self) {
        self.track = None;
        self.device = None;
    }

    /// The path the selected device now sits at, found by its id — a device moved around the
    /// chain (into or out of a group included) stays the selected one. `None` when the device
    /// is gone.
    pub fn device_path(&self, project: &Project) -> Option<Vec<usize>> {
        let id = self.device?;
        let channel = project.channel_of_track(self.track?)?;
        walk_devices(&channel.devices).into_iter().find(|(_, device)| device.id == id).map(|(path, _)| path)
    }

    /// Keeps only what the project still holds: a removed track or device (or the main channel
    /// going) leaves nothing selected. Undo restores the project, never the selection — it is
    /// not an edit.
    pub fn prune(&mut self, project: &Project) {
        if let Some(track) = self.track
            && project.channel_of_track(track).is_none()
        {
            self.clear();
            return;
        }
        if self.device.is_some() && self.device_path(project).is_none() {
            self.device = None;
        }
    }

    /// Selects a track: a different track selects its first device (none when its chain is
    /// empty), and the already-selected track changes nothing.
    pub fn select_track(&mut self, project: &Project, track: TrackId) {
        if self.track == Some(track) {
            return;
        }
        self.track = Some(track);
        self.device = project
            .channel_of_track(track)
            .and_then(|channel| channel.devices.first())
            .map(|device| device.id)
            .filter(|id| id.is_some());
    }

    /// Selects `id` on `track`, selecting the track too.
    pub fn select_device(&mut self, track: TrackId, id: DeviceId) {
        self.track = Some(track);
        self.device = Some(id);
    }

    /// The next or previous track in the project's order; at either end nothing happens, and
    /// with nothing selected the first (next) or last (previous) track is selected.
    pub fn step_track(&mut self, project: &Project, forward: bool) {
        let ids: Vec<TrackId> = project.tracks.iter().map(|track| track.id).collect();
        let next = match self.track.and_then(|current| ids.iter().position(|id| *id == current)) {
            Some(at) if forward => ids.get(at + 1).copied(),
            Some(at) if at > 0 => ids.get(at - 1).copied(),
            Some(_) => None,
            None if forward => ids.first().copied(),
            None => ids.last().copied(),
        };
        if let Some(next) = next {
            self.select_track(project, next);
        }
    }

    /// The next or previous device of the branch the selected device sits in — its track's
    /// top-level chain for a device not inside a group; nothing at an end, and nothing at all
    /// with no device selected.
    pub fn step_device(&mut self, project: &Project, forward: bool) {
        let Some(path) = self.device_path(project) else { return };
        let Some(channel) = self.track.and_then(|track| project.channel_of_track(track)) else { return };
        // The branch the device sits in: the chain itself, or a group's branch of it — a
        // device path is device, branch, device … (`"2/1/0"`), so the parent's own parent
        // holds the group and its last element the branch.
        let parent = &path[..path.len() - 1];
        let siblings = if parent.is_empty() {
            &channel.devices
        } else {
            let Some(group) = device_at(&channel.devices, &parent[..parent.len() - 1]) else { return };
            let Some(branch) = parent.last().and_then(|branch| group.as_group()?.branches.get(*branch)) else {
                return;
            };
            &branch.devices
        };
        let at = path.last().copied().unwrap_or(0);
        let next = if forward {
            siblings.get(at + 1)
        } else if at > 0 {
            siblings.get(at - 1)
        } else {
            None
        };
        if let Some(device) = next {
            self.device = Some(device.id);
        }
    }
}
