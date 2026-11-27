// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! MPE in the piano roll (#930, `docs/spec/ui-clip-editor.md` › MPE): each note's own pitch curve
//! over the grid, and the Slide, Pressure, Velocity and Release lanes below the notes.
//!
//! The selected notes' curves are edited where they are:
//! - a click on a line adds a point on it; a click on a point removes it; Shift and a click on a
//!   point selects it with others, and dragging a selected point moves them all;
//! - a point drags freely (Alt: on the grid and whole semitones), Shift keeps the drag to one
//!   axis and makes vertical moves fine, and a point dragged past a neighbour removes it;
//! - Shift and a drag on a line moves the segment; Alt and a drag curves it, Alt and a
//!   double-click straightens it;
//! - a double-click on the empty grid or lane in a selected note's time adds a point;
//! - with the Draw tool, a drag over a selected note's time draws the curve freehand;
//! - Ctrl and a vertical drag in a lane scales the selected notes' curves there;
//! - a right-click on a point edits its value from the keyboard or deletes it.
//!
//! Each edit is one command and one undo step (`note.setExpression`, `note.scaleExpression`,
//! `note.setReleaseVelocity`).

use std::borrow::Cow;
use std::collections::BTreeSet;

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use serde_json::{Value, json};
use suhina_engine::model::expression::{curve_value, shaped};
use suhina_engine::model::{ExpressionKind, ExpressionPoint, MAX_CURVE_POINTS, Note, NoteId};

use super::{Layout, PianoClip, PianoRollState, Tool, nearest_line};
use crate::interaction::key_name::key_name;
use crate::interaction::timeline_grid::TimelineGrid;
use crate::kit::widgets::Action;

/// How far from a point or a line, in points, a press takes it.
const HANDLE: f32 = 5.0;
/// The smallest gap kept between two points' offsets, in beats.
const GAP: f64 = 1.0e-4;
/// The drag, in points, that curves a segment all the way.
const CURVE_REACH: f32 = 80.0;
/// The fewest beats between two points a freehand stroke makes.
const DRAW_STEP: f64 = 1.0 / 64.0;
/// The height of the band at the top of the lanes that holds their toggles.
const HEADER: f32 = 12.0;

/// One of the lanes below the notes while MPE is on, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lane {
    Slide,
    Pressure,
    Velocity,
    Release,
}

impl Lane {
    pub(crate) const ALL: [Lane; 4] = [Lane::Slide, Lane::Pressure, Lane::Velocity, Lane::Release];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Lane::Slide => "Slide",
            Lane::Pressure => "Pressure",
            Lane::Velocity => "Velocity",
            Lane::Release => "Release",
        }
    }

    /// The curve the lane shows, for Slide and Pressure.
    pub(crate) fn kind(self) -> Option<ExpressionKind> {
        match self {
            Lane::Slide => Some(ExpressionKind::Timbre),
            Lane::Pressure => Some(ExpressionKind::Pressure),
            Lane::Velocity | Lane::Release => None,
        }
    }
}

/// How a point drag moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DragMode {
    /// One point, or the selected points together.
    Points { press: Pos2 },
    /// The segment after point `index`, up or down.
    Segment { press: Pos2 },
    /// The segment after point `index` curved, from the curvature it had.
    Curve { start_y: f32, start: f64 },
    /// A freehand stroke, the last offset it drew at.
    Draw { last: f64 },
}

/// A curve being edited by a drag: the note (by its id, so the drag stays on it when another
/// writer's edit moves it to another index), the curve as it is while dragged, the point or
/// segment it took, and the curve as it was when the drag began.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PointDrag {
    pub(crate) note: NoteId,
    pub(crate) kind: ExpressionKind,
    pub(crate) index: usize,
    pub(crate) curve: Vec<ExpressionPoint>,
    pub(crate) from: Vec<ExpressionPoint>,
    pub(crate) mode: DragMode,
}

impl PointDrag {
    /// Whether this drag is on note `id`'s `kind` curve.
    fn of(&self, id: Option<NoteId>, kind: ExpressionKind) -> bool {
        id == Some(self.note) && self.kind == kind
    }
}

/// The selected points of one note's curve.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct PointSelection {
    pub(crate) note: Option<NoteId>,
    pub(crate) kind: Option<ExpressionKind>,
    pub(crate) points: BTreeSet<usize>,
}

/// A right-click on a point: its menu, and the value being typed after Edit value.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PointMenu {
    pub(crate) note: NoteId,
    pub(crate) kind: ExpressionKind,
    pub(crate) point: usize,
    pub(crate) pos: Pos2,
    /// `Some` while the value is typed.
    pub(crate) text: Option<String>,
}

/// What the MPE editor keeps between frames besides the drag.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MpeState {
    /// Which lanes show, in [`Lane::ALL`]'s order.
    pub(crate) lanes: [bool; 4],
    /// The lanes as this frame lays them out.
    pub(crate) rects: Vec<(Lane, Rect)>,
    /// The band above the lanes that holds their toggles.
    pub(crate) header: Option<Rect>,
    pub(crate) selection: PointSelection,
    pub(crate) menu: Option<PointMenu>,
    /// A release-velocity drag: the notes and the value they take.
    pub(crate) release: Option<(Vec<usize>, f64)>,
    /// A Ctrl drag scaling a lane: its kind, where it started and the factor.
    pub(crate) scale: Option<(ExpressionKind, f32, f64)>,
    /// The key each bend ends on is named beside it.
    pub(crate) bend_targets: bool,
    /// The curve last edited, which Ctrl+A selects the points of.
    pub(crate) last_kind: ExpressionKind,
}

impl Default for MpeState {
    fn default() -> Self {
        Self {
            lanes: [true, true, true, false],
            rects: Vec::new(),
            header: None,
            selection: PointSelection::default(),
            menu: None,
            release: None,
            scale: None,
            bend_targets: true,
            last_kind: ExpressionKind::Pitch,
        }
    }
}

/// Splits `room`, the space below the notes, into the lanes shown while MPE is on, and returns
/// the velocity lane's part (empty at the room's foot when it is hidden). With MPE off the room
/// is the velocity lane's.
pub(super) fn split_lanes(state: &mut PianoRollState, room: Rect) -> Rect {
    state.mpe.rects.clear();
    state.mpe.header = None;
    if state.expression.is_none() {
        return room;
    }
    let header = Rect::from_min_max(room.min, Pos2::new(room.right(), (room.top() + HEADER).min(room.bottom())));
    state.mpe.header = Some(header);
    let shown: Vec<Lane> = Lane::ALL.iter().zip(state.mpe.lanes).filter(|(_, on)| *on).map(|(l, _)| *l).collect();
    let body = Rect::from_min_max(Pos2::new(room.left(), header.bottom()), room.max);
    let each = body.height() / shown.len().max(1) as f32;
    let mut velocity = Rect::from_min_max(room.left_bottom(), room.right_bottom());
    for (i, lane) in shown.iter().enumerate() {
        let top = body.top() + each * i as f32;
        let rect = Rect::from_min_max(Pos2::new(body.left(), top), Pos2::new(body.right(), top + each));
        if *lane == Lane::Velocity {
            velocity = rect;
        }
        state.mpe.rects.push((*lane, rect));
    }
    velocity
}

/// Where `kind`'s curve is drawn: pitch over the grid, slide and pressure in their lanes.
fn area_of(state: &PianoRollState, layout: &Layout, kind: ExpressionKind) -> Option<Rect> {
    match kind {
        ExpressionKind::Pitch => Some(layout.grid),
        _ => state.mpe.rects.iter().find(|(lane, _)| lane.kind() == Some(kind)).map(|(_, rect)| *rect),
    }
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

/// The height in a lane of a value from 0 to 1, 0 at its foot and 1 at its top.
pub(super) fn lane_y(lane: Rect, value: f64) -> f32 {
    lane.bottom() - 2.0 - (lane.height() - 4.0).max(0.0) * value.clamp(0.0, 1.0) as f32
}

/// The value, 0 to 1, at height `y` in a lane.
pub(super) fn lane_value(lane: Rect, y: f32) -> f64 {
    f64::from(((lane.bottom() - 2.0 - y) / (lane.height() - 4.0).max(1.0)).clamp(0.0, 1.0))
}

/// The screen y of `value` of `kind` for `note` in `area`.
fn y_of(layout: &Layout, note: &Note, kind: ExpressionKind, area: Rect, value: f64) -> Option<f32> {
    match kind {
        ExpressionKind::Pitch => pitch_y(layout, f64::from(note.key) + value),
        _ => Some(lane_y(area, value)),
    }
}

/// The value of `kind` a pointer at height `y` gives `note`: on whole semitones (pitch) or
/// whole MIDI steps (the lanes) when `snap`, exactly where the pointer is otherwise.
fn value_at(layout: &Layout, note: &Note, kind: ExpressionKind, area: Rect, y: f32, snap: bool) -> Option<f64> {
    let value = match kind {
        ExpressionKind::Pitch => {
            let value = pitch_at(layout, y)? - f64::from(note.key);
            if snap { value.round() } else { value }
        }
        _ => {
            let value = lane_value(area, y);
            if snap { (value * 127.0).round() / 127.0 } else { value }
        }
    };
    kind.clamp(value)
}

/// The beat at `x`, on the nearest grid line when `snap`.
fn beat_at(layout: &Layout, grid: TimelineGrid, x: f32, snap: bool) -> f64 {
    let beat = layout.beat_at(x);
    if snap { nearest_line(grid, beat) } else { beat }
}

/// A value as it is shown beside its point: a pitch in semitones, a lane value from 0 to 127.
pub(super) fn value_text(kind: ExpressionKind, value: f64) -> String {
    match kind {
        ExpressionKind::Pitch => format!("{value:+.2} st"),
        _ => format!("{:.0}", value * 127.0),
    }
}

/// Every point on the screen of a curve, by index.
fn handles(
    layout: &Layout,
    note: &Note,
    kind: ExpressionKind,
    area: Rect,
    curve: &[ExpressionPoint],
) -> Vec<(usize, Pos2)> {
    curve
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            y_of(layout, note, kind, area, p.value).map(|y| (i, Pos2::new(layout.x_of(note.time + p.offset), y)))
        })
        .collect()
}

/// The line a curve is drawn with: from the note's start at its first value, along its
/// segments (a curved one in steps), held to the note's end.
pub(super) fn curve_line(
    layout: &Layout,
    note: &Note,
    kind: ExpressionKind,
    area: Rect,
    curve: &[ExpressionPoint],
) -> Vec<Pos2> {
    let Some(first) = curve.first() else { return Vec::new() };
    let at = |offset: f64, value: f64| {
        y_of(layout, note, kind, area, value)
            .map(|y| Pos2::new(layout.x_of(note.time + offset.clamp(0.0, note.duration)), y))
    };
    let mut line = Vec::with_capacity(curve.len() + 2);
    line.extend(at(0.0, first.value));
    for (i, p) in curve.iter().enumerate() {
        line.extend(at(p.offset, p.value));
        if let Some(next) = curve.get(i + 1).filter(|_| p.curve != 0.0 && next_rise(p, curve.get(i + 1)) != 0.0) {
            let rise = next.value - p.value;
            for step in 1..16 {
                let t = f64::from(step) / 16.0;
                line.extend(at(p.offset + (next.offset - p.offset) * t, p.value + rise * shaped(t, p.curve, rise)));
            }
        }
    }
    if let Some(last) = curve.last() {
        line.extend(at(note.duration, last.value));
    }
    line
}

fn next_rise(p: &ExpressionPoint, next: Option<&ExpressionPoint>) -> f64 {
    next.map_or(0.0, |n| n.value - p.value)
}

/// The pitch line of a note's curve, for the tests and the hover.
#[cfg(test)]
pub(super) fn pitch_line(layout: &Layout, note: &Note, curve: &[ExpressionPoint]) -> Vec<Pos2> {
    curve_line(layout, note, ExpressionKind::Pitch, layout.grid, curve)
}

/// A note's own `kind` curve, empty when it has none.
fn own_curve(note: &Note, kind: ExpressionKind) -> &[ExpressionPoint] {
    note.expression().map_or(&[], |e| e.curve(kind))
}

/// The curve of `kind` a note shows: the one being dragged, a lane being scaled, or its own.
fn shown_curve<'a>(
    state: &'a PianoRollState,
    (index, id): (usize, Option<NoteId>),
    note: &'a Note,
    kind: ExpressionKind,
) -> Cow<'a, [ExpressionPoint]> {
    match &state.expression_drag {
        Some(drag) if drag.of(id, kind) => Cow::Borrowed(&drag.curve),
        _ => match state.mpe.scale {
            Some((scaled, _, factor)) if scaled == kind && state.selected.contains(&index) => Cow::Owned(
                own_curve(note, kind)
                    .iter()
                    .map(|p| ExpressionPoint { value: (p.value * factor).clamp(0.0, 1.0), ..*p })
                    .collect(),
            ),
            _ => Cow::Borrowed(own_curve(note, kind)),
        },
    }
}

/// Lets go of a drag whose note is gone, or whose curve another writer changed since it began.
pub(super) fn settle_drag(clip: &PianoClip, state: &mut PianoRollState) {
    let Some(drag) = &state.expression_drag else { return };
    let current = clip.notes.position(drag.note).and_then(|i| clip.notes.get(i)).map(|n| own_curve(n, drag.kind));
    if current != Some(drag.from.as_slice()) {
        state.expression_drag = None;
    }
}

/// The colors the curves are drawn in: faint, near, selected, a selected point's ring, and the
/// edge under a selected line.
struct Inks {
    ink: Color32,
    bright: Color32,
    accent: Color32,
    edge: Color32,
}

fn inks(ui: &Ui) -> Inks {
    let ink = crate::kit::element::PIANO_NOTE.attribute("kind", "outline").look().color();
    let accent = crate::kit::element::PIANO_NOTE.attr(crate::kit::element::Attr::Selected, true).look().accent();
    let bright = ui.visuals().strong_text_color();
    let ink = if ink == Color32::TRANSPARENT { bright } else { ink };
    let accent = if accent == Color32::TRANSPARENT { ink } else { accent };
    // A selected note is drawn in the selection's color, so its curve takes the text's strong
    // color over an edge of the grid's own, which stands out on the note and on the grid alike.
    let edge = crate::kit::element::PIANO_GRID.look().background();
    let edge = if edge == Color32::TRANSPARENT { ui.visuals().extreme_bg_color } else { edge };
    Inks { ink, bright, accent, edge }
}

/// Draws the curves: pitch over the grid, the lanes below the notes with their toggles, the
/// selected notes' handles, the hovered or dragged point's value, and the point menu.
pub(super) fn draw(ui: &Ui, layout: &Layout, clip: &PianoClip, state: &PianoRollState) {
    let inks = inks(ui);
    let hover = ui.ctx().pointer_hover_pos();
    draw_curves(ui, layout, clip, state, ExpressionKind::Pitch, layout.grid, &inks, hover);
    for (lane, rect) in &state.mpe.rects {
        draw_lane_frame(ui, layout, *lane, *rect, &inks);
        match lane.kind() {
            Some(kind) => draw_curves(ui, layout, clip, state, kind, *rect, &inks, hover),
            None if *lane == Lane::Release => draw_release(ui, layout, clip, state, *rect, &inks),
            None => {}
        }
    }
    draw_header(ui, layout, state, &inks);
}

#[allow(clippy::too_many_arguments)]
fn draw_curves(
    ui: &Ui,
    layout: &Layout,
    clip: &PianoClip,
    state: &PianoRollState,
    kind: ExpressionKind,
    area: Rect,
    inks: &Inks,
    hover: Option<Pos2>,
) {
    if area.height() < 4.0 {
        return;
    }
    let painter = ui.painter_at(area);
    let font = crate::kit::theme::ui_font(crate::kit::theme::TEXT_SMALL);
    for (index, note) in clip.notes.iter().enumerate() {
        let id = clip.notes.id(index);
        let curve = shown_curve(state, (index, id), note, kind);
        if curve.is_empty() {
            continue;
        }
        let selected = state.selected.contains(&index);
        let line = curve_line(layout, note, kind, area, &curve);
        let dragged = state.expression_drag.as_ref().is_some_and(|d| d.of(id, kind));
        let near = dragged || hover.is_some_and(|p| near_line(layout, note, kind, &line, p));
        let color = if selected {
            inks.bright
        } else if near {
            inks.ink.gamma_multiply(0.8)
        } else {
            inks.ink.gamma_multiply(0.45)
        };
        if selected {
            painter.add(Shape::line(line.clone(), Stroke::new(4.0, inks.edge.gamma_multiply(0.8))));
        }
        painter.add(Shape::line(line, Stroke::new(if selected { 1.75 } else { 1.0 }, color)));
        if kind == ExpressionKind::Pitch
            && state.mpe.bend_targets
            && let Some(last) = curve.last().filter(|last| last.value.abs() >= 0.5)
            && let Some(end) = pitch_y(layout, f64::from(note.key) + last.value)
        {
            let key = (f64::from(note.key) + last.value).round().clamp(0.0, 127.0) as u8;
            let x = layout.x_of(note.time + last.offset) + 3.0;
            painter.text(Pos2::new(x, end - 2.0), Align2::LEFT_BOTTOM, key_name(key), font.clone(), color);
        }
        if !selected {
            continue;
        }
        let chosen = |point: usize| {
            state.mpe.selection.note == id
                && state.mpe.selection.kind == Some(kind)
                && state.mpe.selection.points.contains(&point)
        };
        for (point, at) in handles(layout, note, kind, area, &curve) {
            painter.circle_filled(at, 4.5, inks.edge);
            painter.circle_stroke(at, 4.0, Stroke::new(1.5, inks.accent));
            painter.circle_filled(at, 3.0, if chosen(point) { inks.accent } else { color });
            let active = dragged && state.expression_drag.as_ref().is_some_and(|d| d.index == point);
            if active || hover.is_some_and(|p| p.distance(at) <= HANDLE) {
                if let Some(value) = curve.get(point).map(|p| p.value) {
                    label(&painter, at, &value_text(kind, value), inks);
                }
            }
        }
    }
}

/// A value beside a point, over a patch of the edge color.
fn label(painter: &egui::Painter, at: Pos2, text: &str, inks: &Inks) {
    let font = crate::kit::theme::mono_font(crate::kit::theme::TEXT_TINY);
    let galley = painter.layout_no_wrap(text.to_owned(), font, inks.bright);
    let rect = Rect::from_min_size(at + Vec2::new(8.0, -galley.size().y - 6.0), galley.size()).expand(2.0);
    painter.rect_filled(rect, 2.0, inks.edge);
    painter.galley(rect.min + Vec2::splat(2.0), galley, inks.bright);
}

/// A lane's background, its middle line and its name in the column at its left.
fn draw_lane_frame(ui: &Ui, layout: &Layout, lane: Lane, rect: Rect, inks: &Inks) {
    if lane == Lane::Velocity || rect.height() < 4.0 {
        return;
    }
    let painter = ui.painter_at(rect.union(column_of(layout, rect)));
    painter.rect_filled(rect, 0.0, crate::detail::detail_fill("panel"));
    painter.hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, inks.ink.gamma_multiply(0.25)));
    painter.hline(rect.x_range(), lane_y(rect, 0.5).round() + 0.5, Stroke::new(1.0, inks.ink.gamma_multiply(0.12)));
    let column = column_of(layout, rect);
    painter.rect_filled(column, 0.0, crate::detail::detail_fill("panel"));
    if rect.height() >= 12.0 {
        let hint = crate::kit::element::LABEL.attribute("kind", "hint").look().color();
        let font = crate::kit::theme::mono_font(crate::kit::theme::TEXT_TINY);
        painter.text(column.left_top() + Vec2::new(3.0, 1.0), Align2::LEFT_TOP, lane.name(), font, hint);
    }
}

/// The column at the left of a lane, under the keys.
fn column_of(layout: &Layout, rect: Rect) -> Rect {
    Rect::from_min_max(Pos2::new(layout.strip.left(), rect.top()), Pos2::new(layout.keys.right(), rect.bottom()))
}

/// The release lane: a stem at each note's start as high as its release velocity.
fn draw_release(ui: &Ui, layout: &Layout, clip: &PianoClip, state: &PianoRollState, rect: Rect, inks: &Inks) {
    if rect.height() < 4.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    for (index, note) in clip.notes.iter().enumerate() {
        let selected = state.selected.contains(&index);
        let value = match &state.mpe.release {
            Some((notes, value)) if notes.contains(&index) => *value,
            _ => f64::from(note.release_velocity),
        };
        let x = layout.x_of(note.time).round() + 0.5;
        let top = lane_y(rect, value);
        let color = if selected { inks.bright } else { inks.ink.gamma_multiply(0.6) };
        painter.line_segment([Pos2::new(x, rect.bottom()), Pos2::new(x, top)], Stroke::new(2.0, color));
        painter.circle_filled(Pos2::new(x, top), 2.5, color);
    }
}

/// The toggles of the lanes, in the column at the top of the room below the notes.
fn draw_header(ui: &Ui, layout: &Layout, state: &PianoRollState, inks: &Inks) {
    let Some(header) = state.mpe.header else { return };
    let painter = ui.painter_at(header.union(column_of(layout, header)));
    painter.rect_filled(header.union(column_of(layout, header)), 0.0, crate::detail::detail_fill("panel"));
    let font = crate::kit::theme::mono_font(crate::kit::theme::TEXT_TINY);
    for (rect, (lane, on)) in toggle_rects(layout, header).into_iter().zip(Lane::ALL.iter().zip(state.mpe.lanes)) {
        if on {
            painter.rect_filled(rect, 2.0, inks.accent.gamma_multiply(0.6));
        }
        painter.rect_stroke(rect, 2.0, Stroke::new(1.0, inks.ink.gamma_multiply(0.5)), egui::StrokeKind::Inside);
        let letter = &lane.name()[..1];
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            letter,
            font.clone(),
            if on { inks.bright } else { inks.ink },
        );
    }
}

/// The four toggles' places, side by side in the column's part of the header.
fn toggle_rects(layout: &Layout, header: Rect) -> Vec<Rect> {
    let column = column_of(layout, header);
    let size = (column.width() / 4.0 - 2.0).clamp(6.0, header.height().max(6.0));
    (0..4)
        .map(|i| {
            Rect::from_min_size(
                Pos2::new(column.left() + 2.0 + i as f32 * (size + 2.0), header.top() + 1.0),
                Vec2::new(size, (header.height() - 2.0).max(4.0)),
            )
        })
        .collect()
}

/// Whether `p` is over `note` or near its line.
fn near_line(layout: &Layout, note: &Note, kind: ExpressionKind, line: &[Pos2], p: Pos2) -> bool {
    (kind == ExpressionKind::Pitch && layout.note_rect(note).expand(HANDLE).contains(p))
        || line.windows(2).any(|w| distance_to_segment(p, w[0], w[1]) <= HANDLE * 1.5)
}

/// How far `p` is from the segment `a`–`b`.
fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let length = ab.length_sq();
    let t = if length > 0.0 { ((p - a).dot(ab) / length).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}

/// Everything the widgets of one frame need.
struct Cx<'a> {
    ui: &'a Ui,
    layout: &'a Layout,
    clip: &'a PianoClip,
    grid: TimelineGrid,
    snap: bool,
    shift: bool,
    alt: bool,
    command: bool,
}

/// The MPE editor's widgets, made after the grid's and the lanes' so a press on a point, a line
/// or a lane's own gesture is theirs: the lane toggles, the lanes' backgrounds, the selected
/// notes' points and lines, the point menu. Returns whether one of them took the pointer.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_points(
    ui: &Ui,
    layout: &Layout,
    clip: &PianoClip,
    grid: TimelineGrid,
    snap: bool,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
) -> bool {
    settle_drag(clip, state);
    let (shift, alt, command) =
        ui.input(|i| (i.modifiers.shift, i.modifiers.alt, i.modifiers.command || i.modifiers.ctrl));
    let cx = Cx { ui, layout, clip, grid, snap, shift, alt, command };
    let mut busy = header_toggles(&cx, state);
    busy |= lanes(&cx, state, actions);
    busy |= draw_strokes(&cx, state, actions);
    let selected: Vec<usize> = state.selected.iter().copied().collect();
    let kinds: Vec<ExpressionKind> = std::iter::once(ExpressionKind::Pitch)
        .chain(state.mpe.rects.iter().filter_map(|(lane, _)| lane.kind()))
        .collect();
    for kind in kinds {
        let Some(area) = area_of(state, layout, kind) else { continue };
        for &index in &selected {
            if handle_note(&cx, (index, kind, area), state, actions, &mut busy) {
                return true;
            }
        }
    }
    busy |= point_menu(&cx, state, actions);
    // A drag whose widget no frame made (its note left the selection, a lane was hidden) never
    // hears the button let go: it ends with the button.
    if state.expression_drag.is_some() && !ui.input(|i| i.pointer.primary_down()) {
        state.expression_drag = None;
    }
    busy || state.expression_drag.is_some() || state.mpe.menu.is_some()
}

/// The lane toggles in the header.
fn header_toggles(cx: &Cx<'_>, state: &mut PianoRollState) -> bool {
    let Some(header) = state.mpe.header else { return false };
    let mut busy = false;
    for (i, rect) in toggle_rects(cx.layout, header).into_iter().enumerate() {
        let Some(lane) = Lane::ALL.get(i).copied() else { continue };
        let on = state.mpe.lanes.get(i).copied().unwrap_or(false);
        let response =
            crate::kit::widgets::button_area(cx.ui, rect, Id::unique(("piano-mpe-lane", lane.name())), lane.name());
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, lane.name()));
        if response.hovered() {
            busy = true;
        }
        if response.clicked()
            && let Some(slot) = state.mpe.lanes.get_mut(i)
        {
            *slot = !on;
            state.expression_drag = None;
        }
        response.on_hover_text(format!("{} lane: show or hide", lane.name()));
    }
    busy
}

/// The lanes' own gestures: a double-click in a selected note's time adds a point; Ctrl and a
/// vertical drag scales the selected notes' curves; in the release lane, a drag on a stem sets
/// release velocity.
fn lanes(cx: &Cx<'_>, state: &mut PianoRollState, actions: &mut Vec<Action>) -> bool {
    let rects = state.mpe.rects.clone();
    let mut busy = false;
    for (lane, rect) in rects {
        if lane == Lane::Velocity || rect.height() < 4.0 {
            continue;
        }
        let response = cx.ui.interact(rect, Id::unique(("piano-mpe-lane-area", lane.name())), Sense::click_and_drag());
        let name = format!("{} lane", lane.name());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, &name));
        let pos = response.interact_pointer_pos().or_else(|| cx.ui.ctx().pointer_latest_pos());
        if response.hovered() || response.dragged() {
            busy = true;
        }
        if lane == Lane::Release {
            busy |= release_lane(cx, state, actions, &response, rect, pos);
            continue;
        }
        let Some(kind) = lane.kind() else { continue };
        if response.double_clicked()
            && let Some(pos) = pos
            && let Some(index) = lane_note(cx.layout, cx.clip, state, pos)
        {
            actions.extend(add_point(cx.layout, cx.clip, cx.grid, cx.snap, (index, kind, rect), pos));
            state.mpe.last_kind = kind;
        }
        if response.drag_started() && cx.command {
            let y = pos.map_or(rect.center().y, |p| p.y);
            state.mpe.scale = Some((kind, y, 1.0));
        }
        if response.dragged()
            && let Some(pos) = pos
            && let Some((scaled, start, factor)) = state.mpe.scale.as_mut()
            && *scaled == kind
        {
            *factor = 2f64.powf(f64::from((*start - pos.y) / (rect.height() * 0.5).max(8.0)));
        }
        if response.drag_stopped()
            && let Some((scaled, _, factor)) = state.mpe.scale.take()
            && scaled == kind
            && (factor - 1.0).abs() > 1e-3
            && !state.selected.is_empty()
        {
            let mut params = cx.clip.address.clone();
            if let Some(map) = params.as_object_mut() {
                map.insert("notes".into(), json!(state.selected.iter().collect::<Vec<_>>()));
                map.insert("kind".into(), json!(kind.name()));
                map.insert("factor".into(), json!(factor.clamp(0.0, 127.0)));
            }
            actions.push(Action::new("note.scaleExpression", params));
        }
    }
    busy
}

/// The release lane's drag: the stem nearest the press (within a few points of a note's start)
/// and, when that note is selected, every selected note take the height of the pointer.
fn release_lane(
    cx: &Cx<'_>,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
    response: &egui::Response,
    rect: Rect,
    pos: Option<Pos2>,
) -> bool {
    if response.drag_started()
        && let Some(pos) = pos
        && let Some(index) = cx
            .clip
            .notes
            .iter()
            .enumerate()
            .filter(|(_, n)| (cx.layout.x_of(n.time) - pos.x).abs() <= HANDLE)
            .min_by(|a, b| {
                let d = |n: &Note| (cx.layout.x_of(n.time) - pos.x).abs();
                d(a.1).total_cmp(&d(b.1))
            })
            .map(|(i, _)| i)
    {
        let notes =
            if state.selected.contains(&index) { state.selected.iter().copied().collect() } else { vec![index] };
        state.mpe.release = Some((notes, lane_value(rect, pos.y)));
    }
    if response.dragged()
        && let Some(pos) = pos
        && let Some((_, value)) = state.mpe.release.as_mut()
    {
        *value = ((lane_value(rect, pos.y) * 127.0).round() / 127.0).max(1.0 / 127.0);
    }
    if response.drag_stopped()
        && let Some((notes, value)) = state.mpe.release.take()
    {
        let mut params = cx.clip.address.clone();
        if let Some(map) = params.as_object_mut() {
            map.insert("notes".into(), json!(notes));
            map.insert("velocity".into(), json!((value * 127.0).round().clamp(1.0, 127.0)));
        }
        actions.push(Action::new("note.setReleaseVelocity", params));
        return true;
    }
    state.mpe.release.is_some()
}

/// With the Draw tool, a drag over a selected note's time draws its curve freehand: pitch over
/// the grid, slide or pressure in their lanes.
fn draw_strokes(cx: &Cx<'_>, state: &mut PianoRollState, actions: &mut Vec<Action>) -> bool {
    if state.tool != Tool::Draw {
        return false;
    }
    let mut busy = false;
    let kinds: Vec<ExpressionKind> = std::iter::once(ExpressionKind::Pitch)
        .chain(state.mpe.rects.iter().filter_map(|(lane, _)| lane.kind()))
        .collect();
    let selected: Vec<usize> = state.selected.iter().copied().collect();
    for kind in kinds {
        let Some(area) = area_of(state, cx.layout, kind) else { continue };
        for &index in &selected {
            let Some(note) = cx.clip.notes.get(index) else { continue };
            let id = clip_id(cx.clip, index);
            let x0 = cx.layout.x_of(note.time).max(area.left());
            let x1 = cx.layout.x_of(note.time + note.duration).min(area.right());
            if x1 <= x0 {
                continue;
            }
            let rect = Rect::from_min_max(Pos2::new(x0, area.top()), Pos2::new(x1, area.bottom()));
            let response = cx.ui.interact(rect, Id::unique(("piano-mpe-draw", kind.name(), id)), Sense::drag());
            if response.hovered() || response.dragged() {
                cx.ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                busy = true;
            }
            let pos = cx.ui.ctx().pointer_latest_pos();
            if response.drag_started()
                && let Some(id) = id
            {
                let from = own_curve(note, kind).to_vec();
                state.expression_drag = Some(PointDrag {
                    note: id,
                    kind,
                    index: 0,
                    curve: from.clone(),
                    from,
                    mode: DragMode::Draw { last: f64::NAN },
                });
            }
            if response.dragged()
                && let Some(pos) = pos
                && let Some(drag) = state.expression_drag.as_mut().filter(|d| d.of(id, kind))
                && let DragMode::Draw { last } = &mut drag.mode
            {
                let offset = (beat_at(cx.layout, cx.grid, pos.x, cx.snap) - note.time).clamp(0.0, note.duration);
                if let Some(value) = value_at(cx.layout, note, kind, area, pos.y, cx.snap) {
                    *last = draw_point(&mut drag.curve, *last, offset, value);
                }
            }
            if response.drag_stopped()
                && let Some(drag) = state.expression_drag.take_if(|d| d.of(id, kind))
            {
                if drag.curve != drag.from {
                    actions.push(set_action(cx.clip, index, kind, &drag.curve));
                }
                state.mpe.last_kind = kind;
                return true;
            }
        }
    }
    busy
}

/// Puts a freehand point at `offset`: the points the stroke passed over since `last` go, and a
/// point closer than [`DRAW_STEP`] to the last one drawn takes its place. Returns the offset drawn
/// at.
pub(super) fn draw_point(curve: &mut Vec<ExpressionPoint>, last: f64, offset: f64, value: f64) -> f64 {
    if last.is_finite() {
        let (low, high) = if last <= offset { (last, offset) } else { (offset, last) };
        curve.retain(|p| !(p.offset > low + GAP && p.offset < high - GAP));
        if (offset - last).abs() < DRAW_STEP {
            // Too near the last one: it moves here instead.
            curve.retain(|p| (p.offset - last).abs() > GAP);
        }
    }
    curve.retain(|p| (p.offset - offset).abs() > GAP);
    if curve.len() >= MAX_CURVE_POINTS {
        return last;
    }
    let at = curve.partition_point(|p| p.offset < offset);
    curve.insert(at, ExpressionPoint::new(offset, value));
    if last.is_finite() { if (offset - last).abs() < DRAW_STEP { last } else { offset } } else { offset }
}

fn clip_id(clip: &PianoClip, index: usize) -> Option<NoteId> {
    clip.notes.id(index)
}

/// One selected note's points and lines of `kind` in `area`; true when an edit was sent.
fn handle_note(
    cx: &Cx<'_>,
    (index, kind, area): (usize, ExpressionKind, Rect),
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
    busy: &mut bool,
) -> bool {
    let Some(note) = cx.clip.notes.get(index) else { return false };
    let id = clip_id(cx.clip, index);
    let curve = shown_curve(state, (index, id), note, kind).into_owned();
    let hover = cx.ui.ctx().pointer_hover_pos();
    // The lines first, so a point at a line's end is the point's.
    let at = handles(cx.layout, note, kind, area, &curve);
    for segment in 0..curve.len().saturating_sub(1) {
        let (Some((_, a)), Some((_, b))) =
            (at.iter().find(|(i, _)| *i == segment), at.iter().find(|(i, _)| *i == segment + 1))
        else {
            continue;
        };
        let active = state.expression_drag.as_ref().is_some_and(|d| {
            d.of(id, kind) && d.index == segment && matches!(d.mode, DragMode::Segment { .. } | DragMode::Curve { .. })
        });
        let near = hover.is_some_and(|p| {
            area.contains(p)
                && distance_to_segment(p, *a, *b) <= HANDLE
                && p.distance(*a) > HANDLE
                && p.distance(*b) > HANDLE
        });
        if !active && !near {
            continue;
        }
        let rect = Rect::from_two_pos(*a, *b).expand(HANDLE);
        let response =
            cx.ui.interact(rect, Id::unique(("piano-mpe-line", kind.name(), id, segment)), Sense::click_and_drag());
        cx.ui.ctx().set_cursor_icon(if cx.alt || cx.shift {
            egui::CursorIcon::ResizeVertical
        } else {
            egui::CursorIcon::PointingHand
        });
        *busy = true;
        if line_gestures(cx, (index, id, note, kind, area), segment, &curve, &response, state, actions) {
            return true;
        }
    }
    for (point, pos) in at {
        let dragged = state
            .expression_drag
            .as_ref()
            .is_some_and(|d| d.of(id, kind) && d.index == point && matches!(d.mode, DragMode::Points { .. }));
        if !area.contains(pos) && !dragged {
            continue;
        }
        let rect = Rect::from_center_size(pos, Vec2::splat(HANDLE * 2.0));
        let response =
            cx.ui.interact(rect, Id::unique(("piano-mpe-point", kind.name(), id, point)), Sense::click_and_drag());
        if response.hovered() || response.dragged() {
            cx.ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            *busy = true;
        }
        if point_gestures(cx, (index, id, note, kind, area), point, &curve, &response, state, actions) {
            return true;
        }
    }
    false
}

/// A line's click (a point on it), Shift drag (the segment moves), Alt drag (it curves) and Alt
/// double-click (it straightens). True when an edit was sent.
fn line_gestures(
    cx: &Cx<'_>,
    (index, id, note, kind, area): (usize, Option<NoteId>, &Note, ExpressionKind, Rect),
    segment: usize,
    curve: &[ExpressionPoint],
    response: &egui::Response,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
) -> bool {
    if response.double_clicked() && cx.alt {
        let mut edited = curve.to_vec();
        if let Some(p) = edited.get_mut(segment) {
            p.curve = 0.0;
        }
        actions.push(set_action(cx.clip, index, kind, &edited));
        state.mpe.last_kind = kind;
        return true;
    }
    if response.clicked()
        && !cx.alt
        && !cx.shift
        && let Some(pos) = response.interact_pointer_pos()
    {
        // A point on the line, at the value the line has there.
        let offset = (beat_at(cx.layout, cx.grid, pos.x, cx.snap) - note.time).clamp(0.0, note.duration);
        let Some(value) = curve_value(curve, offset) else { return false };
        let mut edited = curve.to_vec();
        let at = edited.partition_point(|p| p.offset < offset);
        if edited.get(at).is_some_and(|p| (p.offset - offset).abs() <= GAP) || edited.len() >= MAX_CURVE_POINTS {
            return false;
        }
        // The new point keeps the segment's shape on both sides.
        let shape = edited.get(at.wrapping_sub(1)).map_or(0.0, |p| p.curve);
        edited.insert(at, ExpressionPoint::curved(offset, value, shape));
        actions.push(set_action(cx.clip, index, kind, &edited));
        state.mpe.last_kind = kind;
        return true;
    }
    let pos = cx.ui.ctx().pointer_latest_pos();
    if response.drag_started()
        && let Some(id) = id
        && (cx.alt || cx.shift)
    {
        let press = cx.ui.input(|i| i.pointer.press_origin()).or(pos).unwrap_or_default();
        let mode = if cx.alt {
            DragMode::Curve { start_y: press.y, start: curve.get(segment).map_or(0.0, |p| p.curve) }
        } else {
            DragMode::Segment { press }
        };
        let from = own_curve(note, kind).to_vec();
        state.expression_drag = Some(PointDrag { note: id, kind, index: segment, curve: curve.to_vec(), from, mode });
    }
    if response.dragged()
        && let Some(pos) = pos
        && let Some(drag) = state.expression_drag.as_mut().filter(|d| d.of(id, kind) && d.index == segment)
    {
        let base = drag.from.clone();
        match drag.mode {
            DragMode::Curve { start_y, start } => {
                let amount = (start + f64::from((start_y - pos.y) / CURVE_REACH)).clamp(-1.0, 1.0);
                drag.curve = base;
                if let Some(p) = drag.curve.get_mut(segment) {
                    p.curve = if amount.abs() < 0.02 { 0.0 } else { amount };
                }
            }
            DragMode::Segment { press } => {
                let (Some(now), Some(then)) = (
                    value_at(cx.layout, note, kind, area, pos.y, false),
                    value_at(cx.layout, note, kind, area, press.y, false),
                ) else {
                    return false;
                };
                let by = if cx.snap && kind == ExpressionKind::Pitch { (now - then).round() } else { now - then };
                drag.curve = base;
                for i in [segment, segment + 1] {
                    if let Some(p) = drag.curve.get_mut(i) {
                        p.value = kind.clamp(p.value + by).unwrap_or(p.value);
                    }
                }
            }
            DragMode::Points { .. } | DragMode::Draw { .. } => {}
        }
    }
    if response.drag_stopped()
        && let Some(drag) = state.expression_drag.take_if(|d| d.of(id, kind) && d.index == segment)
    {
        if drag.curve != drag.from {
            actions.push(set_action(cx.clip, index, kind, &drag.curve));
        }
        state.mpe.last_kind = kind;
        return true;
    }
    false
}

/// A point's click (removes it, or with Shift selects it), right-click (its menu) and drag (it,
/// or the selected points, move). True when an edit was sent.
fn point_gestures(
    cx: &Cx<'_>,
    (index, id, note, kind, area): (usize, Option<NoteId>, &Note, ExpressionKind, Rect),
    point: usize,
    curve: &[ExpressionPoint],
    response: &egui::Response,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
) -> bool {
    if response.secondary_clicked()
        && let (Some(id), Some(pos)) = (id, response.interact_pointer_pos())
    {
        state.mpe.menu = Some(PointMenu { note: id, kind, point, pos, text: None });
        return false;
    }
    if response.clicked() {
        state.mpe.last_kind = kind;
        if cx.shift {
            let selection = &mut state.mpe.selection;
            if selection.note != id || selection.kind != Some(kind) {
                *selection = PointSelection { note: id, kind: Some(kind), points: BTreeSet::new() };
            }
            if !selection.points.remove(&point) {
                selection.points.insert(point);
            }
            return false;
        }
        actions.push(set_action(cx.clip, index, kind, &removed(curve, point, kind)));
        state.mpe.selection = PointSelection::default();
        return true;
    }
    let pos = cx.ui.ctx().pointer_latest_pos();
    if response.drag_started()
        && let Some(id) = id
    {
        let press = cx.ui.input(|i| i.pointer.press_origin()).or(pos).unwrap_or_default();
        // A point outside the selection drags alone and clears it.
        let selection = &state.mpe.selection;
        if !(selection.note == Some(id) && selection.kind == Some(kind) && selection.points.contains(&point)) {
            state.mpe.selection = PointSelection::default();
        }
        let from = own_curve(note, kind).to_vec();
        state.expression_drag = Some(PointDrag {
            note: id,
            kind,
            index: point,
            curve: curve.to_vec(),
            from,
            mode: DragMode::Points { press },
        });
    }
    if response.dragged()
        && let Some(pos) = pos
        && let Some(drag) = state.expression_drag.as_mut().filter(|d| d.of(id, kind) && d.index == point)
        && let DragMode::Points { press } = drag.mode
    {
        let group: BTreeSet<usize> = if state.mpe.selection.points.is_empty() {
            BTreeSet::from([point])
        } else {
            state.mpe.selection.points.clone()
        };
        let moved = Move {
            beats: beat_at(cx.layout, cx.grid, pos.x, cx.snap) - beat_at(cx.layout, cx.grid, press.x, cx.snap),
            value: (|| {
                let now = value_at(cx.layout, note, kind, area, pos.y, false)?;
                let then = value_at(cx.layout, note, kind, area, press.y, false)?;
                Some(now - then)
            })()
            .unwrap_or(0.0),
        };
        let delta = constrain(moved, pos - press, cx.shift);
        drag.curve = moved_points(&drag.from, &group, point, delta, note.duration, kind, cx.snap);
    }
    if response.drag_stopped()
        && let Some(drag) = state.expression_drag.take_if(|d| d.of(id, kind) && d.index == point)
    {
        if drag.curve != drag.from {
            actions.push(set_action(cx.clip, index, kind, &drag.curve));
        }
        state.mpe.selection = PointSelection::default();
        state.mpe.last_kind = kind;
        return true;
    }
    false
}

/// How far a drag moves its points: beats and value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Move {
    pub(super) beats: f64,
    pub(super) value: f64,
}

/// With Shift, a drag keeps to the axis it moved furthest along, and a vertical one moves a
/// tenth as far.
pub(super) fn constrain(moved: Move, screen: Vec2, shift: bool) -> Move {
    if !shift {
        return moved;
    }
    if screen.x.abs() > screen.y.abs() {
        Move { value: 0.0, ..moved }
    } else {
        Move { beats: 0.0, value: moved.value * 0.1 }
    }
}

/// `from` with the `group`'s points (the one `grabbed` among them) moved by `delta`: kept inside
/// the note and the kind's range; for pitch on whole semitones when `snap`. A point another one
/// passes over goes. The result is in time order without two points at one offset.
pub(super) fn moved_points(
    from: &[ExpressionPoint],
    group: &BTreeSet<usize>,
    grabbed: usize,
    delta: Move,
    duration: f64,
    kind: ExpressionKind,
    snap: bool,
) -> Vec<ExpressionPoint> {
    let mut indices: BTreeSet<usize> = group.clone();
    indices.insert(grabbed);
    // Each moved point, with where it was.
    let moved: Vec<(f64, ExpressionPoint)> = indices
        .iter()
        .filter_map(|&i| from.get(i))
        .map(|p| {
            let value = p.value + delta.value;
            let value = if snap && kind == ExpressionKind::Pitch { value.round() } else { value };
            let offset = (p.offset + delta.beats).clamp(0.0, duration.max(0.0));
            (p.offset, ExpressionPoint { offset, value: kind.clamp(value).unwrap_or(p.value), ..*p })
        })
        .collect();
    // The points left behind, but those a moved point passed over or landed on.
    let mut out: Vec<ExpressionPoint> = from
        .iter()
        .enumerate()
        .filter(|(i, _)| !indices.contains(i))
        .map(|(_, p)| *p)
        .filter(|p| {
            !moved.iter().any(|(was, now)| {
                let (low, high) = if *was <= now.offset { (*was, now.offset) } else { (now.offset, *was) };
                (p.offset > low + GAP && p.offset < high - GAP) || (p.offset - now.offset).abs() <= GAP
            })
        })
        .collect();
    out.extend(moved.into_iter().map(|(_, p)| p));
    out.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    out.dedup_by(|later, earlier| (later.offset - earlier.offset).abs() <= GAP);
    out
}

/// `curve` without point `point`; points left only at the neutral value leave nothing.
pub(super) fn removed(curve: &[ExpressionPoint], point: usize, kind: ExpressionKind) -> Vec<ExpressionPoint> {
    let mut edited = curve.to_vec();
    if point < edited.len() {
        edited.remove(point);
    }
    if edited.iter().all(|p| p.value == kind.neutral()) {
        edited.clear();
    }
    edited
}

/// The menu of a right-clicked point: Edit value…, which then takes the value from the keyboard,
/// and Delete point.
fn point_menu(cx: &Cx<'_>, state: &mut PianoRollState, actions: &mut Vec<Action>) -> bool {
    let Some(menu) = state.mpe.menu.clone() else { return false };
    let Some(index) = cx.clip.notes.position(menu.note) else {
        state.mpe.menu = None;
        return false;
    };
    let Some(note) = cx.clip.notes.get(index) else { return false };
    let curve = own_curve(note, menu.kind).to_vec();
    let Some(point) = curve.get(menu.point).copied() else {
        state.mpe.menu = None;
        return false;
    };
    let mut close = false;
    let mut picked: Option<&'static str> = None;
    let mut typed = menu.text.clone();
    let area = egui::Area::new(Id::unique("piano-mpe-point-menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(menu.pos + Vec2::new(4.0, 4.0))
        .show(cx.ui.ctx(), |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_min_width(120.0);
                match &mut typed {
                    None => {
                        if crate::kit::menu::action(ui, "Edit value…", None, true).clicked() {
                            picked = Some("edit");
                        }
                        if crate::kit::menu::action(ui, "Delete point", None, true).clicked() {
                            picked = Some("delete");
                        }
                    }
                    Some(text) => {
                        crate::kit::menu::hint(
                            ui,
                            if menu.kind == ExpressionKind::Pitch { "Semitones" } else { "0 to 127" },
                        );
                        let field = ui.add(egui::TextEdit::singleline(text).desired_width(100.0));
                        field.request_focus();
                    }
                }
            });
        });
    match picked {
        Some("edit") => typed = Some(value_text(menu.kind, point.value).trim_end_matches(" st").to_owned()),
        Some("delete") => {
            actions.push(set_action(cx.clip, index, menu.kind, &removed(&curve, menu.point, menu.kind)));
            close = true;
        }
        _ => {}
    }
    if let Some(text) = &typed {
        let (enter, escape) = cx.ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
        if escape {
            close = true;
        } else if enter {
            if let Some(value) = parse_value(menu.kind, text) {
                let mut edited = curve.clone();
                if let Some(p) = edited.get_mut(menu.point) {
                    p.value = value;
                }
                actions.push(set_action(cx.clip, index, menu.kind, &edited));
            }
            close = true;
        }
    }
    // A press outside the menu closes it.
    let outside = cx.ui.input(|i| i.pointer.any_pressed())
        && cx.ui.ctx().pointer_latest_pos().is_some_and(|p| !area.response.rect.contains(p));
    if close || (outside && picked.is_none() && menu.text.is_some() == typed.is_some()) {
        state.mpe.menu = None;
    } else if let Some(open) = state.mpe.menu.as_mut() {
        open.text = typed;
    }
    true
}

/// A typed value: semitones (a `+` and ` st` allowed) for pitch, 0 to 127 for the lanes; `None`
/// when it is not a number. Kept in the kind's range.
pub(super) fn parse_value(kind: ExpressionKind, text: &str) -> Option<f64> {
    let cleaned = text.trim().trim_end_matches("st").trim().trim_start_matches('+');
    let number: f64 = cleaned.replace(',', ".").parse().ok()?;
    match kind {
        ExpressionKind::Pitch => kind.clamp(number),
        _ => kind.clamp(number / 127.0),
    }
}

/// Ctrl+A while MPE is on and one note is selected: every point of its curve last edited.
/// `false` when that does not apply, so the key selects every note as it does otherwise.
pub(super) fn select_all_points(clip: &PianoClip, state: &mut PianoRollState) -> bool {
    if state.expression.is_none() || state.selected.len() != 1 {
        return false;
    }
    let Some(&index) = state.selected.first() else { return false };
    let Some(note) = clip.notes.get(index) else { return false };
    let kind = state.mpe.last_kind;
    let count = own_curve(note, kind).len();
    if count == 0 {
        return false;
    }
    state.mpe.selection = PointSelection { note: clip.notes.id(index), kind: Some(kind), points: (0..count).collect() };
    true
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

/// The selected note a double-click at `pos` in a lane adds a point to: one whose time takes in
/// the pointer's beat, the latest to start.
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

/// Adds a point to note `index`'s `kind` curve where `pos` is, in `area`: a curve's first point
/// after the note's start also gets one at the start at the neutral value (the note's own
/// pitch), so the change starts from the note.
pub(super) fn add_point(
    layout: &Layout,
    clip: &PianoClip,
    grid: TimelineGrid,
    snap: bool,
    (index, kind, area): (usize, ExpressionKind, Rect),
    pos: Pos2,
) -> Option<Action> {
    let note = clip.notes.get(index)?;
    let offset = (beat_at(layout, grid, pos.x, snap) - note.time).clamp(0.0, note.duration.max(0.0));
    let value = value_at(layout, note, kind, area, pos.y, snap)?;
    let mut expression = note.expression().cloned().unwrap_or_default();
    if expression.curve(kind).is_empty() && offset > 0.0 {
        expression.set_point(kind, note.duration, 0.0, kind.neutral()).ok()?;
    }
    expression.set_point(kind, note.duration, offset, value).ok()?;
    Some(set_action(clip, index, kind, expression.curve(kind)))
}

/// `note.setExpression` for note `index` of the clip: `kind`'s whole curve becomes `curve`, a
/// curved point with its curvature as a third number.
pub(super) fn set_action(clip: &PianoClip, index: usize, kind: ExpressionKind, curve: &[ExpressionPoint]) -> Action {
    let mut params = clip.address.clone();
    if let Some(map) = params.as_object_mut() {
        map.insert("note".into(), json!(index));
        map.insert("kind".into(), json!(kind.name()));
        let points = curve
            .iter()
            .map(|p| if p.curve == 0.0 { json!([p.offset, p.value]) } else { json!([p.offset, p.value, p.curve]) })
            .collect();
        map.insert("points".into(), Value::Array(points));
    }
    Action::new("note.setExpression", params)
}

#[cfg(test)]
#[path = "expression_tests.rs"]
mod tests;
