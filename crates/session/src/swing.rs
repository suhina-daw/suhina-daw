// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `transport.setSwing` and `clip.setSwing` (`docs/spec/swing.md`, #1221): the project's swing, and
//! a MIDI clip's own swing or its following the project's. Amounts are percent in the commands and
//! 0 to 1 in the model.

use serde_json::{Value, json};
use suhina_model::{LaneData, SWING_RANGE};

use crate::commands::Args;
use crate::notes::{clip_mut, locate};
use crate::{CommandError, Session};

/// An amount in percent, 0 to 100, as a fraction of 1.
fn percent(params: &Value) -> Result<Option<f64>, CommandError> {
    let Some(amount) = Args::new(params).opt_f64("amount")? else { return Ok(None) };
    if !(0.0..=100.0).contains(&amount) {
        return Err(format!("amount must be 0 to 100 percent, got {amount}").into());
    }
    Ok(Some((amount / 100.0).clamp(*SWING_RANGE.start(), *SWING_RANGE.end())))
}

/// A fraction of 1 as the percent a result says, rid of the float's last-digit noise
/// (0.55 × 100 is 55.000000000000014).
fn as_percent(fraction: f64) -> f64 {
    (fraction * 100.0 * 1e6).round() / 1e6
}

/// `transport.setSwing`: the project's swing, which every MIDI clip without its own plays with.
pub(crate) fn set_project_swing(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let amount = percent(params)?.ok_or("give amount, 0 to 100 percent")?;
    session.project.transport.swing = amount;
    Ok(json!({ "amount": as_percent(amount) }))
}

/// `clip.setSwing`: a MIDI clip's own swing (`amount`, which makes the clip play it), or, with
/// `global: true`, the clip following the project's swing again; it keeps its own amount for when
/// it plays its own again (`global: false`).
pub(crate) fn set_clip_swing(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let amount = percent(params)?;
    let global = match Args::new(params).value("global") {
        None => None,
        Some(v) => Some(v.as_bool().ok_or("global must be true or false")?),
    };
    if amount.is_none() && global.is_none() {
        return Err("give amount (0 to 100 percent), global (true or false), or both".into());
    }
    if amount.is_some() && global == Some(true) {
        return Err("a clip that follows the project's swing has no amount of its own to set; leave out global".into());
    }
    let (at, content, _) = locate(&session.project, params)?;
    let notes =
        session.project.content(content).is_some_and(|c| c.lanes.iter().any(|l| matches!(l.data, LaneData::Notes(_))));
    if !notes {
        return Err("only a MIDI clip swings".into());
    }
    let global_swing = session.project.transport.swing;
    let clip = clip_mut(&mut session.project, at).ok_or("the clip is missing")?;
    if let Some(amount) = amount {
        clip.swing.amount = amount;
        clip.swing.own = true;
    }
    if let Some(global) = global {
        clip.swing.own = !global;
    }
    Ok(json!({
        "amount": as_percent(clip.swing.amount),
        "global": !clip.swing.own,
        "plays": as_percent(clip.swing.effective(global_swing)),
    }))
}
