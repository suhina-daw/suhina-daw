// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

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
fn a_huge_finite_input_stays_finite() {
    let w = Waveshaper { damp: 1.0, drive: 0.0, ..Waveshaper::default() };
    for v in [1e80, -1e80, f64::MAX] {
        assert!(w.apply(v).is_finite(), "{v}");
    }
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
fn quadratic_clip_reaches_one_at_two_minus_the_knee() {
    let k = 2.0 / 3.0;
    assert_eq!(quadratic_clip(0.6, k), 0.6);
    assert_eq!(quadratic_clip(-0.6, k), -0.6);
    assert!((quadratic_clip(1.0, k) - 11.0 / 12.0).abs() < 1e-12);
    assert_eq!(quadratic_clip(4.0 / 3.0, k), 1.0);
    assert_eq!(quadratic_clip(-5.0, k), -1.0);
    // No corner at either end: slope 1 at the knee, 0 at the ceiling.
    let slope = |v: f64| (quadratic_clip(v + 1e-7, k) - quadratic_clip(v, k)) / 1e-7;
    assert!((slope(k) - 1.0).abs() < 1e-5);
    assert!(slope(4.0 / 3.0 - 1e-6).abs() < 1e-5);
    assert_eq!(quadratic_clip(0.5, 1.0), 0.5);
    assert_eq!(quadratic_clip(1.5, 1.0), 1.0);
    assert_eq!(quadratic_clip(f64::NAN, k), 0.0);
    let values: Vec<f64> = (-400..=400).map(|i| quadratic_clip(f64::from(i) * 0.01, k)).collect();
    assert!(values.windows(2).all(|w| w[1] >= w[0]));
}

#[test]
fn arctan_curve_is_one_at_one_and_odd() {
    let slope = arctan_curve(1e-7, 10.0) / 1e-7;
    assert!((slope - 10.0 / 10.0f64.atan()).abs() < 1e-4, "{slope}");
    assert!((arctan_curve(1.0, 10.0) - 1.0).abs() < 1e-12);
    assert!((arctan_curve(-0.3, 10.0) + arctan_curve(0.3, 10.0)).abs() < 1e-12);
    assert!(arctan_curve(1e9, 10.0) < std::f64::consts::FRAC_PI_2 / 10.0f64.atan() + 1e-9);
    assert_eq!(arctan_curve(f64::NAN, 10.0), 0.0);
    assert_eq!(arctan_curve(0.5, f64::INFINITY), 0.0);
}

#[test]
fn waveshaper_amounts() {
    // At its defaults it is half the input; with drive 0 it is the straight line.
    for v in [-2.0, -0.3, 0.0, 0.4, 1.5] {
        assert_eq!(Waveshaper::default().apply(v), 0.5 * v);
        let off = Waveshaper { drive: 0.0, curve: 1.0, depth: 1.0, damp: 1.0, ..Waveshaper::default() };
        assert_eq!(off.apply(v), v);
    }
    // Linear is the gain of the straight part: 0 is silence, 1 is unity, ½ is half.
    let gain = |linear| Waveshaper { linear, ..Waveshaper::default() }.apply(0.1) / 0.1;
    assert_eq!(gain(0.0), 0.0);
    assert!((gain(0.5) - 0.5).abs() < 1e-12);
    assert!((gain(1.0) - 1.0).abs() < 1e-12);
    // Damp takes the slope at the origin down by 1 / (1 + 64.2 damp) and gives it back with
    // level: half way back at 0.240 for Damp 50 %.
    let damped = |damp| Waveshaper { damp, ..Waveshaper::default() };
    assert!((damped(0.5).apply(1e-4) / 1e-4 - 0.5 / 33.1).abs() < 1e-6);
    let half = 0.5 * (1.0 / 33.1 + 1.0) / 2.0;
    assert!((damped(0.5).apply(0.240) / 0.240 - half).abs() < 1e-6);
    assert!((damped(0.5).apply(1.0) - 0.5).abs() < 0.002);
    // Curve adds a squared term: even harmonics only, proportional to the setting, with the
    // second harmonic of a small tone at 2π · curve · A against the fundamental.
    let round = |curve| Waveshaper { curve, ..Waveshaper::default() };
    let second = |curve, a: f64| harmonic(|v| round(curve).apply(v), a, 2) / harmonic(|v| round(curve).apply(v), a, 1);
    assert!((second(1.0, 0.05) - std::f64::consts::TAU * 0.05).abs() < 2e-3, "{}", second(1.0, 0.05));
    assert!((second(0.5, 0.05) * 2.0 - second(1.0, 0.05)).abs() < 2e-3);
    assert!(harmonic(|v| round(1.0).apply(v), 0.05, 3) < 1e-6);
    // It folds back once it reaches 1, at x = ½ for Curve 100 %: bounded however hard it is
    // driven.
    assert!((round(1.0).apply(0.5) - (0.25 + 1.0)).abs() < 1e-12);
    assert!((round(1.0).apply(1.0 / 2.0f64.sqrt()) - (0.5 / 2.0f64.sqrt())).abs() < 1e-9);
    assert!((round(1.0).apply(100.0) - 0.5 * 100.0).abs() <= 1.0 + 1e-9);
    // Depth adds a ripple (as its square), Period makes it denser.
    let ripple = |depth, period| Waveshaper { depth, period, ..Waveshaper::default() };
    assert!(harmonic(|v| ripple(1.0, 1.0).apply(v), 1.0, 15) > 10.0 * harmonic(|v| ripple(1.0, 0.0).apply(v), 1.0, 15));
    let part = |depth: f64| ripple(depth, 0.0).apply(0.3) - 0.15;
    assert!((part(0.5) - part(1.0) / 4.0).abs() < 1e-12);
    assert_eq!(Waveshaper::default().apply(f64::NAN), 0.0);
}
