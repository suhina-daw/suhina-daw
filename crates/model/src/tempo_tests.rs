// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn ts(numerator: u8, denominator: u8) -> TimeSignature {
    TimeSignature { numerator, denominator }
}

fn point(beat: f64, bpm: f64, ramp: bool) -> TempoPoint {
    TempoPoint { beat, bpm, ramp }
}

#[test]
fn a_constant_map_converts_exactly_as_before() {
    let rate = 48_000.0;
    let map = TempoMap::constant(123.0, rate);
    let per_beat = 60.0 / 123.0 * rate;
    for beat in [0.0, 0.25, 1.0, 7.3, 1234.5678] {
        // Bit for bit: golden renders depend on it.
        assert_eq!(map.time_at(beat), beat * per_beat);
        assert_eq!(map.span(3.0, beat), beat * per_beat);
        assert_eq!(map.beat_at(beat * 1000.0), beat * 1000.0 / per_beat);
    }
    assert!(map.is_constant());
    assert_eq!(map.bpm_at(50.0), 123.0);
}

#[test]
fn a_step_changes_the_tempo_at_its_beat() {
    // 120 bpm for 4 beats (2 s), then 60 bpm.
    let map = TempoMap::new(120.0, &[point(4.0, 60.0, false)], 1.0);
    assert_eq!(map.time_at(4.0), 2.0);
    assert_eq!(map.time_at(6.0), 4.0);
    assert_eq!(map.beat_at(4.0), 6.0);
    assert_eq!(map.bpm_at(3.999), 120.0);
    assert_eq!(map.bpm_at(4.0), 60.0);
    assert_eq!(map.span(3.0, 2.0), 1.5);
}

#[test]
fn a_ramp_matches_its_closed_form_and_round_trips() {
    // From 120 at beat 0 to 180 at beat 8: dt/db = 60 / (120 + 7.5 b).
    let map = TempoMap::new(120.0, &[point(8.0, 180.0, true)], 1.0);
    let expected = 60.0 / 7.5 * (180.0f64 / 120.0).ln();
    assert!((map.time_at(8.0) - expected).abs() < 1e-12);
    assert!((map.bpm_at(4.0) - 150.0).abs() < 1e-12);
    assert!(map.ramps_at(0.0) && map.ramps_at(7.9) && !map.ramps_at(8.0));
    assert_eq!(map.bpm_at(9.0), 180.0);
    // After the ramp, a constant 180 bpm.
    assert!((map.time_at(10.0) - (expected + 2.0 / 3.0)).abs() < 1e-12);
    for beat in [0.0, 0.5, 3.0, 7.99, 8.0, 12.0, 1000.0] {
        let back = map.beat_at(map.time_at(beat));
        assert!((back - beat).abs() < 1e-9, "{beat} came back as {back}");
    }
    // A downward ramp too.
    let map = TempoMap::new(140.0, &[point(4.0, 70.0, true)], 48_000.0);
    for beat in [0.1, 2.0, 3.9, 4.0, 5.0] {
        assert!((map.beat_at(map.time_at(beat)) - beat).abs() < 1e-9);
    }
}

#[test]
fn several_changes_add_up() {
    // 120 for 4 beats (2 s), 60 for 2 (2 s), then a ramp from 60 to 120 over 4 beats and a
    // step down to 30.
    let changes =
        [point(4.0, 60.0, false), point(10.0, 120.0, true), point(6.0, 60.0, false), point(12.0, 30.0, false)];
    let map = TempoMap::new(120.0, &changes, 1.0);
    let ramp = 60.0 / 15.0 * 2f64.ln();
    assert_eq!(map.time_at(6.0), 4.0);
    assert!((map.time_at(10.0) - (4.0 + ramp)).abs() < 1e-12);
    assert!((map.time_at(11.0) - (4.0 + ramp + 0.5)).abs() < 1e-12);
    assert!((map.time_at(13.0) - (4.0 + ramp + 1.0 + 2.0)).abs() < 1e-12);
    let mut last = -1.0;
    for i in 0..200 {
        let beat = f64::from(i) * 0.1;
        let time = map.time_at(beat);
        assert!(time > last, "time goes backwards at beat {beat}");
        assert!((map.beat_at(time) - beat).abs() < 1e-9);
        last = time;
    }
}

#[test]
fn a_ramp_can_end_in_a_jump() {
    // A glide from 120 to 180 over 8 beats that then jumps to 60.
    let map = TempoMap::new(120.0, &[point(8.0, 180.0, true), point(8.0, 60.0, false)], 1.0);
    let glide = 60.0 / 7.5 * 1.5f64.ln();
    assert!((map.time_at(8.0) - glide).abs() < 1e-12);
    assert!((map.time_at(9.0) - (glide + 1.0)).abs() < 1e-12);
    assert!((map.bpm_at(7.999) - 180.0).abs() < 0.01);
    assert_eq!(map.bpm_at(8.0), 60.0);
    assert!((map.beat_at(glide + 0.5) - 8.5).abs() < 1e-9);
    // A step then a ramp on one beat: the later wins, as for any other pair.
    let map = TempoMap::new(120.0, &[point(8.0, 60.0, false), point(8.0, 180.0, true)], 1.0);
    assert_eq!(map.breaks().count(), 1);
}

#[test]
fn a_long_ramp_does_not_drift() {
    // Summing a ramp frame by frame drifts; the closed form agrees with a fine numerical
    // integral to well under a frame after many minutes.
    let rate = 48_000.0;
    let map = TempoMap::new(60.0, &[point(1000.0, 200.0, true)], rate);
    let steps = 1_000_000;
    let mut frames = 0.0;
    for i in 0..steps {
        let b = (f64::from(i) + 0.5) * 1000.0 / f64::from(steps);
        frames += 1000.0 / f64::from(steps) * 60.0 / map.bpm_at(b) * rate;
    }
    assert!((map.time_at(1000.0) - frames).abs() < 0.01, "{} vs {frames}", map.time_at(1000.0));
}

#[test]
fn malformed_changes_are_ignored() {
    let changes = [
        point(f64::NAN, 100.0, false),
        point(-1.0, 100.0, false),
        point(0.0, 100.0, false),
        point(4.0, 0.0, false),
        point(4.0, f64::INFINITY, true),
        point(1e300, 100.0, false),
        point(8.0, 90.0, false),
        point(8.0, 60.0, false),
    ];
    let map = TempoMap::new(f64::NAN, &changes, f64::NAN);
    assert_eq!(map.start_bpm(), 120.0);
    assert_eq!(map.breaks().collect::<Vec<_>>(), vec![8.0]);
    assert_eq!((map.next_break_after(2.0), map.next_break_after(8.0)), (Some(8.0), None));
    assert_eq!(map.bpm_at(9.0), 60.0, "the later of two points on one beat wins");
    let many: Vec<TempoPoint> = (1..=20_000).map(|i| point(f64::from(i), 100.0, false)).collect();
    assert_eq!(TempoMap::new(120.0, &many, 1.0).breaks().count(), MAX_TEMPO_CHANGES);
}

#[test]
fn the_meter_counts_bars_through_changes() {
    // 4/4 for bars 0 and 1 (8 beats), 3/4 from bar 2, 6/8 from bar 4.
    let meter = Meter::new(
        ts(4, 4),
        &[
            TimeSignatureChange { bar: 4, time_signature: ts(6, 8) },
            TimeSignatureChange { bar: 2, time_signature: ts(3, 4) },
        ],
    );
    assert_eq!(meter.bar_start(2), 8.0);
    assert_eq!(meter.bar_start(3), 11.0);
    assert_eq!(meter.bar_start(4), 14.0);
    assert_eq!(meter.bar_start(6), 20.0);
    let p = meter.position(12.0);
    assert_eq!((p.bar, p.bar_start, p.bar_beats), (3, 11.0, 3.0));
    let p = meter.position(15.5);
    assert_eq!((p.bar, p.bar_start, p.time_signature), (4, 14.0, ts(6, 8)));
    assert_eq!(meter.position(-3.0).bar, 0);
    assert_eq!(meter.changes().map(|c| c.0).collect::<Vec<_>>(), vec![2, 4]);
}

#[test]
fn malformed_signature_changes_are_ignored() {
    let meter = Meter::new(
        ts(4, 4),
        &[
            TimeSignatureChange { bar: 0, time_signature: ts(3, 4) },
            TimeSignatureChange { bar: 2, time_signature: ts(0, 4) },
            TimeSignatureChange { bar: 3, time_signature: ts(3, 3) },
            TimeSignatureChange { bar: u32::MAX, time_signature: ts(5, 4) },
        ],
    );
    assert_eq!(meter.changes().count(), 1);
    assert_eq!(meter.position(1e300).bar, u32::MAX);
    assert!(ts(7, 8).is_valid() && !ts(100, 4).is_valid() && !ts(4, 128).is_valid());
    // Denominators stop at a sixteenth note.
    assert!(ts(3, 16).is_valid() && !ts(3, 32).is_valid() && !ts(3, 64).is_valid());
    assert_eq!(ts(0, 4).bar_beats(), 4.0);
}
