// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Parameter smoothing. A parameter that jumps from one block to the next makes an audible step
//! ("zipper noise"); a [`Smoother`] ramps it linearly to each new target instead.

/// Ramps a value linearly to its target over a fixed number of samples. The ramp ends exactly
/// on the target, so a settled smoother returns the target unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Smoother {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
    ramp: u32,
}

impl Smoother {
    /// A smoother that starts settled at `value` and ramps over `ramp` samples (at least one).
    pub fn new(value: f32, ramp: u32) -> Self {
        let value = if value.is_finite() { value } else { 0.0 };
        Self { current: value, target: value, step: 0.0, remaining: 0, ramp: ramp.max(1) }
    }

    /// Ramp length for `seconds` at `sample_rate`, in samples (at least one).
    pub fn ramp_frames(seconds: f64, sample_rate: f64) -> u32 {
        let frames = seconds * sample_rate;
        if frames.is_finite() && frames >= 1.0 { frames.min(f64::from(u32::MAX)) as u32 } else { 1 }
    }

    /// Changes the ramp length used by later targets.
    pub fn set_ramp(&mut self, ramp: u32) {
        self.ramp = ramp.max(1);
    }

    /// Starts a ramp from the current value to `target`. Non-finite targets are ignored.
    pub fn set_target(&mut self, target: f32) {
        if !target.is_finite() || target == self.target {
            return;
        }
        self.target = target;
        self.remaining = self.ramp;
        self.step = (target - self.current) / self.ramp as f32;
    }

    /// Jumps to `value` without a ramp (after a reset, or before the first block).
    pub fn reset(&mut self, value: f32) {
        if value.is_finite() {
            *self = Self { current: value, target: value, step: 0.0, remaining: 0, ramp: self.ramp };
        }
    }

    /// Advances one sample and returns the value for it.
    #[inline]
    pub fn next_value(&mut self) -> f32 {
        if self.remaining > 0 {
            self.remaining -= 1;
            self.current = if self.remaining == 0 { self.target } else { self.current + self.step };
        }
        self.current
    }

    /// The value the last [`next_value`](Self::next_value) returned.
    pub fn current(&self) -> f32 {
        self.current
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    /// Whether the value has reached its target.
    pub fn is_settled(&self) -> bool {
        self.remaining == 0
    }
}

/// A one-pole low-pass for control signals: follows its input with time constant `seconds`
/// (63 % of a step after that long). Softens edges in a modulation signal, such as a square
/// LFO, that would otherwise click.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OnePole {
    coefficient: f64,
    state: f64,
}

/// The coefficient of a one-pole filter with time constant `seconds` at `sample_rate`:
/// `exp(-1 / (seconds · sample_rate))`, what each step keeps of the distance to its target.
/// 0.0 (no smoothing) when the time constant is not a positive finite number of frames.
pub fn time_constant(seconds: f64, sample_rate: f64) -> f64 {
    let frames = seconds * sample_rate;
    if frames.is_finite() && frames > 0.0 { (-1.0 / frames).exp() } else { 0.0 }
}

impl OnePole {
    pub fn new(seconds: f64, sample_rate: f64) -> Self {
        Self { coefficient: time_constant(seconds, crate::usable_sample_rate(sample_rate)), state: 0.0 }
    }

    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        self.state = crate::flush(x + (self.state - x) * self.coefficient);
        self.state
    }

    /// Jumps to `value`, as if it had been the input forever.
    pub fn reset(&mut self, value: f64) {
        self.state = if value.is_finite() { value } else { 0.0 };
    }
}

/// Two one-pole low-passes in a row with the same time constant (critically damped), for a
/// control that jumps and moves: a jump becomes an S-shaped glide that starts and ends without a
/// corner, a steady motion is followed at a fixed lag of two time constants, and nothing
/// restarts when the target moves on (as a [`Smoother`]'s ramp does). It lands on its target
/// exactly once within a millionth of it, so a settled glide returns the target unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glide {
    coefficient: f64,
    first: f64,
    second: f64,
    target: f64,
}

impl Glide {
    /// A glide settled at `value` with time constant `seconds` at `sample_rate`.
    pub fn new(value: f64, seconds: f64, sample_rate: f64) -> Self {
        let value = if value.is_finite() { value } else { 0.0 };
        let coefficient = time_constant(seconds, crate::usable_sample_rate(sample_rate));
        Self { coefficient, first: value, second: value, target: value }
    }

    /// Changes the time constant (a new sample rate).
    pub fn set_time(&mut self, seconds: f64, sample_rate: f64) {
        self.coefficient = time_constant(seconds, crate::usable_sample_rate(sample_rate));
    }

    /// Glides on toward `target` from where it is. Non-finite targets are ignored.
    pub fn set_target(&mut self, target: f64) {
        if target.is_finite() {
            self.target = target;
        }
    }

    /// Jumps to `value` without a glide.
    pub fn reset(&mut self, value: f64) {
        if value.is_finite() {
            *self = Self { first: value, second: value, target: value, ..*self };
        }
    }

    /// Advances one sample and returns the value for it.
    #[inline]
    pub fn next_value(&mut self) -> f64 {
        if self.is_settled() {
            return self.target;
        }
        let k = self.coefficient;
        self.first = self.target + (self.first - self.target) * k;
        self.second = self.first + (self.second - self.first) * k;
        if (self.first - self.target).abs() < 1e-6 && (self.second - self.target).abs() < 1e-6 {
            (self.first, self.second) = (self.target, self.target);
        }
        self.second
    }

    /// The value the last [`next_value`](Self::next_value) returned.
    pub fn current(&self) -> f64 {
        self.second
    }

    pub fn target(&self) -> f64 {
        self.target
    }

    /// Whether the value has reached its target.
    pub fn is_settled(&self) -> bool {
        self.first == self.target && self.second == self.target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramps_linearly_and_lands_on_the_target() {
        let mut s = Smoother::new(0.0, 4);
        s.set_target(1.0);
        let values: Vec<f32> = (0..6).map(|_| s.next_value()).collect();
        assert_eq!(values, [0.25, 0.5, 0.75, 1.0, 1.0, 1.0]);
        assert!(s.is_settled());
    }

    #[test]
    fn a_new_target_mid_ramp_starts_from_where_it_is() {
        let mut s = Smoother::new(0.0, 4);
        s.set_target(1.0);
        s.next_value();
        s.next_value();
        s.set_target(0.0);
        let values: Vec<f32> = (0..4).map(|_| s.next_value()).collect();
        assert_eq!(values, [0.375, 0.25, 0.125, 0.0]);
    }

    #[test]
    fn ignores_hostile_values() {
        let mut s = Smoother::new(f32::NAN, 0);
        assert_eq!(s.next_value(), 0.0);
        s.set_target(f32::INFINITY);
        assert!(s.is_settled());
        s.reset(f32::NAN);
        assert_eq!(s.current(), 0.0);
        assert_eq!(Smoother::ramp_frames(f64::NAN, 48_000.0), 1);
        assert_eq!(Smoother::ramp_frames(0.02, 48_000.0), 960);
        assert_eq!(Smoother::ramp_frames(1e300, 48_000.0), u32::MAX);
    }

    #[test]
    fn one_pole_follows_with_its_time_constant() {
        let mut p = OnePole::new(0.001, 48_000.0);
        let mut y = 0.0;
        for _ in 0..48 {
            y = p.process(1.0);
        }
        assert!((y - (1.0 - (-1.0f64).exp())).abs() < 1e-9, "{y}");
        p.reset(f64::NAN);
        assert_eq!(p.process(0.0), 0.0);
        let mut instant = OnePole::new(0.0, 48_000.0);
        assert_eq!(instant.process(0.7), 0.7);
    }

    #[test]
    fn a_time_constant_keeps_what_one_frame_of_it_keeps() {
        assert_eq!(time_constant(0.01, 48_000.0), (-1.0f64 / 480.0).exp());
        for none in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(time_constant(none, 48_000.0), 0.0, "{none}");
        }
        assert_eq!(time_constant(0.01, f64::NAN), 0.0);
    }
    #[test]
    fn a_glide_turns_a_jump_into_an_s_and_lands_on_it() {
        // 1 ms at 48 kHz: 48 frames a time constant.
        let mut g = Glide::new(0.0, 0.001, 48_000.0);
        g.set_target(1.0);
        let values: Vec<f64> = (0..2_000).map(|_| g.next_value()).collect();
        // Slow at first (no corner), half-way after 1.68 time constants, rising throughout.
        assert!(values[2] < 0.005, "{}", values[2]);
        let half = values.iter().position(|&v| v >= 0.5).unwrap_or(0);
        assert!((80..=82).contains(&half), "{half}");
        assert!(values.windows(2).all(|w| w[1] >= w[0]));
        assert!(g.is_settled() && g.current() == 1.0);
        // A steady motion is followed two time constants behind.
        let mut g = Glide::new(0.0, 0.001, 48_000.0);
        let mut last = 0.0;
        for i in 1..=4_800 {
            g.set_target(f64::from(i) / 4_800.0);
            last = g.next_value();
        }
        assert!((last - (1.0 - 96.0 / 4_800.0)).abs() < 1e-3, "{last}");
        // Hostile values change nothing.
        g.set_target(f64::NAN);
        g.reset(f64::INFINITY);
        assert!(g.next_value().is_finite());
        let mut instant = Glide::new(0.3, 0.0, 48_000.0);
        instant.set_target(0.7);
        assert_eq!(instant.next_value(), 0.7);
    }
}
