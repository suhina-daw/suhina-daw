// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Launch mappings: commands that add, list and remove the keys and MIDI notes that launch slots,
//! scenes and stop buttons, and [`mapped_command`], which turns a press or release of a mapped
//! key or note into the command the mouse would run (`docs/spec/clip-launching.md` › Launching
//! from keys and notes).

use serde_json::{Value, json};
use suhina_model::{LaunchMapping, MappingSource, MappingTarget, Project};

use crate::commands::Args;
use crate::commands::{column_index, scene_index, track_id};
use crate::{CommandError, Session};

/// The most mappings a project keeps, so a script cannot grow the project without bound.
pub const MAX_MAPPINGS: usize = 1024;

fn source(params: &Value) -> Result<MappingSource, CommandError> {
    let what = "a key (key:Q, key:Space) or a MIDI note (note:<channel 1-16>:<note 0-127>)";
    let text = Args::new(params).raw_str("source").ok_or_else(|| format!("source must be {what}"))?;
    MappingSource::from_name(text).ok_or_else(|| format!("source must be {what}, got {text:?}").into())
}

/// The target `params.target` names, with the track and scene it needs, checked against the
/// project: the track has slots and the scene exists.
fn target(p: &Project, params: &Value) -> Result<MappingTarget, CommandError> {
    let kind = Args::new(params).raw_str("target").ok_or("target must be slot, scene or stop")?;
    let track = || -> Result<_, CommandError> {
        column_index(p, params)?;
        track_id(p, params)
    };
    match kind {
        "slot" => Ok(MappingTarget::Slot { track: track()?, scene: scene_index(p, params)? }),
        "scene" => Ok(MappingTarget::Scene { scene: scene_index(p, params)? }),
        "stop" => Ok(MappingTarget::Stop { track: track()? }),
        other => Err(format!("target must be slot, scene or stop, got {other:?}").into()),
    }
}

/// Whether `target` still points at something in `p`: its track has slots, its scene exists.
pub fn target_exists(p: &Project, target: &MappingTarget) -> bool {
    let track_ok = target.track().is_none_or(|t| p.launcher.columns.iter().any(|c| c.track == t));
    let scene_ok = target.scene().is_none_or(|s| s < p.launcher.scenes.len());
    track_ok && scene_ok
}

/// What the commands and `launcher.mappings` say about a mapping.
fn describe(p: &Project, m: &LaunchMapping) -> Value {
    let mut out = json!({
        "source": m.source.name(),
        "label": m.source.label(),
        "target": m.target.kind(),
        "command": m.target.command(),
        "broken": !target_exists(p, &m.target),
    });
    if let Some(map) = out.as_object_mut() {
        if let Some(track) = m.target.track() {
            map.insert("track".into(), json!(track.0));
        }
        if let Some(scene) = m.target.scene() {
            map.insert("scene".into(), json!(scene.saturating_add(1)));
        }
    }
    out
}

pub(crate) fn add_mapping(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let source = source(params)?;
    let target = target(&session.project, params)?;
    let mappings = &mut session.project.launcher.mappings;
    let full = mappings.len() >= MAX_MAPPINGS;
    let replaced = match mappings.iter_mut().find(|m| m.source == source) {
        Some(existing) => Some(std::mem::replace(&mut existing.target, target)),
        None if full => return Err(format!("a project keeps at most {MAX_MAPPINGS} mappings").into()),
        None => {
            mappings.push(LaunchMapping { source: source.clone(), target });
            None
        }
    };
    let mut out = describe(&session.project, &LaunchMapping { source, target });
    if let (Some(map), Some(old)) = (out.as_object_mut(), replaced) {
        map.insert("replaced".into(), json!(old.kind()));
    }
    Ok(out)
}

pub(crate) fn remove_mapping(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &session.project;
    let removed: Vec<LaunchMapping> = match (Args::new(params).value("source"), Args::new(params).value("target")) {
        (Some(_), None) => {
            let source = source(params)?;
            p.launcher.mappings.iter().filter(|m| m.source == source).cloned().collect()
        }
        (None, Some(_)) => {
            let target = target(p, params)?;
            p.launcher.mappings.iter().filter(|m| m.target == target).cloned().collect()
        }
        _ => return Err("give either source, or target with its track and scene".into()),
    };
    if removed.is_empty() {
        return Err("nothing is mapped there".into());
    }
    let list: Vec<Value> = removed.iter().map(|m| describe(p, m)).collect();
    session.project.launcher.mappings.retain(|m| !removed.contains(m));
    Ok(json!({ "removed": list }))
}

pub(crate) fn list_mappings(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let p = session.project();
    Ok(json!({ "mappings": p.launcher.mappings.iter().map(|m| describe(p, m)).collect::<Vec<_>>() }))
}

/// The command a press (`pressed`) or release of `source` runs, with its parameters, as the
/// mouse would send it: a press of a slot is `clip.launch`, its release `clip.release` when the
/// slot's clip is in a mode that listens for it (gate, repeat); a scene's press is
/// `scene.launch` and a stop button's `track.stop`, and their releases run nothing. `None` when
/// the source is not mapped or its target is gone.
pub fn mapped_command(project: &Project, source: &MappingSource, pressed: bool) -> Option<(&'static str, Value)> {
    let mapping = project.launcher.mappings.iter().find(|m| &m.source == source)?;
    if !target_exists(project, &mapping.target) {
        return None;
    }
    match (mapping.target, pressed) {
        (MappingTarget::Slot { track, scene }, true) => {
            Some(("clip.launch", json!({ "track": track.0, "scene": scene.saturating_add(1) })))
        }
        (MappingTarget::Slot { track, scene }, false) => {
            let clip = project
                .launcher
                .columns
                .iter()
                .find(|c| c.track == track)
                .and_then(|c| c.slots.get(scene))
                .and_then(|s| s.clip.as_ref())?;
            clip.launch
                .mode
                .uses_release()
                .then(|| ("clip.release", json!({ "track": track.0, "scene": scene.saturating_add(1) })))
        }
        (MappingTarget::Scene { scene }, true) => Some(("scene.launch", json!({ "scene": scene.saturating_add(1) }))),
        (MappingTarget::Stop { track }, true) => Some(("track.stop", json!({ "track": track.0 }))),
        (MappingTarget::Scene { .. } | MappingTarget::Stop { .. }, false) => None,
    }
}
