// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `branch.*` commands; what runs them is in the modules they name.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "branch.add",
        label: "Add an empty branch to a device group",
        params: &[
            TRACK,
            GROUP,
            optional("position", ParamType::Integer, "where among the branches, from 0 (default: last)"),
            optional("name", ParamType::Text, "the branch's name (default Branch N)"),
        ],
        kind: CommandKind::Edit,
        run: device_groups::branch_add,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "branch.remove",
        label: "Remove a branch of a device group with its devices; the last branch stays",
        params: &[TRACK, GROUP, BRANCH],
        kind: CommandKind::Edit,
        run: device_groups::branch_remove,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "branch.duplicate",
        label: "Copy a branch of a device group next to itself",
        params: &[TRACK, GROUP, BRANCH],
        kind: CommandKind::Edit,
        run: device_groups::branch_duplicate,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "branch.rename",
        label: "Rename a branch of a device group",
        params: &[TRACK, GROUP, BRANCH, param("name", ParamType::Text, "the new name")],
        kind: CommandKind::Edit,
        run: device_groups::branch_rename,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "branch.setVolume",
        label: "Set a branch's volume in a device group, as track.setVolume",
        params: &[
            TRACK,
            GROUP,
            BRANCH,
            param("db", ParamType::Decibels, "level in dB, at most +6; \"-inf\" or below -150 is silence"),
        ],
        kind: CommandKind::Edit,
        run: device_groups::branch_set_volume,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "branch.setPan",
        label: "Set a branch's pan in a device group, as track.setPan",
        params: &[TRACK, GROUP, BRANCH, param("pan", ParamType::Number, "-1 hard left, 0 centre, 1 hard right")],
        kind: CommandKind::Edit,
        run: device_groups::branch_set_pan,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "branch.setMute",
        label: "Mute or unmute a branch of a device group; its devices keep running",
        params: &[TRACK, GROUP, BRANCH, param("mute", ParamType::Bool, "true mutes the branch")],
        kind: CommandKind::Edit,
        run: device_groups::branch_set_mute,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "branch.setSolo",
        label: "Solo a branch within its device group: while any branch is soloed only soloed branches sound",
        params: &[TRACK, GROUP, BRANCH, param("solo", ParamType::Bool, "true solos the branch")],
        kind: CommandKind::Edit,
        run: device_groups::branch_set_solo,
        policy: Policy::DEFAULT,
    },
];
