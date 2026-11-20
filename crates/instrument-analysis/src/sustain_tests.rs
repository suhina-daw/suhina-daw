// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]
use super::*;
use crate::modal::{Outcome, fit};

const R: f64 = 22050.0;

/// A harmonic tone with sine vibrato: no depth before `delay`, full depth after `delay + fade`.
/// `level(k, t, cents)` scales harmonic k (1-based).
fn tone(
    seconds: f64,
    f0: f64,
    harmonics: usize,
    vibrato: (f64, f64, f64, f64),
    level: impl Fn(usize, f64, f64) -> f64,
) -> Vec<f64> {
    let (rate, cents, delay, fade) = vibrato;
    let mut phase = 0.0;
    (0..(seconds * R) as usize)
        .map(|n| {
            let t = n as f64 / R;
            let depth = if t < delay {
                0.0
            } else if fade > 0.0 {
                ((t - delay) / fade).min(1.0)
            } else {
                1.0
            };
            let deviation = depth * cents * (TAU * rate * t).sin();
            let sample = (1..=harmonics).map(|k| level(k, t, deviation) * (k as f64 * phase).sin()).sum();
            phase += TAU * f0 * 2.0_f64.powf(deviation / 1200.0) / R;
            sample
        })
        .collect()
}

fn plain(k: usize, _: f64, _: f64) -> f64 {
    0.3 / k as f64
}

#[test]
fn vibrato_rate_depth_delay_and_fade_are_recovered() {
    for (f0, cents) in [(130.8, 25.0), (130.8, 60.0), (440.0, 25.0), (440.0, 60.0)] {
        let x = tone(3.0, f0, 8, (5.5, cents, 0.4, 0.3), plain);
        let pitch = pitch(&x).unwrap();
        let v = vibrato(&pitch).unwrap();
        assert!((v.rate - 5.5).abs() < 0.1, "{f0} Hz, {cents} cents: {v:?}");
        assert!((v.cents - cents).abs() <= (0.15 * cents).max(3.0), "{f0} Hz, {cents} cents: {v:?}");
        assert!((v.delay - 0.4).abs() < 0.15 && (v.fade - 0.3).abs() < 0.15, "{f0} Hz, {cents} cents: {v:?}");
    }
}

#[test]
fn flattening_leaves_one_frequency_per_partial() {
    let x = tone(3.0, 196.0, 8, (5.5, 50.0, 0.3, 0.3), plain);
    let p = pitch(&x).unwrap();
    let v = vibrato(&p).unwrap();
    let flat = flatten(&x, &p, &v);
    assert!((flat.len() as f64 - x.len() as f64).abs() < 0.01 * x.len() as f64);
    let again = pitch(&flat).unwrap();
    assert!(vibrato(&again).is_none_or(|v| v.cents < MIN_CENTS));
    let remaining = rms(&detrend(&again.curve));
    assert!(remaining < 3.0, "{remaining} cents remain");
}

#[test]
fn steady_and_beating_tones_have_no_vibrato() {
    // A steady harmonic tone.
    let steady = tone(2.0, 220.0, 6, (5.5, 0.0, 0.0, 0.0), plain);
    assert!(pitch(&steady).and_then(|p| vibrato(&p)).is_none());
    // A struck tone of two beating pairs.
    let ln1000 = 1000.0_f64.ln();
    let struck: Vec<f64> = (0..(3.0 * R) as usize)
        .map(|n| {
            let t = n as f64 / R;
            let decay = (-ln1000 * t / 3.0).exp();
            0.3 * decay
                * ((TAU * 440.0 * t).sin()
                    + (TAU * 441.5 * t).sin()
                    + 0.5 * (TAU * 883.0 * t).sin()
                    + 0.5 * (TAU * 885.0 * t).sin())
        })
        .collect();
    assert!(pitch(&struck).and_then(|p| vibrato(&p)).is_none());
    // One held beating pair, 5 Hz apart: its level dips as its frequency wobbles.
    for partner in [0.3, 0.4, 0.5] {
        let pair: Vec<f64> = (0..(3.0 * R) as usize)
            .map(|n| {
                let t = n as f64 / R;
                0.3 * ((TAU * 440.0 * t).sin() + partner * (TAU * 445.0 * t).sin())
            })
            .collect();
        assert!(pitch(&pair).and_then(|p| vibrato(&p)).is_none(), "partner {partner}");
    }
    // Too short, silent or hostile input never panics.
    assert!(pitch(&[0.0; 1000]).is_none());
    assert!(pitch(&[0.0; 44100]).is_none());
}

struct Gaussian(u32);
impl Gaussian {
    fn uniform(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (f64::from(self.0) + 1.0) / (f64::from(u32::MAX) + 2.0)
    }
    fn next(&mut self) -> f64 {
        let (a, b) = (self.uniform(), self.uniform());
        (-2.0 * a.ln()).sqrt() * (TAU * b).cos()
    }
}

fn inputs(f0: f64, magnitudes: &[f64]) -> Vec<ModeInput> {
    magnitudes
        .iter()
        .enumerate()
        .map(|(i, m)| ModeInput { hz: f0 * (i + 1) as f64, partner_hz: None, magnitude: *m, sustained: true })
        .collect()
}

#[test]
fn band_noise_is_measured_beside_the_partials() {
    let magnitudes: Vec<f64> = (1..=5).map(|k| 0.3 / f64::from(k)).collect();
    let sum: f64 = magnitudes.iter().sum();
    let sigma = 0.01;
    let mut random = Gaussian(5);
    let clean = tone(3.0, 220.0, 5, (5.5, 0.0, 0.0, 0.0), plain);
    let noisy: Vec<f64> = clean.iter().map(|x| x + sigma * random.next()).collect();
    let modes = inputs(220.0, &magnitudes);
    let layer = layer(&noisy, &modes, None, None, true, sum).unwrap();
    // One-sided density of white noise: 2 sigma^2 / rate.
    let expected = (2.0 * sigma * sigma / R).sqrt() / sum;
    for band in 3..NOISE_BANDS {
        let measured = f64::from(layer.noise[band]);
        let error = 20.0 * (measured / expected).log10();
        assert!(error.abs() < 3.0, "band {band}: {error} dB");
    }
    // The partials alone are not noise.
    let quiet = layer_of(&clean, &modes, sum);
    for (band, value) in quiet.noise.iter().enumerate() {
        let width = noise_center_hz(band) * (2.0_f64.powf(1.0 / 3.0) - 2.0_f64.powf(-1.0 / 3.0));
        assert!(f64::from(*value) * width.sqrt() < 1e-2, "band {band}: {value}");
    }
}

fn layer_of(x: &[f64], modes: &[ModeInput], sum: f64) -> Sustain {
    layer(x, modes, None, None, true, sum).unwrap()
}

#[test]
fn onsets_rises_and_a_release_are_measured() {
    // Three partials swelling in one after another, held, then released with T60 0.8 s.
    let onsets = [0.0, 0.15, 0.3];
    let rise = 0.25;
    let ln1000 = 1000.0_f64.ln();
    let x = tone(3.0, 300.0, 3, (5.5, 0.0, 0.0, 0.0), |k, t, _| {
        let onset = onsets[k - 1];
        let swell = if t < onset { 0.0 } else { 1.0 - (-(t - onset) * std::f64::consts::LN_10 / rise).exp() };
        let release = if t < 1.2 { 1.0 } else { (-ln1000 * (t - 1.2) / 0.8).exp() };
        0.3 / k as f64 * swell * release
    });
    let magnitudes = [0.3, 0.15, 0.1];
    let layer = layer_of(&x, &inputs(300.0, &magnitudes), magnitudes.iter().sum());
    for (k, onset) in onsets.iter().enumerate() {
        let (o, r) = (f64::from(layer.onset[k]), f64::from(layer.rise[k]));
        assert!((o - onset).abs() < 0.05, "partial {k}: onset {o}");
        assert!((r - rise).abs() < (0.2 * rise).max(0.05), "partial {k}: rise {r}");
    }
    let release = f64::from(layer.release_seconds);
    assert!((release - 0.8).abs() < 0.16, "release {release}");
    assert_eq!(layer.body_fixed, 1.0);
    assert!(layer.validate(3).is_ok());
}

#[test]
fn a_partial_climbing_a_resonance_with_the_vibrato_is_coupled_to_it() {
    // The second harmonic gains 0.1 dB per cent of deviation; the others stay put.
    let x = tone(3.0, 196.0, 4, (5.5, 40.0, 0.2, 0.2), |k, _, cents| {
        let level = 0.3 / k as f64;
        if k == 2 { level * 10.0_f64.powf(0.1 * cents / 20.0) } else { level }
    });
    let p = pitch(&x).unwrap();
    let v = vibrato(&p).unwrap();
    let flat = flatten(&x, &p, &v);
    let magnitudes = [0.3, 0.15, 0.1, 0.075];
    let layer = layer(&flat, &inputs(196.0, &magnitudes), Some(&p), Some(&v), true, 0.625).unwrap();
    assert!((f64::from(layer.am_slope[1]) - 0.1).abs() < 0.03, "{:?}", &layer.am_slope[..4]);
    assert!(f64::from(layer.am_slope[0]).abs() < 0.03, "{:?}", &layer.am_slope[..4]);
    assert_eq!(layer.vibrato_hz, v.rate as f32);
}

#[test]
fn a_tremolo_without_vibrato_is_found() {
    let x = tone(3.0, 261.6, 4, (5.5, 0.0, 0.0, 0.0), |k, t, _| 0.3 / k as f64 * (1.0 + 0.3 * (TAU * 4.5 * t).sin()));
    let magnitudes = [0.3, 0.15, 0.1, 0.075];
    let layer = layer_of(&x, &inputs(261.6, &magnitudes), 0.625);
    assert!((f64::from(layer.tremolo_hz) - 4.5).abs() < 0.15, "{}", layer.tremolo_hz);
    assert!((f64::from(layer.tremolo_depth) - 0.3).abs() < 0.1, "{}", layer.tremolo_depth);
}

#[test]
fn a_bowed_like_tone_is_fitted_with_its_held_layer() {
    // G3 (key 55) with 40 cents of vibrato at 5.5 Hz after 0.3 s, a 0.15 s swell and noise.
    let mut random = Gaussian(11);
    let x = tone(3.0, 196.0, 8, (5.5, 40.0, 0.3, 0.3), |k, t, _| {
        0.3 / k as f64 * (1.0 - (-t * std::f64::consts::LN_10 / 0.15).exp())
    });
    let pcm: Vec<f32> = x.iter().map(|s| (s + 0.002 * random.next()) as f32).collect();
    let report = fit(&pcm, 22050, 1, 0..pcm.len(), 55, |_, _| true).unwrap();
    assert_eq!(report.outcome, Outcome::Fitted, "{report:?}");
    let recording = report.recording.as_ref().unwrap();
    let layer = recording.sustain.unwrap();
    assert!((f64::from(layer.vibrato_hz) - 5.5).abs() < 0.15, "{layer:?}");
    assert!((f64::from(layer.vibrato_cents) - 40.0).abs() < 8.0, "{layer:?}");
    assert_eq!(layer.body_fixed, 1.0);
    assert!(layer.noise.iter().any(|v| *v > 0.0), "{layer:?}");
    let harmonics = recording
        .modes
        .iter()
        .filter(|m| (m.ratio - m.ratio.round()).abs() < 0.01 * m.ratio && m.decay_seconds == 60.0)
        .count();
    assert!(harmonics >= 5, "{:?}", recording.modes);
    assert!(report.recording_modes.iter().filter(|m| m.sustained).count() >= 5);
    // Deterministic.
    assert_eq!(report, fit(&pcm, 22050, 1, 0..pcm.len(), 55, |_, _| true).unwrap());
}

#[test]
fn interpolation_and_medians_handle_edges() {
    assert_eq!(interpolate(&[], 1.0), 0.0);
    let points = [(0.0, 1.0), (1.0, 3.0)];
    assert_eq!(interpolate(&points, -1.0), 1.0);
    assert_eq!(interpolate(&points, 0.5), 2.0);
    assert_eq!(interpolate(&points, 2.0), 3.0);
    assert_eq!(median(&mut []), 0.0);
    assert_eq!(median(&mut [3.0, 1.0, 2.0]), 2.0);
    assert_eq!(median(&mut [4.0, 1.0, 2.0, 3.0]), 2.5);
    assert!(line(&[(1.0, 1.0)]).is_none());
}
