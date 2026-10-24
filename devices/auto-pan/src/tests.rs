// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_device_api::conformance::test_signal;

use super::*;

const SR: f64 = 48_000.0;

fn auto_pan(settings: &[(usize, f64)]) -> AutoPan {
    let mut p = AutoPan::new();
    for &(index, value) in settings {
        p.set_param(index, value);
    }
    p.reset();
    p
}

/// Each side's gain over `frames` frames: the output for a constant 1.0 input.
fn gains(p: &mut AutoPan, context: ProcessContext, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let mut buffer = vec![1.0f32; frames * 2];
    p.process(&context, &mut buffer);
    buffer.as_chunks::<2>().0.iter().map(|[l, r]| (*l, *r)).unzip()
}

fn stopped() -> ProcessContext {
    ProcessContext { tempo: 120.0, beat: 0.0, playing: false }
}

/// Frames at which `gain` falls below 0.6 after having been above 0.9: once per cycle,
/// not counting where it starts.
fn troughs(gain: &[f32]) -> Vec<usize> {
    let mut armed = false;
    let mut found = Vec::new();
    for (i, &g) in gain.iter().enumerate() {
        if armed && g < 0.6 {
            found.push(i);
            armed = false;
        } else if g > 0.9 {
            armed = true;
        }
    }
    found
}

/// Checks that `gain` dips every `frames` frames (within two frames), at least `cycles` times.
fn assert_period(gain: &[f32], frames: usize, cycles: usize) {
    let t = troughs(gain);
    assert!(t.len() >= cycles, "{t:?}");
    for pair in t.windows(2) {
        assert!((pair[1] - pair[0]).abs_diff(frames) <= 2, "{t:?}");
    }
}

suhina_device_api::conformance_test!(AutoPan::new());

#[test]
fn no_amount_passes_the_signal_through_exactly() {
    let input = test_signal(4_096);
    let mut output = input.clone();
    auto_pan(&[(param::AMOUNT, 0.0)]).process(&stopped(), &mut output);
    assert_eq!(input, output);
}

#[test]
fn pans_between_the_sides() {
    let (l, r) = gains(&mut auto_pan(&[]), stopped(), SR as usize);
    // Half a cycle apart, the two gains always add up to 2 - amount.
    for (a, b) in l.iter().zip(&r) {
        assert!((a + b - 1.5).abs() < 0.01, "{a} {b}");
    }
    let (min, max) = l.iter().fold((1.0f32, 0.0f32), |(lo, hi), &g| (lo.min(g), hi.max(g)));
    assert!((min - 0.5).abs() < 0.01 && (max - 1.0).abs() < 0.01, "{min} {max}");
}

#[test]
fn no_phase_difference_is_tremolo() {
    let (l, r) = gains(&mut auto_pan(&[(param::PHASE, 0.0), (param::AMOUNT, 1.0)]), stopped(), 24_000);
    assert_eq!(l, r);
    assert!(l.iter().any(|&g| g < 0.01));
}

#[test]
fn free_rate_in_hertz() {
    let (l, _) = gains(&mut auto_pan(&[(param::RATE, 4.0)]), stopped(), SR as usize);
    // Four cycles a second.
    assert_period(&l, 12_000, 3);
}

#[test]
fn synced_rate_follows_the_song_position() {
    let settings = [(param::SYNC, 1.0), (param::SYNCED_RATE, 10.0), (param::AMOUNT, 1.0), (param::PHASE, 0.0)];
    // 1/8 notes at 120 bpm: four cycles a second.
    let playing = ProcessContext { tempo: 120.0, beat: 0.0, playing: true };
    let (l, _) = gains(&mut auto_pan(&settings), playing, SR as usize);
    assert_period(&l, 12_000, 3);
    // A cycle is half a beat, 12 000 frames. Starting at beat 3.25 is starting half a cycle
    // in: the same as frame 6000 from beat 0.
    let later = ProcessContext { beat: 3.25, ..playing };
    let (shifted, _) = gains(&mut auto_pan(&settings), later, 2_000);
    for (a, b) in shifted.iter().skip(500).zip(&l[6_500..]) {
        assert!((a - b).abs() < 1e-3, "{a} {b}");
    }
}

#[test]
fn synced_rate_runs_at_the_tempo_when_stopped() {
    let settings = [(param::SYNC, 1.0), (param::SYNCED_RATE, 7.0)];
    // Quarter notes at 90 bpm: 1.5 cycles a second, so three in two seconds.
    let (l, _) = gains(&mut auto_pan(&settings), ProcessContext { tempo: 90.0, ..stopped() }, 2 * SR as usize);
    assert_period(&l, 32_000, 2);
}

#[test]
fn offset_and_invert_move_the_start() {
    // A quarter cycle in, the sine is at its top: the left side is at its quietest.
    let (l, _) = gains(&mut auto_pan(&[(param::OFFSET, 0.25)]), stopped(), 1);
    assert!((l[0] - 0.5).abs() < 1e-3, "{}", l[0]);
    let (l, _) = gains(&mut auto_pan(&[(param::OFFSET, 0.25), (param::INVERT, 1.0)]), stopped(), 1);
    assert!((l[0] - 1.0).abs() < 1e-3, "{}", l[0]);
}

#[test]
fn shaping_flattens_the_wave_towards_a_square() {
    let (plain, _) = gains(&mut auto_pan(&[(param::AMOUNT, 1.0)]), stopped(), SR as usize);
    let (shaped, _) = gains(&mut auto_pan(&[(param::AMOUNT, 1.0), (param::SHAPING, 1.0)]), stopped(), SR as usize);
    let at_ends = |g: &[f32]| g.iter().filter(|&&v| !(0.02..=0.98).contains(&v)).count();
    assert!(at_ends(&shaped) > 3 * at_ends(&plain), "{} {}", at_ends(&shaped), at_ends(&plain));
}

#[test]
fn square_edges_do_not_click() {
    let settings = [(param::SHAPE, 4.0), (param::AMOUNT, 1.0), (param::RATE, 10.0)];
    let (l, _) = gains(&mut auto_pan(&settings), stopped(), SR as usize);
    let largest_step = l.windows(2).map(|w| (w[0] - w[1]).abs()).fold(0.0, f32::max);
    // A 2 ms filter at 48 kHz moves at most about 1/96 of a full step per frame.
    assert!(largest_step < 0.011, "{largest_step}");
    assert!(l.iter().any(|&g| g > 0.99) && l.iter().any(|&g| g < 0.01));
}
