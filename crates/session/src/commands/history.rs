// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `edit.*` commands, and what runs them here.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "edit.undo",
        label: "Undo the last edit",
        params: &[],
        kind: CommandKind::HISTORY,
        run: undo,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "edit.redo",
        label: "Redo the last undone edit",
        params: &[],
        kind: CommandKind::HISTORY,
        run: redo,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "history.endGesture",
        label: "End a drag's gesture: its calls so far are one undo step, and the next call starts another",
        params: &[param("gesture", ParamType::Text, "the gesture's id, as its calls gave it")],
        // It changes no project, only where the next undo step starts.
        kind: CommandKind::READ,
        run: end_gesture,
        policy: Policy::UNHEARD,
    },
];

fn end_gesture(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let gesture = Args::new(params).raw_str("gesture").filter(|g| !g.is_empty() && g.len() <= 128);
    let gesture = gesture.ok_or("gesture must be 1 to 128 bytes of text")?;
    Ok(json!({ "ended": session.end_gesture(gesture) }))
}

fn redo(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    if session.redo() { Ok(json!({ "redone": true })) } else { Err("nothing to redo".into()) }
}
