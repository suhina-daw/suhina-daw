// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// Feeds `input` through `resampler` in calls of `chunk` input frames, collecting every
/// output frame the feed produced; the stream's last `HALF_TAPS + 1` input frames' outputs
/// stay queued, as no flush ends it.
fn run(resampler: &mut Resampler, input: &[f64], chunk: usize) -> Vec<f64> {
    let cap = input.len() * 2 + TAPS;
    let mut out = vec![0.0f64; cap];
    let mut at = 0usize;
    let mut fed = 0usize;
    while fed < input.len() {
        let take = chunk.min(input.len() - fed);
        fed += take;
        at += resampler.process(&input[fed - take..fed], &mut out[at..]);
    }
    while at < out.len() {
        let n = resampler.process(&[], &mut out[at..]);
        if n == 0 {
            break;
        }
        at += n;
    }
    out.truncate(at);
    out
}

#[test]
fn a_flushed_stream_holds_all_of_its_time() {
    // `process` with no input drains nothing; `flush` gives the frames up to the input's end.
    let input = vec![0.25f64; 1_000];
    let mut line = Resampler::new(44_100.0, 48_000.0);
    let mut out = vec![0.0f64; 1_200];
    let mut at = line.process(&input, &mut out);
    assert_eq!(line.process(&[], &mut out[at..]), 0);
    at += line.flush(&mut out[at..]);
    let want = 1_000.0 * 48_000.0 / 44_100.0;
    assert!((at as f64 - want).abs() <= 1.0, "{at}");
}

#[test]
fn a_constant_signal_survives_both_legs() {
    // A 44.1 kHz project into a 48 kHz model and back to 44.1 kHz.
    let input = vec![1.0f64; 2_000];
    let mid = run(&mut Resampler::new(44_100.0, 48_000.0), &input, 256);
    let steady = &mid[mid.len() / 2..];
    let back = run(&mut Resampler::new(48_000.0, 44_100.0), steady, 256);
    for &y in back.iter().skip(48) {
        assert!((y - 1.0).abs() < 1e-9, "{y}");
    }
}

#[test]
fn an_impulse_spreads_over_the_kernel_and_nowhere_else() {
    let mut input = vec![0.0f64; 400];
    input[200] = 1.0;
    let out = run(&mut Resampler::new(44_100.0, 48_000.0), &input, 256);
    assert!(!out.is_empty());
    // The impulse's time in the output rate is 200 * 48 / 44.1 = 217.7; the kernel's spread
    // sits around it and the rest is quiet. The stream starts the instant, so the impulse
    // is where its time says.
    let impulse_time = 200.0 * 48_000.0 / 44_100.0;
    let strongest = out
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map_or(0, |(i, _)| i);
    assert!((strongest as f64 - impulse_time).abs() <= 2.0, "peak at {strongest} of {}", out.len());
    for (i, &y) in out.iter().enumerate() {
        if (i as f64 - impulse_time).abs() > 2.0 * HALF_TAPS as f64 {
            assert!(y.abs() < 1e-7, "tail {i}: {y}");
        }
    }
    // The stream has almost all of the 400 inputs' worth of output: everything except the
    // kernel's last half, which waits for input that never came.
    assert!(out.len() >= (400.0_f64 * 44_100.0 / 48_000.0).round() as usize);
}

#[test]
fn the_output_never_depends_on_how_the_feed_is_divided() {
    let input: Vec<f64> = (0..2_000).map(|i| (i as f64 * 0.017).sin()).collect();
    let one = run(&mut Resampler::new(48_000.0, 44_100.0), &input, input.len());
    let seven = run(&mut Resampler::new(48_000.0, 44_100.0), &input, 7);
    assert_eq!(one, seven);
}

#[test]
fn unfed_frames_wait_for_a_bigger_output_slice() {
    // An output slice with no room keeps the input: the next call resumes where it stopped.
    let mut line = Resampler::new(48_000.0, 96_000.0);
    let mut a = [0.0f64; 2];
    let wrote_now = line.process(&[0.5; 100], &mut a);
    let mut b = [0.0f64; 64];
    let rest = line.process(&[], &mut b);
    // The stream is the same as one call with room for all of it.
    let full = run(&mut Resampler::new(48_000.0, 96_000.0), &vec![0.5f64; 100], 100);
    let combined: Vec<f64> = a.iter().take(wrote_now).copied().chain(b.iter().take(rest).copied()).collect();
    assert_eq!(combined, full[..wrote_now + rest]);
}

#[test]
fn a_steady_signal_comes_out_at_its_level() {
    let out = run(&mut Resampler::new(48_000.0, 96_000.0), &vec![0.5f64; 400], 64);
    for &y in out.iter().skip(64).take(200) {
        assert!((y - 0.5).abs() < 1e-9, "{y}");
    }
}

#[test]
fn a_tone_above_the_target_rate_is_not_aliased_in() {
    // A 30 kHz tone at 96 kHz resampled to 48 kHz: above the target's Nyquist, so it has
    // to come out as near-silence. With the cutoff left at the input's Nyquist it came
    // through at full amplitude, folded to 18 kHz.
    let input: Vec<f64> =
        (0..24_000).map(|i| (std::f64::consts::TAU * 30_000.0 * i as f64 / 96_000.0).sin() * 0.5).collect();
    let out = run(&mut Resampler::new(96_000.0, 48_000.0), &input, 512);
    let steady = &out[out.len() / 2..];
    let peak = steady.iter().fold(0.0f64, |a, v| a.max(v.abs()));
    assert!(peak < 0.02, "the out-of-band tone survived at {peak}");
}

#[test]
fn a_tone_below_both_nyquists_comes_through() {
    let input: Vec<f64> =
        (0..24_000).map(|i| (std::f64::consts::TAU * 5_000.0 * i as f64 / 96_000.0).sin() * 0.5).collect();
    let out = run(&mut Resampler::new(96_000.0, 48_000.0), &input, 512);
    let steady = &out[out.len() / 2..];
    let peak = steady.iter().fold(0.0f64, |a, v| a.max(v.abs()));
    assert!(peak > 0.45, "the in-band tone lost level: {peak}");
}

#[test]
fn hostile_frames_are_silence_and_outputs_stay_finite() {
    let input = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1.0, 1.0, 0.0];
    let out = run(&mut Resampler::new(44_100.0, 48_000.0), &input, 3);
    for &y in &out {
        assert!(y.is_finite(), "{y}");
    }
}

#[test]
fn equal_rates_are_left_to_the_device() {
    assert!(Resampler::new(48_000.0, 48_000.0).passthrough());
    assert!(!Resampler::new(48_000.0, 96_000.0).passthrough());
}

#[test]
fn reset_starts_over() {
    let mut line = Resampler::new(48_000.0, 96_000.0);
    let first = run(&mut line, &vec![0.5f64; 200], 64);
    line.reset();
    let fresh = Resampler::new(48_000.0, 96_000.0);
    assert_eq!(line.table, fresh.table);
    assert_eq!(line.history.as_ref(), fresh.history.as_ref());
    assert_eq!(line.fed, 0);
    assert_eq!(line.produced, 0.0);
    let again = run(&mut line, &vec![0.5f64; 200], 64);
    assert_eq!(first, again);
}
