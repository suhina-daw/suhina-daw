// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! MPE in the piano roll (#930): each note's own pitch curve drawn over it, from the note's row to
//! the rows it bends to, and its points edited where they are. The selected notes' curves are
//! bright with a handle on every point; the others are faint, so the clip's whole gesture shows.
//!
//! - A double-click on the empty grid in the time of a selected note adds a point to the selected
//!   note whose row is nearest: at the beat under the pointer, bending to the key under it. A
//!   curve's first point starts it from the note's own pitch.
//! - A handle dragged moves its point between its neighbours, on the grid and on whole semitones;
//!   Alt frees both.
//! - A handle double-clicked removes its point.
//!
//! Pressure and slide (timbre) are drawn and edited the same way in the lane below the notes,
//! in place of the velocity stems, 0 at its foot and 1 at its top; the lane's name at its left
//! switches it between Velocity, Pressure and Slide.
//!
//! Each edit is one `note.setExpression` that replaces the note's curve, one undo step.

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use serde_json::{Value, json};
use suhina_engine::model::{ExpressionKind, ExpressionPoint, Note};

use super::{Layout, PianoClip, PianoRollState, nearest_line};
use crate::interaction::key_name::key_name;
use crate::interaction::timeline_grid::TimelineGrid;
use crate::kit::widgets::Action;

/// How far from a point, in points, a press grabs it.
const HANDLE: f32 = 5.0;
/// The smallest gap kept between two points' offsets while one is dragged, in beats.
const GAP: f64 = 1.0e-4;

/// A point being dragged: the note, the curve as it is while dragged, and the point's place in it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PointDrag {
    pub(crate) note: usize,
    pub(crate) kind: ExpressionKind,
    pub(crate) index: usize,
    pub(crate) curve: Vec<ExpressionPoint>,
}

/// The y of the middle of a row `pitch` stands for, `pitch` a key and a fraction of a semitone
/// over it: between the rows of the keys either side of it, so folded rows bend to the rows
/// that are there. `None` when no key at or below and at or above it has a row.
pub(super) fn pitch_y(layout: &Layout, pitch: f64) -> Option<f32> {
    let pitch = pitch.clamp(0.0, 127.0);
    let middle = |key: u8| layout.rows.row_of(key).map(|_| layout.y_of(key) + layout.key_height / 2.0);
    let low = pitch.floor() as u8;
    let below = (0..=low).rev().find_map(|key| middle(key).map(|y| (f64::from(key), y)));
    let above = ((pitch.ceil() as u8)..=127).find_map(|key| middle(key).map(|y| (f64::from(key), y)));
    match (below, above) {
        (Some((k0, y0)), Some((k1, y1))) if k1 > k0 => Some(y0 + (y1 - y0) * ((pitch - k0) / (k1 - k0)) as f32),
        (Some((_, y)), _) | (None, Some((_, y))) => Some(y),
        (None, None) => None,
    }
}

/// The pitch, in keys and fractions of one, that a y stands for: the inverse of [`pitch_y`],
/// found among the rows shown.
pub(super) fn pitch_at(layout: &Layout, y: f32) -> Option<f64> {
    let mut rows: Vec<(f64, f32)> = (0..=127u8)
        .filter(|key| layout.rows.row_of(*key).is_some())
        .map(|key| (f64::from(key), layout.y_of(key) + layout.key_height / 2.0))
        .collect();
    // Higher keys are higher up: sort by y, top first.
    rows.sort_by(|a, b| a.1.total_cmp(&b.1));
    let first = rows.first().copied()?;
    let last = rows.last().copied()?;
    if y <= first.1 {
        return Some(first.0 + f64::from((first.1 - y) / layout.key_height));
    }
    if y >= last.1 {
        return Some(last.0 - f64::from((y - last.1) / layout.key_height));
    }
    let at = rows.partition_point(|row| row.1 <= y);
    let (upper, lower) = (rows.get(at.checked_sub(1)?)?, rows.get(at)?);
    let t = f64::from((y - upper.1) / (lower.1 - upper.1).max(f32::EPSILON));
    Some(upper.0 + (lower.0 - upper.0) * t)
}

/// The screen points of a note's pitch curve: from the note's start at its first value, along
/// its points, held to the note's end. Empty for a note without a pitch curve.
pub(super) fn pitch_line(layout: &Layout, note: &Note, curve: &[ExpressionPoint]) -> Vec<Pos2> {
    let Some(first) = curve.first() else { return Vec::new() };
    let key = f64::from(note.key);
    let at = |offset: f64, value: f64| {
        pitch_y(layout, key + value).map(|y| Pos2::new(layout.x_of(note.time + offset.clamp(0.0, note.duration)), y))
    };
    let mut line = Vec::with_capacity(curve.len() + 2);
    line.extend(at(0.0, first.value));
    line.extend(curve.iter().filter_map(|p| at(p.offset, p.value)));
    if let Some(last) = curve.last() {
        line.extend(at(note.duration, last.value));
    }
    line
}

/// The height in the lane of a pressure or slide value, 0 at its foot and 1 at its top.
pub(super) fn lane_y(lane: Rect, value: f64) -> f32 {
    lane.bottom() - 2.0 - (lane.height() - 4.0).max(0.0) * value.clamp(0.0, 1.0) as f32
}

/// The pressure or slide value at height `y` in the lane.
pub(super) fn lane_value(lane: Rect, y: f32) -> f64 {
    f64::from(((lane.bottom() - 2.0 - y) / (lane.height() - 4.0).max(1.0)).clamp(0.0, 1.0))
}

/// Where a point of `kind`'s curve of `note` is on the screen: pitch over the grid, the others in
/// the lane.
fn point_pos(layout: &Layout, note: &Note, kind: ExpressionKind, point: ExpressionPoint) -> Option<Pos2> {
    let x = layout.x_of(note.time + point.offset);
    let y = match kind {
        ExpressionKind::Pitch => pitch_y(layout, f64::from(note.key) + point.value)?,
        _ => lane_y(layout.lane, point.value),
    };
    Some(Pos2::new(x, y))
}

/// The value of `kind` a pointer at height `y` gives `note`: a pitch on whole semitones and a
/// lane value in hundredths, unless `free`.
fn value_at(layout: &Layout, note: &Note, kind: ExpressionKind, y: f32, free: bool) -> Option<f64> {
    let value = match kind {
        ExpressionKind::Pitch => {
            let value = pitch_at(layout, y)? - f64::from(note.key);
            if free { value } else { value.round() }
        }
        _ => {
            let value = lane_value(layout.lane, y);
            if free { value } else { (value * 100.0).round() / 100.0 }
        }
    };
    kind.clamp(value)
}

/// Where each point of a note's `kind` curve is on the screen, by its index.
fn handles(layout: &Layout, note: &Note, kind: ExpressionKind, curve: &[ExpressionPoint]) -> Vec<(usize, Pos2)> {
    curve.iter().enumerate().filter_map(|(i, p)| point_pos(layout, note, kind, *p).map(|at| (i, at))).collect()
}

/// The curve of `kind` a note shows: the one being dragged, or its own.
fn shown_curve<'a>(
    state: &'a PianoRollState,
    index: usize,
    note: &'a Note,
    kind: ExpressionKind,
) -> &'a [ExpressionPoint] {
    match &state.expression_drag {
        Some(drag) if drag.note == index && drag.kind == kind => &drag.curve,
        _ => note.expression().map_or(&[], |e| e.curve(kind)),
    }
}

/// The kind the lane shows while MPE is on: pressure or slide, or `None` for velocity.
pub(super) fn lane_kind(state: &PianoRollState) -> Option<ExpressionKind> {
    state.expression.filter(|kind| *kind != ExpressionKind::Pitch)
}

/// The lane's name while MPE is on.
fn lane_name(kind: Option<ExpressionKind>) -> &'static str {
    match kind {
        Some(ExpressionKind::Pressure) => "Pressure",
        Some(ExpressionKind::Timbre) => "Slide",
        _ => "Velocity",
    }
}

/// The place of the lane's name at its left, under the column's "127".
fn lane_name_rect(layout: &Layout) -> Option<Rect> {
    let lane_keys = Rect::from_min_max(
        Pos2::new(layout.strip.left(), layout.lane.top()),
        Pos2::new(layout.keys.right(), layout.lane.bottom()),
    );
    (lane_keys.height() >= 36.0)
        .then(|| Rect::from_min_size(lane_keys.min + Vec2::new(2.0, 12.0), Vec2::new(lane_keys.width() - 4.0, 11.0)))
}

/// Draws every note's pitch curve over the grid, the selected notes' bright with their handles
/// and the name of the key each bend ends on.
pub(super) fn draw(ui: &Ui, layout: &Layout, clip: &PianoClip, state: &PianoRollState) {
    let painter = ui.painter_at(layout.grid);
    let note_look = crate::kit::element::PIANO_NOTE.attribute("kind", "outline").look();
    let ink = note_look.color();
    let accent = crate::kit::element::PIANO_NOTE.attr(crate::kit::element::Attr::Selected, true).look().accent();
    let ink = if ink == Color32::TRANSPARENT { ui.visuals().strong_text_color() } else { ink };
    let accent = if accent == Color32::TRANSPARENT { ink } else { accent };
    for (index, note) in clip.notes.iter().enumerate() {
        let curve = shown_curve(state, index, note, ExpressionKind::Pitch);
        if curve.is_empty() {
            continue;
        }
        let selected = state.selected.contains(&index);
        let line = pitch_line(layout, note, curve);
        let color = if selected { accent } else { ink.gamma_multiply(0.45) };
        painter.add(Shape::line(line, Stroke::new(if selected { 1.5 } else { 1.0 }, color)));
        if let Some(last) = curve.last().filter(|last| last.value.abs() >= 0.5)
            && let Some(end) = pitch_y(layout, f64::from(note.key) + last.value)
        {
            let key = (f64::from(note.key) + last.value).round().clamp(0.0, 127.0) as u8;
            let x = layout.x_of(note.time + last.offset) + 3.0;
            let font = crate::kit::theme::ui_font(crate::kit::theme::TEXT_SMALL);
            painter.text(Pos2::new(x, end - 2.0), Align2::LEFT_BOTTOM, key_name(key), font, color);
        }
        if selected {
            for (_, at) in handles(layout, note, ExpressionKind::Pitch, curve) {
                painter.circle_filled(at, 3.0, color);
            }
        }
    }
    draw_lane(ui, layout, clip, state, (ink, accent));
}

/// The lane while it shows pressure or slide: the velocity stems veiled, every note's curve over
/// its time, the selected notes' bright with handles; and the lane's name at its left.
fn draw_lane(ui: &Ui, layout: &Layout, clip: &PianoClip, state: &PianoRollState, (ink, accent): (Color32, Color32)) {
    let kind = lane_kind(state);
    if let Some(name) = lane_name_rect(layout) {
        let hint = crate::kit::element::LABEL.attribute("kind", "hint").look().color();
        let font = crate::kit::theme::mono_font(crate::kit::theme::TEXT_TINY);
        let painter = ui.painter_at(name);
        painter.rect_filled(name, 0.0, crate::detail::detail_fill("panel"));
        painter.text(name.left_center(), Align2::LEFT_CENTER, lane_name(kind), font, hint);
    }
    let Some(kind) = kind else { return };
    if layout.lane.height() < 8.0 {
        return;
    }
    let painter = ui.painter_at(layout.lane);
    painter.rect_filled(layout.lane, 0.0, crate::detail::detail_fill("panel").gamma_multiply(0.85));
    let middle = lane_y(layout.lane, 0.5);
    painter.hline(layout.lane.x_range(), middle.round() + 0.5, Stroke::new(1.0, ink.gamma_multiply(0.15)));
    for (index, note) in clip.notes.iter().enumerate() {
        let curve = shown_curve(state, index, note, kind);
        let selected = state.selected.contains(&index);
        let color = if selected { accent } else { ink.gamma_multiply(0.45) };
        let (start, end) = (layout.x_of(note.time), layout.x_of(note.time + note.duration));
        // A note without a curve holds the neutral value: a faint line where it plays.
        if curve.is_empty() {
            let y = lane_y(layout.lane, kind.neutral());
            painter.line_segment([Pos2::new(start, y), Pos2::new(end, y)], Stroke::new(1.0, color.gamma_multiply(0.5)));
            continue;
        }
        let mut line = Vec::with_capacity(curve.len() + 2);
        if let Some(first) = curve.first() {
            line.push(Pos2::new(start, lane_y(layout.lane, first.value)));
        }
        line.extend(curve.iter().filter_map(|p| point_pos(layout, note, kind, *p)));
        if let Some(last) = curve.last() {
            line.push(Pos2::new(end, lane_y(layout.lane, last.value)));
        }
        painter.add(Shape::line(line, Stroke::new(if selected { 1.5 } else { 1.0 }, color)));
        if selected {
            for (_, at) in handles(layout, note, kind, curve) {
                painter.circle_filled(at, 3.0, color);
            }
        }
    }
}

/// The lane's name at its left while MPE is on: a click switches what the lane shows, Velocity,
/// Pressure, Slide.
pub(super) fn lane_switch(ui: &Ui, layout: &Layout, state: &mut PianoRollState) {
    let Some(name) = lane_name_rect(layout).filter(|_| state.expression.is_some()) else { return };
    let current = lane_kind(state);
    let response = crate::kit::widgets::button_area(ui, name, Id::unique("piano-mpe-lane"), lane_name(current));
    if response.clicked() {
        state.expression = Some(match current {
            None => ExpressionKind::Pressure,
            Some(ExpressionKind::Pressure) => ExpressionKind::Timbre,
            _ => ExpressionKind::Pitch,
        });
        state.expression_drag = None;
    }
    response.on_hover_text("What the lane shows: Velocity, Pressure or Slide. Click to switch.");
}

/// The handles of the selected notes' points, made widgets after the grid's, so a press on one
/// is the handle's: dragging moves the point, a double-click removes it. Returns whether a handle
/// took the pointer this frame.
pub(super) fn handle_points(
    ui: &Ui,
    layout: &Layout,
    clip: &PianoClip,
    grid: TimelineGrid,
    free: bool,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
) -> bool {
    let mut busy = false;
    let selected: Vec<usize> = state.selected.iter().copied().collect();
    let kinds: Vec<(ExpressionKind, Rect)> = std::iter::once((ExpressionKind::Pitch, layout.grid))
        .chain(lane_kind(state).map(|kind| (kind, layout.lane)))
        .collect();
    for (kind, area) in kinds {
        for &index in &selected {
            if handle_note(ui, (layout, clip, grid, free), (index, kind, area), state, actions, &mut busy) {
                return true;
            }
        }
    }
    busy || state.expression_drag.is_some()
}

/// One selected note's handles of `kind` in `area`; true when an edit was sent.
fn handle_note(
    ui: &Ui,
    (layout, clip, grid, free): (&Layout, &PianoClip, TimelineGrid, bool),
    (index, kind, area): (usize, ExpressionKind, Rect),
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
    busy: &mut bool,
) -> bool {
    {
        let Some(note) = clip.notes.get(index) else { return false };
        let curve = shown_curve(state, index, note, kind).to_vec();
        for (point, at) in handles(layout, note, kind, &curve) {
            if !area.contains(at) {
                continue;
            }
            let rect = Rect::from_center_size(at, Vec2::splat(HANDLE * 2.0));
            let response =
                ui.interact(rect, Id::unique(("piano-mpe-point", kind.name(), index, point)), Sense::click_and_drag());
            if response.hovered() || response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                *busy = true;
            }
            if response.double_clicked() {
                let mut edited = curve.clone();
                edited.remove(point);
                // Points left only at the neutral value change nothing: the curve goes.
                if edited.iter().all(|p| p.value == kind.neutral()) {
                    edited.clear();
                }
                actions.push(set_action(clip, index, kind, &edited));
                state.expression_drag = None;
                return true;
            }
            if response.drag_started() {
                state.expression_drag = Some(PointDrag { note: index, kind, index: point, curve: curve.clone() });
            }
            if response.dragged()
                && let Some(pos) = ui.ctx().pointer_latest_pos()
                && let Some(drag) =
                    state.expression_drag.as_mut().filter(|d| d.note == index && d.index == point && d.kind == kind)
            {
                drag_point(layout, note, grid, free, drag, pos);
            }
            if response.drag_stopped()
                && let Some(drag) = state.expression_drag.take().filter(|d| d.note == index && d.kind == kind)
            {
                actions.push(set_action(clip, index, drag.kind, &drag.curve));
                return true;
            }
        }
    }
    false
}

/// Moves a dragged point to `pos`: its beat on the grid unless `free`, kept between its
/// neighbours and inside the note; its pitch on whole semitones unless `free`.
fn drag_point(layout: &Layout, note: &Note, grid: TimelineGrid, free: bool, drag: &mut PointDrag, pos: Pos2) {
    let beat = layout.beat_at(pos.x);
    let beat = if free { beat } else { nearest_line(grid, beat) };
    let low = drag.index.checked_sub(1).and_then(|i| drag.curve.get(i)).map_or(0.0, |p| p.offset + GAP);
    let high = drag.curve.get(drag.index + 1).map_or(note.duration, |p| p.offset - GAP);
    let offset = (beat - note.time).clamp(low.min(high), high.max(low)).clamp(0.0, note.duration.max(0.0));
    let Some(value) = value_at(layout, note, drag.kind, pos.y, free) else { return };
    if let Some(point) = drag.curve.get_mut(drag.index) {
        *point = ExpressionPoint::new(offset, value);
    }
}

/// The selected note a double-click at `pos` on the empty grid adds a point to: one whose time
/// takes in the pointer's beat, the one whose row is nearest.
pub(super) fn note_for_point(layout: &Layout, clip: &PianoClip, state: &PianoRollState, pos: Pos2) -> Option<usize> {
    let beat = layout.beat_at(pos.x);
    state
        .selected
        .iter()
        .filter_map(|&i| clip.notes.get(i).map(|n| (i, n)))
        .filter(|(_, n)| beat >= n.time && beat <= n.time + n.duration)
        .min_by(|(_, a), (_, b)| {
            let distance = |n: &Note| pitch_y(layout, f64::from(n.key)).map_or(f32::INFINITY, |y| (y - pos.y).abs());
            distance(a).total_cmp(&distance(b))
        })
        .map(|(i, _)| i)
}

/// Adds a point to note `index`'s `kind` curve where `pos` is: a curve's first point after the
/// note's start also gets one at the start at the neutral value (the note's own pitch), so the
/// change starts from the note.
pub(super) fn add_point(
    layout: &Layout,
    clip: &PianoClip,
    grid: TimelineGrid,
    free: bool,
    (index, kind): (usize, ExpressionKind),
    pos: Pos2,
) -> Option<Action> {
    let note = clip.notes.get(index)?;
    let beat = layout.beat_at(pos.x);
    let beat = if free { beat } else { nearest_line(grid, beat) };
    let offset = (beat - note.time).clamp(0.0, note.duration.max(0.0));
    let value = value_at(layout, note, kind, pos.y, free)?;
    let mut expression = note.expression().cloned().unwrap_or_default();
    if expression.curve(kind).is_empty() && offset > 0.0 {
        expression.set_point(kind, note.duration, 0.0, kind.neutral()).ok()?;
    }
    expression.set_point(kind, note.duration, offset, value).ok()?;
    Some(set_action(clip, index, kind, expression.curve(kind)))
}

/// The selected note a double-click at `pos` in the lane adds a point to: one whose time takes
/// in the pointer's beat, the latest to start.
pub(super) fn lane_note(layout: &Layout, clip: &PianoClip, state: &PianoRollState, pos: Pos2) -> Option<usize> {
    let beat = layout.beat_at(pos.x);
    state
        .selected
        .iter()
        .filter_map(|&i| clip.notes.get(i).map(|n| (i, n)))
        .filter(|(_, n)| beat >= n.time && beat <= n.time + n.duration)
        .max_by(|(_, a), (_, b)| a.time.total_cmp(&b.time))
        .map(|(i, _)| i)
}

/// `note.setExpression` for note `index` of the clip: `kind`'s whole curve becomes `curve`.
pub(super) fn set_action(clip: &PianoClip, index: usize, kind: ExpressionKind, curve: &[ExpressionPoint]) -> Action {
    let mut params = clip.address.clone();
    if let Some(map) = params.as_object_mut() {
        map.insert("note".into(), json!(index));
        map.insert("kind".into(), json!(kind.name()));
        map.insert("points".into(), Value::Array(curve.iter().map(|p| json!([p.offset, p.value])).collect()));
    }
    Action::new("note.setExpression", params)
}

#[cfg(test)]
#[path = "expression_tests.rs"]
mod tests;
