// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `return.*` commands; what runs them is in the modules they name.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "return.add",
        label: "Add a return track, after every other track, playing into the main channel; tracks reach it through sends",
        params: &[optional("name", ParamType::Text, "its name (default: A Return, B Return and so on)")],
        kind: CommandKind::Edit,
        run: crate::sends::add_return,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "return.remove",
        label: "Remove a return track and every send to it",
        params: &[RETURN],
        kind: CommandKind::Edit,
        run: crate::sends::remove_return,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "return.rename",
        label: "Rename a return track",
        params: &[RETURN, param("name", ParamType::Text, "the new name, not empty")],
        kind: CommandKind::Edit,
        run: crate::sends::rename_return,
        policy: Policy::DEFAULT,
    },
];
