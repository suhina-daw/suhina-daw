// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The time axis every timeline view shares (#764 › 6): where a beat is on screen and which
//! beat is under a point, for the Arrangement, its automation lanes, the piano roll and its
//! ruler, and how far each of them zooms. Each view keeps its own rules for what it scrolls and
//! where its axis starts; the arithmetic is written once, here.

use std::ops::RangeInclusive;

/// How wide a beat may be in the Arrangement, in points.
pub const ARRANGEMENT_BEAT_WIDTHS: RangeInclusive<f32> = 1.0..=4000.0;
/// How wide a beat may be in the piano roll and its ruler, in points.
pub const PIANO_ROLL_BEAT_WIDTHS: RangeInclusive<f32> = 4.0..=400.0;

/// A beat width kept within `range`.
pub fn clamp_beat_width(width: f32, range: &RangeInclusive<f32>) -> f32 {
    width.clamp(*range.start(), *range.end())
}

/// A horizontal time axis: beat 0 is at `origin` on screen, and a beat is `beat_width` points
/// wide. A scrolled view's origin is its left edge less its scroll.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeViewport {
    pub origin: f32,
    pub beat_width: f32,
}

impl TimeViewport {
    /// The axis of a view whose content starts at `left` and is scrolled by `scroll` points.
    pub fn scrolled(left: f32, scroll: f32, beat_width: f32) -> Self {
        Self { origin: left - scroll, beat_width }
    }

    /// Where `beat` is on screen.
    pub fn x_of(&self, beat: f64) -> f32 {
        self.origin + beat as f32 * self.beat_width
    }

    /// The beat at `x` on screen: the inverse of [`TimeViewport::x_of`]. A beat width of 0 counts
    /// as the smallest one rather than dividing by zero.
    pub fn beat_at(&self, x: f32) -> f64 {
        f64::from((x - self.origin) / self.beat_width.max(f32::MIN_POSITIVE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_beat_and_its_point_map_both_ways() {
        let axis = TimeViewport::scrolled(100.0, 40.0, 20.0);
        assert_eq!(axis.x_of(0.0), 60.0);
        assert_eq!(axis.x_of(2.0), 100.0);
        assert_eq!(axis.beat_at(100.0), 2.0);
        assert!(!TimeViewport { origin: 0.0, beat_width: 0.0 }.beat_at(0.0).is_nan());
    }

    #[test]
    fn zoom_stays_in_the_views_range() {
        assert_eq!(clamp_beat_width(9000.0, &ARRANGEMENT_BEAT_WIDTHS), 4000.0);
        assert_eq!(clamp_beat_width(1.0, &PIANO_ROLL_BEAT_WIDTHS), 4.0);
    }
}
