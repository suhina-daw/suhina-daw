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

pub use suhina_model::lfo::{Lfo, Wave};
use suhina_model::lfo::{MAX_NOTE_LFOS, NoteLfo};

/// An LFO's settings from `args`: each one given replaces `base`'s, checked for `kind`.
fn lfo_settings(args: &Args<'_>, kind: ExpressionKind, base: Lfo) -> Result<Lfo, CommandError> {
    let wave = match args.raw_str("wave") {
        None => base.wave,
        Some(name) => Wave::from_name(name)
            .ok_or_else(|| format!("wave must be \"sine\", \"square\", \"triangle\" or \"random\", not {name:?}"))?,
    };
    let lfo = Lfo {
        wave,
        shape: args.opt_f64("shape")?.unwrap_or(base.shape),
        period: args.opt_f64("period")?.unwrap_or(base.period),
        shift: args.opt_f64("shift")?.unwrap_or(base.shift),
        amount: args.opt_f64("amount")?.unwrap_or(base.amount),
        base: args.opt_f64("base")?.unwrap_or(base.base),
        attack: args.opt_f64("attack")?.unwrap_or(base.attack),
        decay: args.opt_f64("decay")?.unwrap_or(base.decay),
        seed: args.opt_f64("seed")?.map_or(base.seed, |s| s.abs() as u64),
    };
    lfo.check(kind)?;
    Ok(lfo)
}

/// The LFO a `note.lfoExpression` starts from: a sine of one beat, a semitone (pitch) or half the
/// range wide, around the kind's neutral value.
fn default_lfo(kind: ExpressionKind) -> Lfo {
    Lfo {
        wave: Wave::Sine,
        shape: 0.0,
        period: 1.0,
        shift: 0.0,
        amount: if kind == ExpressionKind::Pitch { 1.0 } else { 0.5 },
        base: kind.neutral(),
        attack: 0.0,
        decay: 0.0,
        seed: 1,
    }
}

/// `note.lfoExpression`: an LFO written into one curve of `notes` (every note when not given),
/// replacing it. One undo step.
pub(crate) fn lfo_expression(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let args = Args::new(params);
    let kind = kind_of(&args)?;
    let range = match (args.opt_f64("from")?, args.opt_f64("to")?) {
        (Some(from), Some(to)) if from.is_finite() && to.is_finite() && to > from => Some((from, to)),
        (None, None) => None,
        _ => return Err("from and to must both be given, to after from, in the clip's beats".into()),
    };
    let (add, keep) = match args.raw_str("mode").unwrap_or("replace") {
        "replace" => (false, false),
        "add" => (true, false),
        "keep" => (true, true),
        other => return Err(format!("mode must be \"replace\", \"add\" or \"keep\", not {other:?}").into()),
    };
    let lfo = lfo_settings(&args, kind, default_lfo(kind))?;
    if keep {
        return keep_lfo(session, params, kind, range, lfo);
    }
    let notes = notes_of(&session.project, params)?;
    let chosen = notes_or_all(params, notes)?;
    let curves: Vec<(usize, Vec<ExpressionPoint>)> = chosen
        .iter()
        .filter_map(|&i| notes.get(i).map(|n| (i, n)))
        // A range that misses a note leaves it as it is.
        .filter(|(_, n)| range.is_none_or(|(from, to)| from < n.time + n.duration && to > n.time))
        .map(|(i, n)| {
            let existing = n.expression().map_or(&[][..], |e| e.curve(kind));
            let local = range.map(|(from, to)| (from - n.time, to - n.time));
            (i, lfo.written(kind, n.duration, existing, local, add))
        })
        .collect();
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

/// `note.lfoExpression` with `mode` "keep": the LFO kept on each chosen note over `range` (the
/// clip's beats; the whole note when `None`), riding on its `kind` curve, editable afterwards.
fn keep_lfo(
    session: &mut Session,
    params: &Value,
    kind: ExpressionKind,
    range: Option<(f64, f64)>,
    lfo: Lfo,
) -> Result<Value, CommandError> {
    let notes = notes_of(&session.project, params)?;
    let chosen = notes_or_all(params, notes)?;
    let kept: Vec<(usize, NoteLfo)> = chosen
        .iter()
        .filter_map(|&i| notes.get(i).map(|n| (i, n)))
        .filter_map(|(i, n)| {
            let (from, to) = range.map_or((0.0, n.duration), |(from, to)| (from - n.time, to - n.time));
            let (from, to) = (from.max(0.0), to.min(n.duration));
            // A range that misses a note leaves it as it is.
            (to - from > 1.0e-4).then(|| (i, NoteLfo::new(kind, (from, to), lfo)))
        })
        .collect();
    for (index, _) in &kept {
        let count = notes.get(*index).and_then(|n| n.expression()).map_or(0, |e| e.lfos.len());
        if count >= MAX_NOTE_LFOS {
            return Err(format!("note {index} keeps {count} LFOs already, the most a note keeps").into());
        }
    }
    edit_notes(&mut session.project, params, |notes| {
        let mut written = Vec::new();
        for (index, lfo) in kept {
            if let Some(note) = notes.get_mut(index) {
                note.expression_mut().lfos.push(lfo);
                written.push(index);
            }
        }
        Ok(json!({ "notes": written, "kind": kind.name(), "kept": true }))
    })
}

/// The note `note` names and its kept LFO `lfo` (its index, from 0).
fn kept_lfo(session: &Session, params: &Value) -> Result<(usize, usize, NoteLfo), CommandError> {
    let args = Args::new(params);
    let item = args.value("note").ok_or("note must be given: the note's index, from 0, or {\"id\": n}")?;
    let notes = notes_of(&session.project, params)?;
    let index = find_one(notes, NoteId, item).map_err(note_miss)?;
    let note = notes.get(index).ok_or("a note is missing")?;
    let which = args.opt_f64("lfo")?.ok_or("lfo must be given: which of the note's LFOs, from 0")?;
    let kept = note.expression().map_or(&[][..], |e| e.lfos.as_slice());
    let at = (which >= 0.0 && which.fract() == 0.0).then_some(which as usize);
    let lfo = at.and_then(|i| kept.get(i)).ok_or_else(|| {
        format!("note {index} keeps {} LFOs; lfo must be a whole number from 0, got {which}", kept.len())
    })?;
    Ok((index, at.unwrap_or(0), *lfo))
}

/// `note.setLfo`: changes a kept LFO's settings and, with `from` and `to` (the clip's beats), the
/// stretch it covers; a setting left out stays. A new stretch is whole: nothing cut off it.
pub(crate) fn set_lfo(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let args = Args::new(params);
    let (index, at, old) = kept_lfo(session, params)?;
    let notes = notes_of(&session.project, params)?;
    let note = notes.get(index).ok_or("a note is missing")?;
    let mut lfo = NoteLfo { lfo: lfo_settings(&args, old.kind, old.lfo)?, ..old };
    match (args.opt_f64("from")?, args.opt_f64("to")?) {
        (None, None) => {}
        (Some(from), Some(to)) => {
            let (from, to) = ((from - note.time).max(0.0), (to - note.time).min(note.duration));
            lfo = NoteLfo { from, to, lead: 0.0, tail: 0.0, ..lfo };
        }
        _ => return Err("from and to must both be given, in the clip's beats".into()),
    }
    lfo.check(note.duration).map_err(|why| format!("note {index}: {why}"))?;
    edit_notes(&mut session.project, params, |notes| {
        let slot =
            notes.get_mut(index).and_then(|n| n.expression_mut().lfos.get_mut(at)).ok_or("the LFO is missing")?;
        *slot = lfo;
        Ok(json!({ "note": index, "lfo": at }))
    })
}

/// `note.removeLfo`: the kept LFO goes; the curve it rode on stays.
pub(crate) fn remove_lfo(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let (index, at, _) = kept_lfo(session, params)?;
    edit_notes(&mut session.project, params, |notes| {
        let note = notes.get_mut(index).ok_or("a note is missing")?;
        if at < note.expression().map_or(0, |e| e.lfos.len()) {
            note.expression_mut().lfos.remove(at);
        }
        note.tidy_expression();
        Ok(json!({ "note": index, "lfo": at }))
    })
}

/// `note.bakeLfo`: every LFO `notes` keep (every note's when left out) written into their curves
/// as points, sounding as it did, and kept no longer.
pub(crate) fn bake_lfo(session: &mut Session, params: &Value) -> Result<Value, CommandError> {
    let notes = notes_of(&session.project, params)?;
    let chosen = notes_or_all(params, notes)?;
    let baked: Vec<(usize, suhina_model::NoteExpression)> = chosen
        .iter()
        .filter_map(|&i| notes.get(i).map(|n| (i, n)))
        .filter_map(|(i, n)| n.expression().filter(|e| !e.lfos.is_empty()).map(|e| (i, e.baked(n.duration))))
        .collect();
    let replaced: usize =
        baked.iter().filter_map(|(i, _)| notes.get(*i)).filter_map(|n| n.expression()).map(|e| e.point_count()).sum();
    let added: usize = baked.iter().map(|(_, e)| e.point_count()).sum();
    let total = content_points(&session.project, params)?.saturating_sub(replaced).saturating_add(added);
    if total > MAX_CONTENT_EXPRESSION_POINTS {
        return Err(format!(
            "a clip's content holds at most {MAX_CONTENT_EXPRESSION_POINTS} expression points; baking would make {total}"
        )
        .into());
    }
    edit_notes(&mut session.project, params, |notes| {
        let mut written = Vec::new();
        for (index, expression) in baked {
            if let Some(note) = notes.get_mut(index) {
                *note.expression_mut() = expression;
                note.tidy_expression();
                written.push(index);
            }
        }
        Ok(json!({ "notes": written }))
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

#[cfg(test)]
mod lfo_range_tests {
    use super::*;

    fn vibrato() -> Lfo {
        Lfo {
            wave: Wave::Sine,
            shape: 0.0,
            period: 0.5,
            shift: 0.0,
            amount: 0.5,
            base: 0.0,
            attack: 0.0,
            decay: 0.0,
            seed: 1,
        }
    }

    #[test]
    fn added_in_a_range_it_rides_the_bend_and_keeps_the_curve_outside() {
        // A bend up two semitones over the first beat, held after it.
        let bend = vec![ExpressionPoint::new(0.0, 0.0), ExpressionPoint::new(1.0, 2.0)];
        let curve = vibrato().written(ExpressionKind::Pitch, 4.0, &bend, Some((2.0, 4.0)), true);
        assert_eq!(&curve[..2], &bend[..], "the bend before the range stays");
        assert!(curve.windows(2).all(|w| w[1].offset > w[0].offset));
        let inside: Vec<_> = curve.iter().filter(|p| p.offset >= 2.0).collect();
        assert!(inside.iter().all(|p| (p.value - 2.0).abs() <= 0.5 + 1e-9), "it swings around the bend's 2");
        assert!(inside.iter().any(|p| p.value > 2.4) && inside.iter().any(|p| p.value < 1.6));
        assert!(validate_curve(ExpressionKind::Pitch, &curve, 4.0).is_ok());
    }

    #[test]
    fn replaced_in_a_range_the_curve_after_it_comes_back() {
        let ramp = vec![ExpressionPoint::new(0.0, 0.0), ExpressionPoint::new(4.0, 4.0)];
        let curve = vibrato().written(ExpressionKind::Pitch, 4.0, &ramp, Some((1.0, 2.0)), false);
        assert_eq!(curve.last(), ramp.last());
        let at_three = suhina_model::expression::curve_value(&curve, 3.0).unwrap();
        assert!((at_three - 3.0).abs() < 1e-3, "{at_three}");
        assert!(validate_curve(ExpressionKind::Pitch, &curve, 4.0).is_ok());
    }
}
