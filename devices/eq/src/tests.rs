// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_device_api::conformance::test_signal;

use super::*;

const SR: f64 = 44_100.0;

/// Band `band` (from 1): (field, value) pairs.
fn a(band: usize, field: usize, value: f64) -> (usize, f64) {
    (param::band(band - 1, field), value)
}

/// Every band off.
fn all_off() -> Vec<(usize, f64)> {
    (1..=BANDS).map(|n| a(n, param::ON, 0.0)).collect()
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

suhina_device_api::conformance_test!(Eq::with_rate);

#[test]
fn every_type_has_the_measured_response() {
    // At 1 kHz, Q 0.71, +12 dB where the type has gain: the measured table (dB) at 250 Hz,
    // 500 Hz, 707 Hz, 1 kHz, 1.41 kHz, 2 kHz and 4 kHz. The notch's center is left out (a bin).
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
fn each_placement_runs_where_it_says() {
    // Band 9, in use, a bell of +12 dB at 1 kHz, placed in turn.
    let placed = |place: f64| {
        let mut s = all_off();
        s.extend([
            a(9, param::IN_USE, 1.0),
            a(9, param::ON, 1.0),
            a(9, param::TYPE, 3.0),
            a(9, param::FREQUENCY, 1_000.0),
            a(9, param::GAIN, 12.0),
            a(9, param::PLACE, place),
            (param::ADAPTIVE_Q, 0.0),
        ]);
        s
    };
    let run = |settings: &[(usize, f64)], left, right| {
        let mut signal = sine(1_000.0, left, right, 44_100);
        eq(settings).process(&ProcessContext::default(), &mut signal);
        (level(&signal, 1_000.0, 0), level(&signal, 1_000.0, 1))
    };
    let boost = 3.981;
    let (l, r) = run(&placed(0.0), 0.1, 0.1);
    assert!((l / 0.1 - boost).abs() < 0.01 && (r / 0.1 - boost).abs() < 0.01, "stereo: {l} {r}");
    let (l, r) = run(&placed(1.0), 0.1, 0.1);
    assert!((l / 0.1 - boost).abs() < 0.01 && (r - 0.1).abs() < 1e-4, "left: {l} {r}");
    let (l, r) = run(&placed(2.0), 0.1, 0.1);
    assert!((l - 0.1).abs() < 1e-4 && (r / 0.1 - boost).abs() < 0.01, "right: {l} {r}");
    // On the side a centered sound has none and passes unchanged; a sound on one side only is
    // half side, so a 1 kHz boost there spreads it.
    let (l, r) = run(&placed(4.0), 0.1, 0.1);
    assert!((l - 0.1).abs() < 1e-4 && (r - 0.1).abs() < 1e-4, "side, centered: {l} {r}");
    let (l, r) = run(&placed(4.0), 0.1, 0.0);
    // Mid 0.05, side 0.05 × 3.98: left 0.249, right 0.149 (out of phase).
    assert!((l - 0.249).abs() < 0.002 && (r - 0.149).abs() < 0.002, "side, one side: {l} {r}");
    // The right side is the mid less the side: out of phase with the left.
    let mut signal = sine(1_000.0, 0.1, 0.0, 44_100);
    eq(&placed(4.0)).process(&ProcessContext::default(), &mut signal);
    let half = signal.as_chunks::<2>().0.len() / 2;
    let correlation: f64 = signal.as_chunks::<2>().0[half..].iter().map(|f| f64::from(f[0]) * f64::from(f[1])).sum();
    assert!(correlation < 0.0, "the right is out of phase: {correlation}");
    // On the mid a centered sound is all mid.
    let (l, r) = run(&placed(3.0), 0.1, 0.1);
    assert!((l / 0.1 - boost).abs() < 0.01 && (r / 0.1 - boost).abs() < 0.01, "mid: {l} {r}");
}

#[test]
fn a_slot_not_in_use_is_silent_whatever_its_values() {
    let mut s = single(3, DEFAULT_Q, 12.0);
    s.push(a(3, param::IN_USE, 0.0));
    assert!(gain_db(&mut eq(&s), 1_000.0).abs() < 0.01);
    // A new EQ uses the first eight slots, and only those.
    let e = Eq::new();
    for band in 0..BANDS {
        let in_use = e.param(param::band(band, param::IN_USE)).unwrap_or_default() >= 0.5;
        assert_eq!(in_use, band < FIRST_BANDS, "band {}", band + 1);
    }
}

#[test]
fn the_tilts_tip_the_spectrum_around_their_frequency() {
    // +12 dB at 1 kHz: a tilt shelf is 6 dB down far below and 6 dB up far above, 0 at the
    // frequency; a flat tilt also rises evenly between, so an octave above it is part way up.
    for kind in [8, 9] {
        let e = |hz| gain_db(&mut eq(&single(kind, DEFAULT_Q, 12.0)), hz);
        let (low, middle, high) = (e(30.0), e(1_000.0), e(16_000.0));
        assert!(low < -4.0 && high > 4.0, "{}: {low} {high}", TYPE_NAMES[kind]);
        assert!(middle.abs() < 1.0, "{}: {middle}", TYPE_NAMES[kind]);
        assert!(has_gain(kind));
    }
    let flat = |hz| gain_db(&mut eq(&single(9, DEFAULT_Q, 12.0)), hz);
    let (octave_up, two_up) = (flat(2_000.0), flat(4_000.0));
    assert!(octave_up > 0.3 && two_up > octave_up, "{octave_up} {two_up}");
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
        (param::band(2, param::FREQUENCY), 3_000.0),
        (param::band(2, param::GAIN), -15.0),
        (param::band(2, param::Q), 0.3),
        (param::band(2, param::ON), 0.0),
        (param::band(2, param::IN_USE), 0.0),
        (param::band(2, param::TYPE), 7.0),
        (param::band(2, param::TYPE), 2.0),
        (param::band(2, param::TYPE), 9.0),
        (param::band(2, param::PLACE), 3.0),
        (param::band(2, param::PLACE), 1.0),
        (param::SCALE, -1.0),
        (param::OUTPUT, -12.0),
        (param::ADAPTIVE_Q, 1.0),
        (param::AUDITION, 3.0),
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
            a(n, param::IN_USE, 1.0),
            a(n, param::ON, 1.0),
            a(n, param::TYPE, ((n - 1) % TYPE_NAMES.len()) as f64),
            a(n, param::PLACE, ((n - 1) % PLACE_NAMES.len()) as f64),
            a(n, param::FREQUENCY, if n % 2 == 0 { 10.0 } else { 22_000.0 }),
            a(n, param::Q, 18.0),
            a(n, param::GAIN, 15.0),
        ]);
    }
    s.push((param::SCALE, 2.0));
    let mut e = eq(&s);
    let mut signal = test_signal(44_100);
    e.process(&ProcessContext::default(), &mut signal);
    // The boosts stack (several shelves and resonant bells on one side lift everything by far
    // more than 100 dB together), so loudness proves nothing; an unstable filter grows without
    // bound, so the last quarter would be far louder than the second.
    assert!(signal.iter().all(|v| v.is_finite()));
    let peak = |part: &[f32]| part.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let quarter = signal.len() / 4;
    assert!(peak(&signal[3 * quarter..]) <= 2.0 * peak(&signal[quarter..2 * quarter]));
    let mut silence = vec![0.0f32; 44_100];
    eq(&s).process(&ProcessContext::default(), &mut silence);
    assert!(silence.iter().all(|v| *v == 0.0));
}

/// One instance at its defaults (four bands on), then with all 24 bands in use as 48 dB cuts,
/// half of them on the mid and side, 60 s at 48 kHz each:
/// `cargo test --release -p suhina-device-eq cost -- --ignored --nocapture`.
#[test]
#[ignore = "a timing measurement; run it on its own in release"]
fn cost() {
    let heavy: Vec<(usize, f64)> = (1..=BANDS)
        .flat_map(|n| {
            [
                a(n, param::IN_USE, 1.0),
                a(n, param::ON, 1.0),
                a(n, param::TYPE, if n % 2 == 0 { 0.0 } else { 7.0 }),
                a(n, param::PLACE, if n > 12 { 3.0 + (n % 2) as f64 } else { 0.0 }),
            ]
        })
        .collect();
    for (what, settings) in [("defaults", Vec::new()), ("24 bands of 48 dB cuts", heavy)] {
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

#[test]
fn audition_plays_only_the_range_the_band_works_on() {
    // Band 3, a +12 dB bell at 1 kHz: auditioned, a 1 kHz tone passes at its own level (no
    // boost) and a 100 Hz one is far down; let go, the whole EQ returns with its boost.
    let mut settings = single(3, 2.0, 12.0);
    settings.push((param::AUDITION, 3.0));
    let at = |settings: &[(usize, f64)], hz| gain_db(&mut eq(settings), hz);
    assert!(at(&settings, 1_000.0).abs() < 0.3, "the band's center at unity");
    assert!(at(&settings, 100.0) < -12.0, "far from it, quiet");
    let mut e = eq(&settings);
    e.set_param(param::AUDITION, 0.0);
    assert!((gain_db(&mut e, 1_000.0) - 12.0).abs() < 0.1, "let go: the bell's boost again");
    // A low cut's audition is what the cut removes: the lows.
    let mut cut = single(0, DEFAULT_Q, 0.0);
    cut.push((param::AUDITION, 3.0));
    assert!(at(&cut, 100.0) > -1.0 && at(&cut, 10_000.0) < -20.0);
    // A slot not in use cannot be auditioned: the EQ plays as it is.
    let mut unused = single(3, 2.0, 12.0);
    unused.push((param::AUDITION, 20.0));
    assert!((at(&unused, 1_000.0) - 12.0).abs() < 0.1);
}

/// Band 3 as a 1 kHz band of `kind` with no gain of its own, in `mode` (1 Dynamic, 2 Spectral),
/// range −12 dB, threshold −30 dB and a quick follower.
fn moving(kind: usize, mode: f64, q: f64) -> Vec<(usize, f64)> {
    let mut s = single(kind, q, 0.0);
    s.extend([
        a(3, param::MODE, mode),
        a(3, param::RANGE, -12.0),
        a(3, param::THRESHOLD, -30.0),
        a(3, param::ATTACK, 0.002),
        a(3, param::RELEASE, 0.05),
        a(3, param::DETAIL, 1.0),
    ]);
    s
}

/// The gain at `hz` on the left of a steady sine of `amplitude` through `e`, in dB.
fn gain_at(e: &mut Eq, hz: f64, amplitude: f64) -> f64 {
    let mut s = sine(hz, amplitude, amplitude, 44_100);
    e.process(&ProcessContext::default(), &mut s);
    20.0 * (level(&s, hz, 0) / amplitude).log10()
}

#[test]
fn a_dynamic_band_turns_down_as_its_band_gets_loud() {
    // The sine peaks at −20 dB, 10 dB over the threshold: 10/12 of the −12 dB range. The peak
    // follower sags a little between the sine's peaks, so it reads about 1 dB under them.
    let loud = gain_at(&mut eq(&moving(3, 1.0, 1.0)), 1_000.0, 0.1);
    assert!((-11.0..=-8.0).contains(&loud), "loud in the band: about −9 dB, got {loud}");
    let quiet = gain_at(&mut eq(&moving(3, 1.0, 1.0)), 1_000.0, 0.001);
    assert!(quiet.abs() < 0.1, "quiet: untouched, got {quiet}");
    let elsewhere = gain_at(&mut eq(&moving(3, 1.0, 4.0)), 100.0, 0.1);
    assert!(elsewhere.abs() < 0.5, "loud but far below a narrow band: untouched, got {elsewhere}");
    // The panel's readout is the band's gain change.
    let mut e = eq(&moving(3, 1.0, 1.0));
    gain_at(&mut e, 1_000.0, 0.1);
    let change = e.readouts().map_or(0.0, |r| r[2]);
    assert!((-11.0..=-8.0).contains(&f64::from(change)), "{change}");
}

#[test]
fn a_cut_or_notch_stays_static_whatever_its_mode() {
    for kind in [0, 4, 7] {
        let still = gain_at(&mut eq(&single(kind, DEFAULT_Q, 0.0)), 2_000.0, 0.1);
        let asked = gain_at(&mut eq(&moving(kind, 1.0, DEFAULT_Q)), 2_000.0, 0.1);
        assert!((still - asked).abs() < 1e-9, "{}: {still} against {asked}", TYPE_NAMES[kind]);
    }
}

#[test]
fn deleting_the_auditioned_band_fades_back_to_the_whole_eq() {
    let mut settings = single(3, 2.0, 12.0);
    settings.push((param::AUDITION, 3.0));
    let mut e = eq(&settings);
    e.set_param(param::band(2, param::IN_USE), 0.0);
    assert_eq!(e.param(param::AUDITION), Some(0.0), "the audition is let go");
    // The slot is gone and nothing else has gain: a 1 kHz tone passes unchanged.
    assert!(gain_db(&mut e, 1_000.0).abs() < 0.1);
}

#[test]
fn a_keyed_band_moves_with_the_key_and_not_its_own_sound() {
    // A quiet 1 kHz tone through a Dynamic bell whose Trigger is Key: a loud key in the band cuts
    // it; without a key connected it hears its own (quiet) sound and leaves it alone.
    let mut settings = moving(3, 1.0, 1.0);
    settings.push(a(3, param::TRIGGER, 1.0));
    let tone = sine(1_000.0, 0.01, 0.01, 44_100);
    let key = sine(1_000.0, 0.5, 0.5, 44_100);
    let mut keyed = tone.clone();
    eq(&settings).process_keyed(&ProcessContext::default(), &key, &mut keyed);
    let cut = 20.0 * (level(&keyed, 1_000.0, 0) / 0.01).log10();
    assert!(cut < -6.0, "the key drives the cut: {cut}");
    let mut alone = tone.clone();
    eq(&settings).process(&ProcessContext::default(), &mut alone);
    let left = 20.0 * (level(&alone, 1_000.0, 0) / 0.01).log10();
    assert!(left.abs() < 0.2, "no key: its own quiet sound moves nothing: {left}");
    // A band whose Trigger is Band ignores the key.
    let mut own = tone.clone();
    eq(&moving(3, 1.0, 1.0)).process_keyed(&ProcessContext::default(), &key, &mut own);
    assert!((20.0 * (level(&own, 1_000.0, 0) / 0.01).log10()).abs() < 0.2);
}

/// A quiet bed of many tones with one loud resonance at `hz`, through `e`; the resonance's and a
/// quiet neighbour's change in dB.
fn resonance_through(e: &mut Eq, hz: f64) -> (f64, f64) {
    let frames = 96_000;
    let mut signal: Vec<f32> = (0..frames)
        .flat_map(|n| {
            let t = n as f64 / SR;
            let bed: f64 = (1..40).map(|i| 0.004 * (std::f64::consts::TAU * (200.0 * i as f64 + 37.0) * t).sin()).sum();
            let v = (bed + 0.2 * (std::f64::consts::TAU * hz * t).sin()) as f32;
            [v, v]
        })
        .collect();
    e.process(&ProcessContext::default(), &mut signal);
    let resonance = 20.0 * (level(&signal, hz, 0) / 0.2).log10();
    let neighbour = 20.0 * (level(&signal, 1_637.0, 0) / 0.004).log10();
    (resonance, neighbour)
}

/// Band 3 as a Spectral bell at 2 kHz, wide, range −12 dB.
fn spectral() -> Vec<(usize, f64)> {
    let mut s = moving(3, 2.0, 0.5);
    s.extend([a(3, param::FREQUENCY, 2_000.0), a(3, param::THRESHOLD, -60.0), a(3, param::DETAIL, 0.9)]);
    s
}

#[test]
fn a_spectral_band_cuts_a_resonance_and_leaves_its_neighbours() {
    let mut e = eq(&spectral());
    assert_eq!(e.latency(), spectral_dynamics::FRAME as u32, "a Spectral band adds the transform's delay");
    let (resonance, neighbour) = resonance_through(&mut e, 2_200.0);
    assert!(resonance < -6.0, "the resonance is cut: {resonance} dB");
    assert!(neighbour > -2.0, "a quiet neighbour in the band is left alone: {neighbour} dB");
    // The live cut reaches the panel after the bands' readouts, and the band's own readout is its
    // deepest cut.
    let readouts = e.readouts().unwrap_or([0.0; suhina_device_api::MAX_READOUTS]);
    assert!(readouts[BANDS..BANDS + SPECTRAL_READOUTS].iter().any(|g| *g < -6.0));
    assert!(readouts[2] < -6.0);
    const { assert!(BANDS + SPECTRAL_READOUTS <= suhina_device_api::MAX_READOUTS) };
}

#[test]
fn an_eq_without_a_spectral_band_adds_no_delay() {
    assert_eq!(Eq::new().latency(), 0);
    assert_eq!(eq(&moving(3, 1.0, 1.0)).latency(), 0, "a Dynamic band adds none");
    // A Spectral band in a slot not in use does not count.
    let mut unused = spectral();
    unused.push(a(3, param::IN_USE, 0.0));
    assert_eq!(eq(&unused).latency(), 0);
}

#[test]
fn scale_multiplies_a_spectral_bands_range() {
    let mut zero = spectral();
    zero.push((param::SCALE, 0.0));
    let (resonance, _) = resonance_through(&mut eq(&zero), 2_200.0);
    assert!(resonance.abs() < 0.5, "at Scale 0 % nothing moves: {resonance}");
}
