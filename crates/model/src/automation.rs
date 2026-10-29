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

/// The shape of a segment: a cubic Bezier from (0, 0) to (1, 1) in (share of the segment's time,
/// share of the way from its first point's position to the next's), through two inner control
/// points (x1, y1) and (x2, y2) (`docs/spec/automation.md` › Lanes and points). With both
/// control points on the diagonal the segment is straight.
///
/// Bending and dragging keep the control points in the unit square; with x1 and x2 in it the
/// time share never falls, so the curve is a function of time, and with y1 and y2 in it the value
/// stays between the segment's two points. A split keeps that shape but not the square: a half of
/// a curve whose points run back can have a coordinate from -1/3 to 4/3 ([`Curve::split`]). Each
/// coordinate is kept from [`CONTROL_MIN`] to [`CONTROL_MAX`]; a curve inside those limits whose
/// time share falls still plays, as [`Curve::shape`] says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Curve {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

impl Default for Curve {
    fn default() -> Self {
        Self::STRAIGHT
    }
}

/// How close to the diagonal or a corner a control point counts as on it.
const NEAR: f64 = 1e-9;

/// The lowest a control point's coordinate goes. A split of a curve in the unit square reaches
/// -1/3 at most, however often it is repeated; the rest is room for rounding.
/// How far rounding may take the share of the way at a cut past an end before it is refused.
const SLIVER: f64 = 1e-9;

pub const CONTROL_MIN: f64 = -0.5;
/// The highest a control point's coordinate goes: a split reaches 4/3 at most.
pub const CONTROL_MAX: f64 = 1.5;

impl Curve {
    /// A straight segment: the mirrored family's middle, (1/4, 1/4) and (3/4, 3/4).
    pub const STRAIGHT: Self = Self { x1: 0.25, y1: 0.25, x2: 0.75, y2: 0.75 };

    /// The curve through control points (x1, y1) and (x2, y2), or `None` when one is not a
    /// number from [`CONTROL_MIN`] to [`CONTROL_MAX`].
    pub fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Option<Self> {
        [x1, y1, x2, y2].iter().all(|v| (CONTROL_MIN..=CONTROL_MAX).contains(v)).then_some(Self { x1, y1, x2, y2 })
    }

    /// The curve as `[x1, y1, x2, y2]`.
    pub fn to_array(self) -> [f64; 4] {
        [self.x1, self.y1, self.x2, self.y2]
    }

    /// The curve of `[x1, y1, x2, y2]`, as [`Curve::new`].
    pub fn from_array([x1, y1, x2, y2]: [f64; 4]) -> Option<Self> {
        Self::new(x1, y1, x2, y2)
    }

    /// The curve with each coordinate held from [`CONTROL_MIN`] to [`CONTROL_MAX`]; one that is
    /// not a number makes the curve straight. For curves from outside, where a refusal is not
    /// wanted.
    pub fn held(self) -> Self {
        self.held_in(CONTROL_MIN, CONTROL_MAX)
    }

    /// The curve with each coordinate held from 0 to 1 on its own, as a drag starts
    /// (`docs/spec/automation.md` › Dragging a curve); one that is not a number makes the curve
    /// straight.
    pub fn boxed(self) -> Self {
        self.held_in(0.0, 1.0)
    }

    fn held_in(self, lo: f64, hi: f64) -> Self {
        let [x1, y1, x2, y2] = self.to_array();
        if [x1, y1, x2, y2].iter().any(|v| v.is_nan()) {
            return Self::STRAIGHT;
        }
        Self { x1: x1.clamp(lo, hi), y1: y1.clamp(lo, hi), x2: x2.clamp(lo, hi), y2: y2.clamp(lo, hi) }
    }

    /// Whether the segment is straight: both control points on the diagonal.
    pub fn is_straight(&self) -> bool {
        (self.x1 - self.y1).abs() < NEAR && (self.x2 - self.y2).abs() < NEAR
    }

    /// A curve of the **mirrored family**, the one bending a straight segment gives: one handle
    /// `bend`, -1 to 1, with the second control point the first's mirror, (1 - y1, 1 - x1). 0 is
    /// straight; above 0 the first point runs along y1 = 1 - 3 x1 to (0, 1) at 1, which moves fast
    /// first and settles; below 0 along x1 = 1 - 3 y1 to (1, 0) at -1, which waits and moves late.
    /// Out-of-range input is held in range; not a number is straight.
    pub fn bend(bend: f64) -> Self {
        let c = if bend.is_nan() { 0.0 } else { bend.clamp(-1.0, 1.0) };
        let (x1, y1) =
            if c >= 0.0 { ((1.0 - c) / 4.0, (1.0 + 3.0 * c) / 4.0) } else { ((1.0 - 3.0 * c) / 4.0, (1.0 + c) / 4.0) };
        Self { x1, y1, x2: 1.0 - y1, y2: 1.0 - x1 }
    }

    /// The bend of a curve of the mirrored family (see [`Curve::bend`]): 0 for a straight one,
    /// `None` for a curve outside the family.
    pub fn bend_of(&self) -> Option<f64> {
        if self.is_straight() {
            return Some(0.0);
        }
        let mirrored = (self.x2 - (1.0 - self.y1)).abs() < NEAR && (self.y2 - (1.0 - self.x1)).abs() < NEAR;
        let (x1, y1) = (self.x1, self.y1);
        if !mirrored {
            None
        } else if x1 <= 0.25 + NEAR && (y1 - (1.0 - 3.0 * x1)).abs() < NEAR {
            Some((1.0 - 4.0 * x1).clamp(0.0, 1.0))
        } else if y1 <= 0.25 + NEAR && (x1 - (1.0 - 3.0 * y1)).abs() < NEAR {
            Some((4.0 * y1 - 1.0).clamp(-1.0, 0.0))
        } else {
            None
        }
    }

    /// The Bezier's parameter at which its time share first reaches `t` (0 to 1). The time share
    /// rises and falls in at most three runs, between the zeros of its slope; the first run that
    /// reaches `t` holds it, and halving finds it there. For a curve whose time share never falls
    /// (every curve bending, dragging and splitting give) that is the one parameter with that
    /// share; for another it is the first, so the curve is still a function of time. Never
    /// allocates.
    fn parameter(&self, t: f64) -> f64 {
        let x = |s: f64| cubic(self.x1, self.x2, s);
        // The slope over 3: a + 2 b s + c s^2, from the Bernstein form of the derivative.
        let (a, b, c) = (self.x1, self.x2 - 2.0 * self.x1, 1.0 + 3.0 * (self.x1 - self.x2));
        let mut ends = [1.0f64; 3];
        let mut n = 0;
        let mut push = |s: f64| {
            if s > 0.0 && s < 1.0 && n < 2 {
                ends[n] = s;
                n += 1;
            }
        };
        if c.abs() < 1e-12 {
            if b != 0.0 {
                push(-a / (2.0 * b));
            }
        } else {
            let disc = b * b - a * c;
            if disc > 0.0 {
                let root = disc.sqrt();
                let (r1, r2) = ((-b - root) / c, (-b + root) / c);
                push(r1.min(r2));
                push(r1.max(r2));
            }
        }
        let mut lo = 0.0f64;
        for hi in ends {
            if hi > lo && x(hi) >= t {
                let (mut lo, mut hi) = (lo, hi);
                for _ in 0..60 {
                    let mid = 0.5 * (lo + hi);
                    if x(mid) < t {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                return 0.5 * (lo + hi);
            }
            lo = lo.max(hi);
        }
        1.0
    }

    /// How far the segment has moved at `t` (0 to 1) of its time, as a share of the way from its
    /// first point's position to the next's. Not a number counts as the start. Never allocates.
    pub fn shape(&self, t: f64) -> f64 {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
        if t == 0.0 || t == 1.0 || self.is_straight() {
            return t;
        }
        cubic(self.y1, self.y2, self.parameter(t)).clamp(0.0, 1.0)
    }

    /// The segment cut at `t` (strictly between 0 and 1) of its time, so that the two halves
    /// play exactly as the whole did: the share of the way at the cut and each half's curve,
    /// scaled to its own box (de Casteljau), not held in the unit square: a half of a curve whose
    /// points run back in time or in value, such as (1, 0, 0, 1), lies partly outside it (down to
    /// -1/3, up to 4/3). A half that does not move (all of its way at one end) is straight; a
    /// coordinate past [`CONTROL_MIN`] or [`CONTROL_MAX`], which only a curve whose time share
    /// falls or rounding in a sliver of a cut can give, is held there. `None` for a `t` at an end
    /// or not a number, and for a cut where the way is past either end (a curve whose points lie
    /// beyond the box can overshoot), and for one whose half would need a control point beyond
    /// the limits: the halves could not play as the whole does.
    pub fn split(&self, t: f64) -> Option<Split> {
        if !(t > 0.0 && t < 1.0) {
            return None;
        }
        if self.is_straight() {
            return Some(Split { share: t, before: Self::STRAIGHT, after: Self::STRAIGHT });
        }
        let s = self.parameter(t);
        let lerp = |a: (f64, f64), b: (f64, f64)| (a.0 + (b.0 - a.0) * s, a.1 + (b.1 - a.1) * s);
        let (p0, p1, p2, p3) = ((0.0, 0.0), (self.x1, self.y1), (self.x2, self.y2), (1.0, 1.0));
        let (q, r, w) = (lerp(p0, p1), lerp(p1, p2), lerp(p2, p3));
        let (u, v) = (lerp(q, r), lerp(r, w));
        let m = lerp(u, v);
        // Each half in its own box: the first from (0, 0) to M, the second from M to (1, 1).
        let scale = |a: (f64, f64), b: (f64, f64), from: (f64, f64), to: (f64, f64)| {
            let (w, h) = (to.0 - from.0, to.1 - from.1);
            if !(w > 0.0 && h > 0.0) {
                // A half that does not move plays as a straight one only if its points do not move either.
                let level = w > 0.0 && [a.1, b.1, to.1].iter().all(|y| (y - from.1).abs() <= SLIVER);
                return level.then_some(Self::STRAIGHT);
            }
            let half = [(a.0 - from.0) / w, (a.1 - from.1) / h, (b.0 - from.0) / w, (b.1 - from.1) / h];
            // A half whose points the limits would move plays differently from the whole.
            let kept = half.iter().all(|v| (CONTROL_MIN - SLIVER..=CONTROL_MAX + SLIVER).contains(v));
            kept.then(|| Self { x1: half[0], y1: half[1], x2: half[2], y2: half[3] }.held())
        };
        // A cut where the way is past either end cannot be kept by two halves that stay between
        // their own points: the whole plays held there, a half could not.
        if !(-SLIVER..=1.0 + SLIVER).contains(&m.1) {
            return None;
        }
        Some(Split { share: m.1.clamp(0.0, 1.0), before: scale(q, u, p0, m)?, after: scale(v, w, m, p3)? })
    }
}

/// A segment cut in two by [`Curve::split`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Split {
    /// The share of the way at the cut, where the new point goes.
    pub share: f64,
    /// The curve from the segment's first point to the new one.
    pub before: Curve,
    /// The curve from the new point to the segment's last.
    pub after: Curve,
}

/// One coordinate of a cubic Bezier from 0 to 1 with inner control values `a` and `b`, at
/// parameter `s`.
fn cubic(a: f64, b: f64, s: f64) -> f64 {
    let r = 1.0 - s;
    3.0 * s * r * r * a + 3.0 * s * s * r * b + s * s * s
}

/// One step of a curve drag, in a control point's place along its path.
pub const DRAG_STEP: f64 = 1.0 / 64.0;

/// A curve being dragged into another shape (`docs/spec/automation.md` › Dragging a curve).
///
/// Each control point has a **path**: a straight line from the corner (0, 1) to a point (p, p)
/// on the diagonal, then on to the corner (1, 0). Its place on the path is F: 0 at (0, 1), 1 at
/// (p, p), 2 at (1, 0). A drag moves F in whole steps of [`DRAG_STEP`] and never changes p. When
/// it starts it chooses a mode it keeps to its end: with both control points on one side of the
/// diagonal (or both on it) they move **together**; on opposite sides (or one on it) they move
/// **apart**, the one with the larger F with the drag and the other against it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveDrag {
    paths: [f64; 2],
    places: [f64; 2],
    /// Which way each control point moves for a step later: 1 or -1.
    signs: [f64; 2],
}

impl CurveDrag {
    /// A drag starting from `curve`. A straight curve, one whose control points are both on the
    /// diagonal and one with both in the corners (0, 1) or (1, 0) start on the mirrored family's
    /// paths (p = 1/4 and 3/4); a single control point in a corner takes its family path too.
    /// A control point outside the unit square (from a split) is first held in it, each
    /// coordinate on its own, so even a drag of no steps gives the held curve.
    pub fn new(curve: &Curve) -> Self {
        let curve = curve.boxed();
        let (first, second) = (path_place(curve.x1, curve.y1), path_place(curve.x2, curve.y2));
        let straight = first.1 == 1.0 && second.1 == 1.0;
        let paths = if straight || (first.0.is_none() && second.0.is_none()) {
            [0.25, 0.75]
        } else {
            [first.0.unwrap_or(0.25), second.0.unwrap_or(0.75)]
        };
        let places = [first.1, second.1];
        let side = |f: f64| (f > 1.0 + NEAR) as i8 - (f < 1.0 - NEAR) as i8;
        let signs = if side(places[0]) == side(places[1]) {
            [1.0, 1.0]
        } else if places[0] >= places[1] {
            [1.0, -1.0]
        } else {
            [-1.0, 1.0]
        };
        Self { paths, places, signs }
    }

    /// Whether the control points move apart (an S bends further or flattens), not together.
    pub fn apart(&self) -> bool {
        self.signs[0] != self.signs[1]
    }

    /// The curve after `steps` steps of the drag: above 0 later (together: the whole curve moves
    /// later; apart: the S bends further), below 0 earlier. A control point that would pass the
    /// end of its path stops there while the other goes on; a step that neither can take is not
    /// taken, so a drag can stop a step short of a corner.
    pub fn dragged(&self, steps: i32) -> Curve {
        let step = if steps >= 0 { DRAG_STEP } else { -DRAG_STEP };
        let mut places = self.places;
        for _ in 0..steps.unsigned_abs() {
            let next = [places[0] + self.signs[0] * step, places[1] + self.signs[1] * step];
            if !next.iter().any(|f| (0.0..=2.0).contains(f)) {
                break;
            }
            places = [next[0].clamp(0.0, 2.0), next[1].clamp(0.0, 2.0)];
        }
        let (x1, y1) = on_path(self.paths[0], places[0]);
        let (x2, y2) = on_path(self.paths[1], places[1]);
        Curve { x1, y1, x2, y2 }.boxed()
    }
}

/// The point at place `f` (0 to 2) on the path through (p, p).
fn on_path(p: f64, f: f64) -> (f64, f64) {
    if f <= 1.0 {
        (f * p, 1.0 - f * (1.0 - p))
    } else {
        let g = f - 1.0;
        (p + g * (1.0 - p), p * (1.0 - g))
    }
}

/// The path (its p, `None` in the corners (0, 1) and (1, 0), which are on every path) and the
/// place on it of control point (x, y).
fn path_place(x: f64, y: f64) -> (Option<f64>, f64) {
    let near = |a: f64, b: f64| (a - b).abs() < NEAR;
    if near(x, 0.0) && near(y, 1.0) {
        (None, 0.0)
    } else if near(x, 1.0) && near(y, 0.0) {
        (None, 2.0)
    } else if near(x, y) {
        (Some(x), 1.0)
    } else if y > x {
        // Above the diagonal: the leg from (0, 1).
        if near(y, 1.0) {
            (Some(1.0), x)
        } else if near(x, 0.0) {
            (Some(0.0), 1.0 - y)
        } else {
            let p = x / (x + 1.0 - y);
            (Some(p), x / p)
        }
    } else if near(x, 1.0) {
        (Some(1.0), 2.0 - y)
    } else if near(y, 0.0) {
        (Some(0.0), 1.0 + x)
    } else {
        // Below the diagonal: the leg to (1, 0).
        let p = y / (y + 1.0 - x);
        (Some(p), 1.0 + (x - p) / (1.0 - p))
    }
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
    let share = a.curve.shape(t);
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
        Self { beat, value, curve: Curve::STRAIGHT }
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
#[path = "automation_tests.rs"]
mod tests;
