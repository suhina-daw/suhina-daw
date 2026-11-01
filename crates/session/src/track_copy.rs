// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Whole tracks copied, pasted and duplicated (#919): a track with its mixer channel, devices,
//! automation, arrangement clips, take lanes and clip slots, a group with the tracks in it, and
//! the clip contents they play, each copy with contents of its own.

use std::collections::BTreeMap;

use suhina_model::{
    ArrangementLane, Channel, ChannelId, ChannelRole, ClipContent, ClipId, ContentId, LaneId, LauncherColumn, Project,
    Slot, TakeLane, Track, TrackId,
};

use crate::CommandError;
use crate::files::{MAX_TRACKS, insert_position, within};

/// One copied track as it was: its channel, its arrangement lane, its take lanes and its slots.
#[derive(Debug, Clone)]
struct CopiedTrack {
    track: Track,
    channel: Option<Channel>,
    lane: Option<ArrangementLane>,
    takes: Vec<TakeLane>,
    column: Option<LauncherColumn>,
}

/// What `track.copy` and `track.cut` hold for `track.paste`: the tracks in project order (a
/// group followed by the tracks in it) and the clip contents their clips play.
#[derive(Debug, Clone, Default)]
pub(crate) struct TrackClipboard {
    tracks: Vec<CopiedTrack>,
    contents: Vec<ClipContent>,
}

impl TrackClipboard {
    /// How many tracks a paste adds.
    pub(crate) fn len(&self) -> usize {
        self.tracks.len()
    }

    /// The copied tracks no other copied track holds: the ones named, without the tracks in
    /// their groups.
    fn tops(&self) -> impl Iterator<Item = &CopiedTrack> {
        let ids: Vec<TrackId> = self.tracks.iter().map(|t| t.track.id).collect();
        self.tracks.iter().filter(move |t| !t.track.parent.is_some_and(|p| ids.contains(&p)))
    }
}

/// `tracks` of `p` and every track inside the groups among them, in project order.
pub(crate) fn with_their_tracks(p: &Project, tracks: &[TrackId]) -> Vec<TrackId> {
    p.tracks
        .iter()
        .filter(|t| tracks.contains(&t.id) || tracks.iter().any(|&group| within(p, t.id, group)))
        .map(|t| t.id)
        .collect()
}

/// Copies `tracks` (and the tracks in the groups among them). Return tracks and Main are not
/// copied.
pub(crate) fn copy(p: &Project, tracks: &[TrackId]) -> Result<TrackClipboard, CommandError> {
    let ids = with_their_tracks(p, tracks);
    if ids.is_empty() {
        return Err("no track to copy".into());
    }
    let mut copied = Vec::new();
    let mut used: Vec<ContentId> = Vec::new();
    for id in ids {
        let track = p.track(id).ok_or("a track to copy is missing")?.clone();
        let channel = track.channel.and_then(|c| p.channel(c)).cloned();
        if channel.as_ref().is_some_and(|c| matches!(c.role, ChannelRole::Effect | ChannelRole::Master)) {
            return Err(format!("{:?} is a return track or Main, which are not copied", track.name).into());
        }
        let lane = p.arrangement.lanes.iter().find(|l| l.track == Some(id)).cloned();
        let takes: Vec<TakeLane> = p.arrangement.takes.iter().filter(|t| t.track == id).cloned().collect();
        let column = p.launcher.columns.iter().find(|c| c.track == id).cloned();
        let clips = lane
            .iter()
            .flat_map(|l| &l.clips)
            .chain(takes.iter().flat_map(|t| &t.clips))
            .chain(column.iter().flat_map(|c| c.slots.iter().filter_map(|s| s.clip.as_ref())));
        for clip in clips {
            if !used.contains(&clip.content) {
                used.push(clip.content);
            }
        }
        copied.push(CopiedTrack { track, channel, lane, takes, column });
    }
    let contents = p.contents.iter().filter(|c| used.contains(&c.id)).cloned().collect();
    Ok(TrackClipboard { tracks: copied, contents })
}

/// Adds the tracks of `clipboard` to `p` right after `after` (in its group), or at the end of
/// the regular tracks, with new ids, new clip contents and names as [`copy_name`] gives them.
/// Returns the new tracks no other new track holds, in order. A send to a return `p` lacks is
/// left out; a track plays into its group, as copied, or into where it is pasted.
pub(crate) fn paste(
    p: &mut Project,
    clipboard: &TrackClipboard,
    after: Option<TrackId>,
    suffix: Suffix,
) -> Result<Vec<TrackId>, CommandError> {
    if clipboard.tracks.is_empty() {
        return Err("no track has been copied; run track.copy first".into());
    }
    if p.tracks.len().saturating_add(clipboard.tracks.len()) > MAX_TRACKS {
        return Err(format!("a project holds at most {MAX_TRACKS} tracks").into());
    }
    let (at, parent) = insert_position(p, after)?;
    let main = p.channels.iter().find(|c| c.role == ChannelRole::Master).map(|c| c.id);
    let into = match parent {
        Some(group) => p.track(group).and_then(|t| t.channel),
        None => main,
    };
    // New ids for the tracks, their channels and the contents, from past the largest in use.
    let mut next_track = p.tracks.iter().map(|t| t.id.0).max().unwrap_or(0);
    let mut next_channel = p.channels.iter().map(|c| c.id.0).max().unwrap_or(0);
    let mut next_content = p.contents.iter().map(|c| c.id.0).max().unwrap_or(0);
    let mut tracks: BTreeMap<TrackId, TrackId> = BTreeMap::new();
    let mut channels: BTreeMap<ChannelId, ChannelId> = BTreeMap::new();
    for copied in &clipboard.tracks {
        next_track = next_track.checked_add(1).ok_or("too many tracks")?;
        tracks.insert(copied.track.id, TrackId(next_track));
        if let Some(channel) = &copied.channel {
            next_channel = next_channel.checked_add(1).ok_or("too many channels")?;
            channels.insert(channel.id, ChannelId(next_channel));
        }
    }
    let mut contents: BTreeMap<ContentId, ContentId> = BTreeMap::new();
    for content in &clipboard.contents {
        next_content = next_content.checked_add(1).ok_or("too many clips")?;
        contents.insert(content.id, ContentId(next_content));
    }
    let content_of = |id: ContentId| contents.get(&id).copied().unwrap_or(id);
    let returns: Vec<ChannelId> = p.channels.iter().filter(|c| c.role == ChannelRole::Effect).map(|c| c.id).collect();
    let scenes = p.launcher.scenes.len();
    let mut names: Vec<String> = p.tracks.iter().map(|t| t.name.clone()).collect();
    let mut added = Vec::new();
    let mut tops = Vec::new();
    for copied in &clipboard.tracks {
        let id = tracks.get(&copied.track.id).copied().ok_or("a copied track has no new id")?;
        let inner = copied.track.parent.and_then(|old| tracks.get(&old).copied());
        let kind = if copied.track.content.audio { "Audio" } else { "MIDI" };
        let name = copy_name(&copied.track.name, kind, &names, suffix);
        names.push(name.clone());
        let channel = copied.channel.as_ref().map(|c| {
            let output = match c.output.and_then(|o| channels.get(&o).copied()) {
                Some(group) => Some(group),
                None if inner.is_some() => c.output,
                None => into,
            };
            Channel {
                id: channels.get(&c.id).copied().unwrap_or(c.id),
                name: name.clone(),
                output,
                solo: false,
                sends: c.sends.iter().filter(|s| returns.contains(&s.target)).cloned().collect(),
                ..c.clone()
            }
        });
        let track = Track {
            id,
            name: name.clone(),
            channel: channel.as_ref().map(|c| c.id),
            parent: inner.or(parent),
            ..copied.track.clone()
        };
        if inner.is_none() {
            tops.push(id);
        }
        let fresh = |clip: &suhina_model::Clip| suhina_model::Clip {
            id: ClipId::NONE,
            content: content_of(clip.content),
            ..clip.clone()
        };
        if let Some(lane) = &copied.lane {
            p.arrangement.lanes.push(ArrangementLane {
                id: LaneId::NONE,
                name: name.clone(),
                track: Some(id),
                clips: lane.clips.iter().map(fresh).collect(),
            });
        }
        for take in &copied.takes {
            p.arrangement.takes.push(TakeLane {
                track: id,
                clips: take.clips.iter().map(fresh).collect(),
                ..take.clone()
            });
        }
        let mut slots: Vec<Slot> = copied
            .column
            .iter()
            .flat_map(|c| &c.slots)
            .map(|s| Slot { clip: s.clip.as_ref().map(fresh), ..s.clone() })
            .take(scenes)
            .collect();
        slots.resize(scenes, Slot { clip: None, has_stop: true });
        p.launcher.columns.push(LauncherColumn { track: id, slots });
        if let Some(channel) = channel {
            p.channels.push(channel);
        }
        added.push(track);
    }
    for content in &clipboard.contents {
        let mut copy = content.clone();
        copy.id = content_of(content.id);
        for lane in &mut copy.lanes {
            if let Some(target) = lane.target.and_then(|t| tracks.get(&t).copied()) {
                lane.target = Some(target);
            }
        }
        p.contents.push(copy);
    }
    let at = at.min(p.tracks.len());
    for (offset, track) in added.into_iter().enumerate() {
        p.tracks.insert(at + offset, track);
    }
    p.assign_ids().map_err(|e| e.to_string())?;
    Ok(tops)
}

/// Copies `tracks` (a group with its tracks) as one block right after the last of them, in
/// their order, and returns the copies: tracks 1 and 3 give 1, 2, 3, the copy of 1, the copy
/// of 3, as a copy and a paste after track 3 do.
pub(crate) fn duplicate(p: &mut Project, tracks: &[TrackId], suffix: Suffix) -> Result<Vec<TrackId>, CommandError> {
    let ids = with_their_tracks(p, tracks);
    // Only the named tracks that no other named track holds: a group's copy brings its tracks.
    let tops: Vec<TrackId> = ids.iter().copied().filter(|&t| !ids.iter().any(|&g| g != t && within(p, t, g))).collect();
    let Some(&after) = tops.last() else { return Err("no track to duplicate".into()) };
    let clipboard = copy(p, &tops)?;
    paste(p, &clipboard, Some(after), suffix)
}

/// How a copy's name says it is a copy when the name is taken (Settings › Arrangement).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Suffix {
    /// "Bass 1 (2)".
    #[default]
    Wrapped,
    /// "Bass 1 #2".
    Hash,
}

impl Suffix {
    /// The suffix named `name` ("wrapped", "hash").
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "wrapped" => Some(Self::Wrapped),
            "hash" => Some(Self::Hash),
            _ => None,
        }
    }

    fn with(self, base: &str, n: u32) -> String {
        match self {
            Self::Wrapped => format!("{base} ({n})"),
            Self::Hash => format!("{base} #{n}"),
        }
    }
}

/// The name a copy of a `kind` track ("MIDI", "Audio") named `name` takes among the tracks
/// named `taken` (#946): a name with the track's number in it ("# Bass") stays, as the number
/// tells the copies apart; a name Suhina gave a new track before names held the number ("Audio
/// 3", "3 MIDI") becomes the new tracks' name ("# MIDI"); any other name stays when it is free
/// and otherwise takes the first free suffix after its own, so a copy of "Bass (2)" is "Bass (3)"
/// rather than "Bass (2) (2)".
fn copy_name(name: &str, kind: &str, taken: &[String], suffix: Suffix) -> String {
    if suhina_model::names::has_number_mark(name) {
        return name.to_string();
    }
    if old_default_name(name) {
        return suhina_model::names::new_track_name(kind);
    }
    if !taken.iter().any(|t| t == name) {
        return name.to_string();
    }
    let (base, own) = split_suffix(name);
    (own.map_or(2, |n| n.saturating_add(1))..)
        .map(|n| suffix.with(base, n))
        .find(|candidate| !taken.contains(candidate))
        .unwrap_or_else(|| name.to_string())
}

/// `name` without a copy's suffix of either kind, and its number: "Bass 1 (2)" and "Bass 1 #2"
/// are "Bass 1" and 2.
fn split_suffix(name: &str) -> (&str, Option<u32>) {
    if let Some(inner) = name.strip_suffix(')')
        && let Some((base, n)) = inner.rsplit_once(" (")
        && let Ok(n) = n.parse::<u32>()
        && !base.is_empty()
    {
        return (base, Some(n));
    }
    match name.rsplit_once(" #") {
        Some((base, n)) if !base.is_empty() => match n.parse::<u32>() {
            Ok(n) => (base, Some(n)),
            Err(_) => (name, None),
        },
        _ => (name, None),
    }
}

/// Whether `name` is one new tracks were given before names held their number: "Audio 3",
/// "MIDI 12", and "3 Audio" as imported sets name them.
fn old_default_name(name: &str) -> bool {
    let Some((first, second)) = name.split_once(' ') else { return false };
    let kind = |word: &str| matches!(word, "Audio" | "MIDI");
    let number = |n: &str| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit());
    (kind(first) && number(second)) || (number(first) && kind(second))
}

impl TrackClipboard {
    /// The names of the tracks a paste adds first, for messages.
    pub(crate) fn first_names(&self) -> Vec<String> {
        self.tops().map(|t| t.track.name.clone()).collect()
    }
}
