// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]

use suhina_instrument_analysis::{Error, Outcome, RATE, analyze};

fn tone(rate: u32, hz: f64, seconds: f64, gain: f64, dc: f64) -> Vec<f32> {
    (0..(f64::from(rate) * seconds) as usize)
        .map(|i| (gain * (std::f64::consts::TAU * hz * i as f64 / f64::from(rate)).sin() + dc) as f32)
        .collect()
}

#[test]
fn measured_pitch_and_residual_survive_rates_gains_and_dc() {
    for (rate, gain, dc) in [(44100, 0.5, 0.0), (48000, 0.01, 0.3), (96000, 3.0, -7.0), (8000, 0.8, 0.0)] {
        let hz = 440.0 * 2.0f64.powf(23.0 / 1200.0);
        let samples = tone(rate, hz, 0.4, gain, dc);
        let report = analyze(&samples, rate, 1, 0..samples.len(), |_, _| true).unwrap();
        let p = report.proposal.unwrap();
        assert_eq!(report.outcome, Outcome::Stable);
        assert_eq!(p.root, 69);
        assert!((p.cents - 23.0).abs() < 4.0, "{rate}: {p:?}");
        assert!(p.periodicity > 0.95 && p.periodicity <= 1.0);
        assert!(p.spread_cents < 3.0);
    }
}

#[test]
fn fundamentals_are_not_replaced_by_stronger_second_harmonics() {
    for hz in [55.0, 110.0, 261.6256, 1046.502] {
        let base = tone(44100, hz, 0.4, 0.3, 0.0);
        let harmonic = tone(44100, hz * 2.0, 0.4, 0.7, 0.0);
        let samples: Vec<_> = base.iter().zip(harmonic).map(|(a, b)| a + b).collect();
        let report = analyze(&samples, 44100, 1, 0..samples.len(), |_, _| true).unwrap();
        let p = report.proposal.unwrap();
        assert!((1200.0 * (f64::from(p.hz) / hz).log2()).abs() < 20.0, "{hz}: {p:?}");
    }
}

#[test]
fn stereo_polarity_preserves_pitch_but_disagreement_refuses_proposal() {
    let a = tone(RATE, 220.0, 0.4, 0.6, 0.0);
    let b = tone(RATE, 329.6276, 0.4, 0.6, 0.0);
    let reversed: Vec<_> = a.iter().flat_map(|a| [*a, -*a]).collect();
    let valid = analyze(&reversed, RATE, 2, 0..a.len(), |_, _| true).unwrap();
    assert_eq!(valid.proposal.unwrap().root, 57);
    let different: Vec<_> = a.iter().zip(b).flat_map(|(a, b)| [*a, b]).collect();
    let invalid = analyze(&different, RATE, 2, 0..a.len(), |_, _| true).unwrap();
    assert_eq!(invalid.outcome, Outcome::NoStablePitch);
    assert_eq!(invalid.proposal, None);
}

#[test]
fn harmonic_mixtures_document_the_unassessed_source_count() {
    // The exact same samples can describe two notes or one tone's second/third harmonics.
    // A stable periodicity is a candidate, never evidence that this source is monophonic.
    let a = tone(RATE, 220.0, 0.4, 0.5, 0.0);
    let b = tone(RATE, 330.0, 0.4, 0.5, 0.0);
    let mixed: Vec<_> = a.iter().zip(b).map(|(a, b)| a + b).collect();
    let report = analyze(&mixed, RATE, 1, 0..mixed.len(), |_, _| true).unwrap();
    assert_eq!(report.outcome, Outcome::Stable);
    assert_eq!(report.proposal.unwrap().root, 45);
}

#[test]
fn silence_short_audio_noise_and_glides_have_no_fabricated_root() {
    let silent = analyze(&[0.0; 5000], RATE, 1, 0..5000, |_, _| true).unwrap();
    assert_eq!(silent.outcome, Outcome::Silent);
    assert_eq!(silent.proposal, None);
    assert_eq!(analyze(&[0.7; 5000], RATE, 1, 0..5000, |_, _| true).unwrap().outcome, Outcome::Silent);
    let short = tone(RATE, 440.0, 0.02, 0.5, 0.0);
    assert_eq!(analyze(&short, RATE, 1, 0..short.len(), |_, _| true).unwrap().outcome, Outcome::TooShort);
    let mut seed = 0x4a32_b715u32;
    let noise: Vec<_> = (0..12000)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed as f32 / u32::MAX as f32 - 0.5
        })
        .collect();
    let noisy = analyze(&noise, RATE, 1, 0..noise.len(), |_, _| true).unwrap();
    assert_eq!(noisy.outcome, Outcome::NoStablePitch);
    let glide: Vec<_> = (0..12000)
        .map(|i| {
            let t = i as f64 / f64::from(RATE);
            (std::f64::consts::TAU * (220.0 * t + 100.0 * t * t)).sin() as f32
        })
        .collect();
    assert_eq!(analyze(&glide, RATE, 1, 0..glide.len(), |_, _| true).unwrap().proposal, None);
}

#[test]
fn out_of_band_tones_are_not_clamped_or_given_a_lower_octave() {
    for hz in [20.0, 4500.0, 8000.0] {
        let samples = tone(44100, hz, 0.4, 0.7, 0.0);
        assert_eq!(analyze(&samples, 44100, 1, 0..samples.len(), |_, _| true).unwrap().proposal, None, "{hz}");
    }
}

#[test]
fn malformed_input_and_cancellation_are_explicit_errors() {
    let samples = tone(RATE, 440.0, 0.3, 0.6, 0.0);
    for channels in [0, 3, u16::MAX] {
        assert_eq!(analyze(&samples, RATE, channels, 0..1, |_, _| true), Err(Error::InvalidFormat));
    }
    assert_eq!(analyze(&samples, 0, 1, 0..1, |_, _| true), Err(Error::InvalidFormat));
    assert_eq!(analyze(&samples, RATE, 1, 3..2, |_, _| true), Err(Error::InvalidRange));
    assert_eq!(analyze(&samples, RATE, 1, 0..usize::MAX, |_, _| true), Err(Error::InvalidRange));
    let mut invalid = samples.clone();
    invalid[15] = f32::INFINITY;
    assert_eq!(analyze(&invalid, RATE, 1, 0..invalid.len(), |_, _| true), Err(Error::NonFinite));
    let mut callbacks = 0;
    assert_eq!(
        analyze(&samples, RATE, 1, 0..samples.len(), |_, _| {
            callbacks += 1;
            callbacks < 100
        }),
        Err(Error::Cancelled)
    );
    assert_eq!(callbacks, 100);
}

#[test]
fn only_the_admitted_prefix_is_analyzed_and_reported() {
    let mut samples = tone(1000, 110.0, 9.0, 0.5, 0.0);
    samples[8500] = f32::NAN;
    let mut last = (0, 0);
    let report = analyze(&samples, 1000, 1, 0..samples.len(), |done, total| {
        assert!(done <= total);
        assert!(done >= last.0);
        last = (done, total);
        true
    })
    .unwrap();
    assert_eq!(report.range, 0..8000);
    assert_eq!(report.proposal.unwrap().root, 45);
    assert_eq!(last.0, last.1);
}
