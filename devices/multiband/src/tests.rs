// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

const SR: f64 = 48_000.0;

fn device(settings: &[(usize, f64)]) -> Multiband {
    let mut d = Multiband::new();
    for &(index, value) in settings {
        d.set_param(index, value);
    }
    d.reset();
    d
}

/// `frames` frames of a sine at `hz` peaking at `peak_db`, the same on both sides.
fn sine(hz: f64, peak_db: f64, frames: usize) -> Vec<f32> {
    let a = 10f64.powf(peak_db / 20.0);
    (0..frames)
        .flat_map(|n| {
            let v = a * (std::f64::consts::TAU * hz * n as f64 / SR).sin();
            [v as f32, v as f32]
        })
        .collect()
}

/// The left side's RMS from frame `from` on, in decibels.
fn rms_db(signal: &[f32], from: usize) -> f64 {
    let frames = signal.as_chunks::<2>().0;
    let tail: Vec<f64> = frames.iter().skip(from).map(|[l, _]| f64::from(*l)).collect();
    let power = tail.iter().map(|v| v * v).sum::<f64>() / tail.len().max(1) as f64;
    10.0 * power.max(1e-30).log10()
}

/// The steady gain in decibels that a 1 kHz sine at `peak_db` gets with `settings`.
fn steady_gain(settings: &[(usize, f64)], peak_db: f64) -> f64 {
    steady_gain_at(1_000.0, settings, peak_db)
}

fn steady_gain_at(hz: f64, settings: &[(usize, f64)], peak_db: f64) -> f64 {
    let mut d = device(settings);
    let input = sine(hz, peak_db, 24_000);
    let mut output = input.clone();
    d.process(&ProcessContext::default(), &mut output);
    rms_db(&output, 12_000) - rms_db(&input, 12_000)
}

const MID: usize = 1;
/// The mid band compressing hard and fast, so a steady sine settles at its static curve.
fn fast(extra: &[(usize, f64)]) -> Vec<(usize, f64)> {
    let mut s = vec![
        (param::SOFT_KNEE, 0.0),
        (param::band(MID, param::ATTACK), 0.001),
        (param::band(MID, param::RELEASE), 0.2),
    ];
    s.extend_from_slice(extra);
    s
}

suhina_device_api::conformance_test!(Multiband::new());

#[test]
fn the_three_bands_sum_flat_at_their_defaults() {
    for hz in [50.0, 120.0, 400.0, 1_000.0, 2_500.0, 8_000.0] {
        let gain = steady_gain_at(hz, &[], -12.0);
        assert!(gain.abs() < 0.05, "{hz} Hz: {gain} dB");
    }
}

#[test]
fn the_above_stage_follows_its_slope() {
    // A 1 kHz sine 20 dB over a -30 dB threshold; the follower reads the peak level.
    for (slope, expected) in [(0.25, -15.0), (0.5, -10.0), (0.0, -20.0), (2.0, 20.0)] {
        let s =
            fast(&[(param::band(MID, param::ABOVE_THRESHOLD), -30.0), (param::band(MID, param::ABOVE_SLOPE), slope)]);
        let expected = limit(expected);
        let got = steady_gain(&s, -10.0);
        assert!((got - expected).abs() < 1.5, "slope {slope}: {got} dB, expected about {expected} dB");
    }
}

#[test]
fn the_below_stage_lifts_or_lowers_quiet_signals() {
    // A 1 kHz sine 20 dB under a -40 dB below threshold.
    let at = |slope| {
        steady_gain(
            &fast(&[(param::band(MID, param::BELOW_THRESHOLD), -40.0), (param::band(MID, param::BELOW_SLOPE), slope)]),
            -60.0,
        )
    };
    let lift = at(0.5);
    assert!((lift - 10.0).abs() < 1.5, "slope 0.5: {lift} dB");
    let cut = at(2.0);
    assert!((cut + 20.0).abs() < 1.5, "slope 2: {cut} dB");
}

#[test]
fn nothing_happens_between_the_thresholds() {
    let s = fast(&[(param::band(MID, param::ABOVE_SLOPE), 0.25), (param::band(MID, param::BELOW_SLOPE), 0.5)]);
    let gain = steady_gain(&s, -40.0);
    assert!(gain.abs() < 0.1, "{gain} dB");
}

#[test]
fn a_band_that_is_off_passes_untouched() {
    let s = fast(&[
        (param::band(MID, param::ON), 0.0),
        (param::band(MID, param::ABOVE_THRESHOLD), -40.0),
        (param::band(MID, param::ABOVE_SLOPE), 0.0),
        (param::band(MID, param::INPUT), 12.0),
        (param::band(MID, param::GAIN), 12.0),
    ]);
    let gain = steady_gain(&s, -6.0);
    assert!(gain.abs() < 0.05, "{gain} dB");
}

#[test]
fn amount_scales_every_gain_in_decibels() {
    let base = [(param::band(MID, param::ABOVE_THRESHOLD), -30.0), (param::band(MID, param::ABOVE_SLOPE), 0.25)];
    let full = steady_gain(&fast(&base), -10.0);
    let mut half_settings = fast(&base);
    half_settings.push((param::AMOUNT, 0.5));
    let half = steady_gain(&half_settings, -10.0);
    assert!((half - full / 2.0).abs() < 0.3, "50 %: {half} dB against {full} dB");
    let mut none = fast(&base);
    none.extend_from_slice(&[(param::AMOUNT, 0.0), (param::OUTPUT, 12.0), (param::band(MID, param::GAIN), 6.0)]);
    let zero = steady_gain(&none, -10.0);
    assert!(zero.abs() < 0.05, "0 %: {zero} dB");
}

#[test]
fn input_gain_moves_the_thresholds() {
    // With +10 dB input, a -20 dB sine meets a -30 dB threshold as if it were at -10 dB.
    let base = [(param::band(MID, param::ABOVE_THRESHOLD), -30.0), (param::band(MID, param::ABOVE_SLOPE), 0.25)];
    let mut boosted = fast(&base);
    boosted.push((param::band(MID, param::INPUT), 10.0));
    let got = steady_gain(&boosted, -20.0);
    let reference = steady_gain(&fast(&base), -10.0) + 10.0;
    assert!((got - reference).abs() < 0.3, "{got} dB against {reference} dB");
}

#[test]
fn peak_and_rms_read_a_sine_alike() {
    let base = [(param::band(MID, param::ABOVE_THRESHOLD), -30.0), (param::band(MID, param::ABOVE_SLOPE), 0.25)];
    let rms = steady_gain(&fast(&base), -10.0);
    let mut p = fast(&base);
    p.push((param::PEAK, 1.0));
    let peak = steady_gain(&p, -10.0);
    assert!((rms - peak).abs() < 1.0, "RMS {rms} dB, Peak {peak} dB");
}

#[test]
fn the_soft_knee_starts_under_the_threshold() {
    let at = |soft| {
        steady_gain(
            &fast(&[
                (param::SOFT_KNEE, soft),
                (param::band(MID, param::ABOVE_THRESHOLD), -20.0),
                (param::band(MID, param::ABOVE_SLOPE), 0.25),
            ]),
            -25.0,
        )
    };
    assert!(at(0.0).abs() < 0.2, "hard knee under the threshold: {} dB", at(0.0));
    assert!(at(1.0) < -0.5, "soft knee under the threshold: {} dB", at(1.0));
}

#[test]
fn the_limit_never_passes_36_db() {
    for g in [-200.0, -60.0, -36.0, 0.0, 10.0, 60.0, 500.0] {
        let l = limit(g);
        assert!(l.abs() < GAIN_LIMIT_DB + 1e-9, "{g} -> {l}");
        assert!(l.signum() == g.signum() || g == 0.0);
    }
    assert!((limit(-10.0) + 10.0).abs() < 0.01);
}

#[test]
fn the_knee_is_continuous() {
    for w in [0.0, 10.0, 23.6] {
        for u in [-30.0, -w / 2.0, 0.0, w / 2.0, 30.0] {
            let below = knee(u - 1e-6, w);
            let above = knee(u + 1e-6, w);
            assert!((above - below).abs() < 1e-4, "w {w}, u {u}");
        }
    }
}

#[test]
fn time_scales_the_attack() {
    // The reduction 30 ms into a burst: slower with Time 300 % than with 100 %.
    let reduction_at = |time: f64| {
        let mut d = device(&[
            (param::band(MID, param::ABOVE_THRESHOLD), -30.0),
            (param::band(MID, param::ABOVE_SLOPE), 0.25),
            (param::band(MID, param::ATTACK), 0.1),
            (param::TIME, time),
        ]);
        let mut s = sine(1_000.0, -6.0, 4_800);
        let input = s.clone();
        d.process(&ProcessContext::default(), &mut s);
        let window = |x: &[f32]| rms_db(&x[2 * 1_200..2 * 1_488], 0);
        window(&s) - window(&input)
    };
    let normal = reduction_at(1.0);
    let slow = reduction_at(3.0);
    assert!(normal < slow - 1.0, "Time 100 %: {normal} dB, 300 %: {slow} dB");
}

#[test]
fn the_follower_reaches_most_of_a_step_in_about_an_eighth_of_the_attack() {
    let follower = AttackRelease::new(0.1 / FOLLOWER_DIVISOR, 0.1, SR);
    let mut v = 0.0;
    for _ in 0..(0.1 / FOLLOWER_DIVISOR * SR) as usize {
        v = follower.rise(v, 1.0);
    }
    assert!((v - (1.0 - (-1.0f64).exp())).abs() < 0.01, "{v}");
}
