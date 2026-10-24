// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sends and return tracks: the send of a track to a return (level, before or after the fader)
//! and adding, removing and renaming returns. The engine plays them (`suhina_engine`, sends).

use serde_json::{Value, json};
use suhina_model::{Channel, ChannelId, ChannelRole, ContentKinds, Project, Send, Track, TrackId};

use crate::commands::{channel_param, remove_track, track_at};
use crate::files::MAX_TRACKS;
use crate::{CommandError, Session};

/// The loudest a send can be, in dB (as a track's volume).
const MAX_SEND_DB: f64 = 6.0;

/// The return track in `params[key]` and its channel; an error for any other track.
fn return_at(p: &Project, params: &Value, key: &str) -> Result<(TrackId, ChannelId), CommandError> {
    let track = track_at(p, params, key)?;
    let channel = p
        .tracks
        .iter()
        .find(|t| t.id == track)
        .and_then(|t| t.channel)
        .and_then(|id| p.channel(id))
        .filter(|c| c.role == ChannelRole::Effect)
        .ok_or_else(|| format!("{key} must be a return track; project.tracks lists them"))?;
    Ok((track, channel.id))
}

/// Whether the channel `from` plays, through outputs and sends, into `to`.
fn plays_into(p: &Project, from: ChannelId, to: ChannelId) -> bool {
    let mut seen = vec![from];
    let mut at = 0;
    while let Some(&id) = seen.get(at) {
        if id == to {
            return true;
        }
        if let Some(c) = p.channel(id) {
            for next in c.output.into_iter().chain(c.sends.iter().map(|s| s.target)) {
                if !seen.contains(&next) {
                    seen.push(next);
                }
            }
        }
        at += 1;
    }
    false
}

/// The send of the track in `params` to the return in `params.return`, made (silent, after the
/// fader) when there is none. Refused when it would feed the track back into itself.
fn send_param<'a>(session: &'a mut Session, params: &Value) -> Result<(&'a mut Send, String), CommandError> {
    let p = &session.project;
    let (ret, target) = return_at(p, params, "return")?;
    let from = channel_param(session, params)?.id;
    let p = &session.project;
    let name = p.track(ret).map(|t| t.name.clone()).unwrap_or_default();
    if from == target {
        return Err("a return cannot send to itself".into());
    }
    let exists = p.channel(from).is_some_and(|c| c.sends.iter().any(|s| s.target == target));
    if !exists && plays_into(p, target, from) {
        return Err(format!("a send to {name:?} would make a loop: it plays back into this track").into());
    }
    let channel = channel_param(session, params)?;
    if !channel.sends.iter().any(|s| s.target == target) {
        channel.sends.push(Send { target, level: 0.0, pre_fader: false });
    }
    let send = channel.sends.iter_mut().find(|s| s.target == target).ok_or("the send is missing")?;
    Ok((send, name))
}

pub(crate) fn set_send(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let db = match params.get("level") {
        Some(Value::String(s)) if s == "-inf" => f64::NEG_INFINITY,
        Some(v) => v.as_f64().filter(|n| n.is_finite()).ok_or("level must be a number in dB or \"-inf\"")?,
        None => return Err("level must be a number in dB or \"-inf\"".into()),
    };
    if db > MAX_SEND_DB {
        return Err(format!("level must be at most +{MAX_SEND_DB} dB, got {db}").into());
    }
    let gain = if db < -150.0 { 0.0 } else { 10f64.powf(db / 20.0) };
    let (ret, _) = return_at(&session.project, params, "return")?;
    let from = channel_param(session, params)?.id;
    let track = session.project.tracks.iter().find(|t| t.channel == Some(from)).map(|t| t.id);
    let heard_before = track.is_some_and(|t| suhina_engine::send_heard(&session.project, t, ret));
    let (send, name) = send_param(session, params)?;
    let was = send.level;
    send.level = gain;
    // A send that plays neither before nor after (a muted track's, one at silence): nothing the
    // engine plays changes, so no new graph (each step of a muted track's knob built one).
    if !heard_before && !track.is_some_and(|t| suhina_engine::send_heard(&session.project, t, ret)) {
        session.sync.unheard = true;
    }
    // A turn of a send that plays: a playing engine takes it as a message, smoothed, instead of
    // a new graph (one a step held the window up 60 to 80 ms in a large project). One that
    // starts from silence is not in the graph yet and takes a new one.
    if was > 0.0
        && let Some(track) = track
    {
        session.sync.mixer_edits =
            vec![crate::ParamEdit { track, device: suhina_engine::MIXER_SEND, param: ret.0 as usize, value: gain }];
    }
    let level = (gain > 0.0).then(|| (20.0 * gain.log10() * 100.0).round() / 100.0);
    Ok(json!({ "return": name, "level_db": level }))
}

pub(crate) fn set_send_mode(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let pre_fader = match params.get("mode").and_then(Value::as_str) {
        Some(mode) if mode.eq_ignore_ascii_case("pre") => true,
        Some(mode) if mode.eq_ignore_ascii_case("post") => false,
        _ => return Err("mode must be \"pre\" (before the track's volume and pan) or \"post\"".into()),
    };
    let (send, name) = send_param(session, params)?;
    send.pre_fader = pre_fader;
    Ok(json!({ "return": name, "mode": if pre_fader { "pre" } else { "post" } }))
}

/// The name of the next return: "A Return", "B Return" and so on, then "Return n".
fn next_name(p: &Project) -> String {
    let taken = |name: &str| p.tracks.iter().any(|t| t.name.eq_ignore_ascii_case(name));
    ('A'..='Z')
        .map(|letter| format!("{letter} Return"))
        .chain((27..).map(|n| format!("Return {n}")))
        .take(MAX_TRACKS + 32)
        .find(|name| !taken(name))
        .unwrap_or_else(|| "Return".to_string())
}

pub(crate) fn add_return(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    if p.tracks.len() >= MAX_TRACKS {
        return Err(format!("a project holds at most {MAX_TRACKS} tracks").into());
    }
    let name = match params.get("name") {
        None | Some(Value::Null) => next_name(p),
        Some(v) => v.as_str().map(str::trim).filter(|n| !n.is_empty()).ok_or("name must be a text, not empty")?.into(),
    };
    let id = TrackId(p.tracks.iter().map(|t| t.id.0).max().unwrap_or(0).checked_add(1).ok_or("too many tracks")?);
    let channel =
        ChannelId(p.channels.iter().map(|c| c.id.0).max().unwrap_or(0).checked_add(1).ok_or("too many channels")?);
    let main = p.channels.iter().find(|c| c.role == ChannelRole::Master).map(|c| c.id);
    p.channels.push(Channel {
        id: channel,
        name: name.clone(),
        role: ChannelRole::Effect,
        volume: 1.0,
        pan: 0.0,
        mute: false,
        solo: false,
        cue: false,
        output: main,
        sends: Vec::new(),
        devices: Vec::new(),
        automation: Vec::new(),
        extensions: Default::default(),
    });
    // Returns come after every other track, in the order they were added.
    p.tracks.push(Track {
        id,
        name: name.clone(),
        color: None,
        content: ContentKinds::default(),
        channel: Some(channel),
        parent: None,
        extensions: Default::default(),
        audio_input: Default::default(),
        midi_input: Default::default(),
    });
    Ok(json!({ "track": id.0, "name": name }))
}

pub(crate) fn remove_return(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (track, channel) = return_at(&session.project, params, "return")?;
    // Every send to it goes with it.
    remove_track(&mut session.project, track, channel);
    Ok(json!({ "removed": track.0 }))
}

pub(crate) fn rename_return(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let name = params.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty());
    let name = name.ok_or("name must be a text, not empty")?.to_string();
    let (track, channel) = return_at(&session.project, params, "return")?;
    let p = &mut session.project;
    if let Some(t) = p.track_mut(track) {
        t.name.clone_from(&name);
    }
    if let Some(c) = p.channel_mut(channel) {
        c.name.clone_from(&name);
    }
    Ok(json!({ "name": name }))
}

/// A channel's sends as `project.tracks` lists them.
pub(crate) fn sends_json(p: &Project, channel: &Channel) -> Vec<Value> {
    channel
        .sends
        .iter()
        .map(|s| {
            let target = p.tracks.iter().find(|t| t.channel == Some(s.target));
            let level =
                (s.level.is_finite() && s.level > 0.0).then(|| (20.0 * s.level.log10() * 100.0).round() / 100.0);
            json!({
                "return": target.map(|t| t.id.0),
                "name": target.map(|t| t.name.clone()),
                "level_db": level,
                "mode": if s.pre_fader { "pre" } else { "post" },
            })
        })
        .collect()
}
