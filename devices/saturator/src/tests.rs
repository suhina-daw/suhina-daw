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
    for (shape, expected_db) in [(0, 0.0), (1, 3.92), (2, 0.0), (3, 6.02), (4, 16.5), (5, 3.92), (6, 0.0), (7, -6.02)] {
        let s = drive_sine(&mut saturator(&[(param::TYPE, f64::from(shape))]), 1_000.0, 0.001, 24_000);
        let gain = db(level(&s, 1_000.0) / 0.001);
        assert!((gain - expected_db).abs() < 0.2, "{}: {gain}", TYPE_NAMES[shape as usize]);
    }
}

#[test]
fn waveshaper_linear_is_the_gain_of_the_straight_path() {
    // Measured: 0 % is silent, 50 % is -6.0 dB, 100 % is unity, at every level.
    for (linear, expected_db) in [(0.5, -6.02), (1.0, 0.0)] {
        for amplitude in [0.001, 0.5] {
            let settings = [(param::TYPE, 7.0), (param::SHAPER_CURVE, 0.0), (param::SHAPER_LINEAR, linear)];
            let s = drive_sine(&mut saturator(&settings), 1_000.0, amplitude, 24_000);
            let gain = db(level(&s, 1_000.0) / amplitude);
            assert!((gain - expected_db).abs() < 0.1, "{linear} at {amplitude}: {gain}");
        }
    }
    let silent = [(param::TYPE, 7.0), (param::SHAPER_CURVE, 0.0), (param::SHAPER_LINEAR, 0.0)];
    assert!(peak(&drive_sine(&mut saturator(&silent), 1_000.0, 0.5, 24_000)) < 1e-6);
}

#[test]
fn waveshaper_curve_adds_even_harmonics_without_an_offset() {
    // Measured at Curve 100 %: the second harmonic is 2π · A of the fundamental for a small tone
    // (+3.9 dB at -12 dBFS), the odd ones are absent, and the output has no DC.
    let settings = [(param::TYPE, 7.0), (param::SHAPER_CURVE, 1.0)];
    let s = drive_sine(&mut saturator(&settings), 1_000.0, 0.0316, 24_000);
    let ratio = level(&s, 2_000.0) / level(&s, 1_000.0);
    assert!((ratio - std::f64::consts::TAU * 0.0316).abs() < 0.02, "{ratio}");
    assert!(level(&s, 3_000.0) < 1e-4 * level(&s, 1_000.0), "{}", level(&s, 3_000.0));
    let tail = &s[s.len() - 4_800..];
    let mean = tail.iter().map(|v| f64::from(*v)).sum::<f64>() / tail.len() as f64;
    assert!(mean.abs() < 1e-3, "{mean}");
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
fn folds_back_as_little_as_the_original_with_hi_quality() {
    // The original's Hi-Quality, on by default, runs the curves at four times the rate. Its
    // Digital Clip at Drive +24 dB, a sine at -6 dBFS and 48 kHz, folds the harmonics back to
    // these lines (dB under the fundamental).
    let measured: [(f64, &[(f64, f64)]); 2] =
        [(10_000.0, &[(22_000.0, -33.2), (2_000.0, -37.1), (18_000.0, -41.6)]), (15_000.0, &[(3_000.0, -26.8)])];
    let rate = 48_000.0;
    for (hz, lines) in measured {
        let mut s = saturator(&[(param::TYPE, 6.0), (param::DRIVE, 24.0)]);
        let _ = s.prepare(rate);
        // One second after the first 4800 frames: whole cycles of every multiple of 100 Hz.
        let mut signal = sine(hz, 0.501, 52_800, rate);
        s.process(&ProcessContext::default(), &mut signal);
        let fundamental = level_at(&signal, hz, rate);
        let under = |line| db(level_at(&signal, line, rate) / fundamental);
        for &(line, expected) in lines {
            assert!((under(line) - expected).abs() < 0.5, "{hz} Hz: {line} Hz at {} dB", under(line));
        }
        let loudest = (1..240).map(|k| f64::from(k) * 100.0).filter(|&f| f != hz).map(under).fold(f64::MIN, f64::max);
        assert!((loudest - lines[0].1).abs() < 0.5, "{hz} Hz: {loudest} dB");
    }
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
    let steep = [(param::TYPE, 7.0), (param::SHAPER_LINEAR, 1.0), (param::SHAPER_CURVE, 0.0), (param::DRIVE, 12.0)];
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

#[test]
fn dc_filter_is_two_one_pole_high_passes_at_27_hz() {
    // Measured: magnitude (f / 27)² / (1 + (f / 27)²), 0.002, 0.034 and 0.341 at 1, 5 and 20 Hz.
    for (hz, expected) in [(5.0, 0.033), (20.0, 0.354), (60.0, 0.832)] {
        let on = drive_sine(&mut saturator(&[(param::DC_FILTER, 1.0)]), hz, 0.1, 48_000);
        let gain = level(&on, hz) / 0.1;
        assert!((gain - expected).abs() < 0.03 * expected.max(0.1), "{hz} Hz: {gain}");
    }
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
    // step of the steady sound before it or after it. The tone is 97 Hz, not 100: at 48 kHz a
    // 100 Hz cycle is a whole 480 frames, so a clip's edges land on the same fraction of a frame
    // every cycle, and the steady sound shows only the smallest of the steps an edge can make
    // (0.289 where other fractions reach 0.332); edges that drift during a change then read as
    // a click.
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
        let mut signal = sine(97.0, 0.25, 48_000, SR);
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
fn a_color_glide_leaves_a_straight_curve_alone() {
    // Drive -36 dB keeps the curve straight even with Color Low at +100 % (+36 dB), so the
    // filters and their inverses cancel as they glide: the output stays the input times Drive,
    // with no step between frames larger than the steady sound's. Retuning that clicked would
    // show here, where nothing else moves.
    for (index, value) in [(param::COLOR_LOW, 1.0), (param::COLOR_HIGH, 1.0), (param::COLOR_FREQUENCY, 100.0)] {
        let mut s = saturator(&[(param::TYPE, 6.0), (param::DRIVE, -36.0), (param::COLOR_HIGH, 0.5)]);
        let mut signal = sine(100.0, 0.8, 48_000, SR);
        let (before, after) = signal.split_at_mut(2 * 24_000);
        s.process(&ProcessContext::default(), before);
        s.set_param(index, value);
        s.process(&ProcessContext::default(), after);
        let frames = signal.as_chunks::<2>().0;
        let steady = largest_step(&frames[12_000..24_000]).max(largest_step(&frames[36_000..]));
        let moving = largest_step(&frames[23_999..26_000]);
        assert!(moving <= 1.05 * steady, "{index} to {value}: {moving} against {steady}");
        let gain = 0.8 * f64::from(db_to_gain(-36.0f32));
        let worst = (12_000..48_000)
            .map(|n| {
                (f64::from(frames[n][0]) - gain * (std::f64::consts::TAU * 100.0 * (n - DELAY) as f64 / SR).sin()).abs()
            })
            .fold(0.0, f64::max);
        assert!(worst < 0.05 * gain, "{index} to {value}: {worst} against {gain}");
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

/// The same input processed in one call and in calls of mixed sizes comes out the same: the
/// device keeps all its state across `process` calls, whatever size they are.
#[test]
fn block_sizes_do_not_change_the_sound() {
    let settings = [(param::DRIVE, 12.0), (param::COLOR_LOW, 0.5), (param::COLOR_HIGH, 0.5), (param::TYPE, 4.0)];
    let input = test_signal(96_000);
    let mut whole = input.clone();
    saturator(&settings).process(&ProcessContext::default(), &mut whole);
    let mut chunked = input.clone();
    let mut device = saturator(&settings);
    let sizes = [2usize, 64, 258, 1_024, 6, 512, 1_332, 2_048, 4];
    let (mut at, mut i) = (0, 0);
    while at < chunked.len() {
        let size = sizes[i % sizes.len()].min(chunked.len() - at) & !1;
        let size = size.max(2);
        device.process(&ProcessContext::default(), &mut chunked[at..at + size]);
        at += size;
        i += 1;
    }
    assert_eq!(whole, chunked);
}

/// Two curve changes back to back, the second while the first's fade still runs: the first
/// morph completes and the second fades from it — no step harder than the sounds themselves.
#[test]
fn two_quick_curve_changes_morph_instead_of_jumping() {
    // The largest step of a settled curve's sound: what the morph may not exceed.
    let settled_step = |shape: f64| {
        let mut s = saturator(&[(param::TYPE, shape)]);
        let mut signal = sine(100.0, 0.25, 48_000, SR);
        s.process(&ProcessContext::default(), &mut signal);
        largest_step(&signal.as_chunks::<2>().0[4_800..])
    };
    for (first, second) in [(4.0, 6.0), (5.0, 6.0), (6.0, 4.0), (1.0, 7.0)] {
        let mut s = saturator(&[]);
        let mut signal = sine(100.0, 0.25, 48_000, SR);
        s.process(&ProcessContext::default(), &mut signal[..2 * 24_000]);
        s.set_param(param::TYPE, first);
        s.process(&ProcessContext::default(), &mut signal[2 * 24_000..2 * 24_064]);
        s.set_param(param::TYPE, second);
        s.process(&ProcessContext::default(), &mut signal[2 * 24_064..]);
        let frames = signal.as_chunks::<2>().0;
        let steady = largest_step(&frames[12_000..24_000]);
        let allowed = steady.max(settled_step(first)).max(settled_step(second));
        let moving = largest_step(&frames[24_000..26_000]);
        assert!(moving <= 1.05 * allowed + 1e-4, "{first} then {second}: {moving} against {allowed}");
    }
}

/// A curve change, then a post clip change after that fade has finished: the crossfade runs from
/// the curve that is playing, so the curve it faded from does not burst back.
#[test]
fn a_post_clip_change_after_a_curve_change_morphs_from_the_curve_playing() {
    let mut s = saturator(&[]);
    let mut signal = sine(100.0, 0.25, 48_000, SR);
    s.process(&ProcessContext::default(), &mut signal[..2 * 24_000]);
    s.set_param(param::TYPE, 4.0);
    // The 20 ms fade completes inside this call; the curve it faded from is not reset.
    s.process(&ProcessContext::default(), &mut signal[2 * 24_000..2 * 25_000]);
    s.set_param(param::POST_CLIP, 2.0);
    s.process(&ProcessContext::default(), &mut signal[2 * 25_000..]);
    let frames = signal.as_chunks::<2>().0;
    let allowed = largest_step(&frames[12_000..24_000]).max(largest_step(&frames[27_000..]));
    let moving = largest_step(&frames[24_900..26_200]);
    assert!(moving <= 1.05 * allowed + 1e-4, "{moving} against {allowed}");
}

/// The post clip switch on a signal that goes past the ceiling: it fades like a curve change.
#[test]
fn the_post_clip_switch_fades() {
    let steep = [(param::TYPE, 7.0), (param::SHAPER_LINEAR, 1.0), (param::DRIVE, 6.0)];
    let mut s = saturator(&steep);
    let mut signal = sine(100.0, 1.0, 48_000, SR);
    s.process(&ProcessContext::default(), &mut signal[..2 * 24_000]);
    s.set_param(param::POST_CLIP, 2.0);
    s.process(&ProcessContext::default(), &mut signal[2 * 24_000..]);
    let frames = signal.as_chunks::<2>().0;
    let steady = largest_step(&frames[12_000..24_000]).max(largest_step(&frames[36_000..]));
    let moving = largest_step(&frames[24_000..25_000]);
    assert!(moving <= 1.05 * steady + 1e-4, "{moving} against {steady}");
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
