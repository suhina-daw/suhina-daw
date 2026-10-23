// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The identity of clips, arrangement lanes and scenes
//! ([ADR 0025](../../docs/decisions/0025-object-identity.md)).
//!
//! The three kinds take their ids from one counter in the project, [`Project::next_id`], so a
//! value names at most one object of any of them, and a value once handed out is never handed
//! out again: deleting an object does not return its id. Id 0 is no id: an object built in code
//! starts with it, and [`Project::assign_ids`] gives every such object the next ids in document
//! order (lanes, each followed by its clips, then the scenes, then the launcher's clips column by
//! column), which is also how a file written before ids existed gets them.
//!
//! Tracks and channels have ids of their own ([`TrackId`], [`ChannelId`]);
//! [`Project::track`] and [`Project::channel`] find one by it.

use std::fmt;

use crate::{Channel, ChannelId, Clip, Project, Track, TrackId};

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

object_id!(ClipId, LaneId, SceneId);

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
        lanes.chain(clips).chain(scenes).max().unwrap_or(0)
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
        Ok(())
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
