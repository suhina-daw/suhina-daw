// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The grid of a timeline at one zoom: the lines drawn and the beats things snap to. The
//! arrangement, the piano roll and the automation lanes all use it, so snapping is the same
//! wherever a user edits on a timeline (`docs/spec/arrangement-editing.md` › The grid and
//! `docs/spec/ui-clip-editor.md` › Grid and snapping). Each view keeps its own grid setting and
//! turns it into a step; only the arithmetic is here.

/// Beats closer than this are the same grid line.
const EPSILON: f64 = 1e-9;

/// The finest note value an adaptive grid reaches, in beats (1/32).
const FINEST: f64 = 0.125;

/// How many times an adaptive grid doubles a bar when zoomed far out, at most.
const MAX_BAR_DOUBLINGS: i32 = 16;

/// How many bars a grid coarsens to at most when zoomed far out.
const MAX_BARS: f64 = 1024.0;

/// A timeline's grid at one zoom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineGrid {
    /// Beats between lines inside a bar; a whole number of bars when it is a bar or more.
    pub step: f64,
    /// Beats in a bar: lines start again at every bar line.
    pub bar_beats: f64,
    /// Whether things snap to the lines; without, the lines are still drawn.
    pub snaps: bool,
}

impl TimelineGrid {
    /// The grid for a chosen step of `chosen` beats (0 or less: no snapping, lines by the zoom
    /// alone), at `beat_width` points a beat with bars of `bar_beats` beats. Lines are the chosen
    /// step apart, or coarser where they would be closer than `min_spacing` points: a straight
    /// step doubles up to the beat, a triplet goes to the beat, then the beat goes to the bar and
    /// bars double. So the lines always land on the bar lines.
    pub fn new(chosen: f64, beat_width: f32, bar_beats: f64, min_spacing: f32) -> Self {
        let bar_beats = bar_length(bar_beats);
        let snaps = chosen.is_finite() && chosen > 0.0;
        let mut step = if snaps { chosen } else { 1.0 };
        // A note value longer than a bar that is not a whole number of bars (a half note in
        // 3/8) is drawn on the bar lines instead, so lines stay on the bar.
        if step > bar_beats + EPSILON && !is_whole(step / bar_beats) {
            step = bar_beats;
        }
        let points = f64::from(beat_width.max(f32::MIN_POSITIVE));
        let min = f64::from(min_spacing);
        while step * points < min && step < MAX_BARS * bar_beats {
            step = if step >= bar_beats - EPSILON {
                step * 2.0
            } else if step < 1.0 - EPSILON && is_whole((1.0 / step).log2()) {
                // A straight note value finer than a beat: 1/32, 1/16, 1/8, 1/4.
                (step * 2.0).min(1.0)
            } else if step < 1.0 - EPSILON {
                // A triplet finer than a beat goes to the beat, whose lines are on the bar.
                1.0
            } else {
                bar_beats
            };
            // A bar shorter than a beat (2/8): the beat is coarser than the bar, so go by bars.
            if step > bar_beats + EPSILON && !is_whole(step / bar_beats) {
                step = bar_beats;
            }
        }
        Self { step, bar_beats, snaps }
    }

    /// Whether the step is one bar or more, so lines are only on bar lines.
    fn in_bars(&self) -> bool {
        self.step >= self.bar_beats - EPSILON
    }

    /// The grid line nearest to `beat`, or with `down` the line at or before it (the start of
    /// the grid cell it is in); `beat` itself with `free` (Alt held) or a grid that does not
    /// snap. Lines inside a bar are counted from its bar line, so a step that does not fill the
    /// bar (two beats in 3/4) starts again at each bar. Never before the start.
    pub fn place(&self, beat: f64, free: bool, down: bool) -> f64 {
        if !beat.is_finite() {
            return 0.0;
        }
        let beat = beat.max(0.0);
        if free || !self.snaps || self.step <= 0.0 {
            return beat;
        }
        if self.in_bars() {
            let lines = beat / self.step;
            return (if down { (lines + EPSILON).floor() } else { lines.round() }) * self.step;
        }
        let bar_start = (beat / self.bar_beats + EPSILON).floor() * self.bar_beats;
        if down {
            return bar_start + ((beat - bar_start) / self.step + EPSILON).floor() * self.step;
        }
        // Inside a bar: lines from the bar line on, and the next bar line.
        let next_bar = bar_start + self.bar_beats;
        let on_step = bar_start + ((beat - bar_start) / self.step).round() * self.step;
        if on_step >= next_bar - EPSILON || (next_bar - beat) < (beat - on_step).abs() { next_bar } else { on_step }
    }

    /// The first line after `beat`, as [`TimelineGrid::lines`] draws them.
    pub fn next_line(&self, beat: f64) -> f64 {
        let cell = self.place(beat, false, true);
        let step = if self.step > 0.0 { self.step } else { 1.0 };
        if self.in_bars() {
            return cell + step;
        }
        let bar_start = (cell / self.bar_beats + EPSILON).floor() * self.bar_beats;
        (cell + step).min(bar_start + self.bar_beats)
    }

    /// The last line before `beat`, never before the start.
    pub fn previous_line(&self, beat: f64) -> f64 {
        if !beat.is_finite() || beat <= EPSILON {
            return 0.0;
        }
        // Just before `beat`, so a line on it is not the one before it.
        self.place(beat - 1e-6, false, true)
    }

    /// The beats of the lines from `first` to `last` (both included), each with whether it is a
    /// bar line. At most `limit` lines, so a bad zoom can never draw without end.
    pub fn lines(&self, first: f64, last: f64, limit: usize) -> Vec<(f64, bool)> {
        let mut lines = Vec::new();
        if !(first.is_finite() && last.is_finite()) || self.step <= 0.0 {
            return lines;
        }
        let first = first.max(0.0);
        if self.in_bars() {
            let mut n = (first / self.step).floor();
            while lines.len() < limit {
                let beat = n * self.step;
                if beat > last {
                    break;
                }
                lines.push((beat, true));
                n += 1.0;
            }
            return lines;
        }
        let mut bar = (first / self.bar_beats).floor();
        'bars: while lines.len() < limit {
            let bar_start = bar * self.bar_beats;
            if bar_start > last {
                break;
            }
            if bar_start >= first - EPSILON {
                lines.push((bar_start, true));
            }
            let mut k = ((first - bar_start) / self.step).ceil().max(1.0);
            loop {
                let beat = bar_start + k * self.step;
                if beat >= bar_start + self.bar_beats - EPSILON {
                    break;
                }
                if lines.len() >= limit || beat > last {
                    break 'bars;
                }
                lines.push((beat, false));
                k += 1.0;
            }
            bar += 1.0;
        }
        lines
    }
}

/// An adaptive grid's step at `beat_width` points a beat: the finest power of two note value
/// (1/32 up to the last one shorter than a bar; two thirds as long with `triplet`), then 1 bar,
/// 2 bars, 4 bars and on, whose lines are at least `spacing` points apart. An unusable zoom (not
/// a positive number) gives one bar.
pub fn adaptive_step(spacing: f64, triplet: bool, beat_width: f32, bar_beats: f64) -> f64 {
    let bar = bar_length(bar_beats);
    if !(beat_width.is_finite() && beat_width > 0.0) {
        return bar;
    }
    let points = f64::from(beat_width);
    let third = if triplet { 2.0 / 3.0 } else { 1.0 };
    // The note values shorter than a bar: 1/32, 1/16, ... at most five of them (to 1/2).
    let mut value = FINEST;
    for _ in 0..5 {
        if value >= bar {
            break;
        }
        if value * third * points >= spacing {
            return value * third;
        }
        value *= 2.0;
    }
    let mut step = bar;
    for _ in 0..MAX_BAR_DOUBLINGS {
        if step * points >= spacing {
            return step;
        }
        step *= 2.0;
    }
    step
}

/// Beats in a bar, or 4 when `bar_beats` is not a positive number.
pub fn bar_length(bar_beats: f64) -> f64 {
    if bar_beats.is_finite() && bar_beats > 0.0 { bar_beats } else { 4.0 }
}

fn is_whole(value: f64) -> bool {
    value.is_finite() && (value - value.round()).abs() < 1e-6
}

#[cfg(test)]
#[path = "timeline_grid_tests.rs"]
mod tests;
