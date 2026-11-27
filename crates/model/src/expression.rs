// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A note's own expression: optional pitch, pressure and timbre curves with times relative to the
//! note's start ([ADR 0039](../../docs/decisions/0039-per-note-expression.md) › 6,
//! [spec](../../docs/spec/mpe.md#note-curves-and-editing)).
//!
//! A curve is a list of points in beats from the note's start, strictly increasing, read with
//! linear interpolation and holding its first and last value outside them. A missing curve is the
//! kind's neutral value. Every operation that changes a note's length or splits it reshapes the
//! curves here, so the piano roll, the commands and the file all keep one rule.

/// What a curve moves. Values are musical, not MIDI bytes (ADR 0039 › 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpressionKind {
    /// Offset from the note's key in semitones, -256 to 256.
    Pitch,
    /// 0 to 1.
    Pressure,
    /// 0 to 1, neutral in the middle (MPE's CC74 "slide").
    Timbre,
}

impl ExpressionKind {
    pub const ALL: [ExpressionKind; 3] = [ExpressionKind::Pitch, ExpressionKind::Pressure, ExpressionKind::Timbre];

    /// The value a note without this curve has.
    pub const fn neutral(self) -> f64 {
        match self {
            ExpressionKind::Pitch => 0.0,
            ExpressionKind::Pressure => 0.0,
            ExpressionKind::Timbre => 0.5,
        }
    }

    /// The lowest and highest value a point may have.
    pub const fn range(self) -> (f64, f64) {
        match self {
            ExpressionKind::Pitch => (-MAX_PITCH_SEMITONES, MAX_PITCH_SEMITONES),
            ExpressionKind::Pressure | ExpressionKind::Timbre => (0.0, 1.0),
        }
    }

    /// The name the file and the commands use.
    pub const fn name(self) -> &'static str {
        match self {
            ExpressionKind::Pitch => "pitch",
            ExpressionKind::Pressure => "pressure",
            ExpressionKind::Timbre => "timbre",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// `value` within the kind's range; `None` when it is not a finite number.
    pub fn clamp(self, value: f64) -> Option<f64> {
        let (low, high) = self.range();
        value.is_finite().then(|| value.clamp(low, high))
    }
}

/// How far a pitch curve may bend, in semitones either way (ADR 0039 › 2).
pub const MAX_PITCH_SEMITONES: f64 = 256.0;
/// Most points one curve may have (ADR 0039 › 6).
pub const MAX_CURVE_POINTS: usize = 4_096;
/// Most points all the curves of one clip content may have together (ADR 0039 › 6).
pub const MAX_CONTENT_EXPRESSION_POINTS: usize = 262_144;

/// One point of a curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExpressionPoint {
    /// Beats from the note's start, 0 to the note's duration.
    pub offset: f64,
    /// In the kind's units (see [`ExpressionKind`]).
    pub value: f64,
    /// The shape of the segment from this point to the next, -1 to 1: 0 is straight, positive
    /// bulges toward higher values and negative toward lower ones ([`shaped`]). The last point's
    /// is not used.
    pub curve: f64,
}

impl ExpressionPoint {
    pub const fn new(offset: f64, value: f64) -> Self {
        Self { offset, value, curve: 0.0 }
    }

    /// A point whose segment to the next one has `curve` (-1 to 1).
    pub const fn curved(offset: f64, value: f64, curve: f64) -> Self {
        Self { offset, value, curve }
    }
}

/// Why a curve was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum ExpressionError {
    /// An offset or a value is not a finite number.
    NotFinite,
    /// An offset is before the note's start or after its end.
    OffsetOutside { offset: f64, duration: f64 },
    /// A value is outside its kind's range.
    ValueOutside { kind: ExpressionKind, value: f64 },
    /// Two points at the same offset, or out of order.
    NotIncreasing,
    /// More points than one curve may have.
    TooManyPoints(usize),
    /// A segment's curvature is outside -1 to 1.
    CurveOutside(f64),
}

impl std::fmt::Display for ExpressionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExpressionError::NotFinite => write!(f, "an expression point is not a finite number"),
            ExpressionError::OffsetOutside { offset, duration } => {
                write!(f, "an expression point at {offset} is outside the note (0 to {duration})")
            }
            ExpressionError::ValueOutside { kind, value } => {
                let (low, high) = kind.range();
                write!(f, "a {} value {value} is outside {low} to {high}", kind.name())
            }
            ExpressionError::NotIncreasing => write!(f, "expression points must have increasing offsets"),
            ExpressionError::CurveOutside(curve) => {
                write!(f, "a segment's curvature {curve} is outside -1 to 1")
            }
            ExpressionError::TooManyPoints(count) => {
                write!(f, "a curve has {count} points, more than {MAX_CURVE_POINTS}")
            }
        }
    }
}

impl std::error::Error for ExpressionError {}

/// Why two notes' curves cannot be joined ([`NoteExpression::joined`]).
#[derive(Debug, Clone, PartialEq)]
pub enum JoinError {
    /// This curve's value at the end of the first note differs from its value at the start of
    /// the next.
    ValueDiffers(ExpressionKind),
    /// This joined curve would have `count` points, more than [`MAX_CURVE_POINTS`].
    TooManyPoints { kind: ExpressionKind, count: usize },
}

impl std::fmt::Display for JoinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JoinError::ValueDiffers(kind) => write!(f, "the {} curve's value differs across the join", kind.name()),
            JoinError::TooManyPoints { kind, count } => {
                write!(f, "the joined {} curve would have {count} points, more than {MAX_CURVE_POINTS}", kind.name())
            }
        }
    }
}

impl std::error::Error for JoinError {}

/// A note's curves. An empty curve is the kind's neutral value throughout.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NoteExpression {
    pub pitch: Vec<ExpressionPoint>,
    pub pressure: Vec<ExpressionPoint>,
    pub timbre: Vec<ExpressionPoint>,
}

impl NoteExpression {
    pub fn curve(&self, kind: ExpressionKind) -> &[ExpressionPoint] {
        match kind {
            ExpressionKind::Pitch => &self.pitch,
            ExpressionKind::Pressure => &self.pressure,
            ExpressionKind::Timbre => &self.timbre,
        }
    }

    pub fn curve_mut(&mut self, kind: ExpressionKind) -> &mut Vec<ExpressionPoint> {
        match kind {
            ExpressionKind::Pitch => &mut self.pitch,
            ExpressionKind::Pressure => &mut self.pressure,
            ExpressionKind::Timbre => &mut self.timbre,
        }
    }

    /// Whether every curve is missing.
    pub fn is_empty(&self) -> bool {
        self.pitch.is_empty() && self.pressure.is_empty() && self.timbre.is_empty()
    }

    /// The points of all three curves together, for the per-content limit.
    pub fn point_count(&self) -> usize {
        self.pitch.len() + self.pressure.len() + self.timbre.len()
    }

    /// The value of `kind` at `offset` beats from the note's start.
    pub fn value_at(&self, kind: ExpressionKind, offset: f64) -> f64 {
        curve_value(self.curve(kind), offset).unwrap_or(kind.neutral())
    }

    /// Checks every curve against a note `duration` beats long.
    pub fn validate(&self, duration: f64) -> Result<(), ExpressionError> {
        for kind in ExpressionKind::ALL {
            validate_curve(kind, self.curve(kind), duration)?;
        }
        Ok(())
    }

    /// Puts a point on `kind`'s curve, replacing one at the same offset. The offset is clamped
    /// into the note and the value into the kind's range.
    pub fn set_point(
        &mut self,
        kind: ExpressionKind,
        duration: f64,
        offset: f64,
        value: f64,
    ) -> Result<(), ExpressionError> {
        let (Some(value), true, true) = (kind.clamp(value), offset.is_finite(), duration.is_finite()) else {
            return Err(ExpressionError::NotFinite);
        };
        let offset = offset.clamp(0.0, duration.max(0.0));
        let curve = self.curve_mut(kind);
        match curve.binary_search_by(|point| point.offset.total_cmp(&offset)) {
            Ok(index) => curve[index].value = value,
            Err(index) => {
                if curve.len() >= MAX_CURVE_POINTS {
                    return Err(ExpressionError::TooManyPoints(curve.len() + 1));
                }
                curve.insert(index, ExpressionPoint::new(offset, value));
            }
        }
        Ok(())
    }

    /// The note's end moves to `duration` (resize from the right): points past it go and the
    /// value there is kept as a new last point. Lengthening holds the last value, so changes
    /// nothing.
    pub fn crop_end(&mut self, duration: f64) {
        if !duration.is_finite() {
            return;
        }
        let duration = duration.max(0.0);
        for kind in ExpressionKind::ALL {
            let curve = self.curve_mut(kind);
            if curve.last().is_none_or(|last| last.offset <= duration) {
                continue;
            }
            let boundary = curve_value(curve, duration);
            curve.retain(|point| point.offset < duration);
            if let Some(value) = boundary {
                curve.push(ExpressionPoint::new(duration, value));
            }
        }
    }

    /// The note's start moves by `delta` beats with its end fixed (resize from the left).
    /// Moving it later drops the points before it and keeps the value there as a first point;
    /// moving it earlier shifts the points along, so the old first value holds from the new start.
    /// A curve whose first point is after the new start gets no new point (its first value holds
    /// to the start already), so a curve never grows: one at [`MAX_CURVE_POINTS`] stays there.
    pub fn move_start(&mut self, delta: f64) {
        if !delta.is_finite() || delta == 0.0 {
            return;
        }
        for kind in ExpressionKind::ALL {
            let curve = self.curve_mut(kind);
            if curve.is_empty() {
                continue;
            }
            if delta > 0.0 {
                let boundary = curve_value(curve, delta);
                let shape = segment_curve(curve, delta);
                let before = curve.len();
                curve.retain(|point| point.offset > delta);
                let dropped = curve.len() < before;
                for point in curve.iter_mut() {
                    point.offset -= delta;
                }
                if let Some(value) = boundary.filter(|_| dropped) {
                    curve.insert(0, ExpressionPoint::curved(0.0, value, shape));
                }
            } else {
                for point in curve.iter_mut() {
                    point.offset -= delta;
                }
            }
        }
    }

    /// The note is stretched by `factor` (new duration / old duration). A factor that is not a
    /// positive finite number changes nothing.
    pub fn stretch(&mut self, factor: f64) {
        if !(factor.is_finite() && factor > 0.0) || factor == 1.0 {
            return;
        }
        for kind in ExpressionKind::ALL {
            for point in self.curve_mut(kind).iter_mut() {
                point.offset *= factor;
            }
            dedup_offsets(self.curve_mut(kind));
        }
    }

    /// Splits the curves at `at` beats from the note's start: the left part ends and the right
    /// part starts with the value there, the right part's offsets from its own start. A part
    /// that already holds that value at the cut (all the curve's points on its other side) gets
    /// no boundary point, so neither part has more points than the curve it came from.
    pub fn split(&self, at: f64) -> (NoteExpression, NoteExpression) {
        let mut left = NoteExpression::default();
        let mut right = NoteExpression::default();
        for kind in ExpressionKind::ALL {
            let curve = self.curve(kind);
            let Some(boundary) = curve_value(curve, at) else { continue };
            let (l, r) = (left.curve_mut(kind), right.curve_mut(kind));
            l.extend(curve.iter().copied().filter(|point| point.offset < at));
            if curve.last().is_some_and(|last| last.offset >= at) {
                l.push(ExpressionPoint::new(at, boundary));
            }
            if curve.first().is_some_and(|first| first.offset <= at) {
                r.push(ExpressionPoint::curved(0.0, boundary, segment_curve(curve, at)));
            }
            r.extend(
                curve
                    .iter()
                    .filter(|point| point.offset > at)
                    .map(|point| ExpressionPoint { offset: point.offset - at, ..*point }),
            );
        }
        (left, right)
    }

    /// Joins `next`, which starts `duration` beats after this note's start, onto this one.
    /// Refused when a curve's value differs across the join (a join never invents a ramp), and
    /// when a joined curve would have more than [`MAX_CURVE_POINTS`] points.
    pub fn joined(&self, duration: f64, next: &NoteExpression) -> Result<NoteExpression, JoinError> {
        let mut out = self.clone();
        for kind in ExpressionKind::ALL {
            let end = self.value_at(kind, duration);
            let start = next.value_at(kind, 0.0);
            if (end - start).abs() > 1e-9 {
                return Err(JoinError::ValueDiffers(kind));
            }
            let curve = out.curve_mut(kind);
            let last = curve.last().map(|last| last.offset);
            let added = next.curve(kind).iter().filter(|p| last.is_none_or(|last| p.offset + duration > last)).count();
            let count = curve.len().saturating_add(added);
            if count > MAX_CURVE_POINTS {
                return Err(JoinError::TooManyPoints { kind, count });
            }
            for point in next.curve(kind) {
                let offset = point.offset + duration;
                if curve.last().is_some_and(|last| last.offset >= offset) {
                    continue;
                }
                curve.push(ExpressionPoint { offset, ..*point });
            }
        }
        Ok(out)
    }
}

/// The value of a curve at `offset`: linear between points, the end values held outside them.
/// `None` for an empty curve.
pub fn curve_value(curve: &[ExpressionPoint], offset: f64) -> Option<f64> {
    let first = curve.first()?;
    if offset <= first.offset {
        return Some(first.value);
    }
    let index = curve.partition_point(|point| point.offset <= offset);
    let Some(next) = curve.get(index) else {
        return curve.last().map(|last| last.value);
    };
    // An offset that is not a number is before no point and after none: the first value holds.
    let Some(prev) = index.checked_sub(1).and_then(|i| curve.get(i)) else {
        return Some(first.value);
    };
    let span = next.offset - prev.offset;
    if span <= 0.0 {
        return Some(next.value);
    }
    let t = (offset - prev.offset) / span;
    let rise = next.value - prev.value;
    Some(prev.value + rise * shaped(t, prev.curve, rise))
}

/// A segment's progress `t` (0 to 1 along its time) as its curvature shapes it: `t^e` with
/// `e = 2^(-3 · curve · sign(rise))`, so a positive curvature bulges toward higher values whichever
/// way the segment goes. Straight for curvature 0, a flat segment or a curvature that is not a
/// number.
pub fn shaped(t: f64, curve: f64, rise: f64) -> f64 {
    if curve == 0.0 || rise == 0.0 || !curve.is_finite() {
        return t;
    }
    let exponent = 2f64.powf(-3.0 * curve.clamp(-1.0, 1.0) * rise.signum());
    t.clamp(0.0, 1.0).powf(exponent)
}

/// The curvature of the segment `offset` falls in: the last point at or before it, 0 before the
/// first point.
fn segment_curve(curve: &[ExpressionPoint], offset: f64) -> f64 {
    let index = curve.partition_point(|point| point.offset <= offset);
    index.checked_sub(1).and_then(|i| curve.get(i)).map_or(0.0, |point| point.curve)
}

/// Checks one curve: finite numbers, inside the note and the kind's range, strictly increasing,
/// not too many points.
pub fn validate_curve(kind: ExpressionKind, curve: &[ExpressionPoint], duration: f64) -> Result<(), ExpressionError> {
    if curve.len() > MAX_CURVE_POINTS {
        return Err(ExpressionError::TooManyPoints(curve.len()));
    }
    let (low, high) = kind.range();
    let mut previous: Option<f64> = None;
    for point in curve {
        if !(point.offset.is_finite() && point.value.is_finite() && point.curve.is_finite()) {
            return Err(ExpressionError::NotFinite);
        }
        if !(-1.0..=1.0).contains(&point.curve) {
            return Err(ExpressionError::CurveOutside(point.curve));
        }
        if point.offset < 0.0 || point.offset > duration {
            return Err(ExpressionError::OffsetOutside { offset: point.offset, duration });
        }
        if point.value < low || point.value > high {
            return Err(ExpressionError::ValueOutside { kind, value: point.value });
        }
        if previous.is_some_and(|previous| point.offset <= previous) {
            return Err(ExpressionError::NotIncreasing);
        }
        previous = Some(point.offset);
    }
    Ok(())
}

/// Keeps the last of points that ended up at the same offset (a stretch rounding them together).
fn dedup_offsets(curve: &mut Vec<ExpressionPoint>) {
    let mut index = 1;
    while index < curve.len() {
        if curve[index].offset <= curve[index - 1].offset {
            curve.remove(index - 1);
        } else {
            index += 1;
        }
    }
}

#[cfg(test)]
#[path = "expression_tests.rs"]
mod tests;
