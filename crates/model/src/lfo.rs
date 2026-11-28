// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A low-frequency oscillation over a note's curve ([MPE spec](../../docs/spec/mpe.md#lfos)):
//! the wave `note.lfoExpression` writes into a curve as points, and the one a note keeps as
//! settings ([`NoteLfo`]), riding on its curve over a stretch of it and editable afterwards. Both
//! use one formula, so baking a kept LFO into points sounds as it did.

use crate::expression::{ExpressionKind, ExpressionPoint, MAX_CURVE_POINTS, curve_value};

/// The waves an LFO draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wave {
    Sine,
    Square,
    Triangle,
    Random,
}

impl Wave {
    pub const ALL: [Wave; 4] = [Wave::Sine, Wave::Square, Wave::Triangle, Wave::Random];

    /// The name the file and the commands use.
    pub const fn name(self) -> &'static str {
        match self {
            Wave::Sine => "sine",
            Wave::Square => "square",
            Wave::Triangle => "triangle",
            Wave::Random => "random",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|wave| wave.name() == name)
    }
}

/// The shortest cycle an LFO may have, in beats.
pub const MIN_LFO_PERIOD: f64 = 1.0 / 32.0;
/// The longest cycle an LFO may have, in beats.
pub const MAX_LFO_PERIOD: f64 = 64.0;
/// How far a pitch LFO may swing either way, in semitones; a pressure or slide one swings at most
/// the kind's whole range.
pub const MAX_PITCH_LFO_AMOUNT: f64 = 96.0;
/// Most LFOs one note may keep.
pub const MAX_NOTE_LFOS: usize = 16;

/// An LFO's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lfo {
    pub wave: Wave,
    /// -1 to 1: the skew of sine and triangle, the pulse width of square, glide of random.
    pub shape: f64,
    /// Beats per cycle, [`MIN_LFO_PERIOD`] to [`MAX_LFO_PERIOD`].
    pub period: f64,
    /// -1 to 1 of a cycle: positive delays the start, negative moves the phase.
    pub shift: f64,
    /// How far the wave swings either way, in the kind's units (negative turns it over).
    pub amount: f64,
    /// The value it swings around when written with Replace; a kept LFO rides on the curve.
    pub base: f64,
    /// Parts of the stretch it covers, 0 to 1, together at most 1: the swing grows from nothing
    /// over the attack and falls to nothing over the decay.
    pub attack: f64,
    pub decay: f64,
    /// Which random wave.
    pub seed: u64,
}

/// The fewest beats between two points of a written LFO.
const LFO_STEP: f64 = 1.0 / 256.0;

impl Lfo {
    /// The settings checked for `kind`: finite numbers in their ranges. A message naming the
    /// first that is not.
    pub fn check(&self, kind: ExpressionKind) -> Result<(), String> {
        let reach = if kind == ExpressionKind::Pitch { MAX_PITCH_LFO_AMOUNT } else { 1.0 };
        let fields = [
            ("shape", self.shape, -1.0, 1.0),
            ("period", self.period, MIN_LFO_PERIOD, MAX_LFO_PERIOD),
            ("shift", self.shift, -1.0, 1.0),
            ("amount", self.amount, -reach, reach),
            ("base", self.base, kind.range().0, kind.range().1),
            ("attack", self.attack, 0.0, 1.0),
            ("decay", self.decay, 0.0, 1.0),
        ];
        for (name, value, low, high) in fields {
            if !(value.is_finite() && (low..=high).contains(&value)) {
                return Err(format!("an LFO's {name} must be {low} to {high}, got {value}"));
            }
        }
        if self.attack + self.decay > 1.0 + 1e-9 {
            return Err("an LFO's attack and decay together must be at most 1".into());
        }
        Ok(())
    }

    /// The wave at `phase` cycles from its start, -1 to 1.
    fn wave_at(&self, phase: f64) -> f64 {
        let x = phase.rem_euclid(1.0);
        // Sine and triangle lean: the half-way point of a cycle moves between 5 % and 95 %.
        let lean = |x: f64| {
            let middle = (0.5 + 0.45 * self.shape).clamp(0.05, 0.95);
            if x < middle { 0.5 * x / middle } else { 0.5 + 0.5 * (x - middle) / (1.0 - middle) }
        };
        match self.wave {
            Wave::Sine => (std::f64::consts::TAU * lean(x)).sin(),
            Wave::Triangle => {
                let x = lean(x);
                if x < 0.25 {
                    4.0 * x
                } else if x < 0.75 {
                    2.0 - 4.0 * x
                } else {
                    4.0 * x - 4.0
                }
            }
            Wave::Square => {
                if x < (0.5 + 0.45 * self.shape).clamp(0.05, 0.95) {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Random => {
                // Four steps a cycle; a positive shape glides from one to the next.
                let steps = phase.max(0.0) * 4.0;
                let step = steps.floor();
                let here = random(self.seed, step as u64);
                let next = random(self.seed, step as u64 + 1);
                let glide = self.shape.max(0.0);
                let t = steps - step;
                let smooth = t * t * (3.0 - 2.0 * t);
                here + (next - here) * smooth * glide
            }
        }
    }

    /// The swing's envelope at `t` beats into a stretch `total` beats long, 0 to 1.
    fn envelope(&self, t: f64, total: f64) -> f64 {
        let attack = self.attack * total;
        let decay = self.decay * total;
        let mut level: f64 = 1.0;
        if attack > 0.0 && t < attack {
            level = t / attack;
        }
        if decay > 0.0 && t > total - decay {
            level = level.min((total - t) / decay);
        }
        level.clamp(0.0, 1.0)
    }

    /// How far the LFO moves its curve `t` beats into a stretch `total` beats long: the wave
    /// (after the delay a positive shift gives), its amount and its envelope.
    pub fn swing(&self, t: f64, total: f64) -> f64 {
        if !(t.is_finite() && total.is_finite() && self.period > 0.0) {
            return 0.0;
        }
        let delay = self.shift.max(0.0) * self.period;
        if t < delay {
            return 0.0;
        }
        let phase = (t - delay) / self.period + (-self.shift).max(0.0);
        self.amount * self.envelope(t, total) * self.wave_at(phase)
    }

    /// The curve for a note `duration` beats long, its values kept in `kind`'s range, at most
    /// [`MAX_CURVE_POINTS`] points; a point between two of the same value is left out.
    pub fn curve(&self, kind: ExpressionKind, duration: f64) -> Vec<ExpressionPoint> {
        self.written(kind, duration, &[], None, false)
    }

    /// `existing` (a note's `kind` curve, `duration` beats long) with the LFO written over
    /// `range` (beats from the note's start; the whole note when `None`): replacing the curve
    /// there around `base`, or with `add` swinging around the curve as it is. Outside the range
    /// the curve stays, held up to the range's edges. The attack and decay are parts of the
    /// range. At most [`MAX_CURVE_POINTS`] points.
    pub fn written(
        &self,
        kind: ExpressionKind,
        duration: f64,
        existing: &[ExpressionPoint],
        range: Option<(f64, f64)>,
        add: bool,
    ) -> Vec<ExpressionPoint> {
        self.written_cut(kind, duration, existing, range, add, (0.0, 0.0))
    }

    /// [`Self::written`] for a stretch `lead` beats into a longer one, with `tail` beats of it
    /// after the range: the wave and its envelope as they are in that longer stretch.
    fn written_cut(
        &self,
        kind: ExpressionKind,
        duration: f64,
        existing: &[ExpressionPoint],
        range: Option<(f64, f64)>,
        add: bool,
        (lead, tail): (f64, f64),
    ) -> Vec<ExpressionPoint> {
        if !(duration.is_finite() && duration > 0.0) {
            return existing.to_vec();
        }
        let (from, to) = range.map_or((0.0, duration), |(a, b)| (a.max(0.0), b.min(duration)));
        let span = to - from;
        if !(span.is_finite() && span > 0.0) {
            return existing.to_vec();
        }
        let total = lead + span + tail;
        const EDGE: f64 = 2.0e-4;
        let before: Vec<ExpressionPoint> = existing.iter().copied().filter(|p| p.offset < from - EDGE).collect();
        let after: Vec<ExpressionPoint> = existing.iter().copied().filter(|p| p.offset > to + EDGE).collect();
        let at = |offset: f64| curve_value(existing, offset).unwrap_or(kind.neutral());
        let mut out: Vec<ExpressionPoint> = Vec::new();
        out.extend(before.iter().copied());
        // The curve as it was, held to the range's start, so the part before keeps its shape.
        if !before.is_empty() && from - EDGE > before.last().map_or(f64::NEG_INFINITY, |p| p.offset) {
            out.push(ExpressionPoint::new(from - EDGE, at(from - EDGE)));
        }
        let room = MAX_CURVE_POINTS.saturating_sub(out.len() + after.len() + 2).max(2);
        let step = (self.period / 32.0).max(LFO_STEP).max(span / (room - 1) as f64);
        let count = ((span / step).ceil() as usize).min(room - 1);
        let (low, high) = kind.range();
        for i in 0..=count {
            let local = (i as f64 * step).min(span);
            let offset = from + local;
            if out.last().is_some_and(|last| offset <= last.offset) {
                continue;
            }
            let centre = if add { at(offset) } else { self.base };
            let value = (centre + self.swing(lead + local, total)).clamp(low, high);
            out.push(ExpressionPoint::new(offset, value));
        }
        // The curve as it was again from the range's end.
        if !after.is_empty() && to + EDGE < after.first().map_or(f64::INFINITY, |p| p.offset) && to + EDGE <= duration {
            out.push(ExpressionPoint::new(to + EDGE, at(to + EDGE)));
        }
        out.extend(after.iter().copied());
        // A point between two of the same value changes nothing.
        let mut kept: Vec<ExpressionPoint> = Vec::with_capacity(out.len());
        for (i, point) in out.iter().enumerate() {
            let same_before = kept.last().is_some_and(|p| p.value == point.value);
            let same_after = out.get(i + 1).is_some_and(|p| p.value == point.value);
            if !(same_before && same_after) {
                kept.push(*point);
            }
        }
        kept
    }
}

/// A random number from -1 to 1 for `seed` and `step`, the same every time.
fn random(seed: u64, step: u64) -> f64 {
    let mut z = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(step).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
}

/// An LFO a note keeps: it rides on the note's `kind` curve from `from` to `to` beats after the
/// note's start, and is played, drawn and edited as settings until it is baked into points.
/// `lead` and `tail` are beats of it a cut took off before `from` and after `to` (a split, a
/// resize), so the part left keeps the wave and the envelope it had there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteLfo {
    pub kind: ExpressionKind,
    pub from: f64,
    pub to: f64,
    pub lead: f64,
    pub tail: f64,
    pub lfo: Lfo,
}

/// The shortest stretch a kept LFO may cover, in beats.
const MIN_LFO_SPAN: f64 = 1.0e-4;

impl NoteLfo {
    /// A kept LFO over `from` to `to`, nothing cut from it.
    pub fn new(kind: ExpressionKind, (from, to): (f64, f64), lfo: Lfo) -> Self {
        Self { kind, from, to, lead: 0.0, tail: 0.0, lfo }
    }

    /// How far it moves its curve `offset` beats after the note's start: nothing outside it.
    pub fn swing_at(&self, offset: f64) -> f64 {
        if !(offset >= self.from && offset <= self.to) {
            return 0.0;
        }
        let total = self.lead + (self.to - self.from) + self.tail;
        self.lfo.swing(self.lead + offset - self.from, total)
    }

    /// Checks it against a note `duration` beats long: its stretch inside the note and not
    /// empty, its cuts not negative, its settings in range.
    pub fn check(&self, duration: f64) -> Result<(), String> {
        let numbers = [self.from, self.to, self.lead, self.tail];
        if numbers.iter().any(|n| !n.is_finite()) {
            return Err("an LFO's stretch is not a finite number".into());
        }
        if self.from < 0.0 || self.to > duration || self.to - self.from < MIN_LFO_SPAN {
            return Err(format!(
                "an LFO's stretch {} to {} must be inside the note (0 to {duration}) and not empty",
                self.from, self.to
            ));
        }
        if self.lead < 0.0 || self.tail < 0.0 {
            return Err("an LFO's lead and tail must not be negative".into());
        }
        self.lfo.check(self.kind)
    }

    /// `curve` (the note's `kind` curve, the note `duration` beats long) with this LFO written in
    /// as points.
    pub fn baked(&self, duration: f64, curve: &[ExpressionPoint]) -> Vec<ExpressionPoint> {
        self.lfo.written_cut(self.kind, duration, curve, Some((self.from, self.to)), true, (self.lead, self.tail))
    }

    /// Cut to end at `end` (the note's end moved there): `None` when nothing of it is left.
    pub fn cropped_to(&self, end: f64) -> Option<Self> {
        if self.to <= end {
            return Some(*self);
        }
        (end - self.from >= MIN_LFO_SPAN).then(|| Self { to: end, tail: self.tail + (self.to - end), ..*self })
    }

    /// Moved `by` beats earlier (the note's start moved `by` later), cut at the new start:
    /// `None` when nothing of it is left.
    pub fn moved_earlier(&self, by: f64) -> Option<Self> {
        let (from, to) = (self.from - by, self.to - by);
        if to < MIN_LFO_SPAN {
            return None;
        }
        let cut = (-from).max(0.0);
        Some(Self { from: from.max(0.0), to, lead: self.lead + cut, ..*self })
    }

    /// Stretched by `factor` with the note: its stretch, its cuts and its cycle.
    pub fn stretched(&self, factor: f64) -> Self {
        let period = (self.lfo.period * factor).clamp(MIN_LFO_PERIOD, MAX_LFO_PERIOD);
        Self {
            from: self.from * factor,
            to: self.to * factor,
            lead: self.lead * factor,
            tail: self.tail * factor,
            lfo: Lfo { period, ..self.lfo },
            ..*self
        }
    }

    /// Split at `at` beats from the note's start: the part before and the part after (its
    /// offsets from `at`), each `None` when nothing of it is on that side.
    pub fn split(&self, at: f64) -> (Option<Self>, Option<Self>) {
        let left = self.cropped_to(at);
        let right = self.moved_earlier(at).filter(|r| r.to - r.from >= MIN_LFO_SPAN);
        let left = left.filter(|l| l.from < at);
        let right = right.filter(|_| self.to > at);
        (left, right)
    }
}

#[cfg(test)]
#[path = "lfo_tests.rs"]
mod tests;
