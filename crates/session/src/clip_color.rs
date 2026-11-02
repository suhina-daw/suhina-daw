// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Clips' colours and names as a track's: a clip with no colour of its own shows its track's
//! (a new clip takes it that way); `clip.setColor` gives clips one, or with `"none"` hands them
//! back to the track's (the track menu's *Assign track color to all clips*); `track.renameClips`
//! names every clip of a track after the track.

use serde_json::{Value, json};
use suhina_model::Rgb;

use crate::commands::Args;
use crate::commands::track_id;
use crate::{CommandError, Session};

/// `"#rrggbb"`, or `None` for `"none"`.
pub(crate) fn parse_color(params: &Value, key: &str) -> Result<Option<Rgb>, CommandError> {
    let bad = || CommandError(format!("{key} must be \"#rrggbb\" or \"none\""));
    let text = Args::new(params).raw_str(key).ok_or_else(bad)?;
    if text.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    let hex = text.strip_prefix('#').filter(|h| h.len() == 6).ok_or_else(bad)?;
    let part = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    match (part(0), part(2), part(4)) {
        (Some(r), Some(g), Some(b)) => Ok(Some(Rgb(r, g, b))),
        _ => Err(bad()),
    }
}

/// `clip.setColor {track, color, index?}`: one arrangement clip's colour (by index), or every
/// clip of the track, its arrangement clips and its slots' clips, when no index is given.
/// `"none"` makes them show the track's colour again.
pub(crate) fn set_clip_color(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let color = parse_color(params, "color")?;
    let track = track_id(&session.project, params)?;
    let index = match Args::new(params).value("index") {
        None => None,
        Some(v) => Some(
            v.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or("index must be a clip index from project.clips")?,
        ),
    };
    let p = &mut session.project;
    let mut changed = 0usize;
    let lane = p.arrangement.lanes.iter_mut().find(|l| l.track == Some(track));
    match index {
        Some(index) => {
            let clip =
                lane.and_then(|l| l.clips.get_mut(index)).ok_or_else(|| format!("the track has no clip {index}"))?;
            clip.color = color;
            changed = 1;
        }
        None => {
            if let Some(lane) = lane {
                for clip in &mut lane.clips {
                    clip.color = color;
                    changed += 1;
                }
            }
            for column in p.launcher.columns.iter_mut().filter(|c| c.track == track) {
                for clip in column.slots.iter_mut().filter_map(|s| s.clip.as_mut()) {
                    clip.color = color;
                    changed += 1;
                }
            }
        }
    }
    Ok(json!({ "clips": changed, "color": Args::new(params).value("color") }))
}

/// `track.renameClips {track}`: every clip of a track, in the arrangement and in its slots, takes
/// the track's name.
pub(crate) fn rename_clips(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_id(&session.project, params)?;
    let p = &mut session.project;
    let name = p.track(track).map(|_| p.shown_track_name(track)).ok_or("the track is missing")?;
    let mut contents: Vec<suhina_model::ContentId> = Vec::new();
    if let Some(lane) = p.arrangement.lanes.iter().find(|l| l.track == Some(track)) {
        contents.extend(lane.clips.iter().map(|c| c.content));
    }
    for column in p.launcher.columns.iter().filter(|c| c.track == track) {
        contents.extend(column.slots.iter().filter_map(|s| s.clip.as_ref()).map(|c| c.content));
    }
    let mut renamed = 0usize;
    for content in p.contents.iter_mut().filter(|c| contents.contains(&c.id)) {
        content.name.clone_from(&name);
        renamed += 1;
    }
    Ok(json!({ "name": name, "renamed": renamed }))
}
