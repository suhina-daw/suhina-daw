// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Low-frequency oscillator waveforms.
//!
//! The waveforms are pure functions of a position in cycles, so a device can derive the position
//! from the song position (for tempo-synced modulation that lands on the same spot however
//! playback got there) or accumulate it itself. The random waveform is a hash of the cycle
//! number, so it is repeatable too.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Sine,
    Triangle,
    SawUp,
    SawDown,
    Square,
    /// A new random value at the start of every cycle, held for the cycle.
    Random,
}

impl Shape {
    /// Every shape, in the order devices list them.
    pub const ALL: [Shape; 6] =
        [Shape::Sine, Shape::Triangle, Shape::SawUp, Shape::SawDown, Shape::Square, Shape::Random];
}

/// The waveform at `cycles` (any real number; the integer part counts cycles), from -1.0 to 1.0.
/// Every shape except the saws starts at its centre, rising, so a phase offset of a quarter
/// cycle reaches the top. Non-finite input gives 0.0.
pub fn wave(shape: Shape, cycles: f64) -> f64 {
    if !cycles.is_finite() {
        return 0.0;
    }
    let phase = cycles - cycles.floor();
    match shape {
        Shape::Sine => (std::f64::consts::TAU * phase).sin(),
        Shape::Triangle => {
            if phase < 0.25 {
                4.0 * phase
            } else if phase < 0.75 {
                2.0 - 4.0 * phase
            } else {
                4.0 * phase - 4.0
            }
        }
        Shape::SawUp => 2.0 * phase - 1.0,
        Shape::SawDown => 1.0 - 2.0 * phase,
        Shape::Square => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        Shape::Random => random(cycles.floor()),
    }
}

/// A smoothly wandering random curve from -1.0 to 1.0 at `cycles`: a new random value at the
/// start of every cycle (the same ones [`Shape::Random`] holds), reached from the one before by
/// a cosine glide over the cycle, so it never jumps. Non-finite input gives 0.0.
pub fn wander(cycles: f64) -> f64 {
    if !cycles.is_finite() {
        return 0.0;
    }
    let cycle = cycles.floor();
    let glide = (1.0 - (std::f64::consts::PI * (cycles - cycle)).cos()) / 2.0;
    let (from, to) = (random(cycle - 1.0), random(cycle));
    from + (to - from) * glide
}

/// A repeatable pseudo-random value from -1.0 to 1.0 for cycle number `cycle` (splitmix64).
fn random(cycle: f64) -> f64 {
    let mut x = (cycle as i64 as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    (x >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_have_their_values() {
        let at = |shape, c| wave(shape, c);
        assert!(at(Shape::Sine, 0.25) > 0.999_999);
        assert!(at(Shape::Sine, 0.0).abs() < 1e-12);
        assert_eq!(at(Shape::Triangle, 0.25), 1.0);
        assert_eq!(at(Shape::Triangle, 0.75), -1.0);
        assert_eq!(at(Shape::Triangle, 0.125), 0.5);
        assert_eq!(at(Shape::SawUp, 0.0), -1.0);
        assert_eq!(at(Shape::SawDown, 0.5), 0.0);
        assert_eq!(at(Shape::Square, 0.49), 1.0);
        assert_eq!(at(Shape::Square, 0.51), -1.0);
        // Whole cycles and negative positions wrap.
        assert_eq!(at(Shape::Triangle, 3.25), 1.0);
        assert_eq!(at(Shape::Triangle, -0.75), 1.0);
    }

    #[test]
    fn every_shape_stays_in_range() {
        for shape in Shape::ALL {
            for n in -2_000..2_000 {
                let v = wave(shape, n as f64 * 0.0137);
                assert!((-1.0..=1.0).contains(&v), "{shape:?} {n}: {v}");
            }
            assert_eq!(wave(shape, f64::NAN), 0.0);
            assert_eq!(wave(shape, f64::INFINITY), 0.0);
        }
    }

    #[test]
    fn wander_glides_through_the_random_values() {
        assert_eq!(wander(4.0), wave(Shape::Random, 3.0));
        assert!((wander(5.0 - 1e-9) - wave(Shape::Random, 4.0)).abs() < 1e-6);
        let mut last = wander(0.0);
        for n in 1..10_000 {
            let v = wander(n as f64 * 0.001);
            assert!((-1.0..=1.0).contains(&v));
            assert!((v - last).abs() < 0.01, "{n}: a jump from {last} to {v}");
            last = v;
        }
        assert_eq!(wander(f64::NAN), 0.0);
    }

    #[test]
    fn random_holds_per_cycle_and_repeats() {
        assert_eq!(wave(Shape::Random, 5.1), wave(Shape::Random, 5.9));
        assert_ne!(wave(Shape::Random, 5.5), wave(Shape::Random, 6.5));
        let values: Vec<f64> = (0..1_000).map(|c| wave(Shape::Random, f64::from(c))).collect();
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        assert!(mean.abs() < 0.1, "{mean}");
        assert!(values.iter().any(|&v| v > 0.9) && values.iter().any(|&v| v < -0.9));
    }
}
