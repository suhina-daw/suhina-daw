// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How a channel's automation lanes are laid out in the Arrangement view (#1322): the lanes
//! mounted under its track's top lane, in their order, and the groups some of them are gathered
//! in under a heading of their own. Part of the project, so a change is an undo step and is
//! saved; the lanes' points live on the channel and its devices, not here.

use crate::{ChannelId, DeviceId};

/// Most lanes one channel's layout holds.
pub const MAX_LAYOUT_LANES: usize = 1024;
/// Most groups one channel's layout holds.
pub const MAX_LAYOUT_GROUPS: usize = 256;
/// Longest group name, in characters.
pub const MAX_GROUP_NAME: usize = 128;

/// A lane in a layout: the parameter's device by its id (`None` for the channel's own volume,
/// pan or mute) and the parameter's id, as its automation lane names it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LaneRef {
    pub device: Option<DeviceId>,
    pub param: String,
}

impl LaneRef {
    /// The lane of the channel's own mixer parameter `param` (volume, pan or mute).
    pub fn mixer(param: impl Into<String>) -> Self {
        Self { device: None, param: param.into() }
    }
}

/// Some of a layout's lanes under a heading: its name, its lanes (each also in the layout's
/// lanes, where they stand together), whether they are folded away, and the device it gathers
/// when it was made for one device's lanes (Settings › Arrangement › Automation).
#[derive(Debug, Clone, PartialEq)]
pub struct LaneGroup {
    pub name: String,
    pub lanes: Vec<LaneRef>,
    pub folded: bool,
    pub device: Option<DeviceId>,
}

/// One channel's lane layout. A channel with no layout has not been laid out: the view shows
/// the lanes that have points under the top lane.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneLayout {
    pub channel: ChannelId,
    pub lanes: Vec<LaneRef>,
    pub groups: Vec<LaneGroup>,
}

impl LaneLayout {
    /// An empty layout of `channel`: no lane mounted, no group.
    pub fn new(channel: ChannelId) -> Self {
        Self { channel, lanes: Vec::new(), groups: Vec::new() }
    }

    /// Whether the layout keeps to its limits and is whole: each group's lanes are among the
    /// layout's lanes and in no other group, no lane twice, names within their length.
    pub fn check(&self) -> Result<(), String> {
        if self.lanes.len() > MAX_LAYOUT_LANES {
            return Err(format!("a layout holds at most {MAX_LAYOUT_LANES} lanes"));
        }
        if self.groups.len() > MAX_LAYOUT_GROUPS {
            return Err(format!("a layout holds at most {MAX_LAYOUT_GROUPS} groups"));
        }
        for (i, lane) in self.lanes.iter().enumerate() {
            if lane.param.trim().is_empty() {
                return Err("a lane needs a parameter".into());
            }
            if self.lanes.iter().take(i).any(|l| l == lane) {
                return Err(format!("the lane of {:?} is in the layout twice", lane.param));
            }
        }
        for (i, group) in self.groups.iter().enumerate() {
            if group.name.trim().is_empty() || group.name.chars().count() > MAX_GROUP_NAME {
                return Err(format!("a group's name has 1 to {MAX_GROUP_NAME} characters"));
            }
            for lane in &group.lanes {
                if !self.lanes.contains(lane) {
                    return Err(format!("group {:?} holds a lane that is not in the layout", group.name));
                }
                if self.groups.iter().take(i).any(|g| g.lanes.contains(lane)) {
                    return Err(format!("the lane of {:?} is in two groups", lane.param));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layout_keeps_its_lanes_whole_and_within_limits() {
        let mut layout = LaneLayout::new(ChannelId(1));
        layout.lanes = vec![LaneRef::mixer("volume"), LaneRef { device: Some(DeviceId(3)), param: "7".into() }];
        layout.groups = vec![LaneGroup {
            name: "Synth".into(),
            lanes: vec![LaneRef { device: Some(DeviceId(3)), param: "7".into() }],
            folded: true,
            device: Some(DeviceId(3)),
        }];
        assert_eq!(layout.check(), Ok(()));
        let mut twice = layout.clone();
        twice.lanes.push(LaneRef::mixer("volume"));
        assert!(twice.check().is_err());
        let mut stray = layout.clone();
        stray.groups[0].lanes.push(LaneRef::mixer("pan"));
        assert!(stray.check().is_err());
        let mut nameless = layout.clone();
        nameless.groups[0].name = " ".into();
        assert!(nameless.check().is_err());
        let mut two_groups = layout;
        two_groups.groups.push(LaneGroup { name: "Again".into(), ..two_groups.groups[0].clone() });
        assert!(two_groups.check().is_err());
    }
}
