// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]
use super::*;
use crate::modal::Mode;

const RATE: f64 = 48000.0;

pub(crate) fn held_layer(modes: usize) -> Sustain {
    let mut layer = Sustain {
        vibrato_hz: 5.5,
        vibrato_cents: 30.0,
        vibrato_delay: 0.2,
        vibrato_fade: 0.3,
        drift_cents: 2.0,
        drift_hz: 3.0,
        release_seconds: 0.6,
        body_fixed: 1.0,
        attack_seconds: 0.1,
        noise_rise: 0.1,
        ..Sustain::NONE
    };
    layer.noise[5] = 1e-4;
    for (i, onset) in layer.onset.iter_mut().take(modes).enumerate() {
        *onset = 0.01 * i as f32;
    }
    layer.rise[..modes].fill(0.15);
    layer.am_slope[..modes].fill(0.05);
    layer.am_curve[..modes].fill(-0.001);
    layer
}

/// One way to break a layer.
type Breakage = Box<dyn Fn(&mut Sustain)>;

#[test]
fn fields_round_trip_and_every_bound_is_checked() {
    let layer = held_layer(3);
    assert_eq!(Sustain::from_fields(&layer.fields()), layer);
    assert!(layer.validate(3).is_ok());
    assert!(Sustain::NONE.is_empty() && !layer.is_empty());
    assert!(Sustain::NONE.validate(0).is_ok());
    let cases: Vec<Breakage> = vec![
        Box::new(|s| s.vibrato_hz = 2.0),
        Box::new(|s| s.vibrato_hz = 11.0),
        Box::new(|s| s.vibrato_cents = 4.0),
        Box::new(|s| s.vibrato_cents = 201.0),
        Box::new(|s| s.vibrato_delay = -0.1),
        Box::new(|s| s.vibrato_fade = 5.0),
        Box::new(|s| {
            s.vibrato_hz = 0.0;
        }),
        Box::new(|s| s.tremolo_depth = 0.5),
        Box::new(|s| {
            s.tremolo_hz = 4.0;
            s.tremolo_depth = 0.95;
        }),
        Box::new(|s| s.drift_cents = 0.5),
        Box::new(|s| s.drift_hz = 0.0),
        Box::new(|s| s.release_seconds = 0.001),
        Box::new(|s| s.release_seconds = 30.0),
        Box::new(|s| s.body_fixed = 1.5),
        Box::new(|s| s.attack_seconds = 0.3),
        Box::new(|s| s.noise_onset = 2.0),
        Box::new(|s| s.noise_rise = -1.0),
        Box::new(|s| s.reserved = 1.0),
        Box::new(|s| s.noise[0] = 2.0),
        Box::new(|s| s.noise[1] = f32::NAN),
        Box::new(|s| s.onset[0] = 1.5),
        Box::new(|s| s.rise[1] = 5.0),
        Box::new(|s| s.am_slope[2] = 0.6),
        Box::new(|s| s.am_curve[2] = 0.02),
        // An entry beyond the recording's modes.
        Box::new(|s| s.rise[3] = 0.1),
        Box::new(|s| s.vibrato_cents = f32::INFINITY),
    ];
    for (index, case) in cases.iter().enumerate() {
        let mut bad = layer;
        case(&mut bad);
        assert!(bad.validate(3).is_err(), "case {index}");
    }
}

#[test]
fn the_envelope_passes_the_peaks_bridges_dips_and_extends_with_the_top_tilt() {
    // Harmonics of a tone whose body peaks at ratio 6 (log2 6 = 2.58 octaves).
    let formant: Vec<(f64, f64)> = (1..=16)
        .map(|k| {
            let octaves = (k as f64 / 6.0).log2();
            (k as f64, 0.06 * (-octaves * octaves / (2.0 * 0.5 * 0.5)).exp())
        })
        .collect();
    let table = envelope(&formant);
    for (ratio, magnitude) in &formant {
        // Through every peak, within the 1/8-octave grid's interpolation.
        assert!((envelope_at(&table, *ratio) - 20.0 * magnitude.log10()).abs() < 0.6, "{ratio}");
    }
    // Flat below the lowest mode; above the highest it keeps falling, at most 24 dB/octave.
    assert!((envelope_at(&table, 0.25) - envelope_at(&table, 1.0)).abs() < 1e-3);
    let above = envelope_at(&table, 32.0) - envelope_at(&table, 16.0);
    assert!((-24.0..0.0).contains(&above), "{above}");
    // A clarinet's even partials, 20 dB down, are bridged: they are the source, not the body.
    let clarinet: Vec<(f64, f64)> =
        (1..=16).map(|k| (k as f64, 0.3 / k as f64 * if k % 2 == 1 { 1.0 } else { 0.1 })).collect();
    let table = envelope(&clarinet);
    for k in [4.0, 6.0, 8.0] {
        let bridge = (envelope_at(&table, k - 1.0) + envelope_at(&table, k + 1.0)) / 2.0;
        assert!((envelope_at(&table, k) - bridge).abs() < 1.5, "{k}");
    }
    // Hostile inputs give a flat zero table, never a panic.
    assert!(envelope(&[]).iter().all(|v| *v == 0.0));
    assert!(envelope(&[(f64::NAN, 1.0), (-1.0, 1.0), (2.0, 0.0)]).iter().all(|v| *v == 0.0));
    assert_eq!(envelope_at(&table, f64::NAN), 0.0);
    assert!(envelope_at(&table, 1e9).is_finite() && envelope_at(&table, 1e-9).is_finite());
}

#[test]
fn the_noise_gain_matches_its_closed_form() {
    let gain = noise_gain(RATE);
    assert!((gain / RATE.sqrt() - 0.6007).abs() < 1e-3, "{gain}");
}

fn one_sine() -> ModalBank<MAX_VOICE_SINES> {
    let mut bank = ModalBank::default();
    bank.strike(RATE, &[Mode { frequency_hz: 1000.0, decay_seconds: 60.0, amplitude: 0.5 }]);
    bank
}

/// Instantaneous frequency (Hz) of `signal` near `nominal`: the phase advance of its complex
/// demodulation, averaged over blocks of 96 samples (four periods of the 2000 Hz image).
fn frequencies(signal: &[f32], nominal: f64) -> Vec<f64> {
    let mut phases = Vec::new();
    for (b, block) in signal.as_chunks::<96>().0.iter().enumerate() {
        let (mut re, mut im) = (0.0, 0.0);
        for (n, x) in block.iter().enumerate() {
            let angle = TAU * nominal * (b * 96 + n) as f64 / RATE;
            re += f64::from(*x) * angle.cos();
            im -= f64::from(*x) * angle.sin();
        }
        phases.push(im.atan2(re));
    }
    phases
        .windows(2)
        .map(|w| {
            let advance = (w[1] - w[0] + PI).rem_euclid(TAU) - PI;
            nominal + advance * RATE / (TAU * 96.0)
        })
        .collect()
}

#[test]
fn an_inactive_layer_plays_the_bank_unchanged() {
    let mut plain = one_sine();
    let mut played = one_sine();
    let mut performer = Performer::default();
    performer.start(&Held::NONE, RATE, 0.0, 7);
    assert!(!performer.is_active());
    for _ in 0..4096 {
        assert_eq!(plain.next_sample(), performer.next(&mut played));
    }
}

#[test]
fn vibrato_moves_the_pitch_by_its_depth_at_its_rate_after_its_delay() {
    let mut held = Held::NONE;
    held.vibrato_hz = 6.0;
    held.vibrato_cents = 50.0;
    held.vibrato_delay = 0.25;
    held.vibrato_fade = 0.0;
    let mut bank = one_sine();
    let mut performer = Performer::default();
    performer.start(&held, RATE, 0.0, 1);
    let output: Vec<f32> = (0..48000).map(|_| performer.next(&mut bank)).collect();
    let hz = frequencies(&output, 1000.0);
    let cents: Vec<f64> = hz.iter().map(|f| 1200.0 * (f / 1000.0).log2()).collect();
    // 500 blocks of 96 samples per second: before the delay the pitch is steady.
    assert!(cents[2..100].iter().all(|c| c.abs() < 1.0), "{:?}", &cents[..100]);
    let late = &cents[200..];
    let (low, high) = late.iter().fold((f64::MAX, f64::MIN), |(l, h), c| (l.min(*c), h.max(*c)));
    assert!((high - 50.0).abs() < 4.0 && (low + 50.0).abs() < 4.0, "{low} {high}");
    // Six upward zero crossings per second.
    let crossings = late.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
    assert!((crossings as f64 - 6.0 * late.len() as f64 / 500.0).abs() <= 1.0, "{crossings}");
}

#[test]
fn onsets_swell_in_and_the_release_sets_the_damper_and_fades_the_noise() {
    let mut held = Held::NONE;
    held.onset[0] = 0.1;
    held.rise[0] = 0.2;
    held.release_seconds = 0.5;
    held.noise[5] = 1e-4;
    let mut bank = one_sine();
    let mut performer = Performer::default();
    performer.start(&held, RATE, 0.0, 1);
    let output: Vec<f32> = (0..48000).map(|_| performer.next(&mut bank)).collect();
    let peak = |range: std::ops::Range<usize>| output[range].iter().fold(0.0_f32, |m, x| m.max(x.abs()));
    // Before the onset only the noise (well under the sine) sounds.
    assert!(peak(0..4500) < 0.05, "{}", peak(0..4500));
    // 90 % at onset + rise (0.3 s), within the control ramp.
    // The sine itself decays with its 60 s T60: 10^(-3 t / 60) of 0.5 at time t.
    let level = |t: f64| 0.5 * 10.0_f64.powf(-3.0 * t / 60.0);
    let at_rise = f64::from(peak(14200..14600));
    assert!((at_rise / level(0.3) - 0.9).abs() < 0.05, "{at_rise}");
    let late = f64::from(peak(40000..48000));
    assert!((late / level(0.85) - 1.0).abs() < 0.05, "{late}");
    // The measured release at the Release control's default, and twice it.
    assert!((performer.release(0.25) - 0.5).abs() < 1e-9);
    assert!(performer.sounding());
    let mut fallen = 0;
    while !performer.finished() && fallen < 96000 {
        performer.next(&mut bank);
        fallen += 1;
    }
    // The noise is 60 dB down after T60 = 0.5 s.
    assert!((fallen as f64 / RATE - 0.5).abs() < 0.01, "{fallen}");
    assert!(!performer.sounding());
    let mut plain = held;
    plain.release_seconds = 0.0;
    let mut none = Performer::default();
    none.start(&plain, RATE, 0.0, 1);
    assert_eq!(none.release(0.3), 0.3, "no measured release: the Release control");
    let mut inactive = Performer::default();
    assert_eq!(inactive.release(0.7), 0.7);
}

#[test]
fn one_noise_band_has_the_level_its_density_asks_for() {
    let mut held = Held::NONE;
    let band = 6;
    held.noise[band] = 2e-3;
    let mut bank = ModalBank::<MAX_VOICE_SINES>::default();
    let mut performer = Performer::default();
    performer.start(&held, RATE, 0.0, 99);
    assert!(performer.sounding());
    let output: Vec<f64> = (0..96000).map(|_| f64::from(performer.next(&mut bank))).collect();
    let tail = &output[4800..];
    let variance = tail.iter().map(|x| x * x).sum::<f64>() / tail.len() as f64;
    let center = noise_center_hz(band);
    // Unit noise through a unity-peak band-pass: (2 / fs) * (pi / 2) * (f0 / Q).
    let expected = (noise_gain(RATE) * 2e-3).powi(2) * PI * center / (NOISE_Q * RATE);
    let error_db = 10.0 * (variance / expected).log10();
    assert!(error_db.abs() < 0.5, "{error_db} dB");
    // The same seed repeats exactly.
    let mut again = Performer::default();
    let mut other = ModalBank::<MAX_VOICE_SINES>::default();
    again.start(&held, RATE, 0.0, 99);
    for x in output.iter().take(512) {
        assert_eq!(*x as f32, again.next(&mut other));
    }
}

#[test]
fn hostile_starts_stay_inactive() {
    let mut held = Held::NONE;
    held.vibrato_hz = 6.0;
    held.vibrato_cents = 20.0;
    for rate in [f64::NAN, 0.0, 999.0, 1e9] {
        let mut performer = Performer::default();
        performer.start(&held, rate, 0.0, 1);
        assert!(!performer.is_active());
    }
    let mut performer = Performer::default();
    performer.start(&held, RATE, f64::NAN, 1);
    let mut bank = one_sine();
    assert!((0..4800).all(|_| performer.next(&mut bank).is_finite()));
}
