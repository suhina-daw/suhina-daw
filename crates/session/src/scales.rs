// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Clip scales: `clip.setScale` and `project.setDefaultScale` (`docs/spec/ui-clip-editor.md` ›
//! Scale and key, ADR 0001 › Amendment: clip scales).
//!
//! A scale belongs to the clip's content. Setting it through a clip whose content other clips also
//! play gives that clip its own copy first, as a note edit does ([`crate::notes::copy_on_write`]),
//! so one undo takes back both.

use serde_json::{Value, json};
use suhina_model::{Project, Scale};

use crate::commands::Args;
use crate::note_tools::{mode_of, root_of};
use crate::notes::{check_midi, copy_on_write, locate};
use crate::{CommandError, Session};

/// A scale as the commands return it: `{ "root": "F#", "mode": "minor" }`, or `null` for none.
pub(crate) fn scale_value(scale: Option<Scale>) -> Value {
    match scale {
        Some(scale) => json!({ "root": scale.root_name(), "mode": scale.mode.name() }),
        None => Value::Null,
    }
}

/// The scale of the MIDI clip `params` names, if it has one.
pub(crate) fn scale_of_clip(p: &Project, params: &Value) -> Result<Option<Scale>, CommandError> {
    let (_, content, _) = locate(p, params)?;
    check_midi(p, content)?;
    Ok(p.content(content).and_then(|c| c.scale))
}

/// Whether `params.off` asks for no scale.
fn off(params: &Value) -> Result<bool, CommandError> {
    match Args::new(params).value("off") {
        None => Ok(false),
        Some(value) => value.as_bool().ok_or_else(|| "off must be true or false".into()),
    }
}

/// The scale `params` asks for over `current`: `None` for `off: true`; otherwise `root` and
/// `mode`, the one left out kept from `current` (C or major when there is none).
fn wanted(params: &Value, current: Option<Scale>) -> Result<Option<Scale>, CommandError> {
    let given = |key: &str| Args::new(params).has(key);
    let (has_root, has_mode) = (given("root"), given("mode"));
    if off(params)? {
        if has_root || has_mode {
            return Err("give off: true, or root and mode, not both".into());
        }
        return Ok(None);
    }
    if !(has_root || has_mode) {
        return Err("give root (a key name such as C or F#, or 0 to 11), mode, or off: true".into());
    }
    let root = if has_root { root_of(params)? } else { current.map_or(0, Scale::root) };
    let mode = match mode_of(params, "mode")? {
        Some(mode) => mode,
        None => current.map(|s| s.mode).unwrap_or_default(),
    };
    Scale::new(root, mode).map(Some).ok_or_else(|| "root must be 0 to 11".into())
}

/// `clip.setScale`: sets the scale of a MIDI clip's content, or clears it with `off: true`.
pub(crate) fn set_scale(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let current = scale_of_clip(&session.project, params)?;
    let scale = wanted(params, current)?;
    if scale == current {
        return Ok(json!({ "scale": scale_value(scale), "changed": false }));
    }
    copy_on_write(&mut session.project, params, |p| {
        let (_, id, _) = locate(p, params)?;
        let content = p.contents.iter_mut().find(|c| c.id == id).ok_or("the clip's content is missing")?;
        content.scale = scale;
        Ok(json!({ "scale": scale_value(scale), "changed": true }))
    })
}

/// `project.setDefaultScale`: the scale new MIDI clips take, or none with `off: true`. Clips that
/// exist keep theirs.
pub(crate) fn set_default_scale(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let current = session.project.default_scale;
    let scale = wanted(params, current)?;
    session.project.default_scale = scale;
    Ok(json!({ "default_scale": scale_value(scale), "changed": scale != current }))
}

/// `clip.scale`: the scale of the MIDI clip `params` names and the project's default scale.
pub(crate) fn clip_scale(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let scale = scale_of_clip(&session.project, params)?;
    Ok(json!({ "scale": scale_value(scale), "default_scale": scale_value(session.project.default_scale) }))
}
