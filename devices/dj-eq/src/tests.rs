// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

const SR: f64 = 48_000.0;
/// Frames after which ramps and filter start-up have passed.
const SETTLED: usize = 12_000;

fn dj_eq(settings: &[(usize, f64)]) -> DjEq {
    let mut eq = DjEq::new();
    for &(index, value) in settings {
        eq.set_param(index, value);
    }
    eq.reset();
    eq
}

/// Gain of a unit sine at `hz` (the same on both sides) through `eq`, from the RMS level
/// after `SETTLED` frames.
fn gain_at(eq: &mut DjEq, hz: f64) -> f64 {
    let frames = 36_000;
    let mut buffer: Vec<f32> = (0..frames)
        .flat_map(|n| {
            let s = (std::f64::consts::TAU * hz * n as f64 / SR).sin() as f32;
            [s, s]
        })
        .collect();
    eq.process(&ProcessContext::default(), &mut buffer);
    let tail = &buffer[SETTLED * 2..];
    (tail.iter().map(|&s| f64::from(s) * f64::from(s)).sum::<f64>() / tail.len() as f64 * 2.0).sqrt()
}

suhina_device_api::conformance_test!(DjEq::new());

#[test]
fn defaults_have_a_flat_response() {
    for slope in [0.0, 1.0] {
        for hz in [30.0, 120.0, 250.0, 700.0, 2_500.0, 6_000.0, 15_000.0] {
            let gain = gain_at(&mut dj_eq(&[(param::SLOPE, slope)]), hz);
            assert!((gain - 1.0).abs() < 0.01, "slope {slope}, {hz} Hz: {gain}");
        }
    }
}

#[test]
fn kills_remove_their_band() {
    for (kill, hz, others) in [
        (param::LOW_KILL, 50.0, [1_000.0, 8_000.0]),
        (param::MID_KILL, 800.0, [40.0, 12_000.0]),
        (param::HIGH_KILL, 12_000.0, [40.0, 800.0]),
    ] {
        let limits = [(0.0, 0.02), (1.0, 0.005)];
        for (slope, limit) in limits {
            let settings = [(kill, 1.0), (param::SLOPE, slope)];
            let killed = gain_at(&mut dj_eq(&settings), hz);
            assert!(killed < limit, "kill {kill}, slope {slope}: {hz} Hz at {killed}");
            for other in others {
                let kept = gain_at(&mut dj_eq(&settings), other);
                assert!((kept - 1.0).abs() < 0.02, "kill {kill}, slope {slope}: {other} Hz at {kept}");
            }
        }
    }
}

#[test]
fn gains_boost_and_cut_their_band() {
    let boosted = gain_at(&mut dj_eq(&[(param::HIGH, 6.0)]), 10_000.0);
    assert!((boosted - 1.995).abs() < 0.03, "{boosted}");
    let cut = gain_at(&mut dj_eq(&[(param::LOW, -12.0)]), 40.0);
    assert!((cut - 0.251).abs() < 0.01, "{cut}");
    let silent = gain_at(&mut dj_eq(&[(param::MID, SILENCE_DB as f64)]), 800.0);
    assert!(silent < 0.02, "{silent}");
}

#[test]
fn crossover_frequencies_move() {
    // With the low band split at 1 kHz, 500 Hz is low and a low kill removes it.
    let settings = [(param::LOW_FREQUENCY, 1_000.0), (param::LOW_KILL, 1.0), (param::SLOPE, 1.0)];
    assert!(gain_at(&mut dj_eq(&settings), 500.0) < 0.01);
    // Moved while running, the crossover arrives at its new frequency.
    let mut eq = dj_eq(&[(param::LOW_KILL, 1.0), (param::SLOPE, 1.0)]);
    assert!((gain_at(&mut eq, 500.0) - 1.0).abs() < 0.02);
    eq.set_param(param::LOW_FREQUENCY, 1_000.0);
    assert!(gain_at(&mut eq, 500.0) < 0.01);
}

#[test]
fn the_high_frequency_never_goes_below_the_low_one() {
    let mut eq = dj_eq(&[(param::LOW_FREQUENCY, 3_000.0), (param::HIGH_FREQUENCY, 300.0)]);
    let [low, high] = eq.frequency_targets();
    assert_eq!(low, high);
    assert!((gain_at(&mut eq, 1_000.0) - 1.0).abs() < 0.01);
}

#[test]
fn a_frequency_sweep_stays_clean() {
    let mut eq = dj_eq(&[(param::MID_KILL, 1.0)]);
    let mut buffer: Vec<f32> = (0..48_000)
        .flat_map(|n| {
            let s = (std::f64::consts::TAU * 440.0 * n as f64 / SR).sin() as f32 * 0.5;
            [s, s]
        })
        .collect();
    for (i, block) in buffer.chunks_mut(512).enumerate() {
        let sweep = 100.0 * 2.0f64.powf(i as f64 / 12.0);
        eq.set_param(param::LOW_FREQUENCY, sweep);
        eq.process(&ProcessContext::default(), block);
    }
    assert!(buffer.iter().all(|s| s.is_finite() && s.abs() < 1.0));
    let largest_step = buffer.as_chunks::<2>().0.windows(2).map(|w| (w[0][0] - w[1][0]).abs()).fold(0.0, f32::max);
    // A 440 Hz sine at 0.5 changes by at most 0.029 per frame; the sweep may not add clicks.
    assert!(largest_step < 0.04, "{largest_step}");
}
