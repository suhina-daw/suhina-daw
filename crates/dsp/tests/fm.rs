// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exercise the public operators without a dependency on any instrument crate.

use suhina_dsp::fm::{LG_N, N, Tables, render_feedback, render_modulated, render_sine};

const UNITY: i32 = 1 << 24;
const QUARTER: i32 = UNITY / 4;

#[test]
fn a_sine_has_the_expected_quarter_cycles_and_wraps_phase() {
    let tables = Tables::new(48_000.0);
    for phase in [0, i32::MIN, i32::MAX - (QUARTER - 1)] {
        let mut output = [0; N];
        render_sine(&mut output, phase, QUARTER, UNITY, UNITY, false, &tables);
        for (i, sample) in output.iter().enumerate() {
            let position = f64::from(phase) / f64::from(UNITY) + i as f64 / 4.0;
            let expected = (position * std::f64::consts::TAU).sin() * f64::from(UNITY);
            assert!((f64::from(*sample) - expected).abs() < 10.0);
        }
    }
}

#[test]
fn modulation_is_phase_in_cycles_and_in_place_reads_before_writing() {
    let tables = Tables::new(48_000.0);
    let input = std::array::from_fn(|i| (i as i32).wrapping_mul(QUARTER));
    let mut external = [0; N];
    render_modulated(&mut external, Some(&input), 0, 0, UNITY, UNITY, false, &tables);
    let mut inplace = input;
    render_modulated(&mut inplace, None, 0, 0, UNITY, UNITY, false, &tables);
    let mut unmodulated = [0; N];
    render_sine(&mut unmodulated, 0, QUARTER, UNITY, UNITY, false, &tables);
    assert_eq!(external, unmodulated);
    assert_eq!(inplace, external);

    let mut added = input;
    render_modulated(&mut added, None, 0, 0, UNITY, UNITY, true, &tables);
    for ((actual, original), rendered) in added.iter().zip(input).zip(external) {
        assert_eq!(*actual, original.wrapping_add(rendered));
    }
}

#[test]
fn gains_ramp_before_each_sample_in_both_directions() {
    let tables = Tables::new(48_000.0);
    for (start, end, first, step) in [(0, 64, 1, 1), (64, 0, 63, -1)] {
        let mut output = [0; N];
        render_sine(&mut output, QUARTER, 0, start, end, false, &tables);
        for (i, sample) in output.iter().enumerate() {
            assert!((*sample - (first + i as i32 * step)).abs() <= 1);
        }
    }
}

#[test]
fn mixing_wraps_and_replacing_ignores_previous_output() {
    let tables = Tables::new(48_000.0);
    let mut rendered = [123; N];
    render_sine(&mut rendered, QUARTER, 0, UNITY, UNITY, false, &tables);
    let mut added = [i32::MAX; N];
    render_sine(&mut added, QUARTER, 0, UNITY, UNITY, true, &tables);
    for (actual, sample) in added.iter().zip(rendered) {
        assert_eq!(*actual, i32::MAX.wrapping_add(sample));
    }
}

#[test]
fn the_caller_can_continue_phase_across_blocks() {
    let tables = Tables::new(48_000.0);
    let frequency: i32 = 17_123;
    let initial_phase = i32::MAX - 100;
    let mut phase = initial_phase;
    for block in 0..5 {
        let mut output = [0; N];
        render_sine(&mut output, phase, frequency, UNITY, UNITY, false, &tables);
        for (i, actual) in output.iter().enumerate() {
            let position =
                (f64::from(initial_phase) + (block * N + i) as f64 * f64::from(frequency)) / f64::from(UNITY);
            let expected = (position * std::f64::consts::TAU).sin() * f64::from(UNITY);
            // Linear interpolation of a 1024-entry sine table has a bounded error.
            assert!((f64::from(*actual) - expected).abs() < 100.0);
        }
        phase = phase.wrapping_add(frequency.wrapping_shl(LG_N));
    }
}

#[test]
fn feedback_history_is_scaled_output_independent_of_the_mix_destination() {
    let tables = Tables::new(48_000.0);
    let mut history = [0; 2];
    let mut mixed_history = history;
    for block in 0..3 {
        let phase = block * 77_777 * N as i32;
        let mut output = [0; N];
        render_feedback(&mut output, phase, 77_777, UNITY / 2, UNITY, &mut history, 2, false, &tables);
        assert_eq!(history, [output[N - 2], output[N - 1]]);
        let mut mixed = [i32::MAX; N];
        render_feedback(&mut mixed, phase, 77_777, UNITY / 2, UNITY, &mut mixed_history, 2, true, &tables);
        assert_eq!(mixed_history, history);
        for (actual, sample) in mixed.iter().zip(output) {
            assert_eq!(*actual, i32::MAX.wrapping_add(sample));
        }
    }
}

#[test]
fn feedback_shift_extremes_use_the_documented_limits() {
    let tables = Tables::new(48_000.0);
    for (extreme, limit) in [(i32::MIN, -1), (i32::MAX, 30)] {
        let mut outputs = [[0; N]; 2];
        let mut histories = [[i32::MAX, i32::MIN]; 2];
        for ((output, history), shift) in outputs.iter_mut().zip(&mut histories).zip([extreme, limit]) {
            render_feedback(output, i32::MAX, i32::MIN, i32::MIN, i32::MAX, history, shift, false, &tables);
        }
        assert_eq!(outputs[0], outputs[1]);
        assert_eq!(histories[0], histories[1]);
    }
}

#[test]
fn all_render_modes_accept_extreme_integer_inputs() {
    let tables = Tables::new(48_000.0);
    for value in [i32::MIN, -1, 0, 1, i32::MAX] {
        let mut pure = [value; N];
        let mut modulated = pure;
        render_sine(&mut pure, value, value, value, value.wrapping_neg(), true, &tables);
        render_modulated(&mut modulated, Some(&[0; N]), value, value, value, value.wrapping_neg(), true, &tables);
        assert_eq!(pure, modulated, "zero phase modulation must equal the pure operator");
    }
}

#[test]
fn invalid_sample_rates_use_the_same_prepared_data_as_48_khz() {
    let expected = Tables::new(48_000.0);
    for rate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -48_000.0, 0.0, f64::MIN_POSITIVE, 1.0, 999.9] {
        let actual = Tables::new(rate);
        assert_eq!(actual.env_rate, expected.env_rate);
        assert_eq!(actual.lfo_unit, expected.lfo_unit);
        assert_eq!(actual.lfo_ratio, expected.lfo_ratio);
        assert_eq!(actual.pitch_unit, expected.pitch_unit);
        assert_eq!(actual.porta, expected.porta);
        assert_eq!(actual.porta_glissando, expected.porta_glissando);
        for value in [i32::MIN, -1, 0, 123_456_789, i32::MAX] {
            assert_eq!(actual.freq(value), expected.freq(value));
            assert_eq!(actual.exp2(value), expected.exp2(value));
            assert_eq!(actual.sin(value), expected.sin(value));
        }
    }
}

#[test]
fn frequency_tables_follow_pitch_and_sample_rate() {
    for rate in [1_000.0, 8_000.0, 44_100.0, 48_000.0, 96_000.0, 384_000.0] {
        let tables = Tables::new(rate);
        for hz in [55.0_f64, 110.0, 220.0, 440.0] {
            let log = (hz.log2() * f64::from(UNITY)) as i32;
            let expected = hz / rate * f64::from(UNITY);
            assert!((f64::from(tables.freq(log)) - expected).abs() < expected * 0.000_001 + 1.0);
        }
        for value in [i32::MIN, i32::MAX] {
            // Accepted inputs remain safe even outside the audible pitch range.
            let _ = (tables.freq(value), tables.exp2(value), tables.sin(value));
        }
    }
    let tables = Tables::new(f64::MAX);
    assert_eq!(tables.freq(i32::MAX), 0);
}
