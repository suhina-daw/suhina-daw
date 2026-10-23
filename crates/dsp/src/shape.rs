// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Waveshapers: memoryless curves that add harmonics. Run them oversampled
//! ([`Oversampler2x`](crate::Oversampler2x)) to keep the harmonics from aliasing.

/// How a shaper limits large values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    /// `tanh`: rounds off gradually, mostly low odd harmonics.
    Soft,
    /// Clips flat at ±1: sharper, more and higher harmonics.
    Hard,
}

impl Curve {
    /// The curve at `v`: about `v` for small values, never beyond ±1. Non-finite input gives 0.
    #[inline]
    pub fn apply(self, v: f64) -> f64 {
        if !v.is_finite() {
            return 0.0;
        }
        match self {
            Self::Soft => v.tanh(),
            Self::Hard => v.clamp(-1.0, 1.0),
        }
    }

    /// An asymmetric version: the negative half limits earlier, at `-1 / (1 + bias)`, which
    /// adds even harmonics (and a DC offset the caller removes). Both halves start with slope
    /// 1 and the curve always rises. `bias` from 0 (symmetric) to 1; other values are limited
    /// to that range.
    #[inline]
    pub fn asymmetric(self, v: f64, bias: f64) -> f64 {
        let k = 1.0 + if bias.is_finite() { bias.clamp(0.0, 1.0) } else { 0.0 };
        if v >= 0.0 { self.apply(v) } else { self.apply(k * v) / k }
    }
}

/// Linear up to `knee`, then bending towards ±1: past the knee the distance to 1 shrinks
/// exponentially, starting with slope 1, so the curve has no corner. `knee` 0 is `1 - e^-v`
/// from the origin, 1 is a hard clip; it is limited to 0 … 1. Non-finite input gives 0.
#[inline]
pub fn knee_clip(v: f64, knee: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    let knee = if knee.is_finite() { knee.clamp(0.0, 1.0) } else { 1.0 };
    let a = v.abs();
    if a <= knee {
        return v;
    }
    let room = 1.0 - knee;
    if room <= 0.0 {
        return v.signum();
    }
    (1.0 - room * (-(a - knee) / room).exp()).copysign(v)
}

/// A quarter sine: `sin(π/2 · v)` up to ±1, flat beyond. Slope π/2 at the origin.
#[inline]
pub fn soft_sine(v: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    (std::f64::consts::FRAC_PI_2 * v.clamp(-1.0, 1.0)).sin()
}

/// A sine fold: `sin(π/2 · v)` without a limit, so louder input folds back (peaks at odd
/// values, zero at even ones).
#[inline]
pub fn sine_fold(v: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    (std::f64::consts::FRAC_PI_2 * v).sin()
}

/// A steep sigmoid: `g·v / (1 + |g·v|^n)^(1/n)`, slope `gain` at the origin, approaching ±1.
/// A larger `sharpness` (n) turns the corner more abruptly.
#[inline]
pub fn sigmoid(v: f64, gain: f64, sharpness: f64) -> f64 {
    let x = gain * v;
    if !x.is_finite() || sharpness <= 0.0 {
        return 0.0;
    }
    (x / (1.0 + x.abs().powf(sharpness)).powf(1.0 / sharpness)).clamp(-1.0, 1.0)
}

/// A curve shaped by six amounts, each 0 … 1: a dead zone around zero, the slope of the
/// straight part, a rounding that adds mostly third harmonics, and a sine ripple on top. Its
/// [`drive`](Self::drive) blends it with the straight line `v`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Waveshaper {
    /// 0: the straight line, 1: the shaped curve.
    pub drive: f64,
    /// Rounding of the curve (third-order).
    pub curve: f64,
    /// Height of the sine ripple.
    pub depth: f64,
    /// Slope of the straight part: 0 is 0.5, ½ is 1, 1 is 1.5.
    pub linear: f64,
    /// Width of the dead zone around zero, up to 0.25.
    pub damp: f64,
    /// How many ripples: 0 is one over ±1, 1 is sixteen.
    pub period: f64,
}

impl Default for Waveshaper {
    /// The straight line.
    fn default() -> Self {
        Self { drive: 1.0, curve: 0.0, depth: 0.0, linear: 0.5, damp: 0.0, period: 0.0 }
    }
}

/// Widest dead zone of a [`Waveshaper`].
const MAX_DAMP: f64 = 0.25;
/// Most ripples of a [`Waveshaper`] across ±1.
const MAX_RIPPLES: f64 = 16.0;
/// Height of a [`Waveshaper`]'s ripple at full depth.
const RIPPLE_HEIGHT: f64 = 0.25;

impl Waveshaper {
    /// The curve at `v`. Non-finite input gives 0.
    #[inline]
    pub fn apply(&self, v: f64) -> f64 {
        if !v.is_finite() {
            return 0.0;
        }
        let unit = |x: f64| if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 };
        let zone = MAX_DAMP * unit(self.damp);
        let x = (v.abs() - zone).max(0.0).copysign(v);
        let y = (0.5 + unit(self.linear)) * x;
        // Monotonic for any rounding: the subtracted term's slope never exceeds 9/8 of y's.
        let mut shaped = y - unit(self.curve) * y * y * y / (3.0 * (1.0 + y * y));
        let ripples = 1.0 + (MAX_RIPPLES - 1.0) * unit(self.period);
        shaped += RIPPLE_HEIGHT * unit(self.depth) * (std::f64::consts::FRAC_PI_2 * ripples * x).sin();
        v + unit(self.drive) * (shaped - v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Amplitude of harmonic `k` when `f` shapes one cycle of a sine of amplitude `a`.
    fn harmonic(f: impl Fn(f64) -> f64, a: f64, k: usize) -> f64 {
        let n = 4_096;
        let (mut re, mut im) = (0.0, 0.0);
        for i in 0..n {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            let y = f(a * t.sin());
            re += y * (k as f64 * t).cos();
            im += y * (k as f64 * t).sin();
        }
        2.0 * re.hypot(im) / n as f64
    }

    #[test]
    fn small_values_pass_and_large_ones_are_limited() {
        for curve in [Curve::Soft, Curve::Hard] {
            assert!((curve.apply(0.001) - 0.001).abs() < 1e-9);
            assert!(curve.apply(100.0) <= 1.0 && curve.apply(-100.0) >= -1.0);
            assert_eq!(curve.apply(f64::NAN), 0.0);
            assert_eq!(curve.asymmetric(f64::INFINITY, 1.0), 0.0);
            // The asymmetric curve always rises, and its negative half stops at -1 / (1 + bias).
            let values: Vec<f64> = (-400..=400).map(|i| curve.asymmetric(f64::from(i) * 0.05, 0.5)).collect();
            assert!(values.windows(2).all(|w| w[1] >= w[0]), "{curve:?}");
            assert!((curve.asymmetric(-100.0, 0.5) + 1.0 / 1.5).abs() < 1e-9);
            assert!((curve.asymmetric(0.001, 0.5) - 0.001).abs() < 1e-6);
            assert!((curve.asymmetric(-0.001, 0.5) + 0.001).abs() < 1e-6);
        }
    }

    #[test]
    fn symmetric_curves_add_odd_harmonics_and_asymmetric_ones_even() {
        let soft = |v| Curve::Soft.apply(v);
        assert!(harmonic(soft, 2.0, 3) > 0.05);
        assert!(harmonic(soft, 2.0, 2) < 1e-9);
        let bent = |v| Curve::Soft.asymmetric(v, 0.5);
        assert!(harmonic(bent, 2.0, 2) > 0.05);
        let hard_bent = |v| Curve::Hard.asymmetric(v, 1.0);
        assert!(harmonic(hard_bent, 2.0, 2) > 0.05);
        // Hard clipping has more energy in high harmonics than soft clipping.
        let hard = |v| Curve::Hard.apply(v);
        assert!(harmonic(hard, 2.0, 9) > harmonic(soft, 2.0, 9));
    }

    #[test]
    fn knee_clip_is_straight_to_the_knee_then_bends_to_one() {
        assert_eq!(knee_clip(0.7, 0.708), 0.7);
        assert_eq!(knee_clip(-0.7, 0.708), -0.7);
        // No corner: the slope just past the knee is still 1.
        let slope = (knee_clip(0.708 + 1e-6, 0.708) - knee_clip(0.708, 0.708)) / 1e-6;
        assert!((slope - 1.0).abs() < 1e-4, "{slope}");
        assert!((knee_clip(1.0, 0.708) - 0.893).abs() < 0.001);
        assert!(knee_clip(1e6, 0.708) <= 1.0 && knee_clip(-1e6, 0.708) >= -1.0);
        // Knee 0 is 1 - e^-v; knee 1 a hard clip.
        assert!((knee_clip(1.0, 0.0) - (1.0 - (-1.0f64).exp())).abs() < 1e-12);
        assert_eq!(knee_clip(3.0, 1.0), 1.0);
        assert_eq!(knee_clip(0.5, 1.0), 0.5);
        assert_eq!(knee_clip(f64::NAN, 0.5), 0.0);
        let values: Vec<f64> = (-400..=400).map(|i| knee_clip(f64::from(i) * 0.01, 0.3)).collect();
        assert!(values.windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn sines_fold_or_hold() {
        assert!((soft_sine(0.5) - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
        assert_eq!(soft_sine(3.0), 1.0);
        assert!((sine_fold(3.0) + 1.0).abs() < 1e-12);
        assert!(sine_fold(2.0).abs() < 1e-12);
        assert_eq!(sine_fold(f64::INFINITY), 0.0);
    }

    #[test]
    fn sigmoid_starts_at_its_gain_and_stays_inside_one() {
        let slope = sigmoid(1e-6, 6.7, 1.75) / 1e-6;
        assert!((slope - 6.7).abs() < 1e-3, "{slope}");
        assert!(sigmoid(1e9, 6.7, 1.75) <= 1.0);
        assert!((sigmoid(-0.3, 6.7, 1.75) + sigmoid(0.3, 6.7, 1.75)).abs() < 1e-12);
        assert_eq!(sigmoid(f64::NAN, 6.7, 1.75), 0.0);
    }

    #[test]
    fn waveshaper_amounts() {
        // At its defaults, or with drive 0, it is the straight line.
        for v in [-2.0, -0.3, 0.0, 0.4, 1.5] {
            assert_eq!(Waveshaper::default().apply(v), v);
            let off = Waveshaper { drive: 0.0, curve: 1.0, depth: 1.0, damp: 1.0, ..Waveshaper::default() };
            assert_eq!(off.apply(v), v);
        }
        // Damp silences small values; Linear sets the slope.
        let damped = Waveshaper { damp: 1.0, ..Waveshaper::default() };
        assert_eq!(damped.apply(0.2), 0.0);
        let steep = Waveshaper { linear: 1.0, ..Waveshaper::default() };
        assert!((steep.apply(0.1) - 0.15).abs() < 1e-12);
        // Curve rounds the top and stays rising; it adds odd harmonics only.
        let round = Waveshaper { curve: 1.0, ..Waveshaper::default() };
        let values: Vec<f64> = (-400..=400).map(|i| round.apply(f64::from(i) * 0.01)).collect();
        assert!(values.windows(2).all(|w| w[1] > w[0]));
        assert!(harmonic(|v| round.apply(v), 1.0, 3) > 0.01);
        assert!(harmonic(|v| round.apply(v), 1.0, 2) < 1e-9);
        // Depth adds a ripple, Period makes it denser.
        let ripple = |period| Waveshaper { depth: 1.0, period, ..Waveshaper::default() };
        assert!(harmonic(|v| ripple(1.0).apply(v), 1.0, 15) > 10.0 * harmonic(|v| ripple(0.0).apply(v), 1.0, 15));
        assert_eq!(Waveshaper::default().apply(f64::NAN), 0.0);
    }
}
