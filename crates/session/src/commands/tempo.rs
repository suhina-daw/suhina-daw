// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `tempo.*` commands; what runs them is in the modules they name.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "tempo.points",
        label: "List the tempo map: the start tempo and every tempo change (numbered from 1), the start time signature and every time-signature change",
        params: &[],
        kind: CommandKind::Read,
        run: crate::tempo::points,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "tempo.addPoint",
        label: "Add a tempo change at a beat: a step, or a ramp that glides from the tempo before it (a beat holds a ramp and a step; one of the same kind already there is replaced)",
        params: &[
            param("beat", ParamType::Number, "where the change is, in beats from the start, after 0"),
            param("bpm", ParamType::Number, "the tempo from there, 20 to 999"),
            optional(
                "ramp",
                ParamType::Bool,
                "glide from the tempo before to this one, reaching it at the beat (default false: a step)",
            ),
        ],
        kind: CommandKind::Edit,
        run: crate::tempo::add_point,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "tempo.movePoint",
        label: "Move a tempo change to another beat, or change its tempo or whether it ramps",
        params: &[
            param("point", ParamType::Integer, "tempo point number, from 1, as in tempo.points"),
            optional("beat", ParamType::Number, "the new beat, after 0"),
            optional("bpm", ParamType::Number, "the new tempo, 20 to 999"),
            optional("ramp", ParamType::Bool, "whether it glides from the tempo before"),
        ],
        kind: CommandKind::Edit,
        run: crate::tempo::move_point,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "tempo.deletePoint",
        label: "Remove a tempo change",
        params: &[param("point", ParamType::Integer, "tempo point number, from 1, as in tempo.points")],
        kind: CommandKind::Edit,
        run: crate::tempo::delete_point,
        policy: Policy::DEFAULT,
    },
];
