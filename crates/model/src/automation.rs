// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Automation envelopes: the value of an [`AutomationLane`] at a beat, and the mixer parameters a
//! channel's lanes can name. Pure functions, shared by the engine, the commands and the tests
//! (`docs/spec/automation.md`).

use crate::{AutomationLane, AutomationPoint, Channel};

/// The mixer parameters of a channel that a lane can automate: its volume (linear gain, 0 up to
/// [`MAX_VOLUME`]), its pan (-1 to 1) and its mute (0 or 1, a step).
pub const MIXER_PARAMS: [&str; 3] = [VOLUME, PAN, MUTE];
pub const VOLUME: &str = "volume";
pub const PAN: &str = "pan";
pub const MUTE: &str = "mute";
/// The loudest a volume lane goes: +6 dB, as a fader.
pub const MAX_VOLUME: f64 = 1.995_262_314_968_879_6;
/// Most points one lane holds.
pub const MAX_POINTS: usize = 65_536;

/// How far a segment of shape `curve` (-1 to 1) has moved at `t` (0 to 1) of its length, as a
/// share of the way from its first point's position to the next's.
///
/// The segment is a cubic Bezier from (0, 0) to (1, 1) in (share of the time, share of the way),
/// with one handle: its first inner control point is (x1, y1) and the second its mirror,
/// (1 - y1, 1 - x1). The handle runs along two lines that meet at (1/4, 1/4), where the segment
/// is straight: for a curve above 0 along y1 = 1 - 3 x1 to (0, 1), which moves fast first and
/// settles; below 0 along x1 = 1 - 3 y1 to (1, 0), which waits and moves late. The same curve
/// has the same shape going up and going down. Out-of-range and non-finite input is held in
/// range. Never allocates.
pub fn shape(t: f64, curve: f64) -> f64 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    let curve = if curve.is_finite() { curve.clamp(-1.0, 1.0) } else { 0.0 };
    if curve == 0.0 || t == 0.0 || t == 1.0 {
        return t;
    }
    let (x1, y1) = handle(curve);
    let (x2, y2) = (1.0 - y1, 1.0 - x1);
    // x(s) rises with s for every handle on the two lines, so halving finds the s of `t`.
    let along = |a: f64, b: f64, s: f64| {
        let r = 1.0 - s;
        3.0 * s * r * r * a + 3.0 * s * s * r * b + s * s * s
    };
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..48 {
        let mid = 0.5 * (lo + hi);
        if along(x1, x2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    along(y1, y2, 0.5 * (lo + hi)).clamp(0.0, 1.0)
}

/// The first inner control point of a segment of shape `curve` (-1 to 1, see [`shape`]).
fn handle(curve: f64) -> (f64, f64) {
    if curve >= 0.0 {
        ((1.0 - curve) / 4.0, (1.0 + 3.0 * curve) / 4.0)
    } else {
        ((1.0 - 3.0 * curve) / 4.0, (1.0 + curve) / 4.0)
    }
}

/// The curve (-1 to 1) whose segment has covered `share` of the way at `t` of its length: the
/// inverse of [`shape`] in its curve. `None` for a `t` or `share` at an end or not a number, or a
/// share no curve reaches there.
pub fn curve_through(t: f64, share: f64) -> Option<f64> {
    if !(t > 0.0 && t < 1.0 && share > 0.0 && share < 1.0) {
        return None;
    }
    let (mut lo, mut hi) = (-1.0f64, 1.0f64);
    if share < shape(t, lo) || share > shape(t, hi) {
        return None;
    }
    // `shape` rises with the curve at every `t` inside the segment.
    for _ in 0..48 {
        let mid = 0.5 * (lo + hi);
        if shape(t, mid) < share {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(0.5 * (lo + hi))
}

/// How a lane's values map onto its **position**, 0 to 1 over the travel of the parameter's
/// control (a fader, a knob). A segment moves evenly in position, not in value, so what a lane
/// draws straight is what plays.
pub trait Taper {
    /// Where `value` sits, 0 to 1.
    fn position(&self, value: f64) -> f64;
    /// The value at `position` (0 to 1).
    fn value(&self, position: f64) -> f64;
}

/// A lane whose position is its value: a segment moves evenly in value (pan, plug-in
/// parameters, a linear control).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Plain;

impl Taper for Plain {
    fn position(&self, value: f64) -> f64 {
        value
    }
    fn value(&self, position: f64) -> f64 {
        position
    }
}

/// The volume fader: a linear gain (0 to [`MAX_VOLUME`]) placed as the mixer's volume field
/// places it ([`volume_position`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VolumeTaper;

impl Taper for VolumeTaper {
    fn position(&self, value: f64) -> f64 {
        if value > 0.0 { volume_position(20.0 * value.log10()) } else { 0.0 }
    }
    fn value(&self, position: f64) -> f64 {
        let db = volume_db(position);
        if db.is_finite() { 10f64.powf(db / 20.0).min(MAX_VOLUME) } else { 0.0 }
    }
}

/// The volume fader's position at which its law changes: above it, 40 dB per travel.
const FADER_KNEE: f64 = 0.4;
/// Decibels per whole travel above the knee: 0 dB sits at 0.85, +6 dB at the top.
const FADER_SLOPE: f64 = 40.0;
/// How fast the fader falls below the knee: the drop added there, per square of the travel.
const FADER_BEND: f64 = 207.0;

/// The volume in decibels at fader position `position` (0 to 1): 40 dB per travel from -18 dB
/// at 0.4 to +6 dB at the top, with 0 dB at 0.85; below 0.4 it falls faster, by 207 times the
/// square of the distance under 0.4, to -67 dB just above the bottom. The bottom is silence
/// (negative infinity). Not a number counts as the bottom. `docs/spec/automation.md` › The
/// volume fader.
pub fn volume_db(position: f64) -> f64 {
    if position.is_nan() || position <= 0.0 {
        return f64::NEG_INFINITY;
    }
    let p = position.min(1.0);
    let line = FADER_SLOPE * (p - 0.85);
    if p >= FADER_KNEE { line } else { line - FADER_BEND * (FADER_KNEE - p) * (FADER_KNEE - p) }
}

/// Where a volume of `db` decibels sits on the fader, 0 to 1: the inverse of [`volume_db`].
/// Silence and anything below the fader's lowest level is 0; above +6 dB is 1.
pub fn volume_position(db: f64) -> f64 {
    if db.is_nan() || db == f64::NEG_INFINITY {
        return 0.0;
    }
    let knee_db = FADER_SLOPE * (FADER_KNEE - 0.85);
    if db >= knee_db {
        return (db / FADER_SLOPE + 0.85).min(1.0);
    }
    // db = knee_db - FADER_SLOPE u - FADER_BEND u^2, u = FADER_KNEE - p.
    let root = FADER_SLOPE * FADER_SLOPE + 4.0 * FADER_BEND * (knee_db - db);
    let u = (root.sqrt() - FADER_SLOPE) / (2.0 * FADER_BEND);
    (FADER_KNEE - u).max(0.0)
}

/// The value of `points` (in beat order) at `beat`: the first point's before it, the last's
/// after it, and along each segment's curve between, evenly in `taper`'s position; at a beat with
/// two points (a jump) the later one's. With `stepped`, a segment holds its first point's value
/// up to the next point (for on/off and choice parameters). `None` for no points.
pub fn value_at(points: &[AutomationPoint], beat: f64, stepped: bool, taper: &impl Taper) -> Option<f64> {
    let first = points.first()?;
    // Before the first point, and for a beat that is not a number.
    if beat.partial_cmp(&first.beat).is_none_or(|o| o == std::cmp::Ordering::Less) {
        return Some(first.value);
    }
    // The last point at or before `beat`.
    let index = points.partition_point(|p| p.beat <= beat).saturating_sub(1);
    let a = points.get(index)?;
    let Some(b) = points.get(index + 1) else { return Some(a.value) };
    let span = b.beat - a.beat;
    if stepped || span.is_nan() || span <= 0.0 {
        return Some(a.value);
    }
    let t = (beat - a.beat) / span;
    Some(along(a, b, t, taper))
}

/// The value `t` (0 to 1) of the way along the segment from `a` to `b`: evenly in `taper`'s
/// position, bent by `a`'s curve. A segment between equal values holds that value exactly.
pub fn along(a: &AutomationPoint, b: &AutomationPoint, t: f64, taper: &impl Taper) -> f64 {
    if a.value == b.value {
        return a.value;
    }
    let (from, to) = (taper.position(a.value), taper.position(b.value));
    let share = shape(t, a.curve);
    if share <= 0.0 {
        return a.value;
    }
    if share >= 1.0 {
        return b.value;
    }
    taper.value(from + (to - from) * share)
}

impl AutomationLane {
    /// An enabled lane of `param` with no points.
    pub fn new(param: impl Into<String>) -> Self {
        Self { param: param.into(), points: Vec::new(), enabled: true }
    }

    /// Puts a point in beat order, after any at the same beat, and returns its index.
    pub fn insert(&mut self, point: AutomationPoint) -> usize {
        let index = self.points.partition_point(|p| p.beat <= point.beat);
        self.points.insert(index, point);
        index
    }

    /// Puts back a point taken out of index `from`, in beat order, keeping its side of the
    /// points at its beat: one that comes from before them goes first, one from after them
    /// last, and one that stays at its beat keeps its place, so moving a point onto another's
    /// beat, or changing only its value, never turns a jump around. Returns its index.
    pub fn insert_near(&mut self, point: AutomationPoint, from: usize) -> usize {
        let first = self.points.partition_point(|p| p.beat < point.beat);
        let after = self.points.partition_point(|p| p.beat <= point.beat);
        let index = from.clamp(first, after);
        self.points.insert(index, point);
        index
    }

    /// The lane's value at `beat`, evenly in `taper`'s position (see [`value_at`]).
    pub fn value_at(&self, beat: f64, stepped: bool, taper: &impl Taper) -> Option<f64> {
        value_at(&self.points, beat, stepped, taper)
    }
}

impl AutomationPoint {
    /// A point on a straight segment.
    pub fn new(beat: f64, value: f64) -> Self {
        Self { beat, value, curve: 0.0 }
    }
}

impl Channel {
    /// The channel's lane for mixer parameter `param`, if it has one.
    pub fn lane(&self, param: &str) -> Option<&AutomationLane> {
        self.automation.iter().find(|l| l.param == param)
    }

    /// Whether `param` follows an enabled lane with points.
    pub fn automated(&self, param: &str) -> bool {
        self.lane(param).is_some_and(|l| l.enabled && !l.points.is_empty())
    }
}

/// The range a mixer parameter's values are kept in, `None` for a name that is not one.
pub fn mixer_range(param: &str) -> Option<(f64, f64)> {
    match param {
        VOLUME => Some((0.0, MAX_VOLUME)),
        PAN => Some((-1.0, 1.0)),
        MUTE => Some((0.0, 1.0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(list: &[(f64, f64)]) -> Vec<AutomationPoint> {
        list.iter().map(|&(beat, value)| AutomationPoint::new(beat, value)).collect()
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
    fn curves_bend_a_segment_both_ways() {
        assert_eq!(shape(0.25, 0.0), 0.25);
        assert!(shape(0.25, 1.0) > 0.5);
        assert!(shape(0.25, -1.0) < 0.01);
        assert_eq!(shape(1.0, 0.7), 1.0);
        assert_eq!(shape(f64::NAN, 1.0), 0.0);
        assert_eq!(shape(0.5, f64::INFINITY), 0.5, "a curve that is not a number is straight");
        let mut p = points(&[(0.0, 0.0), (1.0, 1.0)]);
        if let Some(first) = p.first_mut() {
            first.curve = 0.5;
        }
        assert!(value_at(&p, 0.5, false, &Plain).unwrap_or(0.0) > 0.5);
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

    #[test]
    fn a_curve_is_the_bezier_with_one_mirrored_handle() {
        // A direct cubic Bezier through control points (x1, y1) and (1 - y1, 1 - x1).
        fn bezier(x1: f64, y1: f64, t: f64) -> f64 {
            let (x2, y2) = (1.0 - y1, 1.0 - x1);
            let at = |a: f64, b: f64, s: f64| 3.0 * s * (1.0 - s).powi(2) * a + 3.0 * s * s * (1.0 - s) * b + s.powi(3);
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if at(x1, x2, mid) < t { lo = mid } else { hi = mid }
            }
            at(y1, y2, 0.5 * (lo + hi))
        }
        // Handles of both kinds, as other software stores them, and the two ends.
        for (curve, x1, y1) in [(1.0, 0.0, 1.0), (0.392, 0.152, 0.544), (-0.392, 0.544, 0.152), (-1.0, 1.0, 0.0)] {
            for t in [0.1, 0.3, 0.5, 0.7, 0.9] {
                let (want, got) = (bezier(x1, y1, t), shape(t, curve));
                assert!((want - got).abs() < 1e-6, "curve {curve} at {t}: {got} against {want}");
            }
        }
        // Straight at 0, and the same distance from straight either way at mirrored times.
        for t in [0.2, 0.5, 0.8] {
            assert!((shape(t, 0.0) - t).abs() < 1e-12);
            assert!((shape(t, 0.6) - (1.0 - shape(1.0 - t, -0.6))).abs() < 1e-9);
        }
        // It rises with the curve, and the inverse finds the curve back.
        let middles: Vec<f64> = [-1.0, -0.5, 0.0, 0.5, 1.0].iter().map(|&c| shape(0.5, c)).collect();
        assert!(middles.windows(2).all(|w| w[0] < w[1]), "{middles:?}");
        for curve in [-0.9, -0.3, 0.25, 0.8] {
            let back = curve_through(0.4, shape(0.4, curve)).unwrap_or(f64::NAN);
            assert!((back - curve).abs() < 1e-6, "{curve}: {back}");
        }
        assert_eq!(curve_through(0.0, 0.5), None);
        assert_eq!(curve_through(0.5, f64::NAN), None);
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
}
