// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Groups inside groups in the track list, for the arrangement's track headers and the session
//! view's column headers alike: which tracks a folded group hides, the arrow that folds a
//! group, and what a track header let go among the others asks for (a move in the track
//! order, or a move into another group with `track.setGroup`).

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

/// What letting `dragged`'s header go over `target`'s asks for: a place before `target`, or
/// after it when `after`. After the header of an open group is into that group, as its first
/// track; anywhere else the dragged track joins `target`'s group, with a move in the track order
/// when that is the group it is in already and `track.setGroup` when not. `places` are every
/// track (return tracks left out) in track order. `None` when nothing changes, or when `target`
/// is inside the dragged group.
pub fn drop_action(
    places: &[Place],
    dragged: TrackId,
    target: TrackId,
    after: bool,
    folded: &BTreeSet<TrackId>,
) -> Option<Action> {
    let source_block = block(places, dragged)?;
    let source = places.get(source_block.start)?;
    let target_block = block(places, target)?;
    let target = places.get(target_block.start)?;
    if source_block.contains(&target_block.start) {
        return None;
    }
    if after && target.group && !folded.contains(&target.track) {
        return Some(Action::new("track.setGroup", json!({ "track": dragged.0, "group": target.track.0, "index": 0 })));
    }
    if target.parent == source.parent {
        // After a folded group is after everything inside it.
        let mut to = if after {
            places.get(target_block.end - 1).map_or(target.position, |last| last.position) + 1
        } else {
            target.position
        };
        if target.position > source.position {
            to = to.saturating_sub(source_block.len());
        }
        return (to != source.position).then(|| Action::new("track.move", json!({ "track": dragged.0, "to": to })));
    }
    let mut siblings = places.iter().filter(|p| p.parent == target.parent && p.track != dragged);
    let index = siblings.position(|p| p.track == target.track)? + usize::from(after);
    let group = target.parent.map_or_else(|| json!("none"), |g| json!(g.0));
    Some(Action::new("track.setGroup", json!({ "track": dragged.0, "group": group, "index": index })))
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
    fn a_drop_moves_within_a_group_or_into_another() {
        let places = places();
        let open = ids(&[]);
        // Pad after Drums' header: into Drums, first.
        assert_eq!(
            drop_action(&places, TrackId(5), TrackId(2), true, &open),
            act("track.setGroup", json!({ "track": 5, "group": 2, "index": 0 }))
        );
        // Pad before Bass: into Band, between Drums and Bass.
        assert_eq!(
            drop_action(&places, TrackId(5), TrackId(4), false, &open),
            act("track.setGroup", json!({ "track": 5, "group": 1, "index": 1 }))
        );
        // Kick before Band: out of both groups, first at the top level.
        assert_eq!(
            drop_action(&places, TrackId(3), TrackId(1), false, &open),
            act("track.setGroup", json!({ "track": 3, "group": "none", "index": 0 }))
        );
        // Bass before Drums: the same group, a move in the track order to Drums' place.
        assert_eq!(
            drop_action(&places, TrackId(4), TrackId(2), false, &open),
            act("track.move", json!({ "track": 4, "to": 1 }))
        );
        // Pad after a folded Band: after Band and everything in it, where Pad already is.
        assert_eq!(drop_action(&places, TrackId(5), TrackId(1), true, &ids(&[1])), None);
        // Band before Pad: Band with its three tracks moves to just before Pad, where it is.
        assert_eq!(drop_action(&places, TrackId(1), TrackId(5), false, &open), None);
        // Band after Pad: to the end.
        assert_eq!(
            drop_action(&places, TrackId(1), TrackId(5), true, &open),
            act("track.move", json!({ "track": 1, "to": 1 }))
        );
        // A group cannot go into itself or a group inside it.
        assert_eq!(drop_action(&places, TrackId(1), TrackId(2), true, &open), None);
        assert_eq!(drop_action(&places, TrackId(1), TrackId(3), false, &open), None);
        assert_eq!(drop_action(&places, TrackId(9), TrackId(1), false, &open), None);
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
