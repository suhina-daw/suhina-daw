// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rhythmic slices against the end of the stretch of file the clip's time plays in.

use suhina_stretch::{Slices, Tail};

const RATE: f32 = 48_000.0;

/// A clip at half the file's speed looping file frames 0 to 6000, inside the slice from 0 to
/// 12 000: the slice's sound runs at the file's speed and reaches the loop's end long before the
/// clip's time comes round. Bounded by the loop's end it never plays what lies past it (here
/// sound, where the loop is silence), whichever tail it has.
#[test]
fn a_slice_never_plays_the_file_past_a_loops_end() {
    let past_the_loop = |at: f64, _: usize| if (6_000.0..48_000.0).contains(&at) { 1.0 } else { 0.0 };
    let looped = |t: u64| (t as f64 * 0.5).rem_euclid(6_000.0);
    let points = [12_000.0, 24_000.0, 36_000.0];
    for tail in [Tail::Off, Tail::Loop, Tail::BackAndForth] {
        let mut slices = Slices::new(1, RATE, 48_000, &points, tail, 1.0);
        let mut out = vec![0.0f32; 48_000];
        for (i, chunk) in out.chunks_mut(500).enumerate() {
            slices.render_within((i * 500) as u64, chunk, &looped, &|_| 6_000.0, &past_the_loop);
        }
        let loudest = out.iter().map(|s| s.abs()).fold(0.0, f32::max);
        assert_eq!(loudest, 0.0, "{tail:?}: the file past the loop was heard");
        // Unbounded (as before), the same slice read on past the loop's end.
        let mut slices = Slices::new(1, RATE, 48_000, &points, tail, 1.0);
        let mut out = vec![0.0f32; 48_000];
        for (i, chunk) in out.chunks_mut(500).enumerate() {
            slices.render((i * 500) as u64, chunk, &looped, &past_the_loop);
        }
        assert!(out.iter().any(|s| s.abs() > 0.5), "{tail:?}: unbounded, the test's file past the loop is heard");
    }
}
