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
    // A split's halves can lie outside the unit square; past the limits, or not a number, is
    // refused.
    assert_eq!(Curve::new(1.1958, 0.0, -0.2931, 1.0), Some(curve([1.1958, 0.0, -0.2931, 1.0])));
    assert_eq!(Curve::new(CONTROL_MIN, CONTROL_MAX, 0.5, 0.5), Some(curve([-0.5, 1.5, 0.5, 0.5])));
    for bad in [1.6, -0.6, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300] {
        assert_eq!(Curve::new(0.0, bad, 0.5, 0.5), None, "{bad}");
    }
}

/// Splitting keeps the sound: the halves of a measured split.
#[test]
fn a_split_cuts_the_bezier_exactly() {
    let cut = curve([0.0859375, 0.7421875, 0.2578125, 0.9140625]).split(0.25).unwrap();
    assert!(close(cut.before, [0.170_339_6, 0.495_559_5, 0.425_088_0, 0.802_420_7]));
    assert!(close(cut.after, [0.195_092_2, 0.578_924_1, 0.500_788_1, 0.831_870_0]));
}

/// The halves of curves whose points run back in time or in value lie outside the unit square,
/// unclamped, as measured (split-overshoot: to 3e-9 of exact de Casteljau).
#[test]
fn a_split_of_an_s_curve_leaves_the_square() {
    let (back_in_time, back_in_value) = (curve([1.0, 0.0, 0.0, 1.0]), curve([0.0, 1.0, 1.0, 0.0]));
    for (c, t, before, after) in [
        (
            back_in_time,
            0.75,
            [1.195_800_350_656_066_6, 0.0, 0.246_692_983_368_375_72, 0.828_981_543_588_751_4],
            [0.259_921_049_894_873_3, 0.642_051_826_708_326, 0.587_401_051_968_199_6, 1.0],
        ),
        (
            back_in_time,
            0.25,
            [0.412_598_948_031_800_5, 0.0, 0.740_078_950_105_126_8, 0.357_948_173_291_674_94],
            [0.753_307_016_631_624_4, 0.171_018_456_411_248_47, -0.195_800_350_656_066_38, 1.0],
        ),
        (
            back_in_value,
            0.75,
            [0.0, 1.293_128_413_857_272_3, 0.605_069_156_365_301_5, 0.844_029_628_745_985_5],
            [0.573_977_952_239_538_1, 0.082_167_617_165_228_07, 1.0, 0.318_759_841_365_908_96],
        ),
        (
            back_in_value,
            0.25,
            [0.0, 0.681_240_158_634_091, 0.426_022_047_760_461_8, 0.917_832_382_834_772],
            [0.394_930_843_634_698_5, 0.155_970_371_254_014_53, 1.0, -0.293_128_413_857_272_23],
        ),
    ] {
        let cut = c.split(t).unwrap();
        let near = |a: Curve, b: [f64; 4]| a.to_array().iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-9);
        assert!(near(cut.before, before) && near(cut.after, after), "{c:?} at {t}: {cut:?}");
        // What a split gives, a file and a command take back.
        assert_eq!(Curve::from_array(cut.before.to_array()), Some(cut.before));
        assert_eq!(Curve::from_array(cut.after.to_array()), Some(cut.after));
    }
}

/// Splitting a curve of the square again and again keeps every coordinate from -1/3 to 4/3 (the
/// first point's from 0, the second's up to 1) and the time share rising, so each half stays a
/// function of time and plays as its piece of the whole.
#[test]
fn repeated_splits_stay_within_the_reachable_range_and_keep_time_running_forward() {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut furthest = (0.0f64, 1.0f64);
    for _ in 0..2000 {
        let mut c = curve([random(), random(), random(), random()]);
        if random() < 0.3 {
            // The corners, where the overshoot is largest.
            c = curve([1.0, 0.0, 0.0, 1.0]);
        }
        for _ in 0..6 {
            let cut = c.split(0.02 + 0.96 * random()).unwrap();
            for half in [cut.before, cut.after] {
                let [x1, y1, x2, y2] = half.to_array();
                for v in [x1, y1] {
                    assert!((-1e-9..=4.0 / 3.0 + 1e-6).contains(&v), "{half:?} from {c:?}");
                }
                for v in [x2, y2] {
                    assert!((-1.0 / 3.0 - 1e-6..=1.0 + 1e-9).contains(&v), "{half:?} from {c:?}");
                }
                furthest = (furthest.0.min(x2.min(y2)), furthest.1.max(x1.max(y1)));
                let mut last = 0.0;
                for q in 1..=256 {
                    let x = cubic(half.x1, half.x2, q as f64 / 256.0);
                    assert!(x >= last - 1e-9, "time runs back in {half:?}");
                    last = x;
                }
            }
            c = if random() < 0.5 { cut.before } else { cut.after };
        }
    }
    // The range is reached, not only allowed.
    assert!(furthest.0 < -0.25 && furthest.1 > 1.25, "{furthest:?}");
}

/// A curve inside the limits whose time share falls, as a hostile file can hold, still plays:
/// at the first moment its time share reaches t, between the segment's two points, never a
/// panic.
#[test]
fn a_curve_whose_time_runs_back_plays_at_the_first_moment_it_gets_there() {
    for c in [
        curve([1.5, 0.0, -0.5, 1.0]),
        curve([1.5, 1.5, -0.5, -0.5]),
        curve([-0.5, 1.5, 1.5, -0.5]),
        curve([1.5, -0.5, 1.5, -0.5]),
        curve([CONTROL_MIN, CONTROL_MIN, CONTROL_MIN, CONTROL_MIN]),
    ] {
        let mut last = 0.0;
        for q in 0..=1000 {
            let t = q as f64 / 1000.0;
            let v = c.shape(t);
            assert!((0.0..=1.0).contains(&v), "{c:?} at {t}: {v}");
            let s = c.parameter(t);
            if t > 0.0 {
                assert!(s >= last - 1e-12, "{c:?} at {t}: the parameter went back");
                assert!(cubic(c.x1, c.x2, s) >= t - 1e-9);
                let earlier = (0..1000).map(|i| s * i as f64 / 1000.0).all(|e| cubic(c.x1, c.x2, e) < t + 1e-9);
                assert!(earlier, "{c:?} at {t}: not the first moment");
            }
            last = s;
        }
        for t in [f64::NAN, f64::INFINITY, -1.0, 2.0] {
            assert!(c.shape(t).is_finite());
        }
        for t in [1e-12, 0.3, 0.5, 1.0 - 1e-12] {
            // A cut past the curve's own ends is refused; any other keeps both halves valid.
            if let Some(cut) = c.split(t) {
                for half in [cut.before, cut.after] {
                    assert_eq!(Curve::from_array(half.to_array()), Some(half), "{c:?} at {t}");
                }
            }
        }
    }
}

/// A drag first holds each coordinate of a point outside the square in it on its own, then
/// moves along the paths from the held curve (measured: split-overshoot-drag); a drag of no
/// steps still holds.
#[test]
fn a_drag_holds_a_split_half_in_the_square_first() {
    for (start, steps, got) in [
        (
            [1.195_800_352_596_018_6, 0.0, 0.246_692_986_530_421_18, 0.828_981_542_001_562_4],
            4,
            [1.0, 0.0, 0.209_781_592_648_116_16, 0.854_570_148_119_257_4],
        ),
        (
            [0.753_307_013_469_578_7, 0.171_018_457_998_437_65, -0.195_800_352_596_018_66, 1.0],
            3,
            [0.780_990_558_881_307_5, 0.151_827_003_410_166_44, 0.0, 1.0],
        ),
        (
            [0.0, 1.293_128_414_787_242_8, 0.605_069_157_031_980_2, 0.844_029_626_998_212_5],
            3,
            [0.011_718_75, 0.964_843_75, 0.642_337_413_957_629_8, 0.834_422_883_923_862_1],
        ),
        (
            [0.394_930_842_968_019_9, 0.155_970_373_001_787_54, 1.0, -0.293_128_414_787_242_9],
            -3,
            [0.357_662_586_042_370_17, 0.165_577_116_076_137_98, 0.988_281_25, 0.035_156_25],
        ),
        (
            [1.195_800_352_596_018_6, 0.0, 0.246_692_986_530_421_18, 0.828_981_542_001_562_4],
            -1,
            [0.988_281_25, 0.003_906_25, 0.255_920_835_000_997, 0.822_584_390_472_138_4],
        ),
        (
            [0.753_307_013_469_578_7, 0.171_018_457_998_437_65, -0.195_800_352_596_018_66, 1.0],
            -4,
            [0.716_395_619_587_273_7, 0.196_607_064_116_132_6, 0.046_875, 0.984_375],
        ),
        (
            [0.0, 1.293_128_414_787_242_8, 0.605_069_157_031_980_2, 0.844_029_626_998_212_5],
            -9,
            [0.0, 1.0, 0.493_264_386_255_031_44, 0.872_849_856_221_263_8],
        ),
        (
            [0.394_930_842_968_019_9, 0.155_970_373_001_787_54, 1.0, -0.293_128_414_787_242_9],
            4,
            [0.444_621_852_202_219_2, 0.143_161_382_235_987, 1.0, 0.0],
        ),
    ] {
        let start = curve(start);
        let end = CurveDrag::new(&start).dragged(steps);
        assert!(close(end, got), "{start:?} by {steps}: {end:?}");
        // Less than a step: only the hold.
        let held = CurveDrag::new(&start).dragged(0);
        assert!(close(held, start.boxed().to_array()), "{start:?}: {held:?}");
        assert!(held.to_array().iter().all(|v| (0.0..=1.0).contains(v)));
    }
    assert_eq!(curve([1.1958, 0.0, -0.2931, 1.0]).boxed(), curve([1.0, 0.0, 0.0, 1.0]));
    assert_eq!(curve([0.0, 1.2931, 0.6, 0.8]).boxed(), curve([0.0, 1.0, 0.6, 0.8]));
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

/// A segment of any bend, a lopsided one or an S whose points run back, split anywhere (and its
/// halves split again) plays as it did, and a straight one splits into straight halves; a cut at
/// an end is no cut.
#[test]
fn a_split_segment_plays_as_the_whole_did() {
    let mut curves: Vec<Curve> = [-1.0, -0.6, -0.4, 0.0, 0.3, 0.6, 1.0].into_iter().map(Curve::bend).collect();
    curves.extend(
        [
            [0.1, 0.7, 0.825, 0.525],
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 1.0, 0.0],
            [0.9, 0.1, 0.2, 0.95],
            [1.0, 1.0, 0.0, 0.0],
            [0.6, 0.0, 0.4, 1.0],
            [1.0, 0.0, 1.0, 0.0],
        ]
        .map(curve),
    );
    let mut worst = 0.0f64;
    for c in curves {
        for t in [0.01, 0.125, 0.25, 0.375, 0.5, 0.75, 0.99] {
            let cut = c.split(t).unwrap();
            let whole = [AutomationPoint { beat: 0.0, value: 0.0, curve: c }, AutomationPoint::new(8.0, 1.0)];
            let halves = [
                AutomationPoint { beat: 0.0, value: 0.0, curve: cut.before },
                AutomationPoint { beat: 8.0 * t, value: cut.share, curve: cut.after },
                AutomationPoint::new(8.0, 1.0),
            ];
            // The second half split again at its middle.
            let again = cut.after.split(0.5).unwrap();
            let quarters = [
                halves[0],
                AutomationPoint { curve: again.before, ..halves[1] },
                AutomationPoint {
                    beat: 4.0 * t + 4.0,
                    value: cut.share + (1.0 - cut.share) * again.share,
                    curve: again.after,
                },
                halves[2],
            ];
            for beat in (0..=1024).map(|q| q as f64 / 128.0) {
                let old = value_at(&whole, beat, false, &Plain).unwrap();
                // Where the time share stops for an instant (an S whose points run back, at its
                // middle) the line is upright: the value moves as the cube root of the time, so
                // the rounding of a double moves it by about 1e-5. Elsewhere the halves agree to 1e-12.
                let at = |b: f64| value_at(&whole, b, false, &Plain).unwrap();
                let upright = (at(beat + 1e-13) - at(beat - 1e-13)).abs();
                for split in [&halves[..], &quarters[..]] {
                    let new = value_at(split, beat, false, &Plain).unwrap();
                    let off = (old - new).abs();
                    if upright < 1e-9 {
                        worst = worst.max(off);
                    }
                    assert!(off < 1e-12 + 2.0 * upright, "{c:?} at {t}, beat {beat}: {old} against {new}");
                }
            }
        }
    }
    assert!(worst < 1e-12, "{worst}");
    let straight = Curve::STRAIGHT.split(0.3);
    assert_eq!(straight, Some(Split { share: 0.3, before: Curve::STRAIGHT, after: Curve::STRAIGHT }));
    assert_eq!(
        (Curve::bend(0.5).split(0.0), Curve::bend(0.5).split(1.0), Curve::bend(0.5).split(f64::NAN)),
        (None, None, None)
    );
}
