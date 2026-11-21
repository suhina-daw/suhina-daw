// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Second-order IIR filters ("biquads"). The coefficient formulas are the standard ones from
//! Robert Bristow-Johnson's *Audio EQ Cookbook*, a freely published reference; the code is our
//! own.

use crate::{flush, usable_frequency, usable_sample_rate};

/// Q of a second-order Butterworth section: maximally flat, -3 dB at the corner frequency.
pub const BUTTERWORTH_Q: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// Normalized biquad coefficients (`a0` divided out).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coefficients {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Default for Coefficients {
    /// Passes the signal through unchanged.
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Shared terms of the cookbook formulas.
struct Terms {
    cos: f64,
    alpha: f64,
}

impl Terms {
    fn new(sample_rate: f64, frequency: f64, q: f64) -> Self {
        let sample_rate = usable_sample_rate(sample_rate);
        let w0 = std::f64::consts::TAU * usable_frequency(frequency, sample_rate) / sample_rate;
        let q = if q.is_finite() { q.clamp(0.025, 100.0) } else { BUTTERWORTH_Q };
        Self { cos: w0.cos(), alpha: w0.sin() / (2.0 * q) }
    }
}

/// `10^(gain_db / 40)`, the cookbook's `A`; non-finite or extreme gains are limited to ±48 dB.
fn shelf_amplitude(gain_db: f64) -> f64 {
    let gain_db = if gain_db.is_finite() { gain_db.clamp(-48.0, 48.0) } else { 0.0 };
    10.0f64.powf(gain_db / 40.0)
}

impl Coefficients {
    pub const IDENTITY: Self = Self { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

    fn normalized(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        let c = Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 };
        if [c.b0, c.b1, c.b2, c.a1, c.a2].iter().all(|v| v.is_finite()) { c } else { Self::IDENTITY }
    }

    /// 12 dB/octave low-pass with its corner at `frequency`.
    pub fn lowpass(sample_rate: f64, frequency: f64, q: f64) -> Self {
        let Terms { cos, alpha } = Terms::new(sample_rate, frequency, q);
        let b = (1.0 - cos) / 2.0;
        Self::normalized(b, 1.0 - cos, b, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
    }

    /// 12 dB/octave high-pass with its corner at `frequency`.
    pub fn highpass(sample_rate: f64, frequency: f64, q: f64) -> Self {
        let Terms { cos, alpha } = Terms::new(sample_rate, frequency, q);
        let b = (1.0 + cos) / 2.0;
        Self::normalized(b, -(1.0 + cos), b, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
    }

    /// Band-pass around `frequency` with 0 dB at its peak.
    pub fn bandpass(sample_rate: f64, frequency: f64, q: f64) -> Self {
        let Terms { cos, alpha } = Terms::new(sample_rate, frequency, q);
        Self::normalized(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
    }

    /// Q of a band-pass or bell `octaves` wide (between its -3 dB points).
    pub fn q_for_octaves(octaves: f64) -> f64 {
        let octaves = if octaves.is_finite() { octaves.clamp(0.01, 10.0) } else { 1.0 };
        let ratio = 2.0f64.powf(octaves);
        ratio.sqrt() / (ratio - 1.0)
    }

    /// Bell boosting or cutting `gain_db` around `frequency`.
    pub fn peak(sample_rate: f64, frequency: f64, q: f64, gain_db: f64) -> Self {
        let Terms { cos, alpha } = Terms::new(sample_rate, frequency, q);
        let a = shelf_amplitude(gain_db);
        Self::normalized(1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a)
    }

    /// Shelf boosting or cutting `gain_db` below `frequency`.
    pub fn low_shelf(sample_rate: f64, frequency: f64, q: f64, gain_db: f64) -> Self {
        let Terms { cos, alpha } = Terms::new(sample_rate, frequency, q);
        let a = shelf_amplitude(gain_db);
        let k = 2.0 * a.sqrt() * alpha;
        Self::normalized(
            a * ((a + 1.0) - (a - 1.0) * cos + k),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - k),
            (a + 1.0) + (a - 1.0) * cos + k,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - k,
        )
    }

    /// Shelf boosting or cutting `gain_db` above `frequency`.
    pub fn high_shelf(sample_rate: f64, frequency: f64, q: f64, gain_db: f64) -> Self {
        let Terms { cos, alpha } = Terms::new(sample_rate, frequency, q);
        let a = shelf_amplitude(gain_db);
        let k = 2.0 * a.sqrt() * alpha;
        Self::normalized(
            a * ((a + 1.0) + (a - 1.0) * cos + k),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
            a * ((a + 1.0) + (a - 1.0) * cos - k),
            (a + 1.0) - (a - 1.0) * cos + k,
            2.0 * ((a - 1.0) - (a + 1.0) * cos),
            (a + 1.0) - (a - 1.0) * cos - k,
        )
    }

    /// Gain of the filter at `frequency` (linear, 1.0 = unchanged), for tests and for drawing
    /// response curves.
    pub fn magnitude(&self, sample_rate: f64, frequency: f64) -> f64 {
        let sample_rate = usable_sample_rate(sample_rate);
        let w = std::f64::consts::TAU * frequency / sample_rate;
        let (c1, s1) = (w.cos(), w.sin());
        let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
        // H(e^jw) = (b0 + b1 e^-jw + b2 e^-2jw) / (1 + a1 e^-jw + a2 e^-2jw)
        let num = (self.b0 + self.b1 * c1 + self.b2 * c2, -(self.b1 * s1 + self.b2 * s2));
        let den = (1.0 + self.a1 * c1 + self.a2 * c2, -(self.a1 * s1 + self.a2 * s2));
        (num.0.hypot(num.1) / den.0.hypot(den.1)).max(0.0)
    }
}

/// One biquad section in transposed direct form II, with `f64` state so low corner frequencies
/// stay accurate at high sample rates.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Biquad {
    c: Coefficients,
    z1: f64,
    z2: f64,
}

impl Biquad {
    pub fn new(coefficients: Coefficients) -> Self {
        Self { c: coefficients, z1: 0.0, z2: 0.0 }
    }

    /// Changes the coefficients and keeps the state, so a filter can be retuned while it runs.
    pub fn set(&mut self, coefficients: Coefficients) {
        self.c = coefficients;
    }

    pub fn coefficients(&self) -> Coefficients {
        self.c
    }

    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let c = &self.c;
        let y = c.b0 * x + self.z1;
        self.z1 = flush(c.b1 * x - c.a1 * y + self.z2);
        self.z2 = flush(c.b2 * x - c.a2 * y);
        y
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const SR: f64 = 48_000.0;

    /// Gain of a unit sine at `frequency` through `process` once it has settled, from the RMS
    /// level, so it does not depend on where the samples fall on the wave.
    pub(crate) fn measured_gain(frequency: f64, mut process: impl FnMut(f64) -> f64) -> f64 {
        let frames = SR as usize;
        let mut sum = 0.0f64;
        for n in 0..frames {
            let y = process((std::f64::consts::TAU * frequency * n as f64 / SR).sin());
            if n >= frames / 2 {
                sum += y * y;
            }
        }
        (sum / (frames / 2) as f64 * 2.0).sqrt()
    }

    fn db(gain: f64) -> f64 {
        20.0 * gain.log10()
    }

    #[test]
    fn lowpass_and_highpass_responses() {
        let lp = Coefficients::lowpass(SR, 1_000.0, BUTTERWORTH_Q);
        assert!((lp.magnitude(SR, 0.0) - 1.0).abs() < 1e-9);
        assert!((db(lp.magnitude(SR, 1_000.0)) + 3.01).abs() < 0.05);
        assert!(db(lp.magnitude(SR, 10_000.0)) < -38.0);
        let hp = Coefficients::highpass(SR, 1_000.0, BUTTERWORTH_Q);
        assert!(hp.magnitude(SR, 0.0) < 1e-9);
        assert!((db(hp.magnitude(SR, 1_000.0)) + 3.01).abs() < 0.05);
        assert!((hp.magnitude(SR, 20_000.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn running_filter_matches_its_response() {
        let c = Coefficients::lowpass(SR, 500.0, BUTTERWORTH_Q);
        for f in [100.0, 500.0, 2_000.0] {
            let mut filter = Biquad::new(c);
            let measured = measured_gain(f, |x| filter.process(x));
            assert!((measured - c.magnitude(SR, f)).abs() < 0.01, "{f} Hz: {measured}");
        }
    }

    #[test]
    fn peak_and_shelves_reach_their_gain() {
        let peak = Coefficients::peak(SR, 2_000.0, 1.0, 6.0);
        assert!((db(peak.magnitude(SR, 2_000.0)) - 6.0).abs() < 0.01);
        assert!(db(peak.magnitude(SR, 50.0)).abs() < 0.05);
        let low = Coefficients::low_shelf(SR, 200.0, BUTTERWORTH_Q, -12.0);
        assert!((db(low.magnitude(SR, 10.0)) + 12.0).abs() < 0.1);
        assert!(db(low.magnitude(SR, 10_000.0)).abs() < 0.1);
        let high = Coefficients::high_shelf(SR, 5_000.0, BUTTERWORTH_Q, 9.0);
        assert!((db(high.magnitude(SR, 22_000.0)) - 9.0).abs() < 0.2);
        assert!(db(high.magnitude(SR, 50.0)).abs() < 0.05);
    }

    #[test]
    fn bandpass_peaks_at_its_frequency() {
        let q = Coefficients::q_for_octaves(1.0);
        assert!((q - 1.414).abs() < 0.001, "{q}");
        let bp = Coefficients::bandpass(SR, 1_000.0, q);
        assert!((bp.magnitude(SR, 1_000.0) - 1.0).abs() < 1e-6);
        // One octave wide: the -3 dB points are half an octave either side.
        let edge = bp.magnitude(SR, 1_000.0 * 2.0f64.sqrt());
        assert!((db(edge) + 3.01).abs() < 0.2, "{}", db(edge));
        assert!(bp.magnitude(SR, 0.0) < 1e-9);
    }

    #[test]
    fn hostile_parameters_give_a_stable_filter() {
        for (f, q, sr) in [(f64::NAN, 1.0, SR), (1e9, 1.0, SR), (-5.0, 0.0, SR), (1_000.0, f64::NAN, 0.0)] {
            let mut filter = Biquad::new(Coefficients::lowpass(sr, f, q));
            let mut out = 0.0;
            for n in 0..10_000 {
                out = filter.process(if n % 2 == 0 { 1.0 } else { -1.0 });
                assert!(out.is_finite());
            }
            assert!(out.abs() < 100.0);
        }
        let c = Coefficients::peak(SR, 1_000.0, 1.0, f64::INFINITY);
        assert!(c.magnitude(SR, 1_000.0).is_finite());
    }

    #[test]
    fn broken_input_does_not_stick() {
        let mut filter = Biquad::new(Coefficients::lowpass(SR, 250.0, 1.3));
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e30, -1e300] {
            filter.process(x);
        }
        let mut out = 0.0;
        for n in 0..100 {
            out = filter.process((n as f64 * 0.05).sin());
        }
        assert!(out.is_finite() && out.abs() < 2.0, "{out}");
    }

    #[test]
    fn decaying_state_flushes_to_zero() {
        let mut filter = Biquad::new(Coefficients::lowpass(SR, 100.0, BUTTERWORTH_Q));
        filter.process(1.0);
        for _ in 0..200_000 {
            filter.process(0.0);
        }
        assert_eq!((filter.z1, filter.z2), (0.0, 0.0));
    }
}
