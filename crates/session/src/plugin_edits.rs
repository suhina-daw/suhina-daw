// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Changes made in a plug-in's own editor (issue #362). Reading a plug-in's state is neither a
//! change to the open project nor an undo step (ADR 0010, decision 7), but a project whose
//! plug-in was set differently in its editor has unsaved changes all the same: saving writes
//! what the plug-in reports now. So each state read is compared with the one the plug-in
//! reported when the project was last saved, or when it was first read.
//!
//! A plug-in may report slightly different bytes on every read (a counter, a random seed), so
//! one read that differs is not enough: a change counts once two reads in a row differ from
//! that state and agree with each other.

use std::collections::HashMap;

use suhina_plugins::SavedState;

/// What is known about one plug-in device's state.
#[derive(Default)]
struct Watched {
    /// The state it reported when the project was last saved, or when it was first read.
    baseline: Option<SavedState>,
    /// The last read, when it differed from the baseline; a second read the same counts.
    differing: Option<SavedState>,
    /// This differing candidate was already counted after two agreeing reads.
    confirmed: bool,
    /// A confirmed value away from the baseline has not returned to it yet.
    away: bool,
}

/// The plug-in devices' states as last saved, and whether one has changed since.
#[derive(Default)]
pub(crate) struct PluginEdits {
    watched: HashMap<u64, Watched>,
    changed: bool,
    revision: u64,
}

impl PluginEdits {
    /// A state just read from the plug-in device `key`.
    pub(crate) fn read(&mut self, key: u64, state: &SavedState) {
        let watched = self.watched.entry(key).or_default();
        let Some(baseline) = &watched.baseline else {
            watched.baseline = Some(state.clone());
            return;
        };
        if baseline == state {
            if watched.away {
                self.revision = self.revision.saturating_add(1);
            }
            watched.away = false;
            watched.confirmed = false;
            watched.differing = None;
        } else if watched.differing.as_ref() == Some(state) {
            if !watched.confirmed {
                self.revision = self.revision.saturating_add(1);
            }
            watched.confirmed = true;
            watched.away = true;
            self.changed = true;
        } else {
            watched.differing = Some(state.clone());
            watched.confirmed = false;
        }
    }

    /// The project was saved with `states`, each plug-in device's by key: they are what later
    /// reads are compared with, and nothing has changed yet.
    pub(crate) fn saved(&mut self, states: &HashMap<u64, SavedState>) {
        for (key, state) in states {
            self.watched.insert(*key, Watched { baseline: Some(state.clone()), ..Watched::default() });
        }
        for watched in self.watched.values_mut() {
            watched.differing = None;
            watched.confirmed = false;
            watched.away = false;
        }
        self.changed = false;
    }

    /// Confirmed state changes since start, including changes after an earlier backup.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether a plug-in's state has changed since the project was last saved or opened.
    pub(crate) fn changed(&self) -> bool {
        self.changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(byte: u8) -> SavedState {
        SavedState::from(vec![byte; 4])
    }

    #[test]
    fn the_first_read_is_what_later_ones_are_compared_with() {
        let mut edits = PluginEdits::default();
        edits.read(1, &state(1));
        edits.read(1, &state(1));
        edits.read(1, &state(1));
        assert!(!edits.changed());
    }

    #[test]
    fn revisions_count_later_confirmed_changes_but_not_repeated_identical_reads() {
        let mut edits = PluginEdits::default();
        edits.read(1, &state(0));
        edits.read(1, &state(1));
        assert_eq!(edits.revision(), 0);
        edits.read(1, &state(1));
        edits.read(1, &state(1));
        assert_eq!(edits.revision(), 1);
        edits.read(1, &state(2));
        edits.read(1, &state(2));
        assert_eq!(edits.revision(), 2);
        edits.read(1, &state(3));
        edits.read(1, &state(0));
        edits.read(1, &state(0));
        assert_eq!(edits.revision(), 3, "returning to the baseline also changes the backup");
        edits.saved(&HashMap::from([(1, state(0))]));
        edits.read(1, &state(0));
        assert_eq!(edits.revision(), 3);
        assert!(!edits.changed());
    }

    #[test]
    fn two_reads_in_a_row_that_agree_and_differ_are_a_change() {
        let mut edits = PluginEdits::default();
        edits.read(1, &state(1));
        edits.read(1, &state(2));
        assert!(!edits.changed(), "one read that differs is not enough");
        edits.read(1, &state(2));
        assert!(edits.changed());
        // Set back as it was in the editor: still unsaved, as an edit undone by hand.
        edits.read(1, &state(1));
        assert!(edits.changed());
    }

    #[test]
    fn a_state_that_differs_on_every_read_is_not_a_change() {
        let mut edits = PluginEdits::default();
        for n in 0..50 {
            edits.read(1, &state(n));
        }
        assert!(!edits.changed());
        assert_eq!(edits.revision(), 0);
    }

    #[test]
    fn a_read_like_the_baseline_in_between_starts_over() {
        let mut edits = PluginEdits::default();
        edits.read(1, &state(1));
        edits.read(1, &state(2));
        edits.read(1, &state(1));
        edits.read(1, &state(2));
        assert!(!edits.changed());
    }

    #[test]
    fn devices_are_compared_each_with_its_own_state() {
        let mut edits = PluginEdits::default();
        edits.read(1, &state(1));
        edits.read(2, &state(2));
        edits.read(1, &state(2));
        edits.read(2, &state(1));
        assert!(!edits.changed());
    }

    #[test]
    fn a_controller_state_counts_too() {
        let mut edits = PluginEdits::default();
        let with = |c: u8| SavedState { data: vec![1], controller: Some(vec![c]) };
        edits.read(1, &with(1));
        edits.read(1, &with(2));
        edits.read(1, &with(2));
        assert!(edits.changed());
    }

    #[test]
    fn saving_makes_the_saved_states_the_baseline() {
        let mut edits = PluginEdits::default();
        edits.read(1, &state(1));
        edits.read(1, &state(2));
        edits.read(1, &state(2));
        assert!(edits.changed());
        edits.saved(&HashMap::from([(1, state(2))]));
        assert!(!edits.changed());
        edits.read(1, &state(2));
        assert!(!edits.changed());
        // A read that differed before the save does not count with one after it.
        edits.read(1, &state(3));
        edits.saved(&HashMap::from([(1, state(2))]));
        edits.read(1, &state(3));
        assert!(!edits.changed());
        edits.read(1, &state(3));
        assert!(edits.changed());
    }
}
