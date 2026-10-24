// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_device_api::conformance::test_signal;

use super::*;

const SR: f64 = 48_000.0;
const RATES: [f64; 6] = [8_000.0, 22_050.0, 44_100.0, 96_000.0, 192_000.0, 384_000.0];

fn limiter(sample_rate: f64, settings: &[(usize, f64)]) -> Limiter {
    let mut l = Limiter::new();
    for &(index, value) in settings {
        l.set_param(index, value);
    }
    let _ = l.prepare(sample_rate);
    l
}

/// `frames` frames of a sine of `hz` at `level` on both sides, at `sample_rate`.
fn sine(hz: f64, level: f64, frames: usize, sample_rate: f64) -> Vec<f32> {
    (0..frames)
        .flat_map(|n| {
            let v = ((std::f64::consts::TAU * hz * n as f64 / sample_rate).sin() * level) as f32;
            [v, v]
        })
        .collect()
}

/// Loud, nasty input: noise with sudden spikes up to +60 dB, a different side each time,
/// steps and silences.
fn hostile(frames: usize) -> Vec<f32> {
    let mut seed = 0x1234_5678_u32;
    let mut out = Vec::with_capacity(frames * 2);
    for n in 0..frames {
        for side in 0..2 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0;
            let v = match (n / 997) % 4 {
                0 => noise * 4.0,
                1 if (n + side * 13) % 61 == 0 => noise.signum() * 1_000.0,
                1 => noise * 0.01,
                2 => 30.0,
                _ => 0.0,
            };
            out.push(v);
        }
    }
    out
}

/// Amplitude of `hz` on the left side from frame `from` on, from one DFT bin.
fn level(signal: &[f32], hz: f64, from: usize) -> f64 {
    let frames = signal.as_chunks::<2>().0.get(from..).unwrap_or_default();
    let (mut re, mut im) = (0.0, 0.0);
    for (n, [l, _]) in frames.iter().enumerate() {
        let w = std::f64::consts::TAU * hz * n as f64 / SR;
        re += f64::from(*l) * w.cos();
        im += f64::from(*l) * w.sin();
    }
    2.0 * re.hypot(im) / frames.len().max(1) as f64
}

suhina_device_api::conformance_test!(Limiter::new());

#[test]
fn reports_the_lookahead_as_latency_at_every_rate() {
    assert_eq!(Limiter::new().latency(), 72);
    for (sr, frames) in RATES.into_iter().zip([12, 34, 67, 144, 288, 576]) {
        assert_eq!(limiter(sr, &[]).latency(), frames, "{sr}");
        assert_eq!(lookahead_frames(sr), frames as usize);
    }
}

#[test]
fn never_exceeds_the_ceiling_at_any_rate() {
    for sr in RATES {
        for (threshold, ceiling, release) in [(0.0, -0.3, 0.05), (-30.0, -12.0, 0.001), (-10.0, 0.0, 1.0)] {
            let settings = [(param::THRESHOLD, threshold), (param::CEILING, ceiling), (param::RELEASE, release)];
            let mut l = limiter(sr, &settings);
            let limit = db_to_gain(ceiling as f32);
            let mut s = hostile(20_000);
            for (i, block) in s.chunks_mut(2 * 377).enumerate() {
                if i % 7 == 3 {
                    // Settings moving while it plays: the ceiling ramps down by 0.5 dB.
                    l.set_param(param::CEILING, ceiling - 0.5);
                    l.set_param(param::RELEASE, release * 2.0);
                }
                l.process(&ProcessContext::default(), block);
            }
            if let Some(n) = s.iter().position(|v| v.abs() > limit) {
                panic!("{sr} Hz, {settings:?}: sample {n} is {:?} over {limit}", s.get(n));
            }
        }
    }
}

#[test]
fn transparent_under_the_threshold_apart_from_the_delay() {
    let input = test_signal(4_096);
    let mut output = input.clone();
    let mut l = limiter(SR, &[(param::CEILING, 0.0)]);
    l.process(&ProcessContext::default(), &mut output);
    let delay = l.latency() as usize * 2;
    assert!(output[..delay].iter().all(|&v| v == 0.0));
    assert_eq!(output[delay..], input[..input.len() - delay]);
    assert_eq!(l.gain_reduction_db(), 0.0);

    // With the ceiling lower, a quiet signal is only scaled: threshold 0, ceiling -6 dB.
    let mut output = input.clone();
    limiter(SR, &[(param::CEILING, -6.0)]).process(&ProcessContext::default(), &mut output);
    let scale = db_to_gain(-6.0);
    for (o, i) in output[delay..].iter().zip(&input) {
        assert!((o - i * scale).abs() < 1e-6, "{o} {i}");
    }
}

#[test]
fn the_threshold_drives_into_the_ceiling() {
    // A sine at -20 dB with the threshold at -12 dB: raised by 11.7 dB to -8.3 dB, under the
    // -0.3 dB ceiling, so no limiting; at -24 dB threshold it would come out at +3.7 dB, so it
    // is held at the ceiling.
    let peak_out = |threshold| {
        let mut s = sine(1_000.0, 0.1, 24_000, SR);
        limiter(SR, &[(param::THRESHOLD, threshold)]).process(&ProcessContext::default(), &mut s);
        s.iter().skip(24_000).fold(0.0f32, |m, v| m.max(v.abs()))
    };
    let raised = peak_out(-12.0);
    assert!((raised - 0.1 * db_to_gain(11.7)).abs() < 1e-3, "{raised}");
    let held = peak_out(-24.0);
    let ceiling = db_to_gain(-0.3);
    assert!(held <= ceiling && held > ceiling * 0.97, "{held}");
}

#[test]
fn limits_smoothly_instead_of_clipping() {
    // A 1 kHz sine 12 dB over the ceiling: a hard clip adds a strong third harmonic; the
    // limiter turns the gain down instead and adds very little.
    let mut s = sine(1_000.0, 4.0, 48_000, SR);
    limiter(SR, &[(param::CEILING, 0.0), (param::RELEASE, 0.2)]).process(&ProcessContext::default(), &mut s);
    let mut clipped = sine(1_000.0, 4.0, 48_000, SR);
    clipped.iter_mut().for_each(|v| *v = v.clamp(-1.0, 1.0));
    let limited = level(&s, 3_000.0, 24_000) / level(&s, 1_000.0, 24_000);
    let hard = level(&clipped, 3_000.0, 24_000) / level(&clipped, 1_000.0, 24_000);
    assert!(limited < 0.01 && hard > 0.1, "{limited} {hard}");
    assert!((level(&s, 1_000.0, 24_000) - 1.0).abs() < 0.03, "{}", level(&s, 1_000.0, 24_000));
}

#[test]
fn release_lets_the_gain_back_up() {
    // A loud burst, then a quiet tone: the gain comes back up with the release time.
    let recovery = |release: f64| {
        let mut l = limiter(SR, &[(param::CEILING, 0.0), (param::RELEASE, release)]);
        let mut burst = sine(1_000.0, 10.0, 4_800, SR);
        l.process(&ProcessContext::default(), &mut burst);
        let after = (0.02 * SR) as usize;
        let mut quiet = sine(1_000.0, 0.1, after, SR);
        l.process(&ProcessContext::default(), &mut quiet);
        l.gain_reduction_db()
    };
    let (fast, slow) = (recovery(0.002), recovery(0.5));
    assert!(fast < 0.1, "{fast}");
    assert!(slow > 15.0, "{slow}");
}

#[test]
fn reset_and_prepare_clear_the_delay_line() {
    let mut l = limiter(SR, &[]);
    let mut loud = sine(500.0, 8.0, 1_000, SR);
    l.process(&ProcessContext::default(), &mut loud);
    l.reset();
    let mut silence = vec![0.0f32; 2_000];
    l.process(&ProcessContext::default(), &mut silence);
    assert!(silence.iter().all(|&v| v == 0.0));
    assert_eq!(l.gain_reduction_db(), 0.0);
}
