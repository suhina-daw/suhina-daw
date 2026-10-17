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

use std::fmt;

use crate::{Clip, Project};

/// Identifies a [`Clip`](crate::Clip), on the arrangement or in a launcher slot. Stays the same
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

#[cfg(test)]
mod tests {
    use crate::{
        Arrangement, ArrangementLane, Clip, ClipContent, ClipId, ClipPlace, ContentId, Extensions, IdsExhausted,
        LaneId, Launcher, LauncherColumn, Project, Scene, SceneId, Slot, Track, TrackId, ValidationError,
    };

    fn clip(start: f64) -> Clip {
        Clip::new(ContentId(1), start, 4.0)
    }

    /// Two lanes of two clips, two scenes and a column with one clip, none with an id.
    fn project() -> Project {
        let lane = |track| ArrangementLane {
            track: Some(TrackId(track)),
            clips: vec![clip(0.0), clip(4.0)],
            ..ArrangementLane::default()
        };
        Project {
            tracks: vec![Track::new(TrackId(1), "1"), Track::new(TrackId(2), "2")],
            contents: vec![ClipContent {
                id: ContentId(1),
                name: String::new(),
                length: 4.0,
                lanes: Vec::new(),
                extensions: Extensions::default(),
                scale: None,
            }],
            arrangement: Arrangement { lanes: vec![lane(1), lane(2)], markers: Vec::new() },
            launcher: Launcher {
                scenes: vec![Scene::default(), Scene::default()],
                columns: vec![LauncherColumn {
                    track: TrackId(1),
                    slots: vec![Slot::default(), Slot { clip: Some(clip(0.0)), has_stop: false }],
                }],
                mappings: Vec::new(),
            },
            ..Project::default()
        }
    }

    #[test]
    fn ids_are_given_in_document_order_and_kept_on_a_second_pass() {
        let mut p = project();
        p.assign_ids().unwrap();
        let lanes: Vec<_> = p.arrangement.lanes.iter().map(|l| l.id).collect();
        let clips: Vec<_> = p.all_clips().map(|c| c.id).collect();
        let scenes: Vec<_> = p.launcher.scenes.iter().map(|s| s.id).collect();
        assert_eq!(lanes, [LaneId(1), LaneId(4)]);
        assert_eq!(clips, [ClipId(2), ClipId(3), ClipId(5), ClipId(6), ClipId(9)]);
        assert_eq!(scenes, [SceneId(7), SceneId(8)]);
        assert_eq!(p.next_id, 10);
        assert_eq!(p.validate(), Ok(()));

        let again = p.clone();
        p.assign_ids().unwrap();
        assert_eq!(p, again, "objects with ids keep them");
        assert_eq!(project_with_ids(), p, "the same project always gets the same ids");
    }

    fn project_with_ids() -> Project {
        let mut p = project();
        p.assign_ids().unwrap();
        p
    }

    #[test]
    fn a_new_id_is_never_one_handed_out_before() {
        let mut p = project_with_ids();
        let last = p.arrangement.lanes[1].clips.pop().unwrap();
        let new = p.new_clip_id().unwrap();
        assert!(new > last.id, "a deleted clip's id {} is not handed out again ({new})", last.id);
        assert!(p.new_scene_id().unwrap().0 > new.0, "lanes, clips and scenes share the counter");

        // An object added with an id the counter has not reached moves the counter on.
        p.arrangement.lanes[0].clips.push(Clip { id: ClipId(50), ..clip(8.0) });
        assert_eq!(p.validate(), Err(vec![ValidationError::IdAhead(50)]));
        p.assign_ids().unwrap();
        assert_eq!(p.next_id, 51);
        assert_eq!(p.new_lane_id(), Ok(LaneId(51)));
    }

    #[test]
    fn the_counter_never_hands_out_the_largest_value() {
        let mut p = Project { next_id: u32::MAX - 1, ..Project::default() };
        assert_eq!(p.new_clip_id(), Ok(ClipId(u32::MAX - 1)));
        assert_eq!(p.new_clip_id(), Err(IdsExhausted));
        assert_eq!(p.next_id, u32::MAX);

        // Too few ids left for the objects that need one: as many as fit get one, then it fails.
        let mut p = project();
        p.next_id = u32::MAX - 2;
        assert_eq!(p.assign_ids(), Err(IdsExhausted));
        assert_eq!(p.arrangement.lanes[0].id, LaneId(u32::MAX - 2));
        assert_eq!(p.arrangement.lanes[0].clips[0].id, ClipId(u32::MAX - 1));
        assert_eq!(p.arrangement.lanes[0].clips[1].id, ClipId::NONE);

        // An object that already has the largest value leaves nothing to hand out.
        let mut p = project();
        p.launcher.scenes[0].id = SceneId(u32::MAX);
        assert_eq!(p.assign_ids(), Err(IdsExhausted));
    }

    #[test]
    fn duplicate_ids_do_not_validate() {
        let mut p = project_with_ids();
        p.arrangement.lanes[1].clips[0].id = ClipId(2);
        p.launcher.scenes[1].id = SceneId(7);
        p.arrangement.lanes[1].id = LaneId(1);
        assert_eq!(
            p.validate(),
            Err(vec![
                ValidationError::DuplicateClip(ClipId(2)),
                ValidationError::DuplicateLane(LaneId(1)),
                ValidationError::DuplicateScene(SceneId(7)),
            ])
        );
        // A lane and a clip with the same value: the kinds share the counter, so that is refused
        // too, once, beside nothing else.
        let mut p = project_with_ids();
        p.arrangement.lanes[0].clips[0].id = ClipId(1);
        assert_eq!(p.validate(), Err(vec![ValidationError::SharedId(1)]));
        // A clip in a slot and a scene, the same.
        let mut p = project_with_ids();
        p.launcher.scenes[0].id = SceneId(9);
        assert_eq!(p.validate(), Err(vec![ValidationError::SharedId(9)]));
    }

    #[test]
    fn a_clip_is_found_by_its_id_wherever_it_moves() {
        let mut p = project_with_ids();
        assert_eq!(p.clip_place(ClipId(9)), Some(ClipPlace::Slot { column: 0, slot: 1 }));
        assert_eq!(p.clip_place(ClipId(3)), Some(ClipPlace::Lane { lane: 0, index: 1 }));
        // The first clip of the lane goes; the second is now at index 0 and still clip 3.
        p.arrangement.lanes[0].clips.remove(0);
        assert_eq!(p.clip_place(ClipId(3)), Some(ClipPlace::Lane { lane: 0, index: 0 }));
        p.clip_mut(ClipId(3)).unwrap().start = 12.0;
        assert_eq!(p.clip(ClipId(3)).map(|c| c.start), Some(12.0));
        assert_eq!(p.clip_place(ClipId(2)), None);
        assert_eq!(p.clip_place(ClipId::NONE), None, "no id names no clip");
    }
}
