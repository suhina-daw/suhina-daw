// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`ControlError`]: why the engine's control side refused something (#764 › 12), where it
//! returned text. Its `Display` is the text it returned, and it converts into a `String` for the
//! callers that pass the reason on.

use std::fmt;

/// Why a [`crate::Controller`] or a [`crate::Plugins`] call did not go through.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlError {
    /// The audio thread has not taken what was sent before: `what` (edits, parameter changes,
    /// plug-ins, the preview, commands). The engine is behind or not running.
    Full(&'static str),
    /// The playing graph cannot take this as a message; the edit needs a new graph (`what`: the
    /// track's volume, the send, the track's pan).
    NeedsGraph(&'static str),
    /// The project could not be built into a graph.
    Build(crate::RenderError),
    /// The stretch thread is not taking a new graph's stretched clips.
    StretchBusy,
    /// A running plug-in could not follow (a new sample rate it did not take in time).
    Plugin(String),
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full("commands") => write!(f, "the audio engine is not taking commands (queue full)"),
            Self::Full(what) => write!(f, "the audio engine has not taken the previous {what} yet"),
            Self::NeedsGraph(what) => write!(f, "{what} takes a new graph"),
            Self::StretchBusy => write!(f, "the stretch worker thread is not taking new work"),
            Self::Build(e) => write!(f, "{e}"),
            Self::Plugin(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for ControlError {}

impl From<ControlError> for String {
    fn from(e: ControlError) -> Self {
        e.to_string()
    }
}

impl From<crate::RenderError> for ControlError {
    fn from(e: crate::RenderError) -> Self {
        Self::Build(e)
    }
}
