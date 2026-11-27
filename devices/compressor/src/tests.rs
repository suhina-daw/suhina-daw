// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_device_api::conformance::test_signal;

use super::*;

const SR: f64 = 48_000.0;

fn compressor(settings: &[(usize, f64)]) -> Compressor {
    let mut c = Compressor::new();
    for &(index, value) in settings {
        c.set_param(index, value);
    }
    c.reset();
    c
}

/// `frames` frames of the sum of `(hz, amplitude)` sines, the same on both sides.
fn sines(parts: &[(f64, f64)], frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|n| {
            let t = n as f64 / SR;
            let v: f64 = parts.iter().map(|(hz, a)| a * (std::f64::consts::TAU * hz * t).sin()).sum();
            [v as f32, v as f32]
        })
        .collect()
}

/// The largest sample on the left side from frame `from` on.
fn peak(signal: &[f32], from: usize) -> f64 {
    signal.as_chunks::<2>().0.iter().skip(from).fold(0.0, |m, [l, _]| m.max(f64::from(l.abs())))
}

fn db(gain: f64) -> f64 {
    20.0 * gain.log10()
}

suhina_device_api::conformance_test!(Compressor::with_rate);

#[test]
fn reduces_a_steady_sine_by_threshold_and_ratio() {
    // A 1 kHz sine peaking at -6 dB, a hard knee at -20 dB: 14 dB over the threshold.
    for ratio in [1.0, 2.0, 4.0, 10.0] {
        let mut c = compressor(&[(param::THRESHOLD, -20.0), (param::RATIO, ratio), (param::KNEE, 0.0)]);
        let mut s = sines(&[(1_000.0, 0.5)], 48_000);
        c.process(&ProcessContext::default(), &mut s);
        let expected = -6.02 - 14.0 * (1.0 - 1.0 / ratio);
        let got = db(peak(&s, 24_000));
        assert!((got - expected).abs() < 0.5, "ratio {ratio}: {got} dB, expected {expected} dB");
    }
}

#[test]
fn leaves_a_signal_under_the_threshold_alone() {
    let input = sines(&[(440.0, 0.05)], 24_000);
    let mut output = input.clone();
    compressor(&[(param::THRESHOLD, -12.0), (param::KNEE, 0.0)]).process(&ProcessContext::default(), &mut output);
    assert_eq!(input, output);
}

#[test]
fn soft_knee_compresses_gently_at_the_threshold() {
    // A peak right at the threshold: no reduction with a hard knee, some with a soft one.
    let at = |knee| {
        let mut c = compressor(&[(param::THRESHOLD, -12.0), (param::RATIO, 4.0), (param::KNEE, knee)]);
        let mut s = sines(&[(1_000.0, db_to_gain(-12.0) as f64)], 24_000);
        c.process(&ProcessContext::default(), &mut s);
        db(peak(&s, 12_000))
    };
    let hard = at(0.0);
    let soft = at(12.0);
    assert!((hard + 12.0).abs() < 0.05, "{hard}");
    // Halfway into a 12 dB knee at ratio 4: 0.75 * 6^2 / 24 = 1.125 dB.
    assert!((soft - (-12.0 - 1.125)).abs() < 0.2, "{soft}");
}

/// A signal whose detector level is constant: full scale at the Nyquist frequency, `level`
/// from frame `on` to frame `off` and silent outside, run through `c` one frame at a time;
/// returns the gain reduction after each frame.
fn reduction_curve(c: &mut Compressor, level: f32, on: usize, off: usize, frames: usize) -> Vec<f64> {
    (0..frames)
        .map(|n| {
            let v = if (on..off).contains(&n) { if n % 2 == 0 { level } else { -level } } else { 0.0 };
            c.process(&ProcessContext::default(), &mut [v, v]);
            c.gain_reduction_db()
        })
        .collect()
}

#[test]
fn attack_and_release_are_time_constants() {
    // 20 dB over a hard-knee threshold at ratio 20: 19 dB of reduction once settled.
    let attack = 0.01;
    let release = 0.1;
    let mut c = compressor(&[
        (param::THRESHOLD, -20.0),
        (param::RATIO, 20.0),
        (param::KNEE, 0.0),
        (param::ATTACK, attack),
        (param::RELEASE, release),
    ]);
    let (on, off) = (1_000, 49_000);
    let curve = reduction_curve(&mut c, 1.0, on, off, 98_000);
    let full = 19.0;
    let settled = curve.get(off - 1).copied().unwrap_or(0.0);
    assert!((settled - full).abs() < 0.2, "{settled}");
    let after_attack = curve.get(on + (attack * SR) as usize).copied().unwrap_or(0.0);
    assert!((after_attack / settled - 0.632).abs() < 0.03, "{after_attack}");
    let after_release = curve.get(off + (release * SR) as usize).copied().unwrap_or(0.0);
    assert!((after_release / settled - 0.368).abs() < 0.03, "{after_release}");
    assert_eq!(curve.get(on - 1), Some(&0.0));
}

#[test]
fn dry_wet_blends_compressed_with_uncompressed() {
    let input = test_signal(4_096);
    let mut output = input.clone();
    compressor(&[(param::THRESHOLD, -40.0), (param::RATIO, 20.0), (param::MIX, 0.0)])
        .process(&ProcessContext::default(), &mut output);
    assert_eq!(input, output, "fully dry is the input exactly");

    let level = |mix| {
        let mut s = sines(&[(1_000.0, 0.5)], 24_000);
        compressor(&[(param::THRESHOLD, -30.0), (param::RATIO, 20.0), (param::MIX, mix)])
            .process(&ProcessContext::default(), &mut s);
        peak(&s, 12_000)
    };
    let (wet, half) = (level(1.0), level(0.5));
    assert!(wet < 0.1, "{wet}");
    // Parallel compression: half of the dry signal plus half of the compressed one.
    assert!((half - (0.25 + wet / 2.0)).abs() < 0.01, "{half} {wet}");
}

#[test]
fn input_and_output_gains() {
    // Under the threshold the two gains add up: +6 dB in, -12 dB out.
    let mut s = sines(&[(440.0, 0.01)], 4_800);
    compressor(&[(param::INPUT, 6.0), (param::OUTPUT, -12.0)]).process(&ProcessContext::default(), &mut s);
    assert!((db(peak(&s, 0) / 0.01) + 6.0).abs() < 0.05, "{}", db(peak(&s, 0) / 0.01));
    // Input gain drives the detector: the same signal 12 dB hotter is compressed more.
    let reduction = |input| {
        let mut c = compressor(&[(param::INPUT, input), (param::THRESHOLD, -24.0)]);
        let mut s = sines(&[(1_000.0, 0.1)], 24_000);
        c.process(&ProcessContext::default(), &mut s);
        c.gain_reduction_db()
    };
    assert!(reduction(12.0) > reduction(0.0) + 6.0, "{} {}", reduction(0.0), reduction(12.0));
}

#[test]
fn the_side_chain_high_pass_lets_bass_through_uncompressed() {
    // Loud 50 Hz bass with a quiet 2 kHz melody on top.
    let run = |hz| {
        let mut c = compressor(&[(param::THRESHOLD, -30.0), (param::RATIO, 10.0), (param::SIDECHAIN_HIGH_PASS, hz)]);
        let mut s = sines(&[(50.0, 0.5), (2_000.0, 0.05)], 48_000);
        let mut most = 0.0f64;
        for (n, frame) in s.chunks_mut(2).enumerate() {
            c.process(&ProcessContext::default(), frame);
            if n > 24_000 {
                most = most.max(c.gain_reduction_db());
            }
        }
        (most, peak(&s, 24_000))
    };
    let (full_reduction, full_peak) = run(20.0);
    let (filtered_reduction, filtered_peak) = run(1_000.0);
    assert!(full_reduction > 15.0, "{full_reduction}");
    assert!(filtered_reduction < full_reduction - 10.0, "{filtered_reduction} {full_reduction}");
    assert!(filtered_peak > 2.0 * full_peak, "{filtered_peak} {full_peak}");
}

#[test]
fn a_threshold_change_is_ramped() {
    let mut c = compressor(&[(param::THRESHOLD, 0.0), (param::RATIO, 20.0), (param::ATTACK, 0.0001)]);
    let mut warm = sines(&[(1_000.0, 0.5)], 4_800);
    c.process(&ProcessContext::default(), &mut warm);
    c.set_param(param::THRESHOLD, -60.0);
    // One frame later the reduction has barely started; after the ramp it is complete.
    c.process(&ProcessContext::default(), &mut [0.5, 0.5]);
    assert!(c.gain_reduction_db() < 2.0, "{}", c.gain_reduction_db());
    let mut s = sines(&[(1_000.0, 0.5)], 4_800);
    c.process(&ProcessContext::default(), &mut s);
    assert!(c.gain_reduction_db() > 40.0, "{}", c.gain_reduction_db());
}

#[test]
fn a_loud_key_ducks_a_quiet_signal_and_a_silent_key_leaves_it() {
    let settings = [(param::THRESHOLD, -20.0), (param::RATIO, 8.0), (param::KNEE, 0.0)];
    // A signal well under the threshold: compressed by its own level, it would pass unchanged.
    let quiet = sines(&[(220.0, 0.01)], 9600);
    let loud = sines(&[(60.0, 0.9)], 9600);
    let peak = |s: &[f32]| s.iter().skip(4800 * 2).step_by(2).fold(0.0f32, |m, v| m.max(v.abs()));
    let mut own = quiet.clone();
    compressor(&settings).process(&ProcessContext::default(), &mut own);
    let mut ducked = quiet.clone();
    compressor(&settings).process_keyed(&ProcessContext::default(), &loud, &mut ducked);
    let mut silent = quiet.clone();
    compressor(&settings).process_keyed(&ProcessContext::default(), &vec![0.0; quiet.len()], &mut silent);
    // The key's level sets the reduction; the device's own sound is what it reduces.
    assert!(peak(&ducked) < peak(&own) * 0.25, "ducked {} vs own {}", peak(&ducked), peak(&own));
    assert!((peak(&silent) - peak(&own)).abs() < 1e-4, "a silent key reduces nothing");
    // A key shorter than the buffer reads silence past its end, never panics.
    let mut short = quiet.clone();
    compressor(&settings).process_keyed(&ProcessContext::default(), &loud[..100], &mut short);
}

#[test]
fn the_readouts_show_what_the_detector_heard_and_the_reduction() {
    let mut c = compressor(&[(param::THRESHOLD, -20.0), (param::RATIO, 4.0), (param::KNEE, 0.0)]);
    let idle = c.readouts().unwrap();
    assert_eq!(idle[readout::DETECTOR], READOUT_FLOOR_DB as f32, "nothing heard yet");
    assert_eq!(idle[readout::REDUCTION], 0.0);
    // A sine peaking at -6 dB, 14 dB over the threshold: about 10.5 dB of reduction at 4 : 1.
    let mut s = sines(&[(1_000.0, 0.5)], 24_000);
    c.process(&ProcessContext::default(), &mut s);
    let r = c.readouts().unwrap();
    assert!((f64::from(r[readout::DETECTOR]) + 6.02).abs() < 0.3, "detector {}", r[readout::DETECTOR]);
    assert!((f64::from(r[readout::INPUT]) + 6.02).abs() < 0.3, "input {}", r[readout::INPUT]);
    assert!((f64::from(r[readout::REDUCTION]) - 10.5).abs() < 0.5, "reduction {}", r[readout::REDUCTION]);
    assert!((f64::from(r[readout::OUTPUT]) + 16.5).abs() < 0.6, "output {}", r[readout::OUTPUT]);
    assert_eq!(r[readout::KEYED], 0.0);
    // Silence: the peaks fall back toward the floor and the reduction toward nothing.
    // Three seconds: the levels fall about 60 dB a second, the reduction with the 0.3 s release.
    let mut quiet = vec![0.0f32; 288_000];
    c.process(&ProcessContext::default(), &mut quiet);
    let r = c.readouts().unwrap();
    assert!(r[readout::DETECTOR] < -90.0 && r[readout::OUTPUT] < -90.0, "{r:?}");
    assert!(r[readout::REDUCTION] < 0.01, "{}", r[readout::REDUCTION]);
    c.reset();
    assert_eq!(c.readouts().unwrap(), idle, "a reset clears them");
}

#[test]
fn a_key_drives_the_detector_readout_and_marks_it_keyed() {
    let mut c = compressor(&[(param::THRESHOLD, -30.0), (param::KNEE, 0.0)]);
    let key = sines(&[(100.0, 0.5)], 24_000);
    let mut quiet = sines(&[(1_000.0, 0.01)], 24_000);
    c.process_keyed(&ProcessContext::default(), &key, &mut quiet);
    let r = c.readouts().unwrap();
    assert_eq!(r[readout::KEYED], 1.0);
    assert!(r[readout::DETECTOR] > -7.0, "the detector heard the key: {}", r[readout::DETECTOR]);
    assert!(r[readout::INPUT] < -38.0, "the input is the device's own quiet sound: {}", r[readout::INPUT]);
    assert!(r[readout::REDUCTION] > 10.0, "the key pushes the gain down: {}", r[readout::REDUCTION]);
}
