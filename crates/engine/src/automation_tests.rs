// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_model::AutomationPoint;
use suhina_model::automation::volume_db;

fn lane(param: &str, points: &[(f64, f64)]) -> AutomationLane {
    AutomationLane {
        param: param.into(),
        points: points.iter().map(|&(beat, value)| AutomationPoint::new(beat, value)).collect(),
        enabled: true,
    }
}

#[test]
fn an_envelope_is_built_from_an_enabled_lane_with_finite_points() {
    let mut l = lane("gain", &[(1.0, 2.0), (f64::NAN, 1.0), (0.5, f64::INFINITY), (2.0, 4.0)]);
    let envelope = Envelope::new(&l, 100.0, false, Position::Value).unwrap();
    assert_eq!(envelope.value_at(0), 2.0);
    assert_eq!(envelope.value_at(150), 3.0);
    assert_eq!(envelope.value_at(u64::MAX), 4.0);
    let stepped = Envelope::new(&l, 100.0, true, Position::Value).unwrap();
    assert_eq!(stepped.value_at(199), 2.0);
    l.enabled = false;
    assert!(Envelope::new(&l, 100.0, false, Position::Value).is_none());
    assert!(Envelope::new(&lane("gain", &[(f64::NAN, 1.0)]), 100.0, false, Position::Value).is_none());
}

#[test]
fn device_changes_fall_on_the_arrangements_grid_and_at_points() {
    let device = suhina_device_registry::create("utility").unwrap();
    let mut lanes = DeviceLanes::none();
    // 1 beat = 100 frames; a ramp from frame 100 to 200 and a jump at 300.
    lanes.add(0, device.as_ref(), &[lane("gain", &[(1.0, 0.0), (2.0, -10.0), (3.0, -10.0), (3.0, -20.0)])], 100.0);
    lanes.add(0, device.as_ref(), &[lane("no-such-param", &[(0.0, 1.0)])], 100.0);
    assert!(lanes.covers(0, 0) && !lanes.covers(1, 0));
    lanes.plan(None, 512);
    assert_eq!(lanes.changes(0, 0, 512).count(), 0, "stopped: nothing");
    // From frame 40: the first frame, then the grid of the arrangement (64, 128, ...) where
    // the value moves, and the points.
    lanes.plan(Some(40), 512);
    let frames: Vec<usize> = lanes.changes(0, 0, 512).map(|c| c.frame).collect();
    assert_eq!(frames, [0, 88, 152, 160, 260]);
    let last = lanes.changes(0, 0, 512).last().map(|c| c.value);
    assert_eq!(last, Some(-20.0));
    // The same block again: only what changed (nothing, the value holds after the jump).
    lanes.plan(Some(552), 512);
    assert_eq!(lanes.changes(0, 0, 512).count(), 0);
    assert_eq!(lanes.changes(0, 0, 100).count(), 0);
}

#[test]
fn a_device_lane_moves_evenly_along_its_parameters_knob() {
    let device = suhina_device_registry::create("utility").unwrap();
    let info = device.info();
    let at_middle = |param: &str, from: f64, to: f64| {
        let mut lanes = DeviceLanes::none();
        lanes.add(0, device.as_ref(), &[lane(param, &[(0.0, from), (2.0, to)])], 100.0);
        lanes.plan(Some(100), 1);
        lanes.changes(0, 0, 1).next().map(|c| c.value).unwrap_or(f64::NAN)
    };
    // A logarithmic knob: half-way from 50 Hz to 500 Hz is the middle of the knob, 158 Hz.
    let middle = at_middle("bassMonoFrequency", 50.0, 500.0);
    assert!((middle - (50.0f64 * 500.0).sqrt()).abs() < 0.01, "{middle}");
    // The gain knob: half-way between two settings is the middle of the knob, not of the dB.
    let gain = info.params.iter().find(|p| p.id == "gain").copied().unwrap();
    let want = gain.from_normalized((gain.to_normalized(-60.0) + gain.to_normalized(0.0)) / 2.0);
    let middle = at_middle("gain", -60.0, 0.0);
    assert!((middle - want).abs() < 1e-9 && middle > -20.0, "{middle} against {want}");
}

/// A whole-number parameter takes the lane's value with its fraction cut off toward zero
/// (`docs/spec/automation.md` › Lanes and points): not rounded, not floored.
#[test]
fn a_whole_number_lane_cuts_its_fraction_toward_zero() {
    // Eight frames a beat: one frame is the measurement's 0.125-beat step.
    let transpose = ParamInfo::integer("t", "T", (-48, 48), 0, suhina_device_api::Unit::Semitones);
    let read = |points: &[(f64, f64)], beat: f64| {
        let envelope = Envelope::new(&lane("t", points), 8.0, false, Position::Param(transpose)).unwrap();
        envelope.value_at((beat * 8.0) as u64)
    };
    let through_zero = [(0.0, 3.0), (12.0, -3.0)];
    // 2.94 at the first step plays 2; 0.94 at beat 4.125 plays 0, until -1.0 at beat 8.
    let plays: Vec<f64> =
        [0.0, 0.125, 4.0, 4.125, 6.0, 7.875, 8.0, 11.875, 12.0].iter().map(|&b| read(&through_zero, b)).collect();
    assert_eq!(plays, [3.0, 2.0, 1.0, 0.0, 0.0, 0.0, -1.0, -2.0, -3.0]);
    // Twice as long on 0 as on any other number less one step: 0.94 to -0.94 against 1.94 to 1.
    let steps_on = |n: f64| (0..=96).filter(|&s| read(&through_zero, f64::from(s) / 8.0) == n).count();
    assert_eq!((steps_on(0.0), steps_on(1.0), steps_on(-1.0)), (31, 16, 16));
    // Nothing is rounded to the nearest: 40 to 41 stays at 40 until the last point, 41 to 40
    // plays 40 from the first step, and 40.5 held plays 40.
    assert_eq!(read(&[(0.0, 40.0), (8.0, 41.0)], 7.875), 40.0);
    assert_eq!(read(&[(0.0, 40.0), (8.0, 41.0)], 8.0), 41.0);
    assert_eq!(read(&[(0.0, 41.0), (8.0, 40.0)], 0.125), 40.0);
    assert_eq!(read(&[(0.0, 40.0), (8.0, 40.5)], 8.0), 40.0);
    assert_eq!(read(&[(0.0, 40.5)], 3.0), 40.0);

    // What a device gets: the pitch MIDI effect's semitones, cut toward zero.
    let device = suhina_device_registry::create("pitch").unwrap();
    let mut lanes = DeviceLanes::none();
    lanes.add(0, device.as_ref(), &[lane("pitch", &[(0.0, 3.0), (12.0, -3.0)])], 8.0);
    let index = device.info().param_index("pitch").unwrap();
    let mut got = |frame: u64| {
        lanes.plan(Some(frame), 1);
        lanes.changes(0, 0, 1).find(|c| c.param == index).map(|c| c.value)
    };
    assert_eq!(got(33), Some(0.0), "0.94 at beat 4.125 plays 0");
    assert_eq!(got(79), Some(-1.0), "-1.94 at beat 9.875 plays -1, not -2");
}

#[test]
fn hostile_device_lanes_stay_within_bounds() {
    let device = suhina_device_registry::create("utility").unwrap();
    let mut lanes = DeviceLanes::none();
    let many: Vec<(f64, f64)> = (0..5_000).map(|i| (f64::from(i) * 0.001, f64::from(i % 2) * -10.0)).collect();
    for slot in 0..100 {
        lanes.add(slot, device.as_ref(), &[lane("gain", &many), lane("width", &[(1e300, 0.5), (-5.0, 1.0)])], 22_050.0);
    }
    for time in [0, 1, u64::MAX / 2, u64::MAX - 10] {
        lanes.plan(Some(time), 1024);
        assert!(lanes.block.len() <= MAX_EVENTS);
        assert!(lanes.block.windows(2).all(|w| w[0].frame <= w[1].frame));
        assert!(lanes.block.iter().all(|c| c.frame < 1024 && c.value.is_finite()));
    }
}

fn channel(lanes: Vec<AutomationLane>) -> Channel {
    Channel { automation: lanes, ..Channel::new(suhina_model::ChannelId(1), "C") }
}

#[test]
fn mixer_gains_are_a_line_between_grid_values_in_any_block() {
    // The volume moves evenly along the fader, from its bottom (silence) to 0 dB (0.85).
    let c = channel(vec![lane("volume", &[(0.0, 0.0), (10.0, 1.0)]), lane("pan", &[(0.0, 0.0)])]);
    // No smoothing (a sample rate of 0): the gains are the lane's own.
    let mut mix = MixLanes::new(&c, 100.0, true, 0.0).unwrap();
    let at = |frame: f64| VolumeTaper.value(0.85 * (frame / 1000.0).min(1.0)) as f32;
    mix.plan(0, 1024, true);
    let whole: Vec<(f32, f32)> = (0..1024).map(|f| mix.gains(f)).collect();
    // At each step of the grid the lane's value, in a straight line between, centred.
    for (f, g) in whole.iter().enumerate().take(960) {
        let grid = (f - f % MIX_STEP) as f64;
        let x = (f % MIX_STEP) as f32 / MIX_STEP as f32;
        let want = at(grid) + (at(grid + MIX_STEP as f64) - at(grid)) * x;
        assert!((g.0 - want).abs() < 1e-6 && (g.0 - g.1).abs() < 1e-6, "frame {f}: {g:?} against {want}");
    }
    // In pieces of 333 frames: the same.
    for start in (0..1024).step_by(333) {
        mix.plan(start as u64, 333, true);
        for f in 0..333.min(1024 - start) {
            assert_eq!(mix.gains(f), whole[start + f], "frame {}", start + f);
        }
    }
    // Stopped: the value at the play start marker throughout.
    mix.plan(500, 64, false);
    assert!((0..64).all(|f| mix.gains(f) == whole[500]));
    // Latency compensation reads the lanes earlier.
    mix.set_latency(100);
    mix.plan(500, 1, true);
    assert_eq!(mix.gains(0), whole[400]);
}

#[test]
fn a_volume_jump_is_an_s_shaped_ramp_centred_on_it_and_a_straight_segment_is_left_alone() {
    // 0 dB, a jump to -18 dB at frame 10,000 (beat 100), then a straight segment to -6 dB.
    let db = |d: f64| 10f64.powf(d / 20.0);
    let c = channel(vec![lane("volume", &[(0.0, 1.0), (100.0, 1.0), (100.0, db(-18.0)), (200.0, db(-6.0))])]);
    let mut mix = MixLanes::new(&c, 100.0, true, 48_000.0).unwrap();
    let length = (VOLUME_SMOOTHING * 48_000.0).round();
    let level = |mix: &mut MixLanes, frame: u64| {
        mix.plan(frame, 1, true);
        20.0 * f64::from(mix.gains(0).0).log10()
    };
    // Untouched outside the ramp; half-way in the fader's position at the jump.
    assert!(level(&mut mix, 10_000 - length as u64).abs() < 1e-6);
    let middle = volume_db((model::volume_position(0.0) + model::volume_position(-18.0)) / 2.0);
    assert!((level(&mut mix, 10_000) - middle).abs() < 0.05, "{}", level(&mut mix, 10_000));
    // An S: a quarter of the ramp in, much less than a quarter of the way down.
    let quarter = level(&mut mix, 10_000 - (length / 4.0) as u64);
    assert!(quarter > -18.0 * 0.15 && quarter < 0.0, "{quarter}");
    // Past the ramp, on the straight segment, the lane's own value.
    let frame = 15_000u64;
    let want = 20.0
        * VolumeTaper
            .value(model::volume_position(-18.0) + (model::volume_position(-6.0) - model::volume_position(-18.0)) * 0.5)
            .log10();
    assert!((level(&mut mix, frame) - want).abs() < 1e-3, "{} against {want}", level(&mut mix, frame));
}

#[test]
fn a_mute_lane_silences_and_pan_follows_only_where_it_applies() {
    let c = channel(vec![lane("mute", &[(0.0, 1.0), (1.0, 0.0)]), lane("pan", &[(0.0, 1.0)])]);
    let mut mix = MixLanes::new(&c, 64.0, true, 48_000.0).unwrap();
    mix.plan(0, 256, true);
    assert_eq!(mix.gains(0), (0.0, 0.0));
    let (l, r) = mix.gains(200);
    assert!(l.abs() < 1e-6 && (r - std::f32::consts::SQRT_2).abs() < 1e-5);
    // On a group's bus the pan does not apply.
    let mut bus = MixLanes::new(&c, 64.0, false, 48_000.0).unwrap();
    bus.plan(0, 256, true);
    assert_eq!(bus.gains(200), (1.0, 1.0));
    // No lane that plays: none.
    assert!(MixLanes::new(&channel(vec![]), 64.0, true, 48_000.0).is_none());
    let mut off = lane("volume", &[(0.0, 0.5)]);
    off.enabled = false;
    assert!(MixLanes::new(&channel(vec![off]), 64.0, true, 48_000.0).is_none());
}

#[test]
fn a_neutral_project_has_the_automated_mixer_parameters_at_rest() {
    let mut project = Project::default();
    assert!(neutral(&project).is_none());
    let mut c = channel(vec![lane("volume", &[(0.0, 0.5)]), lane("mute", &[(0.0, 0.0)])]);
    c.volume = 0.2;
    c.pan = 0.5;
    c.mute = true;
    project.channels.push(c);
    let n = neutral(&project).unwrap();
    let c = &n.channels[0];
    assert_eq!((c.volume, c.pan, c.mute), (1.0, 0.5, false));
}

/// An S whose points run back, split so its halves lie outside the unit square, plays as the
/// whole did, frame by frame; a curve whose time runs back (a hostile file) plays finite values
/// between its points.
#[test]
fn a_split_s_curve_plays_as_the_whole_did() {
    use suhina_model::automation::Curve;
    for [x1, y1, x2, y2] in [[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 1.0, 0.0]] {
        let c = Curve { x1, y1, x2, y2 };
        let mut whole = lane("gain", &[(0.0, -1.0), (8.0, 1.0)]);
        whole.points[0].curve = c;
        let mut split = whole.clone();
        // Cut at 0.75, then the first half again at a third of it (beat 2).
        let cut = c.split(0.75).unwrap();
        assert!(cut.before.to_array().iter().any(|v| !(0.0..=1.0).contains(v)), "{cut:?}");
        let again = cut.before.split(1.0 / 3.0).unwrap();
        split.points[0].curve = again.before;
        split
            .points
            .insert(1, AutomationPoint { beat: 2.0, value: -1.0 + 2.0 * cut.share * again.share, curve: again.after });
        split.points.insert(2, AutomationPoint { beat: 6.0, value: -1.0 + 2.0 * cut.share, curve: cut.after });
        let (old, new) = (
            Envelope::new(&whole, 1000.0, false, Position::Value).unwrap(),
            Envelope::new(&split, 1000.0, false, Position::Value).unwrap(),
        );
        for frame in 0..=8000 {
            // Frame 4000 is the S's upright middle, where rounding alone moves the value by 1e-5.
            let tolerance = if frame == 4000 { 1e-4 } else { 1e-9 };
            let (a, b) = (old.value_at(frame), new.value_at(frame));
            assert!((a - b).abs() < tolerance, "{c:?} at frame {frame}: {a} against {b}");
        }
    }
    let mut hostile = lane("gain", &[(0.0, -1.0), (8.0, 1.0)]);
    hostile.points[0].curve = Curve { x1: 1.5, y1: -0.5, x2: -0.5, y2: 1.5 };
    let envelope = Envelope::new(&hostile, 1000.0, false, Position::Value).unwrap();
    assert!((0..=8000).all(|f| (-1.0..=1.0).contains(&envelope.value_at(f))));
}
