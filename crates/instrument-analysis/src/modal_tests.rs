// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

#![allow(clippy::unwrap_used)]
use super::*;

// Independent analytic fixture with arbitrary initial phases and inharmonic frequencies.
fn struck(rate: u32, seconds: f64, modes: &[(f64, f64, f64)]) -> Vec<f32> {
    (0..(f64::from(rate) * seconds) as usize)
        .map(|n| {
            let t = n as f64 / f64::from(rate);
            modes
                .iter()
                .enumerate()
                .map(|(i, (hz, t60, amplitude))| {
                    amplitude * (-1000.0_f64.ln() * t / t60).exp() * (TAU * hz * t + 0.37 * i as f64).sin()
                })
                .sum::<f64>() as f32
        })
        .collect()
}

#[test]
fn inharmonic_modes_retain_measured_frequencies_decay_and_reference_mapping() {
    let truth = [(421.7, 1.8, 0.5), (1139.3, 1.1, 0.3), (2497.1, 0.7, 0.15)];
    for rate in [22050, 48000, 96000] {
        let pcm = struck(rate, 2.0, &truth);
        let report = fit(&pcm, rate, 1, 0..pcm.len(), 57, |_, _| true).unwrap();
        assert_eq!(report.outcome, Outcome::Fitted, "{rate}: {report:?}");
        let profile = report.profile.unwrap();
        assert_eq!(profile.reference_key(), 57);
        assert!((profile.reference_hz() - 220.0).abs() < 1e-10);
        for &(hz, t60, _) in &truth {
            let mode = report
                .modes
                .iter()
                .min_by(|a, b| (a.frequency_hz - hz).abs().total_cmp(&(b.frequency_hz - hz).abs()))
                .unwrap();
            assert!((mode.frequency_hz - hz).abs() < 0.4, "{rate}: {mode:?} vs {hz}");
            assert!((mode.decay_seconds - t60).abs() / t60 < 0.05, "{rate}: {mode:?}");
            assert!(mode.estimation_windows >= 3 && mode.validation_windows >= 3);
            assert!(mode.validation_rms_db < 0.5, "{mode:?}");
        }
        assert!(profile.modes().iter().map(|m| m.magnitude).sum::<f64>() <= 1.0 + 1e-12);
        assert_eq!(report, fit(&pcm, rate, 1, 0..pcm.len(), 57, |_, _| true).unwrap());
    }
}

#[test]
fn opposed_stereo_channels_do_not_cancel_and_channel_choice_is_reported() {
    let pcm = struck(22050, 2.0, &[(733.2, 1.4, 0.7)]);
    let stereo: Vec<_> = pcm.iter().flat_map(|x| [x * 0.25, -x]).collect();
    let report = fit(&stereo, 22050, 2, 0..pcm.len(), 60, |_, _| true).unwrap();
    assert_eq!(report.channel, 1);
    assert_eq!(report.outcome, Outcome::Fitted);
    assert!((report.modes[0].frequency_hz - 733.2).abs() < 0.4);
}

#[test]
fn constant_offset_and_gain_do_not_change_relative_modal_energy() {
    let original = struck(22050, 2.0, &[(521.3, 1.5, 0.5), (1319.7, 0.9, 0.2)]);
    let shifted: Vec<_> = original.iter().map(|x| x * 0.2 + 0.5).collect();
    let a = fit(&original, 22050, 1, 0..original.len(), 60, |_, _| true).unwrap().profile.unwrap();
    let b = fit(&shifted, 22050, 1, 0..shifted.len(), 60, |_, _| true).unwrap().profile.unwrap();
    assert_eq!(a.modes().len(), b.modes().len());
    for (left, right) in a.modes().iter().zip(b.modes()) {
        assert!((left.ratio - right.ratio).abs() < 0.002);
        assert!((left.decay_seconds - right.decay_seconds).abs() < 0.01);
        assert!((left.magnitude - right.magnitude).abs() < 0.002);
    }
}

#[test]
fn silence_short_constant_growing_and_noise_sources_do_not_get_a_model() {
    for pcm in [vec![0.0; 22050], vec![0.3; 22050]] {
        assert_eq!(fit(&pcm, 22050, 1, 0..pcm.len(), 60, |_, _| true).unwrap().outcome, Outcome::Silent);
    }
    let short = struck(22050, 0.05, &[(660.0, 1.0, 0.8)]);
    assert_eq!(fit(&short, 22050, 1, 0..short.len(), 60, |_, _| true).unwrap().outcome, Outcome::TooShort);
    for slope in [0.0, 1.0] {
        let pcm: Vec<_> = (0..44100)
            .map(|n| {
                let t = n as f64 / 22050.0;
                (0.05 * (slope * t).exp() * (TAU * 733.0 * t).sin()) as f32
            })
            .collect();
        let report = fit(&pcm, 22050, 1, 0..pcm.len(), 60, |_, _| true).unwrap();
        assert!(report.profile.is_none(), "{report:?}");
    }
    let mut seed = 1u32;
    let noise: Vec<_> = (0..44100)
        .map(|_| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (f64::from(seed) / f64::from(u32::MAX) * 2.0 - 1.0) as f32
        })
        .collect();
    assert!(fit(&noise, 22050, 1, 0..noise.len(), 60, |_, _| true).unwrap().profile.is_none());
}

#[test]
fn malformed_input_and_cancellation_are_explicit_and_work_is_bounded() {
    let pcm = struck(22050, 2.0, &[(557.0, 1.2, 0.6)]);
    assert_eq!(fit(&pcm, 0, 1, 0..pcm.len(), 60, |_, _| true), Err(Error::InvalidFormat));
    assert_eq!(fit(&pcm, 22050, 0, 0..pcm.len(), 60, |_, _| true), Err(Error::InvalidFormat));
    assert_eq!(fit(&pcm, 22050, 1, 1..usize::MAX, 60, |_, _| true), Err(Error::InvalidRange));
    assert_eq!(fit(&pcm, 22050, 1, 0..pcm.len(), 128, |_, _| true), Err(Error::InvalidSettings));
    let mut invalid = pcm.clone();
    invalid[123] = f32::NAN;
    assert_eq!(fit(&invalid, 22050, 1, 0..invalid.len(), 60, |_, _| true), Err(Error::NonFinite));
    assert_eq!(fit(&pcm, 22050, 1, 0..pcm.len(), 60, |_, _| false), Err(Error::Cancelled));
    let after_preparation = 2 * pcm.len().div_ceil(CHUNK) + 5;
    assert_eq!(fit(&pcm, 22050, 1, 0..pcm.len(), 60, |done, _| done < after_preparation), Err(Error::Cancelled));
    let mut long = pcm;
    long.resize(22050 * 9, f32::NAN);
    long[44100..22050 * 8].fill(0.0);
    let mut previous = 0;
    let report = fit(&long, 22050, 1, 0..long.len(), 60, |done, total| {
        assert!(done >= previous && done <= total);
        previous = done;
        true
    })
    .unwrap();
    assert_eq!(report.range, 0..22050 * 8);
    assert!(report.frame_size <= MAX_FRAME && report.modes.len() <= MAX_PROFILE_MODES);
}

#[test]
fn held_out_temporal_errors_are_not_used_to_fit_the_decay() {
    let hz = 440.0;
    let slope = -3.0;
    let data: Vec<_> = (0..12).map(|i| (i, i as f64 * 0.1, -0.3 + slope * i as f64 * 0.1)).collect();
    let (_, fitted, evidence) = estimate(hz, &data).unwrap();
    assert!((fitted - slope).abs() < 1e-12);
    assert!(evidence.validation_rms_db < 1e-12);
    let mut disturbed = data;
    for (i, _, log_amp) in &mut disturbed {
        if *i % 2 == 1 {
            *log_amp += 0.5;
        }
    }
    assert!(estimate(hz, &disturbed).is_none(), "held-out envelope mismatch must reject this mode");
}
