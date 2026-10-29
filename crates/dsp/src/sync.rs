// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tempo sync for LFOs: how a rate mode turns a note length into a cycle length in beats, and an
//! [`LfoClock`] that turns that length into cycles per frame and, while the song plays, into a
//! position locked to the song.
//!
//! Each device keeps its own table of note lengths (in beats, in bars or as `1/n`); what they share
//! is the arithmetic.

/// How an LFO's speed is set, in the order the devices list the modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateMode {
    /// A rate in hertz.
    Rate,
    /// A cycle length in time.
    Time,
    /// A note length.
    Synced,
    /// Two thirds of a note length.
    Triplet,
    /// One and a half note lengths.
    Dotted,
    /// A number of sixteenth notes.
    Sixteenths,
}

impl RateMode {
    /// The modes in order.
    pub const ALL: [Self; 6] = [Self::Rate, Self::Time, Self::Synced, Self::Triplet, Self::Dotted, Self::Sixteenths];

    /// The modes' names, for a choice parameter.
    pub const NAMES: [&'static str; 6] = ["Rate", "Time", "Synced", "Triplet", "Dotted", "Sixteenths"];

    /// The mode at a choice parameter's value, rounded and clamped to the list (`Rate` for NaN).
    pub fn from_value(value: f64) -> Self {
        let index = if value.is_nan() { 0 } else { value.round().clamp(0.0, 5.0) as usize };
        Self::ALL.get(index).copied().unwrap_or(Self::Rate)
    }

    /// One cycle's length in beats, from the synced note length `note_beats` and the number of
    /// `sixteenths`; `None` for a mode that runs free of the tempo.
    pub fn cycle_beats(self, note_beats: f64, sixteenths: f64) -> Option<f64> {
        match self {
            Self::Rate | Self::Time => None,
            Self::Synced => Some(note_beats),
            Self::Triplet => Some(note_beats * 2.0 / 3.0),
            Self::Dotted => Some(note_beats * 1.5),
            Self::Sixteenths => Some(sixteenths * 0.25),
        }
    }
}

/// Beats that pass in one frame at `tempo` (beats per minute) and `sample_rate`.
#[inline]
pub fn beats_per_frame(tempo: f64, sample_rate: f64) -> f64 {
    tempo / 60.0 / sample_rate
}

/// Shortest cycle a synced clock divides by, in beats, so a zero length cannot divide by zero.
const SHORTEST_CYCLE: f64 = 1e-6;

/// An LFO's clock for one block: a cycle synced to the tempo or a free rate in hertz, and the
/// song position the synced cycle locks to while the song plays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LfoClock {
    beats_per_frame: f64,
    cycle_beats: Option<f64>,
    hertz: f64,
    sample_rate: f64,
    song_beat: Option<f64>,
}

impl LfoClock {
    /// A clock at `tempo` and `sample_rate` whose cycle is `cycle_beats` long when synced, or runs
    /// at `hertz` when that is `None`.
    pub fn new(tempo: f64, sample_rate: f64, cycle_beats: Option<f64>, hertz: f64) -> Self {
        Self {
            beats_per_frame: beats_per_frame(tempo, sample_rate),
            cycle_beats: cycle_beats.map(|beats| beats.max(SHORTEST_CYCLE)),
            hertz,
            sample_rate,
            song_beat: None,
        }
    }

    /// The clock locked to the song from `beat`, the song position at the block's first frame,
    /// when it is synced, the song is `playing` and the position is a number.
    pub fn following(mut self, playing: bool, beat: f64) -> Self {
        self.song_beat = (self.cycle_beats.is_some() && playing && beat.is_finite()).then_some(beat);
        self
    }

    /// Whether the clock is locked to the song in this block.
    pub fn follows_song(&self) -> bool {
        self.song_beat.is_some()
    }

    /// Cycles that pass in one frame.
    pub fn cycles_per_frame(&self) -> f64 {
        match self.cycle_beats {
            Some(beats) => self.beats_per_frame / beats,
            None => self.hertz / self.sample_rate,
        }
    }

    /// The position in cycles at `frame` of the block, from the song position; `None` when the
    /// clock does not follow the song, and the LFO keeps its own phase.
    #[inline]
    pub fn song_cycles(&self, frame: usize) -> Option<f64> {
        let beat = self.song_beat?;
        let beats = self.cycle_beats?;
        Some((beat + frame as f64 * self.beats_per_frame) / beats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_modes_give_the_note_length_and_its_triplet_dotted_and_sixteenths() {
        assert_eq!(RateMode::Rate.cycle_beats(1.0, 4.0), None);
        assert_eq!(RateMode::Time.cycle_beats(1.0, 4.0), None);
        assert_eq!(RateMode::Synced.cycle_beats(1.0, 4.0), Some(1.0));
        assert_eq!(RateMode::Triplet.cycle_beats(1.5, 4.0), Some(1.0));
        assert_eq!(RateMode::Dotted.cycle_beats(1.0, 4.0), Some(1.5));
        assert_eq!(RateMode::Sixteenths.cycle_beats(1.0, 4.0), Some(1.0));
    }

    #[test]
    fn a_mode_value_is_rounded_and_clamped() {
        assert_eq!(RateMode::from_value(2.4), RateMode::Synced);
        assert_eq!(RateMode::from_value(9.0), RateMode::Sixteenths);
        assert_eq!(RateMode::from_value(-1.0), RateMode::Rate);
        assert_eq!(RateMode::from_value(f64::NAN), RateMode::Rate);
        assert_eq!(RateMode::from_value(f64::INFINITY), RateMode::Sixteenths);
        assert_eq!(RateMode::NAMES.len(), RateMode::ALL.len());
    }

    #[test]
    fn a_synced_clock_runs_one_cycle_per_note_and_locks_to_the_song() {
        // 120 BPM at 48 kHz: a beat is 24 000 frames.
        let clock = LfoClock::new(120.0, 48_000.0, Some(1.0), 5.0);
        assert_eq!(clock.cycles_per_frame(), 1.0 / 24_000.0);
        assert!(!clock.follows_song());
        assert_eq!(clock.song_cycles(0), None);

        let playing = clock.following(true, 3.0);
        assert!(playing.follows_song());
        assert_eq!(playing.song_cycles(0), Some(3.0));
        assert_eq!(playing.song_cycles(12_000), Some(3.5));
        assert!(!clock.following(false, 3.0).follows_song());
        assert!(!clock.following(true, f64::NAN).follows_song());
    }

    #[test]
    fn a_free_clock_runs_at_its_rate_and_never_follows_the_song() {
        let clock = LfoClock::new(120.0, 48_000.0, None, 4.8).following(true, 3.0);
        assert!((clock.cycles_per_frame() - 1e-4).abs() < 1e-18);
        assert!(!clock.follows_song());
    }

    #[test]
    fn a_zero_cycle_does_not_divide_by_zero() {
        let clock = LfoClock::new(120.0, 48_000.0, Some(0.0), 0.0).following(true, 1.0);
        assert!(clock.cycles_per_frame().is_finite());
        assert!(clock.song_cycles(10).is_some_and(f64::is_finite));
    }
}
