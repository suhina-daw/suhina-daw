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
    /// The open gesture (#1146): the id a drag's caller picked, whose calls make the undo step
    /// on top. `history.endGesture`, any other edit, a call with another gesture, undo and redo
    /// end it.
    pub(crate) gesture: Option<String>,
}
