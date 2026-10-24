// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `edit.*` commands, and what runs them here.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "edit.undo",
        label: "Undo the last edit",
        params: &[],
        kind: CommandKind::History,
        run: undo,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "edit.redo",
        label: "Redo the last undone edit",
        params: &[],
        kind: CommandKind::History,
        run: redo,
        policy: Policy::DEFAULT,
    },
];

fn redo(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    if session.redo() { Ok(json!({ "redone": true })) } else { Err("nothing to redo".into()) }
}
