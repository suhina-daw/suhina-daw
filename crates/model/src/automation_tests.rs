// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn points(list: &[(f64, f64)]) -> Vec<AutomationPoint> {
    list.iter().map(|&(beat, value)| AutomationPoint::new(beat, value)).collect()
}

fn curve([x1, y1, x2, y2]: [f64; 4]) -> Curve {
    Curve { x1, y1, x2, y2 }
}

fn close(a: Curve, b: [f64; 4]) -> bool {
    a.to_array().iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-4)
}

#[test]
fn a_lane_holds_outside_its_points_and_moves_in_a_line_between() {
    let p = points(&[(1.0, 0.0), (3.0, 1.0)]);
    assert_eq!(value_at(&p, 0.0, false, &Plain), Some(0.0));
    assert_eq!(value_at(&p, 2.0, false, &Plain), Some(0.5));
    assert_eq!(value_at(&p, 10.0, false, &Plain), Some(1.0));
    assert_eq!(value_at(&p, 2.0, true, &Plain), Some(0.0));
    assert_eq!(value_at(&[], 2.0, false, &Plain), None);
}

#[test]
fn two_points_at_one_beat_jump_there() {
    let p = points(&[(0.0, 0.0), (2.0, 0.2), (2.0, 0.8), (4.0, 0.8)]);
    assert!((value_at(&p, 1.999, false, &Plain).unwrap_or(0.0) - 0.2).abs() < 1e-3);
    assert_eq!(value_at(&p, 2.0, false, &Plain), Some(0.8));
}

#[test]
fn hostile_beats_never_panic() {
    let p = points(&[(0.0, 1.0), (f64::MAX, 2.0)]);
    for beat in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, f64::MAX] {
        assert!(value_at(&p, beat, false, &Plain).is_some());
    }
}

#[test]
fn points_go_in_beat_order_after_those_at_the_same_beat() {
    let mut lane = AutomationLane::new(VOLUME);
    assert_eq!(lane.insert(AutomationPoint::new(2.0, 1.0)), 0);
    assert_eq!(lane.insert(AutomationPoint::new(1.0, 0.0)), 0);
    assert_eq!(lane.insert(AutomationPoint::new(2.0, 0.5)), 2);
    assert_eq!(lane.points.iter().map(|p| p.value).collect::<Vec<_>>(), [0.0, 1.0, 0.5]);
    // Put back near where it was: from before the points at its beat, first among them;
    // from after them, last; from among them, where it was.
    let mut jump = AutomationLane::new("volume");
    for (beat, value) in [(0.0, 0.1), (4.0, 0.2), (4.0, 0.3), (8.0, 0.4)] {
        jump.insert(AutomationPoint::new(beat, value));
    }
    let mut moved = jump.clone();
    let point = moved.points.remove(0);
    assert_eq!(moved.insert_near(AutomationPoint { beat: 4.0, ..point }, 0), 0);
    let mut moved = jump.clone();
    let point = moved.points.remove(3);
    assert_eq!(moved.insert_near(AutomationPoint { beat: 4.0, ..point }, 3), 3);
    let mut moved = jump.clone();
    let point = moved.points.remove(1);
    assert_eq!(moved.insert_near(AutomationPoint { value: 0.9, ..point }, 1), 1);
    assert_eq!(moved.points[2].value, 0.3);
}

/// A curve is the Bezier through its two control points; a bend is the mirrored family.
#[test]
fn a_curve_is_the_bezier_through_its_control_points() {
    // x = y = 3s(1 - s)^2 * 0.1 + 3s^2(1 - s) * 0.9 + s^3 at s = 0.5 is 0.5.
    let c = curve([0.1, 0.1, 0.9, 0.9]);
    assert!((c.shape(0.5) - 0.5).abs() < 1e-12);
    // Fast first: at s = 0.5, x = 0.125, y = 0.875.
    assert!((curve([0.0, 1.0, 0.0, 1.0]).shape(0.125) - 0.875).abs() < 1e-9);
    assert_eq!(Curve::bend(0.0), Curve::STRAIGHT);
    assert_eq!(Curve::bend(1.0), curve([0.0, 1.0, 0.0, 1.0]));
    assert_eq!(Curve::bend(-0.5).bend_of(), Some(-0.5));
    assert_eq!(curve([0.1, 0.7, 0.825, 0.525]).bend_of(), None);
    assert_eq!(Curve::new(0.0, 1.1, 0.5, 0.5), None);
}

/// Splitting keeps the sound: the halves of a measured split.
#[test]
fn a_split_cuts_the_bezier_exactly() {
    let cut = curve([0.0859375, 0.7421875, 0.2578125, 0.9140625]).split(0.25).unwrap();
    assert!(close(cut.before, [0.170_339_6, 0.495_559_5, 0.425_088_0, 0.802_420_7]));
    assert!(close(cut.after, [0.195_092_2, 0.578_924_1, 0.500_788_1, 0.831_870_0]));
}

/// Measured hand drags: together from straight, apart for an S (stopping a step short of the
/// corners), and a second drag that chooses its mode again.
#[test]
fn a_drag_moves_the_control_points_along_their_paths() {
    for (start, steps, got) in [
        (Curve::STRAIGHT, &[47][..], [0.800_781_25, 0.066_406_25, 0.933_593_75, 0.199_218_75]),
        (curve([0.1, 0.7, 0.825, 0.525]), &[-25], [0.197_656_25, 0.407_031_25, 0.682_031_25, 0.772_656_25]),
        (curve([0.6, 0.0, 0.4, 1.0]), &[25], [0.990_625, 0.0, 0.009_375, 1.0]),
        (curve([0.225, 0.325, 0.45, 0.85]), &[22, 46], [0.971_875, 0.009_375, 0.168_75, 0.943_75]),
    ] {
        let end = steps.iter().fold(start, |c, &n| CurveDrag::new(&c).dragged(n));
        assert!(close(end, got), "{start:?} by {steps:?}: {end:?}");
    }
}

#[test]
fn the_volume_fader_is_40_db_per_travel_above_minus_18_db_and_falls_faster_below() {
    // The fader measured (docs/spec/automation.md › The volume fader), to 0.3 dB.
    for (position, db) in
        [(0.1, -48.6), (0.2, -34.4), (0.3, -24.2), (0.4, -18.0), (0.5, -14.0), (0.7, -6.0), (0.85, 0.0), (1.0, 6.0)]
    {
        assert!((volume_db(position) - db).abs() < 0.3, "{position}: {}", volume_db(position));
    }
    assert_eq!(volume_db(0.0), f64::NEG_INFINITY);
    assert_eq!(volume_db(f64::NAN), f64::NEG_INFINITY);
    assert_eq!(volume_position(f64::NEG_INFINITY), 0.0);
    assert_eq!(volume_position(-70.0), 0.0);
    assert_eq!(volume_position(12.0), 1.0);
    for position in [0.01, 0.05, 0.2, 0.39, 0.4, 0.41, 0.6, 0.85, 1.0] {
        assert!((volume_position(volume_db(position)) - position).abs() < 1e-9, "{position}");
    }
    // Even in decibels above the knee: a straight segment from 0 dB to -18 dB is at -9 dB
    // half-way, not at the half of its gain.
    let p = points(&[(0.0, 1.0), (2.0, 10f64.powf(-18.0 / 20.0))]);
    let middle = value_at(&p, 1.0, false, &VolumeTaper).unwrap_or(0.0);
    assert!((20.0 * middle.log10() + 9.0).abs() < 1e-6, "{}", 20.0 * middle.log10());
    // A silent end is the fader's bottom; a level segment keeps its value exactly.
    let p = points(&[(0.0, 1.0), (2.0, 0.0)]);
    assert!(value_at(&p, 1.999, false, &VolumeTaper).unwrap_or(1.0) < 0.001);
    let p = points(&[(0.0, 0.3), (2.0, 0.3)]);
    assert_eq!(value_at(&p, 1.0, false, &VolumeTaper), Some(0.3));
}
