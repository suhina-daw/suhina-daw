// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `range.*` commands; what runs them is in the modules they name.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "range.split",
        label: "Split the clips of some tracks at both edges of a time selection, so the span is clips of its own; answers with the clips inside it",
        params: &[TRACKS, RANGE_START, RANGE_END],
        kind: CommandKind::Edit,
        run: crate::range::split_range,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "range.insertTime",
        label: "Insert empty time as long as a time selection at its start, on every track: clips crossing it are split there, and clips, markers, automation, tempo changes and the loop region after it move later by as much (time-signature changes too, when the length is whole bars)",
        params: &[RANGE_START, RANGE_END],
        kind: CommandKind::Edit,
        run: crate::range::insert_time,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "range.delete",
        label: "Delete what plays inside a time selection on some tracks: clips crossing an edge keep their parts outside it, and the gap stays",
        params: &[TRACKS, RANGE_START, RANGE_END],
        kind: CommandKind::Edit,
        run: crate::range::delete_range,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "range.copy",
        label: "Copy what plays inside a time selection on some tracks, cut to it, for clip.paste; a paste with no track puts each part back on its own track",
        params: &[TRACKS, RANGE_START, RANGE_END],
        kind: CommandKind::Read,
        run: crate::range::copy_range,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "range.cut",
        label: "Copy what plays inside a time selection for clip.paste and delete it, as one undo step",
        params: &[TRACKS, RANGE_START, RANGE_END],
        kind: CommandKind::Edit,
        run: crate::range::cut_range,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "range.duplicate",
        label: "Put a copy of what plays inside a time selection right after it, on the same tracks",
        params: &[
            TRACKS,
            RANGE_START,
            RANGE_END,
            optional(
                "stack",
                ParamType::Bool,
                "true leaves clips under the copy as they are; false (default) replaces them",
            ),
        ],
        kind: CommandKind::Edit,
        run: crate::range::duplicate_range,
        policy: Policy::DEFAULT,
    },
];
