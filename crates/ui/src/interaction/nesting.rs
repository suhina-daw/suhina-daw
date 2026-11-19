// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Groups inside groups in the track list, for the arrangement's track headers and the session
//! view's column headers alike: which tracks a folded group hides, the arrow that folds a
//! group, and what a track header let go among the others asks for (a move in the track
//! order and hierarchy with `track.moveTo`).

use std::collections::BTreeSet;

use egui::{Color32, Id, Painter, Rect, Shape, Stroke, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{Project, TrackId};

use crate::kit::widgets::{self, Action};

/// Where a track sits in the track list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub track: TrackId,
    /// The group it is inside.
    pub parent: Option<TrackId>,
    /// How many groups it is inside.
    pub depth: usize,
    /// Its place in the project's track order, as `track.move` counts it.
    pub position: usize,
    pub group: bool,
}

/// The tracks inside a folded group, at any depth. `places` are the tracks in track order.
pub fn hidden(places: &[Place], folded: &BTreeSet<TrackId>) -> BTreeSet<TrackId> {
    if folded.is_empty() {
        return BTreeSet::new();
    }
    let inside_folded = |place: &Place| {
        let mut parent = place.parent;
        // Bounded, so a parent cycle cannot loop.
        for _ in 0..=places.len() {
            match parent {
                Some(group) if folded.contains(&group) => return true,
                Some(group) => parent = places.iter().find(|p| p.track == group).and_then(|p| p.parent),
                None => return false,
            }
        }
        false
    };
    places.iter().filter(|p| inside_folded(p)).map(|p| p.track).collect()
}

/// `track` and the tracks nested under it, as a range of indices into `places`.
fn block(places: &[Place], track: TrackId) -> Option<std::ops::Range<usize>> {
    let index = places.iter().position(|p| p.track == track)?;
    let depth = places.get(index)?.depth;
    let len = 1 + places.iter().skip(index + 1).take_while(|p| p.depth > depth).count();
    Some(index..index + len)
}

/// The selected roots captured when a header drag begins. A group's descendants travel with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackDrag {
    pub source: TrackId,
    pub tracks: Vec<TrackId>,
}

impl TrackDrag {
    pub fn new(places: &[Place], source: TrackId, selected: &[TrackId]) -> Self {
        let selected = if selected.contains(&source) { selected } else { std::slice::from_ref(&source) };
        let mut covered = 0;
        let mut tracks = Vec::new();
        for (index, place) in places.iter().enumerate() {
            if index >= covered && selected.contains(&place.track) {
                tracks.push(place.track);
                covered = block(places, place.track).map_or(index + 1, |range| range.end);
            }
        }
        Self { source, tracks }
    }
}

/// A container's middle half accepts children, independently of whether it is folded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Before,
    Inside,
    After,
}

impl Placement {
    pub fn at(fraction: f32, group: bool) -> Self {
        if group && (0.25..=0.75).contains(&fraction) {
            Self::Inside
        } else if fraction < 0.5 {
            Self::Before
        } else {
            Self::After
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Before => "before",
            Self::Inside => "inside",
            Self::After => "after",
        }
    }
}

/// One command for the whole selection. Moving subtrees cannot be their own targets.
pub fn drop_action(places: &[Place], dragged: &[TrackId], target: TrackId, placement: Placement) -> Option<Action> {
    let target_index = places.iter().position(|p| p.track == target)?;
    if dragged.is_empty()
        || dragged.iter().any(|&track| block(places, track).is_none_or(|range| range.contains(&target_index)))
        || (placement == Placement::Inside && !places.get(target_index)?.group)
    {
        return None;
    }
    Some(Action::new(
        "track.moveTo",
        json!({
            "tracks": dragged.iter().map(|id| id.0).collect::<Vec<_>>(),
            "target": target.0,
            "placement": placement.name(),
        }),
    ))
}

/// The groups `track` can move into from its menu: every group but the one it is in, itself
/// and the groups inside it.
pub fn move_targets(places: &[Place], track: TrackId) -> Vec<TrackId> {
    let own = block(places, track).unwrap_or(0..0);
    let parent = places.iter().find(|p| p.track == track).and_then(|p| p.parent);
    places
        .iter()
        .enumerate()
        .filter(|(i, p)| p.group && !own.contains(i) && Some(p.track) != parent)
        .map(|(_, p)| p.track)
        .collect()
}

/// Taking `track` out of its group, one level up: it lands right after the group it leaves.
/// `None` for a track in no group.
pub fn out_of_group(places: &[Place], track: TrackId) -> Option<Action> {
    let parent = places.iter().find(|p| p.track == track)?.parent?;
    let grandparent = places.iter().find(|p| p.track == parent)?.parent;
    let index = places.iter().filter(|p| p.parent == grandparent).position(|p| p.track == parent)? + 1;
    let group = grandparent.map_or_else(|| json!("none"), |g| json!(g.0));
    Some(Action::new("track.setGroup", json!({ "track": track.0, "group": group, "index": index })))
}

/// The arrow on a group's header: pointing right while the group is folded (its tracks
/// hidden), down while it is open. A click folds or unfolds it.
#[allow(clippy::too_many_arguments)]
pub fn fold_arrow(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    id: Id,
    track: TrackId,
    name: &str,
    color: Color32,
    folded: &mut BTreeSet<TrackId>,
) {
    let is_folded = folded.contains(&track);
    let label = if is_folded { format!("Unfold {name}") } else { format!("Fold {name}") };
    let response = widgets::button_area(ui, rect, id, label);
    if response.clicked() && !folded.remove(&track) {
        folded.insert(track);
    }
    let size = if widgets::hover(ui, &response) > 0.5 { 4.0 } else { 3.5 };
    let c = rect.center();
    let points = if is_folded {
        vec![c + Vec2::new(-size * 0.6, -size), c + Vec2::new(size * 0.9, 0.0), c + Vec2::new(-size * 0.6, size)]
    } else {
        vec![c + Vec2::new(-size, -size * 0.6), c + Vec2::new(size, -size * 0.6), c + Vec2::new(0.0, size * 0.9)]
    };
    painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
}

/// How many groups a track is nested in; a broken chain stops counting.
pub(crate) fn depth(project: &Project, mut parent: Option<TrackId>) -> usize {
    let mut depth = 0;
    while let Some(id) = parent {
        depth += 1;
        if depth > project.tracks.len() {
            break;
        }
        parent = project.track(id).and_then(|t| t.parent);
    }
    depth
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Band (group) holds Drums (group) holding Kick, and Bass; Pad is at the top level.
    fn places() -> Vec<Place> {
        let place = |track: u32, parent: Option<u32>, depth: usize, position: usize, group: bool| Place {
            track: TrackId(track),
            parent: parent.map(TrackId),
            depth,
            position,
            group,
        };
        vec![
            place(1, None, 0, 0, true),     // Band
            place(2, Some(1), 1, 1, true),  // Drums
            place(3, Some(2), 2, 2, false), // Kick
            place(4, Some(1), 1, 3, false), // Bass
            place(5, None, 0, 4, false),    // Pad
        ]
    }

    fn ids(list: &[u32]) -> BTreeSet<TrackId> {
        list.iter().map(|&t| TrackId(t)).collect()
    }

    fn act(id: &'static str, params: serde_json::Value) -> Option<Action> {
        Some(Action::new(id, params))
    }

    #[test]
    fn a_folded_group_hides_everything_inside_it() {
        let places = places();
        assert_eq!(hidden(&places, &ids(&[])), ids(&[]));
        assert_eq!(hidden(&places, &ids(&[2])), ids(&[3]));
        assert_eq!(hidden(&places, &ids(&[1])), ids(&[2, 3, 4]));
        // A plain track that is "folded" hides nothing.
        assert_eq!(hidden(&places, &ids(&[5])), ids(&[]));
    }

    #[test]
    fn a_drag_captures_selected_roots_in_project_order() {
        let places = places();
        let drag = TrackDrag::new(&places, TrackId(3), &[TrackId(5), TrackId(3), TrackId(1)]);
        assert_eq!(drag.tracks, [TrackId(1), TrackId(5)]);
        assert_eq!(TrackDrag::new(&places, TrackId(4), &[TrackId(5)]).tracks, [TrackId(4)]);
    }

    #[test]
    fn drops_distinguish_inside_from_before_and_after_without_fold_state() {
        let places = places();
        for (fraction, placement) in [(0.1, Placement::Before), (0.5, Placement::Inside), (0.9, Placement::After)] {
            assert_eq!(Placement::at(fraction, true), placement);
            assert_eq!(
                drop_action(&places, &[TrackId(4), TrackId(5)], TrackId(2), placement),
                act("track.moveTo", json!({"tracks": [4, 5], "target": 2, "placement": placement.name()}))
            );
        }
        assert_eq!(Placement::at(0.4, false), Placement::Before);
        assert_eq!(Placement::at(0.6, false), Placement::After);
        assert_eq!(drop_action(&places, &[TrackId(1)], TrackId(3), Placement::Before), None);
        assert_eq!(drop_action(&places, &[TrackId(4)], TrackId(4), Placement::After), None);
        assert_eq!(drop_action(&places, &[TrackId(9)], TrackId(1), Placement::Inside), None);
        assert_eq!(drop_action(&places, &[TrackId(4)], TrackId(5), Placement::Inside), None);
    }

    #[test]
    fn the_menu_offers_other_groups_and_a_way_out() {
        let places = places();
        assert_eq!(move_targets(&places, TrackId(5)), [TrackId(1), TrackId(2)]);
        assert_eq!(move_targets(&places, TrackId(3)), [TrackId(1)]);
        assert_eq!(move_targets(&places, TrackId(1)), Vec::<TrackId>::new());
        // Kick out of Drums lands in Band, right after Drums.
        assert_eq!(
            out_of_group(&places, TrackId(3)),
            act("track.setGroup", json!({ "track": 3, "group": 1, "index": 1 }))
        );
        // Bass out of Band lands at the top level, right after Band.
        assert_eq!(
            out_of_group(&places, TrackId(4)),
            act("track.setGroup", json!({ "track": 4, "group": "none", "index": 1 }))
        );
        assert_eq!(out_of_group(&places, TrackId(5)), None);
    }
}
