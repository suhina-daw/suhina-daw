// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An empty track takes the kind of what is dropped on it (#945): a MIDI track that holds no
//! clip becomes an audio track when audio lands on it, and an empty audio track a MIDI track for
//! notes. A track that holds any clip, on its lane, its take lanes or its slots, keeps its kind.

use serde_json::{Value, json};
use suhina_model::{ContentId, LaneData, Project, TrackId};

use crate::commands::track_id;
use crate::{CommandError, Session};

/// Whether `track` holds no clip: none on its arrangement lane, its take lanes or its slots.
pub(crate) fn is_empty(p: &Project, track: TrackId) -> bool {
    let lane = p.arrangement.lanes.iter().filter(|l| l.track == Some(track)).all(|l| l.clips.is_empty());
    let takes = p.arrangement.takes.iter().filter(|t| t.track == track).all(|t| t.clips.is_empty());
    let slots =
        p.launcher.columns.iter().filter(|c| c.track == track).all(|c| c.slots.iter().all(|s| s.clip.is_none()));
    lane && takes && slots
}

/// Makes the empty `track` hold notes (`notes`) or audio when it holds the other kind, for a clip
/// of that kind dropped on it. Returns whether it changed; a track that holds clips, already
/// takes the kind, or is a return or a group, is left alone.
pub(crate) fn fit_empty(session: &mut Session, track: TrackId, notes: bool, audio: bool) -> Result<bool, CommandError> {
    if notes == audio {
        return Ok(false);
    }
    let p = &session.project;
    let Some(t) = p.track(track) else { return Ok(false) };
    let takes = if notes { t.content.notes } else { t.content.audio };
    let plays = t.content.notes || t.content.audio;
    if takes || !plays || !is_empty(p, track) {
        return Ok(false);
    }
    set_kind_of(session, track, notes)?;
    Ok(true)
}

/// [`fit_empty`] for a clip playing `content` dropped on `track`.
pub(crate) fn fit_empty_for(session: &mut Session, track: TrackId, content: ContentId) -> Result<bool, CommandError> {
    let (notes, audio) = session.project.content(content).map_or((false, false), |c| {
        (
            c.lanes.iter().any(|l| matches!(l.data, LaneData::Notes(_))),
            c.lanes.iter().any(|l| matches!(l.data, LaneData::Audio(_))),
        )
    });
    fit_empty(session, track, notes, audio)
}

/// Turns `track` into a MIDI track (`notes`) or an audio track. A MIDI track that becomes an
/// audio track loses its instrument and MIDI effects, which play nothing on audio; a name that is
/// the new tracks' one follows the kind ("# MIDI" to "# Audio").
fn set_kind_of(session: &mut Session, track: TrackId, notes: bool) -> Result<(), CommandError> {
    use crate::device_groups::{Kind, kind_of};
    // The names the track's instruments gave it ("# FM Synth"), which follow the kind too.
    let mut instruments: Vec<String> = Vec::new();
    if !notes && let Some(channel) = session.project.channel_of_track(track) {
        instruments = channel
            .devices
            .iter()
            .filter(|d| kind_of(session, d) == Kind::Instrument)
            .map(|d| suhina_model::names::new_track_name(&d.name))
            .collect();
        let keep: Vec<bool> = channel.devices.iter().map(|d| kind_of(session, d) == Kind::Effect).collect();
        if let Some(channel) = session.project.channel_of_track_mut(track) {
            let mut keep = keep.into_iter();
            channel.devices.retain(|_| keep.next().unwrap_or(true));
        }
    }
    let (from, to) = if notes { ("Audio", "MIDI") } else { ("MIDI", "Audio") };
    let t = session.project.tracks.iter_mut().find(|t| t.id == track).ok_or("the track is missing")?;
    t.content.notes = notes;
    t.content.audio = !notes;
    if t.name == suhina_model::names::new_track_name(from) || instruments.contains(&t.name) {
        t.name = suhina_model::names::new_track_name(to);
    }
    Ok(())
}

/// `track.setKind {track, kind}`: an empty track becomes a MIDI or an audio track (#945).
pub(crate) fn set_kind(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_id(&session.project, params)?;
    let notes = match params.get("kind").and_then(Value::as_str) {
        Some("midi") => true,
        Some("audio") => false,
        _ => return Err("kind must be midi or audio".into()),
    };
    let t = session.project.track(track).ok_or("the track is missing")?;
    if !(t.content.notes || t.content.audio) {
        return Err("only an audio or MIDI track changes its kind".into());
    }
    if !is_empty(&session.project, track) {
        return Err("the track holds clips: only an empty track changes its kind".into());
    }
    set_kind_of(session, track, notes)?;
    Ok(json!({ "track": track.0, "kind": if notes { "midi" } else { "audio" } }))
}
