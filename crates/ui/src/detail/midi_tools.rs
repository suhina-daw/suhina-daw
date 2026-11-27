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
pub const TOOLS_WIDTH: f32 = 128.0;

/// The tools part's whole width: `midi-tools { width }` in the theme (120 to 480 points), else
/// [`TOOLS_WIDTH`] and the part's border and padding round it (#1336).
pub fn tools_width() -> f32 {
    let el = crate::kit::element::MIDI_TOOLS.enter();
    let [_, right, _, left] = el.insets();
    el.style().and_then(|c| c.width).map_or(TOOLS_WIDTH + left + right, |w| w.clamp(120.0, 480.0))
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
    /// An LFO written into the notes' pitch, slide or pressure curves (› MPE LFO).
    MpeLfo,
}

impl Transform {
    pub const ALL: [Transform; 5] = [
        Transform::Quantise,
        Transform::Humanise,
        Transform::RandomiseVelocity,
        Transform::RampVelocity,
        Transform::MpeLfo,
    ];

    /// Its name in the list.
    pub fn name(self) -> &'static str {
        match self {
            Transform::Quantise => "Quantise",
            Transform::Humanise => "Humanise",
            Transform::RandomiseVelocity => "Randomise velocity",
            Transform::RampVelocity => "Ramp velocity",
            Transform::MpeLfo => "MPE LFO",
        }
    }

    /// The verb its Apply button starts with: "Humanise 3 notes", "Ramp clip".
    pub fn verb(self) -> &'static str {
        match self {
            Transform::Quantise => "Quantise",
            Transform::Humanise => "Humanise",
            Transform::RandomiseVelocity => "Randomise",
            Transform::RampVelocity => "Ramp",
            Transform::MpeLfo => "LFO",
        }
    }

    /// The line under the list that says what it does, short enough for the narrow part.
    pub fn hint(self) -> &'static str {
        match self {
            Transform::Quantise => "Starts toward the grid",
            Transform::Humanise => "Small shifts, like a player",
            Transform::RandomiseVelocity => "Velocities up or down",
            Transform::RampVelocity => "Velocities rise or fall",
            Transform::MpeLfo => "A wave in pitch, slide or pressure",
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
    /// Beats in a bar, which the MPE LFO's rate is a part of.
    pub bar_beats: f64,
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

    /// Where Legato's last notes end: at the end of the stretch selected in time (`span_end`, given
    /// only when no note is selected), else where the next note after them starts, on any key,
    /// else at the end of the clip.
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
            (false, _) => "Selection",
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

    /// The count beside the head: "4 notes", "1 note", or "3 of 12" for a selection.
    pub(crate) fn count_text(&self) -> String {
        let count = self.count();
        let plural = if count == 1 { "note" } else { "notes" };
        if self.selected.is_empty() { format!("{count} {plural}") } else { format!("{count} of {}", self.notes.len()) }
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
    /// Semitones **Transpose** moves the notes by: 0 until one is chosen, which moves nothing.
    pub transpose: i32,
    /// How far **Humanise** may move a start, in milliseconds (#1214).
    pub humanize_ms: f64,
    /// How far **Humanise** may move a velocity, in MIDI steps.
    pub humanize_velocity: f64,
    /// How far **Randomise velocity** may move a velocity, in MIDI steps.
    pub random_amount: f64,
    /// The velocity **Ramp velocity** starts from and ends on.
    pub ramp_from: i32,
    pub ramp_to: i32,
    /// **MPE LFO**: the target (0 pitch, 1 slide, 2 pressure), the wave (0 sine, 1 square,
    /// 2 triangle, 3 random), the shape and the time shift in percent, the rate (an index into
    /// [`LFO_RATES`]), the amount and base (semitones for pitch, 0 to 127 otherwise), attack
    /// and decay in percent of the note, and the random wave's seed.
    pub lfo_target: usize,
    pub lfo_wave: usize,
    pub lfo_shape: f64,
    pub lfo_rate: usize,
    pub lfo_shift: f64,
    pub lfo_amount: f64,
    pub lfo_base: f64,
    pub lfo_attack: f64,
    pub lfo_decay: f64,
    pub lfo_seed: u64,
    /// Add swings the wave around the curve as it is; off (Replace) around the base.
    pub lfo_add: bool,
}

/// The MPE LFO's rates: a cycle per part of a bar, each also dotted (`.`) and triplet (`T`), and
/// the part of a bar each is.
pub const LFO_RATES: [(&str, f64); 24] = [
    ("1", 1.0),
    ("1.", 1.5),
    ("1T", 2.0 / 3.0),
    ("1/2", 0.5),
    ("1/2.", 0.75),
    ("1/2T", 1.0 / 3.0),
    ("1/4", 0.25),
    ("1/4.", 0.375),
    ("1/4T", 1.0 / 6.0),
    ("1/8", 0.125),
    ("1/8.", 0.1875),
    ("1/8T", 1.0 / 12.0),
    ("1/16", 0.0625),
    ("1/16.", 0.09375),
    ("1/16T", 1.0 / 24.0),
    ("1/32", 0.03125),
    ("1/32.", 0.046875),
    ("1/32T", 1.0 / 48.0),
    ("1/64", 0.015625),
    ("1/64.", 0.0234375),
    ("1/64T", 1.0 / 96.0),
    ("1/128", 0.0078125),
    ("1/128.", 0.01171875),
    ("1/128T", 1.0 / 192.0),
];

/// The MPE LFO's targets, as the command names them and as the list shows them.
pub const LFO_TARGETS: [(&str, &str); 3] = [("pitch", "Pitch"), ("timbre", "Slide"), ("pressure", "Pressure")];
/// The MPE LFO's waves.
pub const LFO_WAVES: [(&str, &str); 4] =
    [("sine", "Sine"), ("square", "Square"), ("triangle", "Triangle"), ("random", "Random")];

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            transpose: 0,
            humanize_ms: 10.0,
            humanize_velocity: 8.0,
            random_amount: 16.0,
            ramp_from: 20,
            ramp_to: 120,
            lfo_target: 0,
            lfo_wave: 0,
            lfo_shape: 0.0,
            lfo_rate: 6,
            lfo_shift: 0.0,
            lfo_amount: 1.0,
            lfo_base: 0.0,
            lfo_attack: 0.0,
            lfo_decay: 0.0,
            lfo_seed: 1,
            lfo_add: false,
        }
    }
}

impl ToolSettings {
    /// The MPE LFO these settings make, the curve it writes and whether it adds to the curve, for
    /// a bar `bar_beats` long: what `note.lfoExpression` is given and what the piano roll
    /// previews.
    pub fn lfo(&self, bar_beats: f64) -> (suhina_engine::model::ExpressionKind, suhina_session::Lfo, bool) {
        use suhina_engine::model::ExpressionKind;
        let kind = match self.lfo_target {
            1 => ExpressionKind::Timbre,
            2 => ExpressionKind::Pressure,
            _ => ExpressionKind::Pitch,
        };
        let pitch = kind == ExpressionKind::Pitch;
        let part = LFO_RATES.get(self.lfo_rate).map_or(0.25, |(_, part)| *part);
        let bar = if bar_beats.is_finite() && bar_beats > 0.0 { bar_beats } else { 4.0 };
        let scale = if pitch { 1.0 } else { 1.0 / 127.0 };
        let wave = LFO_WAVES
            .get(self.lfo_wave)
            .and_then(|(name, _)| suhina_session::LfoWave::from_name(name))
            .unwrap_or(suhina_session::LfoWave::Sine);
        let attack = (self.lfo_attack / 100.0).clamp(0.0, 1.0);
        let lfo = suhina_session::Lfo {
            wave,
            shape: (self.lfo_shape / 100.0).clamp(-1.0, 1.0),
            period: (part * bar).clamp(1.0 / 32.0, 64.0),
            shift: (self.lfo_shift / 100.0).clamp(-1.0, 1.0),
            amount: if pitch { self.lfo_amount.clamp(-96.0, 96.0) } else { (self.lfo_amount * scale).clamp(-1.0, 1.0) },
            base: if pitch { self.lfo_base.clamp(-96.0, 96.0) } else { (self.lfo_base * scale).clamp(0.0, 1.0) },
            attack,
            decay: (self.lfo_decay / 100.0).clamp(0.0, 1.0 - attack),
            seed: self.lfo_seed,
        };
        (kind, lfo, self.lfo_add)
    }
}
