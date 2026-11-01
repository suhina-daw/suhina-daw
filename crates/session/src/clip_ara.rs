// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A clip's ARA plug-in (ADR 0033 › Amendment: the ARA plug-in belongs to the clip, #990): the
//! channel's clip host, its first device, which the device lists do not show, and the clips that
//! play through it (`Clip::ara`).

use serde_json::{Value, json};
use suhina_model::{LaneData, Project, TrackId};

use crate::commands::clip_list;
use crate::{CommandError, Session};

/// Whether `track`'s channel has a clip host: an ARA plug-in its clips play through.
pub(crate) fn has_host(p: &Project, track: TrackId) -> bool {
    p.channel_of_track(track).and_then(|c| c.devices.first()).is_some_and(|d| d.clip_host)
}

/// Puts each channel's clip host back first in its chain, where the engine runs it, after a
/// device command put another device before it (#990).
pub(crate) fn keep_hosts_first(p: &mut Project) {
    for channel in &mut p.channels {
        if let Some(at) = channel.devices.iter().position(|d| d.clip_host).filter(|&at| at > 0) {
            let host = channel.devices.remove(at);
            channel.devices.insert(0, host);
        }
    }
}

/// Whether the clip plays audio on `track`.
fn plays_audio(p: &Project, track: TrackId, content: suhina_model::ContentId) -> bool {
    p.content(content).is_some_and(|c| {
        c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_)) && (l.target.is_none() || l.target == Some(track)))
    })
}

/// Makes the ARA plug-in that is `track`'s first device its clip host, its audio clips playing
/// through it, as a track-wide ARA device did before clips named their own (#990). Does nothing
/// when it is a clip host already. Returns whether it changed anything.
pub(crate) fn make_host(p: &mut Project, track: TrackId) -> bool {
    let Some(first) = p.channel_of_track_mut(track).and_then(|c| c.devices.first_mut()) else { return false };
    if first.clip_host {
        return false;
    }
    first.clip_host = true;
    let audio: Vec<suhina_model::ContentId> = p
        .arrangement
        .lanes
        .iter()
        .filter(|l| l.track == Some(track))
        .flat_map(|l| &l.clips)
        .map(|c| c.content)
        .filter(|&c| plays_audio(p, track, c))
        .collect();
    // Clips that name it already (moved here with their edits) keep it alone; otherwise every
    // audio clip of the track, as the plug-in edited the whole track before.
    let any = p.arrangement.lanes.iter().filter(|l| l.track == Some(track)).flat_map(|l| &l.clips).any(|c| c.ara);
    if !any {
        for clip in p.arrangement.lanes.iter_mut().filter(|l| l.track == Some(track)).flat_map(|l| l.clips.iter_mut()) {
            if audio.contains(&clip.content) {
                clip.ara = true;
            }
        }
    }
    true
}

/// `clip.setAra {clips, on, plugin?}`: the clips play through their track's ARA plug-in, or no
/// longer do. A track with none gets `plugin` (or the installed one, Melodyne first) as its clip
/// host, first in its chain and not shown in its device list; a track whose ARA plug-in edited
/// all its clips before makes it its clip host. Only audio clips on the arrangement take one.
pub(crate) fn set_ara(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let on = params.get("on").and_then(Value::as_bool).ok_or("on must be true or false")?;
    let clips = clip_list(&session.project, params)?;
    let mut tracks: Vec<TrackId> = clips.iter().map(|(t, _)| *t).collect();
    tracks.dedup();
    for &(track, index) in &clips {
        let content = session
            .project
            .arrangement
            .lanes
            .iter()
            .find(|l| l.track == Some(track))
            .and_then(|l| l.clips.get(index))
            .map(|c| c.content)
            .ok_or("the clip is missing")?;
        if !plays_audio(&session.project, track, content) {
            return Err("only an audio clip plays through an ARA plug-in".into());
        }
    }
    if on {
        for &track in &tracks {
            if has_host(&session.project, track) {
                continue;
            }
            // A track-wide ARA plug-in from before becomes the clip host.
            if session.ara_device(track) == Some(0) {
                make_host(&mut session.project, track);
                continue;
            }
            let plugin = match params.get("plugin").and_then(Value::as_str) {
                Some(plugin) => plugin.to_string(),
                None => session
                    .ara_plugin()
                    .map(|(_, spec)| spec)
                    .ok_or("no plug-in that edits clips (such as Melodyne) is installed; plugin.scan looks for it")?,
            };
            let added = crate::devices::add_plugin(
                session,
                &json!({ "track": track.0, "plugin": plugin, "position": 0, "clip_host": true }),
            )?;
            if added["ara"] != json!(true) {
                return Err(format!("{plugin} does not edit clips over ARA").into());
            }
        }
    }
    for &(track, index) in &clips {
        if let Some(clip) = session
            .project
            .arrangement
            .lanes
            .iter_mut()
            .find(|l| l.track == Some(track))
            .and_then(|l| l.clips.get_mut(index))
        {
            clip.ara = on;
        }
    }
    Ok(json!({ "clips": clips.len(), "on": on }))
}
