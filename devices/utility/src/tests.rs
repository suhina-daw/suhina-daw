// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_device_api::conformance::test_signal;

use super::*;

const SR: f64 = 48_000.0;
/// Frames after which every ramp has finished.
const SETTLED: usize = 2_000;

fn utility(settings: &[(usize, f64)]) -> Utility {
    let mut u = Utility::new();
    for &(index, value) in settings {
        u.set_param(index, value);
    }
    u.reset();
    u
}

fn run(u: &mut Utility, signal: &mut [f32]) {
    u.process(&ProcessContext::default(), signal);
}

/// Interleaved stereo with a sine of `left_hz` on the left and `right_hz` on the right
/// (0 for silence).
fn sines(left_hz: f64, right_hz: f64, frames: usize) -> Vec<f32> {
    let sine =
        |hz: f64, n: usize| if hz > 0.0 { (std::f64::consts::TAU * hz * n as f64 / SR).sin() as f32 } else { 0.0 };
    (0..frames).flat_map(|n| [sine(left_hz, n), sine(right_hz, n)]).collect()
}

/// Amplitude of each side's sine (RMS times the square root of 2, so it does not depend on
/// where the samples fall on the wave) after the ramps have finished.
fn peaks(signal: &[f32]) -> (f32, f32) {
    let frames = signal.as_chunks::<2>().0.get(SETTLED..).unwrap_or_default();
    let (l, r) = frames.iter().fold((0.0f64, 0.0f64), |(l, r), [a, b]| (l + f64::from(a * a), r + f64::from(b * b)));
    let amplitude = |sum: f64| (sum / frames.len().max(1) as f64 * 2.0).sqrt() as f32;
    (amplitude(l), amplitude(r))
}

suhina_device_api::conformance_test!(Utility::new());

#[test]
fn defaults_pass_the_signal_through_exactly() {
    let input = test_signal(4_096);
    let mut output = input.clone();
    run(&mut Utility::new(), &mut output);
    assert_eq!(input, output);
}

#[test]
fn gain_balance_and_mute() {
    let mut s = sines(1_000.0, 1_000.0, 8_000);
    run(&mut utility(&[(param::GAIN, -6.0)]), &mut s);
    let (l, r) = peaks(&s);
    assert!((l - 0.501).abs() < 0.002 && (r - 0.501).abs() < 0.002, "{l} {r}");

    let mut s = sines(1_000.0, 1_000.0, 8_000);
    run(&mut utility(&[(param::BALANCE, 0.5)]), &mut s);
    let (l, r) = peaks(&s);
    assert!((l - 0.5).abs() < 0.002 && (r - 1.0).abs() < 0.002, "{l} {r}");

    for settings in [[(param::MUTE, 1.0)], [(param::GAIN, -70.0)]] {
        let mut s = sines(1_000.0, 1_000.0, 8_000);
        run(&mut utility(&settings), &mut s);
        assert_eq!(peaks(&s), (0.0, 0.0), "{settings:?}");
    }
}

#[test]
fn mono_and_zero_width_make_both_sides_equal() {
    for settings in [[(param::MONO, 1.0)], [(param::WIDTH, 0.0)], [(param::MID_SIDE, -1.0)]] {
        let mut s = test_signal(4_096);
        run(&mut utility(&settings), &mut s);
        for [l, r] in s.as_chunks::<2>().0 {
            assert!((l - r).abs() < 1e-6, "{settings:?}: {l} {r}");
        }
    }
}

#[test]
fn side_only_and_double_width() {
    // Only the side signal: a sound on the left alone becomes half on the left and half,
    // inverted, on the right.
    let mut s = sines(1_000.0, 0.0, 8_000);
    run(&mut utility(&[(param::MID_SIDE, 1.0)]), &mut s);
    for [l, r] in s.as_chunks::<2>().0.iter().skip(SETTLED) {
        assert!((l + r).abs() < 1e-6);
    }
    let (l, _) = peaks(&s);
    assert!((l - 0.5).abs() < 0.002, "{l}");

    // 200 % width: mid 0.5, side 1.0, so left 1.5 and right -0.5 of the left input.
    let mut s = sines(1_000.0, 0.0, 8_000);
    run(&mut utility(&[(param::WIDTH, 2.0)]), &mut s);
    let (l, r) = peaks(&s);
    assert!((l - 1.5).abs() < 0.003 && (r - 0.5).abs() < 0.003, "{l} {r}");
}

/// A parameter index and the value it is set to.
type Setting = (usize, f64);
/// What a frame should become.
type Mapping = fn([f32; 2]) -> [f32; 2];

#[test]
fn channel_selection_and_inversion() {
    let input = sines(500.0, 1_300.0, 4_096);
    let frames = |s: &[f32]| s.as_chunks::<2>().0.to_vec();
    let cases: [(&[Setting], Mapping); 5] = [
        (&[(param::CHANNEL, channel::LEFT)], |[l, _]| [l, l]),
        (&[(param::CHANNEL, channel::RIGHT)], |[_, r]| [r, r]),
        (&[(param::CHANNEL, channel::SWAP)], |[l, r]| [r, l]),
        (&[(param::INVERT_LEFT, 1.0)], |[l, r]| [-l, r]),
        (&[(param::INVERT_LEFT, 1.0), (param::INVERT_RIGHT, 1.0), (param::CHANNEL, channel::SWAP)], |[l, r]| [-r, -l]),
    ];
    for (settings, expected) in cases {
        let mut s = input.clone();
        run(&mut utility(settings), &mut s);
        for (got, original) in frames(&s).into_iter().zip(frames(&input)) {
            assert_eq!(got, expected(original), "{settings:?}");
        }
    }
}

#[test]
fn bass_mono_centres_the_lows_and_keeps_the_highs() {
    let settings = [(param::BASS_MONO, 1.0), (param::BASS_MONO_FREQUENCY, 200.0)];
    // 40 Hz only on the left comes out at half level on both sides.
    let mut s = sines(40.0, 0.0, 48_000);
    run(&mut utility(&settings), &mut s);
    let (l, r) = peaks(&s);
    assert!((l - 0.5).abs() < 0.01 && (r - 0.5).abs() < 0.01, "{l} {r}");
    // 3 kHz only on the left stays on the left.
    let mut s = sines(3_000.0, 0.0, 48_000);
    run(&mut utility(&settings), &mut s);
    let (l, r) = peaks(&s);
    assert!((l - 1.0).abs() < 0.01 && r < 0.001, "{l} {r}");
    // A mono signal is not changed in level at any frequency.
    for hz in [40.0, 200.0, 1_000.0] {
        let mut s = sines(hz, hz, 48_000);
        run(&mut utility(&settings), &mut s);
        let (l, r) = peaks(&s);
        assert!((l - 1.0).abs() < 0.01 && (r - 1.0).abs() < 0.01, "{hz} Hz: {l} {r}");
    }
}

#[test]
fn dc_filter_removes_an_offset() {
    let mut s: Vec<f32> = sines(200.0, 200.0, 96_000).iter().map(|x| x * 0.5 + 0.25).collect();
    run(&mut utility(&[(param::DC_FILTER, 1.0)]), &mut s);
    let tail = &s[s.len() - 48_000..];
    let mean = tail.iter().sum::<f32>() / tail.len() as f32;
    assert!(mean.abs() < 0.002, "{mean}");
}

#[test]
fn changes_are_smoothed() {
    let mut u = Utility::new();
    let mut s = vec![1.0f32; 2 * 2_000];
    u.set_param(param::MUTE, 1.0);
    run(&mut u, &mut s);
    // 20 ms at 48 kHz is 960 frames: the level falls by about 1/960 per frame, then stays at 0.
    let left: Vec<f32> = s.as_chunks::<2>().0.iter().map(|[l, _]| *l).collect();
    let largest_step = left.windows(2).map(|w| (w[0] - w[1]).abs()).fold(0.0, f32::max);
    assert!(largest_step < 0.0011, "{largest_step}");
    assert!(left[958] > 0.0 && left[960..].iter().all(|&v| v == 0.0));
}

#[test]
fn ramps_follow_the_sample_rate() {
    let mut u = Utility::new();
    assert!(u.prepare(96_000.0).is_ok());
    u.set_param(param::MUTE, 1.0);
    let mut s = vec![1.0f32; 2 * 2_000];
    run(&mut u, &mut s);
    let left: Vec<f32> = s.as_chunks::<2>().0.iter().map(|[l, _]| *l).collect();
    assert!(left[1_900] > 0.0 && left[1_920] == 0.0);
}

#[test]
fn bass_mono_listen_plays_only_the_lows() {
    let listen = [(param::BASS_MONO_LISTEN, 1.0), (param::BASS_MONO_FREQUENCY, 200.0)];
    // 3 kHz is above the band: silent.
    let mut s = sines(3_000.0, 3_000.0, 48_000);
    run(&mut utility(&listen), &mut s);
    let (l, r) = peaks(&s);
    assert!(l < 0.001 && r < 0.001, "{l} {r}");
    // 40 Hz on the left only stays on the left without bass mono...
    let mut s = sines(40.0, 0.0, 48_000);
    run(&mut utility(&listen), &mut s);
    let (l, r) = peaks(&s);
    assert!((l - 1.0).abs() < 0.01 && r < 0.001, "{l} {r}");
    // ...and is centred with it.
    let mut s = sines(40.0, 0.0, 48_000);
    run(&mut utility(&[listen.as_slice(), &[(param::BASS_MONO, 1.0)]].concat()), &mut s);
    let (l, r) = peaks(&s);
    assert!((l - 0.5).abs() < 0.01 && (r - 0.5).abs() < 0.01, "{l} {r}");
}

#[test]
fn the_gain_knob_is_even_in_db_over_its_top_three_quarters_and_falls_to_silence() {
    let gain = PARAMS[param::GAIN];
    for (position, db) in [(1.0, 35.0), (0.75, 17.5), (0.5, 0.0), (0.25, -17.5)] {
        assert!((gain.from_normalized(position) - db).abs() < 1e-9, "{position}");
        assert!((gain.to_normalized(db) - position).abs() < 1e-9, "{db}");
    }
    // Below a quarter the gain is a power of the travel: -29.6 dB at an eighth.
    assert!((gain.from_normalized(0.125) + 29.6).abs() < 0.1, "{}", gain.from_normalized(0.125));
    assert_eq!(gain.from_normalized(0.0), f64::from(SILENCE_DB));
    assert_eq!(gain.to_normalized(f64::from(SILENCE_DB)), 0.0);
    for position in [0.02, 0.1, 0.2, 0.249, 0.3, 0.9] {
        assert!((gain.to_normalized(gain.from_normalized(position)) - position).abs() < 1e-9, "{position}");
    }
}

#[test]
fn a_jump_in_gain_glides_along_the_knob_in_a_few_milliseconds() {
    let gain = PARAMS[param::GAIN];
    let mut u = utility(&[(param::GAIN, -54.0)]);
    u.set_param(param::GAIN, 8.0);
    // A steady 1 on both sides: the output is the gain.
    let mut ones = vec![1.0f32; 2 * 1_000];
    run(&mut u, &mut ones);
    let position: Vec<f64> =
        ones.iter().step_by(2).map(|&g| gain.to_normalized(20.0 * f64::from(g).max(1e-12).log10())).collect();
    let (from, to) = (gain.to_normalized(-54.0), gain.to_normalized(8.0));
    let share = |n: usize| (position[n] - from) / (to - from);
    // An S along the knob: it starts slowly, is half-way after 1.1 ms and there by 8 ms.
    assert!(share(2) < 0.01, "{}", share(2));
    let half = (0..1_000).find(|&n| share(n) >= 0.5).unwrap_or(0);
    assert!((50..=58).contains(&half), "{half}");
    assert!(position.windows(2).all(|w| w[1] >= w[0] - 1e-12));
    assert!((f64::from(ones[2 * 400]) - 10f64.powf(8.0 / 20.0)).abs() < 1e-3, "{}", ones[2 * 400]);
}
