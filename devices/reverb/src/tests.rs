// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_device_api::conformance;

use super::*;

suhina_device_api::conformance_test!(Reverb::new());

#[test]
fn the_mixing_matrix_is_orthogonal_for_every_density() {
    for n in DENSITY_LINES {
        for i in 0..n {
            let mut column = [0.0f64; 16];
            column[i] = 1.0;
            mix_lines(&mut column, n);
            let energy: f64 = column.iter().map(|x| x * x).sum();
            assert!((energy - 1.0).abs() < 1e-12, "{n} lines, column {i}: {energy}");
            assert!(column.iter().skip(n).all(|x| *x == 0.0));
            // Every line feeds every other one.
            assert!(column.iter().take(n).all(|x| x.abs() > 0.1), "{n}: {column:?}");
            for j in 0..i {
                let mut other = [0.0f64; 16];
                other[j] = 1.0;
                mix_lines(&mut other, n);
                let dot: f64 = column.iter().zip(&other).map(|(a, b)| a * b).sum();
                assert!(dot.abs() < 1e-12, "{n}: columns {i} and {j}");
            }
        }
    }
}

/// A reverb at `sample_rate`, fully wet, no modulation, then `settings`.
fn reverb(sample_rate: f64, settings: &[(usize, f64)]) -> Reverb {
    let mut r = Reverb::new();
    for &(index, value) in [(param::MIX, 1.0), (param::CHORUS, 0.0), (param::SPIN, 0.0)].iter().chain(settings) {
        r.set_param(index, value);
    }
    let _ = r.prepare(sample_rate);
    r
}

/// The response to an impulse of 0.5 on both sides, `seconds` long, in blocks of 256 frames.
fn impulse_response(r: &mut Reverb, sample_rate: f64, seconds: f64) -> Vec<[f32; 2]> {
    let frames = (seconds * sample_rate) as usize;
    let mut buffer = vec![0.0f32; frames * 2];
    buffer[0] = 0.5;
    buffer[1] = 0.5;
    for block in buffer.chunks_mut(512) {
        r.process(&ProcessContext::default(), block);
    }
    buffer.as_chunks::<2>().0.to_vec()
}

/// Energy (both sides) of `ir` from `from` to `to` seconds.
fn energy(ir: &[[f32; 2]], sample_rate: f64, from: f64, to: f64) -> f64 {
    let range = (from * sample_rate) as usize..(to * sample_rate) as usize;
    ir.get(range).unwrap_or_default().iter().map(|[l, r]| f64::from(*l).powi(2) + f64::from(*r).powi(2)).sum()
}

/// Decay time from the energy between two windows of the tail.
fn t60(ir: &[[f32; 2]], sample_rate: f64) -> f64 {
    let (a, b) = (energy(ir, sample_rate, 0.3, 0.5), energy(ir, sample_rate, 1.3, 1.5));
    60.0 / (10.0 * (a / b).log10())
}

fn correlation(ir: &[[f32; 2]], sample_rate: f64) -> f64 {
    let window = ir.get((0.3 * sample_rate) as usize..(1.5 * sample_rate) as usize).unwrap_or_default();
    let (mut ll, mut rr, mut lr) = (0.0f64, 0.0f64, 0.0f64);
    for [l, r] in window {
        let (l, r) = (f64::from(*l), f64::from(*r));
        ll += l * l;
        rr += r * r;
        lr += l * r;
    }
    lr / (ll * rr).sqrt()
}

#[test]
fn the_tail_decays_in_the_decay_time_at_every_sample_rate() {
    // The network is tuned to 0.9 of the setting, as the reference renders decay.
    for decay in [1.0, 3.0] {
        let mut times = Vec::new();
        for sr in [44_100.0, 96_000.0] {
            let mut r =
                reverb(sr, &[(param::DECAY, decay), (param::REFLECT, MIN_LEVEL_DB), (param::HIGH_DAMPING, 0.0)]);
            let ir = impulse_response(&mut r, sr, 1.6);
            times.push(t60(&ir, sr));
        }
        for t in &times {
            assert!((t / (decay * DECAY_RATIO) - 1.0).abs() < 0.1, "decay {decay}: {times:?}");
        }
        assert!((times[0] / times[1] - 1.0).abs() < 0.05, "decay {decay}: {times:?}");
    }
}

/// A 1 kHz sine burst, 0.2 s long, then silence, `seconds` in all.
fn burst(sample_rate: f64, seconds: f64) -> Vec<f32> {
    let frames = (seconds * sample_rate) as usize;
    (0..frames)
        .flat_map(|n| {
            let t = n as f64 / sample_rate;
            let v = if t < 0.2 { (std::f64::consts::TAU * 1_000.0 * t).sin() as f32 * 0.5 } else { 0.0 };
            [v, v]
        })
        .collect()
}

#[test]
fn levels_match_across_sample_rates() {
    // Energy per second of the response to the same sound, at two rates.
    let levels: Vec<f64> = [44_100.0, 96_000.0]
        .map(|sr| {
            let mut r = reverb(sr, &[]);
            let mut signal = burst(sr, 1.5);
            for block in signal.chunks_mut(512) {
                r.process(&ProcessContext::default(), block);
            }
            let frames = signal.as_chunks::<2>().0.to_vec();
            10.0 * (energy(&frames, sr, 0.2, 1.5) / sr).log10()
        })
        .to_vec();
    assert!((levels[0] - levels[1]).abs() < 0.5, "{levels:?}");
}

#[test]
fn stereo_turns_two_uncorrelated_sides_into_one() {
    let sr = 44_100.0;
    for (stereo, expected) in [(120.0, 0.0), (100.0, (std::f64::consts::FRAC_PI_4 / 6.0 * 2.0).sin()), (0.0, 1.0)] {
        let mut r = reverb(sr, &[(param::STEREO, stereo), (param::REFLECT, MIN_LEVEL_DB)]);
        let c = correlation(&impulse_response(&mut r, sr, 1.6), sr);
        assert!((c - expected).abs() < 0.1, "stereo {stereo}: {c} against {expected}");
    }
}

#[test]
fn reflect_and_diffuse_are_independent_gains() {
    let sr = 44_100.0;
    let tail = |reflect: f64| {
        let mut r = reverb(sr, &[(param::REFLECT, reflect)]);
        energy(&impulse_response(&mut r, sr, 1.0), sr, 0.3, 1.0)
    };
    // The tail after the reflections does not change with Reflect.
    assert!((tail(MIN_LEVEL_DB) / tail(6.0) - 1.0).abs() < 1e-6);
    let early = |diffuse: f64| {
        let mut r = reverb(sr, &[(param::DIFFUSE, diffuse), (param::SHAPE, 1.0)]);
        energy(&impulse_response(&mut r, sr, 0.1), sr, 0.0, 0.03)
    };
    // The reflections before the tail starts do not change with Diffuse.
    assert!((early(MIN_LEVEL_DB) / early(6.0) - 1.0).abs() < 1e-6);
}

#[test]
fn the_dry_signal_is_not_delayed_and_the_mix_is_equal_power() {
    let sr = 44_100.0;
    let mut r = reverb(sr, &[(param::MIX, 0.55)]);
    let ir = impulse_response(&mut r, sr, 0.01);
    let dry = (0.55 * std::f64::consts::FRAC_PI_2).cos() * 0.5;
    assert!((f64::from(ir[0][0]) - dry).abs() < 1e-6, "{:?}", ir[0]);
    assert_eq!(r.latency(), 0);
}

#[test]
fn the_first_reflection_arrives_after_the_predelay() {
    let sr = 48_000.0;
    for predelay in [0.0005, 0.01, 0.25] {
        let mut r = reverb(sr, &[(param::PREDELAY, predelay), (param::DIFFUSE, MIN_LEVEL_DB)]);
        let ir = impulse_response(&mut r, sr, 0.3);
        let first = ir.iter().skip(1).position(|[l, r]| l.abs().max(r.abs()) > 1e-4).map(|n| n + 1);
        let expected = (predelay * sr).round() as usize;
        assert!(first.is_some_and(|n| n.abs_diff(expected) <= 2), "{predelay}: {first:?} against {expected}");
    }
}

#[test]
fn freeze_holds_the_tail_and_cut_keeps_new_input_out() {
    let sr = 44_100.0;
    let mut r = reverb(sr, &[(param::REFLECT, MIN_LEVEL_DB), (param::DECAY, 0.5)]);
    let run = |r: &mut Reverb, signal: &mut [f32]| {
        for block in signal.chunks_mut(512) {
            r.process(&ProcessContext::default(), block);
        }
    };
    let level = |b: &[f32]| 10.0 * (b.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / b.len() as f64).log10();
    // A burst, and Freeze while its tail is still loud: the tail then holds its level.
    let mut start = burst(sr, 0.3);
    run(&mut r, &mut start);
    r.set_param(param::FREEZE, 1.0);
    let mut held = vec![0.0f32; 44_100 * 2 * 2];
    run(&mut r, &mut held);
    let (early, late) = (level(&held[17_640..35_280]), level(&held[158_760..]));
    // Lossless, but the energy spreads evenly over the lines, which moves the output a little.
    assert!((early - late).abs() < 2.0, "{early} dB then {late} dB");
    // With Cut on, another burst while frozen adds nothing to the tail.
    let mut more = burst(sr, 1.0);
    run(&mut r, &mut more);
    let after = level(&more[44_100..]);
    assert!((after - late).abs() < 2.0, "{late} dB, then {after} dB after more input");
}

#[test]
fn silence_decays_to_exact_silence() {
    let sr = 44_100.0;
    let mut r = reverb(sr, &[(param::DECAY, 0.2), (param::CHORUS, 1.0), (param::SPIN, 1.0)]);
    let mut signal = conformance::test_signal(4_410);
    signal.resize(44_100 * 2 * 20, 0.0);
    for block in signal.chunks_mut(512) {
        r.process(&ProcessContext::default(), block);
    }
    assert!(signal[signal.len() - 44_100..].iter().all(|v| *v == 0.0));
}

/// Largest step from one frame to the next, on either side.
fn largest_step(signal: &[f32]) -> f32 {
    let frames = signal.as_chunks::<2>().0;
    frames.windows(2).map(|w| (w[1][0] - w[0][0]).abs().max((w[1][1] - w[0][1]).abs())).fold(0.0, f32::max)
}

#[test]
fn moving_any_control_does_not_click() {
    // A low sine through the reverb, 50 % wet; each parameter jumps from one end to the other in
    // the middle. The output must not step much further than the sine and the tail do anyway.
    let sr = 44_100.0;
    let sine = |frames: usize| -> Vec<f32> {
        (0..frames)
            .flat_map(|n| {
                let v = (std::f64::consts::TAU * 110.0 * n as f64 / sr).sin() as f32 * 0.5;
                [v, v]
            })
            .collect()
    };
    let render = |index: usize, jump: Option<f64>| {
        let mut r = reverb(sr, &[(param::MIX, 0.5), (param::SMOOTH, 2.0)]);
        if let Some(info) = PARAMS.get(index) {
            r.set_param(index, info.min);
        }
        let _ = r.prepare(sr);
        let mut signal = sine(44_100);
        let (first, second) = signal.split_at_mut(22_050 * 2);
        for block in first.chunks_mut(512) {
            r.process(&ProcessContext::default(), block);
        }
        if let (Some(value), Some(info)) = (jump, PARAMS.get(index)) {
            let _ = info;
            r.set_param(index, value);
        }
        for block in second.chunks_mut(512) {
            r.process(&ProcessContext::default(), block);
        }
        largest_step(signal.get(22_050 * 2 - 2_000..22_050 * 2 + 8_000).unwrap_or_default())
    };
    for (index, info) in PARAMS.iter().enumerate() {
        // Switching Density on or off lines, and Freeze, change the sound by design.
        if matches!(index, param::DENSITY | param::FREEZE | param::SMOOTH) {
            continue;
        }
        let still = render(index, None);
        let moved = render(index, Some(info.max));
        assert!(moved < still * 1.5 + 0.01, "{}: a step of {moved} against {still}", info.id);
    }
}

/// The cost of one instance at its defaults: run with `cargo test --release -p suhina-device-reverb
/// cost -- --ignored --nocapture`. The budget (docs/spec/reverb.md › Cost) is checked by hand, as CI
/// machines vary too much for a timing test.
#[test]
#[ignore]
fn cost_of_a_minute_at_48_khz() {
    let mut device = Reverb::new();
    let _ = device.prepare(48_000.0);
    let mut signal = conformance::test_signal(48_000 * 60);
    let start = std::time::Instant::now();
    for block in signal.chunks_mut(512) {
        device.process(&ProcessContext::default(), block);
    }
    let seconds = start.elapsed().as_secs_f64();
    println!("60 s at 48 kHz in {seconds:.3} s: {:.2} % of a core", seconds / 60.0 * 100.0);
}
