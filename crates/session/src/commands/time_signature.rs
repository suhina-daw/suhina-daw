// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `timeSignature.*` commands; what runs them is in the modules they name.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "timeSignature.add",
        label: "Change the time signature from the start of a bar (one already at that bar is replaced)",
        params: &[
            param("bar", ParamType::Integer, "bar number, from 2 (bar 1 has the start signature)"),
            param("numerator", ParamType::Integer, "beats in a bar, 1 to 99"),
            param("denominator", ParamType::Integer, "the note value of a beat: 1, 2, 4, 8, 16, 32 or 64"),
        ],
        kind: CommandKind::Edit,
        run: crate::tempo::add_signature,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "timeSignature.move",
        label: "Move a time-signature change to another bar, or change its signature",
        params: &[
            param("bar", ParamType::Integer, "the bar the change is at, from 2"),
            optional("to", ParamType::Integer, "the bar to move it to, from 2"),
            optional("numerator", ParamType::Integer, "beats in a bar, 1 to 99"),
            optional("denominator", ParamType::Integer, "1, 2, 4, 8, 16, 32 or 64"),
        ],
        kind: CommandKind::Edit,
        run: crate::tempo::move_signature,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "timeSignature.delete",
        label: "Remove a time-signature change",
        params: &[param("bar", ParamType::Integer, "the bar the change is at, from 2")],
        kind: CommandKind::Edit,
        run: crate::tempo::delete_signature,
        policy: Policy::DEFAULT,
    },
];
