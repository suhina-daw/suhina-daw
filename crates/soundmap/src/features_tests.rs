// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn sine(hz: f32, seconds: f32, rate: u32) -> Vec<f32> {
    let n = (seconds * rate as f32) as usize;
    (0..n).map(|i| (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin() * 0.5).collect()
}

/// A deterministic noise source, so tests do not depend on a random crate.
fn noise(seconds: f32, rate: u32) -> Vec<f32> {
    let mut state = 0x1234_5678u32;
    (0..(seconds * rate as f32) as usize)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as f32 / u32::MAX as f32) - 0.5
        })
        .collect()
}

#[test]
fn silence_and_empty_input_have_no_features() {
    assert!(analyse(&[], 1, 44_100).is_none());
    assert!(analyse(&[0.0; 44_100], 1, 44_100).is_none());
    assert!(analyse(&[0.5; 100], 0, 44_100).is_none());
    assert!(analyse(&[f32::NAN; 4096], 1, 44_100).is_none());
}

#[test]
fn a_sine_has_its_pitch_and_a_low_flatness() {
    let features = analyse(&sine(220.0, 1.0, 44_100), 1, 44_100).unwrap();
    // A3 is MIDI note 57.
    assert!((features[42] - 57.0).abs() < 0.3, "pitch {}", features[42]);
    assert!(features[43] > 0.9, "voiced share {}", features[43]);
    assert!(features[28] < 0.05, "flatness {}", features[28]);
    // The centroid sits near 220 Hz (log2 of 240).
    assert!((features[26] - 240f32.log2()).abs() < 0.5, "centroid {}", features[26]);
}

#[test]
fn noise_is_flat_bright_and_unvoiced() {
    let noisy = analyse(&noise(1.0, 44_100), 1, 44_100).unwrap();
    let tone = analyse(&sine(220.0, 1.0, 44_100), 1, 44_100).unwrap();
    assert!(noisy[28] > 0.3, "flatness {}", noisy[28]);
    assert!(noisy[26] > tone[26] + 2.0, "centroid {} against {}", noisy[26], tone[26]);
    assert!(noisy[43] < 0.2, "voiced share {}", noisy[43]);
    assert!(noisy[36] > tone[36] * 10.0, "zero crossings {} against {}", noisy[36], tone[36]);
}

#[test]
fn a_short_hit_has_a_short_effective_length() {
    let rate = 44_100;
    let mut hit: Vec<f32> = noise(0.05, rate);
    hit.resize(rate as usize, 0.0);
    let long = analyse(&noise(1.0, rate), 1, rate).unwrap();
    let short = analyse(&hit, 1, rate).unwrap();
    assert!(short[40] < long[40] - 0.5, "length {} against {}", short[40], long[40]);
}

#[test]
fn stereo_and_rate_do_not_change_much() {
    let mono = sine(440.0, 1.0, 44_100);
    let stereo: Vec<f32> = mono.iter().flat_map(|s| [*s, *s]).collect();
    let a = analyse(&mono, 1, 44_100).unwrap();
    let b = analyse(&stereo, 2, 44_100).unwrap();
    assert_eq!(a, b);
    let c = analyse(&sine(440.0, 1.0, 48_000), 1, 48_000).unwrap();
    assert!((a[42] - c[42]).abs() < 0.2);
    assert!((a[26] - c[26]).abs() < 0.2);
}

#[test]
fn only_the_window_is_read() {
    let mut long = sine(220.0, WINDOW_SECONDS as f32, 22_050);
    let first = analyse(&long, 1, 22_050).unwrap();
    long.extend(noise(2.0, 22_050));
    assert_eq!(analyse(&long, 1, 22_050).unwrap(), first);
}

/// A sine whose pitch falls from `from` to `to` Hz over 0.1 s, then holds, fading out.
fn sweep(from: f32, to: f32, seconds: f32, rate: u32) -> Vec<f32> {
    let mut phase = 0.0f32;
    (0..(seconds * rate as f32) as usize)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let hz = to + (from - to) * (-t * 30.0).exp();
            phase += std::f32::consts::TAU * hz / rate as f32;
            phase.sin() * (-t * 4.0).exp() * 0.8
        })
        .collect()
}

#[test]
fn leading_silence_is_skipped() {
    let hit = sweep(150.0, 50.0, 0.5, 44_100);
    let mut late = vec![0.0f32; 44_100];
    // A faint floor, as a recording has, 80 dB down.
    late.iter_mut().enumerate().for_each(|(i, s)| *s = if i % 2 == 0 { 1e-4 } else { -1e-4 });
    late.extend_from_slice(&hit);
    let a = analyse(&hit, 1, 44_100).unwrap();
    let b = analyse(&late, 1, 44_100).unwrap();
    // The frames fall a little differently over the sound, so the numbers move a little.
    for i in 0..FEATURE_COUNT {
        assert!((a[i] - b[i]).abs() <= 0.15 * a[i].abs().max(3.0), "feature {i}: {} against {}", a[i], b[i]);
    }
    // A file silent for longer than the onset is looked for has nothing to analyse.
    let mut silent = vec![0.0f32; (ONSET_SEARCH_SECONDS as usize + 1) * 44_100];
    silent.extend_from_slice(&hit);
    assert!(analyse(&silent, 1, 44_100).is_none());
}

#[test]
fn the_level_does_not_matter() {
    let hit = sweep(150.0, 50.0, 0.5, 44_100);
    let quiet: Vec<f32> = hit.iter().map(|s| s * 0.01).collect();
    let a = analyse(&hit, 1, 44_100).unwrap();
    let b = analyse(&quiet, 1, 44_100).unwrap();
    for i in 0..FEATURE_COUNT {
        assert!((a[i] - b[i]).abs() <= 0.02 * a[i].abs().max(1.0), "feature {i}: {} against {}", a[i], b[i]);
    }
}

#[test]
fn band_shares_say_where_the_energy_is() {
    let low = analyse(&sine(60.0, 1.0, 44_100), 1, 44_100).unwrap();
    // Nearly all of a 60 Hz sine is under 120 Hz.
    assert!(low[BODY_BANDS.start] > -0.5, "{:?}", &low[BODY_BANDS]);
    assert!(low[BODY_BANDS.end - 1] < -40.0, "{:?}", &low[BODY_BANDS]);
    let bright = analyse(&noise(1.0, 44_100), 1, 44_100).unwrap();
    // White noise: most of it above 6 kHz, little under 120 Hz.
    let top = bright[BODY_BANDS.end - 1];
    assert!(top > -5.0 && bright[BODY_BANDS.start] < -15.0, "{:?}", &bright[BODY_BANDS]);
    assert!(bright[ATTACK_BANDS.start] < -10.0, "{:?}", &bright[ATTACK_BANDS]);
    // Each band is a share: the shares add up to the whole.
    let total: f32 = bright[BODY_BANDS].iter().map(|db| 10f32.powf(db / 10.0)).sum();
    assert!((total - 1.0).abs() < 0.01, "{total}");
}

#[test]
fn the_attack_is_read_apart_from_the_body() {
    // A bright click, then a low tone.
    let rate = 44_100;
    let mut hit: Vec<f32> = noise(0.02, rate).iter().map(|s| s * 2.0).collect();
    hit.extend(sine(60.0, 0.6, rate));
    let f = analyse(&hit, 1, rate).unwrap();
    assert!(f[ATTACK_CENTROID] > f[26] + 1.0, "attack centroid {} against {}", f[ATTACK_CENTROID], f[26]);
    assert!(f[ATTACK_BANDS.start] < f[BODY_BANDS.start] - 6.0, "{:?} {:?}", &f[ATTACK_BANDS], &f[BODY_BANDS]);
    assert!(f[ATTACK_FLATNESS] > 0.1, "attack flatness {}", f[ATTACK_FLATNESS]);
}

#[test]
fn a_falling_pitch_has_a_glide_and_a_steady_one_none() {
    let kick = analyse(&sweep(160.0, 50.0, 0.5, 44_100), 1, 44_100).unwrap();
    // 160 Hz to 50 Hz is about 20 semitones; the first frame already averages some of the fall.
    assert!(kick[GLIDE] > 8.0, "glide {}", kick[GLIDE]);
    let steady = analyse(&sine(55.0, 0.5, 44_100), 1, 44_100).unwrap();
    assert!(steady[GLIDE].abs() < 1.0, "glide {}", steady[GLIDE]);
    let hiss = analyse(&noise(0.5, 44_100), 1, 44_100).unwrap();
    assert_eq!(hiss[GLIDE], 0.0);
}

#[test]
fn a_tail_changes_the_crest_factor_little() {
    let rate = 44_100;
    let hit = sweep(150.0, 50.0, 0.3, rate);
    let mut tailed = hit.clone();
    // A long quiet tail, 30 dB down.
    tailed.extend(noise(1.5, rate).iter().map(|s| s * 0.03));
    let a = analyse(&hit, 1, rate).unwrap();
    let b = analyse(&tailed, 1, rate).unwrap();
    assert!((a[41] - b[41]).abs() < 1.0, "crest {} against {}", a[41], b[41]);
}
