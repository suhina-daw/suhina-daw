// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Local sample preparation. Only explicit acceptance changes the project.

use super::*;
use crate::instrument_analysis;

const JOB: Param = param("job", ParamType::Text, "the exact analysis job id returned by instrument.analyzeSample");
pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "instrument.analyzeSample",
        label: "Analyze the loaded sample locally and propose its root note without changing the instrument",
        params: &[DEVICE_TRACK, DEVICE],
        kind: CommandKind::on(Target::Project, Effect::Change),
        run: instrument_analysis::start,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "instrument.analysis",
        label: "Read the sample analysis status, measured pitch and explicit root-note proposal",
        params: &[DEVICE_TRACK, DEVICE],
        kind: CommandKind::READ,
        run: instrument_analysis::status,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "instrument.cancelAnalysis",
        label: "Cancel a sample analysis without waiting or changing the playable instrument",
        params: &[DEVICE_TRACK, DEVICE, JOB],
        kind: CommandKind::on(Target::Project, Effect::Change),
        run: instrument_analysis::cancel,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "instrument.acceptAnalysis",
        label: "Accept a current sample root-note proposal, optionally corrected, as one undoable manual edit",
        params: &[
            DEVICE_TRACK,
            DEVICE,
            JOB,
            optional("root", ParamType::Integer, "corrected integer MIDI root key, 0..127; omitted uses the proposal"),
        ],
        kind: CommandKind::EDIT,
        run: instrument_analysis::accept,
        policy: Policy::DEFAULT,
    },
];
