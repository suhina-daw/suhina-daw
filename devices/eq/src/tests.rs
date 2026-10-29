// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_device_api::conformance::test_signal;

use super::*;

const SR: f64 = 44_100.0;

/// Band `band` (from 1) of curve A: (field, value) pairs.
fn a(band: usize, field: usize, value: f64) -> (usize, f64) {
    (param::band(0, band - 1, field), value)
}

/// Band `band` (from 1) of curve B.
fn b(band: usize, field: usize, value: f64) -> (usize, f64) {
    (param::band(1, band - 1, field), value)
}

/// Every band of both curves off.
fn all_off() -> Vec<(usize, f64)> {
    (1..=BANDS).flat_map(|n| [a(n, param::ON, 0.0), b(n, param::ON, 0.0)]).collect()
}

/// Only band 3 of curve A on, as the measurements were made: `kind` at 1 kHz, Q and gain.
fn single(kind: usize, q: f64, gain: f64) -> Vec<(usize, f64)> {
    let mut s = all_off();
    s.extend([
        a(3, param::ON, 1.0),
        a(3, param::TYPE, kind as f64),
        a(3, param::FREQUENCY, 1_000.0),
        a(3, param::Q, q),
        a(3, param::GAIN, gain),
        (param::ADAPTIVE_Q, 0.0),
    ]);
    s
}

fn eq(settings: &[(usize, f64)]) -> Eq {
    let mut e = Eq::new();
    for &(index, value) in settings {
        e.set_param(index, value);
    }
    let _ = e.prepare(SR);
    e
}

/// A sine of `hz` at `left` on the left and `right` on the right, `frames` long.
fn sine(hz: f64, left: f64, right: f64, frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|n| {
            let v = (std::f64::consts::TAU * hz * n as f64 / SR).sin();
            [(v * left) as f32, (v * right) as f32]
        })
        .collect()
}

/// Amplitude of `hz` on `side` over the second half of `signal`.
fn level(signal: &[f32], hz: f64, side: usize) -> f64 {
    let frames = signal.as_chunks::<2>().0;
    let half = frames.get(frames.len() / 2..).unwrap_or_default();
    let (mut re, mut im) = (0.0, 0.0);
    for (n, frame) in half.iter().enumerate() {
        let w = std::f64::consts::TAU * hz * n as f64 / SR;
        let v = f64::from(frame[side]);
        re += v * w.cos();
        im += v * w.sin();
    }
    2.0 * re.hypot(im) / half.len().max(1) as f64
}

/// The gain of `e` at `hz` on the left, in dB, from a steady sine.
fn gain_db(e: &mut Eq, hz: f64) -> f64 {
    let mut s = sine(hz, 0.1, 0.1, 44_100);
    e.process(&ProcessContext::default(), &mut s);
    20.0 * (level(&s, hz, 0) / 0.1).log10()
}

suhina_device_api::conformance_test!(Eq::new());

#[test]
fn every_type_has_the_measured_response() {
    // At 1 kHz, Q 0.71, +12 dB where the type has gain: the measured table (dB) at 250 Hz,
    // 500 Hz, 707 Hz, 1 kHz, 1.41 kHz, 2 kHz and 4 kHz. The notch's centre is left out (a bin).
    let hz = [250.0, 500.0, 707.0, 1_000.0, 1_414.0, 2_000.0, 4_000.0];
    let measured: [(usize, [f64; 7]); 8] = [
        (0, [-96.4, -48.3, -24.2, -3.01, -0.02, 0.0, 0.0]),
        (1, [-24.1, -12.3, -7.0, -3.01, -0.96, -0.26, -0.02]),
        (2, [11.9, 11.1, 9.3, 6.0, 2.7, 0.9, 0.06]),
        (3, [1.8, 5.7, 9.3, 12.0, 9.3, 5.7, 1.7]),
        (4, [-0.58, -2.75, -6.97, f64::NAN, -6.96, -2.73, -0.55]),
        (5, [0.06, 0.9, 2.7, 6.0, 9.3, 11.1, 11.9]),
        (6, [-0.02, -0.26, -0.97, -3.01, -7.0, -12.4, -24.6]),
        (7, [0.0, 0.0, -0.02, -3.01, -24.2, -48.5, -98.1]),
    ];
    for (kind, row) in measured {
        for (f, expected) in hz.iter().zip(row) {
            if expected.is_nan() {
                continue;
            }
            let got = gain_db(&mut eq(&single(kind, DEFAULT_Q, 12.0)), *f);
            // The table's own rounding, and a little more far down the steep cuts.
            let tolerance = if expected < -40.0 { 0.6 } else { 0.1 };
            assert!((got - expected).abs() < tolerance, "{} at {f} Hz: {got} against {expected}", TYPE_NAMES[kind]);
        }
    }
}

#[test]
fn shelf_q_has_the_measured_shape() {
    // 1 kHz, +12 dB: the measured response (dB) at 508 Hz, 640 Hz, 806 Hz, 1.016 kHz, 1.28 kHz and
    // 1.613 kHz, for a low shelf at Q 6 and high shelves at Q 0.1, 2 and 18. A cookbook shelf
    // with the Q itself is up to 11 dB off these.
    let hz = [508.0, 640.0, 806.3, 1_015.9, 1_280.0, 1_612.7];
    let measured: [(usize, f64, [f64; 6]); 4] = [
        (2, 6.0, [14.46, 14.6, 11.61, 5.66, -0.17, -2.71]),
        (5, 0.1, [4.32, 4.94, 5.51, 6.04, 6.57, 7.14]),
        (5, 2.0, [-1.03, -0.32, 2.23, 6.34, 10.3, 12.52]),
        (5, 18.0, [-3.31, -4.43, -0.92, 6.7, 13.96, 16.4]),
    ];
    for (kind, q, row) in measured {
        for (f, expected) in hz.iter().zip(row) {
            let got = gain_db(&mut eq(&single(kind, q, 12.0)), *f);
            assert!((got - expected).abs() < 0.35, "{} at Q {q}, {f} Hz: {got} against {expected}", TYPE_NAMES[kind]);
        }
    }
}

#[test]
fn cut_q_raises_a_peak_at_the_corner() {
    // Measured: the 12 dB low-pass peaks at about 20 log10 Q at the corner; the 48 dB one at
    // +6.9 dB near 901 Hz for Q 2 and +13.5 dB near 941 Hz for Q 6.
    for (q, at_corner) in [(2.0, 6.0), (6.0, 15.6), (18.0, 25.1)] {
        let got = gain_db(&mut eq(&single(6, q, 0.0)), 1_000.0);
        assert!((got - at_corner).abs() < 0.15, "Q {q}: {got}");
    }
    for (q, peak_hz, peak_db) in [(2.0, 901.0, 6.9), (6.0, 941.0, 13.5)] {
        let got = gain_db(&mut eq(&single(7, q, 0.0)), peak_hz);
        assert!((got - peak_db).abs() < 0.3, "48 dB at Q {q}: {got}");
    }
}

#[test]
fn adaptive_q_narrows_big_bells_and_leaves_shelves() {
    // A bell's half-gain point (500 Hz side) moves in at +12 dB and out at +3 dB, as measured:
    // at +12 dB the half-gain points are 706 Hz and 1.42 kHz with Adaptive Q.
    let with = |kind, gain| {
        let mut s = single(kind, DEFAULT_Q, gain);
        s.push((param::ADAPTIVE_Q, 1.0));
        s
    };
    let at_706 = gain_db(&mut eq(&with(3, 12.0)), 706.0);
    assert!((at_706 - 6.0).abs() < 0.1, "{at_706}");
    let wide = gain_db(&mut eq(&with(3, 3.0)), 418.0);
    assert!((wide - 1.5).abs() < 0.1, "{wide}");
    for hz in [500.0, 1_000.0, 2_000.0] {
        let on = gain_db(&mut eq(&with(5, 12.0)), hz);
        let off = gain_db(&mut eq(&single(5, DEFAULT_Q, 12.0)), hz);
        assert!((on - off).abs() < 1e-9, "{hz}");
    }
}

#[test]
fn scale_multiplies_the_gains() {
    let scaled = |scale| {
        let mut s = single(3, 2.0, 12.0);
        s.push((param::SCALE, scale));
        gain_db(&mut eq(&s), 1_000.0)
    };
    assert!((scaled(0.5) - 6.0).abs() < 0.05);
    assert!(scaled(0.0).abs() < 0.05);
    assert!((scaled(-1.0) + 12.0).abs() < 0.05);
    assert!((scaled(2.0) - 24.0).abs() < 0.05);
    // A cut has no gain to scale.
    let mut cut = single(1, DEFAULT_Q, 0.0);
    cut.push((param::SCALE, 0.0));
    assert!((gain_db(&mut eq(&cut), 1_000.0) + 3.01).abs() < 0.05);
}

#[test]
fn a_band_switched_on_after_scale_moved_plays_at_the_new_scale() {
    // Band 3 is off while Scale moves and settles, so nothing retunes it; switching it on must
    // tune it with the settled Scale before it fades in.
    let mut off = all_off();
    off.extend([
        a(3, param::TYPE, 3.0),
        a(3, param::FREQUENCY, 1_000.0),
        a(3, param::Q, 0.71),
        a(3, param::GAIN, 12.0),
    ]);
    let mut e = eq(&off);
    e.set_param(param::SCALE, 0.0);
    let mut settle = vec![[0.0f32; 2]; 4_096].concat();
    e.process(&ProcessContext::default(), &mut settle);
    e.set_param(a(3, param::ON, 1.0).0, 1.0);
    assert!(gain_db(&mut e, 1_000.0).abs() < 0.05, "the woken band plays at the Scale it now has");
}

#[test]
fn defaults_and_bands_off_leave_the_sound_alone() {
    // At its defaults every band that is on has no gain, and Output is 0 dB.
    let input = test_signal(4_096);
    for settings in [vec![], all_off()] {
        let mut output = input.clone();
        eq(&settings).process(&ProcessContext::default(), &mut output);
        let worst = input.iter().zip(&output).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1e-5, "{worst}");
    }
    let mut quieter = all_off();
    quieter.push((param::OUTPUT, -12.0));
    assert!((gain_db(&mut eq(&quieter), 1_000.0) + 12.0).abs() < 0.01);
}

#[test]
fn left_right_and_mid_side_use_both_curves() {
    // Curve B's bell (+12 dB at 1 kHz) is heard on the right in Left/Right, not in Stereo.
    let mut s = all_off();
    s.extend([
        b(3, param::ON, 1.0),
        b(3, param::FREQUENCY, 1_000.0),
        b(3, param::GAIN, 12.0),
        (param::ADAPTIVE_Q, 0.0),
    ]);
    let run = |settings: &[(usize, f64)], left, right| {
        let mut signal = sine(1_000.0, left, right, 44_100);
        eq(settings).process(&ProcessContext::default(), &mut signal);
        (level(&signal, 1_000.0, 0), level(&signal, 1_000.0, 1))
    };
    let (l, r) = run(&s, 0.1, 0.1);
    assert!((l - 0.1).abs() < 1e-4 && (r - 0.1).abs() < 1e-4, "{l} {r}");
    let mut lr = s.clone();
    lr.push((param::MODE, 1.0));
    let (l, r) = run(&lr, 0.1, 0.1);
    assert!((l - 0.1).abs() < 1e-4 && (r / 0.1 - 3.981).abs() < 0.01, "{l} {r}");
    // In Mid/Side curve B is the side: a centred sound has none and passes unchanged; a sound
    // on one side only is half side, so a 1 kHz boost there spreads it.
    let mut ms = s.clone();
    ms.push((param::MODE, 2.0));
    let (l, r) = run(&ms, 0.1, 0.1);
    assert!((l - 0.1).abs() < 1e-4 && (r - 0.1).abs() < 1e-4, "{l} {r}");
    let (l, r) = run(&ms, 0.1, 0.0);
    // Mid 0.05, side 0.05 × 3.98: left 0.249, right 0.149 (out of phase).
    assert!((l - 0.249).abs() < 0.002 && (r - 0.149).abs() < 0.002, "{l} {r}");
}

/// The largest step between two frames on the left side in `frames`.
fn largest_step(frames: &[[f32; 2]]) -> f32 {
    frames.windows(2).fold(0.0f32, |m, w| m.max((w[1][0] - w[0][0]).abs()))
}

#[test]
fn changes_while_playing_do_not_click() {
    // A tone through a narrow +15 dB bell; each change ramps or fades, so no step between frames
    // is much larger than in the steady sound before or after it. A bell whose Q falls from 6
    // releases the energy its resonance held, a smooth swell of up to half again the steady
    // level while it glides (the same when retuned every frame), not a click.
    let base = single(3, 6.0, 15.0);
    let changes = [
        (param::band(0, 2, param::FREQUENCY), 3_000.0),
        (param::band(0, 2, param::GAIN), -15.0),
        (param::band(0, 2, param::Q), 0.3),
        (param::band(0, 2, param::ON), 0.0),
        (param::band(0, 2, param::TYPE), 7.0),
        (param::band(0, 2, param::TYPE), 2.0),
        (param::SCALE, -1.0),
        (param::OUTPUT, -12.0),
        (param::MODE, 2.0),
        (param::ADAPTIVE_Q, 1.0),
    ];
    for (index, value) in changes {
        let mut e = eq(&base);
        let mut signal = sine(1_000.0, 0.05, 0.03, 44_100);
        let (before, after) = signal.split_at_mut(2 * 22_050);
        e.process(&ProcessContext::default(), before);
        e.set_param(index, value);
        e.process(&ProcessContext::default(), after);
        let frames = signal.as_chunks::<2>().0;
        let steady = largest_step(&frames[11_000..22_050]).max(largest_step(&frames[33_000..]));
        let moving = largest_step(&frames[22_049..24_000]);
        assert!(moving <= 1.5 * steady, "{index} to {value}: {moving} against {steady}");
    }
}

#[test]
fn the_same_response_at_any_rate_below_the_top() {
    // The bilinear transform bends responses near Nyquist (as measured at 44.1 kHz), but at 1 kHz
    // a bell is the same at every rate.
    for rate in [22_050.0, 48_000.0, 96_000.0, 192_000.0] {
        let mut e = eq(&single(3, 2.0, 9.0));
        let _ = e.prepare(rate);
        let mut s: Vec<f32> = (0..rate as usize)
            .flat_map(|n| {
                let v = ((std::f64::consts::TAU * 1_000.0 * n as f64 / rate).sin() * 0.1) as f32;
                [v, v]
            })
            .collect();
        e.process(&ProcessContext::default(), &mut s);
        let frames = s.as_chunks::<2>().0;
        let half = frames.get(frames.len() / 2..).unwrap_or_default();
        let peak = half.iter().fold(0.0f32, |m, f| m.max(f[0].abs()));
        assert!((20.0 * (f64::from(peak) / 0.1).log10() - 9.0).abs() < 0.05, "{rate}: {peak}");
    }
}

#[test]
fn extreme_settings_stay_stable() {
    // Every band on, at the top of its Q and gain, the corners near both ends: finite and bounded.
    let mut s = Vec::new();
    for n in 1..=BANDS {
        s.extend([
            a(n, param::ON, 1.0),
            a(n, param::TYPE, ((n - 1) % 8) as f64),
            a(n, param::FREQUENCY, if n % 2 == 0 { 10.0 } else { 22_000.0 }),
            a(n, param::Q, 18.0),
            a(n, param::GAIN, 15.0),
        ]);
    }
    s.push((param::SCALE, 2.0));
    let mut e = eq(&s);
    let mut signal = test_signal(44_100);
    e.process(&ProcessContext::default(), &mut signal);
    assert!(signal.iter().all(|v| v.is_finite() && v.abs() < 1e4));
    let mut silence = vec![0.0f32; 44_100];
    eq(&s).process(&ProcessContext::default(), &mut silence);
    assert!(silence.iter().all(|v| *v == 0.0));
}

/// One instance at its defaults (four bands on), then with all eight bands of both curves on as
/// 48 dB cuts in Left/Right, 60 s at 48 kHz each:
/// `cargo test --release -p suhina-device-eq cost -- --ignored --nocapture`.
#[test]
#[ignore = "a timing measurement; run it on its own in release"]
fn cost() {
    let mut heavy: Vec<(usize, f64)> = (1..=BANDS)
        .flat_map(|n| [a(n, param::ON, 1.0), a(n, param::TYPE, 0.0), b(n, param::ON, 1.0), b(n, param::TYPE, 7.0)])
        .collect();
    heavy.push((param::MODE, 1.0));
    for (what, settings) in [("defaults", Vec::new()), ("16 bands of 48 dB cuts", heavy)] {
        let mut e = eq(&settings);
        let _ = e.prepare(48_000.0);
        let mut block = test_signal(512);
        let start = std::time::Instant::now();
        for _ in 0..(60 * 48_000 / 256) {
            e.process(&ProcessContext::default(), &mut block);
        }
        let seconds = start.elapsed().as_secs_f64();
        println!("{what}: 60 s rendered in {seconds:.3} s, {:.2} % of one core", seconds / 60.0 * 100.0);
    }
}
