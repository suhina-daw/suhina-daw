// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's own grid value (› Grid and snapping in `docs/spec/ui-clip-editor.md`):
//! whether things snap to it, an adaptive width whose lines follow the zoom or a fixed value from
//! 8 bars to 1/32, and triplets. The toolbar's grid menu and the keys Ctrl+1 to Ctrl+4 change it;
//! the window keeps it. The lines and snapping it gives are the shared timeline grid's
//! ([`crate::interaction::timeline_grid`]).

use crate::interaction::timeline_grid;

/// How far apart an adaptive grid's lines are at least.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    Widest,
    Wide,
    Medium,
    Narrow,
    Narrowest,
}

impl Width {
    /// From the widest to the narrowest, as the menu lists them.
    pub const ALL: [Self; 5] = [Self::Widest, Self::Wide, Self::Medium, Self::Narrow, Self::Narrowest];

    /// The name the menu shows.
    pub fn name(self) -> &'static str {
        match self {
            Self::Widest => "Widest",
            Self::Wide => "Wide",
            Self::Medium => "Medium",
            Self::Narrow => "Narrow",
            Self::Narrowest => "Narrowest",
        }
    }

    /// The least distance between two lines, in points.
    pub fn spacing(self) -> f64 {
        match self {
            Self::Widest => 96.0,
            Self::Wide => 48.0,
            Self::Medium => 24.0,
            Self::Narrow => 12.0,
            Self::Narrowest => 6.0,
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|w| *w == self).unwrap_or(0)
    }

    fn at(index: usize) -> Self {
        Self::ALL.get(index).copied().unwrap_or(Self::Narrowest)
    }
}

/// The fixed grid values, from the coarsest to the finest, as the menu lists them.
pub const FIXED_VALUES: [&str; 9] = ["8 bars", "4 bars", "2 bars", "1 bar", "1/2", "1/4", "1/8", "1/16", "1/32"];

/// The index of 1/16 in [`FIXED_VALUES`]: the default.
const SIXTEENTH: usize = 7;

/// The first note value (not a bar count) in [`FIXED_VALUES`].
const FIRST_NOTE: usize = 4;

/// The control bar's names for the triplet note values 1/2 to 1/16 (it has no 1/32 triplet).
const TRIPLET_VALUES: [&str; 4] = ["1/2T", "1/4T", "1/8T", "1/16T"];

/// The finest fixed value, in beats (1/32).
const FINEST: f64 = 0.125;

/// Whether the lines follow the zoom or stay one value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridMode {
    Adaptive(Width),
    /// An index into [`FIXED_VALUES`].
    Fixed(usize),
}

/// The piano roll's grid value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PianoGrid {
    /// Notes snap to the lines; off, the lines are still drawn and nothing snaps.
    pub snap: bool,
    pub mode: GridMode,
    /// Note values are triplets: two thirds as long. Bar values are left as they are.
    pub triplet: bool,
}

impl Default for PianoGrid {
    /// Fixed 1/16, snapping, no triplets.
    fn default() -> Self {
        Self { snap: true, mode: GridMode::Fixed(SIXTEENTH), triplet: false }
    }
}

impl PianoGrid {
    /// The grid for a control bar value such as `1/8T`, `1 bar` or `none` (snap off at 1/16);
    /// an unknown value is the default.
    pub fn from_value(value: &str) -> Self {
        let normal = value.trim().to_ascii_lowercase();
        if normal == "none" || normal == "off" {
            return Self { snap: false, ..Self::default() };
        }
        let (name, triplet) = match normal.strip_suffix('t') {
            Some(name) => (name, true),
            None => (normal.as_str(), false),
        };
        match FIXED_VALUES.iter().position(|v| *v == name) {
            Some(index) => Self { snap: true, mode: GridMode::Fixed(index), triplet: triplet && index >= FIRST_NOTE },
            None => Self::default(),
        }
    }

    /// The control bar's value for this grid: `none` when snap is off, else the fixed value.
    /// `None` when the control bar has no such value (an adaptive grid, a 1/32 triplet).
    pub fn control_bar_value(&self) -> Option<&'static str> {
        if !self.snap {
            return Some("none");
        }
        let GridMode::Fixed(index) = self.mode else {
            return None;
        };
        if self.triplet && index >= FIRST_NOTE {
            return TRIPLET_VALUES.get(index - FIRST_NOTE).copied();
        }
        FIXED_VALUES.get(index).copied()
    }

    /// Whether the lines follow the zoom.
    pub fn is_adaptive(&self) -> bool {
        matches!(self.mode, GridMode::Adaptive(_))
    }

    /// A fixed value's step in beats, for `bar_beats` beats in a bar; 0 for an adaptive grid.
    pub fn fixed_step(&self, bar_beats: f64) -> f64 {
        match self.mode {
            GridMode::Fixed(index) => fixed_beats(index, bar_beats, self.triplet),
            GridMode::Adaptive(_) => 0.0,
        }
    }

    /// The step in beats at `beat_width` points per beat: a fixed value's, or the finest power
    /// of two note value (1/32 to 1/2; then 1 bar, 2 bars, 4 bars and on) whose lines are at
    /// least the width's spacing apart. An unusable zoom (not a positive number) gives one bar.
    pub fn step(&self, beat_width: f32, bar_beats: f64) -> f64 {
        match self.mode {
            GridMode::Fixed(_) => self.fixed_step(bar_beats),
            GridMode::Adaptive(width) => {
                timeline_grid::adaptive_step(width.spacing(), self.triplet, beat_width, bar_beats)
            }
        }
    }

    /// One value finer (Ctrl+1): the next fixed value, or the next narrower width; the finest
    /// stays.
    pub fn narrower(self) -> Self {
        let mode = match self.mode {
            GridMode::Fixed(index) => GridMode::Fixed((index + 1).min(FIXED_VALUES.len() - 1)),
            GridMode::Adaptive(width) => GridMode::Adaptive(Width::at(width.index() + 1)),
        };
        Self { mode, ..self }
    }

    /// One value coarser (Ctrl+2): the previous fixed value, or the next wider width; the
    /// coarsest stays.
    pub fn wider(self) -> Self {
        let mode = match self.mode {
            GridMode::Fixed(index) => GridMode::Fixed(index.saturating_sub(1)),
            GridMode::Adaptive(width) => GridMode::Adaptive(Width::at(width.index().saturating_sub(1))),
        };
        Self { mode, ..self }
    }

    /// Triplets on or off (Ctrl+3).
    pub fn toggle_triplet(self) -> Self {
        Self { triplet: !self.triplet, ..self }
    }

    /// Snapping on or off (Ctrl+4).
    pub fn toggle_snap(self) -> Self {
        Self { snap: !self.snap, ..self }
    }

    /// The text of the toolbar's grid button: `1/16`, `1/8T`, `2 bars`, `Adaptive`, `Adaptive T`,
    /// with ` off` after it while snap is off.
    pub fn label(&self) -> String {
        let value = match self.mode {
            GridMode::Fixed(index) => {
                let name = FIXED_VALUES.get(index).copied().unwrap_or("1/16");
                if self.triplet && index >= FIRST_NOTE { format!("{name}T") } else { name.to_string() }
            }
            GridMode::Adaptive(_) => {
                if self.triplet {
                    "Adaptive T".to_string()
                } else {
                    "Adaptive".to_string()
                }
            }
        };
        if self.snap { value } else { format!("{value} off") }
    }
}

/// Fixed value `index` in beats.
fn fixed_beats(index: usize, bar_beats: f64, triplet: bool) -> f64 {
    let bar = timeline_grid::bar_length(bar_beats);
    let third = if triplet { 2.0 / 3.0 } else { 1.0 };
    match index {
        0 => 8.0 * bar,
        1 => 4.0 * bar,
        2 => 2.0 * bar,
        3 => bar,
        4 => 2.0 * third,
        5 => third,
        6 => 0.5 * third,
        7 => 0.25 * third,
        _ => FINEST * third,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adaptive(width: Width) -> PianoGrid {
        PianoGrid { snap: true, mode: GridMode::Adaptive(width), triplet: false }
    }

    #[test]
    fn the_default_is_a_snapping_sixteenth() {
        let grid = PianoGrid::default();
        assert_eq!(grid.step(40.0, 4.0), 0.25);
        assert_eq!(grid.step(400.0, 4.0), 0.25, "a fixed value ignores the zoom");
        assert_eq!(grid.label(), "1/16");
        assert_eq!(grid.control_bar_value(), Some("1/16"));
    }

    #[test]
    fn an_adaptive_grid_gets_finer_as_the_view_zooms_in() {
        let grid = adaptive(Width::Medium);
        // 24 points apart at least.
        assert_eq!(grid.step(10.0, 4.0), 4.0, "a bar at 10 points per beat");
        assert_eq!(grid.step(40.0, 4.0), 1.0, "a quarter at 40");
        assert_eq!(grid.step(100.0, 4.0), 0.25, "a sixteenth at 100");
        assert_eq!(grid.step(1000.0, 4.0), 0.125, "never finer than 1/32");
        assert_eq!(grid.step(1.0, 4.0), 32.0, "bars double when zoomed far out");
        // A narrower width is finer at the same zoom.
        assert!(adaptive(Width::Narrowest).step(40.0, 4.0) < adaptive(Width::Widest).step(40.0, 4.0));
        let steps: Vec<f64> = Width::ALL.iter().map(|w| adaptive(*w).step(40.0, 4.0)).collect();
        assert_eq!(steps, vec![4.0, 2.0, 1.0, 0.5, 0.25]);
    }

    #[test]
    fn an_adaptive_grid_follows_the_bar_length_and_triplets() {
        // In 3/4 the step after the half note is the 3-beat bar.
        assert_eq!(adaptive(Width::Medium).step(10.0, 3.0), 3.0);
        assert_eq!(adaptive(Width::Medium).step(12.0, 3.0), 2.0);
        let triplet = adaptive(Width::Medium).toggle_triplet();
        assert!((triplet.step(100.0, 4.0) - 1.0 / 3.0).abs() < 1e-12, "an eighth triplet at 100");
        assert_eq!(triplet.label(), "Adaptive T");
    }

    #[test]
    fn unusable_zooms_never_panic_or_loop() {
        for width in [f32::NAN, 0.0, -5.0, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(adaptive(Width::Narrowest).step(width, 4.0), 4.0, "{width}");
        }
        assert_eq!(adaptive(Width::Widest).step(f32::MIN_POSITIVE, 4.0), 4.0 * 2f64.powi(16));
        assert_eq!(adaptive(Width::Medium).step(40.0, f64::NAN), 1.0, "a broken bar length is 4 beats");
        assert_eq!(PianoGrid::default().step(f32::NAN, -1.0), 0.25);
        assert_eq!(PianoGrid { mode: GridMode::Fixed(99), ..PianoGrid::default() }.step(40.0, 4.0), 0.125);
    }

    #[test]
    fn narrower_and_wider_step_through_the_values_and_stop_at_the_ends() {
        let grid = PianoGrid::default();
        assert_eq!(grid.narrower().label(), "1/32");
        assert_eq!(grid.narrower().narrower().label(), "1/32");
        assert_eq!(grid.wider().label(), "1/8");
        let mut coarse = grid;
        for _ in 0..20 {
            coarse = coarse.wider();
        }
        assert_eq!(coarse.label(), "8 bars");
        assert_eq!(coarse.step(40.0, 4.0), 32.0);
        assert_eq!(adaptive(Width::Medium).narrower().mode, GridMode::Adaptive(Width::Narrow));
        assert_eq!(adaptive(Width::Narrowest).narrower().mode, GridMode::Adaptive(Width::Narrowest));
        assert_eq!(adaptive(Width::Widest).wider().mode, GridMode::Adaptive(Width::Widest));
    }

    #[test]
    fn triplets_and_snap_toggle_and_show_in_the_label() {
        let eighth = PianoGrid::from_value("1/8");
        let triplet = eighth.toggle_triplet();
        assert!((triplet.step(40.0, 4.0) - 1.0 / 3.0).abs() < 1e-12);
        assert_eq!(triplet.label(), "1/8T");
        assert_eq!(triplet.toggle_triplet(), eighth);
        let free = eighth.toggle_snap();
        assert!(!free.snap);
        assert_eq!(free.label(), "1/8 off");
        assert_eq!(free.step(40.0, 4.0), 0.5, "the lines stay");
        // Bars are not made triplets.
        let bar = PianoGrid::from_value("1 bar").toggle_triplet();
        assert_eq!((bar.label(), bar.step(40.0, 4.0)), ("1 bar".to_string(), 4.0));
    }

    #[test]
    fn control_bar_values_round_trip_where_the_control_bar_has_them() {
        for value in ["8 bars", "4 bars", "2 bars", "1 bar", "1/2", "1/2T", "1/4", "1/4T", "1/8", "1/8T", "1/16"] {
            assert_eq!(PianoGrid::from_value(value).control_bar_value(), Some(value), "{value}");
        }
        assert_eq!(PianoGrid::from_value("1/16T").control_bar_value(), Some("1/16T"));
        assert_eq!(PianoGrid::from_value("none").control_bar_value(), Some("none"));
        assert_eq!(PianoGrid::from_value("1/32").toggle_triplet().control_bar_value(), None);
        assert_eq!(adaptive(Width::Wide).control_bar_value(), None);
        assert_eq!(PianoGrid::from_value("nonsense"), PianoGrid::default());
        assert_eq!(PianoGrid::from_value(""), PianoGrid::default());
    }
}
