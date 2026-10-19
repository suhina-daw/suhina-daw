// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Delay lines: a ring buffer that is read back at any delay, between samples by linear or cubic
//! interpolation, and the Schroeder allpass built on one.
//!
//! A line is read before the next sample is pushed: `read(d)` is the input `d` frames before the
//! one about to be pushed, so a line read at `d` and then pushed delays its input by exactly `d`
//! frames, also inside a feedback loop.

/// Most frames a line holds (about 44 s at 384 kHz); longer requests are cut to this.
pub const MAX_FRAMES: usize = 1 << 24;

/// Samples below this (about -600 dB) are written as zero, so a decaying tail never reaches
/// subnormal numbers.
pub const SAMPLE_FLOOR: f32 = 1e-30;

/// A ring buffer of samples. The buffer is allocated by [`new`](Self::new); pushing and reading
/// never allocate.
#[derive(Debug, Clone, PartialEq)]
pub struct DelayLine {
    /// The samples, a power of two long so positions wrap with a mask.
    buffer: Vec<f32>,
    /// `buffer.len() - 1`, which wraps a position into the buffer.
    mask: usize,
    /// Where the next sample goes.
    write: usize,
}

impl Default for DelayLine {
    /// A line of a few frames, for a device that has not been prepared yet.
    fn default() -> Self {
        Self::new(0)
    }
}

impl DelayLine {
    /// A line that can be read at delays up to at least `frames` (allocates).
    pub fn new(frames: usize) -> Self {
        let size = frames.saturating_add(4).min(MAX_FRAMES).next_power_of_two();
        Self { buffer: vec![0.0; size], mask: size - 1, write: 0 }
    }

    /// A line for delays up to `seconds` at `sample_rate` (allocates).
    pub fn for_seconds(seconds: f64, sample_rate: f64) -> Self {
        let frames = seconds * crate::usable_sample_rate(sample_rate);
        let frames = if frames.is_finite() && frames > 0.0 { frames.min(MAX_FRAMES as f64).ceil() as usize } else { 0 };
        Self::new(frames)
    }

    /// Longest delay [`read_linear`](Self::read_linear) and [`read_hermite`](Self::read_hermite)
    /// reach, in frames.
    pub fn capacity(&self) -> f64 {
        self.buffer.len().saturating_sub(3) as f64
    }

    /// Silences the line.
    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
    }

    /// Writes the next sample. Non-finite samples, and ones so small they would be subnormal on
    /// their next trip round a feedback loop, are written as silence.
    #[inline]
    pub fn push(&mut self, x: f32) {
        if let Some(slot) = self.buffer.get_mut(self.write) {
            *slot = if x.is_finite() && x.abs() >= SAMPLE_FLOOR { x } else { 0.0 };
        }
        self.write = (self.write + 1) & self.mask;
    }

    /// The input `delay` frames before the next push (`delay` from 1).
    #[inline]
    fn at(&self, delay: usize) -> f32 {
        self.buffer.get(self.write.wrapping_sub(delay) & self.mask).copied().unwrap_or(0.0)
    }

    /// The input `delay` frames before the next push, whole frames only, at least one.
    #[inline]
    pub fn read(&self, delay: usize) -> f32 {
        self.at(delay.clamp(1, self.buffer.len().saturating_sub(1)))
    }

    /// The input `delay` frames before the next push, between samples by linear interpolation.
    /// `delay` is limited to 1 ..= [`capacity`](Self::capacity).
    #[inline]
    pub fn read_linear(&self, delay: f64) -> f32 {
        let delay = if delay.is_finite() { delay.clamp(1.0, self.capacity()) } else { 1.0 };
        let whole = delay as usize;
        let t = (delay - whole as f64) as f32;
        let older = self.write.wrapping_sub(whole + 1) & self.mask;
        let [b, a] = match self.buffer.get(older..older + 2) {
            Some(&[b, a]) => [b, a],
            _ => [self.at(whole + 1), self.at(whole)],
        };
        a + (b - a) * t
    }

    /// The input `delay` frames before the next push, between samples by cubic (Catmull-Rom
    /// Hermite) interpolation, which keeps more of the top octave than linear interpolation.
    /// `delay` is limited to 2 ..= [`capacity`](Self::capacity).
    #[inline]
    pub fn read_hermite(&self, delay: f64) -> f32 {
        let delay = if delay.is_finite() { delay.clamp(2.0, self.capacity().max(2.0)) } else { 2.0 };
        let whole = delay as usize;
        let t = (delay - whole as f64) as f32;
        // The four samples around the position, oldest first.
        let oldest = self.write.wrapping_sub(whole + 2) & self.mask;
        let [y2, y1, y0, before] = match self.buffer.get(oldest..oldest + 4) {
            Some(&[y2, y1, y0, before]) => [y2, y1, y0, before],
            _ => [self.at(whole + 2), self.at(whole + 1), self.at(whole), self.at(whole - 1)],
        };
        let c1 = 0.5 * (y1 - before);
        let c2 = before - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
        let c3 = 0.5 * (y2 - before) + 1.5 * (y0 - y1);
        ((c3 * t + c2) * t + c1) * t + y0
    }
}

/// A Schroeder allpass: a delay with feedback `gain` and feedforward `-gain`, which passes every
/// frequency at the same level and smears an impulse into a train of echoes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Allpass {
    /// The allpass's state: its input plus the fed-back output.
    line: DelayLine,
}

impl Allpass {
    /// An allpass for delays up to `seconds` at `sample_rate` (allocates).
    pub fn for_seconds(seconds: f64, sample_rate: f64) -> Self {
        Self { line: DelayLine::for_seconds(seconds, sample_rate) }
    }

    /// One sample through an allpass `delay` frames long (fractional, at least 2) with
    /// `gain` (limited to ±0.99 so it stays stable).
    #[inline]
    pub fn process(&mut self, x: f32, delay: f64, gain: f32) -> f32 {
        let gain = if gain.is_finite() { gain.clamp(-0.99, 0.99) } else { 0.0 };
        let delayed = self.line.read_hermite(delay);
        let v = x + gain * delayed;
        self.line.push(v);
        delayed - gain * v
    }

    /// Silences the allpass.
    pub fn clear(&mut self) {
        self.line.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_frame_delays_are_exact() {
        let mut line = DelayLine::new(16);
        let mut out = Vec::new();
        for n in 0..10 {
            out.push(line.read(3));
            line.push(n as f32 + 1.0);
        }
        assert_eq!(out, [0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
        assert_eq!(line.read_linear(3.0), 8.0);
        assert_eq!(line.read_hermite(3.0), 8.0);
    }

    #[test]
    fn fractional_delays_interpolate() {
        let mut line = DelayLine::new(16);
        for n in 0..8 {
            line.push(n as f32);
        }
        // The last push was 7, so a delay of 2.5 lies between 6 and 5.
        assert_eq!(line.read_linear(2.5), 5.5);
        // A straight line is reproduced exactly by the cubic too.
        assert!((line.read_hermite(2.5) - 5.5).abs() < 1e-6);
        assert!((line.read_hermite(3.25) - 4.75).abs() < 1e-6);
    }

    #[test]
    fn reads_across_the_end_of_the_buffer_match_reads_inside_it() {
        // Fill a short line past its end, so some reads wrap and some do not.
        let mut line = DelayLine::new(12);
        let mut history = Vec::new();
        for n in 0..40 {
            let x = ((n * 7) % 11) as f32 - 5.0;
            history.push(x);
            line.push(x);
            if n < 16 {
                continue;
            }
            for delay in [2.0, 2.5, 3.75, 6.2, 9.9, 11.0] {
                // The four samples around the read, from the history: a delay of d is the sample
                // pushed d frames before the next one.
                let whole = delay as usize;
                let t = (delay - whole as f64) as f32;
                let at = |d: usize| history[history.len() - d];
                let (before, y0, y1, y2) = (at(whole - 1), at(whole), at(whole + 1), at(whole + 2));
                let c1 = 0.5 * (y1 - before);
                let c2 = before - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
                let c3 = 0.5 * (y2 - before) + 1.5 * (y0 - y1);
                let expected = ((c3 * t + c2) * t + c1) * t + y0;
                assert!((line.read_hermite(delay) - expected).abs() < 1e-5, "{n} {delay}");
                assert!((line.read_linear(delay) - (y0 + (y1 - y0) * t)).abs() < 1e-5, "{n} {delay}");
            }
        }
    }

    #[test]
    fn hermite_keeps_a_high_sine_better_than_linear() {
        let sr = 48_000.0;
        let hz = 12_000.0;
        let mut line = DelayLine::new(64);
        let (mut lin, mut cubic) = (0.0f64, 0.0f64);
        for n in 0..4_800 {
            let x = (std::f64::consts::TAU * hz * n as f64 / sr).sin() as f32;
            if n > 100 {
                lin = lin.max(f64::from(line.read_linear(10.5)).abs());
                cubic = cubic.max(f64::from(line.read_hermite(10.5)).abs());
            }
            line.push(x);
        }
        assert!(cubic > lin + 0.1, "linear {lin}, cubic {cubic}");
    }

    #[test]
    fn delays_are_limited_and_input_cleaned() {
        let mut line = DelayLine::new(8);
        assert!(line.capacity() >= 8.0);
        line.push(f32::NAN);
        line.push(f32::INFINITY);
        assert_eq!(line.read(1), 0.0);
        line.push(1e-35);
        assert_eq!(line.read(1), 0.0);
        for delay in [f64::NAN, -5.0, 0.0, 1e12, f64::INFINITY] {
            assert!(line.read_linear(delay).is_finite());
            assert!(line.read_hermite(delay).is_finite());
        }
        assert!(DelayLine::for_seconds(f64::NAN, 48_000.0).capacity() >= 1.0);
        assert!(DelayLine::for_seconds(1.0, 48_000.0).capacity() >= 48_000.0);
        line.push(1.0);
        line.clear();
        assert_eq!(line.read(1), 0.0);
    }

    #[test]
    fn an_allpass_keeps_the_energy_of_an_impulse() {
        let mut allpass = Allpass::for_seconds(0.01, 48_000.0);
        let mut energy = 0.0f64;
        for n in 0..48_000 {
            let y = allpass.process(if n == 0 { 1.0 } else { 0.0 }, 113.0, 0.7);
            energy += f64::from(y) * f64::from(y);
        }
        assert!((energy - 1.0).abs() < 1e-4, "{energy}");
        assert!(allpass.process(1.0, f64::NAN, f32::NAN).is_finite());
    }
}
