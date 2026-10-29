// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fitting an automation line with as few bent segments as stay within a tolerance of it, for
//! `automation.simplify` (`docs/spec/automation.md` › Simplify): the old line is sampled, cut at
//! its jumps and sharp corners, and each piece gets one cubic Bézier by least squares along the
//! piece's end directions, split where it strays furthest until every piece is close enough
//! (Schneider's curve fitting, as in VectorCraft's path Simplify, MIT or Apache-2.0).
//!
//! A lane's segment is such a cubic already ([`Curve`]: its two control points with the time
//! and the value as shares of the segment), so a fitted cubic becomes a point and its curve.

use suhina_model::automation::Curve;

/// How much wider than tall the lane is taken to be when fitting, so the end directions and the
/// least squares weigh time and value about as they look on screen.
const ASPECT: f64 = 4.0;
/// Samples along each old segment.
const PER_SEGMENT: usize = 24;
/// How sharp a turn at an old point keeps it as a corner, in degrees.
const CORNER: f64 = 45.0;
/// How deep the splitting goes, a guard against a line that never fits.
const MAX_DEPTH: usize = 24;
const NEAR: f64 = 1e-9;

/// A place on the lane: time as a share of the range times [`ASPECT`], and the place 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
struct P {
    x: f64,
    y: f64,
}

impl P {
    fn sub(self, o: P) -> P {
        P { x: self.x - o.x, y: self.y - o.y }
    }
    fn add(self, o: P) -> P {
        P { x: self.x + o.x, y: self.y + o.y }
    }
    fn mul(self, k: f64) -> P {
        P { x: self.x * k, y: self.y * k }
    }
    fn dot(self, o: P) -> f64 {
        self.x * o.x + self.y * o.y
    }
    fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
    fn unit(self) -> Option<P> {
        let l = self.len();
        (l > NEAR).then(|| self.mul(1.0 / l))
    }
}

/// A point of the fitted line: its beat, its place on the lane (0 to 1) and the curve of the
/// segment after it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Fitted {
    pub beat: f64,
    pub place: f64,
    pub curve: Curve,
}

/// The fewest points (with their curves) whose line stays within `tolerance` of the line through
/// `old` (beats and places 0 to 1, in beat order, each with the curve after it) everywhere from
/// the first to the last. The first and last points stay; a jump (two points at one beat) stays a
/// jump. The last point's curve is its own. `None` when the fit has no fewer points than `old`.
pub(crate) fn simplify(old: &[(f64, f64, Curve)], tolerance: f64) -> Option<Vec<Fitted>> {
    let (first, last) = (old.first()?, old.last()?);
    let span = (last.0 - first.0).max(NEAR);
    let at = |beat: f64, place: f64| P { x: (beat - first.0) / span * ASPECT, y: place };
    // The old line, sampled; `breaks` are the samples where a jump or a corner is kept.
    let mut samples: Vec<P> = Vec::new();
    let mut breaks: Vec<usize> = vec![0];
    for (i, pair) in old.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        if i > 0 && is_break(old, i, &at) {
            breaks.push(samples.len().saturating_sub(1));
        }
        if b.0 - a.0 <= NEAR {
            // A jump: its two ends are samples of their own, a break between them.
            samples.push(at(b.0, b.1));
            breaks.push(samples.len() - 1);
            continue;
        }
        if samples.is_empty() {
            samples.push(at(a.0, a.1));
        }
        for k in 1..=PER_SEGMENT {
            let t = k as f64 / PER_SEGMENT as f64;
            let beat = a.0 + (b.0 - a.0) * t;
            samples.push(at(beat, a.1 + (b.1 - a.1) * a.2.shape(t)));
        }
    }
    breaks.push(samples.len().saturating_sub(1));
    breaks.dedup();
    // A corner stays only where the line needs it: one that a single fit across it follows within
    // the tolerance, with no more segments than the two sides apart, is smoothed away.
    let jump = |k: usize| {
        let x = samples.get(k).map(|p| p.x);
        [k.checked_sub(1), Some(k + 1)].into_iter().flatten().any(|n| samples.get(n).map(|p| p.x) == x)
    };
    let mut k = 1;
    while k + 1 < breaks.len() {
        let (i, b, j) = (breaks[k - 1], breaks[k], breaks[k + 1]);
        if !jump(b) {
            let apart = piece_fit(&samples, i, b, tolerance).len() + piece_fit(&samples, b, j, tolerance).len();
            if piece_fit(&samples, i, j, tolerance).len() <= apart {
                breaks.remove(k);
                continue;
            }
        }
        k += 1;
    }
    let mut out: Vec<(P, Curve)> = Vec::new();
    for w in breaks.windows(2) {
        out.extend(piece_fit(&samples, w[0], w[1], tolerance));
    }
    let end = samples.last()?;
    let to_beat = |p: P| first.0 + p.x / ASPECT * span;
    let mut fitted: Vec<Fitted> =
        out.into_iter().map(|(p, curve)| Fitted { beat: to_beat(p), place: p.y, curve }).collect();
    fitted.push(Fitted { beat: to_beat(*end), place: end.y, curve: last.2 });
    (fitted.len() < old.len()).then_some(fitted)
}

/// The fit of the samples from `i` to `j`: each segment's start and curve. A jump (both ends at
/// one time) is its first end, straight to the second.
fn piece_fit(samples: &[P], i: usize, j: usize, tolerance: f64) -> Vec<(P, Curve)> {
    let mut out = Vec::new();
    let Some(piece) = samples.get(i..=j).filter(|p| p.len() >= 2) else { return out };
    if (piece[piece.len() - 1].x - piece[0].x).abs() <= NEAR {
        out.push((piece[0], Curve::STRAIGHT));
        return out;
    }
    fit(piece, direction(piece, true), direction(piece, false), tolerance, &mut out, 0);
    out
}

/// Whether the old line keeps point `i` as a corner: a jump starts or ends there, or the line
/// turns there by more than [`CORNER`].
fn is_break(old: &[(f64, f64, Curve)], i: usize, at: &impl Fn(f64, f64) -> P) -> bool {
    let (Some(a), Some(p), Some(b)) = (old.get(i - 1), old.get(i), old.get(i + 1)) else { return false };
    if p.0 - a.0 <= NEAR || b.0 - p.0 <= NEAR {
        return true;
    }
    let near = |from: &(f64, f64, Curve), to: &(f64, f64, Curve), t: f64| {
        at(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * from.2.shape(t))
    };
    let here = at(p.0, p.1);
    let before = here.sub(near(a, p, 1.0 - 1.0 / PER_SEGMENT as f64)).unit();
    let after = near(p, b, 1.0 / PER_SEGMENT as f64).sub(here).unit();
    match (before, after) {
        (Some(u), Some(v)) => u.dot(v).clamp(-1.0, 1.0).acos().to_degrees() > CORNER,
        _ => false,
    }
}

/// The direction the line leaves a piece's first sample (`start`) or arrives at its last, both
/// pointing forward in time.
fn direction(piece: &[P], start: bool) -> P {
    let n = piece.len();
    let (a, b) = if start { (piece[0], piece[1.min(n - 1)]) } else { (piece[n.saturating_sub(2)], piece[n - 1]) };
    b.sub(a).unit().unwrap_or(P { x: 1.0, y: 0.0 })
}

/// Fits `piece` (from its first sample to its last) with cubics leaving along `t0` and arriving
/// along `t1`, pushing each one's start and curve onto `out`.
fn fit(piece: &[P], t0: P, t1: P, tolerance: f64, out: &mut Vec<(P, Curve)>, depth: usize) {
    let (a, b) = (piece[0], piece[piece.len() - 1]);
    if piece.len() <= 2 || depth >= MAX_DEPTH {
        out.push((a, Curve::STRAIGHT));
        return;
    }
    let u = chord(piece);
    let (c1, c2) = handles(piece, &u, t0, t1);
    if let Some(curve) = as_curve(a, c1, c2, b) {
        let worst = worst(piece, a, b, &curve);
        if worst.0 <= tolerance {
            out.push((a, curve));
            return;
        }
        split(piece, worst.1, t0, t1, tolerance, out, depth);
        return;
    }
    // A cubic the lane cannot hold (one that runs back in time, or bends a level segment): cut
    // where a straight line strays furthest.
    let straight = worst(piece, a, b, &Curve::STRAIGHT);
    if straight.0 <= tolerance {
        out.push((a, Curve::STRAIGHT));
    } else {
        split(piece, straight.1, t0, t1, tolerance, out, depth);
    }
}

/// Splits `piece` at sample `k` and fits each side, the two meeting in one direction there.
fn split(piece: &[P], k: usize, t0: P, t1: P, tolerance: f64, out: &mut Vec<(P, Curve)>, depth: usize) {
    let k = k.clamp(1, piece.len() - 2);
    let mid = piece[k + 1].sub(piece[k - 1]).unit().unwrap_or(P { x: 1.0, y: 0.0 });
    fit(&piece[..=k], t0, mid, tolerance, out, depth + 1);
    fit(&piece[k..], mid, t1, tolerance, out, depth + 1);
}

/// Each sample's share of the way along the piece, by the length of the line through them.
fn chord(piece: &[P]) -> Vec<f64> {
    let mut u = vec![0.0; piece.len()];
    for i in 1..piece.len() {
        u[i] = u[i - 1] + piece[i].sub(piece[i - 1]).len();
    }
    let total = u.last().copied().unwrap_or(0.0).max(NEAR);
    u.iter_mut().for_each(|v| *v /= total);
    u
}

/// The two control points of the cubic from the piece's first sample to its last, leaving along
/// `t0` and arriving along `t1`, whose handle lengths fit the samples best by least squares; a
/// third of the chord each when the fit has no answer.
fn handles(piece: &[P], u: &[f64], t0: P, t1: P) -> (P, P) {
    let (a, b) = (piece[0], piece[piece.len() - 1]);
    let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (p, &t) in piece.iter().zip(u) {
        let s = 1.0 - t;
        let (b0, b1, b2, b3) = (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
        let (a1, a2) = (t0.mul(b1), t1.mul(-b2));
        c00 += a1.dot(a1);
        c01 += a1.dot(a2);
        c11 += a2.dot(a2);
        let rest = p.sub(a.mul(b0 + b1).add(b.mul(b2 + b3)));
        x0 += a1.dot(rest);
        x1 += a2.dot(rest);
    }
    let det = c00 * c11 - c01 * c01;
    let chord = b.sub(a).len();
    let (mut l0, mut l1) =
        if det.abs() > NEAR { ((x0 * c11 - x1 * c01) / det, (c00 * x1 - c01 * x0) / det) } else { (0.0, 0.0) };
    if l0 < chord * 1e-3 || l1 < chord * 1e-3 {
        (l0, l1) = (chord / 3.0, chord / 3.0);
    }
    (a.add(t0.mul(l0)), b.sub(t1.mul(l1)))
}

/// The cubic from `a` to `b` through controls `c1` and `c2` as a lane's curve: time and value as
/// shares of the segment. `None` when the lane cannot hold it: a control outside the segment's
/// time, outside the curve's limits, or a bend on a segment whose ends are level.
fn as_curve(a: P, c1: P, c2: P, b: P) -> Option<Curve> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    if dx <= NEAR {
        return None;
    }
    let x1 = (c1.x - a.x) / dx;
    let x2 = (c2.x - a.x) / dx;
    if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) {
        return None;
    }
    if dy.abs() <= NEAR {
        // Level ends: only a straight line is a curve here.
        let flat = (c1.y - a.y).abs() <= NEAR && (c2.y - a.y).abs() <= NEAR;
        return flat.then_some(Curve::STRAIGHT);
    }
    Curve::new(x1, (c1.y - a.y) / dy, x2, (c2.y - a.y) / dy)
}

/// How far the segment from `a` to `b` with `curve` is from the piece at worst, up or down as a
/// share of the lane, and at which sample.
fn worst(piece: &[P], a: P, b: P, curve: &Curve) -> (f64, usize) {
    let dx = (b.x - a.x).max(NEAR);
    let mut out = (0.0, piece.len() / 2);
    for (i, p) in piece.iter().enumerate() {
        let t = ((p.x - a.x) / dx).clamp(0.0, 1.0);
        let off = (a.y + (b.y - a.y) * curve.shape(t) - p.y).abs();
        if off > out.0 {
            out = (off, i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(points: &[(f64, f64)]) -> Vec<(f64, f64, Curve)> {
        points.iter().map(|&(b, v)| (b, v, Curve::STRAIGHT)).collect()
    }

    /// The fitted line's place at `beat`.
    fn place(fitted: &[Fitted], beat: f64) -> f64 {
        let i = fitted.iter().rposition(|f| f.beat <= beat).unwrap_or(0).min(fitted.len() - 2);
        let (a, b) = (fitted[i], fitted[i + 1]);
        let t = ((beat - a.beat) / (b.beat - a.beat).max(NEAR)).clamp(0.0, 1.0);
        a.place + (b.place - a.place) * a.curve.shape(t)
    }

    #[test]
    fn a_dense_sine_becomes_a_few_curves_that_follow_it() {
        let old: Vec<(f64, f64)> = (0..=64)
            .map(|i| (f64::from(i) / 8.0, 0.5 + 0.4 * (f64::from(i) / 8.0 * std::f64::consts::PI).sin()))
            .collect();
        let fitted = simplify(&line(&old), 0.02).unwrap();
        assert!(fitted.len() < 30, "{} points", fitted.len());
        for &(beat, value) in &old {
            assert!((place(&fitted, beat) - value).abs() < 0.03, "at {beat}");
        }
    }

    #[test]
    fn points_on_one_straight_line_become_two() {
        let old: Vec<(f64, f64)> = (0..=10).map(|i| (f64::from(i), f64::from(i) / 10.0)).collect();
        let fitted = simplify(&line(&old), 0.01).unwrap();
        assert_eq!(fitted.len(), 2);
    }

    #[test]
    fn a_jump_and_a_sharp_corner_stay() {
        let old =
            line(&[(0.0, 0.0), (1.0, 0.1), (2.0, 0.2), (2.0, 0.9), (3.0, 0.9), (4.0, 0.9), (5.0, 0.0), (6.0, 0.0)]);
        let fitted = simplify(&old, 0.01).unwrap();
        // The jump at 2 is two points at one beat; the peak's corner at 4 is a point.
        assert!(fitted.windows(2).any(|w| (w[0].beat - 2.0).abs() < 1e-9 && (w[1].beat - 2.0).abs() < 1e-9));
        assert!(fitted.iter().any(|f| (f.beat - 4.0).abs() < 1e-9 && (f.place - 0.9).abs() < 1e-9));
    }

    #[test]
    fn a_small_sharp_bump_inside_the_tolerance_leaves_no_corner() {
        // Level, a spike of 0.03 at 2, then a fall: no point of the fit sits at the spike.
        let old = line(&[(0.0, 0.8), (1.9, 0.8), (2.0, 0.83), (2.1, 0.8), (4.0, 0.8), (6.0, 0.4), (8.0, 0.0)]);
        let fitted = simplify(&old, 0.05).unwrap();
        assert!(fitted.iter().all(|f| (f.beat - 2.0).abs() > 1e-6), "{fitted:?}");
    }

    #[test]
    fn a_line_that_cannot_lose_a_point_is_left() {
        assert_eq!(simplify(&line(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)]), 0.001), None);
    }
}
