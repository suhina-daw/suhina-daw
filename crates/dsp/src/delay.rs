// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A delay line that holds a signal back by a whole or fractional number of frames, for a
//! device's trim delay. One channel; the size is fixed when it is built and the length can move
//! smoothly while running (the reading position follows it).

use crate::{flush, usable_sample_rate};

/// Delay of one part of the input can write backwards in the ring, it reads nothing behind what
/// was written; lengths beyond that are short of whole-number of frames.
/// One channel.
#[derive(Debug, Clone, PartialEq)]
pub struct DelayLine {
    buffer: Box<[f64]>,
    /// The frame the next input is written to.
    write: usize,
    /// How many frames the output trails the input, in whole and fractional frames.
    delay: f64,
}

impl DelayLine {
    /// A delay line holding at most `seconds` of audio at `sample_rate` (a non-positive or
    /// non-finite number of seconds keeps one frame, a non-finite rate is 48 kHz).
    pub fn new(sample_rate: f64, seconds: f64) -> Self {
        let rate = usable_sample_rate(sample_rate);
        let frames = if seconds.is_finite() && seconds > 0.0 {
            (seconds * rate).ceil().clamp(1.0, 4_000_000.0) as usize
        } else {
            1
        };
        Self { buffer: vec![0.0; frames].into_boxed_slice(), write: 0, delay: 0.0 }
    }

    /// The delay's longest length, in frames.
    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    /// How many frames the output trails the input.
    pub fn frames(&self) -> f64 {
        self.delay
    }

    /// Sets the delay, in frames (whole and fractional); a number beyond the capacity, negative
    /// or not finite keeps the longest, the shortest or the length it had.
    pub fn set_frames(&mut self, frames: f64) {
        if !frames.is_finite() {
            return;
        }
        self.delay = frames.clamp(0.0, (self.buffer.len() - 1) as f64);
    }

    /// Feeds one sample in and gets the delayed one out. The length set by [`Self::set_frames`]
    /// is followed at once; moving it smoothly re-reads positions in between.
    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        self.buffer[self.write] = flush(x);
        self.write = (self.write + 1) % self.buffer.len();
        let position = self.write as f64 - 1.0 - self.delay;
        let whole = position.floor();
        let before = (whole.rem_euclid(self.buffer.len() as f64)) as usize;
        let after = (before + 1) % self.buffer.len();
        let fraction = (position - whole).clamp(0.0, 1.0);
        let first = self.buffer[before];
        let second = self.buffer[after];
        flush(first + (second - first) * fraction)
    }

    /// Empties the line, as if it had just been built.
    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds `samples` in and collects the output.
    fn run(delay: &mut DelayLine, samples: &[f64]) -> Vec<f64> {
        samples.iter().map(|&x| delay.process(x)).collect()
    }

    #[test]
    fn zero_delay_passes_the_input_through() {
        let mut line = DelayLine::new(48_000.0, 0.01);
        let input: Vec<f64> = (0..45).map(|i| (i as f64 * 0.2).sin()).collect();
        let out = run(&mut line, &input);
        for (y, &x) in out.iter().zip(&input) {
            assert!((y - x).abs() < 1e-9);
        }
    }

    #[test]
    fn a_whole_number_of_frames_delays_by_that_many() {
        let mut line = DelayLine::new(48_000.0, 0.01);
        line.set_frames(5.0);
        let input: Vec<f64> = (1..50).map(|i| i as f64).collect();
        let out = run(&mut line, &input);
        for (i, &y) in out.iter().enumerate() {
            let want = if i >= 5 { input[i - 5] } else { 0.0 };
            assert!((y - want).abs() < 1e-9, "i {i}: {y} vs {want}");
        }
    }

    #[test]
    fn a_fractional_delay_sits_between_the_two_frames() {
        let mut line = DelayLine::new(48_000.0, 0.01);
        line.set_frames(2.5);
        let input: Vec<f64> = (0..40).map(|i| i as f64).collect();
        let out = run(&mut line, &input);
        // From input frame 3 on, the output reads between frames i-2.5 and i-3.5.
        for (i, &y) in out.iter().enumerate().skip(4) {
            let want = 0.5 * (input[i - 2] + input[i - 3]);
            assert!((y - want).abs() < 1e-9, "i {i}: {y} vs {want}");
        }
    }

    #[test]
    fn moving_the_length_smoothly_keeps_the_signal() {
        let mut line = DelayLine::new(48_000.0, 0.01);
        let input: Vec<f64> = (0..100).map(|i| (i as f64 * 0.13).sin()).collect();
        // 0 frames for the first 30, then growing by 1/10 frame to 5, then 5 for good.
        let mut out = Vec::new();
        for (i, &x) in input.iter().enumerate() {
            let set = match i {
                0..=29 => 0.0,
                30..=79 => ((i - 30) as f64) / 10.0,
                _ => 5.0,
            };
            line.set_frames(set);
            out.push(line.process(x));
        }
        // Nothing is lost or wrapped and nothing NaN: the output stays in the input's range.
        for &y in &out {
            assert!(y.is_finite());
            assert!(y.abs() <= 1.0);
        }
    }

    #[test]
    fn lengths_beyond_the_capacity_are_kept_and_reset_empties() {
        let mut line = DelayLine::new(48_000.0, 0.001); // 48 frames
        line.set_frames(1_000_000.0);
        assert_eq!(line.frames(), 47.0);
        let input: Vec<f64> = (1..=60).map(|i| i as f64).collect();
        let out = run(&mut line, &input);
        // Input must wait the first 47 frames, then come through.
        for (i, &y) in out.iter().enumerate() {
            let want = if i >= 47 { Some(input[i - 47]) } else { Some(0.0) };
            assert_eq!(Some(y), want, "frame {i}");
        }
        line.reset();
        assert_eq!(line.frames(), 47.0);
        let after = run(&mut line, &input);
        // Cleared memory: the first frames are silence again.
        for &y in &after[..47] {
            assert_eq!(y, 0.0);
        }
        for (i, &y) in after.iter().enumerate().skip(47) {
            assert_eq!(y, input[i - 47]);
        }
    }

    #[test]
    fn hostile_numbers_do_not_panic() {
        let mut line = DelayLine::new(f64::NAN, f64::INFINITY); // 48 kHz rate, 1 frame long
        line.set_frames(f64::NAN);
        line.set_frames(-5.0);
        assert_eq!(line.frames(), 0.0);
        assert_eq!(line.process(f64::NAN), 0.0);
        assert_eq!(line.process(1.0), 1.0);
        let tiny = DelayLine::new(48_000.0, -1.0);
        assert_eq!(tiny.capacity(), 1);
    }
}
