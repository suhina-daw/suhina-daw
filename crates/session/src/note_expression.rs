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
use crate::notes::{edit_notes, locate, note_miss, notes_of};
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
    let list =
        args.value("points").and_then(Value::as_array).ok_or("points must be a list of [offset, value] pairs")?;
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

/// One entry of `points`: `[offset, value]`, two finite numbers.
fn point(value: &Value, at: usize) -> Result<ExpressionPoint, CommandError> {
    let pair = value.as_array().filter(|pair| pair.len() == 2);
    let numbers = pair.and_then(|pair| Some((pair[0].as_f64()?, pair[1].as_f64()?)));
    match numbers {
        Some((offset, value)) if offset.is_finite() && value.is_finite() => Ok(ExpressionPoint::new(offset, value)),
        _ => Err(format!("point {at} must be [offset, value], two numbers").into()),
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
