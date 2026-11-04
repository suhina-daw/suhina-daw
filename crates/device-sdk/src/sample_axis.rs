// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Source-time coordinates shared by waveform editors and beat slicing.
//! A project tempo change cannot move a point in the source file.

/// A bounded source axis with its first musical beat at a normalized file position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SampleAxis {
    frames: usize,
    seconds: f64,
    bpm: f64,
    origin: f64,
}

impl SampleAxis {
    /// Unknown tempo and empty/invalid sources have no beat axis.
    pub fn new(frames: usize, sample_rate: f64, bpm: f64, origin: f64) -> Option<Self> {
        if frames == 0
            || !sample_rate.is_finite()
            || sample_rate <= 0.
            || !bpm.is_finite()
            || !(1. ..=999.).contains(&bpm)
            || !origin.is_finite()
            || !(0. ..=1.).contains(&origin)
        {
            return None;
        }
        let seconds = frames as f64 / sample_rate;
        if !seconds.is_finite() || seconds <= 0. {
            return None;
        }
        Some(Self { frames, seconds, bpm, origin })
    }

    /// Quarter-note beat at a normalized source position; negative before the origin.
    pub fn beat(self, point: f64) -> f64 {
        (point - self.origin) * self.seconds * self.bpm / 60.
    }

    /// Source position of a quarter-note beat, without clipping to the file.
    pub fn point(self, beat: f64) -> f64 {
        self.origin + beat * 60. / self.bpm / self.seconds
    }

    pub fn seconds(self, point: f64) -> f64 {
        point * self.seconds
    }

    /// Whole-bar division follows the source meter; other divisions are quarter-note units.
    pub fn division(index: usize, numerator: f64, denominator_index: usize) -> f64 {
        if index == 0 {
            let denominator = [2., 4., 8., 16.].get(denominator_index).copied().unwrap_or(4.);
            numerator.round().clamp(1., 16.) * 4. / denominator
        } else {
            [4., 2., 1., 0.5, 0.25, 0.125, 1. / 3., 1. / 6.].get(index).copied().unwrap_or(0.5)
        }
    }

    /// Snap to a beat grid, bounded to the file and rounded to a source frame.
    /// Invalid input is rejected rather than becoming a marker at an arbitrary point.
    pub fn snap(self, point: f64, step: f64) -> Option<f64> {
        if !point.is_finite() || !step.is_finite() || step <= 0. {
            return None;
        }
        let beat = (self.beat(point) / step).round() * step;
        if !beat.is_finite() {
            return None;
        }
        let point = self.point(beat).clamp(0., 1.);
        Some((point * self.frames as f64).round() / self.frames as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_beats_round_trip_with_a_leading_offset() {
        let axis = SampleAxis::new(96000, 48000., 120., 0.125).unwrap();
        assert_eq!(axis.beat(0.125), 0.);
        assert_eq!(axis.beat(0.375), 1.);
        for point in [0., 0.125, 0.375, 0.9, 1.] {
            assert!((axis.point(axis.beat(point)) - point).abs() < 1e-12);
        }
    }

    #[test]
    fn snap_uses_source_tempo_and_rounds_to_frames() {
        let axis = SampleAxis::new(96000, 48000., 120., 0.).unwrap();
        assert_eq!(axis.snap(0.14, 0.5), Some(0.125));
        assert_eq!(axis.snap(0.14, 1.), Some(0.25));
        assert_eq!(axis.snap(-0.2, 1.), Some(0.));
        assert_eq!(axis.snap(1.2, 1.), Some(1.));
        assert!(axis.snap(f64::NAN, 1.).is_none());
        assert!(axis.snap(0.5, 0.).is_none());
    }

    #[test]
    fn unknown_tempo_and_invalid_sources_have_no_beat_grid() {
        for (frames, rate, bpm, origin) in [
            (0, 48000., 120., 0.),
            (1, 0., 120., 0.),
            (1, 48000., 0., 0.),
            (1, 48000., f64::INFINITY, 0.),
            (1, 48000., 120., f64::NAN),
            (1, 48000., 120., 1.1),
        ] {
            assert!(SampleAxis::new(frames, rate, bpm, origin).is_none());
        }
        assert!(SampleAxis::new(1, 48000., 120., 0.).is_some());
    }
}
