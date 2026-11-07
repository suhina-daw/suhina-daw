// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Controller mappings: what the `mapping.*` commands run to add, list and remove the keys and
//! MIDI notes that run a command (so far the launch commands of slots, scenes and stop buttons),
//! and [`mapped_command`], which turns a press or release of a mapped key or note into the
//! command the mouse would run (`docs/spec/midi-mapping.md` › Launch keys).

use serde_json::{Map, Value, json};
use suhina_model::{ControllerMapping, MappingSource, MappingTarget, Project};

use crate::commands::Args;
use crate::commands::{column_index, scene_id, scene_index, track_id};
use crate::{CommandError, Session};

pub use suhina_model::MAX_MAPPINGS;

fn source(params: &Value) -> Result<MappingSource, CommandError> {
    let what = "a key (key:Q, key:Space) or a MIDI note (note:<channel 1-16>:<note 0-127>)";
    let text = Args::new(params).raw_str("source").ok_or_else(|| format!("source must be {what}"))?;
    MappingSource::from_name(text).ok_or_else(|| format!("source must be {what}, got {text:?}").into())
}

/// The target `params.target` names, `{ "command", "track", "scene" }`, with the track and scene
/// its command needs, checked against the project: the track has slots and the scene exists. A
/// parameter the command does not take is refused, as dispatch refuses one for a command.
fn target(p: &Project, params: &Value) -> Result<MappingTarget, CommandError> {
    let commands = MappingTarget::COMMANDS.join(", ");
    let t = Args::new(params).value("target").ok_or("target must be given")?;
    let object = t.as_object().ok_or("target must be an object with its command")?;
    let command = object
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("target.command must be one of {commands}"))?;
    let takes = MappingTarget::takes(command)
        .ok_or_else(|| format!("target.command must be one of {commands}, got {command:?}"))?;
    if let Some(key) = object.keys().find(|k| k.as_str() != "command" && !takes.contains(&k.as_str())) {
        return Err(format!("target: {command} takes {}, not {key:?}", takes.join(" and ")).into());
    }
    for key in takes {
        if !Args::new(t).has(key) {
            return Err(format!("target: {command} needs a {key}").into());
        }
    }
    let track = |t: &Value| -> Result<_, CommandError> {
        column_index(p, t)?;
        track_id(p, t)
    };
    let track = if takes.contains(&"track") { Some(track(t)?) } else { None };
    let scene = if takes.contains(&"scene") { Some(scene_index(p, t)?) } else { None };
    MappingTarget::from_command(command, track, scene).ok_or_else(|| format!("target: {command} is incomplete").into())
}

/// Whether `target` still points at something in `p`: its track has slots, its scene exists.
pub fn target_exists(p: &Project, target: &MappingTarget) -> bool {
    let track_ok = target.track().is_none_or(|t| p.launcher.columns.iter().any(|c| c.track == t));
    let scene_ok = target.scene().is_none_or(|s| s < p.launcher.scenes.len());
    track_ok && scene_ok
}

/// A target as `mapping.add` takes it: its command and the track and scene that command fixes.
fn target_value(target: &MappingTarget) -> Value {
    let mut out = Map::new();
    out.insert("command".into(), json!(target.command()));
    if let Some(track) = target.track() {
        out.insert("track".into(), json!(track.0));
    }
    if let Some(scene) = target.scene() {
        out.insert("scene".into(), json!(scene));
    }
    Value::Object(out)
}

/// What the commands say about the mapping at `index`: its source, its target as `mapping.add`
/// takes it, the scene's id beside it, the command a slot's release runs, and whether the
/// target is gone.
fn describe(p: &Project, index: usize, m: &ControllerMapping) -> Value {
    let mut out = json!({
        "index": index,
        "source": m.source.name(),
        "label": m.source.label(),
        "target": target_value(&m.target),
        "broken": !target_exists(p, &m.target),
    });
    if let Some(map) = out.as_object_mut() {
        if let Some(scene) = m.target.scene() {
            map.insert("scene_id".into(), scene_id(p, scene));
        }
        if let MappingTarget::Slot { .. } = m.target {
            map.insert("release".into(), json!("clip.release"));
        }
    }
    out
}

/// Keeps mappings on their scenes when a row is inserted at `at`: a target on a scene from `at`
/// on moves down one (`scene.insert`, `scene.duplicate`).
pub(crate) fn scenes_inserted(p: &mut Project, at: usize) {
    for m in &mut p.controller_mappings {
        match &mut m.target {
            MappingTarget::Slot { scene, .. } | MappingTarget::Scene { scene } if *scene >= at => {
                *scene = scene.saturating_add(1)
            }
            _ => {}
        }
    }
}

/// Keeps mappings on their scenes when the scene at `at` is deleted: a mapping on it goes with it
/// (undo brings it back), and one on a later scene moves up one (`scene.delete`).
pub(crate) fn scene_deleted(p: &mut Project, at: usize) {
    p.controller_mappings.retain_mut(|m| match &mut m.target {
        MappingTarget::Slot { scene, .. } | MappingTarget::Scene { scene } => match (*scene).cmp(&at) {
            std::cmp::Ordering::Equal => false,
            std::cmp::Ordering::Greater => {
                *scene -= 1;
                true
            }
            std::cmp::Ordering::Less => true,
        },
        MappingTarget::Stop { .. } => true,
    });
}

pub(crate) fn add_mapping(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let source = source(params)?;
    let target = target(&session.project, params)?;
    let mappings = &mut session.project.controller_mappings;
    let full = mappings.len() >= MAX_MAPPINGS;
    let (index, replaced) = match mappings.iter().position(|m| m.source == source) {
        Some(index) => (index, Some(std::mem::replace(&mut mappings[index].target, target))),
        None if full => return Err(format!("a project keeps at most {MAX_MAPPINGS} mappings").into()),
        None => {
            mappings.push(ControllerMapping { source: source.clone(), target });
            (mappings.len() - 1, None)
        }
    };
    let mut out = describe(&session.project, index, &ControllerMapping { source, target });
    if let (Some(map), Some(old)) = (out.as_object_mut(), replaced) {
        map.insert("replaced".into(), target_value(&old));
    }
    Ok(out)
}

pub(crate) fn remove_mapping(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &session.project;
    let args = Args::new(params);
    let mappings = p.controller_mappings.iter().enumerate();
    let removed: Vec<(usize, &ControllerMapping)> = match (args.has("index"), args.has("source"), args.has("target")) {
        (true, false, false) => {
            let index = args.index("index", "from 0, as in mapping.list")?;
            let count = p.controller_mappings.len();
            let m = p
                .controller_mappings
                .get(index)
                .ok_or_else(|| format!("mapping {index} does not exist; the project has {count} mappings"))?;
            vec![(index, m)]
        }
        (false, true, false) => {
            let source = source(params)?;
            mappings.filter(|(_, m)| m.source == source).collect()
        }
        (false, false, true) => {
            let target = target(p, params)?;
            mappings.filter(|(_, m)| m.target == target).collect()
        }
        _ => return Err("give one of index, source or target".into()),
    };
    if removed.is_empty() {
        return Err("nothing is mapped there".into());
    }
    let list: Vec<Value> = removed.iter().map(|(i, m)| describe(p, *i, m)).collect();
    let indices: Vec<usize> = removed.iter().map(|(i, _)| *i).collect();
    for index in indices.into_iter().rev() {
        session.project.controller_mappings.remove(index);
    }
    Ok(json!({ "removed": list }))
}

pub(crate) fn list_mappings(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    let p = session.project();
    let list: Vec<Value> = p.controller_mappings.iter().enumerate().map(|(i, m)| describe(p, i, m)).collect();
    Ok(json!({ "mappings": list }))
}

/// The command a press (`pressed`) or release of `source` runs, with its parameters, as the
/// mouse would send it: a press of a slot is `clip.launch`, its release `clip.release` when the
/// slot's clip is in a mode that listens for it (gate, repeat); a scene's press is
/// `scene.launch` and a stop button's `track.stop`, and their releases run nothing. `None` when
/// the source is not mapped or its target is gone.
pub fn mapped_command(project: &Project, source: &MappingSource, pressed: bool) -> Option<(&'static str, Value)> {
    let mapping = project.controller_mappings.iter().find(|m| &m.source == source)?;
    if !target_exists(project, &mapping.target) {
        return None;
    }
    match (mapping.target, pressed) {
        (MappingTarget::Slot { track, scene }, true) => {
            Some(("clip.launch", json!({ "track": track.0, "scene": scene })))
        }
        (MappingTarget::Slot { track, scene }, false) => {
            let clip = project
                .launcher
                .columns
                .iter()
                .find(|c| c.track == track)
                .and_then(|c| c.slots.get(scene))
                .and_then(|s| s.clip.as_ref())?;
            clip.launch.mode.uses_release().then(|| ("clip.release", json!({ "track": track.0, "scene": scene })))
        }
        (MappingTarget::Scene { scene }, true) => Some(("scene.launch", json!({ "scene": scene }))),
        (MappingTarget::Stop { track }, true) => Some(("track.stop", json!({ "track": track.0 }))),
        (MappingTarget::Scene { .. } | MappingTarget::Stop { .. }, false) => None,
    }
}
