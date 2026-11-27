// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::math::C;
use super::*;

/// A self-made continuation: `count` harmonics of 200 Hz falling 3 dB per harmonic, each with a
/// slow diffuse part, a 5.5 Hz vibrato of 20 cents, and a side when `stereo`.
pub(crate) fn sample(count: usize, stereo: bool) -> Continuation {
    let diffuse = |power: f32| {
        let ar = [[0.9_f32, 0.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0]];
        let innovation = power * (1.0 - 0.81);
        let mut d = Diffuse { ar, innovation, power, cutoff_hz: 20.0, power_fix: 0.0, history: [[0.0; 2]; HISTORY] };
        let coefficients = d.coefficients();
        d.power_fix = Diffuse::fix_for(&coefficients, f64::from(innovation), f64::from(power), 20.0) as f32;
        d
    };
    let partials = (0..count)
        .map(|k| {
            let magnitude = 0.2 * 10f32.powf(-3.0 * k as f32 / 20.0);
            let mut partial = Partial::NONE;
            partial.carrier = [magnitude, 0.0];
            partial.mean_log = magnitude.ln();
            partial.sigma_db = 1.5;
            partial.spread = 0.8;
            partial.start_level = magnitude;
            partial.diffuse = diffuse(magnitude * magnitude * 0.01);
            if stereo {
                partial.side_rho = [0.2, 0.05];
                partial.side = diffuse(magnitude * magnitude * 0.002);
            }
            partial
        })
        .collect();
    let noise_db = std::array::from_fn(|i| -(i as f32) * 0.2);
    Continuation {
        recording_rate: 48000.0,
        f0: 200.0,
        vibrato_hz: 5.5,
        vibrato_cents: 20.0,
        vibrato_phase: 0.3,
        phase: 1.0,
        deviation_phase: 0.2,
        join: 1.5,
        release_start: if stereo { 2.5 } else { 0.0 },
        jitter_ar: [0.5, 0.0, 0.0, 0.0],
        jitter_innovation: 0.25,
        jitter_cents: 1.0,
        jitter_gain: 1.0,
        level_db: 0.0,
        coef_target: [0.0; 3],
        coef_model: [0.0; 3],
        noise_db,
        noise_rms: 0.001,
        side_noise_db: if stereo { noise_db } else { [0.0; NOISE_POINTS] },
        side_noise_rms: if stereo { 0.0005 } else { 0.0 },
        stereo,
        partials,
    }
}

#[test]
fn a_continuation_round_trips_through_its_bytes() {
    for stereo in [false, true] {
        let continuation = sample(40, stereo);
        continuation.validate().unwrap();
        let bytes = continuation.encode();
        assert_eq!(bytes.len(), 16 + 4 * (SCALAR_FIELDS + 2 * NOISE_POINTS + 40 * PARTIAL_FIELDS));
        assert_eq!(Continuation::decode(&bytes).unwrap(), continuation);
    }
}

#[test]
fn hostile_bytes_are_refused() {
    let bytes = sample(8, true).encode();
    // Truncated, extended, wrong magic, wrong version, zero or too many partials.
    assert!(Continuation::decode(&bytes[..bytes.len() - 1]).is_err());
    let mut longer = bytes.clone();
    longer.push(0);
    assert!(Continuation::decode(&longer).is_err());
    let mut magic = bytes.clone();
    magic[0] = b'X';
    assert!(Continuation::decode(&magic).is_err());
    let mut version = bytes.clone();
    version[8] = 9;
    assert!(Continuation::decode(&version).is_err());
    for count in [0_u16, 161] {
        let mut wrong = bytes.clone();
        wrong[10..12].copy_from_slice(&count.to_le_bytes());
        assert!(Continuation::decode(&wrong).is_err());
    }
    // A NaN, the reserved value set, a stereo flag of 2.
    let mut nan = bytes.clone();
    nan[16..20].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(Continuation::decode(&nan).is_err());
    let mut reserved = bytes.clone();
    let at = 16 + 4 * (SCALAR_FIELDS - 1);
    reserved[at..at + 4].copy_from_slice(&1.0_f32.to_le_bytes());
    assert!(Continuation::decode(&reserved).is_err());
    let mut flag = bytes.clone();
    flag[12] = 2;
    assert!(Continuation::decode(&flag).is_err());
    assert!(Continuation::decode(&[]).is_err());
}

#[test]
fn bounds_are_checked() {
    let mut mono_with_side = sample(4, false);
    mono_with_side.partials[1].side_rho = [0.1, 0.0];
    assert!(mono_with_side.validate().is_err());
    let mut fast_vibrato = sample(4, false);
    fast_vibrato.vibrato_hz = 30.0;
    assert!(fast_vibrato.validate().is_err());
    let mut wide_gamma = sample(4, false);
    wide_gamma.partials[0].gamma = 2.0;
    assert!(wide_gamma.validate().is_err());
    let mut early_release = sample(4, true);
    early_release.release_start = 1.0;
    assert!(early_release.validate().is_err());
    let empty = Continuation { partials: Vec::new(), ..sample(1, false) };
    assert!(empty.validate().is_err());
}

#[test]
fn the_recordings_own_note_continues_from_its_history() {
    let source = sample(30, true);
    let mut note = NoteModel::default();
    note.transposed(&source, 1.0, 48000.0);
    assert!(note.from_history);
    assert_eq!(note.count, 30);
    assert_eq!(note.partials[7], source.partials[7]);
    assert!((note.level_db - f64::from(source.level_db)).abs() < 1e-9);
}

#[test]
fn a_transposed_note_keeps_the_spectral_envelope_in_hertz() {
    let source = sample(60, false);
    let mut note = NoteModel::default();
    // A fifth up: harmonic k of the new note sits at 1.5 k of the source.
    note.transposed(&source, 1.5, 48000.0);
    assert!(!note.from_history);
    assert_eq!(note.count, 40, "no harmonic is taken from beyond the source");
    for k in [0_usize, 1, 5, 20] {
        let at = (k + 1) as f64 * 1.5;
        // The source falls 3 dB per harmonic: at harmonic `at` (1-based) it is -3 (at - 1) dB.
        let expected = 0.2 * 10f64.powf(-3.0 * (at - 1.0) / 20.0);
        let got = C::from_pair(note.partials[k].carrier).abs();
        assert!((20.0 * (got / expected).log10()).abs() < 0.01, "harmonic {k}: {got} against {expected}");
        assert_eq!(note.partials[k].diffuse.history, [[0.0; 2]; HISTORY]);
    }
    // An octave down at 48 kHz is limited by the source; far up by the sample rate.
    note.transposed(&source, 0.5, 48000.0);
    assert_eq!(note.count, 120.min(MAX_PARTIALS));
    note.transposed(&source, 8.0, 48000.0);
    assert_eq!(note.count, 7);
}

#[test]
fn a_morph_at_its_ends_is_each_model() {
    let a_source = sample(30, true);
    let mut b_source = sample(30, false);
    for partial in &mut b_source.partials {
        partial.carrier = C::from_pair(partial.carrier).scale(0.5).to_pair();
    }
    let mut a = NoteModel::default();
    a.transposed(&a_source, 1.0, 48000.0);
    let mut b = NoteModel::default();
    b.transposed(&b_source, 1.0, 48000.0);
    let untouched = a.clone();
    a.morph(&b, 0.0, 0.001, 0.001);
    assert_eq!(a.partials[3], untouched.partials[3]);
    let mut whole = untouched.clone();
    whole.morph(&b, 1.0, 0.001, 0.001);
    for k in [0_usize, 4, 12] {
        let got = C::from_pair(whole.partials[k].carrier).abs();
        let want = C::from_pair(b.partials[k].carrier).abs();
        assert!((got / want - 1.0).abs() < 1e-4, "harmonic {k}");
        // The mono target has no side: none of the mixed side's independent power remains.
        assert!(whole.partials[k].side.power < 1e-12);
    }
    // Halfway, magnitudes meet in the middle in dB.
    let mut half = untouched;
    half.morph(&b, 0.5, 0.001, 0.001);
    let got = C::from_pair(half.partials[2].carrier).abs();
    let want = (C::from_pair(a_source.partials[2].carrier).abs() * C::from_pair(b.partials[2].carrier).abs()).sqrt();
    assert!((got / want - 1.0).abs() < 1e-4);
}
