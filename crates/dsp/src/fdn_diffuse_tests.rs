// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn validates_settings_and_bounds_all_history() {
    for bad in [f64::NAN, f64::INFINITY, -1., 101.] {
        assert!(DiffusedFdn::new(DiffuseSettings { predelay_ms: bad, ..DiffuseSettings::default() }).is_err());
    }
    for bad in [f32::NAN, f32::INFINITY, -0.1, 3.] {
        for settings in [
            DiffuseSettings { early_diffusion: bad, ..DiffuseSettings::default() },
            DiffuseSettings { diffusion: bad, ..DiffuseSettings::default() },
            DiffuseSettings { early_level: bad, ..DiffuseSettings::default() },
            DiffuseSettings { width: bad, ..DiffuseSettings::default() },
        ] {
            assert!(DiffusedFdn::new(settings).is_err());
        }
    }
    let largest = DiffusedFdn::new(DiffuseSettings {
        core: Settings { sample_rate: 96_000, room_scale: 2., ..Settings::default() },
        predelay_ms: 100.,
        ..DiffuseSettings::default()
    })
    .unwrap();
    assert!(largest.history_bytes() < 1024 * 1024);
}

#[test]
fn processing_is_block_independent_and_reset_clears_every_path() {
    let settings = DiffuseSettings { early_diffusion: 0.6, ..DiffuseSettings::default() };
    let mut whole = DiffusedFdn::new(settings).unwrap();
    let mut split = DiffusedFdn::new(settings).unwrap();
    let mut samples = vec![0.; 48_000];
    samples[..2].fill(0.25);
    let mut blocks = samples.clone();
    whole.process(&mut samples);
    for block in blocks.chunks_mut(74) {
        split.process(block);
    }
    assert_eq!(samples, blocks);
    whole.reset();
    assert!((0..48_000).all(|_| whole.process_frame([0.; 2]) == [0.; 2]));
    let mut odd = [0., 0., 42.];
    whole.process(&mut odd);
    assert_eq!(odd[2], 42.);
}

#[test]
fn early_path_starts_at_predelay_and_diffusion_reduces_impulse_concentration() {
    let mut core = Fdn::new(Settings::default()).unwrap();
    let mut developed = DiffusedFdn::new(DiffuseSettings::default()).unwrap();
    let mut old_energy = Vec::new();
    let mut new_energy = Vec::new();
    for i in 0..9600 {
        let x = if i == 0 { [0.25; 2] } else { [0.; 2] };
        old_energy.push(core.process_frame(x).iter().map(|v| v * v).sum::<f32>());
        new_energy.push(developed.process_frame(x).iter().map(|v| v * v).sum::<f32>());
    }
    assert_eq!(new_energy.iter().position(|v| *v > 1e-12), Some(120));
    let concentration = |v: &[f32]| v.iter().copied().fold(0_f32, f32::max) / v.iter().sum::<f32>();
    assert!(concentration(&new_energy) < concentration(&old_energy) * 0.6);
}

#[test]
fn invalid_samples_do_not_poison_diffusers_or_filters() {
    let mut core = DiffusedFdn::new(DiffuseSettings::default()).unwrap();
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
        core.process_frame([value; 2]);
    }
    assert!((0..48_000).all(|_| core.process_frame([0.; 2]).iter().all(|v| v.is_finite() && v.abs() < 100.)));
}

#[test]
fn zero_width_folds_all_paths_to_mono() {
    let mut core = DiffusedFdn::new(DiffuseSettings { width: 0., ..DiffuseSettings::default() }).unwrap();
    for i in 0..24_000 {
        let y = core.process_frame(if i == 0 { [0.25; 2] } else { [0.; 2] });
        assert_eq!(y[0], y[1]);
    }
}

#[test]
fn early_tilt_redistributes_reflections_without_changing_the_late_path() {
    for bad in [f64::NAN, f64::INFINITY, -12.1, 12.1] {
        assert!(DiffusedFdn::new(DiffuseSettings { early_tilt_db: bad, ..DiffuseSettings::default() }).is_err());
    }
    let mut flat = DiffusedFdn::new(DiffuseSettings { early_level: 1., ..DiffuseSettings::default() }).unwrap();
    let mut tilted =
        DiffusedFdn::new(DiffuseSettings { early_level: 1., early_tilt_db: 6., ..DiffuseSettings::default() }).unwrap();
    let mut tail = DiffusedFdn::new(DiffuseSettings { early_level: 0., ..DiffuseSettings::default() }).unwrap();
    let mut tilted_tail =
        DiffusedFdn::new(DiffuseSettings { early_level: 0., early_tilt_db: 6., ..DiffuseSettings::default() }).unwrap();
    let mut energy = [[0_f64; 2]; 2];
    for frame in 0..48_000 {
        let input = if frame == 0 { [0.25; 2] } else { [0.; 2] };
        let plain = flat.process_frame(input);
        let shaped = tilted.process_frame(input);
        let late = tail.process_frame(input);
        assert_eq!(late, tilted_tail.process_frame(input));
        if frame == 120 {
            assert_eq!(plain, shaped);
        }
        let window = if frame < 720 {
            Some(0)
        } else if (960..2640).contains(&frame) {
            Some(1)
        } else {
            None
        };
        if let Some(window) = window {
            for channel in 0..2 {
                energy[0][window] += f64::from(plain[channel] - late[channel]).powi(2);
                energy[1][window] += f64::from(shaped[channel] - late[channel]).powi(2);
            }
        }
    }
    assert!(energy[1][1] / energy[1][0] > 1.5 * energy[0][1] / energy[0][0]);
}

#[test]
fn early_spreading_reduces_peaks_without_changing_the_late_path() {
    let settings = DiffuseSettings { early_level: 1., ..DiffuseSettings::default() };
    let mut plain = DiffusedFdn::new(settings).unwrap();
    let mut spread = DiffusedFdn::new(DiffuseSettings { early_diffusion: 0.6, ..settings }).unwrap();
    let mut late = DiffusedFdn::new(DiffuseSettings { early_level: 0., ..settings }).unwrap();
    let mut spread_late =
        DiffusedFdn::new(DiffuseSettings { early_level: 0., early_diffusion: 0.6, ..settings }).unwrap();
    let mut energy = [0_f64; 2];
    let mut peak = [0_f64; 2];
    for frame in 0..48_000 {
        let x = if frame == 0 { [0.25; 2] } else { [0.; 2] };
        let a = plain.process_frame(x);
        let b = spread.process_frame(x);
        let tail = late.process_frame(x);
        assert_eq!(tail, spread_late.process_frame(x));
        for channel in 0..2 {
            for (i, sample) in [a[channel] - tail[channel], b[channel] - tail[channel]].into_iter().enumerate() {
                let power = f64::from(sample).powi(2);
                energy[i] += power;
                peak[i] = peak[i].max(power);
            }
        }
    }
    assert!(peak[1] < peak[0] * 0.6);
    // Output filters and overlapping reflections can change path energy after phase spreading.
    assert!((energy[1] / energy[0] - 1.).abs() < 0.1);
}

#[test]
fn late_scale_preserves_early_arrivals_and_validates_effective_size() {
    for bad in [f64::NAN, f64::INFINITY, 0.49, 2.01] {
        assert!(DiffusedFdn::new(DiffuseSettings { late_scale: bad, ..DiffuseSettings::default() }).is_err());
    }
    assert!(
        DiffusedFdn::new(DiffuseSettings {
            late_scale: 0.5,
            core: Settings { room_scale: 0.5, ..Settings::default() },
            ..DiffuseSettings::default()
        })
        .is_err()
    );
    let mut original = DiffusedFdn::new(DiffuseSettings::default()).unwrap();
    let mut compact = DiffusedFdn::new(DiffuseSettings { late_scale: 0.5, ..DiffuseSettings::default() }).unwrap();
    // The shorter network cannot return anything before 120 + round(1429 / 2) frames.
    for frame in 0..834 {
        let x = if frame == 0 { [0.25; 2] } else { [0.; 2] };
        assert_eq!(original.process_frame(x), compact.process_frame(x));
    }
    assert!(compact.history_bytes() < original.history_bytes());
}

#[test]
fn onset_gain_changes_first_reflection_without_moving_it() {
    for bad in [f32::NAN, f32::INFINITY, -0.1, 8.01] {
        assert!(DiffusedFdn::new(DiffuseSettings { early_onset_gain: bad, ..DiffuseSettings::default() }).is_err());
    }
    let mut a = DiffusedFdn::new(DiffuseSettings::default()).unwrap();
    let mut b = DiffusedFdn::new(DiffuseSettings { early_onset_gain: 2., ..DiffuseSettings::default() }).unwrap();
    for i in 0..121 {
        let x = if i == 0 { [0.25; 2] } else { [0.; 2] };
        let plain = a.process_frame(x);
        let doubled = b.process_frame(x);
        assert_eq!(doubled, plain.map(|v| v * 2.));
        if i == 120 {
            assert!(plain[0] != 0.);
        }
    }
}

#[test]
fn early_channel_weights_are_bounded_and_isolate_the_early_path() {
    for value in [f32::NAN, f32::INFINITY, -4.01, 4.01] {
        assert!(
            DiffusedFdn::new(DiffuseSettings { early_output_weights: [value, 1.], ..DiffuseSettings::default() })
                .is_err()
        );
    }
    let mut fdn = DiffusedFdn::new(DiffuseSettings {
        core: Settings { output_weights: Some([[0.; 8]; 2]), ..Settings::default() },
        early_level: 1.,
        early_output_weights: [1., 0.],
        width: 1.,
        ..DiffuseSettings::default()
    })
    .unwrap();
    let mut energy = 0.;
    for frame in 0..6000 {
        let output = fdn.process_frame(if frame == 0 { [1.; 2] } else { [0.; 2] });
        assert_eq!(output[1], 0.);
        energy += output[0] * output[0];
    }
    assert!(energy > 0.01);
}

#[test]
fn custom_early_patterns_reject_unpaired_or_unbounded_data() {
    let base = DiffuseSettings::default();
    assert!(DiffusedFdn::new(DiffuseSettings { early_tap_times_ms: Some([[0.; 8]; 2]), ..base }).is_err());
    assert!(DiffusedFdn::new(DiffuseSettings { early_tap_weights: Some([[0.; 8]; 2]), ..base }).is_err());
    for bad in [f64::NAN, f64::INFINITY, -0.01, 100.01] {
        assert!(
            DiffusedFdn::new(DiffuseSettings {
                early_tap_times_ms: Some([[bad; 8]; 2]),
                early_tap_weights: Some([[1.; 8]; 2]),
                ..base
            })
            .is_err()
        );
    }
    for bad in [f32::NAN, f32::INFINITY, -4.01, 4.01] {
        assert!(
            DiffusedFdn::new(DiffuseSettings {
                early_tap_times_ms: Some([[0.; 8]; 2]),
                early_tap_weights: Some([[bad; 8]; 2]),
                ..base
            })
            .is_err()
        );
    }
}

#[test]
fn custom_early_arrivals_match_independent_filtered_impulses_and_reset() {
    use crate::{FirstOrder, FirstOrderCoefficients};
    let mut times = [[0.; 8]; 2];
    let mut gains = [[0.; 8]; 2];
    times[0][0] = 5.;
    times[1][0] = 10.;
    gains[0][0] = 0.5;
    gains[1][0] = -0.25;
    let settings = DiffuseSettings {
        core: Settings { output_weights: Some([[0.; 8]; 2]), ..Settings::default() },
        early_level: 1.,
        width: 1.,
        early_tap_times_ms: Some(times),
        early_tap_weights: Some(gains),
        ..DiffuseSettings::default()
    };
    let mut fdn = DiffusedFdn::new(settings).unwrap();
    let mut first = Vec::new();
    for pass in 0..2 {
        let mut hp = [FirstOrder::new(FirstOrderCoefficients::highpass(48_000., 120.)); 2];
        let mut lp = [FirstOrder::new(FirstOrderCoefficients::lowpass(48_000., 14_000.)); 2];
        for frame in 0..1800 {
            let actual = fdn.process_frame(if frame == 0 { [1.; 2] } else { [0.; 2] });
            let raw = [if frame == 360 { 0.5 } else { 0. }, if frame == 600 { -0.25 } else { 0. }];
            for channel in 0..2 {
                let expected = lp[channel].process(hp[channel].process(raw[channel])) as f32;
                assert!((actual[channel] - expected).abs() < 1e-7, "frame {frame}, channel {channel}");
            }
            if pass == 0 {
                first.push(actual);
            } else {
                assert_eq!(actual, first[frame]);
            }
        }
        fdn.reset();
    }
}
