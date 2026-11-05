// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tempo map's commands (`docs/spec/tempo-map.md` › Commands): tempo changes and
//! time-signature changes over the song, each edit one undo step. Tempo points are numbered from
//! 1 in beat order; bars from 1, as shown.

use serde_json::{Value, json};
use suhina_model::{MAX_CHANGE_BEAT, TempoPoint, TimeSignature, TimeSignatureChange, Transport};

use crate::commands::Args;
use crate::{CommandError, Session};

/// Tempos the commands take, as `transport.setTempo` does.
const BPM: std::ops::RangeInclusive<f64> = 20.0..=999.0;

/// The start tempo and every change, numbered.
fn tempo_list(t: &Transport) -> Value {
    let points: Vec<Value> = t
        .tempo_changes
        .iter()
        .enumerate()
        .map(|(i, p)| json!({ "point": i + 1, "beat": p.beat, "bpm": p.bpm, "ramp": p.ramp }))
        .collect();
    json!({ "start_bpm": t.tempo_bpm, "points": points })
}

/// The start signature and every change, with bars from 1.
fn signature_list(t: &Transport) -> Value {
    let name = |ts: TimeSignature| format!("{}/{}", ts.numerator, ts.denominator);
    let changes: Vec<Value> = t
        .time_signature_changes
        .iter()
        .map(|c| json!({ "bar": u64::from(c.bar) + 1, "time_signature": name(c.time_signature) }))
        .collect();
    json!({ "start": name(t.time_signature), "changes": changes })
}

pub(crate) fn points(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let t = &session.project().transport;
    Ok(json!({ "tempo": tempo_list(t), "time_signatures": signature_list(t) }))
}

fn beat_param(params: &Value) -> Result<f64, CommandError> {
    let beat = Args::new(params).f64("beat")?;
    if !(beat > 0.0 && beat <= MAX_CHANGE_BEAT) {
        return Err(format!("beat must be after 0 and at most {MAX_CHANGE_BEAT}, got {beat}").into());
    }
    Ok(beat)
}

fn bpm_param(params: &Value) -> Result<f64, CommandError> {
    let bpm = Args::new(params).f64("bpm")?;
    if !BPM.contains(&bpm) {
        return Err(format!("bpm must be between 20 and 999, got {bpm}").into());
    }
    Ok(bpm)
}

/// Puts `point` among the changes in beat order. A beat holds at most a ramp and a step, the
/// ramp first: the tempo glides to the ramp's tempo and then jumps to the step's. A point of the
/// same kind already on its beat is replaced.
fn insert_point(changes: &mut Vec<TempoPoint>, point: TempoPoint) -> Result<(), CommandError> {
    changes.retain(|p| p.beat != point.beat || p.ramp != point.ramp);
    if changes.len() >= suhina_model::MAX_TEMPO_CHANGES {
        return Err("the project has as many tempo changes as it can hold".into());
    }
    let at = changes.partition_point(|p| p.beat < point.beat || (p.beat == point.beat && p.ramp && !point.ramp));
    changes.insert(at, point);
    Ok(())
}

pub(crate) fn add_point(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let point = TempoPoint {
        beat: beat_param(params)?,
        bpm: bpm_param(params)?,
        ramp: Args::new(params).opt_bool("ramp")?.unwrap_or(false),
    };
    let transport = &mut session.project.transport;
    insert_point(&mut transport.tempo_changes, point)?;
    Ok(tempo_list(transport))
}

/// The index of `params.point` (from 1) among the changes.
fn point_index(t: &Transport, params: &Value) -> Result<usize, CommandError> {
    let count = t.tempo_changes.len();
    let number =
        Args::new(params).value("point").and_then(Value::as_u64).ok_or("point must be a whole number from 1")?;
    usize::try_from(number)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .filter(|i| *i < count)
        .ok_or_else(|| format!("tempo point {number} does not exist; the project has {count}").into())
}

pub(crate) fn move_point(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let index = point_index(&session.project.transport, params)?;
    let beat = if Args::new(params).has("beat") { Some(beat_param(params)?) } else { None };
    let bpm = if Args::new(params).has("bpm") { Some(bpm_param(params)?) } else { None };
    let ramp = Args::new(params).opt_bool("ramp")?;
    if beat.is_none() && bpm.is_none() && ramp.is_none() {
        return Err("give at least one of beat, bpm and ramp".into());
    }
    let changes = &mut session.project.transport.tempo_changes;
    let Some(old) = changes.get(index).copied() else { return Err("the tempo point is missing".into()) };
    changes.remove(index);
    let point =
        TempoPoint { beat: beat.unwrap_or(old.beat), bpm: bpm.unwrap_or(old.bpm), ramp: ramp.unwrap_or(old.ramp) };
    insert_point(changes, point)?;
    Ok(tempo_list(&session.project.transport))
}

pub(crate) fn delete_point(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let index = point_index(&session.project.transport, params)?;
    session.project.transport.tempo_changes.remove(index);
    Ok(tempo_list(&session.project.transport))
}

/// `params.<name>` as a bar shown from 1, after the first bar; returned counted from 0.
fn bar_param(params: &Value, name: &str) -> Result<u32, CommandError> {
    let bar = Args::new(params)
        .value(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{name} must be a whole number"))?;
    u32::try_from(bar)
        .ok()
        .filter(|b| *b >= 2)
        .map(|b| b - 1)
        .ok_or_else(|| format!("{name} must be a bar from 2 (bar 1 has the start signature), got {bar}").into())
}

fn signature_part(params: &Value, name: &str) -> Result<Option<u8>, CommandError> {
    match Args::new(params).value(name) {
        None => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| format!("{name} must be a whole number").into()),
    }
}

fn valid(ts: TimeSignature) -> Result<TimeSignature, CommandError> {
    if ts.is_valid() {
        Ok(ts)
    } else {
        Err(format!(
            "{}/{} is not a time signature: the numerator is 1 to 99, the denominator 1, 2, 4, 8 or 16",
            ts.numerator, ts.denominator
        )
        .into())
    }
}

/// Puts `change` among the changes in bar order; one already on its bar is replaced.
fn insert_change(changes: &mut Vec<TimeSignatureChange>, change: TimeSignatureChange) -> Result<(), CommandError> {
    changes.retain(|c| c.bar != change.bar);
    if changes.len() >= suhina_model::MAX_TIME_SIGNATURE_CHANGES {
        return Err("the project has as many time-signature changes as it can hold".into());
    }
    let at = changes.partition_point(|c| c.bar < change.bar);
    changes.insert(at, change);
    Ok(())
}

pub(crate) fn add_signature(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let bar = bar_param(params, "bar")?;
    let numerator = signature_part(params, "numerator")?.ok_or("numerator must be a whole number")?;
    let denominator = signature_part(params, "denominator")?.ok_or("denominator must be a whole number")?;
    let time_signature = valid(TimeSignature { numerator, denominator })?;
    let transport = &mut session.project.transport;
    insert_change(&mut transport.time_signature_changes, TimeSignatureChange { bar, time_signature })?;
    Ok(signature_list(transport))
}

/// The index of the change at `params.bar`.
fn change_index(t: &Transport, params: &Value) -> Result<usize, CommandError> {
    let bar = bar_param(params, "bar")?;
    t.time_signature_changes
        .iter()
        .position(|c| c.bar == bar)
        .ok_or_else(|| format!("bar {} has no time-signature change", u64::from(bar) + 1).into())
}

pub(crate) fn move_signature(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let index = change_index(&session.project.transport, params)?;
    let to = if Args::new(params).has("to") { Some(bar_param(params, "to")?) } else { None };
    let numerator = signature_part(params, "numerator")?;
    let denominator = signature_part(params, "denominator")?;
    if to.is_none() && numerator.is_none() && denominator.is_none() {
        return Err("give at least one of to, numerator and denominator".into());
    }
    let changes = &mut session.project.transport.time_signature_changes;
    let Some(old) = changes.get(index).copied() else { return Err("the time-signature change is missing".into()) };
    let time_signature = valid(TimeSignature {
        numerator: numerator.unwrap_or(old.time_signature.numerator),
        denominator: denominator.unwrap_or(old.time_signature.denominator),
    })?;
    changes.remove(index);
    insert_change(changes, TimeSignatureChange { bar: to.unwrap_or(old.bar), time_signature })?;
    Ok(signature_list(&session.project.transport))
}

pub(crate) fn delete_signature(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let index = change_index(&session.project.transport, params)?;
    session.project.transport.time_signature_changes.remove(index);
    Ok(signature_list(&session.project.transport))
}
