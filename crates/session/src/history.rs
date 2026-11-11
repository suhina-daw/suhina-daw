// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! [`History`]: a session's undo and redo, and what tells a change from an unsaved one.

use suhina_model::Project;

/// The steps a session can go back and forward through, and how they were made.
#[derive(Default)]
pub(crate) struct History {
    /// Counts changes to the project (edits, undo, redo), and the count when it was last saved.
    pub(crate) changes: u64,
    pub(crate) saved_at: u64,
    pub(crate) undo: Vec<Project>,
    pub(crate) redo: Vec<Project>,
    /// How many undo steps were ever made (not undone or redone ones): a new step is told apart
    /// from one added to an open gesture, also once the oldest step goes at the limit.
    pub(crate) made: u64,
    /// The open gesture (#1146): the id a drag's caller picked, whose calls make the undo step
    /// on top. `history.endGesture`, any other edit, a call with another gesture, undo and redo
    /// end it.
    pub(crate) gesture: Option<Gesture>,
}

/// An open gesture.
pub(crate) struct Gesture {
    pub(crate) id: String,
    /// The gesture's last change, when a command whose calls replace each other made it.
    pub(crate) replacing: Option<Replacing>,
}

/// The last change of an open gesture, by a command whose calls replace each other
/// ([`crate::commands::Policy::gesture_replaces`]): the next call of the same command on the same
/// target starts from `base`, so it replaces the last; a call of anything else adds to the step
/// and leaves what the gesture changed before.
pub(crate) struct Replacing {
    pub(crate) command: &'static str,
    /// The call's parameters but the gesture and those it changes live.
    pub(crate) target: serde_json::Value,
    /// The project just before the first call of this run of calls.
    pub(crate) base: Project,
}
