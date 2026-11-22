// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Playlist presentation, independently of audio routing. Evidence: flp-model-mapping.md.
use crate::{
    Flp, FlpError, Import,
    records::{data, dword, error, number},
};
use std::collections::{BTreeMap, BTreeSet};
use suhina_model::{Rgb, Track, TrackId};

#[derive(Default)]
pub(crate) struct Row {
    pub name: String,
    pub color: Option<Rgb>,
    pub grouped: bool,
}

pub(crate) fn rows(flp: &Flp, selected: u32) -> Result<BTreeMap<u32, Row>, FlpError> {
    let mut rows = BTreeMap::new();
    let mut arrangement = flp.first(99).map(number).transpose()?.unwrap_or(0);
    let mut current = None;
    let mut stored_color = None;
    for e in &flp.events {
        if e.id == 99 {
            arrangement = number(e)?;
            current = None;
        }
        if arrangement != selected {
            continue;
        }
        match e.id {
            238 => {
                let d = data(e)?;
                let row = dword(d, 0, e.offset)?
                    .checked_sub(1)
                    .filter(|v| *v < 500)
                    .ok_or_else(|| error(e.offset, "invalid playlist row"))?;
                let rgb = dword(d, 4, e.offset)?;
                stored_color = Some(Rgb(rgb as u8, (rgb >> 8) as u8, (rgb >> 16) as u8));
                // A row's color is its own unless it is FL's gray; event 43, where FL writes it,
                // says whether it is enabled.
                rows.insert(
                    row,
                    Row {
                        grouped: d.get(46).is_some_and(|v| *v != 0),
                        color: stored_color.filter(|c| !crate::import::is_fl_default(*c)),
                        ..Row::default()
                    },
                );
                current = Some(row);
            }
            43 => {
                if let Some(row) = current.and_then(|id| rows.get_mut(&id)) {
                    row.color =
                        if number(e)? != 0 { stored_color.filter(|c| !crate::import::is_fl_default(*c)) } else { None };
                }
            }
            239 => {
                if let Some(row) = current.and_then(|id| rows.get_mut(&id)) {
                    row.name = e.text(flp.encoding()?)?;
                }
            }
            _ => {}
        }
    }
    Ok(rows)
}

pub(crate) fn apply(import: &mut Import, rows: &BTreeMap<u32, Row>) {
    let p = &mut import.project;
    let mut parent = None;
    let mut groups = BTreeSet::new();
    let mut row_group = BTreeMap::new();
    for (&index, row) in rows {
        if row.grouped {
            if let Some(root) = parent {
                groups.insert(root);
                row_group.insert(index, root);
            }
        } else {
            parent = Some(index);
        }
    }
    for &root in &groups {
        row_group.insert(root, root);
    }
    let mut memberships: BTreeMap<TrackId, BTreeSet<Option<u32>>> = BTreeMap::new();
    let mut order = BTreeMap::new();
    for lane in &mut p.arrangement.lanes {
        let Some(track) = lane.track else { continue };
        for clip in &mut lane.clips {
            let Some(index) = clip
                .extensions
                .0
                .iter()
                .find(|e| e.format == "flp" && e.key == "playlist-index")
                .and_then(|e| e.data.as_slice().try_into().ok())
                .map(u32::from_le_bytes)
            else {
                continue;
            };
            clip.color = rows.get(&index).and_then(|row| row.color).or(clip.color);
            order.entry(track).and_modify(|v: &mut u32| *v = (*v).min(index)).or_insert(index);
            memberships.entry(track).or_default().insert(row_group.get(&index).copied());
        }
    }
    for track in &mut p.tracks {
        if let Some(row) = order.get(&track.id).and_then(|i| rows.get(i)) {
            track.color = row.color.or(track.color);
        }
    }
    // Preserve each existing audio-bus family. A family crossing visual groups stays outside
    // them rather than duplicating its instruments or changing a single routing edge.
    let roots: Vec<_> = p.tracks.iter().filter(|t| t.parent.is_none()).map(|t| t.id).collect();
    for root in roots {
        let family = p.with_descendants(root);
        let membership: BTreeSet<_> =
            family.iter().filter_map(|id| memberships.get(id)).flat_map(|m| m.iter().copied()).collect();
        let first = family.iter().filter_map(|id| order.get(id)).copied().min();
        if let Some(first) = first {
            order.insert(root, first);
            if let Some(track) = p.track_mut(root) {
                track.color = rows.get(&first).and_then(|row| row.color).or(track.color);
            }
        }
        if membership.len() == 1 {
            if let Some(Some(group)) = membership.first() {
                if p.nesting_below(root) < suhina_model::MAX_GROUP_DEPTH {
                    if let Some(track) = p.track_mut(root) {
                        track.parent = Some(TrackId(196_608 + group));
                    }
                } else {
                    import.warnings.push(
                        "A mixer family already reaches the nesting limit and stays outside its playlist folder."
                            .into(),
                    );
                }
            }
        } else if membership.len() > 1 && membership.iter().any(Option::is_some) {
            import.warnings.push(
                "A shared source or mixer group spans several playlist groups; it stays together outside those groups."
                    .into(),
            );
        }
    }
    for group in groups {
        let Some(row) = rows.get(&group) else { continue };
        let id = TrackId(196_608 + group);
        let mut folder =
            Track::new(id, if row.name.is_empty() { format!("Playlist {}", group + 1) } else { row.name.clone() });
        folder.color = row.color;
        folder.folder = true;
        order.insert(id, group);
        p.tracks.push(folder);
    }
    // Stable preorder keeps every family contiguous and returns unrelated sources last.
    let mut children: BTreeMap<Option<TrackId>, Vec<Track>> = BTreeMap::new();
    for track in std::mem::take(&mut p.tracks) {
        children.entry(track.parent).or_default().push(track);
    }
    for siblings in children.values_mut() {
        siblings.sort_by_key(|t| order.get(&t.id).copied().unwrap_or(u32::MAX));
    }
    let mut pending = children.remove(&None).unwrap_or_default();
    pending.reverse();
    while let Some(track) = pending.pop() {
        if let Some(mut nested) = children.remove(&Some(track.id)) {
            nested.reverse();
            pending.extend(nested);
        }
        p.tracks.push(track);
    }
}

/// Every track still without a color (FL's gray, or none given) gets Suhina's own: an audio
/// track the color a new audio track gets, any other the new MIDI track's; a folder or a group
/// the color of its first track that has one, so a family reads as one.
pub(crate) fn default_colors(p: &mut suhina_model::Project) {
    for track in &mut p.tracks {
        let parent_like = track.channel.is_none() || track.content == suhina_model::ContentKinds::default();
        if track.color.is_none() && !parent_like {
            track.color = Some(if track.content.audio && !track.content.notes {
                suhina_model::NEW_AUDIO_COLOR
            } else {
                suhina_model::NEW_MIDI_COLOR
            });
        }
    }
    // Deepest first, so a folder above a group takes the group's color.
    let mut parents: Vec<(usize, TrackId)> =
        p.tracks.iter().filter(|t| t.color.is_none()).map(|t| (p.with_descendants(t.id).len(), t.id)).collect();
    parents.sort();
    for (_, id) in parents {
        let first =
            p.tracks.iter().filter(|t| t.id != id && p.with_descendants(id).contains(&t.id)).find_map(|t| t.color);
        if let Some(track) = p.track_mut(id) {
            track.color = Some(first.unwrap_or(suhina_model::NEW_MIDI_COLOR));
        }
    }
}
