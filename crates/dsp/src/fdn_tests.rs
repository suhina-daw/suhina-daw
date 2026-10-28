// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn mixing_preserves_energy_and_output_vectors_are_orthogonal() {
    for i in 0..8 {
        let mut v = [0.; 8];
        v[i] = 1.;
        mix(&mut v);
        assert!((v.iter().map(|x| x * x).sum::<f32>() - 1.).abs() < 1e-6);
    }
    assert_eq!(LEFT.iter().zip(RIGHT).map(|(a, b)| a * b).sum::<f32>(), 0.);
}

#[test]
fn rejects_hostile_settings_and_bounds_history() {
    for bad in [f64::NAN, f64::INFINITY, -1., 0., 1000.] {
        assert!(Fdn::new(Settings { decay_seconds: bad, ..Settings::default() }).is_err());
        assert!(Fdn::new(Settings { room_scale: bad, ..Settings::default() }).is_err());
        assert!(Fdn::new(Settings { high_decay_ratio: bad, ..Settings::default() }).is_err());
    }
    for rate in [0, 7999, 384_001, u32::MAX] {
        assert!(Fdn::new(Settings { sample_rate: rate, ..Settings::default() }).is_err());
    }
    let largest = Fdn::new(Settings { sample_rate: 384_000, room_scale: 2., ..Settings::default() }).unwrap();
    assert!(largest.history_bytes() <= 4 * 1024 * 1024);
}

#[test]
fn rendering_is_block_independent_and_reset_removes_tail() {
    let settings = Settings {
        input_rotation: 0.7,
        output_rotation: -0.3,
        loop_diffusion: 0.6,
        high_decay_ratio: 0.4,
        ..Settings::default()
    };
    let mut a = Fdn::new(settings).unwrap();
    let mut b = Fdn::new(settings).unwrap();
    let mut signal = vec![0.; 48_000 * 2];
    signal[0] = 0.25;
    signal[1] = 0.25;
    let mut split = signal.clone();
    a.process(&mut signal);
    for block in split.chunks_mut(128) {
        b.process(block);
    }
    assert_eq!(signal, split);
    a.reset();
    assert!((0..10_000).all(|_| a.process_frame([0., 0.]) == [0., 0.]));
    let mut odd = [0., 0., 42.];
    a.process(&mut odd);
    assert_eq!(odd[2], 42.);
}

#[test]
fn hostile_input_does_not_poison_network() {
    let mut fdn = Fdn::new(Settings::default()).unwrap();
    for x in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX, -f32::MAX] {
        fdn.process_frame([x, x]);
    }
    assert!((0..48_000).all(|_| fdn.process_frame([0., 0.]).iter().all(|x| x.is_finite() && x.abs() < 100.)));
}

#[test]
fn decay_tracks_target_at_multiple_sample_rates() {
    for rate in [8000, 44_100, 48_000, 96_000] {
        for (decay, scattering) in [(0.5, 0.), (2., 0.), (0.5, 0.6), (2., 0.6)] {
            let mut fdn = Fdn::new(Settings {
                sample_rate: rate,
                feedback_rotation: 0.7,
                input_rotation: 0.3,
                output_rotation: -0.4,
                loop_diffusion: scattering,
                decay_seconds: decay,
                damping_hz: 2000.,
                ..Settings::default()
            })
            .unwrap();
            let mut energy = vec![0.; (f64::from(rate) * decay * 2.) as usize];
            for (i, e) in energy.iter_mut().enumerate() {
                let input = if i == 0 { [0.25; 2] } else { [0.; 2] };
                let out = fdn.process_frame(input);
                *e = out.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
            }
            let mut sum = 0.;
            for e in energy.iter_mut().rev() {
                sum += *e;
                *e = sum;
            }
            let total = energy[0];
            let first = energy.iter().position(|e| *e / total < 10_f64.powf(-0.5)).unwrap();
            let last = energy.iter().position(|e| *e / total < 10_f64.powf(-3.5)).unwrap();
            let measured = 2. * (last - first) as f64 / f64::from(rate);
            assert!((measured / decay - 1.).abs() < 0.12, "rate={rate} decay={decay} measured={measured}");
        }
    }
}

#[test]
fn scattering_rejects_hostile_settings_and_remains_finite() {
    for bad in [f32::NAN, f32::INFINITY, -0.1, 0.71] {
        assert!(Fdn::new(Settings { loop_diffusion: bad, ..Settings::default() }).is_err());
    }
    let mut core = Fdn::new(Settings { loop_diffusion: 0.7, ..Settings::default() }).unwrap();
    for x in [f32::NAN, f32::MAX, f32::NEG_INFINITY] {
        core.process_frame([x; 2]);
    }
    assert!((0..48_000).all(|_| core.process_frame([0.; 2]).iter().all(|v| v.is_finite() && v.abs() < 100.)));
}

#[test]
fn rotations_preserve_vector_norms_and_output_orthogonality() {
    for angle in [-std::f64::consts::PI, -1., 0., 0.7, std::f64::consts::PI] {
        let core = Fdn::new(Settings { input_rotation: angle, output_rotation: angle, ..Settings::default() }).unwrap();
        for weights in [&core.input_weights, &core.output_weights[0], &core.output_weights[1]] {
            assert!((weights.iter().map(|v| v * v).sum::<f32>() - 8.).abs() < 2e-6);
        }
        let dot: f32 = core.output_weights[0].iter().zip(core.output_weights[1]).map(|(a, b)| a * b).sum();
        assert!(dot.abs() < 2e-6);
    }
    for bad in [f64::NAN, f64::INFINITY, -4., 4.] {
        assert!(Fdn::new(Settings { input_rotation: bad, ..Settings::default() }).is_err());
        assert!(Fdn::new(Settings { output_rotation: bad, ..Settings::default() }).is_err());
    }
}

#[test]
fn feedback_rotations_preserve_energy_and_reject_hostile_angles() {
    for angle in [-3., -0.5, 0., 0.7, 3.] {
        let core = Fdn::new(Settings { feedback_rotation: angle, ..Settings::default() }).unwrap();
        if let Some(rotations) = core.rotations {
            for i in 0..8 {
                let mut v = [0.; 8];
                v[i] = 1.;
                mix(&mut v);
                v.rotate_left(1);
                rotate_pairs(&mut v, rotations);
                assert!((v.iter().map(|x| x * x).sum::<f32>() - 1.).abs() < 1e-6);
            }
        }
    }
    for bad in [f64::NAN, f64::INFINITY, -4., 4.] {
        assert!(Fdn::new(Settings { feedback_rotation: bad, ..Settings::default() }).is_err());
    }
}

#[test]
fn observation_taps_preserve_feedback_state_and_history_budget() {
    let settings = Settings { high_decay_ratio: 0.4, feedback_rotation: 0.7, ..Settings::default() };
    let mut original = Fdn::new(settings).unwrap();
    let mut tapped = Fdn::new(Settings { output_tap_spread: 0.8, ..settings }).unwrap();
    assert_eq!(original.history_bytes(), tapped.history_bytes());
    let mut changed = false;
    for n in 0..24_000 {
        let input = if n == 0 { [0.25; 2] } else { [0.; 2] };
        let a = original.process_frame(input);
        let b = tapped.process_frame(input);
        assert!(b.iter().all(|x| x.is_finite()));
        changed |= a != b;
    }
    assert!(changed);
    assert_eq!(original.lines, tapped.lines);
    tapped.reset();
    assert!((0..10_000).all(|_| tapped.process_frame([0.; 2]) == [0.; 2]));
}

#[test]
fn observation_taps_reject_invalid_and_ambiguous_scattering_settings() {
    for spread in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
        assert!(Fdn::new(Settings { output_tap_spread: spread, ..Settings::default() }).is_err());
    }
    assert!(Fdn::new(Settings { output_tap_spread: 0.5, loop_diffusion: 0.1, ..Settings::default() }).is_err());
    for rate in [8000, 48_000, 384_000] {
        let mut fdn =
            Fdn::new(Settings { sample_rate: rate, damping_hz: 1000., output_tap_spread: 1., ..Settings::default() })
                .unwrap();
        assert_eq!(fdn.process_frame([0.25; 2]), [0.; 2]);
        assert_ne!(fdn.process_frame([0.; 2]), [0.; 2]);
    }
}

#[test]
fn custom_projections_match_linear_observations_without_changing_feedback() {
    let settings = Settings { input_rotation: -0.8, output_tap_spread: 0.3, ..Settings::default() };
    let mut primary = Fdn::new(settings).unwrap();
    let mut alternate = Fdn::new(Settings { output_rotation: std::f64::consts::FRAC_PI_2, ..settings }).unwrap();
    let weights = std::array::from_fn(|channel| {
        std::array::from_fn(|i| {
            0.6 * primary.output_weights[channel][i] - 0.2 * alternate.output_weights[1 - channel][i]
        })
    });
    let mut custom = Fdn::new(Settings { output_weights: Some(weights), ..settings }).unwrap();
    for n in 0..24_000 {
        let input = if n == 0 { [0.25; 2] } else { [0.; 2] };
        let a = primary.process_frame(input);
        let b = alternate.process_frame(input);
        let c = custom.process_frame(input);
        for channel in 0..2 {
            assert!((c[channel] - (0.6 * a[channel] - 0.2 * b[1 - channel])).abs() < 1e-7);
        }
    }
    assert_eq!(primary.lines, custom.lines);
    assert_eq!(primary.history_bytes(), custom.history_bytes());
}

#[test]
fn custom_projections_reject_nonfinite_and_unbounded_gains() {
    for bad in [f32::NAN, f32::INFINITY, -8.01, 8.01] {
        let mut weights = [LEFT, RIGHT];
        weights[1][7] = bad;
        assert!(Fdn::new(Settings { output_weights: Some(weights), ..Settings::default() }).is_err());
    }
}

#[test]
fn long_loop_diffusion_is_bounded_and_zero_gain_preserves_audio() {
    for bad in [f64::NAN, f64::INFINITY, 0., 32.01] {
        assert!(Fdn::new(Settings { loop_diffusion_scale: bad, ..Settings::default() }).is_err());
    }
    let mut original = Fdn::new(Settings::default()).unwrap();
    let mut expanded = Fdn::new(Settings { loop_diffusion_scale: 32., ..Settings::default() }).unwrap();
    for n in 0..48000 {
        let input = if n == 0 { [0.25; 2] } else { [0.; 2] };
        assert_eq!(original.process_frame(input), expanded.process_frame(input));
    }
    let mut core = Fdn::new(Settings {
        loop_diffusion_scale: 32.,
        loop_diffusion: 0.7,
        decay_seconds: 0.5,
        ..Settings::default()
    })
    .unwrap();
    let mut late_energy = 0_f64;
    for n in 0..96000 {
        let y = core.process_frame(if n == 0 { [0.25; 2] } else { [0.; 2] });
        assert!(y.iter().all(|x| x.is_finite()));
        if n >= 72000 {
            late_energy += y.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
        }
    }
    assert!(late_energy < 1e-15, "decay must not grow with allpass history: {late_energy}");
    core.reset();
    for _ in 0..24000 {
        assert_eq!(core.process_frame([0.; 2]), [0.; 2]);
    }
}

#[test]
fn sixteen_line_mix_preserves_energy_and_network_decays() {
    let mut values = std::array::from_fn::<_, 16, _>(|i| i as f32 - 7.5);
    let before = values.iter().map(|x| x * x).sum::<f32>();
    mix(&mut values);
    assert!((before - values.iter().map(|x| x * x).sum::<f32>()).abs() < 1e-3);
    assert!(Network::<7>::new(Settings::default()).is_err());
    assert!(Fdn16::new(Settings { output_weights: Some([[1.; 8]; 2]), ..Default::default() }).is_err());
    let mut core = Fdn16::new(Settings { decay_seconds: 0.5, ..Default::default() }).unwrap();
    let mut energy = 0_f64;
    let mut late = 0_f64;
    for n in 0..96000 {
        let y = core.process_frame(if n == 0 { [0.25; 2] } else { [0.; 2] });
        assert!(y.iter().all(|x| x.is_finite()));
        let p = y.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
        energy += p;
        if n > 72000 {
            late += p;
        }
    }
    assert!(energy > 1e-5);
    assert!(late < 1e-12);
    core.reset();
    for _ in 0..48000 {
        assert_eq!(core.process_frame([0.; 2]), [0.; 2]);
    }
}

#[test]
fn prepared_matrix_rejects_nonorthogonal_and_wrong_order() {
    let identity = std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1. } else { 0. }));
    assert!(Fdn::new(Settings { feedback_matrix: Some(identity), ..Settings::default() }).is_ok());
    assert!(Fdn16::new(Settings { feedback_matrix: Some(identity), ..Settings::default() }).is_err());
    for value in [f32::NAN, f32::INFINITY, 0., 1.001, 2.] {
        let mut matrix = identity;
        matrix[0][0] = value;
        assert!(Fdn::new(Settings { feedback_matrix: Some(matrix), ..Settings::default() }).is_err());
    }
}

#[test]
fn diagonal_matrix_matches_independent_comb_recurrence() {
    let identity = std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1. } else { 0. }));
    let mut fdn = Fdn::new(Settings { feedback_matrix: Some(identity), ..Settings::default() }).unwrap();
    assert_eq!(fdn.process_frame([1.; 2]), [0.; 2]);
    let gain = 10_f64.powf(-3. * 1429. / 48_000. / 2.) as f32;
    for frame in 1..=2858 {
        let y = fdn.process_frame([0.; 2]);
        if frame == 1429 {
            assert!((y[0] - 0.125).abs() < 1e-6);
        }
        if frame == 2858 {
            assert!((y[0] - 0.125 * gain).abs() < 1e-6);
        }
    }
    fdn.reset();
    for _ in 0..4000 {
        assert_eq!(fdn.process_frame([0.; 2]), [0.; 2]);
    }
}

#[test]
fn prepared_input_projection_is_bounded_and_matches_the_comb_amplitude() {
    for value in [f32::NAN, f32::INFINITY, -8.01, 8.01] {
        assert!(Fdn::new(Settings { input_weights: Some([value; 8]), ..Settings::default() }).is_err());
    }
    assert!(Fdn16::new(Settings { input_weights: Some([1.; 8]), ..Settings::default() }).is_err());
    let identity = std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1. } else { 0. }));
    let mut fdn = Fdn::new(Settings {
        feedback_matrix: Some(identity),
        input_weights: Some([2., 0., 0., 0., 0., 0., 0., 0.]),
        ..Settings::default()
    })
    .unwrap();
    fdn.process_frame([1.; 2]);
    for _ in 1..1429 {
        assert_eq!(fdn.process_frame([0.; 2]), [0.; 2]);
    }
    let y = fdn.process_frame([0.; 2]);
    assert!((y[0] - 0.25).abs() < 1e-6 && (y[1] - 0.25).abs() < 1e-6);
}

#[test]
fn scattering_delay_compensation_rejects_consumed_main_delays() {
    assert!(
        Fdn::new(Settings {
            room_scale: 0.5,
            loop_diffusion: 0.3,
            loop_diffusion_scale: 32.,
            loop_delay_compensation: true,
            ..Settings::default()
        })
        .is_err()
    );
}

#[test]
fn scattering_delay_compensation_is_identical_when_scattering_is_bypassed() {
    let mut a = Fdn::new(Settings::default()).unwrap();
    let mut b = Fdn::new(Settings { loop_delay_compensation: true, ..Settings::default() }).unwrap();
    for n in 0..48000 {
        let x = if n % 173 == 0 { [0.25, -0.125] } else { [0.; 2] };
        assert_eq!(a.process_frame(x), b.process_frame(x));
    }
}

#[test]
fn compensated_scattering_matches_isolated_path_arrivals_and_reset() {
    let identity = std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 1. } else { 0. }));
    let mut core = Fdn::new(Settings {
        feedback_matrix: Some(identity),
        input_weights: Some([1., 0., 0., 0., 0., 0., 0., 0.]),
        loop_diffusion: 0.3,
        loop_delay_compensation: true,
        ..Settings::default()
    })
    .unwrap();
    let scatter_loss = 10_f64.powf(-3. * 43. / 48000. / 2.) as f32;
    for _ in 0..2 {
        for n in 0..=1429 {
            let y = core.process_frame(if n == 0 { [1.; 2] } else { [0.; 2] });
            if n < 1386 {
                assert_eq!(y, [0.; 2]);
            }
            if n == 1386 {
                assert!((y[0] + 0.3 / 8.).abs() < 1e-6);
                assert!((y[1] - y[0]).abs() < 1e-7);
            }
            if n == 1429 {
                assert!((y[0] - (1. - 0.3 * 0.3) * scatter_loss / 8.).abs() < 1e-6);
            }
        }
        core.reset();
    }
}
