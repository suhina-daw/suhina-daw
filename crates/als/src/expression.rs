// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A MIDI clip's per-note expression: the lists in `Notes/PerNoteEventStore` become the notes'
//! pitch, pressure and timbre curves (`docs/spec/als-format.md` › Notes › Per-note expression).

use std::collections::HashMap;

use roxmltree::Node;
use suhina_model::{ExpressionKind, ExpressionPoint, MAX_CONTENT_EXPRESSION_POINTS, MAX_CURVE_POINTS, Note};

use crate::{parse_number, path};

/// A per-note pitch value per semitone: ±8191.5, the 14-bit half range, is ±48 semitones.
const PITCH_PER_SEMITONE: f64 = 8191.5 / 48.0;

/// What a list's `CC` moves. `-2` (pitch) is observed; `-1` (pressure) and `74` (slide) are read
/// as 0 … 127 on a best-effort basis until a set we authored shows them. Anything else is left out.
fn kind(cc: &str) -> Option<ExpressionKind> {
    match cc.trim() {
        "-2" => Some(ExpressionKind::Pitch),
        "-1" => Some(ExpressionKind::Pressure),
        "74" => Some(ExpressionKind::Timbre),
        _ => None,
    }
}

/// A stored value in the kind's Suhina units, clamped into its range.
fn convert(kind: ExpressionKind, value: f64) -> Option<f64> {
    let value = match kind {
        ExpressionKind::Pitch => value / PITCH_PER_SEMITONE,
        ExpressionKind::Pressure | ExpressionKind::Timbre => value / 127.0,
    };
    kind.clamp(value)
}

/// A `NoteId` attribute, the key that ties a list to its note.
pub(crate) fn note_id(text: &str) -> Option<i64> {
    text.trim().parse().ok()
}

/// What was left out of one clip's expression, for its warnings.
#[derive(Debug, Default)]
pub(crate) struct Report {
    unknown_note: usize,
    unknown_kind: usize,
    repeated: usize,
    bad_points: usize,
    cut_short: usize,
    over_budget: usize,
    invalid: usize,
}

impl Report {
    pub(crate) fn messages(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut say = |count: usize, text: &str| {
            if count > 0 {
                out.push(format!("{count} {text}"));
            }
        };
        say(self.unknown_note, "per-note expression lists name no note of the clip; left out");
        say(self.unknown_kind, "per-note expression lists move something Suhina does not read; left out");
        say(self.repeated, "per-note expression lists repeat a note's curve; the first is kept");
        say(self.bad_points, "per-note expression points are not numbers; left out");
        say(self.cut_short, "per-note curves reach the point limit; their later points are left out");
        say(
            self.over_budget,
            &format!("per-note curves are past the {MAX_CONTENT_EXPRESSION_POINTS} points a clip keeps; left out"),
        );
        say(self.invalid, "notes' per-note curves could not be read; left out");
        out
    }
}

/// Reads the per-note expression stored in a clip's `Notes` element onto `notes`, whose
/// `NoteId`s are `ids` in the same order. Offsets stay relative to each note's start, so the
/// clip's loop and start markers change nothing here.
///
/// Limits truncate, as the rest of the importer does: a curve keeps its first
/// [`MAX_CURVE_POINTS`] points in file order, and once the clip's curves hold
/// [`MAX_CONTENT_EXPRESSION_POINTS`] points the later lists are left out. Nothing is allocated
/// past either limit.
pub(crate) fn read(notes_node: Node, notes: &mut [Note], ids: &[Option<i64>], report: &mut Report) {
    let Some(lists) = path(notes_node, &["PerNoteEventStore", "EventLists"]) else { return };
    let mut by_id: HashMap<i64, usize> = HashMap::new();
    for (index, id) in ids.iter().enumerate() {
        if let Some(id) = id {
            by_id.entry(*id).or_insert(index);
        }
    }
    let mut budget = MAX_CONTENT_EXPRESSION_POINTS;
    for list in lists.children().filter(|n| n.has_tag_name("PerNoteEventList")) {
        let Some(kind) = list.attribute("CC").and_then(kind) else {
            report.unknown_kind = report.unknown_kind.saturating_add(1);
            continue;
        };
        let note = list.attribute("NoteId").and_then(note_id).and_then(|id| by_id.get(&id)).copied();
        let Some(note) = note.and_then(|index| notes.get_mut(index)) else {
            report.unknown_note = report.unknown_note.saturating_add(1);
            continue;
        };
        if note.expression().is_some_and(|e| !e.curve(kind).is_empty()) {
            report.repeated = report.repeated.saturating_add(1);
            continue;
        }
        let events = path(list, &["Events"])
            .into_iter()
            .flat_map(|events| events.children())
            .filter(|n| n.has_tag_name("PerNoteEvent"));
        let cap = MAX_CURVE_POINTS.min(budget);
        let duration = note.duration.max(0.0);
        let mut points: Vec<ExpressionPoint> = Vec::new();
        for event in events {
            let offset = event.attribute("TimeOffset").and_then(parse_number);
            let value = event.attribute("Value").and_then(parse_number).and_then(|v| convert(kind, v));
            let (Some(offset), Some(value)) = (offset, value) else {
                report.bad_points = report.bad_points.saturating_add(1);
                continue;
            };
            if points.len() >= cap {
                if cap < MAX_CURVE_POINTS {
                    report.over_budget = report.over_budget.saturating_add(1);
                } else {
                    report.cut_short = report.cut_short.saturating_add(1);
                }
                break;
            }
            points.push(ExpressionPoint::new(offset.clamp(0.0, duration), value));
        }
        // Stable, and -0 sorts with 0, so of points at one offset the file's last stays.
        points.sort_by(|a, b| a.offset.partial_cmp(&b.offset).unwrap_or(std::cmp::Ordering::Equal));
        points.dedup_by(|later, kept| {
            let same = later.offset == kept.offset;
            if same {
                *kept = *later;
            }
            same
        });
        if points.is_empty() {
            continue;
        }
        budget = budget.saturating_sub(points.len());
        *note.expression_mut().curve_mut(kind) = points;
    }
    for note in notes.iter_mut() {
        if note.expression.as_deref().is_some_and(|e| e.validate(note.duration).is_err()) {
            report.invalid = report.invalid.saturating_add(1);
            note.expression = None;
        }
        note.tidy_expression();
    }
}
