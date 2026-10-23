// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `preview.*` commands; what runs them is in the modules they name.

use super::*;

pub(super) static COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "preview.play",
        label: "Preview a sample or a MIDI file once, at its own speed, after the main channel; a MIDI file plays through a track's instrument or the FM synth",
        params: &[
            param("path", ParamType::Path(PathKind::File), "the sample or MIDI file"),
            optional("from", ParamType::Number, "seconds into the file to start at (default 0)"),
            optional("track", ParamType::Track, "a MIDI file plays through this track's instrument when it has one"),
            optional("gain", ParamType::Number, "level in dB (default -6)"),
        ],
        kind: CommandKind::Transport,
        run: crate::preview::play,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "preview.setGain",
        label: "Change the level of the preview playing now, gliding there without a click; the next preview gets its own gain",
        params: &[param("gain", ParamType::Number, "level in dB, at most 12")],
        kind: CommandKind::Transport,
        run: crate::preview::set_gain,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "preview.stop",
        label: "Stop the preview",
        params: &[],
        kind: CommandKind::Transport,
        run: crate::preview::stop,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "preview.noteOn",
        label: "Play a note, or a chord, through a track's instrument and hold it until preview.noteOff, as the piano roll's key column does",
        params: &[
            TRACK,
            optional("key", ParamType::Integer, "the key to play, 0 to 127 (60 is C3)"),
            optional("keys", ParamType::Integers, "instead: several keys to play together, 1 to 128 of them"),
            optional("velocity", ParamType::Number, "how hard, 1 to 127 (default 100)"),
        ],
        kind: CommandKind::Transport,
        run: crate::preview::note_on,
        policy: Policy::DEFAULT,
    },
    CommandSpec {
        id: "preview.noteOff",
        label: "Let go of the note preview.noteOn started on a track, or of every held preview note",
        params: &[TRACK, optional("key", ParamType::Integer, "the key to let go of; every held key when left out")],
        kind: CommandKind::Transport,
        run: crate::preview::note_off,
        policy: Policy::DEFAULT,
    },
];
