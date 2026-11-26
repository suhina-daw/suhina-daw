// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window actions a command may ask for: the detail area's show/hide and its Clip / Devices
//! switch. A surface program's buttons reach the window through these; the window runs the
//! action at its next frame.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "view.detail",
        label: "Show or hide the detail area, as the window's Detail View button does",
        params: &[],
        kind: CommandKind::on(Target::Project, Effect::Change),
        run: toggle_detail,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "view.devices",
        label: "Switch the detail area between the shown clip and its track's devices, as Clip / Devices does",
        params: &[],
        kind: CommandKind::on(Target::Project, Effect::Change),
        run: toggle_devices,
        policy: Policy::DEFAULT,
    },
];

/// Most window actions a session holds before a window drains them.
const MAX_VIEW_ACTIONS: usize = 8;

fn queue(session: &mut Session, id: &str, params: Value) -> Result<Value, CommandError> {
    if session.view_actions.len() >= MAX_VIEW_ACTIONS {
        return Err(format!(
            "the window already has {MAX_VIEW_ACTIONS} actions waiting; try again after its next frame"
        )
        .into());
    }
    session.view_actions.push((id.to_string(), params));
    Ok(json!({ "queued": true }))
}

fn toggle_detail(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    queue(session, "view.detail", json!({}))
}

fn toggle_devices(session: &mut Session, _: &Value) -> Result<Value, CommandError> {
    queue(session, "view.devices", json!({}))
}
