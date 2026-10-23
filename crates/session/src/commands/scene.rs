// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `scene.*` commands, and what runs them here.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "scene.launch",
        label: "Launch a scene: every track launches its slot in that row at the next quantisation boundary",
        params: &[SCENE],
        kind: CommandKind::Transport,
        run: launch_scene,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "scene.stop",
        label: "Stop a scene's row: every track whose slot in that row has a clip stops at the next quantisation boundary",
        params: &[SCENE],
        kind: CommandKind::Transport,
        run: stop_scene,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "scene.insert",
        label: "Insert an empty scene (a row of empty slots with stop buttons) at a position, or after the last scene",
        params: &[
            optional(
                "scene",
                ParamType::Integer,
                "number the new scene gets, from 1 to one past the last scene; after the last scene if left out",
            ),
            optional("name", ParamType::Text, "the new scene's name; empty if left out"),
        ],
        kind: CommandKind::Edit,
        run: insert_scene,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "scene.duplicate",
        label: "Copy a scene, with its name, tempo, time signature and the clips in its row, to a new scene right below it",
        params: &[SCENE],
        kind: CommandKind::Edit,
        run: duplicate_scene,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "scene.delete",
        label: "Delete a scene and the clips in its row; the last scene left cannot be deleted",
        params: &[SCENE],
        kind: CommandKind::Edit,
        run: delete_scene,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "scene.rename",
        label: "Rename a scene",
        params: &[SCENE, param("name", ParamType::Text, "the new name; empty clears it")],
        kind: CommandKind::Edit,
        run: rename_scene,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "scene.setTempo",
        label: "Set the tempo a scene switches the project to when it launches, or clear it",
        params: &[
            SCENE,
            optional("bpm", ParamType::Number, "20 … 999; left out or null, the scene keeps the tempo playing"),
        ],
        kind: CommandKind::Edit,
        run: set_scene_tempo,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "scene.setTimeSignature",
        label: "Set the time signature a scene switches to when it launches, or clear it",
        params: &[
            SCENE,
            optional(
                "numerator",
                ParamType::Integer,
                "beats in a bar, 1 … 99; left out with the denominator, the scene keeps the time signature playing",
            ),
            optional("denominator", ParamType::Integer, "the note value of a beat: 1, 2, 4, 8 or 16"),
        ],
        kind: CommandKind::Edit,
        run: set_scene_time_signature,
        policy: Policy::DEFAULT,
    },
];

fn launch_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(session.project(), params)?;
    send(session, LaunchCommand::LaunchScene { scene })?;
    Ok(json!({ "scene": scene + 1, "queued": "launch" }))
}

fn stop_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(session.project(), params)?;
    send(session, LaunchCommand::StopScene { scene })?;
    Ok(json!({ "scene": scene + 1, "queued": "stop" }))
}

/// Scenes a project can have; inserting stops there.
const MAX_SCENES: usize = 1000;

/// Gives every launcher column one slot per scene, so a row can be inserted or removed in all of
/// them at the same index.
fn even_slots(p: &mut Project) {
    let scenes = p.launcher.scenes.len();
    for column in &mut p.launcher.columns {
        column.slots.resize_with(scenes, || Slot { clip: None, has_stop: true });
    }
}

/// An optional scene name: trimmed text, empty when left out.
fn scene_name(params: &Value) -> Result<String, CommandError> {
    match params.get("name") {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(name)) => Ok(name.trim().to_string()),
        Some(_) => Err("name must be a text".into()),
    }
}

fn insert_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let p = &mut session.project;
    let count = p.launcher.scenes.len();
    if count >= MAX_SCENES {
        return Err(format!("the project has {count} scenes, as many as it can have").into());
    }
    let at = match params.get("scene") {
        None | Some(Value::Null) => count,
        Some(value) => value
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .and_then(|n| n.checked_sub(1))
            .filter(|i| *i <= count)
            .ok_or_else(|| format!("scene must be a whole number from 1 to {}", count + 1))?,
    };
    let name = scene_name(params)?;
    even_slots(p);
    let id = p.new_scene_id()?;
    p.launcher.scenes.insert(at, Scene { id, name, ..Scene::default() });
    for column in &mut p.launcher.columns {
        column.slots.insert(at, Slot { clip: None, has_stop: true });
    }
    Ok(json!({ "scene": at + 1 }))
}

fn duplicate_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(&session.project, params)?;
    let p = &mut session.project;
    if p.launcher.scenes.len() >= MAX_SCENES {
        return Err(format!("the project has {} scenes, as many as it can have", p.launcher.scenes.len()).into());
    }
    even_slots(p);
    let mut copy = p.launcher.scenes.get(scene).cloned().unwrap_or_default();
    copy.id = p.new_scene_id()?;
    p.launcher.scenes.insert(scene + 1, copy);
    let mut next_content = p.contents.iter().map(|c| c.id.0).max().unwrap_or(0);
    for column in &mut p.launcher.columns {
        let mut slot = column.slots.get(scene).cloned().unwrap_or(Slot { clip: None, has_stop: true });
        // Each copied clip plays its own content, so editing one does not change the other.
        if let Some(clip) = slot.clip.as_mut()
            && let Some(content) = p.contents.iter().find(|c| c.id == clip.content).cloned()
        {
            next_content = next_content.checked_add(1).ok_or("the project has too many clip contents")?;
            let id = suhina_model::ContentId(next_content);
            p.contents.push(suhina_model::ClipContent { id, ..content });
            clip.content = id;
        }
        // The copy is a new clip: it gets its own id when the command is done.
        if let Some(clip) = slot.clip.as_mut() {
            clip.id = suhina_model::ClipId::NONE;
        }
        column.slots.insert(scene + 1, slot);
    }
    Ok(json!({ "scene": scene + 2 }))
}

fn delete_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(&session.project, params)?;
    let p = &mut session.project;
    if p.launcher.scenes.len() <= 1 {
        return Err("the last scene cannot be deleted".into());
    }
    even_slots(p);
    p.launcher.scenes.remove(scene);
    for column in &mut p.launcher.columns {
        column.slots.remove(scene);
    }
    Ok(json!({ "deleted": scene + 1 }))
}

fn rename_scene(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(&session.project, params)?;
    let name = match params.get("name") {
        Some(Value::String(name)) => name.trim().to_string(),
        _ => return Err("name must be a text (empty clears it)".into()),
    };
    if let Some(s) = session.project.launcher.scenes.get_mut(scene) {
        s.name.clone_from(&name);
    }
    Ok(json!({ "scene": scene + 1, "name": name }))
}

fn set_scene_tempo(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(&session.project, params)?;
    let bpm = Args::new(params).opt_f64("bpm")?;
    if let Some(bpm) = bpm
        && !(20.0..=999.0).contains(&bpm)
    {
        return Err(format!("bpm must be between 20 and 999, got {bpm}").into());
    }
    if let Some(s) = session.project.launcher.scenes.get_mut(scene) {
        s.tempo_bpm = bpm;
    }
    Ok(json!({ "scene": scene + 1, "tempo_bpm": bpm }))
}

fn set_scene_time_signature(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scene = scene_index(&session.project, params)?;
    let part = |key: &str| match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| CommandError::from(format!("{key} must be a whole number"))),
    };
    let signature = match (part("numerator")?, part("denominator")?) {
        (None, None) => None,
        (Some(numerator), Some(denominator)) => {
            if !(1..=99).contains(&numerator) {
                return Err(format!("numerator must be between 1 and 99, got {numerator}").into());
            }
            if ![1, 2, 4, 8, 16].contains(&denominator) {
                return Err(format!("denominator must be 1, 2, 4, 8 or 16, got {denominator}").into());
            }
            Some(TimeSignature { numerator, denominator })
        }
        _ => return Err("give both numerator and denominator, or neither to clear the time signature".into()),
    };
    if let Some(s) = session.project.launcher.scenes.get_mut(scene) {
        s.time_signature = signature;
    }
    Ok(json!({ "scene": scene + 1, "time_signature": signature.map(signature_text) }))
}
