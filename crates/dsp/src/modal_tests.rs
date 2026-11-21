// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn a_modes_level_is_its_decaying_amplitude() {
    let mut bank = ModalBank::<2>::default();
    assert_eq!(bank.level(0), 0.0);
    bank.strike(48000.0, &[Mode { frequency_hz: 440.0, decay_seconds: 0.5, amplitude: 0.5 }]);
    for n in 1..=24000 {
        bank.next_sample();
        if n % 6000 == 0 {
            let expected = 0.5 * (-1000.0_f64.ln() * n as f64 / 24000.0).exp();
            assert!((bank.level(0) - expected).abs() < 1e-6, "sample {n}: {} against {expected}", bank.level(0));
        }
    }
    assert_eq!(bank.level(1), 0.0, "an unused mode");
    assert_eq!(bank.level(9), 0.0, "past the bank");
}

#[test]
fn one_mode_matches_the_analytical_decay_and_pitch() {
    let mut bank = ModalBank::<1>::default();
    bank.strike(48000.0, &[Mode { frequency_hz: 440.0, decay_seconds: 0.25, amplitude: 0.5 }]);
    for n in 0..24000 {
        let expected = 0.5 * (-1000.0_f64.ln() * n as f64 / 12000.0).exp() * (TAU * 440.0 * n as f64 / 48000.0).sin();
        assert!((f64::from(bank.next_sample()) - expected).abs() < 2e-8);
    }
}

/// The second output plays each mode times its gain and shifted by its offset, from the same
/// resonator; without weights both outputs are `next_sample`.
#[test]
fn the_second_output_places_each_mode_by_gain_and_offset() {
    let modes = [
        Mode { frequency_hz: 440.0, decay_seconds: 0.5, amplitude: 0.4 },
        Mode { frequency_hz: 1250.0, decay_seconds: 0.2, amplitude: 0.3 },
    ];
    let mut plain = ModalBank::<2>::default();
    plain.strike_phased(48000.0, &modes, &[0.3, -1.1]);
    let mut placed = plain;
    placed.set_other(&[[0.5 * 0.8_f32.cos(), 0.5 * 0.8_f32.sin()]]);
    let mut unplaced = plain;
    for n in 0..4800 {
        let t = n as f64 / 48000.0;
        let mono = f64::from(plain.next_sample());
        let [first, second] = placed.next_stereo();
        let decay = |t60: f64| (-1000.0_f64.ln() * t / t60).exp();
        let expected = 0.5 * 0.4 * decay(0.5) * (TAU * 440.0 * t + 0.3 + 0.8).sin()
            + 0.3 * decay(0.2) * (TAU * 1250.0 * t - 1.1).sin();
        assert!((f64::from(first) - mono).abs() < 1e-6, "{n}");
        assert!((f64::from(second) - expected).abs() < 1e-5, "{n}: {second} vs {expected}");
        assert_eq!(unplaced.next_stereo(), [mono as f32; 2]);
    }
}

#[test]
fn hostile_modes_are_silent_and_strikes_are_bounded() {
    let mut bank = ModalBank::<16>::default();
    for rate in [f64::NAN, f64::INFINITY, -1.0, 0.0, 8000.0, 48000.0, 384000.0] {
        for frequency in [f64::NAN, f64::INFINITY, -1.0, 0.0, 440.0, 200000.0] {
            bank.strike(rate, &[Mode { frequency_hz: frequency, decay_seconds: 0.01, amplitude: 1.0 }; 32]);
            for _ in 0..8192 {
                let output = bank.next_sample();
                assert!(output.is_finite() && output.abs() <= 1.00001);
            }
            bank.reset();
            assert_eq!(bank.next_sample(), 0.0);
            assert!(!bank.is_active());
        }
    }
}

#[test]
fn modes_above_the_anti_alias_margin_are_not_folded_down() {
    let mut bank = ModalBank::<1>::default();
    bank.strike(8000.0, &[Mode { frequency_hz: 3600.0, decay_seconds: 1.0, amplitude: 1.0 }]);
    assert!(!bank.is_active());
    assert_eq!(bank.next_sample(), 0.0);
}

#[test]
fn a_capped_decay_keeps_phase_and_follows_the_new_t60() {
    let mut bank = ModalBank::<1>::default();
    bank.strike(48000.0, &[Mode { frequency_hz: 440.0, decay_seconds: 10.0, amplitude: 0.5 }]);
    for _ in 0..480 {
        bank.next_sample();
    }
    let at_cap = 0.5 * (-1000.0_f64.ln() * 480.0 / (48000.0 * 10.0)).exp();
    bank.cap_decay(48000.0, 0.1);
    for n in 0..4800 {
        let t = (480 + n) as f64;
        let expected =
            at_cap * (-1000.0_f64.ln() * n as f64 / (48000.0 * 0.1)).exp() * (TAU * 440.0 * t / 48000.0).sin();
        assert!((f64::from(bank.next_sample()) - expected).abs() < 1e-7, "{n}");
    }
    // A longer cap changes nothing.
    let mut long = ModalBank::<1>::default();
    let mut plain = ModalBank::<1>::default();
    let mode = [Mode { frequency_hz: 220.0, decay_seconds: 0.2, amplitude: 0.5 }];
    long.strike(48000.0, &mode);
    plain.strike(48000.0, &mode);
    long.cap_decay(48000.0, 5.0);
    for _ in 0..2000 {
        assert_eq!(long.next_sample(), plain.next_sample());
    }
}

#[test]
fn phased_strikes_start_each_mode_at_its_phase() {
    let mut bank = ModalBank::<2>::default();
    let modes = [
        Mode { frequency_hz: 440.0, decay_seconds: 0.5, amplitude: 0.3 },
        Mode { frequency_hz: 1234.5, decay_seconds: 0.2, amplitude: 0.2 },
    ];
    bank.strike_phased(48000.0, &modes, &[0.7, -2.1]);
    for n in 0..4800 {
        let t = n as f64;
        let expected: f64 = modes
            .iter()
            .zip([0.7, -2.1])
            .map(|(m, phase)| {
                m.amplitude
                    * (-1000.0_f64.ln() * t / (48000.0 * m.decay_seconds)).exp()
                    * (TAU * m.frequency_hz * t / 48000.0 + phase).sin()
            })
            .sum();
        assert!((f64::from(bank.next_sample()) - expected).abs() < 2e-7, "{n}");
    }
    let mut plain = ModalBank::<2>::default();
    let mut zero = ModalBank::<2>::default();
    plain.strike(48000.0, &modes);
    zero.strike_phased(48000.0, &modes, &[]);
    for _ in 0..256 {
        assert_eq!(plain.next_sample(), zero.next_sample());
    }
}

#[test]
fn gained_samples_weight_each_mode_and_unit_gains_change_nothing() {
    let modes = [
        Mode { frequency_hz: 440.0, decay_seconds: 0.5, amplitude: 0.3 },
        Mode { frequency_hz: 1234.5, decay_seconds: 0.2, amplitude: 0.2 },
    ];
    let mut plain = ModalBank::<2>::default();
    let mut unit = ModalBank::<2>::default();
    plain.strike_phased(48000.0, &modes, &[0.3, 1.0]);
    unit.strike_phased(48000.0, &modes, &[0.3, 1.0]);
    let mut gains = [1.0; 2];
    for _ in 0..512 {
        assert_eq!(plain.next_sample(), unit.next_sample_gained(&mut gains, &[0.0; 2]));
    }
    let mut one = ModalBank::<2>::default();
    let mut weighted = ModalBank::<2>::default();
    one.strike_phased(48000.0, &modes[..1], &[0.3]);
    weighted.strike_phased(48000.0, &modes, &[0.3, 1.0]);
    let mut gains = [0.0, 0.0];
    let steps = [1.0 / 256.0, 0.0];
    for n in 0..256 {
        let expected = f64::from(one.next_sample()) * n as f64 / 256.0;
        assert!((f64::from(weighted.next_sample_gained(&mut gains, &steps)) - expected).abs() < 1e-6, "{n}");
    }
}

#[test]
fn retuning_keeps_phase_and_applies_an_absolute_pitch_offset() {
    let mut bank = ModalBank::<1>::default();
    bank.strike(48000.0, &[Mode { frequency_hz: 440.0, decay_seconds: 1.0, amplitude: 0.5 }]);
    for _ in 0..100 {
        bank.next_sample();
    }
    bank.transpose(48000.0, 12.0);
    bank.transpose(48000.0, 12.0);
    for n in 0..100 {
        let expected = 0.5
            * (-1000.0_f64.ln() * (100 + n) as f64 / 48000.0).exp()
            * (TAU * (440.0 * 100.0 + 880.0 * n as f64) / 48000.0).sin();
        assert!((f64::from(bank.next_sample()) - expected).abs() < 2e-8);
    }
}
