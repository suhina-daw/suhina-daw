// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Local sample preparation. Only explicit acceptance changes the project.

use super::*;
use crate::instrument_analysis;

const JOB: Param = param("job", ParamType::Text, "the exact analysis job id returned by instrument.analyzeSample");
pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "instrument.fitSample",
        label: "Fit an experimental decaying modal profile from the loaded sample without changing playback",
        params: &[DEVICE_TRACK, DEVICE],
        kind: CommandKind::on(Target::Project, Effect::Change),
        run: instrument_analysis::start_fit,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "instrument.fitFolder",
        label: "Fit every one-note audio file in a folder into one fitted model (velocity layers by level) without changing playback",
        params: &[
            DEVICE_TRACK,
            DEVICE,
            param("path", ParamType::Path(PathKind::Folder), "folder of one-note audio files, at most 128"),
        ],
        kind: CommandKind::on(Target::Project, Effect::Change),
        run: instrument_analysis::start_folder,
        policy: Policy::UNHEARD,
    },
    CommandSpec {
        id: "instrument.acceptFit",
        label: "Adopt the current modal fit as a fitted model (its modes, attack and level), reset model controls and enable all fitted modes in one undo step",
        params: &[DEVICE_TRACK, DEVICE, JOB],
        kind: CommandKind::EDIT,
        run: instrument_analysis::accept_fit,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "instrument.addFit",
        label: "Add the current modal fit's recording to the Instrument's fitted model, fused with its other recordings by key and velocity, in one undo step",
        params: &[
            DEVICE_TRACK,
            DEVICE,
            JOB,
            optional(
                "velocity",
                ParamType::Number,
                "the velocity this recording stands for, above 0 and at most 1 (default 0.8)",
            ),
            optional(
                "source",
                ParamType::Integer,
                "which instrument the recording is from, 0 to 15 (default 0), for Source morph: ten pianos are ten sources",
            ),
        ],
        kind: CommandKind::EDIT,
        run: instrument_analysis::add_fit,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "instrument.auditionFit",
        label: "Play the fit's original source or its fitted model, level-matched, and report their level and spectral distance",
        params: &[
            DEVICE_TRACK,
            DEVICE,
            JOB,
            param("which", ParamType::Text, "original (the analyzed source range) or fitted (the model rendered)"),
        ],
        // Plays through the preview player when there is one, and reports the comparison either way.
        kind: CommandKind::on(Target::Project, Effect::Change),
        run: instrument_analysis::audition_fit,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "instrument.analyzeSample",
        label: "Analyze the loaded sample locally and propose its root note (one note), or the tonic of its key (a loop or phrase), without changing the instrument",
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
