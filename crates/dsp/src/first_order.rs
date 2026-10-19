// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! First-order (6 dB/octave) filters: low-pass, high-pass and shelves, from the analog
//! prototypes by the bilinear transform with the corner prewarped, so it lands where it is set.

use crate::{flush, usable_frequency, usable_sample_rate};

/// `y[n] = b0 x[n] + b1 x[n-1] - a1 y[n-1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FirstOrderCoefficients {
    /// Gain of the current input.
    pub b0: f64,
    /// Gain of the previous input.
    pub b1: f64,
    /// Gain of the previous output, subtracted.
    pub a1: f64,
}

impl Default for FirstOrderCoefficients {
    /// Passes the signal through unchanged.
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// The prewarped analog frequency of `frequency`, in units where the bilinear transform is
/// `s = (1 - z^-1) / (1 + z^-1)`.
fn warp(sample_rate: f64, frequency: f64) -> f64 {
    let sample_rate = usable_sample_rate(sample_rate);
    (std::f64::consts::PI * usable_frequency(frequency, sample_rate) / sample_rate).tan()
}

/// A gain factor made usable: positive and finite, at most 60 dB either way.
fn usable_gain(gain: f64) -> f64 {
    if gain.is_finite() { gain.clamp(1e-3, 1e3) } else { 1.0 }
}

impl FirstOrderCoefficients {
    /// Coefficients that pass the signal through unchanged.
    pub const IDENTITY: Self = Self { b0: 1.0, b1: 0.0, a1: 0.0 };

    /// The digital filter of the analog `(n1 s + n0) / (s + d0)`.
    fn bilinear(n1: f64, n0: f64, d0: f64) -> Self {
        let norm = 1.0 + d0;
        let c = Self { b0: (n0 + n1) / norm, b1: (n0 - n1) / norm, a1: (d0 - 1.0) / norm };
        if [c.b0, c.b1, c.a1].iter().all(|v| v.is_finite()) && c.a1.abs() < 1.0 { c } else { Self::IDENTITY }
    }

    /// Low-pass, -3 dB at `frequency`.
    pub fn lowpass(sample_rate: f64, frequency: f64) -> Self {
        let w = warp(sample_rate, frequency);
        Self::bilinear(0.0, w, w)
    }

    /// High-pass, -3 dB at `frequency`.
    pub fn highpass(sample_rate: f64, frequency: f64) -> Self {
        let w = warp(sample_rate, frequency);
        Self::bilinear(1.0, 0.0, w)
    }

    /// Unity at low frequencies and `gain` (a linear factor) at high ones; half way in decibels
    /// at `frequency`.
    pub fn high_shelf(sample_rate: f64, frequency: f64, gain: f64) -> Self {
        let (w, root) = (warp(sample_rate, frequency), usable_gain(gain).sqrt());
        // gain * (s + w / root) / (s + w * root)
        Self::bilinear(root * root, root * w, w * root)
    }

    /// `gain` (a linear factor) at low frequencies and unity at high ones; half way in decibels
    /// at `frequency`.
    pub fn low_shelf(sample_rate: f64, frequency: f64, gain: f64) -> Self {
        let (w, root) = (warp(sample_rate, frequency), usable_gain(gain).sqrt());
        // (s + w * root) / (s + w / root)
        Self::bilinear(1.0, w * root, w / root)
    }

    /// Gain at `frequency` (linear), for tests and response curves.
    pub fn magnitude(&self, sample_rate: f64, frequency: f64) -> f64 {
        let w = std::f64::consts::TAU * frequency / usable_sample_rate(sample_rate);
        let num = (self.b0 + self.b1 * w.cos(), -self.b1 * w.sin());
        let den = (1.0 + self.a1 * w.cos(), -self.a1 * w.sin());
        num.0.hypot(num.1) / den.0.hypot(den.1)
    }
}

/// One first-order section, `f64` state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FirstOrder {
    /// The filter it runs.
    c: FirstOrderCoefficients,
    /// The one state variable (transposed direct form II).
    z: f64,
}

impl FirstOrder {
    /// A silent section running `coefficients`.
    pub fn new(coefficients: FirstOrderCoefficients) -> Self {
        Self { c: coefficients, z: 0.0 }
    }

    /// Changes the coefficients and keeps the state.
    pub fn set(&mut self, coefficients: FirstOrderCoefficients) {
        self.c = coefficients;
    }

    /// The coefficients it runs.
    pub fn coefficients(&self) -> FirstOrderCoefficients {
        self.c
    }

    /// One sample through the filter.
    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let y = self.c.b0 * x + self.z;
        self.z = flush(self.c.b1 * x - self.c.a1 * y);
        y
    }

    /// Silences the filter and keeps the coefficients.
    pub fn reset(&mut self) {
        self.z = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biquad::tests::{SR, measured_gain};

    fn db(gain: f64) -> f64 {
        20.0 * gain.log10()
    }

    #[test]
    fn lowpass_and_highpass_are_3_db_down_at_the_corner() {
        let lp = FirstOrderCoefficients::lowpass(SR, 1_000.0);
        let hp = FirstOrderCoefficients::highpass(SR, 1_000.0);
        assert!((db(lp.magnitude(SR, 1_000.0)) + 3.01).abs() < 0.01);
        assert!((db(hp.magnitude(SR, 1_000.0)) + 3.01).abs() < 0.01);
        assert!((lp.magnitude(SR, 0.0) - 1.0).abs() < 1e-9);
        assert!(lp.magnitude(SR, SR / 2.0) < 1e-6);
        // 6 dB per octave well above the corner.
        assert!((db(lp.magnitude(SR, 8_000.0)) - db(lp.magnitude(SR, 4_000.0)) + 6.0).abs() < 1.0);
        let mut filter = FirstOrder::new(lp);
        let measured = measured_gain(1_000.0, |x| filter.process(x));
        assert!((measured - lp.magnitude(SR, 1_000.0)).abs() < 1e-3, "{measured}");
    }

    #[test]
    fn shelves_reach_their_gain_and_are_half_way_at_the_frequency() {
        let gain = 0.2;
        let high = FirstOrderCoefficients::high_shelf(SR, 1_000.0, gain);
        let low = FirstOrderCoefficients::low_shelf(SR, 1_000.0, gain);
        assert!((high.magnitude(SR, 0.0) - 1.0).abs() < 1e-9);
        assert!((high.magnitude(SR, 20_000.0) - gain).abs() < 0.01);
        assert!((low.magnitude(SR, 0.0) - gain).abs() < 1e-9);
        assert!((low.magnitude(SR, 20_000.0) - 1.0).abs() < 0.01);
        for shelf in [high, low] {
            assert!((db(shelf.magnitude(SR, 1_000.0)) - db(gain) / 2.0).abs() < 0.05);
        }
        assert_eq!(FirstOrderCoefficients::high_shelf(SR, 1_000.0, 1.0).magnitude(SR, 5_000.0), 1.0);
    }

    #[test]
    fn hostile_settings_stay_stable() {
        for f in [f64::NAN, -1.0, 0.0, 1e9] {
            for c in [
                FirstOrderCoefficients::lowpass(SR, f),
                FirstOrderCoefficients::highpass(SR, f),
                FirstOrderCoefficients::high_shelf(SR, f, f),
                FirstOrderCoefficients::low_shelf(f, 1_000.0, f64::INFINITY),
            ] {
                assert!(c.a1.abs() < 1.0 && c.b0.is_finite() && c.b1.is_finite(), "{c:?}");
            }
        }
        let mut filter = FirstOrder::new(FirstOrderCoefficients::lowpass(SR, 100.0));
        filter.process(f64::NAN);
        assert_eq!(filter.process(0.0), 0.0);
    }
}
