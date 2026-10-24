// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// A grid of `step` beats in bars of `bar_beats`, at `beat_width` points a beat, with lines
/// at least 8 points apart.
fn grid(step: f64, beat_width: f32, bar_beats: f64) -> TimelineGrid {
    TimelineGrid::new(step, beat_width, bar_beats, 8.0)
}

/// The lines from `first` to `last`, in thousandths of a beat.
fn drawn(grid: TimelineGrid, first: f64, last: f64) -> Vec<f64> {
    grid.lines(first, last, 1000).into_iter().map(|(b, _)| (b * 1000.0).round()).collect()
}

/// Whether every place `grid` snaps to between beats 0 and `last` is a drawn line.
fn snaps_to_drawn_lines(grid: TimelineGrid, last: f64) -> bool {
    let lines = drawn(grid, 0.0, last + grid.step.max(grid.bar_beats));
    (0..(last * 100.0) as usize).map(|n| n as f64 / 100.0).all(|beat| {
        [grid.place(beat, false, false), grid.place(beat, false, true)]
            .iter()
            .all(|placed| lines.contains(&(placed * 1000.0).round()))
    })
}

#[test]
fn a_straight_step_doubles_to_the_beat_then_goes_to_bars() {
    assert_eq!(grid(0.25, 32.0, 4.0).step, 0.25);
    assert_eq!(grid(0.25, 31.0, 4.0).step, 0.5);
    assert_eq!(grid(0.25, 15.0, 4.0).step, 1.0);
    assert_eq!(grid(0.25, 7.0, 4.0).step, 4.0);
    assert_eq!(grid(0.25, 1.9, 4.0).step, 8.0);
    assert_eq!(grid(0.0, 400.0, 4.0), TimelineGrid { step: 1.0, bar_beats: 4.0, snaps: false });
    // A half note in 3/4 is not on the bar: once too narrow it goes to the bar.
    assert_eq!(grid(2.0, 5.0, 3.0).step, 2.0);
    assert_eq!(grid(2.0, 3.0, 3.0).step, 3.0);
    // A broken bar length is four beats; a broken zoom coarsens to the most bars and stops.
    assert_eq!(grid(1.0, 400.0, f64::NAN).bar_beats, 4.0);
    assert_eq!(grid(1.0, f32::NAN, 4.0).step, 4096.0);
}

#[test]
fn places_snap_to_the_lines_inside_each_bar_unless_free() {
    let sixteenths = grid(0.25, 1000.0, 4.0);
    assert_eq!(sixteenths.place(2.13, false, false), 2.25);
    assert_eq!(sixteenths.place(2.13, true, false), 2.13);
    assert_eq!(sixteenths.place(-1.0, false, false), 0.0);
    assert_eq!(sixteenths.place(f64::NAN, false, false), 0.0);
    assert_eq!(sixteenths.place(1.45, false, true), 1.25);
    // Triplet lines start again at each bar line: in 3/4 a half-note triplet (4/3 beats)
    // has lines at 0, 4/3 and 8/3, then the bar line at 3.
    let triplets = grid(4.0 / 3.0, 400.0, 3.0);
    assert!((triplets.place(1.5, false, false) - 4.0 / 3.0).abs() < 1e-12);
    assert_eq!(triplets.place(2.9, false, false), 3.0);
    assert!((triplets.place(4.4, false, false) - (3.0 + 4.0 / 3.0)).abs() < 1e-12);
    // Bars: on whole steps from the start.
    let two_bars = grid(8.0, 8.0, 4.0);
    assert_eq!(two_bars.place(11.0, false, false), 8.0);
    assert_eq!(two_bars.place(13.0, false, false), 16.0);
}

#[test]
fn down_gives_the_line_before_counted_from_its_bar() {
    // In 3/4 a two-beat step has lines at 0 and 2 in each bar, then the bar line at 3: 3.1
    // is in the cell that starts on the bar line, not at beat 2.
    let halves = grid(2.0, 400.0, 3.0);
    assert_eq!(halves.place(3.1, false, true), 3.0);
    assert_eq!(halves.place(2.9, false, true), 2.0);
    assert_eq!(halves.place(5.5, false, true), 5.0);
    assert_eq!(halves.place(3.1, true, true), 3.1);
    assert_eq!(halves.place(-1.0, false, true), 0.0);
    assert_eq!(grid(8.0, 8.0, 4.0).place(15.9, false, true), 8.0);
}

#[test]
fn the_lines_next_to_a_beat_are_drawn_lines() {
    let halves = grid(2.0, 400.0, 3.0);
    assert_eq!(halves.next_line(0.5), 2.0);
    assert_eq!(halves.next_line(2.0), 3.0, "the bar line, not beat 4");
    assert_eq!(halves.next_line(3.0), 5.0);
    assert_eq!(halves.previous_line(3.0), 2.0);
    assert_eq!(halves.previous_line(5.0), 3.0);
    assert_eq!(halves.previous_line(2.5), 2.0);
    assert_eq!(halves.previous_line(0.0), 0.0);
    let bars = grid(4.0, 8.0, 4.0);
    assert_eq!((bars.next_line(4.0), bars.previous_line(4.0)), (8.0, 0.0));
}

#[test]
fn lines_are_drawn_at_every_step_with_bar_lines_marked() {
    let beats = grid(1.0, 1000.0, 4.0);
    assert_eq!(
        beats.lines(3.5, 6.0, 100),
        vec![(4.0, true), (5.0, false), (6.0, false)],
        "the bar before the view starts the lines"
    );
    assert_eq!(drawn(grid(4.0 / 3.0, 400.0, 3.0), 0.0, 3.0), vec![0.0, 1333.0, 2667.0, 3000.0]);
    assert_eq!(grid(8.0, 8.0, 4.0).lines(1.0, 17.0, 100), vec![(0.0, true), (8.0, true), (16.0, true)]);
    // Never more than asked for.
    assert_eq!(grid(0.125, 1000.0, 4.0).lines(0.0, 1e9, 50).len(), 50);
    assert!(beats.lines(f64::NAN, 4.0, 10).is_empty());
}

#[test]
fn a_triplet_zoomed_out_in_4_4_stays_on_the_bar_lines() {
    // 1/16T at 6 points a line, as the piano roll draws it: a sixth of a beat from 36 points.
    let at = |beat_width: f32| TimelineGrid::new(1.0 / 6.0, beat_width, 4.0, 6.0);
    assert!((at(36.0).step - 1.0 / 6.0).abs() < 1e-12);
    // Narrower: the beat, then the bar, then bars double; never 1/3, 2/3, 4/3 or 8/3.
    assert_eq!(at(35.0).step, 1.0);
    assert_eq!(at(5.0).step, 4.0);
    assert_eq!(at(1.0).step, 8.0);
    for beat_width in [100.0, 35.0, 20.0, 10.0, 5.0, 2.0, 1.0] {
        let grid = at(beat_width);
        assert!(drawn(grid, 0.0, 16.0).contains(&16000.0), "a line on bar 5 at {beat_width}");
        assert!(snaps_to_drawn_lines(grid, 16.0), "{beat_width}");
    }
}

#[test]
fn odd_bars_keep_their_lines_on_the_bar_lines() {
    // 5/4, one beat zoomed out: the bar (5 beats), then 10, not 2, 4 or 8 beats.
    let five = |beat_width: f32| TimelineGrid::new(1.0, beat_width, 5.0, 6.0);
    assert_eq!(five(5.0).step, 5.0);
    assert_eq!(five(1.0).step, 10.0);
    // 5/4 with a two-beat step: lines at 0, 2, 4 and the bar line at 5, then 7, 9, 10.
    let halves = TimelineGrid::new(2.0, 100.0, 5.0, 6.0);
    assert_eq!(drawn(halves, 0.0, 10.0), vec![0.0, 2000.0, 4000.0, 5000.0, 7000.0, 9000.0, 10000.0]);
    // 3/8 (a bar of 1.5 beats), 1/4 triplet: lines at 0, 2/3, 4/3 and on each bar line.
    let in_3_8 = TimelineGrid::new(2.0 / 3.0, 100.0, 1.5, 6.0);
    assert_eq!(drawn(in_3_8, 0.0, 3.0), vec![0.0, 667.0, 1333.0, 1500.0, 2167.0, 2833.0, 3000.0]);
    for grid in [five(5.0), five(1.0), halves, in_3_8, TimelineGrid::new(2.0 / 3.0, 3.0, 1.5, 6.0)] {
        assert!(snaps_to_drawn_lines(grid, 20.0), "{grid:?}");
    }
}

#[test]
fn an_adaptive_step_follows_the_zoom_the_bar_and_triplets() {
    // 24 points apart at least.
    assert_eq!(adaptive_step(24.0, false, 10.0, 4.0), 4.0, "a bar at 10 points per beat");
    assert_eq!(adaptive_step(24.0, false, 40.0, 4.0), 1.0, "a quarter at 40");
    assert_eq!(adaptive_step(24.0, false, 100.0, 4.0), 0.25, "a sixteenth at 100");
    assert_eq!(adaptive_step(24.0, false, 1000.0, 4.0), 0.125, "never finer than 1/32");
    assert_eq!(adaptive_step(24.0, false, 1.0, 4.0), 32.0, "bars double when zoomed far out");
    // In 3/4 the step after the half note is the 3-beat bar.
    assert_eq!(adaptive_step(24.0, false, 10.0, 3.0), 3.0);
    assert_eq!(adaptive_step(24.0, false, 12.0, 3.0), 2.0);
    assert!((adaptive_step(24.0, true, 100.0, 4.0) - 1.0 / 3.0).abs() < 1e-12, "an eighth triplet at 100");
    for beat_width in [f32::NAN, 0.0, -5.0, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(adaptive_step(6.0, false, beat_width, 4.0), 4.0, "{beat_width}");
    }
    assert_eq!(adaptive_step(96.0, false, f32::MIN_POSITIVE, 4.0), 4.0 * 2f64.powi(MAX_BAR_DOUBLINGS));
    assert_eq!(adaptive_step(24.0, false, 40.0, f64::NAN), 1.0, "a broken bar length is 4 beats");
}
