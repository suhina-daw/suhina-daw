// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`LiveInput`]: what a session plays and records live, kept beside the project, not in it.

use suhina_engine::{ClickSound, Quantization};
use suhina_model::TrackId;

use crate::recording::Recording;

/// The armed tracks, the metronome, the takes being recorded and the launch grid.
pub(crate) struct LiveInput {
    /// The armed tracks, whose instruments play the MIDI inputs (ADR 0017). Not part of the
    /// project: arming is not undone and not saved.
    pub(crate) armed: Vec<TrackId>,
    pub(crate) targets: Vec<TrackId>,
    /// Takes being recorded and the count-in (`docs/spec/recording.md`).
    pub(crate) recording: Recording,
    /// The metronome: on, its level in dB, its sound. A new output gets it again.
    pub(crate) metronome: (bool, f64, ClickSound),
    /// The grid launches wait for, which the Arrangement view's grid follows too. Kept here,
    /// not in the project: it reaches a player when one is attached (`launcher.setQuantization`).
    pub(crate) quantization: Quantization,
}

impl Default for LiveInput {
    fn default() -> Self {
        Self {
            armed: Vec::new(),
            targets: Vec::new(),
            recording: Recording::default(),
            metronome: (false, -6.0, ClickSound::Beep),
            quantization: Quantization::Bars(1),
        }
    }
}
