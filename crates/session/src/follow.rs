// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `clip.setFollowAction`: a slot clip's follow action (`docs/spec/clip-launching.md` › Follow
//! actions).

use serde_json::{Value, json};
use suhina_model::{FollowAction, FollowKind};

use crate::commands::{Args, column_index, scene_index};
use crate::{CommandError, Session};

/// The parameter list of `clip.setFollowAction`, for its errors.
const SETTINGS: &str = "enabled, time, linked, actionA, actionB, chanceA or chanceB";

pub(crate) fn set_follow_action(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = session.project();
    let column = column_index(p, params)?;
    let scene = scene_index(p, params)?;
    let scenes = p.launcher.scenes.len();
    let clip = p
        .launcher
        .columns
        .get(column)
        .and_then(|c| c.slots.get(scene))
        .and_then(|s| s.clip.as_ref())
        .ok_or("that slot has no clip")?;
    let old = clip.follow_action;
    let follow = FollowAction {
        enabled: Args::new(params).opt_bool("enabled")?.unwrap_or(old.enabled),
        time: time(params)?.unwrap_or(old.time),
        linked: Args::new(params).opt_bool("linked")?.unwrap_or(old.linked),
        action_a: action(params, "actionA", scenes)?.unwrap_or(old.action_a),
        action_b: action(params, "actionB", scenes)?.unwrap_or(old.action_b),
        chance_a: chance(params, "chanceA")?.unwrap_or(old.chance_a),
        chance_b: chance(params, "chanceB")?.unwrap_or(old.chance_b),
    };
    let given = ["enabled", "time", "linked", "actionA", "actionB", "chanceA", "chanceB"];
    if !given.iter().any(|name| Args::new(params).has(name)) {
        return Err(format!("give {SETTINGS}").into());
    }
    let slot = session.project.launcher.columns.get_mut(column).and_then(|c| c.slots.get_mut(scene));
    if let Some(clip) = slot.and_then(|s| s.clip.as_mut()) {
        clip.follow_action = follow;
    }
    Ok(json!({ "scene": scene + 1, "follow_action": describe(&follow) }))
}

/// A follow action as commands report it.
pub(crate) fn describe(f: &FollowAction) -> Value {
    json!({
        "enabled": f.enabled,
        "time": f.time,
        "linked": f.linked,
        "action_a": f.action_a.name(),
        "action_b": f.action_b.name(),
        "chance_a": f.chance_a,
        "chance_b": f.chance_b,
    })
}

fn time(params: &Value) -> Result<Option<f64>, CommandError> {
    match Args::new(params).value("time") {
        None => Ok(None),
        Some(_) => {
            let beats = Args::new(params).f64("time")?;
            if !(beats > 0.0 && beats <= FollowAction::MAX_TIME) {
                return Err(format!(
                    "time must be more than 0 and at most {} beats, got {beats}",
                    FollowAction::MAX_TIME
                )
                .into());
            }
            Ok(Some(beats))
        }
    }
}

fn chance(params: &Value, name: &str) -> Result<Option<u32>, CommandError> {
    match Args::new(params).value(name) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v <= FollowAction::MAX_CHANCE)
            .map(Some)
            .ok_or_else(|| format!("{name} must be a whole number from 0 to {}", FollowAction::MAX_CHANCE).into()),
    }
}

fn action(params: &Value, name: &str, scenes: usize) -> Result<Option<FollowKind>, CommandError> {
    let text = match Args::new(params).value(name) {
        None => return Ok(None),
        Some(Value::String(text)) => text,
        Some(_) => return Err(format!("{name} must be an action name such as \"next\"").into()),
    };
    let kind = FollowKind::from_name(text).ok_or_else(|| {
        let names: Vec<String> = FollowKind::NAMED.iter().map(|k| k.name()).collect();
        format!("{name} must be {} or \"jump N\" (a scene number), got {text:?}", names.join(", "))
    })?;
    if let FollowKind::Jump(slot) = kind
        && usize::try_from(slot).map_or(true, |slot| slot >= scenes)
    {
        return Err(
            format!("{name}: scene {} does not exist; the project has {scenes} scenes", u64::from(slot) + 1).into()
        );
    }
    Ok(Some(kind))
}
