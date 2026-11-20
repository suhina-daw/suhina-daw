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
fn silence_short_growing_and_noise_sources_do_not_get_a_model_but_a_held_tone_is_sustained() {
    for pcm in [vec![0.0; 22050], vec![0.3; 22050]] {
        assert_eq!(fit(&pcm, 22050, 1, 0..pcm.len(), 60, |_, _| true).unwrap().outcome, Outcome::Silent);
    }
    let short = struck(22050, 0.05, &[(660.0, 1.0, 0.8)]);
    assert_eq!(fit(&short, 22050, 1, 0..short.len(), 60, |_, _| true).unwrap().outcome, Outcome::TooShort);
    let held = |slope: f64| -> Vec<f32> {
        (0..44100)
            .map(|n| {
                let t = n as f64 / 22050.0;
                (0.05 * (slope * t).exp() * (TAU * 733.0 * t).sin()) as f32
            })
            .collect()
    };
    // A rising envelope is no resonance.
    let rising = held(1.0);
    assert!(fit(&rising, 22050, 1, 0..rising.len(), 60, |_, _| true).unwrap().profile.is_none());
    // A level that stays put (a bowed or blown partial) is kept as sustained, longest T60.
    let steady = held(0.0);
    let report = fit(&steady, 22050, 1, 0..steady.len(), 60, |_, _| true).unwrap();
    assert_eq!(report.outcome, Outcome::Fitted, "{report:?}");
    let mode = report.modes.iter().find(|m| (m.frequency_hz - 733.0).abs() < 0.5).unwrap();
    assert!(mode.sustained && mode.decay_seconds == SUSTAINED_SECONDS, "{mode:?}");
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
    let fitted = estimate(hz, &data).unwrap();
    assert!((fitted.slope - slope).abs() < 1e-12);
    assert!(fitted.evidence.validation_rms_db < 1e-12);
    assert!(fitted.fast.is_none(), "one exponential needs no fast part");
    let mut disturbed = data;
    for (i, _, log_amp) in &mut disturbed {
        if *i % 2 == 1 {
            *log_amp += 0.5;
        }
    }
    assert!(estimate(hz, &disturbed).is_none(), "held-out envelope mismatch must reject this mode");
}

struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        f64::from(self.0) / f64::from(u32::MAX)
    }
}

/// A recording-like strike at 22050 Hz: eight slightly stretched partials, each two components
/// `beat_hz` apart (beating) with a fast early decay followed by a slower one, a 20 ms noise
/// burst on the attack, 5 ms of lead-in and a -60 dB noise floor. None of it is one exponential.
fn recorded_like(seconds: f64, root: f64, beat_hz: f64, seed: u32) -> (Vec<f32>, Vec<f64>) {
    let mut random = Lcg(seed);
    let n = (22050.0 * seconds) as usize;
    let lead = 110;
    let mut out = vec![0.0_f64; n];
    let mut partials = Vec::new();
    for k in 1..=8 {
        let k = f64::from(k);
        let hz = root * k * (1.0 + 0.0004 * k * k);
        partials.push(hz);
        for (detune, fast, slow, share) in [(0.0, 0.35, 4.0 / k.sqrt(), 0.6), (beat_hz, 0.5, 3.0 / k.sqrt(), 0.4)] {
            let phase = random.next() * TAU;
            for (i, sample) in out.iter_mut().enumerate().skip(lead) {
                let t = (i - lead) as f64 / 22050.0;
                let envelope = 0.55 * (-1000.0_f64.ln() * t / fast).exp() + 0.45 * (-1000.0_f64.ln() * t / slow).exp();
                *sample += 0.6 / k * share * envelope * (TAU * (hz + detune) * t + phase).sin();
            }
        }
    }
    let burst = 441;
    for (i, sample) in out.iter_mut().enumerate().skip(lead).take(burst) {
        *sample += 0.3 * (random.next() * 2.0 - 1.0) * (1.0 - (i - lead) as f64 / burst as f64);
    }
    (out.into_iter().map(|x| (x + 0.001 * (random.next() * 2.0 - 1.0)) as f32).collect(), partials)
}

// Regression: algorithm version 1 found no model in such a strike. It searched peaks only in the
// first envelope window (1024 samples for a short trim), where neighbouring partials of a low note
// hid each other, and demanded one exponential within 1 dB. Measured on this fixture in a script
// mirroring the estimator, version 1 kept 0..3 of 8 partials; version 2 keeps 6..8.
#[test]
fn recorded_strikes_with_beating_two_stage_decay_and_an_attack_are_fitted() {
    for (seconds, root, beat, at_least) in
        [(0.9, 110.0, 0.8, 4), (2.0, 110.0, 0.8, 4), (0.9, 110.0, 0.0, 5), (0.9, 55.0, 0.0, 5)]
    {
        let (pcm, partials) = recorded_like(seconds, root, beat, 7);
        let report = fit(&pcm, 22050, 1, 0..pcm.len(), 45, |_, _| true).unwrap();
        assert_eq!(report.outcome, Outcome::Fitted, "{seconds} s at {root} Hz: {report:?}");
        let matched: Vec<_> =
            report.modes.iter().filter(|mode| partials.iter().any(|hz| (mode.frequency_hz - hz).abs() < 1.5)).collect();
        assert!(matched.len() >= at_least, "{seconds} s at {root} Hz kept {matched:?} of {partials:?}");
        for mode in &report.modes {
            assert!(mode.estimation_rms_db <= MAX_ESTIMATION_DB && mode.validation_rms_db <= MAX_VALIDATION_DB);
        }
        for mode in matched {
            assert!((0.05..=10.0).contains(&mode.decay_seconds), "{mode:?}");
        }
        assert_eq!(report, fit(&pcm, 22050, 1, 0..pcm.len(), 45, |_, _| true).unwrap());
    }
}

#[test]
fn a_second_strike_inside_the_trim_ends_each_mode_span_instead_of_rejecting_the_fit() {
    let (mut pcm, _) = recorded_like(2.0, 110.0, 0.8, 7);
    let (second, _) = recorded_like(1.4, 110.0, 0.8, 9);
    for (sample, extra) in pcm.iter_mut().skip(13230).zip(second) {
        *sample += extra;
    }
    let report = fit(&pcm, 22050, 1, 0..pcm.len(), 45, |_, _| true).unwrap();
    assert_eq!(report.outcome, Outcome::Fitted, "{report:?}");
}

#[test]
fn a_mode_span_skips_an_attack_and_stops_at_the_floor_or_a_rise() {
    let log = |db: f64| db / DB;
    // Rising first window (an attack), a decay, then a 7 dB rise (the next strike).
    let values: Vec<_> = [-20.0, -6.0, -8.0, -10.0, -12.0, -14.0, -16.0, -9.0, -11.0]
        .iter()
        .enumerate()
        .map(|(i, db)| (i, i as f64 * 0.05, log(*db)))
        .collect();
    let span = envelope_span(&values);
    assert_eq!(span.first().map(|v| v.0), Some(1));
    assert_eq!(span.last().map(|v| v.0), Some(6));
    // A level more than 60 dB below the peak ends the span.
    let deep: Vec<_> = [0.0, -20.0, -40.0, -59.0, -61.0, -62.0]
        .iter()
        .enumerate()
        .map(|(i, db)| (i, i as f64 * 0.05, log(*db)))
        .collect();
    assert_eq!(envelope_span(&deep).len(), 4);
}

fn angle_between(a: f64, b: f64) -> f64 {
    (a - b + std::f64::consts::PI).rem_euclid(TAU) - std::f64::consts::PI
}

#[test]
fn a_recording_keeps_phases_level_and_up_to_32_modes() {
    let truth = [(421.7, 1.8, 0.5), (1139.3, 1.1, 0.3), (2497.1, 0.7, 0.15)];
    for rate in [22050, 48000] {
        let pcm = struck(rate, 2.0, &truth);
        let report = fit(&pcm, rate, 1, 0..pcm.len(), 57, |_, _| true).unwrap();
        let recording = report.recording.as_ref().unwrap();
        assert_eq!(recording.key, 57);
        assert!(recording.validate().is_ok());
        assert!(recording.modes.len() <= 32 && recording.modes.len() == report.recording_modes.len());
        assert!((recording.level - 0.95).abs() < 0.15, "{rate}: level {}", recording.level);
        for (i, &(hz, _, _)) in truth.iter().enumerate() {
            let mode = report
                .recording_modes
                .iter()
                .min_by(|a, b| (a.frequency_hz - hz).abs().total_cmp(&(b.frequency_hz - hz).abs()))
                .unwrap();
            assert!((mode.frequency_hz - hz).abs() < 0.4);
            assert!(angle_between(mode.phase, 0.37 * i as f64).abs() < 0.25, "{rate} {hz}: {mode:?}");
        }
    }
}

#[test]
fn a_fast_first_decay_becomes_a_two_part_mode() {
    let ln1000 = 1000.0_f64.ln();
    let pcm: Vec<f32> = (0..22050 * 3)
        .map(|n| {
            let t = n as f64 / 22050.0;
            let envelope = 0.6 * (-ln1000 * t / 0.25).exp() + 0.4 * (-ln1000 * t / 2.5).exp();
            (0.5 * envelope * (TAU * 523.0 * t).sin()) as f32
        })
        .collect();
    let report = fit(&pcm, 22050, 1, 0..pcm.len(), 60, |_, _| true).unwrap();
    let mode = report.recording_modes.iter().find(|m| (m.frequency_hz - 523.0).abs() < 1.0).unwrap();
    assert!((mode.decay_seconds - 2.5).abs() / 2.5 < 0.15, "{mode:?}");
    assert!((mode.fast_decay_seconds - 0.25).abs() / 0.25 < 0.3, "{mode:?}");
    assert!((mode.fast_share - 0.6).abs() < 0.15, "{mode:?}");
    let stored = report.recording.unwrap();
    assert!(stored.modes.iter().any(|m| m.fast_share > 0.0 && m.fast_decay_seconds > 0.0));
}

#[test]
fn a_beating_pair_becomes_a_mode_with_a_partner() {
    let ln1000 = 1000.0_f64.ln();
    let pcm: Vec<f32> = (0..22050 * 4)
        .map(|n| {
            let t = n as f64 / 22050.0;
            let envelope = (-ln1000 * t / 2.0).exp();
            (0.5 * envelope * ((TAU * 440.0 * t).sin() + 0.2 * (TAU * 443.0 * t).sin())) as f32
        })
        .collect();
    let report = fit(&pcm, 22050, 1, 0..pcm.len(), 69, |_, _| true).unwrap();
    let mode = report.recording_modes.iter().find(|m| (m.frequency_hz - 440.5).abs() < 3.0).unwrap();
    assert!((2.7..=3.3).contains(&mode.beat_hz), "{mode:?}");
    assert!((0.1..=0.25).contains(&mode.beat_share), "{mode:?}");
    // A single decaying sine has no partner.
    let plain = struck(22050, 2.0, &[(733.2, 1.4, 0.7)]);
    let report = fit(&plain, 22050, 1, 0..plain.len(), 60, |_, _| true).unwrap();
    assert!(report.recording_modes.iter().all(|m| m.beat_share == 0.0), "{:?}", report.recording_modes);
}

/// Two strings of one note a little apart beat deeply: the partial misses the error limits but
/// stays in the recording, loose, instead of leaving only the smooth partials.
#[test]
fn a_deeply_beating_partial_is_kept_loose_in_the_recording_but_not_the_profile() {
    let ln1000 = 1000.0_f64.ln();
    let pcm: Vec<f32> = (0..22050 * 3)
        .map(|n| {
            let t = n as f64 / 22050.0;
            let smooth = 0.5 * (-ln1000 * t / 2.5).exp() * (TAU * 220.0 * t).sin();
            let strings = (TAU * 660.0 * t).sin() + 0.9 * (TAU * 661.5 * t).sin();
            (smooth + 0.3 * (-ln1000 * t / 2.0).exp() * strings) as f32
        })
        .collect();
    let report = fit(&pcm, 22050, 1, 0..pcm.len(), 57, |_, _| true).unwrap();
    assert_eq!(report.outcome, Outcome::Fitted, "{report:?}");
    assert!(report.modes.iter().all(|m| !m.loose), "the profile holds validated modes only");
    let beating = report.recording_modes.iter().find(|m| (m.frequency_hz - 660.75).abs() < 3.0);
    let beating = beating.unwrap_or_else(|| panic!("the beating partial is kept: {report:?}"));
    assert!(beating.loose && (0.5..=10.0).contains(&beating.decay_seconds), "{beating:?}");
    assert!(report.recording.as_ref().is_some_and(|r| r.validate().is_ok()));
}

#[test]
fn a_noisy_strike_keeps_an_attack_that_a_clean_one_barely_needs() {
    let clean = struck(22050, 2.0, &[(421.7, 1.8, 0.5), (1139.3, 1.1, 0.3)]);
    let mut noisy = clean.clone();
    let mut seed = 7u32;
    for (n, sample) in noisy.iter_mut().take(441).enumerate() {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        *sample += 0.4 * (f64::from(seed) / f64::from(u32::MAX) * 2.0 - 1.0) as f32 * (1.0 - n as f32 / 441.0);
    }
    let energy = |pcm: &[f32]| -> f64 {
        let recording = fit(pcm, 22050, 1, 0..pcm.len(), 60, |_, _| true).unwrap().recording.unwrap();
        assert!(recording.attack.len() <= 22050 / 4);
        recording.attack.iter().map(|x| f64::from(*x).powi(2)).sum()
    };
    let (clean_energy, noisy_energy) = (energy(&clean), energy(&noisy));
    assert!(noisy_energy > 0.0, "the strike noise is kept as the attack");
    assert!(noisy_energy > 10.0 * clean_energy, "{noisy_energy} vs {clean_energy}");
}
