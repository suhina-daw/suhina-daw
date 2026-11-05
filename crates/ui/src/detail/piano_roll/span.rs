// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What is selected in time (› Selecting in `docs/spec/ui-clip-editor.md`): the stretch a rectangle
//! chose when it reached no note, the bar that shows the selection's extent over the grid, the mark
//! a click leaves where a paste goes, and how a copy carries the stretch on.

use std::hash::{DefaultHasher, Hash, Hasher};

use super::*;

/// Height of the bar along the ruler's lower edge that shows what is selected in time, the width of
/// the edges of a time selection and the size of the insert mark's flag, in points.
const SPAN_BAR: f32 = 3.0;
const SPAN_EDGE: f32 = 1.5;
const MARK_FLAG: f32 = 5.0;
/// How strongly a time selection tints the rows, unless the sheet sets it.
const SPAN_TINT: f32 = 0.38;
/// How many copies in a row Ctrl+Z walks back through; older ones keep their notes, not the stretch.
const SPAN_STEPS: usize = 64;

/// A stretch of the clip selected in time, in beats, and the notes that were selected with it: another
/// selection ends it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct TimeSpan {
    pub(super) start: f64,
    pub(super) end: f64,
    pub(super) with: BTreeSet<usize>,
}

/// What a copy (the duplicate key, a paste) left behind, to go back to when it is undone: the
/// selection then, the copies it made, and the clip's notes then, by their hash. Notes back as they
/// were say the copy was undone, even where it replaced as many notes as it made.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SpanStep {
    span: Option<TimeSpan>,
    selected: BTreeSet<usize>,
    copies: BTreeSet<usize>,
    notes: u64,
}

/// A hash of what `notes` hold.
fn notes_hash(notes: &[Note]) -> u64 {
    let mut hasher = DefaultHasher::new();
    notes.len().hash(&mut hasher);
    for note in notes {
        note.time.to_bits().hash(&mut hasher);
        note.duration.to_bits().hash(&mut hasher);
        note.key.hash(&mut hasher);
        note.velocity.to_bits().hash(&mut hasher);
        note.muted.hash(&mut hasher);
    }
    hasher.finish()
}

/// The stretch a selecting rectangle covers: from the grid line before its left edge to the one after
/// its right edge, or its own edges without a snapping grid, and all of every note it touches.
pub(super) fn swept_span(grid: TimelineGrid, layout: &Layout, notes: &[Note], rect: Rect) -> (f64, f64) {
    let start = grid.place(layout.beat_at(rect.left()), false, true);
    let right = layout.beat_at(rect.right()).max(0.0);
    let floor = grid.place(right, false, true);
    let end = if grid.snaps && right > floor + EPSILON { grid.next_line(floor) } else { floor };
    let touched = || notes.iter().filter(|n| layout.note_rect(n).intersects(rect));
    let start = touched().map(|n| n.time).fold(start, f64::min);
    let end = touched().map(|n| n.time + n.duration).fold(end, f64::max);
    (start, end.min(f64::from(BEAT_LIMIT)))
}

/// The notes `set` once the notes `replaced` (in order) are gone: those left, at their new indices.
pub(super) fn after_replaced(set: &BTreeSet<usize>, replaced: &[usize]) -> BTreeSet<usize> {
    set.iter().filter(|i| replaced.binary_search(i).is_err()).map(|i| i - replaced.partition_point(|r| r < i)).collect()
}

impl PianoRollState {
    /// Brings the selection and what hangs on it up to date with the clip's `notes`, before anything
    /// uses them. An undo that left the notes as they were before a copy took the copy back, and a
    /// copy past the end of the notes never came (the command refused it): the selection before it
    /// comes back.
    pub(super) fn settle_selection(&mut self, notes: &[Note]) {
        let count = notes.len();
        let taken_back = self.copy_taken_back(notes);
        self.undone = false;
        if taken_back && let Some(step) = self.span_undo.pop() {
            self.span = step.span;
            self.selected = step.selected;
        }
        self.selected.retain(|i| *i < count);
        if self.span_undo.last().is_some_and(|step| step.copies != self.selected) {
            self.span_undo.clear();
        }
        if self.span.as_ref().is_some_and(|span| span.with != self.selected) {
            self.span = None;
        }
        if !self.selected.is_empty() {
            self.insert_at = None;
        }
        self.span_notes = match &self.span {
            Some(span) => notes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.time >= span.start - EPSILON && n.time < span.end - EPSILON)
                .map(|(i, _)| i)
                .collect(),
            None => BTreeSet::new(),
        };
    }

    /// Whether the selection is the copies of the last copy and that copy is gone: undone, or
    /// refused for running past the end of the notes. [`Self::settle_selection`] then brings back
    /// the selection before it.
    pub(super) fn copy_taken_back(&self, notes: &[Note]) -> bool {
        self.span_undo.last().is_some_and(|step| {
            step.copies == self.selected
                && (step.copies.iter().any(|i| *i >= notes.len()) || (self.undone && step.notes == notes_hash(notes)))
        })
    }

    /// An undo ran: the window says so, as a copy can leave the notes just as they were.
    pub fn undo_ran(&mut self) {
        self.undone = true;
    }

    /// The notes the edit keys act on: the selected ones, or with none selected those that start in
    /// the time selection.
    pub fn acting(&self) -> &BTreeSet<usize> {
        if self.selected.is_empty() { &self.span_notes } else { &self.selected }
    }

    /// Where the stretch selected in time ends, in beats.
    pub fn span_end(&self) -> Option<f64> {
        self.span.as_ref().map(|span| round_beat(span.end))
    }

    /// Forgets everything selected in time.
    pub(super) fn clear_span(&mut self) {
        self.span = None;
        self.span_notes.clear();
        self.span_undo.clear();
        self.insert_at = None;
    }

    /// A rectangle let go: what it covered is selected in time, the empty parts too, with the notes
    /// it selected.
    pub(super) fn select_span(&mut self, start: f64, end: f64) {
        self.clear_span();
        if end - start > EPSILON {
            self.span = Some(TimeSpan { start, end, with: self.selected.clone() });
        }
    }

    /// Copies are about to become the selection, indices `copies`, made from `notes`: a time selection
    /// goes on to the stretch after it, so the next copy lands after these, and an undo comes back here.
    pub(super) fn select_copies(&mut self, copies: BTreeSet<usize>, notes: &[Note]) {
        let next = self.span.as_ref().map(|span| TimeSpan {
            start: span.end,
            end: span.end + (span.end - span.start),
            with: copies.clone(),
        });
        if self.span_undo.len() >= SPAN_STEPS {
            self.span_undo.remove(0);
        }
        self.span_undo.push(SpanStep {
            span: self.span.take(),
            selected: std::mem::take(&mut self.selected),
            copies: copies.clone(),
            notes: notes_hash(notes),
        });
        self.span = next;
        self.selected = copies;
        self.selecting_new = true;
    }

    /// Moved notes take a time selection along.
    pub(super) fn span_moved(&mut self, beats: f64) {
        if let Some(span) = &mut self.span {
            span.start += beats;
            span.end += beats;
        }
    }

    /// A command's replaced notes moved the indices: what hangs on the selection follows (see
    /// [`Self::notes_replaced`]). What an undo brings back keeps its indices, as the notes come back too.
    pub(super) fn span_notes_replaced(&mut self, replaced: &[usize]) {
        if let Some(span) = &mut self.span {
            span.with = after_replaced(&span.with, replaced);
        }
        if let Some(step) = self.span_undo.last_mut() {
            step.copies = after_replaced(&step.copies, replaced);
        }
    }

    /// What is selected in time, to show on the bar: the stretch, or the extent of the selected notes.
    pub(super) fn span_shown(&self, clip: &PianoClip) -> Option<(f64, f64)> {
        if let Some(span) = &self.span {
            return Some((span.start, span.end));
        }
        let chosen = || self.selected.iter().filter_map(|i| clip.notes.get(*i));
        let start = chosen().map(|n| n.time).fold(f64::INFINITY, f64::min);
        let end = chosen().map(|n| n.time + n.duration).fold(f64::NEG_INFINITY, f64::max);
        (start.is_finite() && end > start).then_some((start, end))
    }
}

/// A time selection over the rows, under the notes, and the insert mark. Notes selected with it (the
/// copies it went on with) say for themselves what is selected.
pub(super) fn draw_time_span(painter: &egui::Painter, layout: &Layout, state: &PianoRollState) {
    let style = part_style("piano-time-selection", &[]);
    let edge = style.border.unwrap_or_else(theme::accent);
    if let Some(span) = state.span.as_ref().filter(|_| state.selected.is_empty() && state.gesture.is_none()) {
        let rect = Rect::from_x_y_ranges(layout.x_of(span.start)..=layout.x_of(span.end), layout.grid.y_range());
        painter.rect_filled(rect, 0.0, style.background.unwrap_or_else(|| theme::accent().gamma_multiply(SPAN_TINT)));
        painter.vline(rect.left(), rect.y_range(), Stroke::new(SPAN_EDGE, edge));
        painter.vline(rect.right(), rect.y_range(), Stroke::new(SPAN_EDGE, edge));
    }
    if let Some(beat) = state.insert_at {
        let color = part_style("piano-insert-mark", &[]).background.unwrap_or_else(theme::text_primary);
        painter.vline(layout.x_of(beat).round() + 0.5, layout.grid.y_range(), Stroke::new(1.0, color));
    }
}

/// Along the ruler's lower edge, what is selected in time: while a rectangle is being drawn what it
/// covers, else the time selection or the selected notes' extent. The insert mark's flag beside it.
pub(super) fn draw_span_bar(ui: &Ui, layout: &Layout, clip: &PianoClip, state: &PianoRollState, live: Option<Rect>) {
    let grid = note_grid(clip, layout.beat_width);
    let kept = state.span_shown(clip).filter(|_| matches!(state.gesture, Some(Gesture::Select { add: true, .. })));
    let shown = live
        .map(|rect| {
            let (start, end) = swept_span(grid, layout, &clip.notes, rect);
            kept.map_or((start, end), |k| (k.0.min(start), k.1.max(end)))
        })
        .or_else(|| state.span_shown(clip));
    let painter = ui.painter_at(layout.ruler);
    if let Some((start, end)) = shown {
        let color = part_style("piano-time-selection", &[]).border.unwrap_or_else(theme::accent);
        let bottom = layout.ruler.bottom();
        let bar = Rect::from_min_max(
            Pos2::new(layout.x_of(start), bottom - SPAN_BAR),
            Pos2::new(layout.x_of(end).max(layout.x_of(start) + 1.0), bottom),
        );
        painter.rect_filled(bar, 0.0, color);
        for x in [bar.left(), bar.right()] {
            painter.vline(x, bottom - 2.0 * SPAN_BAR..=bottom, Stroke::new(1.0, color));
        }
        let response =
            ui.interact(bar.intersect(layout.ruler), Id::unique("piano-roll-time-selection"), Sense::hover());
        response.widget_info(|| {
            let label = format!("Selected in time, beats {} to {}", round_beat(start), round_beat(end));
            egui::WidgetInfo::labeled(egui::Role::Unknown, true, label)
        });
    }
    if let Some(beat) = state.insert_at {
        let color = part_style("piano-insert-mark", &[]).background.unwrap_or_else(theme::text_primary);
        let (x, bottom) = (layout.x_of(beat).round() + 0.5, layout.ruler.bottom());
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(x - MARK_FLAG, bottom - MARK_FLAG),
                Pos2::new(x + MARK_FLAG, bottom - MARK_FLAG),
                Pos2::new(x, bottom),
            ],
            color,
            Stroke::NONE,
        ));
        let flag = Rect::from_center_size(Pos2::new(x, bottom - MARK_FLAG / 2.0), Vec2::splat(2.0 * MARK_FLAG));
        let response = ui.interact(flag.intersect(layout.ruler), Id::unique("piano-roll-insert-mark"), Sense::hover());
        response.widget_info(|| {
            let label = format!("Insert mark at beat {}", round_beat(beat));
            egui::WidgetInfo::labeled(egui::Role::Unknown, true, label)
        });
    }
}
