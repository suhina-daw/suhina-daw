// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Swing (`docs/spec/swing.md`, #1221): the project's swing amount, and a MIDI clip's own, which
//! it plays instead of the project's when it has one. Swing delays every second sixteenth of each
//! eighth when the clip plays; the notes themselves never move.

/// The range of a swing amount: 0 is straight, 1 puts the second sixteenth of each eighth on the
/// eighth-note triplet.
pub const SWING_RANGE: std::ops::RangeInclusive<f64> = 0.0..=1.0;

/// How a MIDI clip swings: the project's swing (the default), or its own `amount`. The clip keeps
/// its own amount while it follows the project's, so going back to its own brings that amount back.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ClipSwing {
    /// The clip's own amount, 0 to 1.
    pub amount: f64,
    /// Whether the clip plays its own amount instead of the project's.
    pub own: bool,
}

impl ClipSwing {
    /// The amount the clip plays with when the project's swing is `global`.
    pub fn effective(self, global: f64) -> f64 {
        let amount = if self.own { self.amount } else { global };
        if amount.is_finite() { amount.clamp(*SWING_RANGE.start(), *SWING_RANGE.end()) } else { 0.0 }
    }

    /// Whether it is the starting one: following the project's swing, its own amount 0.
    pub fn is_default(self) -> bool {
        self == Self::default()
    }
}

/// A beat of a clip's content as it plays with `amount` of swing: within each eighth (half a
/// beat), the time up to its second sixteenth stretches and the time after it shrinks, so a note
/// on the second sixteenth lands `amount` of the way to the eighth-note triplet (a twelfth of a
/// beat later at 1), and the eighths themselves stay where they are. Every beat moves forward or
/// stays, never past the next eighth, so the order of notes never changes.
pub fn swung(beat: f64, amount: f64) -> f64 {
    if !(beat.is_finite() && amount.is_finite()) || amount <= 0.0 {
        return beat;
    }
    let amount = amount.min(*SWING_RANGE.end());
    let eighth = (beat / 0.5).floor() * 0.5;
    let within = beat - eighth;
    // Where the second sixteenth lands: from 0.25 straight to a third of a beat (the triplet).
    let second = 0.25 + amount / 12.0;
    if within < 0.25 {
        eighth + within * second / 0.25
    } else {
        eighth + second + (within - 0.25) * (0.5 - second) / 0.25
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swing_moves_the_second_sixteenth_and_keeps_the_eighths() {
        assert_eq!(swung(0.25, 0.0), 0.25, "no swing, nothing moves");
        assert!((swung(0.25, 1.0) - 1.0 / 3.0).abs() < 1e-12, "full swing: the triplet");
        assert!((swung(0.25, 0.5) - (0.25 + 1.0 / 24.0)).abs() < 1e-12);
        for beat in [0.0, 0.5, 1.0, 7.5] {
            assert!((swung(beat, 1.0) - beat).abs() < 1e-12, "the eighths stay: {beat}");
        }
        assert!((swung(1.75, 1.0) - (1.5 + 1.0 / 3.0)).abs() < 1e-12, "in every eighth");
        // The order of notes never changes.
        let mut last = -1.0;
        for step in 0..200 {
            let beat = f64::from(step) * 0.01;
            let at = swung(beat, 1.0);
            assert!(at >= last, "{beat}");
            last = at;
        }
        assert!(swung(f64::NAN, 1.0).is_nan());
        assert_eq!(swung(0.25, f64::NAN), 0.25);
    }

    #[test]
    fn a_clip_follows_the_projects_swing_unless_it_has_its_own() {
        let follows = ClipSwing { amount: 0.8, own: false };
        assert_eq!(follows.effective(0.3), 0.3, "the project's, its own kept aside");
        assert_eq!(ClipSwing { own: true, ..follows }.effective(0.3), 0.8);
        assert_eq!(ClipSwing { amount: 5.0, own: true }.effective(0.0), 1.0, "kept in range");
        assert!(ClipSwing::default().is_default());
    }
}
