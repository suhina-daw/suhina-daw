// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `commands.*` commands; what runs them is in the modules they name.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "command.list",
    label: "List every command with its parameters",
    params: &[],
    kind: CommandKind::Read,
    run: list,
    policy: Policy::DEFAULT,
}];
