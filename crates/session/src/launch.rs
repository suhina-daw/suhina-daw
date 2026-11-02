// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Commands for a slot clip's launch settings (mode, legato, its own quantisation) and for the
//! release of a slot's launch button (`docs/spec/clip-launching.md` › Launch settings).

use serde_json::{Value, json};
use suhina_engine::LaunchCommand;
use suhina_model::{Clip, LaunchMode, LaunchQuantization, Project};

use crate::commands::Args;
use crate::commands::{column_index, scene_index};
use crate::{CommandError, Session};

/// The clip in the slot `params.track`, `params.scene`.
fn slot_clip<'a>(p: &'a mut Project, params: &Value) -> Result<&'a mut Clip, CommandError> {
    let column = column_index(p, params)?;
    let scene = scene_index(p, params)?;
    p.launcher
        .columns
        .get_mut(column)
        .and_then(|c| c.slots.get_mut(scene))
        .and_then(|s| s.clip.as_mut())
        .ok_or_else(|| "that slot has no clip".into())
}

fn text<'a>(params: &'a Value, name: &str, what: &str) -> Result<&'a str, CommandError> {
    Args::new(params).raw_str(name).ok_or_else(|| format!("{name} must be {what}").into())
}

pub(crate) fn set_launch_mode(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let names: Vec<&str> = LaunchMode::ALL.iter().map(|m| m.name()).collect();
    let what = format!("one of {}", names.join(", "));
    let name = text(params, "mode", &what)?;
    let mode = LaunchMode::from_name(name).ok_or_else(|| format!("mode must be {what}, got {name:?}"))?;
    slot_clip(&mut session.project, params)?.launch.mode = mode;
    Ok(json!({ "mode": mode.name() }))
}

pub(crate) fn set_legato(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let on = Args::new(params).bool("on")?;
    slot_clip(&mut session.project, params)?.launch.legato = on;
    Ok(json!({ "legato": on }))
}

pub(crate) fn set_quantization(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let names: Vec<&str> = LaunchQuantization::ALL.iter().map(|q| q.name()).collect();
    let what = format!("one of {}", names.join(", "));
    let name = text(params, "value", &what)?;
    let quantization =
        LaunchQuantization::from_name(name).ok_or_else(|| format!("value must be {what}, got {name:?}"))?;
    slot_clip(&mut session.project, params)?.launch.quantization = quantization;
    Ok(json!({ "quantization": quantization.name() }))
}

/// The release of a slot's launch button: `clip.launch` is the press.
pub(crate) fn release_clip(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let column = column_index(session.project(), params)?;
    let scene = scene_index(session.project(), params)?;
    session.player_mut()?.send(LaunchCommand::ReleaseClip { column, slot: scene })?;
    Ok(json!({ "scene": scene + 1, "released": true }))
}
