// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`History`]: a session's undo and redo, and what tells a change from an unsaved one.

use suhina_model::{Project, TrackId};

/// The steps a session can go back and forward through, and how they were made.
#[derive(Default)]
pub(crate) struct History {
    /// Counts changes to the project (edits, undo, redo), and the count when it was last saved.
    pub(crate) changes: u64,
    pub(crate) saved_at: u64,
    pub(crate) undo: Vec<Project>,
    pub(crate) redo: Vec<Project>,
    /// The command and track of the undo step on top that a call with `continue` may join: one
    /// drag of a value is one step. Any other edit, undo or redo ends it.
    pub(crate) open_step: Option<(&'static str, TrackId)>,
    /// Parameter whose live drag has already recorded its single undo snapshot.
    pub(crate) param_gesture: Option<(TrackId, usize, String)>,
}
