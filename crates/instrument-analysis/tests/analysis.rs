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
    let reversed = std::ops::Range { start: 3, end: 2 };
    assert_eq!(analyze(&samples, RATE, 1, reversed, |_, _| true), Err(Error::InvalidRange));
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

/// A harmonic note (six partials at 1/k) decaying over its duration, at the analysis rate.
fn note(midi: f64, seconds: f64) -> Vec<f64> {
    let hz = 440.0 * 2.0f64.powf((midi - 69.0) / 12.0);
    (0..(f64::from(RATE) * seconds) as usize)
        .map(|i| {
            let t = i as f64 / f64::from(RATE);
            let tone: f64 = (1..=6)
                .map(f64::from)
                .filter(|k| hz * k < 9000.0)
                .map(|k| (std::f64::consts::TAU * hz * k * t).sin() / k)
                .sum();
            0.4 * tone * (-t / 1.5 * 3.0).exp()
        })
        .collect()
}

/// Notes (start seconds, MIDI keys, length) summed into `seconds` of audio.
fn phrase(events: &[(f64, &[f64], f64)], seconds: f64) -> Vec<f32> {
    let mut out = vec![0.0f64; (f64::from(RATE) * seconds) as usize];
    for (start, keys, length) in events {
        for key in *keys {
            let first = (start * f64::from(RATE)) as usize;
            for (sample, value) in out.iter_mut().skip(first).zip(note(*key, *length)) {
                *sample += value;
            }
        }
    }
    out.into_iter().map(|x| x as f32).collect()
}

fn melody(keys: &[f64]) -> Vec<f32> {
    let events: Vec<_> =
        keys.iter().enumerate().map(|(i, key)| (i as f64 * 0.5, std::slice::from_ref(key), 0.5)).collect();
    phrase(&events, keys.len() as f64 * 0.5)
}

#[test]
fn melodic_and_chordal_loops_propose_the_tonic_of_their_key() {
    use suhina_instrument_analysis::KeyMode::{Major, Minor};
    // A natural minor melody A3 C4 E4 D4 C4 B3 A3 E3; C F G C triads; an E minor bass line.
    let cases: [(&str, Vec<f32>, u8, suhina_instrument_analysis::KeyMode, u8); 3] = [
        ("melody", melody(&[57.0, 60.0, 64.0, 62.0, 60.0, 59.0, 57.0, 52.0]), 9, Minor, 57),
        (
            "chords",
            phrase(
                &[
                    (0.0, &[48.0, 52.0, 55.0][..], 1.0),
                    (1.0, &[53.0, 57.0, 60.0][..], 1.0),
                    (2.0, &[55.0, 59.0, 62.0][..], 1.0),
                    (3.0, &[48.0, 52.0, 55.0][..], 1.0),
                ],
                4.0,
            ),
            0,
            Major,
            48,
        ),
        ("bass", melody(&[40.0, 43.0, 45.0, 47.0, 40.0, 47.0, 45.0, 43.0]), 4, Minor, 40),
    ];
    for (name, samples, tonic, mode, root) in cases {
        let report = analyze(&samples, RATE, 1, 0..samples.len(), |_, _| true).unwrap();
        assert_eq!(report.outcome, Outcome::TonalCentre, "{name}: {report:?}");
        assert_eq!(report.proposal, None, "{name}: no single note");
        let key = report.key.unwrap();
        assert_eq!((key.key.tonic, key.key.mode, key.root), (tonic, mode, root), "{name}: {key:?}");
        assert!(key.key.correlation >= key.runner_up.correlation && key.key.correlation >= 0.6, "{name}: {key:?}");
        assert!(key.tuning_cents.abs() < 5.0, "{name}: {key:?}");
        // Opposite polarity in stereo cancels nothing: magnitudes are summed per channel.
        let stereo: Vec<_> = samples.iter().flat_map(|x| [*x, -*x]).collect();
        let both = analyze(&stereo, RATE, 2, 0..samples.len(), |_, _| true).unwrap().key.unwrap();
        assert_eq!((both.key.tonic, both.key.mode, both.root), (tonic, mode, root), "{name} stereo");
    }
}

#[test]
fn a_detuned_loop_reports_its_tuning_and_keeps_its_key() {
    let detuned: Vec<_> = [57.0, 60.0, 64.0, 62.0, 60.0, 59.0, 57.0, 52.0].iter().map(|k| k + 0.3).collect();
    let samples = melody(&detuned);
    let key = analyze(&samples, RATE, 1, 0..samples.len(), |_, _| true).unwrap().key.unwrap();
    assert_eq!((key.key.tonic, key.root), (9, 57));
    assert!((key.tuning_cents - 30.0).abs() < 5.0, "{key:?}");
}

#[test]
fn unpitched_loops_and_single_notes_get_no_key() {
    // Decaying noise bursts on every quarter beat at 120 BPM: no prominent partials.
    let mut seed = 0x1234_5678u32;
    let mut drums = vec![0.0f32; RATE as usize * 4];
    for hit in 0..16 {
        let first = hit * RATE as usize / 4;
        for (i, sample) in drums.iter_mut().skip(first).take(RATE as usize / 12).enumerate() {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *sample += (seed as f32 / u32::MAX as f32 - 0.5) * (-(i as f32) / RATE as f32 * 40.0).exp();
        }
    }
    let report = analyze(&drums, RATE, 1, 0..drums.len(), |_, _| true).unwrap();
    assert_eq!((report.outcome, report.key), (Outcome::NoStablePitch, None), "{report:?}");
    // A single stable note keeps its note proposal; the key path is not used.
    let single = tone(RATE, 220.0, 2.0, 0.5, 0.0);
    let report = analyze(&single, RATE, 1, 0..single.len(), |_, _| true).unwrap();
    assert_eq!((report.outcome, report.key), (Outcome::Stable, None));
}
