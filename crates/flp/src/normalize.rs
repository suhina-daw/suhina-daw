// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Put imported placements on the tracks that own their sources and mixer controls.
use crate::{FlpError, Import, records::error};
use std::collections::{BTreeMap, BTreeSet};
use suhina_model::{ArrangementLane, ChannelId, ClipContent, ClipId, ContentId, Extension, TrackId};

pub(crate) fn apply(import: &mut Import) -> Result<(), FlpError> {
    let p = &mut import.project;
    let contents: BTreeMap<_, _> = p.contents.iter().map(|c| (c.id, c.clone())).collect();
    let mut next = p.contents.iter().map(|c| c.id.0).max().unwrap_or(0);
    let mut split = BTreeMap::new();
    let mut replacements: BTreeMap<ContentId, ClipContent> = BTreeMap::new();
    let mut lanes: BTreeMap<TrackId, ArrangementLane> = BTreeMap::new();
    let mut order = Vec::new();
    let mut memberships: BTreeMap<TrackId, BTreeSet<Option<TrackId>>> = BTreeMap::new();
    let mut members: BTreeMap<TrackId, BTreeSet<TrackId>> = BTreeMap::new();
    let mut replaced = BTreeSet::new();
    let mut count = 0usize;
    for row in &p.arrangement.lanes {
        let parent = row.track.and_then(|id| p.track(id)).and_then(|t| t.parent);
        for placed in &row.clips {
            let original =
                contents.get(&placed.content).ok_or_else(|| error(0, "missing content during source mapping"))?;
            let targets: BTreeSet<_> = if original.lanes.is_empty() {
                original.id.0.checked_sub(65_536).map(TrackId).into_iter().collect()
            } else {
                original.lanes.iter().filter_map(|l| l.target).collect()
            };
            for target in targets {
                if p.track(target).is_none() {
                    return Err(error(0, "missing source during track mapping"));
                }
                count += 1;
                if count > 1_000_000 {
                    return Err(error(0, "too many split placements"));
                }
                memberships.entry(target).or_default().insert(parent);
                if let Some(group) = parent {
                    members.entry(group).or_default().insert(target);
                }
                let content_id = if let Some(id) = split.get(&(original.id, target)) {
                    *id
                } else {
                    let id = if original.lanes.len() <= 1 {
                        original.id
                    } else {
                        next = next.checked_add(1).ok_or_else(|| error(0, "content id limit exceeded"))?;
                        replaced.insert(original.id);
                        ContentId(next)
                    };
                    let mut content = original.clone();
                    content.id = id;
                    content.lanes = original
                        .lanes
                        .iter()
                        .filter(|l| l.target == Some(target))
                        .cloned()
                        .map(|mut l| {
                            l.target = None;
                            l
                        })
                        .collect();
                    if original.lanes.len() > 1 {
                        content.name =
                            format!("{} — {}", content.name, p.track(target).map_or("Source", |t| t.name.as_str()));
                    }
                    replacements.insert(id, content);
                    split.insert((original.id, target), id);
                    id
                };
                if !lanes.contains_key(&target) {
                    order.push(target);
                }
                let lane = lanes.entry(target).or_insert_with(|| ArrangementLane {
                    track: Some(target),
                    name: p.track(target).map_or_else(String::new, |t| t.name.clone()),
                    ..Default::default()
                });
                let mut clip = placed.clone();
                clip.id = ClipId::NONE;
                clip.content = content_id;
                clip.extensions.0.push(Extension {
                    format: "flp".into(),
                    key: "playlist-row".into(),
                    data: row.name.as_bytes().to_vec(),
                });
                lane.clips.push(clip);
            }
        }
    }
    p.contents.retain(|c| !replaced.contains(&c.id));
    for content in &mut p.contents {
        if let Some(replacement) = replacements.remove(&content.id) {
            *content = replacement;
        }
    }
    p.contents.extend(replacements.into_values());
    let mut keep_groups = BTreeSet::new();
    for (group, children) in &members {
        let exclusive = children.iter().all(|id| memberships.get(id).is_some_and(|m| m.len() == 1));
        let outputs: BTreeSet<_> = children
            .iter()
            .filter_map(|id| p.track(*id))
            .filter(|t| t.content.notes || t.content.audio)
            .filter_map(|t| p.channel_of_track(t.id))
            .map(|c| c.output)
            .collect();
        if exclusive && outputs.len() == 1 {
            let output = outputs.iter().next().copied().flatten();
            if let Some(channel) = p.channel_mut(ChannelId(group.0)) {
                channel.output = output;
            }
            for id in children {
                if let Some(track) = p.tracks.iter_mut().find(|t| t.id == *id) {
                    track.parent = Some(*group);
                }
                if let Some(channel) = p.channel_of_track_mut(*id) {
                    channel.output = Some(ChannelId(group.0));
                }
            }
            keep_groups.insert(*group);
        } else {
            import.warnings.push(format!("Playlist group {} was flattened: shared sources or different mixer destinations must keep their routing.", (group.0 - 65_536) / 4 + 1));
        }
    }
    let visual = |id: u32| (65_536..131_072).contains(&id);
    p.tracks.retain(|t| !visual(t.id.0) || keep_groups.contains(&t.id));
    p.channels.retain(|c| !visual(c.id.0) || keep_groups.contains(&TrackId(c.id.0)));
    let mut tracks: BTreeMap<_, _> = std::mem::take(&mut p.tracks).into_iter().map(|t| (t.id, t)).collect();
    for id in &order {
        if let Some(group) = tracks.get(id).and_then(|t| t.parent)
            && let Some(track) = tracks.remove(&group)
        {
            p.tracks.push(track);
        }
        if let Some(track) = tracks.remove(id) {
            p.tracks.push(track);
        }
    }
    p.tracks.extend(tracks.into_values());
    p.arrangement.lanes = order
        .into_iter()
        .filter_map(|id| lanes.remove(&id))
        .map(|mut lane| {
            lane.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
            lane
        })
        .collect();
    Ok(())
}
