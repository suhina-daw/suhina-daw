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

/// [`gain_at`] in decibels.
fn db_at(eq: &mut DjEq, hz: f64) -> f64 {
    20.0 * gain_at(eq, hz).log10()
}

/// Settings with the band gains and the crossover frequencies of the measurements: every band
/// except `kept` at silence, crossovers at 500 Hz and 2 kHz.
fn one_band(kept: usize, slope: f64) -> Vec<(usize, f64)> {
    vec![
        (param::LOW, if kept == param::LOW { 0.0 } else { SILENCE_DB as f64 }),
        (param::MID, if kept == param::MID { 0.0 } else { SILENCE_DB as f64 }),
        (param::HIGH, if kept == param::HIGH { 0.0 } else { SILENCE_DB as f64 }),
        (param::LOW_FREQUENCY, 500.0),
        (param::HIGH_FREQUENCY, 2_000.0),
        (param::SLOPE, slope),
    ]
}

suhina_device_api::conformance_test!(DjEq::new());

/// Far from the crossovers the defaults stay flat.
#[test]
fn the_defaults_stay_close_to_flat_away_from_the_crossovers() {
    for slope in [0.0, 1.0] {
        for hz in [30.0, 120.0, 700.0, 6_000.0, 15_000.0] {
            let db = db_at(&mut dj_eq(&[(param::SLOPE, slope)]), hz);
            assert!(db.abs() < 0.15, "slope {slope}, {hz} Hz: {db} dB");
        }
    }
}

/// Each band alone against the measured magnitudes (crossovers 500 Hz and 2 kHz). Entries the
/// measurement read near its render floor are held loosely; the 48 dB high band at 250 Hz was
/// below the floor.
#[test]
fn the_bands_match_the_measured_shapes() {
    let tables = [
        (0.0, param::LOW, &[(250.0, -0.53), (500.0, -6.02), (1_000.0, -24.7), (2_000.0, -48.4), (4_000.0, -73.2)][..]),
        (0.0, param::MID, &[(250.0, -24.6), (500.0, -6.05), (1_000.0, -1.04), (2_000.0, -6.05), (4_000.0, -25.3)][..]),
        (0.0, param::HIGH, &[(250.0, -97.1), (500.0, -54.4), (1_000.0, -25.3), (2_000.0, -6.05), (4_000.0, -0.49)][..]),
        (1.0, param::LOW, &[(250.0, -0.03), (500.0, -6.02), (1_000.0, -48.3), (2_000.0, -96.7)][..]),
        (1.0, param::MID, &[(250.0, -48.2), (500.0, -6.02), (1_000.0, -0.07), (2_000.0, -6.02), (4_000.0, -49.6)][..]),
        (1.0, param::HIGH, &[(500.0, -103.0), (1_000.0, -48.6), (2_000.0, -6.02), (4_000.0, -0.03)][..]),
    ];
    for (slope, band, table) in tables {
        for &(hz, db) in table {
            let tolerance = if db > -60.0 { 0.2 } else { 1.5 };
            let got = db_at(&mut dj_eq(&one_band(band, slope)), hz);
            assert!((got - db).abs() < tolerance, "slope {slope}, band {band}, {hz} Hz: {got} dB against {db} dB");
        }
    }
    // The 48 dB high band at 250 Hz was below the measurement's floor.
    let got = db_at(&mut dj_eq(&one_band(param::HIGH, 1.0)), 250.0);
    assert!(got < -99.0, "slope 1, high band at 250 Hz: {got} dB");
}

/// With every gain at 0 dB the sum keeps the measured dip at the low crossover, and stays flat
/// far from both crossovers.
#[test]
fn the_flat_setting_sums_like_the_measured_device() {
    let crossovers = [(param::LOW_FREQUENCY, 500.0), (param::HIGH_FREQUENCY, 2_000.0)];
    let sums = [
        (0.0, [(500.0, -0.57), (650.0, -0.75), (2_000.0, -0.07)]),
        (1.0, [(500.0, -2.01), (650.0, -1.09), (2_000.0, 0.0)]),
    ];
    for (slope, table) in sums {
        for (hz, db) in table {
            let mut settings = vec![(param::SLOPE, slope)];
            settings.extend_from_slice(&crossovers);
            let got = db_at(&mut dj_eq(&settings), hz);
            assert!((got - db).abs() < 0.2, "slope {slope}, sum at {hz} Hz: {got} dB against {db} dB");
        }
    }
    for slope in [0.0, 1.0] {
        for hz in [30.0, 15_000.0] {
            let mut settings = vec![(param::SLOPE, slope)];
            settings.extend_from_slice(&crossovers);
            let db = db_at(&mut dj_eq(&settings), hz);
            assert!(db.abs() < 0.2, "slope {slope}, sum at {hz} Hz: {db} dB");
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
fn the_gains_stop_at_plus_six_db() {
    let mut eq = dj_eq(&[(param::HIGH, 12.0)]);
    assert_eq!(eq.param(param::HIGH), Some(6.0));
    let boosted = gain_at(&mut eq, 10_000.0);
    assert!((boosted - 1.995).abs() < 0.03, "{boosted}");
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
fn crossed_crossovers_match_the_measurements() {
    // Low frequency above the high one, 24 dB, as measured (dB at 905, 1280, 1810 and 2560 Hz):
    // the mid band alone is almost empty, with every gain at 0 dB the sum is flat to 0.3 dB, and
    // the high band alone with its crossover at 200 Hz below the low one at 500 Hz is the two
    // high-passes in series.
    let hz = [905.1, 1_280.0, 1_810.2, 2_560.0];
    let crossed = [(param::LOW_FREQUENCY, 3_000.0), (param::HIGH_FREQUENCY, 500.0)];
    let only = |kept: usize| -> Vec<(usize, f64)> {
        let gain = |band| (band, if band == kept { 0.0 } else { SILENCE_DB as f64 });
        [gain(param::LOW), gain(param::MID), gain(param::HIGH)].into_iter().chain(crossed).collect()
    };
    for (f, expected) in hz.iter().zip([-63.62, -63.24, -63.87, -66.43]) {
        let got = db_at(&mut dj_eq(&only(param::MID)), *f);
        assert!((got - expected).abs() < 0.6, "mid band alone at {f} Hz: {got} against {expected}");
    }
    for (f, expected) in hz.iter().zip([-0.08, -0.16, -0.27, -0.30]) {
        let got = db_at(&mut dj_eq(&crossed), *f);
        assert!((got - expected).abs() < 0.1, "sum at {f} Hz: {got} against {expected}");
    }
    let mut high_only = vec![
        (param::LOW, SILENCE_DB as f64),
        (param::MID, SILENCE_DB as f64),
        (param::LOW_FREQUENCY, 500.0),
        (param::HIGH_FREQUENCY, 200.0),
    ];
    high_only.push((param::HIGH, 0.0));
    for (f, expected) in [(160.0, -50.0), (226.3, -31.79), (320.0, -17.96), (452.5, -8.21), (640.0, -2.83)] {
        let got = db_at(&mut dj_eq(&high_only), f);
        // Within the harness's 0.5 dB, on a slope of 24 dB per octave.
        assert!((got - expected).abs() < 0.5, "high band at {f} Hz: {got} against {expected}");
    }
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

/// The cost of one instance at its defaults: run with `cargo test --release -p suhina-device-dj-eq
/// cost -- --ignored --nocapture`. The budget (docs/spec/dj-eq.md › Cost) is checked by hand, as
/// CI machines vary too much for a timing test.
#[test]
#[ignore]
fn cost_of_a_minute_at_48_khz() {
    let mut device = DjEq::new();
    let _ = device.prepare(48_000.0);
    let mut signal = suhina_device_api::conformance::test_signal(48_000 * 60);
    let start = std::time::Instant::now();
    for block in signal.chunks_mut(512) {
        device.process(&ProcessContext::default(), block);
    }
    let seconds = start.elapsed().as_secs_f64();
    println!("60 s at 48 kHz in {seconds:.3} s: {:.2} % of a core", seconds / 60.0 * 100.0);
}
