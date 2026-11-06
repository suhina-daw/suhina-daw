// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_device_api::conformance::test_signal;

use super::*;

const SR: f64 = 48_000.0;
/// Every effect off: the device only delays.
const CLEAN: [(usize, f64); 1] = [(param::TRACING, 0.0)];

fn vinyl(settings: &[(usize, f64)]) -> VinylDistortion {
    let mut v = VinylDistortion::new();
    for &(index, value) in settings {
        v.set_param(index, value);
    }
    v.reset();
    v
}

fn run(v: &mut VinylDistortion, signal: &mut [f32]) {
    v.process(&ProcessContext::default(), signal);
}

/// A sine of `hz` at `level` on both sides.
fn sine(hz: f64, level: f64, frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|n| {
            let s = ((std::f64::consts::TAU * hz * n as f64 / SR).sin() * level) as f32;
            [s, s]
        })
        .collect()
}

/// Amplitude of `hz` in one side (0 left, 1 right, 2 left minus right) after the first 4800
/// frames, from one DFT bin.
fn level(signal: &[f32], side: usize, hz: f64) -> f64 {
    let frames = signal.as_chunks::<2>().0.get(4_800..).unwrap_or_default();
    let (mut re, mut im) = (0.0, 0.0);
    for (n, [l, r]) in frames.iter().enumerate() {
        let x = f64::from(match side {
            0 => *l,
            1 => *r,
            _ => l - r,
        });
        let w = std::f64::consts::TAU * hz * n as f64 / SR;
        re += x * w.cos();
        im += x * w.sin();
    }
    2.0 * re.hypot(im) / frames.len().max(1) as f64
}

suhina_device_api::conformance_test!(VinylDistortion::new());

#[test]
fn with_every_effect_off_the_input_comes_out_delayed_exactly() {
    let input = test_signal(4_096);
    let mut output = input.clone();
    run(&mut vinyl(&CLEAN), &mut output);
    let delay = DELAY * 2;
    assert!(output[..delay].iter().all(|&s| s == 0.0));
    assert_eq!(output[delay..], input[..input.len() - delay]);
    assert_eq!(VinylDistortion::new().latency(), LATENCY);
}

#[test]
fn tracing_adds_even_harmonics_around_its_frequency() {
    let settings = [(param::TRACING_DRIVE, 0.8), (param::TRACING_FREQUENCY, 1_000.0)];
    let mut s = sine(1_000.0, 0.5, 48_000);
    run(&mut vinyl(&settings), &mut s);
    let (fundamental, second, third) = (level(&s, 0, 1_000.0), level(&s, 0, 2_000.0), level(&s, 0, 3_000.0));
    assert!(second > 0.02, "2nd {second}");
    assert!(second > 2.0 * third, "2nd {second}, 3rd {third}");
    assert!((fundamental - 0.5).abs() < 0.15, "{fundamental}");
    // Far from the band, a sine passes almost untouched.
    let mut low = sine(60.0, 0.5, 48_000);
    run(&mut vinyl(&settings), &mut low);
    assert!(level(&low, 0, 120.0) < 0.002, "{}", level(&low, 0, 120.0));
}

#[test]
fn stereo_pinch_distorts_out_of_phase_and_mono_pinch_in_phase() {
    let base =
        [(param::TRACING, 0.0), (param::PINCH, 1.0), (param::PINCH_DRIVE, 0.9), (param::PINCH_FREQUENCY, 1_000.0)];
    let mut stereo = sine(1_000.0, 0.5, 48_000);
    run(&mut vinyl(&base), &mut stereo);
    // A mono source: the difference between the sides is all distortion, mostly odd.
    let third = level(&stereo, 2, 3_000.0);
    assert!(third > 0.02, "{third}");
    assert!(third > 2.0 * level(&stereo, 2, 2_000.0));

    let mut mono = sine(1_000.0, 0.5, 48_000);
    run(&mut vinyl(&[base.as_slice(), &[(param::PINCH_MODE, 1.0)]].concat()), &mut mono);
    assert!(mono.as_chunks::<2>().0.iter().all(|[l, r]| l == r));
    assert!(level(&mono, 0, 3_000.0) > 0.01);
}

#[test]
fn hard_curve_is_brighter_than_soft() {
    // Moderate drive: fully saturated, both curves sound the same.
    let base = [(param::TRACING_DRIVE, 0.3), (param::TRACING_FREQUENCY, 1_000.0)];
    let mut soft = sine(1_000.0, 0.8, 48_000);
    run(&mut vinyl(&base), &mut soft);
    let mut hard = sine(1_000.0, 0.8, 48_000);
    run(&mut vinyl(&[base.as_slice(), &[(param::CURVE, 1.0)]].concat()), &mut hard);
    // Tracing adds even harmonics; compare the high ones.
    let high = |s: &[f32]| [4, 6, 8, 10, 12].iter().map(|&k| level(s, 0, 1_000.0 * f64::from(k))).sum::<f64>();
    assert!(high(&hard) > high(&soft), "{} {}", high(&hard), high(&soft));
}

#[test]
fn crackle_clicks_at_its_density_and_repeats_after_reset() {
    let settings = [(param::TRACING, 0.0), (param::CRACKLE_LEVEL, -6.0), (param::CRACKLE_DENSITY, 50.0)];
    let mut v = vinyl(&settings);
    let mut s = vec![0.0f32; 2 * 2 * SR as usize];
    run(&mut v, &mut s);
    let left: Vec<f32> = s.as_chunks::<2>().0.iter().map(|[l, _]| *l).collect();
    let clicks = left.windows(2).filter(|w| (w[1] - w[0] * v.click_decay as f32).abs() > 0.1).count();
    assert!((60..=140).contains(&clicks), "{clicks} clicks in 2 s");
    assert!(left.iter().all(|x| x.abs() < 2.0));

    v.reset();
    let mut again = vec![0.0f32; s.len()];
    run(&mut v, &mut again);
    assert_eq!(s, again);

    let mut quiet = vec![0.0f32; 2 * SR as usize];
    run(&mut vinyl(&[(param::TRACING, 0.0), (param::CRACKLE_DENSITY, 200.0)]), &mut quiet);
    assert!(quiet.iter().all(|&x| x == 0.0));
}
