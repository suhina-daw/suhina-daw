// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the note tools of a MIDI clip's panel act on and the settings they keep: the tools part
//! beside the Clip part, with its Notes and Transform tabs, draws them
//! (`detail_view::midi_panel`; `docs/spec/ui-clip-editor.md` › Note tools). Each tool is one
//! `note.*` command over the selection or the whole clip.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use suhina_engine::model::Note;

/// The tools part's width in a row, beside the Clip part.
pub const TOOLS_WIDTH: f32 = 136.0;

/// The tools part's width: `.midi-tools { width }` in the theme (120 to 480 points), else
/// [`TOOLS_WIDTH`].
pub fn tools_width() -> f32 {
    let _style = crate::kit::style_class::enter("midi-tools");
    crate::kit::style_class::computed().and_then(|c| c.width).map_or(TOOLS_WIDTH, |w| w.clamp(120.0, 480.0))
}

/// The tools part's tabs: one-click tools on the notes, or a transformation with its settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolsTab {
    #[default]
    Notes,
    Transform,
}

/// The transformations the Transform tab's list offers, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transform {
    Quantise,
    #[default]
    Humanise,
    RandomiseVelocity,
    RampVelocity,
}

impl Transform {
    pub const ALL: [Transform; 4] =
        [Transform::Quantise, Transform::Humanise, Transform::RandomiseVelocity, Transform::RampVelocity];

    /// Its name in the list.
    pub fn name(self) -> &'static str {
        match self {
            Transform::Quantise => "Quantise",
            Transform::Humanise => "Humanise",
            Transform::RandomiseVelocity => "Randomise velocity",
            Transform::RampVelocity => "Ramp velocity",
        }
    }

    /// The verb its Apply button starts with: "Humanise 3 notes", "Ramp clip".
    pub fn verb(self) -> &'static str {
        match self {
            Transform::Quantise => "Quantise",
            Transform::Humanise => "Humanise",
            Transform::RandomiseVelocity => "Randomise",
            Transform::RampVelocity => "Ramp",
        }
    }

    /// The line under the list that says what it does, short enough for the narrow part.
    pub fn hint(self) -> &'static str {
        match self {
            Transform::Quantise => "Starts toward the grid",
            Transform::Humanise => "Small shifts, like a player",
            Transform::RandomiseVelocity => "Velocities up or down",
            Transform::RampVelocity => "Velocities rise or fall",
        }
    }
}

/// What the tools act on: the clip's address for a command, its notes, and the selection (indices
/// into `notes`).
pub struct Tools<'a> {
    pub address: &'a Value,
    pub notes: &'a [Note],
    pub selected: &'a BTreeSet<usize>,
    /// The clip's length in beats.
    pub length: f64,
    /// A time selection with no note in it: the tools act on nothing, not on the whole clip.
    pub nothing: bool,
}

impl Tools<'_> {
    /// The params a tool's command takes: the clip's address and, with notes selected, their
    /// indices (`notes` left out means the whole clip, as the commands take it).
    pub(crate) fn params(&self) -> Value {
        let mut params = self.address.clone();
        if !self.selected.is_empty() {
            params["notes"] = json!(self.selected.iter().collect::<Vec<_>>());
        }
        params
    }

    /// Where Legato's last notes end: at the end of the stretch selected in time (`span_end`), else
    /// where the next note after them starts, on any key, else at the end of the clip.
    pub(crate) fn legato_until(&self, span_end: Option<f64>) -> f64 {
        if let Some(end) = span_end {
            return end;
        }
        let acts_on = |i: usize| self.selected.is_empty() || self.selected.contains(&i);
        let last = self
            .notes
            .iter()
            .enumerate()
            .filter(|(i, _)| acts_on(*i))
            .map(|(_, n)| n.time)
            .fold(f64::NEG_INFINITY, f64::max);
        self.notes.iter().map(|n| n.time).filter(|time| *time > last).fold(self.length, f64::min)
    }

    /// What the tools act on, as the chip in the tab row says it.
    pub(crate) fn head(&self) -> &'static str {
        match (self.selected.is_empty(), self.nothing) {
            (false, _) => "Selected notes",
            (true, true) => "No notes in time",
            (true, false) => "Whole clip",
        }
    }

    /// How many notes the tools act on.
    pub(crate) fn count(&self) -> usize {
        if self.nothing {
            0
        } else if self.selected.is_empty() {
            self.notes.len()
        } else {
            self.selected.len()
        }
    }

    /// The count beside the head: "4 notes", "1 note", "3 of 12 notes".
    pub(crate) fn count_text(&self) -> String {
        let count = self.count();
        let plural = if count == 1 { "note" } else { "notes" };
        if self.selected.is_empty() {
            format!("{count} {plural}")
        } else {
            format!("{count} of {} notes", self.notes.len())
        }
    }

    /// What an Apply button says it acts on: "3 notes", "1 note", or "clip".
    pub(crate) fn target(&self) -> String {
        match (self.selected.len(), self.nothing) {
            (_, true) => "nothing".into(),
            (0, false) => "clip".into(),
            (1, false) => "1 note".into(),
            (n, false) => format!("{n} notes"),
        }
    }
}

/// The tools' own settings, kept until Suhina closes, as quantise's are. Every value is clamped
/// where it is used, so a typed number never reaches a command out of range.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSettings {
    /// Semitones **Transpose** moves the notes by.
    pub transpose: i32,
    /// How far **Humanise** may move a start, in milliseconds (#1214).
    pub humanize_ms: f64,
    /// How far **Humanise** may move a velocity, in MIDI steps.
    pub humanize_velocity: f64,
    /// Whether **Humanise** moves notes that start together together.
    pub humanize_chords: bool,
    /// How far **Randomise velocity** may move a velocity, in MIDI steps.
    pub random_amount: f64,
    /// The velocity **Ramp velocity** starts from and ends on.
    pub ramp_from: i32,
    pub ramp_to: i32,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            transpose: 1,
            humanize_ms: 10.0,
            humanize_velocity: 8.0,
            humanize_chords: true,
            random_amount: 16.0,
            ramp_from: 20,
            ramp_to: 120,
        }
    }
}
