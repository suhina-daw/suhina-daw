// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

const RATE: f64 = 48_000.0;

fn tone(hz: f64, amplitude: f64, n: usize) -> f64 {
    amplitude * (std::f64::consts::TAU * hz * n as f64 / RATE).sin()
}

/// The amplitude of `hz` in the second half of `signal`.
fn level(signal: &[f64], hz: f64) -> f64 {
    let half = &signal[signal.len() / 2..];
    let (mut re, mut im) = (0.0, 0.0);
    for (n, x) in half.iter().enumerate() {
        let w = std::f64::consts::TAU * hz * (n + signal.len() / 2) as f64 / RATE;
        re += x * w.cos();
        im += x * w.sin();
    }
    2.0 * re.hypot(im) / half.len() as f64
}

fn run(block: &mut SpectralDynamics, frames: usize, f: impl Fn(usize) -> f64) -> Vec<f64> {
    (0..frames)
        .map(|n| {
            let x = f(n);
            block.process([x, x], None)[0]
        })
        .collect()
}

#[test]
fn with_nothing_moving_it_is_a_pure_delay() {
    let mut block = SpectralDynamics::new(RATE);
    let input: Vec<f64> = (0..20_000).map(|n| tone(440.0, 0.3, n) + tone(3_000.0, 0.1, n)).collect();
    let output: Vec<f64> = input.iter().map(|&x| block.process([x, x], None)[0]).collect();
    let worst = (FRAME * 2..input.len()).map(|n| (output[n] - input[n - FRAME]).abs()).fold(0.0, f64::max);
    assert!(worst < 1e-4, "the output is the input FRAME frames later: {worst}");
}

#[test]
fn a_resonance_inside_the_band_is_cut_and_its_neighbours_are_not() {
    let mut block = SpectralDynamics::new(RATE);
    block.set(
        0,
        Settings {
            on: true,
            frequency: 2_000.0,
            q: 0.5,
            range: -12.0,
            threshold: -60.0,
            density: 0.9,
            tilt: false,
            ..Settings::default()
        },
    );
    // Quiet noise-like bed of many tones, one loud resonance at 2.2 kHz.
    let bed = |n: usize| (1..40).map(|i| tone(200.0 * i as f64 + 37.0, 0.004, n)).sum::<f64>();
    let out = run(&mut block, 96_000, |n| bed(n) + tone(2_200.0, 0.2, n));
    let resonance = 20.0 * (level(&out, 2_200.0) / 0.2).log10();
    assert!(resonance < -6.0, "the resonance is cut: {resonance} dB");
    let neighbour = 20.0 * (level(&out, 1_637.0) / 0.004).log10();
    assert!(neighbour > -2.0, "a quiet neighbour in the band is left alone: {neighbour} dB");
    assert!(block.live(0).iter().any(|g| *g < -6.0), "the live cut shows it");
}

/// A dense bed of tones 37 Hz apart over 1 – 4 kHz, each 0.01 (−40 dBFS), and a resonance of
/// 0.06 (−24.4 dBFS) at 2.21 kHz: as a mix's band holds it, every bin under −24 dBFS while
/// the band itself is over it.
fn mix(n: usize) -> f64 {
    (0..81).map(|i| tone(1_000.0 + 37.0 * i as f64 + 3.0, 0.01, n)).sum::<f64>() + tone(2_210.0, 0.06, n)
}

fn mix_band(threshold: f64) -> Settings {
    Settings {
        on: true,
        frequency: 2_000.0,
        q: 0.5,
        range: -12.0,
        threshold,
        density: 0.5,
        tilt: false,
        ..Settings::default()
    }
}

#[test]
fn the_threshold_is_the_bands_level_not_a_bins() {
    // At −24 dB, each bin of the mix lies under the threshold; the band as a whole is over it,
    // so the resonance is cut by about as much as it rises past its neighbours.
    let mut block = SpectralDynamics::new(RATE);
    block.set(0, mix_band(-24.0));
    let out = run(&mut block, 96_000, mix);
    let resonance = 20.0 * (level(&out, 2_210.0) / 0.06).log10();
    assert!(resonance < -3.0, "the resonance is cut with the band over its threshold: {resonance} dB");
}

#[test]
fn a_band_under_its_threshold_moves_nothing() {
    let mut block = SpectralDynamics::new(RATE);
    block.set(0, mix_band(-3.0));
    let out = run(&mut block, 96_000, mix);
    let resonance = 20.0 * (level(&out, 2_210.0) / 0.06).log10();
    assert!(resonance > -0.5, "the band is under −3 dB, so nothing moves: {resonance} dB");
    assert!(block.live(0).iter().all(|g| g.abs() < 0.1));
}

#[test]
fn a_band_opens_over_a_soft_knee_around_its_threshold() {
    assert_eq!(opening(-3.0), 0.0);
    assert_eq!(opening(3.0), 1.0);
    assert!((opening(0.0) - 0.5).abs() < 1e-12);
    assert!(opening(-1.0) < opening(1.0));
}

#[test]
fn a_band_off_or_with_no_range_moves_nothing() {
    let mut block = SpectralDynamics::new(RATE);
    block.set(0, Settings { on: true, range: 0.0, threshold: -80.0, ..Settings::default() });
    let out = run(&mut block, 48_000, |n| tone(1_000.0, 0.5, n));
    assert!((level(&out, 1_000.0) / 0.5 - 1.0).abs() < 0.01);
    assert!(block.live(0).iter().all(|g| *g == 0.0));
}

#[test]
fn low_density_spreads_a_cut_wider_than_high_density() {
    let width = |density: f64| {
        let mut block = SpectralDynamics::new(RATE);
        block.set(
            0,
            Settings {
                on: true,
                frequency: 2_000.0,
                q: 0.3,
                range: -12.0,
                threshold: -60.0,
                density,
                tilt: false,
                ..Settings::default()
            },
        );
        let bed = |n: usize| (1..40).map(|i| tone(200.0 * i as f64 + 37.0, 0.004, n)).sum::<f64>();
        run(&mut block, 48_000, |n| bed(n) + tone(2_200.0, 0.2, n));
        block.live(0).iter().filter(|g| **g < -1.0).count()
    };
    let (wide, narrow) = (width(0.1), width(0.9));
    assert!(narrow >= 1, "a narrow cut still shows");
    assert!(wide > narrow, "low density spreads the cut: {wide} points against {narrow}");
}

#[test]
fn a_bins_times_are_faster_in_the_highs() {
    assert!((bin_time(0.01, 1_000.0) - 0.01).abs() < 1e-12, "the band's time is its time at 1 kHz");
    assert!(bin_time(0.01, 10_000.0) < 0.004, "about three times as fast at 10 kHz");
    assert!(bin_time(0.01, 100.0) > 0.03, "about three times as slow at 100 Hz");
    assert_eq!(bin_time(0.01, 10.0), bin_time(0.01, 40.0), "held below 40 Hz");
}

#[test]
fn a_smoothed_notch_keeps_its_depth_in_the_middle_and_softens_its_edges() {
    let mut gains = [0.0f32, 0.0, -12.0, -12.0, -12.0, 0.0, 0.0];
    smooth_across(&mut gains);
    assert_eq!(gains[3], -12.0);
    assert_eq!(gains[2], -9.0);
    assert_eq!(gains[1], -3.0);
    assert_eq!(gains[0], 0.0, "the ends are left as they are");
}

#[test]
fn shapes_weigh_their_own_bins() {
    assert!((shape_weight(Shape::Bell, 1_000.0, 1_000.0, 1.0) - 1.0).abs() < 1e-9);
    assert!(shape_weight(Shape::Bell, 4_000.0, 1_000.0, 2.0) < 0.05);
    assert!(shape_weight(Shape::LowShelf, 100.0, 1_000.0, 1.0) > 0.99);
    assert!(shape_weight(Shape::HighShelf, 100.0, 1_000.0, 1.0) < 0.01);
}
