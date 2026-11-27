// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `note.setExpression`: replaces one of a note's pitch, pressure and timbre curves
//! ([ADR 0039](../../docs/decisions/0039-per-note-expression.md) › 6,
//! [spec](../../docs/spec/mpe.md#note-curves-and-editing)). One command, one undo step, like the
//! other `note.*` commands; the curve is checked by the model's rules before anything changes.

use serde_json::{Value, json};
use suhina_model::expression::validate_curve;
use suhina_model::{
    ExpressionKind, ExpressionPoint, LaneData, MAX_CONTENT_EXPRESSION_POINTS, MAX_CURVE_POINTS, NoteId, Project,
};

use crate::commands::Args;
use crate::named::find_one;
use crate::notes::{edit_notes, indices, locate, note_miss, notes_of, velocity_of};
use crate::{CommandError, Session};

/// `note.setExpression`: `points` (`[[offset, value], …]`, offsets in beats from the note's start,
/// strictly increasing, within the note; values within the kind's range) becomes the note's
/// `kind` curve, the whole of it; `[]` removes the curve. Nothing is clamped or sorted: a point
/// out of order or out of range fails the command. The limits per curve and per clip content are
/// checked before the points are read.
pub(crate) fn set_expression(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let args = Args::new(params);
    let name = args.raw_str("kind").ok_or("kind must be \"pitch\", \"pressure\" or \"timbre\"")?;
    let kind = ExpressionKind::from_name(name)
        .ok_or_else(|| format!("kind must be \"pitch\", \"pressure\" or \"timbre\", not {name:?}"))?;
    let list = args
        .value("points")
        .and_then(Value::as_array)
        .ok_or("points must be a list of [offset, value] or [offset, value, curvature] entries")?;
    if list.len() > MAX_CURVE_POINTS {
        return Err(format!("a curve has at most {MAX_CURVE_POINTS} points, got {}", list.len()).into());
    }
    let item = args.value("note").ok_or("note must be given: the note's index, from 0, or {\"id\": n}")?;
    let notes = notes_of(&session.project, params)?;
    let index = find_one(notes, NoteId, item).map_err(note_miss)?;
    let note = notes.get(index).ok_or("a note is missing")?;
    // Every curve point of the content, but the curve this command replaces.
    let replaced = note.expression().map_or(0, |e| e.curve(kind).len());
    let others = content_points(&session.project, params)?.saturating_sub(replaced);
    let total = others.saturating_add(list.len());
    if total > MAX_CONTENT_EXPRESSION_POINTS {
        return Err(format!(
            "a clip's content holds at most {MAX_CONTENT_EXPRESSION_POINTS} expression points; this would make {total}"
        )
        .into());
    }
    let points = list.iter().enumerate().map(|(at, value)| point(value, at)).collect::<Result<Vec<_>, _>>()?;
    validate_curve(kind, &points, note.duration).map_err(|e| format!("note {index}: {e}"))?;
    let result = json!({ "note": index, "kind": kind.name(), "points": points.len() });
    edit_notes(&mut session.project, params, |notes| {
        let note = notes.get_mut(index).ok_or("a note is missing")?;
        if !(points.is_empty() && note.expression.is_none()) {
            *note.expression_mut().curve_mut(kind) = points;
            note.tidy_expression();
        }
        Ok(result)
    })
}

/// One entry of `points`: `[offset, value]`, or `[offset, value, curvature]` for a curved segment
/// after the point, finite numbers.
fn point(value: &Value, at: usize) -> Result<ExpressionPoint, CommandError> {
    let numbers = value.as_array().and_then(|entry| match entry.as_slice() {
        [offset, value] => Some((offset.as_f64()?, value.as_f64()?, 0.0)),
        [offset, value, curve] => Some((offset.as_f64()?, value.as_f64()?, curve.as_f64()?)),
        _ => None,
    });
    match numbers {
        Some((offset, value, curve)) if offset.is_finite() && value.is_finite() && curve.is_finite() => {
            Ok(ExpressionPoint::curved(offset, value, curve))
        }
        _ => Err(format!("point {at} must be [offset, value] or [offset, value, curvature], numbers").into()),
    }
}

/// How many expression points the notes of the content of the clip `params` names hold, in all
/// its lanes.
fn content_points(p: &Project, params: &Value) -> Result<usize, CommandError> {
    let (_, id, _) = locate(p, params)?;
    let content = p.content(id).ok_or("the clip's content is missing")?;
    Ok(content
        .lanes
        .iter()
        .filter_map(|lane| match &lane.data {
            LaneData::Notes(notes) => Some(notes),
            LaneData::Audio(_) => None,
        })
        .flat_map(|notes| notes.iter())
        .filter_map(|note| note.expression())
        .map(|expression| expression.point_count())
        .sum())
}

/// The kind a command's `kind` names.
fn kind_of(args: &Args<'_>) -> Result<ExpressionKind, CommandError> {
    let name = args.raw_str("kind").ok_or("kind must be \"pitch\", \"pressure\" or \"timbre\"")?;
    ExpressionKind::from_name(name)
        .ok_or_else(|| format!("kind must be \"pitch\", \"pressure\" or \"timbre\", not {name:?}").into())
}

/// The notes a command acts on: `notes` when given, every note of the clip when not.
fn notes_or_all(params: &Value, notes: &suhina_model::Notes) -> Result<Vec<usize>, CommandError> {
    if Args::new(params).value("notes").is_some() { indices(params, notes) } else { Ok((0..notes.len()).collect()) }
}

/// `note.clearExpression`: every curve of `notes` goes. One undo step.
pub(crate) fn clear_expression(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    edit_notes(&mut session.project, params, |notes| {
        let chosen = indices(params, notes)?;
        let mut cleared = Vec::new();
        for &index in &chosen {
            if let Some(note) = notes.get_mut(index).filter(|n| n.expression.is_some()) {
                note.expression = None;
                cleared.push(index);
            }
        }
        Ok(json!({ "notes": cleared }))
    })
}

/// `note.scaleExpression`: every value of `notes`' pressure or timbre curve times `factor`, each
/// kept from 0 to 1. Pitch is not scaled. One undo step.
pub(crate) fn scale_expression(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let args = Args::new(params);
    let kind = kind_of(&args)?;
    if kind == ExpressionKind::Pitch {
        return Err("pitch curves are not scaled: scale pressure or timbre".into());
    }
    let factor = args.opt_f64("factor")?.ok_or("factor must be given, 0 or more")?;
    if !(factor.is_finite() && (0.0..=127.0).contains(&factor)) {
        return Err(format!("factor must be 0 to 127, got {factor}").into());
    }
    edit_notes(&mut session.project, params, |notes| {
        let chosen = indices(params, notes)?;
        let mut scaled = Vec::new();
        for &index in &chosen {
            let Some(note) = notes.get_mut(index).filter(|n| n.expression().is_some_and(|e| !e.curve(kind).is_empty()))
            else {
                continue;
            };
            for point in note.expression_mut().curve_mut(kind).iter_mut() {
                point.value = (point.value * factor).clamp(0.0, 1.0);
            }
            scaled.push(index);
        }
        Ok(json!({ "notes": scaled }))
    })
}

/// `note.setReleaseVelocity`: `velocity` (1 to 127) for every note of `notes`, or `velocities`,
/// one per entry of `notes`. One undo step.
pub(crate) fn set_release_velocity(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let args = Args::new(params);
    let velocity = args.opt_f64("velocity")?;
    let each: Option<Vec<f64>> = args
        .value("velocities")
        .map(|v| {
            v.as_array()
                .ok_or("velocities must be a list of numbers")?
                .iter()
                .map(|n| n.as_f64().filter(|n| (1.0..=127.0).contains(n)).ok_or("each velocity must be 1 to 127"))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    match (velocity, &each) {
        (Some(_), Some(_)) | (None, None) => return Err("give velocity (1 to 127) or velocities (one per note)".into()),
        (Some(v), None) if !(1.0..=127.0).contains(&v) => {
            return Err(format!("velocity must be 1 to 127, got {v}").into());
        }
        _ => {}
    }
    edit_notes(&mut session.project, params, |notes| {
        let chosen = indices(params, notes)?;
        if let Some(each) = &each
            && each.len() != chosen.len()
        {
            return Err(format!(
                "velocities must have one value per note: {} notes, {} velocities",
                chosen.len(),
                each.len()
            )
            .into());
        }
        for (at, &index) in chosen.iter().enumerate() {
            let midi = each.as_ref().and_then(|e| e.get(at).copied()).or(velocity).unwrap_or(64.0);
            if let Some(note) = notes.get_mut(index) {
                note.release_velocity = velocity_of(midi.round());
            }
        }
        Ok(json!({ "notes": chosen }))
    })
}

/// The waves `note.lfoExpression` draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Wave {
    Sine,
    Square,
    Triangle,
    Random,
}

impl Wave {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "sine" => Some(Wave::Sine),
            "square" => Some(Wave::Square),
            "triangle" => Some(Wave::Triangle),
            "random" => Some(Wave::Random),
            _ => None,
        }
    }
}

/// The settings of one `note.lfoExpression`, checked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Lfo {
    pub(crate) wave: Wave,
    /// -1 to 1: the skew of sine and triangle, the pulse width of square, glide of random.
    pub(crate) shape: f64,
    /// Beats per cycle.
    pub(crate) period: f64,
    /// -1 to 1 of a cycle: positive delays the start, negative moves the phase.
    pub(crate) shift: f64,
    /// How far the wave swings either way, in the kind's units (negative turns it over).
    pub(crate) amount: f64,
    /// The value it swings around.
    pub(crate) base: f64,
    /// Parts of the note, 0 to 1, together at most 1.
    pub(crate) attack: f64,
    pub(crate) decay: f64,
    pub(crate) seed: u64,
}

/// The fewest beats between two points of an LFO curve.
const LFO_STEP: f64 = 1.0 / 256.0;

impl Lfo {
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

    /// The swing's envelope at `offset` beats into a note `duration` long, 0 to 1.
    fn envelope(&self, offset: f64, duration: f64) -> f64 {
        let attack = self.attack * duration;
        let decay = self.decay * duration;
        let mut level: f64 = 1.0;
        if attack > 0.0 && offset < attack {
            level = offset / attack;
        }
        if decay > 0.0 && offset > duration - decay {
            level = level.min((duration - offset) / decay);
        }
        level.clamp(0.0, 1.0)
    }

    /// The curve for a note `duration` beats long, its values kept in `kind`'s range, at most
    /// [`MAX_CURVE_POINTS`] points; a point between two of the same value is left out.
    pub(crate) fn curve(&self, kind: ExpressionKind, duration: f64) -> Vec<ExpressionPoint> {
        if !(duration.is_finite() && duration > 0.0) {
            return Vec::new();
        }
        let step = (self.period / 32.0).max(LFO_STEP).max(duration / (MAX_CURVE_POINTS - 1) as f64);
        let count = ((duration / step).ceil() as usize).min(MAX_CURVE_POINTS - 1);
        let (low, high) = kind.range();
        let mut points: Vec<ExpressionPoint> = Vec::with_capacity(count + 1);
        for i in 0..=count {
            let offset = (i as f64 * step).min(duration);
            if points.last().is_some_and(|last| offset <= last.offset) {
                continue;
            }
            let delay = self.shift.max(0.0) * self.period;
            let swing = if offset < delay {
                0.0
            } else {
                let phase = (offset - delay) / self.period + (-self.shift).max(0.0);
                self.wave_at(phase)
            };
            let value = (self.base + self.amount * self.envelope(offset, duration) * swing).clamp(low, high);
            points.push(ExpressionPoint::new(offset, value));
        }
        // A point between two of the same value changes nothing.
        let mut kept: Vec<ExpressionPoint> = Vec::with_capacity(points.len());
        for (i, point) in points.iter().enumerate() {
            let same_before = kept.last().is_some_and(|p| p.value == point.value);
            let same_after = points.get(i + 1).is_some_and(|p| p.value == point.value);
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

/// `note.lfoExpression`: an LFO written into one curve of `notes` (every note when not given),
/// replacing it. One undo step.
pub(crate) fn lfo_expression(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let args = Args::new(params);
    let kind = kind_of(&args)?;
    let wave_name = args.raw_str("wave").unwrap_or("sine");
    let wave = Wave::from_name(wave_name)
        .ok_or_else(|| format!("wave must be \"sine\", \"square\", \"triangle\" or \"random\", not {wave_name:?}"))?;
    let number = |name: &str, default: f64, (low, high): (f64, f64)| -> Result<f64, CommandError> {
        let value = args.opt_f64(name)?.unwrap_or(default);
        if value.is_finite() && (low..=high).contains(&value) {
            Ok(value)
        } else {
            Err(format!("{name} must be {low} to {high}, got {value}").into())
        }
    };
    let (low, high) = kind.range();
    let reach = if kind == ExpressionKind::Pitch { 96.0 } else { 1.0 };
    let lfo = Lfo {
        wave,
        shape: number("shape", 0.0, (-1.0, 1.0))?,
        period: number("period", 1.0, (1.0 / 32.0, 64.0))?,
        shift: number("shift", 0.0, (-1.0, 1.0))?,
        amount: number("amount", if kind == ExpressionKind::Pitch { 1.0 } else { 0.5 }, (-reach, reach))?,
        base: number("base", kind.neutral(), (low, high))?,
        attack: number("attack", 0.0, (0.0, 1.0))?,
        decay: number("decay", 0.0, (0.0, 1.0))?,
        seed: args.opt_f64("seed")?.map_or(1, |s| s.abs() as u64),
    };
    if lfo.attack + lfo.decay > 1.0 + 1e-9 {
        return Err("attack and decay together must be at most 1".into());
    }
    let notes = notes_of(&session.project, params)?;
    let chosen = notes_or_all(params, notes)?;
    let curves: Vec<(usize, Vec<ExpressionPoint>)> =
        chosen.iter().filter_map(|&i| notes.get(i).map(|n| (i, lfo.curve(kind, n.duration)))).collect();
    let replaced: usize =
        chosen.iter().filter_map(|&i| notes.get(i)).filter_map(|n| n.expression()).map(|e| e.curve(kind).len()).sum();
    let added: usize = curves.iter().map(|(_, c)| c.len()).sum();
    let total = content_points(&session.project, params)?.saturating_sub(replaced).saturating_add(added);
    if total > MAX_CONTENT_EXPRESSION_POINTS {
        return Err(format!(
            "a clip's content holds at most {MAX_CONTENT_EXPRESSION_POINTS} expression points; this would make {total}"
        )
        .into());
    }
    edit_notes(&mut session.project, params, |notes| {
        let mut written = Vec::new();
        for (index, curve) in curves {
            if let Some(note) = notes.get_mut(index) {
                *note.expression_mut().curve_mut(kind) = curve;
                note.tidy_expression();
                written.push(index);
            }
        }
        Ok(json!({ "notes": written, "kind": kind.name() }))
    })
}

#[cfg(test)]
mod lfo_tests {
    use super::*;

    fn lfo(wave: Wave) -> Lfo {
        Lfo { wave, shape: 0.0, period: 1.0, shift: 0.0, amount: 2.0, base: 0.0, attack: 0.0, decay: 0.0, seed: 1 }
    }

    #[test]
    fn a_sine_swings_around_its_base_by_its_amount() {
        let curve = lfo(Wave::Sine).curve(ExpressionKind::Pitch, 4.0);
        assert!(curve.windows(2).all(|w| w[1].offset > w[0].offset));
        let at = |o: f64| curve.iter().find(|p| (p.offset - o).abs() < 1e-9).map(|p| p.value).unwrap();
        assert!((at(0.25) - 2.0).abs() < 1e-9);
        assert!((at(0.75) + 2.0).abs() < 1e-9);
        assert!(at(0.0).abs() < 1e-9);
    }

    #[test]
    fn values_stay_in_the_kinds_range_and_the_envelope_starts_from_nothing() {
        let wide = Lfo { amount: 1.0, base: 0.9, attack: 0.5, ..lfo(Wave::Square) };
        let curve = wide.curve(ExpressionKind::Pressure, 2.0);
        assert!(curve.iter().all(|p| (0.0..=1.0).contains(&p.value)));
        assert!((curve[0].value - 0.9).abs() < 1e-9, "no swing at the start of the attack");
    }

    #[test]
    fn a_long_note_stays_within_the_curve_limit_and_random_repeats_its_seed() {
        let fast = Lfo { period: 1.0 / 32.0, ..lfo(Wave::Random) };
        let curve = fast.curve(ExpressionKind::Timbre, 64.0);
        assert!(curve.len() <= MAX_CURVE_POINTS);
        assert_eq!(curve, fast.curve(ExpressionKind::Timbre, 64.0));
        assert_ne!(curve, Lfo { seed: 2, ..fast }.curve(ExpressionKind::Timbre, 64.0));
        assert!(validate_curve(ExpressionKind::Timbre, &curve, 64.0).is_ok());
    }

    #[test]
    fn a_positive_shift_waits_before_the_swing() {
        let late = Lfo { shift: 0.5, ..lfo(Wave::Square) };
        let curve = late.curve(ExpressionKind::Pitch, 2.0);
        assert!(curve.iter().filter(|p| p.offset < 0.5).all(|p| p.value == 0.0));
        assert!(curve.iter().any(|p| p.offset > 0.5 && p.value != 0.0));
    }
}
