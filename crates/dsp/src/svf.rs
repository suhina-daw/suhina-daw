// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A state-variable filter integrated with the trapezoidal rule (a "topology-preserving"
//! SVF, as in Andrew Simper's technical papers for Cytomic, 2013). It computes a band-pass and
//! a low-pass at once; mixing them with the input gives every second-order response. With the
//! corner prewarped, each response is exactly the bilinear transform of its analog prototype,
//! the same as the cookbook biquads ([`Coefficients`](crate::Coefficients)), but its state stays
//! meaningful when the coefficients change, so a frequency, gain or Q can move while sound
//! passes without the bumps a direct-form biquad makes.

use crate::{flush, usable_frequency, usable_sample_rate};

/// How one section is tuned and what it mixes: `m0 · input + m1 · band-pass + m2 · low-pass`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvfCoefficients {
    /// The prewarped corner, `tan(π f / fs)`.
    g: f64,
    /// Damping, `1 / Q`.
    k: f64,
    m0: f64,
    m1: f64,
    m2: f64,
    a1: f64,
    a2: f64,
    a3: f64,
}

impl Default for SvfCoefficients {
    /// Passes the signal through unchanged.
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// A Q made usable: positive and finite, from 0.01 to 1000.
fn usable_q(q: f64) -> f64 {
    if q.is_finite() { q.clamp(0.01, 1_000.0) } else { std::f64::consts::FRAC_1_SQRT_2 }
}

/// The amplitude factor of a shelf or bell, `10^(dB / 40)` (its square is the gain).
fn amplitude(gain_db: f64) -> f64 {
    let db = if gain_db.is_finite() { gain_db.clamp(-60.0, 60.0) } else { 0.0 };
    10f64.powf(db / 40.0)
}

impl SvfCoefficients {
    /// Coefficients that pass the signal through unchanged.
    pub const IDENTITY: Self = Self { g: 0.0, k: 1.0, m0: 1.0, m1: 0.0, m2: 0.0, a1: 1.0, a2: 0.0, a3: 0.0 };

    /// A section with its corner at `frequency × scale` (shelves move their corner so the
    /// half-gain point stays at `frequency`), damping `1 / q` and the mix `m`.
    fn new(sample_rate: f64, frequency: f64, scale: f64, q: f64, m: [f64; 3]) -> Self {
        let sample_rate = usable_sample_rate(sample_rate);
        let corner = usable_frequency(frequency, sample_rate);
        let g = (std::f64::consts::PI * corner / sample_rate).tan() * scale;
        let k = 1.0 / usable_q(q);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let c = Self { g, k, m0: m[0], m1: m[1], m2: m[2], a1, a2: g * a1, a3: g * g * a1 };
        let all = [c.g, c.k, c.m0, c.m1, c.m2, c.a1, c.a2, c.a3];
        if all.iter().all(|v| v.is_finite()) && g > 0.0 { c } else { Self::IDENTITY }
    }

    /// 12 dB/octave low-pass with its corner at `frequency`.
    pub fn lowpass(sample_rate: f64, frequency: f64, q: f64) -> Self {
        Self::new(sample_rate, frequency, 1.0, q, [0.0, 0.0, 1.0])
    }

    /// 12 dB/octave high-pass with its corner at `frequency`.
    pub fn highpass(sample_rate: f64, frequency: f64, q: f64) -> Self {
        let k = 1.0 / usable_q(q);
        Self::new(sample_rate, frequency, 1.0, q, [1.0, -k, -1.0])
    }

    /// Notch at `frequency`, its width set by `q`.
    pub fn notch(sample_rate: f64, frequency: f64, q: f64) -> Self {
        let k = 1.0 / usable_q(q);
        Self::new(sample_rate, frequency, 1.0, q, [1.0, -k, 0.0])
    }

    /// Bell boosting or cutting `gain_db` at `frequency`: the cookbook peaking filter, whose Q
    /// sets the width between its half-gain points.
    pub fn bell(sample_rate: f64, frequency: f64, q: f64, gain_db: f64) -> Self {
        let a = amplitude(gain_db);
        let q = usable_q(q) * a;
        Self::new(sample_rate, frequency, 1.0, q, [1.0, (a * a - 1.0) / q, 0.0])
    }

    /// Shelf boosting or cutting `gain_db` below `frequency`, half way (in decibels) at it.
    pub fn low_shelf(sample_rate: f64, frequency: f64, q: f64, gain_db: f64) -> Self {
        let a = amplitude(gain_db);
        let k = 1.0 / usable_q(q);
        Self::new(sample_rate, frequency, 1.0 / a.sqrt(), q, [1.0, k * (a - 1.0), a * a - 1.0])
    }

    /// Shelf boosting or cutting `gain_db` above `frequency`, half way (in decibels) at it.
    pub fn high_shelf(sample_rate: f64, frequency: f64, q: f64, gain_db: f64) -> Self {
        let a = amplitude(gain_db);
        let k = 1.0 / usable_q(q);
        Self::new(sample_rate, frequency, a.sqrt(), q, [a * a, k * a * (1.0 - a), 1.0 - a * a])
    }

    /// Gain at `frequency` (linear, 1.0 = unchanged), for tests and for drawing response curves.
    pub fn magnitude(&self, sample_rate: f64, frequency: f64) -> f64 {
        if self.g <= 0.0 {
            return self.m0.abs();
        }
        // The digital frequency on the prototype's axis, through the same prewarp.
        let w = (std::f64::consts::PI * frequency / usable_sample_rate(sample_rate)).tan() / self.g;
        // D = p² + k p + 1 and N = m0 D + m1 p + m2 at p = jw.
        let (d_re, d_im) = (1.0 - w * w, self.k * w);
        let (n_re, n_im) = (self.m0 * d_re + self.m2, self.m0 * d_im + self.m1 * w);
        n_re.hypot(n_im) / d_re.hypot(d_im)
    }
}

/// One section, `f64` state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Svf {
    c: SvfCoefficients,
    /// The two integrators' states.
    ic1: f64,
    ic2: f64,
}

impl Svf {
    /// A silent section running `coefficients`.
    pub fn new(coefficients: SvfCoefficients) -> Self {
        Self { c: coefficients, ic1: 0.0, ic2: 0.0 }
    }

    /// Changes the coefficients and keeps the state, so the sound goes on smoothly.
    pub fn set(&mut self, coefficients: SvfCoefficients) {
        self.c = coefficients;
    }

    /// The coefficients it runs.
    pub fn coefficients(&self) -> SvfCoefficients {
        self.c
    }

    /// One sample through the section.
    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let c = &self.c;
        let v3 = x - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);
        c.m0 * x + c.m1 * v1 + c.m2 * v2
    }

    /// Silences the section and keeps the coefficients.
    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Coefficients;

    const SR: f64 = 44_100.0;

    fn db(gain: f64) -> f64 {
        20.0 * gain.log10()
    }

    /// The gain of `f` at `hz`, measured by running a sine through it.
    fn measured(c: SvfCoefficients, hz: f64) -> f64 {
        let mut svf = Svf::new(c);
        let n = 44_100;
        let (mut re, mut im) = (0.0, 0.0);
        for i in 0..n {
            let phase = std::f64::consts::TAU * hz * i as f64 / SR;
            let y = svf.process(phase.sin());
            if i >= n / 2 {
                re += y * phase.sin();
                im += y * phase.cos();
            }
        }
        2.0 * re.hypot(im) / (n / 2) as f64
    }

    #[test]
    fn every_response_is_the_cookbook_biquad() {
        let frequencies = [20.0, 100.0, 500.0, 1_000.0, 2_000.0, 8_000.0, 15_000.0, 21_000.0];
        for (q, gain) in [(0.1, 12.0), (std::f64::consts::FRAC_1_SQRT_2, 12.0), (2.0, -9.0), (18.0, 15.0)] {
            let pairs = [
                (SvfCoefficients::lowpass(SR, 1_000.0, q), Coefficients::lowpass(SR, 1_000.0, q)),
                (SvfCoefficients::highpass(SR, 1_000.0, q), Coefficients::highpass(SR, 1_000.0, q)),
                (SvfCoefficients::bell(SR, 1_000.0, q, gain), Coefficients::peak(SR, 1_000.0, q, gain)),
                (SvfCoefficients::low_shelf(SR, 1_000.0, q, gain), Coefficients::low_shelf(SR, 1_000.0, q, gain)),
                (SvfCoefficients::high_shelf(SR, 1_000.0, q, gain), Coefficients::high_shelf(SR, 1_000.0, q, gain)),
            ];
            for (i, (svf, biquad)) in pairs.iter().enumerate() {
                for hz in frequencies {
                    let (a, b) = (db(svf.magnitude(SR, hz)), db(biquad.magnitude(SR, hz)));
                    assert!((a - b).abs() < 1e-6, "type {i}, q {q}, {hz} Hz: {a} against {b}");
                }
            }
            // The notch has no cookbook twin here; its prototype is (p² + 1) / (p² + p/Q + 1).
            let notch = SvfCoefficients::notch(SR, 1_000.0, q);
            assert!(notch.magnitude(SR, 1_000.0) < 1e-9);
        }
    }

    #[test]
    fn the_filter_does_what_its_magnitude_says() {
        for (c, hz) in [
            (SvfCoefficients::bell(SR, 1_000.0, 2.0, 12.0), 1_000.0),
            (SvfCoefficients::low_shelf(SR, 300.0, std::f64::consts::FRAC_1_SQRT_2, -10.0), 100.0),
            (SvfCoefficients::high_shelf(SR, 3_000.0, std::f64::consts::FRAC_1_SQRT_2, 8.0), 9_000.0),
            (SvfCoefficients::highpass(SR, 500.0, 6.0), 500.0),
            (SvfCoefficients::notch(SR, 1_000.0, std::f64::consts::FRAC_1_SQRT_2), 700.0),
        ] {
            let (expected, got) = (db(c.magnitude(SR, hz)), db(measured(c, hz)));
            assert!((expected - got).abs() < 0.01, "{c:?} at {hz} Hz: {got} against {expected}");
        }
    }

    #[test]
    fn a_sweeping_corner_does_not_blow_up() {
        // The corner jumps between 30 Hz and 18 kHz every 32 samples at Q 18: the state stays
        // bounded and the output never runs away.
        let mut svf = Svf::default();
        let mut peak = 0.0f64;
        for i in 0..48_000 {
            if i % 32 == 0 {
                let hz = if (i / 32) % 2 == 0 { 30.0 } else { 18_000.0 };
                svf.set(SvfCoefficients::bell(SR, hz, 18.0, 15.0));
            }
            let y = svf.process((i as f64 * 0.05).sin() * 0.5);
            peak = peak.max(y.abs());
        }
        assert!(peak.is_finite() && peak < 50.0, "{peak}");
    }

    #[test]
    fn broken_input_and_settings_are_harmless() {
        let mut svf = Svf::new(SvfCoefficients::bell(SR, f64::NAN, f64::INFINITY, f64::NAN));
        svf.process(f64::NAN);
        let out: Vec<f64> = (0..100).map(|i| svf.process((i as f64 * 0.1).sin())).collect();
        assert!(out.iter().all(|v| v.is_finite()));
        assert!(SvfCoefficients::lowpass(SR, 0.0, 0.7).magnitude(SR, 100.0) > 0.0);
    }
}
