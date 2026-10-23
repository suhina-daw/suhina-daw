// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_device_api::conformance::test_signal;

use super::*;

const SR: f64 = 48_000.0;

fn saturator(settings: &[(usize, f64)]) -> Saturator {
    let mut s = Saturator::new();
    for &(index, value) in settings {
        s.set_param(index, value);
    }
    s.reset();
    s
}

fn sine(hz: f64, level: f64, frames: usize, sample_rate: f64) -> Vec<f32> {
    (0..frames)
        .flat_map(|n| {
            let v = ((std::f64::consts::TAU * hz * n as f64 / sample_rate).sin() * level) as f32;
            [v, v]
        })
        .collect()
}

/// `frames` frames of a sine of `hz` at `level`, on both sides, through `s` at 48 kHz.
fn drive_sine(s: &mut Saturator, hz: f64, level: f64, frames: usize) -> Vec<f32> {
    let mut signal = sine(hz, level, frames, SR);
    s.process(&ProcessContext::default(), &mut signal);
    signal
}

/// Amplitude of `hz` on the left side after the first 4800 frames, from one DFT bin.
fn level_at(signal: &[f32], hz: f64, sample_rate: f64) -> f64 {
    let frames = signal.as_chunks::<2>().0.get(4_800..).unwrap_or_default();
    let (mut re, mut im) = (0.0, 0.0);
    for (n, [l, _]) in frames.iter().enumerate() {
        let w = std::f64::consts::TAU * hz * n as f64 / sample_rate;
        re += f64::from(*l) * w.cos();
        im += f64::from(*l) * w.sin();
    }
    2.0 * re.hypot(im) / frames.len().max(1) as f64
}

fn level(signal: &[f32], hz: f64) -> f64 {
    level_at(signal, hz, SR)
}

/// The largest sample on the left side after the first 4800 frames.
fn peak(signal: &[f32]) -> f32 {
    signal.as_chunks::<2>().0.iter().skip(4_800).fold(0.0, |m, [l, _]| m.max(l.abs()))
}

fn db(gain: f64) -> f64 {
    20.0 * gain.log10()
}

suhina_device_api::conformance_test!(Saturator::new());

#[test]
fn fully_dry_is_the_input_delayed_exactly() {
    let input = test_signal(4_096);
    let mut output = input.clone();
    saturator(&[(param::MIX, 0.0), (param::DRIVE, 36.0)]).process(&ProcessContext::default(), &mut output);
    let delay = DELAY * 2;
    assert_eq!(output[delay..], input[..input.len() - delay]);
    assert_eq!(Saturator::new().latency(), LATENCY);
}

#[test]
fn small_signal_gain_of_each_curve() {
    // Measured from the device the saturator imports from: the slope of each curve at zero.
    for (shape, expected_db) in [(0, 0.0), (1, 3.92), (2, 0.0), (3, 6.02), (4, 16.5), (5, 3.92), (6, 0.0), (7, 0.0)] {
        let s = drive_sine(&mut saturator(&[(param::TYPE, f64::from(shape))]), 1_000.0, 0.001, 24_000);
        let gain = db(level(&s, 1_000.0) / 0.001);
        assert!((gain - expected_db).abs() < 0.2, "{}: {gain}", TYPE_NAMES[shape as usize]);
    }
}

#[test]
fn drive_is_a_gain_in_decibels_before_the_curve() {
    for drive in [-24.0, -12.0, 12.0, 30.0] {
        let s = drive_sine(&mut saturator(&[(param::DRIVE, drive)]), 1_000.0, 0.001, 24_000);
        assert!((db(level(&s, 1_000.0) / 0.001) - drive).abs() < 0.05, "{drive}");
    }
    // Drive +36 at -36 dBFS is the same point of the curve as Drive 0 at 0 dBFS.
    let a = level(&drive_sine(&mut saturator(&[(param::DRIVE, 36.0)]), 200.0, 0.015_85, 24_000), 200.0);
    let b = level(&drive_sine(&mut saturator(&[]), 200.0, 1.0, 24_000), 200.0);
    assert!((db(a / 0.015_85) - 36.0 - db(b)).abs() < 0.05, "{a} {b}");
}

#[test]
fn analog_clip_bends_from_minus_3_db_to_full_scale() {
    // Below the knee the curve is a straight line: no third harmonic.
    let clean = drive_sine(&mut saturator(&[]), 200.0, 0.7, 24_000);
    assert!(level(&clean, 600.0) < 1e-4, "{}", level(&clean, 600.0));
    // At full scale the peak is 0.893 (0.901 measured), and it never passes 1 by much however
    // hard it is driven (the oversampling filter rings a little).
    let full = drive_sine(&mut saturator(&[]), 100.0, 1.0, 24_000);
    assert!((peak(&full) - 0.893).abs() < 0.01, "{}", peak(&full));
    let hard = drive_sine(&mut saturator(&[(param::DRIVE, 36.0)]), 100.0, 1.0, 24_000);
    assert!(peak(&hard) < 1.15, "{}", peak(&hard));
}

#[test]
fn sinoid_fold_folds_and_soft_sine_holds() {
    // Driven to an amplitude of 2 the fold is back at zero at the sine's peak, so less of the
    // fundamental comes out than at 1; the soft sine holds at 1.
    let fold = |drive| {
        level(&drive_sine(&mut saturator(&[(param::TYPE, 5.0), (param::DRIVE, drive)]), 100.0, 0.5, 24_000), 100.0)
    };
    assert!(fold(12.04) < 0.6 * fold(6.02), "{} {}", fold(6.02), fold(12.04));
    let held = drive_sine(&mut saturator(&[(param::TYPE, 1.0), (param::DRIVE, 12.04)]), 100.0, 0.5, 24_000);
    assert!(level(&held, 100.0) > 1.0, "{}", level(&held, 100.0));
}

#[test]
fn bass_threshold_moves_the_knee() {
    // At 0 dB the bass shaper is a hard clip, clean at -1 dBFS; at -12 dB it is clean below
    // -12 dBFS; at -50 dB it bends from the start.
    let third = |threshold, amplitude| {
        let settings = [(param::TYPE, 2.0), (param::THRESHOLD, threshold)];
        let s = drive_sine(&mut saturator(&settings), 200.0, amplitude, 24_000);
        level(&s, 600.0) / level(&s, 200.0)
    };
    assert!(third(0.0, 0.89) < 1e-4, "{}", third(0.0, 0.89));
    assert!(third(-12.0, 0.2) < 1e-4, "{}", third(-12.0, 0.2));
    assert!(third(-50.0, 0.2) > 0.01, "{}", third(-50.0, 0.2));
}

#[test]
fn color_changes_the_saturation_not_the_tone() {
    // While the curve stays straight the Color filters and their inverse cancel exactly.
    let color = [(param::DRIVE, -24.0), (param::COLOR_LOW, 1.0), (param::COLOR_HIGH, 1.0)];
    for hz in [40.0, 200.0, 1_000.0, 5_000.0, 15_000.0] {
        let s = drive_sine(&mut saturator(&color), hz, 0.1, 24_000);
        let gain = db(level(&s, hz) / 0.1);
        assert!((gain + 24.0).abs() < 0.01, "{hz}: {gain}");
    }
    // Into a hard clip, Color Low +100 % drives the bass 36 dB harder, so less of it comes back;
    // the treble is untouched.
    let clip = |low, hz| {
        let settings = [(param::TYPE, 6.0), (param::DRIVE, 12.0), (param::COLOR_LOW, low)];
        level(&drive_sine(&mut saturator(&settings), hz, 0.25, 24_000), hz)
    };
    assert!(clip(1.0, 60.0) < 0.5 * clip(0.0, 60.0), "{} {}", clip(1.0, 60.0), clip(0.0, 60.0));
    assert!((db(clip(1.0, 15_000.0) / clip(0.0, 15_000.0))).abs() < 0.5);
    // Color off ignores the amounts.
    let off = [(param::TYPE, 6.0), (param::DRIVE, 12.0), (param::COLOR_LOW, 1.0), (param::COLOR, 0.0)];
    let s = level(&drive_sine(&mut saturator(&off), 60.0, 0.25, 24_000), 60.0);
    assert!((s - clip(0.0, 60.0)).abs() < 1e-6);
}

#[test]
fn color_high_is_a_bell_at_its_frequency() {
    let clipped = |frequency, hz| {
        let settings =
            [(param::TYPE, 6.0), (param::DRIVE, 12.0), (param::COLOR_HIGH, 1.0), (param::COLOR_FREQUENCY, frequency)];
        level(&drive_sine(&mut saturator(&settings), hz, 0.25, 24_000), hz)
    };
    // The band at the bell's centre is clipped harder than one three octaves away.
    assert!(clipped(1_000.0, 1_000.0) < 0.5 * clipped(1_000.0, 125.0));
    assert!(clipped(125.0, 125.0) < 0.5 * clipped(125.0, 1_000.0));
}

#[test]
fn post_clip_holds_the_output_level() {
    // A steep waveshaper goes past full scale; the hard post clip stops it at the output level.
    let steep = [(param::TYPE, 7.0), (param::SHAPER_LINEAR, 1.0), (param::SHAPER_CURVE, 0.0), (param::DRIVE, 6.0)];
    let free = drive_sine(&mut saturator(&steep), 100.0, 1.0, 24_000);
    assert!(peak(&free) > 2.0, "{}", peak(&free));
    for (mode, ceiling) in [(1.0, 1.0), (2.0, 1.0)] {
        let mut settings = steep.to_vec();
        settings.extend([(param::POST_CLIP, mode), (param::OUTPUT, -6.0)]);
        let s = drive_sine(&mut saturator(&settings), 100.0, 1.0, 24_000);
        let limit = ceiling * db_to_gain(-6.0);
        assert!(f64::from(peak(&s)) < f64::from(limit) * 1.1, "{mode}: {}", peak(&s));
    }
}

#[test]
fn output_and_an_equal_gain_mix() {
    let full = level(&drive_sine(&mut saturator(&[(param::DRIVE, 18.0)]), 200.0, 0.3, 24_000), 200.0);
    let quieter =
        level(&drive_sine(&mut saturator(&[(param::DRIVE, 18.0), (param::OUTPUT, -12.0)]), 200.0, 0.3, 24_000), 200.0);
    assert!((db(quieter / full) + 12.0).abs() < 0.01);
    // Half wet is exactly half the input (delayed) plus half the wet render.
    let input = test_signal(4_096);
    let render = |mix| {
        let mut s = input.clone();
        saturator(&[(param::DRIVE, 18.0), (param::MIX, mix)]).process(&ProcessContext::default(), &mut s);
        s
    };
    let (dry, wet, half) = (render(0.0), render(1.0), render(0.5));
    for ((d, w), h) in dry.iter().zip(&wet).zip(&half) {
        assert!((0.5 * (d + w) - h).abs() < 1e-6);
    }
}

#[test]
fn dc_filter_removes_an_offset() {
    let offset = |on| {
        let mut s: Vec<f32> = vec![0.3; 96_000];
        saturator(&[(param::DC_FILTER, on)]).process(&ProcessContext::default(), &mut s);
        s.last().copied().unwrap_or(0.0)
    };
    assert!(offset(1.0).abs() < 0.01, "{}", offset(1.0));
    assert!((offset(0.0) - 0.3).abs() < 1e-3, "{}", offset(0.0));
}

/// The largest step between two frames on the left side in `frames`.
fn largest_step(frames: &[[f32; 2]]) -> f32 {
    frames.windows(2).fold(0.0f32, |m, w| m.max((w[1][0] - w[0][0]).abs()))
}

/// Settings to start from, then the parameter that changes and its new value.
type Change = (&'static [(usize, f64)], usize, f64);

#[test]
fn changes_while_playing_do_not_click() {
    // Each change ramps: while it moves, no step between frames is larger than the largest
    // step of the steady sound before it or after it.
    let changes: [Change; 11] = [
        (&[], param::TYPE, 6.0),
        (&[], param::TYPE, 5.0),
        (&[], param::DRIVE, 12.0),
        (&[(param::TYPE, 6.0), (param::DRIVE, 12.0)], param::COLOR_LOW, 1.0),
        (&[(param::TYPE, 6.0), (param::DRIVE, 12.0)], param::COLOR_HIGH, -1.0),
        (&[(param::TYPE, 6.0), (param::DRIVE, 12.0), (param::COLOR_HIGH, 1.0)], param::COLOR_FREQUENCY, 100.0),
        (&[(param::TYPE, 6.0), (param::DRIVE, 12.0), (param::COLOR_LOW, 1.0)], param::COLOR, 0.0),
        (&[(param::DRIVE, 6.0)], param::OUTPUT, -12.0),
        (&[(param::TYPE, 7.0)], param::SHAPER_DAMP, 1.0),
        (&[(param::DRIVE, 12.0)], param::MIX, 0.0),
        (&[], param::DC_FILTER, 1.0),
    ];
    for (settings, index, value) in changes {
        let mut s = saturator(settings);
        let mut signal = sine(100.0, 0.25, 48_000, SR);
        let (before, after) = signal.split_at_mut(2 * 24_000);
        s.process(&ProcessContext::default(), before);
        s.set_param(index, value);
        s.process(&ProcessContext::default(), after);
        let frames = signal.as_chunks::<2>().0;
        let steady = largest_step(&frames[12_000..24_000]).max(largest_step(&frames[36_000..]));
        let moving = largest_step(&frames[23_999..26_000]);
        assert!(moving <= 1.05 * steady + 1e-4, "{index} to {value}: {moving} against {steady}");
    }
}

#[test]
fn the_same_sound_at_44_1_and_96_khz() {
    // One second, read over its second half: 100 whole cycles of 200 Hz at either rate.
    let gain = |sample_rate: f64, settings: &[(usize, f64)]| {
        let mut s = saturator(settings);
        let _ = s.prepare(sample_rate);
        let mut signal = sine(200.0, 0.5, sample_rate as usize, sample_rate);
        s.process(&ProcessContext::default(), &mut signal);
        let half = signal.get(signal.len() / 2 - 9_600..).unwrap_or_default();
        (level_at(half, 200.0, sample_rate), level_at(half, 600.0, sample_rate))
    };
    for settings in [&[(param::DRIVE, 12.0)][..], &[(param::TYPE, 6.0), (param::DRIVE, 6.0), (param::COLOR_HIGH, 1.0)]]
    {
        let (a, b) = (gain(44_100.0, settings), gain(96_000.0, settings));
        assert!(db(a.0 / b.0).abs() < 0.05 && db(a.1 / b.1).abs() < 0.2, "{a:?} {b:?}");
    }
}

#[test]
fn silence_stays_silent() {
    let mut s = saturator(&[(param::DRIVE, 36.0), (param::COLOR_LOW, 1.0), (param::TYPE, 7.0)]);
    let mut signal = vec![0.0f32; 96_000];
    s.process(&ProcessContext::default(), &mut signal);
    assert!(signal.iter().all(|v| *v == 0.0));
}

/// One instance at its defaults, then with the heaviest curve and Color on, 60 s at 48 kHz each:
/// `cargo test --release -p suhina-device-saturator cost -- --ignored --nocapture`.
#[test]
#[ignore = "a timing measurement; run it on its own in release"]
fn cost() {
    let heavy = [(param::TYPE, 4.0), (param::COLOR_LOW, 0.5), (param::COLOR_HIGH, 0.5), (param::POST_CLIP, 1.0)];
    for (what, settings) in [("defaults", &[][..]), ("Hard Curve, Color, soft post clip", &heavy[..])] {
        let mut s = saturator(settings);
        let mut block = test_signal(512);
        let start = std::time::Instant::now();
        for _ in 0..(60 * 48_000 / 256) {
            s.process(&ProcessContext::default(), &mut block);
        }
        let seconds = start.elapsed().as_secs_f64();
        println!("{what}: 60 s rendered in {seconds:.3} s, {:.2} % of one core", seconds / 60.0 * 100.0);
    }
}
