// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_device_api::conformance::test_signal;

use super::*;

const SR: f64 = 48_000.0;

fn overdrive(settings: &[(usize, f64)]) -> Overdrive {
    let mut o = Overdrive::new();
    for &(index, value) in settings {
        o.set_param(index, value);
    }
    o.reset();
    o
}

/// `frames` frames of a sine of `hz` at `level`, on both sides, through `o`.
fn drive_sine(o: &mut Overdrive, hz: f64, level: f64, frames: usize) -> Vec<f32> {
    let mut s: Vec<f32> = (0..frames)
        .flat_map(|n| {
            let v = ((std::f64::consts::TAU * hz * n as f64 / SR).sin() * level) as f32;
            [v, v]
        })
        .collect();
    o.process(&ProcessContext::default(), &mut s);
    s
}

/// Amplitude of `hz` on the left side after the first 4800 frames, from one DFT bin.
fn level(signal: &[f32], hz: f64) -> f64 {
    let frames = signal.as_chunks::<2>().0.get(4_800..).unwrap_or_default();
    let (mut re, mut im) = (0.0, 0.0);
    for (n, [l, _]) in frames.iter().enumerate() {
        let w = std::f64::consts::TAU * hz * n as f64 / SR;
        re += f64::from(*l) * w.cos();
        im += f64::from(*l) * w.sin();
    }
    2.0 * re.hypot(im) / frames.len().max(1) as f64
}

/// The largest sample on the left side after the first 4800 frames.
fn peak(signal: &[f32]) -> f32 {
    signal.as_chunks::<2>().0.iter().skip(4_800).fold(0.0, |m, [l, _]| m.max(l.abs()))
}

suhina_device_api::conformance_test!(Overdrive::new());

#[test]
fn fully_dry_is_the_input_delayed_exactly() {
    let input = test_signal(4_096);
    let mut output = input.clone();
    overdrive(&[(param::MIX, 0.0)]).process(&ProcessContext::default(), &mut output);
    let delay = DELAY * 2;
    assert_eq!(output[delay..], input[..input.len() - delay]);
    assert_eq!(Overdrive::new().latency(), LATENCY);
}

#[test]
fn more_drive_more_harmonics_at_a_steady_level() {
    let harmonics = |drive| {
        let s = drive_sine(&mut overdrive(&[(param::DRIVE, drive)]), 200.0, 0.3, 48_000);
        let added: f64 = (2..8).map(|k| level(&s, 200.0 * k as f64)).sum();
        (added / level(&s, 200.0), peak(&s))
    };
    let (gentle, gentle_peak) = harmonics(0.1);
    let (hard, hard_peak) = harmonics(0.9);
    assert!(hard > 3.0 * gentle, "{gentle} {hard}");
    // Even harmonics from the bias, strongest at moderate drive; driven flat the wave turns
    // square and odd harmonics take over, as in a pedal.
    let s = drive_sine(&mut overdrive(&[(param::DRIVE, 0.3)]), 200.0, 0.3, 48_000);
    assert!(level(&s, 400.0) > 0.05 * level(&s, 200.0), "{} {}", level(&s, 400.0), level(&s, 200.0));
    // +32 dB more drive, but the curve keeps the level within about 9 dB.
    assert!(hard_peak < 3.0 * gentle_peak && hard_peak > gentle_peak, "{gentle_peak} {hard_peak}");
    assert!(hard_peak < 1.0);
}

#[test]
fn tone_darkens() {
    let high = |tone| {
        let s = drive_sine(&mut overdrive(&[(param::DRIVE, 0.8), (param::TONE, tone)]), 300.0, 0.3, 48_000);
        (10..30).map(|k| level(&s, 300.0 * k as f64)).sum::<f64>()
    };
    assert!(high(0.0) < 0.3 * high(1.0), "{} {}", high(0.0), high(1.0));
}

#[test]
fn the_band_picks_what_is_driven() {
    // A narrow band at 3 kHz leaves little of a 100 Hz sine to drive.
    let narrow = [(param::BAND_FREQUENCY, 3_000.0), (param::BAND_WIDTH, 0.5)];
    let low = drive_sine(&mut overdrive(&narrow), 100.0, 0.5, 48_000);
    let wide = drive_sine(&mut overdrive(&[]), 100.0, 0.5, 48_000);
    assert!(peak(&low) < 0.5 * peak(&wide), "{} {}", peak(&low), peak(&wide));
}

#[test]
fn dynamics_keeps_quiet_passages_quieter() {
    let ratio = |dynamics| {
        let settings = [(param::DRIVE, 0.8), (param::DYNAMICS, dynamics)];
        let quiet = peak(&drive_sine(&mut overdrive(&settings), 200.0, 0.02, 24_000));
        let loud = peak(&drive_sine(&mut overdrive(&settings), 200.0, 0.5, 24_000));
        loud / quiet
    };
    let (squashed, dynamic) = (ratio(0.0), ratio(1.0));
    assert!(dynamic > 3.0 * squashed, "{squashed} {dynamic}");
}

#[test]
fn output_and_mix() {
    let full = peak(&drive_sine(&mut overdrive(&[]), 200.0, 0.3, 24_000));
    let quieter = peak(&drive_sine(&mut overdrive(&[(param::OUTPUT, -12.0)]), 200.0, 0.3, 24_000));
    assert!((quieter / full - 0.251).abs() < 0.01, "{}", quieter / full);
    // Half wet: the 200 Hz fundamental is between the dry and the wet level.
    let dry = level(&drive_sine(&mut overdrive(&[(param::MIX, 0.0)]), 200.0, 0.3, 24_000), 200.0);
    let wet = level(&drive_sine(&mut overdrive(&[]), 200.0, 0.3, 24_000), 200.0);
    let half = level(&drive_sine(&mut overdrive(&[(param::MIX, 0.5)]), 200.0, 0.3, 24_000), 200.0);
    assert!(half > dry.min(wet) && half < dry.max(wet), "{dry} {half} {wet}");
}
