// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `preview.play` and `preview.stop` (ADR 0011): the browser's preview of a sample or a MIDI
//! file, through the attached engine. `preview.noteOn` and `preview.noteOff` are the piano
//! roll's key column: a note through the track's instrument until it is let go. They never
//! change the project.

use std::path::Path;

use serde_json::{Value, json};
use suhina_media::info::{FileKind, kind_of};
use suhina_model::TrackId;

use crate::commands::Args;
use crate::commands::track_at;
use crate::files::absolute;
use crate::{CommandError, Session};

/// The preview's level when none is given: −6 dB.
const DEFAULT_GAIN_DB: f64 = -6.0;
/// The velocity a previewed key takes when none is given, MIDI 1 to 127.
const DEFAULT_VELOCITY: f64 = 100.0;

pub(crate) fn play(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    session.player_mut()?;
    let path = Args::new(params).raw_str("path").ok_or("path must be the file to preview")?;
    let path = absolute(Path::new(path));
    let from = match Args::new(params).value("from") {
        None => 0.0,
        Some(v) => v.as_f64().filter(|f| f.is_finite() && *f >= 0.0).ok_or("from must be seconds, at least 0")?,
    };
    let gain_db = gain_db(params)?.unwrap_or(DEFAULT_GAIN_DB);
    let gain = 10f64.powf(gain_db / 20.0) as f32;
    let track: Option<TrackId> = match Args::new(params).value("track") {
        None => None,
        Some(_) => Some(track_at(&session.project, params, "track")?),
    };
    match kind_of(&path) {
        Some(FileKind::Sample | FileKind::Stems) => {
            let data = session.preview_audio(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let seconds = data.frames() as f64 / f64::from(data.sample_rate.max(1));
            session.player_mut()?.controller_mut().preview_sample(data, from, gain).map_err(CommandError)?;
            Ok(json!({ "path": path.display().to_string(), "kind": "sample", "seconds": seconds }))
        }
        Some(FileKind::Midi) => {
            let midi = suhina_media::midi::read_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let bpm = midi.tempo_bpm.unwrap_or(120.0);
            let notes: Vec<(f64, f64, u8, f32)> = midi
                .tracks
                .iter()
                .flat_map(|t| t.notes.iter())
                .map(|n| (n.time, n.duration, n.key, n.velocity))
                .filter(|n| n.0 * 60.0 / bpm >= from)
                .map(|(time, duration, key, velocity)| (time - from * bpm / 60.0, duration, key, velocity))
                .collect();
            let player = session.player_mut()?;
            let rate = player.sample_rate();
            let controller = player.controller_mut();
            let fallback = controller.preview_instrument();
            let prepared = suhina_engine::preview_notes(notes, bpm, rate, track, fallback);
            let seconds = prepared.length as f64 / f64::from(rate.max(1));
            controller.preview_notes(prepared, gain).map_err(CommandError)?;
            Ok(json!({ "path": path.display().to_string(), "kind": "midi", "seconds": seconds, "bpm": bpm }))
        }
        _ => Err(format!("{}: only samples and MIDI files can be previewed", path.display()).into()),
    }
}

/// A level in dB from `params`' `gain`: finite, at most 12; `None` when left out.
fn gain_db(params: &Value) -> Result<Option<f64>, CommandError> {
    match Args::new(params).value("gain") {
        None => Ok(None),
        Some(v) => {
            Ok(Some(v.as_f64().filter(|g| g.is_finite() && *g <= 12.0).ok_or("gain must be decibels, at most 12")?))
        }
    }
}

pub(crate) fn set_gain(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    session.player_mut()?;
    let db = gain_db(params)?.ok_or("gain must be decibels, at most 12")?;
    let gain = 10f64.powf(db / 20.0) as f32;
    session.player_mut()?.controller_mut().preview_gain(gain).map_err(CommandError)?;
    Ok(json!({ "gain": db }))
}

pub(crate) fn stop(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    session.player_mut()?.controller_mut().preview_stop().map_err(CommandError)?;
    Ok(json!({ "stopped": true }))
}

/// `preview.noteOn`: plays `key` through `track`'s instrument until `preview.noteOff`. The note
/// is prepared on this thread and held on the audio thread, so nothing allocates, locks or does
/// I/O there. Transport kind: the project and the undo history are untouched.
pub(crate) fn note_on(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let track = track_at(&session.project, params, "track")?;
    let keys = held_keys(params)?;
    let midi = match Args::new(params).value("velocity") {
        None => DEFAULT_VELOCITY,
        Some(v) => v.as_f64().filter(|v| (1.0..=127.0).contains(v)).ok_or("velocity must be 1 to 127")?,
    };
    let gain = 10f64.powf(DEFAULT_GAIN_DB / 20.0) as f32;
    let bpm = session.project.transport.tempo_bpm;
    let player = session.player_mut()?;
    let rate = player.sample_rate();
    let controller = player.controller_mut();
    let fallback = controller.preview_instrument();
    // Preview notes of no length: the engine holds them until they are let go.
    let velocity = (midi / 127.0) as f32;
    let notes = suhina_engine::preview_notes(
        keys.iter().map(|key| (0.0, 0.0, *key, velocity)),
        bpm,
        rate,
        Some(track),
        fallback,
    );
    controller.preview_note_on(notes, gain).map_err(CommandError)?;
    match keys.as_slice() {
        [key] if Args::new(params).value("keys").is_none_or(Value::is_null) => {
            Ok(json!({ "track": track.0, "key": key, "velocity": midi }))
        }
        _ => Ok(json!({ "track": track.0, "keys": keys, "velocity": midi })),
    }
}

/// The keys `preview.noteOn` plays: `key`, or the list `keys` (1 to 128 keys, each 0 to 127 and
/// counted once), one of them.
fn held_keys(params: &Value) -> Result<Vec<u8>, CommandError> {
    let one = |v: &Value| -> Option<u8> { v.as_u64().and_then(|k| u8::try_from(k).ok()).filter(|k| *k <= 127) };
    let key = Args::new(params).value("key").filter(|v| !v.is_null());
    let keys = Args::new(params).value("keys").filter(|v| !v.is_null());
    match (key, keys) {
        (Some(_), Some(_)) => Err("give key or keys, not both".into()),
        (None, None) => Err("key must be a whole number from 0 to 127 (or give keys, a list of them)".into()),
        (Some(key), None) => Ok(vec![one(key).ok_or("key must be a whole number from 0 to 127")?]),
        (None, Some(keys)) => {
            let list = keys.as_array().ok_or("keys must be a list of keys, each 0 to 127")?;
            if list.is_empty() || list.len() > 128 {
                return Err(format!("keys must hold 1 to 128 keys, got {}", list.len()).into());
            }
            let mut out = list
                .iter()
                .map(|k| one(k).ok_or("each of keys must be a whole number from 0 to 127"))
                .collect::<Result<Vec<u8>, _>>()?;
            out.sort_unstable();
            out.dedup();
            Ok(out)
        }
    }
}

/// `preview.noteOff`: releases the key `key` on `track`, or every held preview note when `key`
/// is left out.
pub(crate) fn note_off(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    track_at(&session.project, params, "track")?;
    let key = match Args::new(params).value("key") {
        None => None,
        Some(v) => Some(
            v.as_u64()
                .and_then(|k| u8::try_from(k).ok())
                .filter(|k| *k <= 127)
                .ok_or("key must be a whole number from 0 to 127")?,
        ),
    };
    session.player_mut()?.controller_mut().preview_note_off(key).map_err(CommandError)?;
    Ok(json!({ "key": key }))
}
