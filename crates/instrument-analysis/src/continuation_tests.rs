// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

const RATE: u32 = 16_000;

/// A self-made held tone: harmonics of 440 Hz falling 4 dB each, a 5.5 Hz vibrato of `cents`
/// starting at full depth, a little noise, `seconds` long; stereo with the right channel a
/// quieter, delayed copy when `stereo`, else both channels equal. A release over its last 0.3 s
/// when `release`.
fn tone(seconds: f64, cents: f64, stereo: bool, release: bool) -> Vec<f32> {
    let sr = f64::from(RATE);
    let frames = (seconds * sr) as usize;
    let mut phase = 0.0;
    let mut noise = 12345_u32;
    let mut out = Vec::with_capacity(frames * 2);
    for n in 0..frames {
        let t = n as f64 / sr;
        let f = 440.0 * 2f64.powf(cents * (TAU * 5.5 * t).sin() / 1200.0);
        phase += TAU * f / sr;
        let mut x = 0.0;
        for h in 1..=12 {
            x += 0.15 * 10f64.powf(-4.0 * (h - 1) as f64 / 20.0) * (h as f64 * phase).sin();
        }
        noise ^= noise << 13;
        noise ^= noise >> 17;
        noise ^= noise << 5;
        x += 0.002 * (f64::from(noise) / f64::from(u32::MAX) - 0.5);
        let envelope = if release && t > seconds - 0.3 { (-(t - (seconds - 0.3)) * 20.0).exp() } else { 1.0 };
        let left = x * envelope;
        let right = if stereo { 0.7 * left + 0.05 * (TAU * 3.0 * t).sin() * left } else { left };
        out.push(left as f32);
        out.push(right as f32);
    }
    out
}

fn held(start: f64, end: f64) -> Range<usize> {
    (start * f64::from(RATE)) as usize..(end * f64::from(RATE)) as usize
}

#[test]
fn a_held_tone_with_vibrato_is_fitted() {
    let samples = tone(2.5, 25.0, false, false);
    let continuation = fit(&samples, RATE, 2, held(0.3, 2.2), 440.0, |_, _| true).unwrap();
    assert!((f64::from(continuation.f0) - 440.0).abs() < 3.0, "f0 {}", continuation.f0);
    assert!((f64::from(continuation.vibrato_hz) - 5.5).abs() < 0.15, "rate {}", continuation.vibrato_hz);
    assert!((f64::from(continuation.vibrato_cents) - 25.0).abs() < 5.0, "depth {}", continuation.vibrato_cents);
    // Harmonics up to 0.45 of the rate: 16 of 440 Hz at 16 kHz.
    assert_eq!(continuation.partials.len(), 16);
    // The carriers fall 4 dB per harmonic, as made (the first ten, well above the noise).
    let first = C::from_pair(continuation.partials[0].carrier).abs();
    assert!((first - 0.15).abs() < 0.03, "first carrier {first}");
    for k in 1..10 {
        let ratio = C::from_pair(continuation.partials[k].carrier).abs() / first;
        let db = 20.0 * ratio.log10();
        assert!((db + 4.0 * k as f64).abs() < 2.0, "harmonic {k}: {db} dB");
        // A tone whose every partial follows the pitch.
        assert!(continuation.partials[k].gamma > 0.7, "harmonic {k}: gamma {}", continuation.partials[k].gamma);
    }
    assert!(!continuation.stereo, "equal channels are mono");
    assert!(continuation.join > 1.4 && continuation.join <= 2.2, "join {}", continuation.join);
    assert_eq!(continuation.release_start, 0.0);
    assert_eq!(Continuation::decode(&continuation.encode()).unwrap(), continuation);
}

#[test]
fn the_fitted_model_plays_at_the_recordings_level() {
    let samples = tone(2.5, 25.0, true, false);
    let continuation = fit(&samples, RATE, 2, held(0.3, 2.2), 440.0, |_, _| true).unwrap();
    assert!(continuation.stereo);
    let sr = f64::from(RATE);
    let prepared = Prepared::new(&continuation, sr);
    let mut generator = Generator::new(&prepared, None);
    let mut model = NoteModel::default();
    model.transposed(&continuation, 1.0, sr);
    generator.start(&model, 0.0, 1);
    let out: Vec<[f32; 2]> = (0..(3.0 * sr) as usize).map(|_| generator.next()).collect();
    let mid: Vec<f64> = out.iter().skip(sr as usize).map(|p| f64::from(p[0])).collect();
    let side: Vec<f64> = out.iter().skip(sr as usize).map(|p| f64::from(p[1])).collect();
    // The recording's mid over its held part.
    let recorded: Vec<f64> = samples
        .chunks_exact(2)
        .skip((0.5 * sr) as usize)
        .take((1.5 * sr) as usize)
        .map(|p| (f64::from(p[0]) + f64::from(p[1])) / 2.0)
        .collect();
    let recorded_side: Vec<f64> = samples
        .chunks_exact(2)
        .skip((0.5 * sr) as usize)
        .take((1.5 * sr) as usize)
        .map(|p| (f64::from(p[0]) - f64::from(p[1])) / 2.0)
        .collect();
    let db = |a: f64, b: f64| 20.0 * (a / b).log10();
    let level = db(rms(&mid), rms(&recorded));
    assert!(level.abs() < 2.0, "model {level} dB from the recording");
    let width = db(rms(&side), rms(&mid)) - db(rms(&recorded_side), rms(&recorded));
    assert!(width.abs() < 3.0, "side to mid {width} dB from the recording's");
}

#[test]
fn a_release_after_the_held_part_is_found() {
    let samples = tone(2.5, 20.0, false, true);
    let continuation = fit(&samples, RATE, 2, held(0.3, 2.0), 440.0, |_, _| true).unwrap();
    let start = f64::from(continuation.release_start);
    assert!(start > 2.1 && start < 2.3, "release at {start}");
}

#[test]
fn bad_input_is_refused() {
    let samples = tone(1.5, 20.0, false, false);
    assert_eq!(fit(&samples, RATE, 3, held(0.2, 1.2), 440.0, |_, _| true), Err(FitError::InvalidFormat));
    assert_eq!(fit(&samples, 4000, 2, held(0.2, 1.2), 440.0, |_, _| true), Err(FitError::InvalidFormat));
    assert_eq!(fit(&samples, RATE, 2, held(0.2, 0.4), 440.0, |_, _| true), Err(FitError::InvalidRange));
    assert_eq!(fit(&samples, RATE, 2, held(0.2, 9.0), 440.0, |_, _| true), Err(FitError::InvalidRange));
    assert_eq!(fit(&samples, RATE, 2, held(0.2, 1.2), f64::NAN, |_, _| true), Err(FitError::NoPitch));
    let mut broken = samples.clone();
    broken[100] = f32::NAN;
    assert_eq!(fit(&broken, RATE, 2, held(0.2, 1.2), 440.0, |_, _| true), Err(FitError::NonFinite));
    let silent = vec![0.0_f32; samples.len()];
    assert!(fit(&silent, RATE, 2, held(0.2, 1.2), 440.0, |_, _| true).is_err());
}

#[test]
fn fitting_can_be_cancelled() {
    let samples = tone(1.5, 20.0, false, false);
    let mut calls = 0;
    let result = fit(&samples, RATE, 2, held(0.2, 1.3), 440.0, |_, _| {
        calls += 1;
        calls < 3
    });
    assert_eq!(result, Err(FitError::Cancelled));
}
