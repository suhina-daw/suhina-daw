// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `automation.*` commands; what runs them is in the modules they name.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "automation.addPoint",
        label: "Add a point to a parameter's automation (envelope) over the arrangement, making the lane if it has none; the value moves from point to point along each segment's curve. Without a value the point goes on the envelope and a bent segment is split so it plays as before",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            AUTOMATED,
            param("beat", ParamType::Number, "arrangement beat, from 0; a second point at one beat makes a jump"),
            optional(
                "value",
                ParamType::AutomationValue,
                "value in the unit the parameter's set command takes: volume in dB (at most +6; below -150 is silence), pan -1 to 1, mute 0 or 1, a device's as device.setParam; left out, the envelope's value at the beat",
            ),
            CURVE,
        ],
        kind: CommandKind::Edit,
        run: crate::automation::add_point,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "automation.movePoint",
        label: "Move a point of a parameter's automation to another beat or value, or change its curve",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            AUTOMATED,
            POINT,
            optional("beat", ParamType::Number, "new arrangement beat"),
            optional("value", ParamType::AutomationValue, "new value, as for automation.addPoint"),
            CURVE,
        ],
        kind: CommandKind::Edit,
        run: crate::automation::move_point,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "automation.deletePoint",
        label: "Delete a point of a parameter's automation; the lane goes with its last point",
        params: &[TRACK, AUTOMATED_DEVICE, AUTOMATED, POINT],
        kind: CommandKind::Edit,
        run: crate::automation::delete_point,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "automation.movePoints",
        label: "Move several points of one parameter's automation together, by a number of beats and by a value, in one undo step; values stay in the parameter's range",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            AUTOMATED,
            param(
                "points",
                ParamType::Numbers,
                "point indices on the lane, from 0 in beat order, as in automation.list",
            ),
            optional(
                "beats",
                ParamType::Number,
                "beats to move the points by: later above 0, earlier below; they must stay from beat 0",
            ),
            optional(
                "value",
                ParamType::Number,
                "how much to change each value by, in the unit of automation.addPoint (dB for the volume, where silence stays silent)",
            ),
            optional(
                "curve",
                ParamType::Curve,
                "a curve to give every moved point's segment, as for automation.addPoint",
            ),
        ],
        kind: CommandKind::Edit,
        run: crate::automation::move_points,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "automation.deletePoints",
        label: "Delete several points of one parameter's automation in one undo step; the lane goes with its last point",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            AUTOMATED,
            param(
                "points",
                ParamType::Numbers,
                "point indices on the lane, from 0 in beat order, as in automation.list",
            ),
        ],
        kind: CommandKind::Edit,
        run: crate::automation::delete_points,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "automation.clear",
        label: "Remove a parameter's automation, or without param every lane of the track's mixer or of the device",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            optional("param", ParamType::Text, "the parameter, as for automation.addPoint; left out for every lane"),
        ],
        kind: CommandKind::Edit,
        run: crate::automation::clear,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "automation.setEnabled",
        label: "Turn a parameter's automation off or on again; a manual change to an automated parameter turns it off, and the value set by hand holds until it is on again",
        params: &[
            TRACK,
            AUTOMATED_DEVICE,
            AUTOMATED,
            param("enabled", ParamType::Bool, "false turns the lane off, true on"),
        ],
        kind: CommandKind::Edit,
        run: crate::automation::set_enabled,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "automation.list",
        label: "List a track's automation: its mixer's lanes and each device's, with their points (beat, value, curve as [x1, y1, x2, y2]) and whether they play",
        params: &[TRACK],
        kind: CommandKind::Read,
        run: crate::automation::list,
        policy: Policy::DEFAULT,
    },
];
