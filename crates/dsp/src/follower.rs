// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Attack/release followers: a one-pole step toward a target with one time constant when the
//! target moves one way and another when it moves back, as a compressor's gain reduction, a
//! limiter's gain or an envelope does.

use crate::smooth::time_constant;

/// One step of a one-pole filter: `value` moved toward `target`, keeping `coefficient` (0..1) of
/// the distance between them. Every follower here takes this step.
#[inline]
pub fn approach(value: f64, target: f64, coefficient: f64) -> f64 {
    target + (value - target) * coefficient
}

/// The attack and release coefficients of a follower. The value it follows is kept by the caller,
/// which may clamp or flush it between steps.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AttackRelease {
    attack: f64,
    release: f64,
}

impl AttackRelease {
    /// Coefficients for attack and release times in seconds at `sample_rate`; a time that is not
    /// a positive finite number of frames follows at once.
    pub fn new(attack: f64, release: f64, sample_rate: f64) -> Self {
        Self { attack: time_constant(attack, sample_rate), release: time_constant(release, sample_rate) }
    }

    /// A level that attacks upward: `value` moved toward `target` with the attack when the target
    /// is above it and the release otherwise (an envelope, a gain reduction in decibels).
    #[inline]
    pub fn rise(&self, value: f64, target: f64) -> f64 {
        approach(value, target, if target > value { self.attack } else { self.release })
    }

    /// A gain that attacks downward: `value` moved toward `target` with the attack when the target
    /// is below it and the release otherwise (a limiter's gain).
    #[inline]
    pub fn fall(&self, value: f64, target: f64) -> f64 {
        approach(value, target, if target < value { self.attack } else { self.release })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rising_level_takes_the_attack_and_a_falling_one_the_release() {
        let follower = AttackRelease::new(0.001, 0.1, 48_000.0);
        let up = follower.rise(0.0, 1.0);
        let down = follower.rise(1.0, 0.0);
        assert!(up > 0.0 && up < 1.0);
        assert!(1.0 - down < up, "the release is slower than the attack");
    }

    #[test]
    fn a_falling_gain_takes_the_attack_and_an_instant_attack_lands_on_the_target() {
        let follower = AttackRelease::new(0.0, 0.05, 48_000.0);
        assert_eq!(follower.fall(1.0, 0.25), 0.25);
        let back = follower.fall(0.25, 1.0);
        assert!(back > 0.25 && back < 1.0);
    }

    #[test]
    fn a_follower_settles_on_its_target_without_overshoot() {
        let follower = AttackRelease::new(0.002, 0.02, 48_000.0);
        let mut value = 0.0;
        for _ in 0..48_000 {
            value = follower.rise(value, 0.5);
            assert!(value <= 0.5);
        }
        assert!((value - 0.5).abs() < 1e-9);
    }

    #[test]
    fn bad_times_follow_at_once() {
        for seconds in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(AttackRelease::new(seconds, seconds, 48_000.0).rise(0.0, 1.0), 1.0);
        }
    }
}
