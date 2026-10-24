// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll's ruler (`docs/spec/ui-clip-editor.md` › Layout › Ruler, › Zoom and scroll): an
//! 8 px loop row on top, with a bar over the clip's loop and a handle at each end, and under it
//! the bar numbers with the clip's start and end markers. Dragging a handle, the bar or a marker
//! changes the clip's loop, start or length on the piano roll's grid (Alt: free), shown while it
//! is dragged and made one command (`clip.setLoop`, `clip.setStart`, `clip.setLength`) when it is
//! let go. Dragging anywhere else on the ruler zooms time (up and down) and scrolls it (sideways).

use crate::kit::surface::Backgrounds as _;
use egui::{Color32, CursorIcon, Id, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use serde_json::{Value, json};

use crate::detail::piano_roll::{PianoClip, PianoRollState, note_grid};
use crate::interaction::timeline_grid::TimelineGrid;
use crate::kit::theme;
use crate::kit::widgets::Action;

/// Height of the loop row at the top of the ruler, of the bar in it, and how far from a handle or
/// a marker a press still grabs it, in points.
pub(crate) const LOOP_ROW: f32 = 8.0;
const LOOP_BAR: f32 = 4.0;
/// Width of a loop handle, in points.
const HANDLE: f32 = 2.0;
const GRAB: f32 = 4.0;
/// The shortest loop or clip a drag makes when the grid does not snap, in beats.
const MIN_LENGTH: f64 = 0.01;
/// The furthest beat, and the longest length, the commands take.
const BEAT_LIMIT: f64 = 120_000.0;
/// How much a point of vertical drag on the ruler zooms time: down zooms in.
const ZOOM_PER_POINT: f32 = 0.01;
/// The range of points per beat, as the wheel zooms it.
const BEAT_WIDTH_RANGE: (f32, f32) = (4.0, 400.0);
const EPSILON: f64 = 1e-9;

/// A clip's start marker, length and loop, as the ruler shows and changes them.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ClipShape {
    /// The start marker: where in the content the clip starts playing, in beats.
    pub offset: f64,
    /// How long the clip plays, in beats.
    pub length: f64,
    /// The loop's start and end in content beats, while the clip loops.
    pub loop_range: Option<(f64, f64)>,
    /// The longest the clip may be: an arrangement clip reaches up to the next clip on its lane;
    /// `None` when nothing follows it (a slot's clip, the last clip of a lane).
    pub room: Option<f64>,
}

impl ClipShape {
    /// The part of the content the clip ever plays, `(start, end)` in beats: from the start
    /// marker (or the loop's start, when that is earlier) to the loop's end, or without a loop
    /// to the clip's end marker. Notes outside it are shaded.
    pub fn played(&self) -> (f64, f64) {
        match self.loop_range {
            Some((start, end)) => (self.offset.min(start), end),
            None => (self.offset, self.offset + self.length),
        }
    }

    /// The loop bar's ends: the loop, or the loop turning it on would give (the whole length,
    /// from 0, as `clip.setLoop` makes it).
    pub fn bar(&self) -> (f64, f64) {
        self.loop_range.unwrap_or((0.0, self.length))
    }

    /// Where the end marker is, in content beats; `None` while the clip loops (the loop's end is
    /// where its content ends then).
    pub fn end_marker(&self) -> Option<f64> {
        self.loop_range.is_none().then_some(self.offset + self.length)
    }

    fn max_length(&self) -> f64 {
        self.room.map_or(BEAT_LIMIT, |r| r.min(BEAT_LIMIT))
    }
}

/// What a press in the ruler grabbed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    LoopStart,
    LoopEnd,
    /// The loop bar between its handles: dragging moves the loop, a click turns it on or off.
    Loop,
    StartMarker,
    EndMarker,
}

impl Part {
    /// The name a screen reader and a test find it by.
    pub fn label(self) -> &'static str {
        match self {
            Self::LoopStart => "Loop start",
            Self::LoopEnd => "Loop end",
            Self::Loop => "Loop",
            Self::StartMarker => "Clip start marker",
            Self::EndMarker => "Clip end marker",
        }
    }
}

/// A drag on the ruler in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    /// A handle, the bar or a marker: the content beat the press was at, and where it is now.
    Part { part: Part, from: f64, now: f64 },
    /// Zooming (up and down) and scrolling (sideways) time.
    Zoom,
}

/// What the ruler keeps between frames.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RulerState {
    drag: Option<Drag>,
}

/// The time axis: the x of beat 0 and the points per beat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axis {
    pub origin: f32,
    pub beat_width: f32,
}

impl Axis {
    pub fn x_of(&self, beat: f64) -> f32 {
        self.origin + beat as f32 * self.beat_width
    }

    pub fn beat_at(&self, x: f32) -> f64 {
        f64::from((x - self.origin) / self.beat_width.max(f32::MIN_POSITIVE))
    }
}

/// What a press at `pos` in the `ruler` grabs: in the loop row a handle (the nearer one when both
/// are in reach) or the bar between them; under it the start marker or the end marker. `None`
/// elsewhere, where a drag zooms and scrolls.
pub fn part_at(shape: &ClipShape, axis: &Axis, ruler: Rect, pos: Pos2) -> Option<Part> {
    let distance = |beat: f64| (pos.x - axis.x_of(beat)).abs();
    let nearest = |candidates: &[(Part, f64)]| {
        candidates
            .iter()
            .map(|(part, beat)| (*part, distance(*beat)))
            .filter(|(_, d)| *d <= GRAB)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(part, _)| part)
    };
    if pos.y < ruler.top() + LOOP_ROW {
        let (start, end) = shape.bar();
        nearest(&[(Part::LoopStart, start), (Part::LoopEnd, end)])
            .or_else(|| (axis.x_of(start)..=axis.x_of(end)).contains(&pos.x).then_some(Part::Loop))
    } else {
        let mut markers = vec![(Part::StartMarker, shape.offset)];
        markers.extend(shape.end_marker().map(|end| (Part::EndMarker, end)));
        nearest(&markers)
    }
}

/// How far a dragged loop or clip edge may come to the other edge `at`: to the drawn line before
/// it (`after`: after it) while it snaps, else a hundredth of a beat.
fn limit(grid: TimelineGrid, free: bool, at: f64, after: bool) -> f64 {
    match (grid.snaps && !free, after) {
        (true, false) => grid.previous_line(at),
        (true, true) => grid.next_line(at),
        (false, false) => at - MIN_LENGTH,
        (false, true) => at + MIN_LENGTH,
    }
}

/// The clip as a drag of `part` from content beat `from` to `now` leaves it: on the grid unless
/// `free`, never before beat 0 or past the last beat, a loop or a clip never shorter than
/// [`limit`] (a drawn line), and an arrangement clip never longer than its room. The commands take every shape
/// this gives, so a drag never fails.
pub fn dragged(shape: &ClipShape, part: Part, from: f64, now: f64, grid: TimelineGrid, free: bool) -> ClipShape {
    let place = |beat: f64| grid.place(beat, free, false).clamp(0.0, BEAT_LIMIT);
    let before = |at: f64| limit(grid, free, at, false);
    let after = |at: f64| limit(grid, free, at, true);
    let (start, end) = shape.bar();
    let mut out = *shape;
    match part {
        Part::LoopStart => {
            let at = place(now);
            let at = if at > before(end) + EPSILON { before(end) } else { at };
            if at >= 0.0 && at < end {
                out.loop_range = Some((at, end));
            }
        }
        Part::LoopEnd => {
            let at = place(now);
            let at = if at < after(start) - EPSILON { after(start) } else { at };
            if at <= BEAT_LIMIT && at > start {
                out.loop_range = Some((start, at));
            }
        }
        Part::Loop => {
            let length = end - start;
            let at = place(start + (now - from)).min(BEAT_LIMIT - length).max(0.0);
            out.loop_range = Some((at, at + length));
        }
        Part::StartMarker => {
            let at = place(now);
            match shape.loop_range {
                Some((_, end)) => {
                    let at = if at > before(end) + EPSILON { before(end) } else { at };
                    out.offset = at.max(0.0);
                }
                None => {
                    let end = shape.offset + shape.length;
                    let at = if at > before(end) + EPSILON { before(end) } else { at };
                    // Earlier makes the clip longer: never past its room.
                    let at = at.max(end - shape.max_length()).max(0.0);
                    if at < end {
                        out.offset = at;
                        out.length = end - at;
                    }
                }
            }
        }
        Part::EndMarker => {
            if shape.loop_range.is_none() {
                let at = place(now);
                let at = if at < after(shape.offset) - EPSILON { after(shape.offset) } else { at };
                out.length = (at - shape.offset).min(shape.max_length());
            }
        }
    }
    out
}

/// Beats rounded to a millionth, as the piano roll sends them.
fn round(beat: f64) -> f64 {
    (beat * 1e6).round() / 1e6
}

/// The command a drag of `part` that left the clip `after` runs; `None` when nothing changed.
pub fn command(address: &Value, before: &ClipShape, after: &ClipShape, part: Part) -> Option<Action> {
    if before == after {
        return None;
    }
    let mut params = address.clone();
    let id = match part {
        Part::LoopStart | Part::LoopEnd | Part::Loop => {
            let (start, end) = after.loop_range?;
            params["start"] = json!(round(start));
            params["end"] = json!(round(end));
            "clip.setLoop"
        }
        Part::StartMarker => {
            params["offset"] = json!(round(after.offset));
            "clip.setStart"
        }
        Part::EndMarker => {
            // Rounded down, so a clip that reaches its room exactly is not a millionth past it.
            params["length"] = json!((after.length * 1e6).floor() / 1e6);
            "clip.setLength"
        }
    };
    Some(Action::new(id, params))
}

/// The ruler's gestures for this frame, over `ruler`, with the grid's left edge at `left`: a
/// press on a handle, the bar or a marker drags it on the piano roll's grid (Alt, `free`: off it), and the
/// command is pushed when it is let go; a click on the bar turns the loop on or off; a drag
/// anywhere else zooms time around the pointer (up and down) and scrolls it (sideways). Returns
/// the clip as the drag in progress leaves it, to draw.
pub fn interact(
    ui: &Ui,
    ruler: Rect,
    left: f32,
    clip: &PianoClip,
    state: &mut PianoRollState,
    free: bool,
    actions: &mut Vec<Action>,
) -> Option<ClipShape> {
    let grid = note_grid(clip, state.beat_width);
    let axis = |state: &PianoRollState| Axis { origin: left - state.scroll.x, beat_width: state.beat_width };
    let response = ui.interact(ruler, Id::new("piano-roll-ruler"), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Piano roll ruler"));
    let shape = clip.shape;

    if let Some(pos) = response.hover_pos() {
        let cursor = match state.ruler.drag {
            Some(Drag::Part { part: Part::Loop, .. }) => Some(CursorIcon::Grabbing),
            Some(Drag::Part { .. }) => Some(CursorIcon::ResizeHorizontal),
            Some(Drag::Zoom) => Some(CursorIcon::ResizeVertical),
            None => match part_at(&shape, &axis(state), ruler, pos) {
                Some(Part::Loop) => Some(CursorIcon::Grab),
                Some(_) => Some(CursorIcon::ResizeHorizontal),
                None => None,
            },
        };
        if let Some(cursor) = cursor {
            ui.ctx().set_cursor_icon(cursor);
        }
    }

    if response.drag_started_by(PointerButton::Primary)
        && let Some(origin) = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos())
    {
        let at = axis(state).beat_at(origin.x);
        state.ruler.drag = Some(match part_at(&shape, &axis(state), ruler, origin) {
            Some(part) => Drag::Part { part, from: at, now: at },
            None => Drag::Zoom,
        });
    }
    let pointer = ui.input(|i| i.pointer.latest_pos()).or(response.interact_pointer_pos());
    if response.dragged_by(PointerButton::Primary)
        && let Some(pos) = pointer
    {
        match state.ruler.drag {
            Some(Drag::Part { part, from, .. }) => {
                state.ruler.drag = Some(Drag::Part { part, from, now: axis(state).beat_at(pos.x) });
            }
            Some(Drag::Zoom) => {
                let delta = ui.input(|i| i.pointer.delta());
                // The beat that was under the pointer stays under it: sideways it follows the
                // pointer (a scroll), and up and down the time around it grows or shrinks.
                let beat = axis(state).beat_at(pos.x - delta.x);
                state.beat_width =
                    (state.beat_width * (delta.y * ZOOM_PER_POINT).exp()).clamp(BEAT_WIDTH_RANGE.0, BEAT_WIDTH_RANGE.1);
                state.scroll.x = (beat as f32 * state.beat_width - (pos.x - left)).max(0.0);
            }
            None => {}
        }
    }
    let preview = match state.ruler.drag {
        Some(Drag::Part { part, from, now }) => Some((part, dragged(&shape, part, from, now, grid, free))),
        _ => None,
    };
    if response.drag_stopped_by(PointerButton::Primary) {
        if let Some((part, after)) = preview {
            actions.extend(command(&clip.address, &shape, &after, part));
        }
        state.ruler.drag = None;
    } else if !ui.input(|i| i.pointer.primary_down()) {
        // A drag that lost its ruler (the view changed under it) ends.
        state.ruler.drag = None;
    }
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && part_at(&shape, &axis(state), ruler, pos) == Some(Part::Loop)
    {
        let mut params = clip.address.clone();
        params["loop"] = json!(shape.loop_range.is_none());
        actions.push(Action::new("clip.setLoop", params));
    }
    preview.map(|(_, after)| after)
}

/// Draws the loop row and the markers over the `ruler` (its bar numbers are drawn with the grid),
/// for `shape` as the clip is or as a drag leaves it, and names the handles and markers for
/// screen readers and tests.
pub fn draw(ui: &Ui, ruler: Rect, axis: &Axis, shape: &ClipShape) {
    let painter = ui.painter_at(ruler);
    let row = Rect::from_min_max(ruler.min, Pos2::new(ruler.right(), ruler.top() + LOOP_ROW));
    painter.fill_background(row, 0.0, theme::window_background());
    painter.hline(row.x_range(), row.bottom() - 0.5, theme::hairline(theme::border()));

    // The loop bar: bright while the clip loops, dim where turning the loop on would put it.
    let looping = shape.loop_range.is_some();
    let (start, end) = shape.bar();
    let (left, right) = (axis.x_of(start).round(), axis.x_of(end).round());
    let bar = Rect::from_min_max(
        Pos2::new(left, row.center().y - LOOP_BAR / 2.0),
        Pos2::new(right.max(left + 1.0), row.center().y + LOOP_BAR / 2.0),
    );
    let (fill, edge) = if looping {
        (theme::control_active(), theme::text_secondary())
    } else {
        (theme::control_background(), theme::text_hint())
    };
    painter.fill_background(bar, 0.0, fill);
    painter.rect_stroke(bar, 0.0, theme::hairline(edge), StrokeKind::Inside);
    let handle_color = if looping { theme::text_primary() } else { theme::text_hint() };
    for x in [left, right] {
        painter.rect_filled(
            Rect::from_center_size(Pos2::new(x, row.center().y), Vec2::new(HANDLE, LOOP_ROW)),
            0.0,
            handle_color,
        );
    }

    // The markers, under the loop row: a flag pointing into the part the clip plays.
    let numbers = Rect::from_min_max(Pos2::new(ruler.left(), row.bottom()), ruler.max);
    marker(&painter, numbers, axis.x_of(shape.offset), 1.0, theme::text_primary());
    if let Some(end) = shape.end_marker() {
        marker(&painter, numbers, axis.x_of(end), -1.0, theme::text_primary());
    }

    // Names for readers: each handle and marker, where it is.
    let mut parts = vec![(Part::LoopStart, start, row), (Part::LoopEnd, end, row)];
    parts.push((Part::StartMarker, shape.offset, numbers));
    if let Some(end) = shape.end_marker() {
        parts.push((Part::EndMarker, end, numbers));
    }
    for (part, beat, band) in parts {
        let x = axis.x_of(beat);
        let rect =
            Rect::from_min_max(Pos2::new(x - GRAB, band.top()), Pos2::new(x + GRAB, band.bottom())).intersect(ruler);
        if !rect.is_positive() {
            continue;
        }
        let label = format!("{} at beat {}", part.label(), round(beat));
        let response = ui.interact(rect, Id::new(("piano-roll-ruler-part", part.label())), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, part.label()));
        response.on_hover_text(label);
    }
}

/// A marker at `x`: a line down the bar-number row and a small flag at its top pointing right
/// (`side` 1, the start marker) or left (−1, the end marker).
fn marker(painter: &egui::Painter, numbers: Rect, x: f32, side: f32, color: Color32) {
    let x = x.round() + 0.5;
    painter.vline(x, numbers.y_range(), Stroke::new(1.0, color));
    let top = numbers.top();
    painter.add(egui::Shape::convex_polygon(
        vec![Pos2::new(x, top), Pos2::new(x + 6.0 * side, top + 3.5), Pos2::new(x, top + 7.0)],
        color,
        Stroke::NONE,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(step: f64) -> TimelineGrid {
        TimelineGrid { step, bar_beats: 4.0, snaps: true }
    }

    const AXIS: Axis = Axis { origin: 100.0, beat_width: 20.0 };

    fn ruler() -> Rect {
        Rect::from_min_size(Pos2::new(100.0, 0.0), Vec2::new(400.0, 24.0))
    }

    #[test]
    fn a_press_grabs_the_nearest_handle_the_bar_or_a_marker() {
        let looping = ClipShape { offset: 1.0, length: 16.0, loop_range: Some((0.0, 4.0)), room: None };
        // The loop row: handles at x 100 and 180, the bar between.
        assert_eq!(part_at(&looping, &AXIS, ruler(), Pos2::new(101.0, 3.0)), Some(Part::LoopStart));
        assert_eq!(part_at(&looping, &AXIS, ruler(), Pos2::new(178.0, 3.0)), Some(Part::LoopEnd));
        assert_eq!(part_at(&looping, &AXIS, ruler(), Pos2::new(140.0, 3.0)), Some(Part::Loop));
        assert_eq!(part_at(&looping, &AXIS, ruler(), Pos2::new(300.0, 3.0)), None);
        // Under it: the start marker at beat 1 (x 120); no end marker while it loops.
        assert_eq!(part_at(&looping, &AXIS, ruler(), Pos2::new(122.0, 16.0)), Some(Part::StartMarker));
        assert_eq!(part_at(&looping, &AXIS, ruler(), Pos2::new(440.0, 16.0)), None);
        let plain = ClipShape { loop_range: None, ..looping };
        // Not looping: the end marker at beat 17 (x 440), and the bar where a loop would go.
        assert_eq!(part_at(&plain, &AXIS, ruler(), Pos2::new(438.0, 16.0)), Some(Part::EndMarker));
        assert_eq!(part_at(&plain, &AXIS, ruler(), Pos2::new(420.0, 3.0)), Some(Part::LoopEnd));
        // Both handles in reach of a tiny loop: the nearer wins.
        let tiny = ClipShape { loop_range: Some((1.0, 1.25)), ..looping };
        assert_eq!(part_at(&tiny, &AXIS, ruler(), Pos2::new(119.5, 3.0)), Some(Part::LoopStart));
        assert_eq!(part_at(&tiny, &AXIS, ruler(), Pos2::new(125.5, 3.0)), Some(Part::LoopEnd));
    }

    #[test]
    fn loop_handles_stop_on_a_drawn_line_when_they_meet() {
        // 3/4 with a two-beat grid: lines at 0, 2, 3, 5, 6. A loop from 3 to 6 keeps its start on
        // the line at 5, and a loop from 0 to 3 keeps its end on the line at 2.
        let grid = TimelineGrid { step: 2.0, bar_beats: 3.0, snaps: true };
        let late = ClipShape { offset: 0.0, length: 8.0, loop_range: Some((3.0, 6.0)), room: None };
        assert_eq!(dragged(&late, Part::LoopStart, 3.0, 7.0, grid, false).loop_range, Some((5.0, 6.0)));
        let early = ClipShape { offset: 0.0, length: 8.0, loop_range: Some((0.0, 3.0)), room: None };
        assert_eq!(dragged(&early, Part::LoopEnd, 3.0, -1.0, grid, false).loop_range, Some((0.0, 2.0)));
        let ends = ClipShape { offset: 3.0, length: 3.0, loop_range: None, room: None };
        assert_eq!(dragged(&ends, Part::EndMarker, 6.0, 0.0, grid, false).length, 2.0);
        assert_eq!(dragged(&ends, Part::StartMarker, 3.0, 9.0, grid, false).offset, 5.0);
        // Free, a hundredth of a beat.
        let free = dragged(&late, Part::LoopStart, 3.0, 7.0, grid, true).loop_range;
        assert!(free.is_some_and(|(start, _)| (start - (6.0 - MIN_LENGTH)).abs() < 1e-9));
    }

    #[test]
    fn loop_handles_snap_to_the_grid_unless_free_and_never_cross() {
        let shape = ClipShape { offset: 0.0, length: 8.0, loop_range: Some((0.0, 4.0)), room: None };
        let end = |now: f64, free: bool| dragged(&shape, Part::LoopEnd, 4.0, now, grid(1.0), free).loop_range;
        assert_eq!(end(5.4, false), Some((0.0, 5.0)));
        assert_eq!(end(5.4, true), Some((0.0, 5.4)));
        // Dragged onto or before the start: one step after it.
        assert_eq!(end(-3.0, false), Some((0.0, 1.0)));
        let start = |now: f64| dragged(&shape, Part::LoopStart, 0.0, now, grid(1.0), false).loop_range;
        assert_eq!(start(2.6), Some((3.0, 4.0)));
        assert_eq!(start(9.0), Some((3.0, 4.0)));
        assert_eq!(start(-2.0), Some((0.0, 4.0)));
        // The bar keeps its length and stays at 0 or later.
        let moved = |now: f64| dragged(&shape, Part::Loop, 1.0, now, grid(1.0), false).loop_range;
        assert_eq!(moved(3.2), Some((2.0, 6.0)));
        assert_eq!(moved(-5.0), Some((0.0, 4.0)));
        // A handle of a clip that does not loop yet makes a loop from the bar it shows.
        let plain = ClipShape { loop_range: None, ..shape };
        assert_eq!(dragged(&plain, Part::LoopEnd, 8.0, 6.1, grid(1.0), false).loop_range, Some((0.0, 6.0)));
    }

    #[test]
    fn markers_change_the_start_and_length_within_the_room() {
        let plain = ClipShape { offset: 0.0, length: 4.0, loop_range: None, room: Some(6.0) };
        let end = |now: f64, free: bool| dragged(&plain, Part::EndMarker, 4.0, now, grid(0.5), free).length;
        assert_eq!(end(5.2, false), 5.0);
        assert_eq!(end(5.2, true), 5.2);
        // Never past the next clip, never shorter than a step.
        assert_eq!(end(50.0, false), 6.0);
        assert_eq!(end(-1.0, false), 0.5);
        // The start marker keeps the end where it is.
        let later = dragged(&plain, Part::StartMarker, 0.0, 1.1, grid(0.5), false);
        assert_eq!((later.offset, later.length), (1.0, 3.0));
        let shifted = ClipShape { offset: 4.0, ..plain };
        // Earlier from 4 to 0 would make it 8 long: the room stops it at 6.
        let earlier = dragged(&shifted, Part::StartMarker, 4.0, 0.0, grid(0.5), false);
        assert_eq!((earlier.offset, earlier.length), (2.0, 6.0));
        // A looping clip keeps its length, and its start marker stays before the loop's end.
        let looping = ClipShape { loop_range: Some((0.0, 2.0)), ..plain };
        let start = dragged(&looping, Part::StartMarker, 0.0, 7.0, grid(0.5), false);
        assert_eq!((start.offset, start.length), (1.5, 4.0));
        assert_eq!(dragged(&looping, Part::EndMarker, 4.0, 7.0, grid(0.5), false), looping);
    }

    #[test]
    fn a_drag_becomes_one_command_and_an_unchanged_one_none() {
        let address = json!({ "track": 1, "index": 0 });
        let before = ClipShape { offset: 0.0, length: 4.0, loop_range: Some((0.0, 4.0)), room: None };
        assert!(command(&address, &before, &before, Part::LoopEnd).is_none());
        let after = ClipShape { loop_range: Some((1.0, 3.0)), ..before };
        let action = command(&address, &before, &after, Part::LoopEnd).unwrap();
        assert_eq!(action.id, "clip.setLoop");
        assert_eq!(action.params, json!({ "track": 1, "index": 0, "start": 1.0, "end": 3.0 }));
        let longer = ClipShape { length: 1.0 / 3.0 + 4.0, loop_range: None, ..before };
        let action = command(&address, &before, &longer, Part::EndMarker).unwrap();
        assert_eq!((action.id, action.params["length"].as_f64()), ("clip.setLength", Some(4.333333)));
        let moved = ClipShape { offset: 1.0, ..before };
        assert_eq!(command(&address, &before, &moved, Part::StartMarker).unwrap().id, "clip.setStart");
    }

    #[test]
    fn the_played_part_runs_from_the_start_marker_or_loop_start_to_the_end() {
        let plain = ClipShape { offset: 1.0, length: 4.0, loop_range: None, room: None };
        assert_eq!(plain.played(), (1.0, 5.0));
        assert_eq!(ClipShape { loop_range: Some((2.0, 3.0)), ..plain }.played(), (1.0, 3.0));
        assert_eq!(ClipShape { loop_range: Some((0.5, 3.0)), ..plain }.played(), (0.5, 3.0));
    }
}
