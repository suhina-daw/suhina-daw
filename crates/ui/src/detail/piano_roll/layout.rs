// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where things are in the piano roll: keys and beats to points and back, the grid, what is under the pointer, scroll and zoom.

use super::*;

/// Where things are this frame.
#[derive(Debug, Clone, Copy)]
pub(super) struct Layout {
    /// The scale strip left of the keys.
    pub(super) strip: Rect,
    pub(super) keys: Rect,
    pub(super) ruler: Rect,
    pub(super) grid: Rect,
    pub(super) lane: Rect,
    pub(super) scroll: Vec2,
    pub(super) beat_width: f32,
    pub(super) key_height: f32,
    /// The key rows shown, from the top: every key, or the keys a fold leaves (› Scale and key).
    pub(super) rows: KeyRows,
}

impl Layout {
    pub(super) fn x_of(&self, beat: f64) -> f32 {
        self.axis().x_of(beat)
    }

    pub(super) fn beat_at(&self, x: f32) -> f64 {
        self.axis().beat_at(x)
    }

    /// The time axis of the grid, after its scroll.
    pub(super) fn axis(&self) -> crate::kit::time_axis::TimeViewport {
        crate::kit::time_axis::TimeViewport::scrolled(self.grid.left(), self.scroll.x, self.beat_width)
    }

    /// The top of `key`'s row; far above the grid for a key the fold hides, so nothing on it is
    /// drawn or hit.
    pub(super) fn y_of(&self, key: u8) -> f32 {
        match self.rows.row_of(key) {
            Some(row) => self.grid.top() - self.scroll.y + row as f32 * self.key_height,
            None => self.grid.top() - 1.0e6,
        }
    }

    /// The key whose row is at `y`, if any.
    pub(super) fn key_at(&self, y: f32) -> Option<u8> {
        let row = ((y - self.grid.top() + self.scroll.y) / self.key_height).floor();
        if !(row >= 0.0 && row < self.rows.len() as f32) {
            return None;
        }
        self.rows.key_of(row as usize)
    }

    pub(super) fn note_rect(&self, note: &Note) -> Rect {
        let left = self.x_of(note.time);
        let right = self.x_of(note.time + note.duration.max(0.0)).max(left + 2.0);
        let top = self.y_of(note.key);
        Rect::from_min_max(Pos2::new(left, top + 1.0), Pos2::new(right - 1.0, top + self.key_height))
    }
}

/// The grid step chosen for `clip` at `beat_width` points per beat, before it is made coarser to
/// keep lines apart: the fixed value, or the adaptive grid's step at this zoom.
pub(super) fn chosen_step(clip: &PianoClip, beat_width: f32) -> f64 {
    if clip.grid_menu.is_adaptive() { clip.grid_menu.step(beat_width, clip.bar_beats) } else { clip.grid }
}

/// The grid `clip` is edited on at `beat_width`: snapping only while the menu's Snap to grid is on.
pub(crate) fn note_grid(clip: &PianoClip, beat_width: f32) -> TimelineGrid {
    let grid = TimelineGrid::new(chosen_step(clip, beat_width), beat_width, clip.bar_beats, MIN_LINE_SPACING);
    TimelineGrid { snaps: grid.snaps && clip.grid_menu.snap, ..grid }
}

/// The grid a grid key (Ctrl+1 to Ctrl+4 by default) makes of `clip`'s.
pub(super) fn grid_key(action: &str, clip: &PianoClip) -> Option<PianoGrid> {
    let grid = clip.grid_menu;
    let picked = match action {
        "pianoRoll.gridNarrow" => grid.narrower(),
        "pianoRoll.gridWiden" => grid.wider(),
        "pianoRoll.gridTriplet" => grid.toggle_triplet(),
        "pianoRoll.gridSnap" => grid.toggle_snap(),
        _ => return None,
    };
    Some(picked)
}

/// The grid line nearest to `beat`, within the piano roll's beats: what a slice snaps its cut
/// to and a Shift+Draw stroke ends on (`beat` itself while the grid does not snap).
pub(super) fn nearest_line(grid: TimelineGrid, beat: f64) -> f64 {
    grid.place(beat, false, false).clamp(0.0, f64::from(BEAT_LIMIT))
}

/// How far a drag of `raw` beats and `raw_keys` semitones moves the selection: by whole grid
/// steps unless `free`, and never past the content's start or the keys 0 to 127.
pub fn move_delta(raw: f64, raw_keys: i32, moved: &[Note], grid: TimelineGrid, free: bool) -> (f64, i32) {
    let mut beats = if free || !grid.snaps { raw } else { (raw / grid.step).round() * grid.step };
    let earliest = moved.iter().map(|n| n.time).fold(f64::INFINITY, f64::min);
    if earliest.is_finite() {
        beats = beats.max(-earliest);
    }
    let lowest = moved.iter().map(|n| i32::from(n.key)).min().unwrap_or(0);
    let highest = moved.iter().map(|n| i32::from(n.key)).max().unwrap_or(127);
    let keys = raw_keys.clamp(-lowest, 127 - highest);
    (if beats.is_finite() { beats } else { 0.0 }, keys)
}

/// How far an arrow key moves `moved`: the first of them to the next grid line, or with `earlier`
/// the one before, and the rest with it, so notes off the grid come onto it and then go a line at a
/// time. A step when the grid does not snap.
pub fn key_step(moved: &[Note], grid: TimelineGrid, earlier: bool) -> f64 {
    let first = moved.iter().map(|n| n.time).fold(f64::INFINITY, f64::min);
    if !grid.snaps || !first.is_finite() {
        return if earlier { -grid.step } else { grid.step };
    }
    if earlier { grid.previous_line(first) - first } else { grid.next_line(first) - first }
}

/// What a selected note is filled with: `accent`, or `plain` where a note of `color` would look much
/// the same in the accent, so a selected note always stands out from the others.
pub fn selected_note_color(color: Color32, accent: Color32, plain: Color32) -> Color32 {
    let [r, g, b, _] = color.to_array();
    let [ar, ag, ab, _] = accent.to_array();
    let apart = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
    if apart(r, ar) + apart(g, ag) + apart(b, ab) < NEAR_COLOR * NEAR_COLOR { plain } else { accent }
}

/// How far apart two colours are at least, in steps of red, green and blue, to tell them apart at a glance.
const NEAR_COLOR: i32 = 120;

/// How much a drag of the grabbed note's right end by `raw` beats changes the selection's
/// lengths: the end on the grid unless `free`, and no note shorter than [`MIN_LENGTH`].
pub fn resize_by(raw: f64, grabbed: &Note, resized: &[Note], grid: TimelineGrid, free: bool) -> f64 {
    let end = grabbed.time + grabbed.duration;
    let mut new_end = end + magnetic_by(end, raw, grid, free);
    if grid.snaps && !free && new_end <= grabbed.time + EPSILON {
        // Snapped onto or before its start: the end goes to the first line after the start.
        new_end = grid.next_line(grabbed.time);
    }
    let shortest = resized.iter().map(|n| n.duration).fold(f64::INFINITY, f64::min);
    let by = new_end - end;
    let by = if shortest.is_finite() { by.max(MIN_LENGTH - shortest) } else { by };
    if by.is_finite() { by } else { 0.0 }
}

/// How far a drag of `raw` beats takes a start or an end at `from`, the grid snapping
/// magnetically (› Moving, › Resizing): one on the grid goes to the grid line nearest to where
/// the pointer is. One off the grid (played in, or humanised) moves freely until it reaches the
/// first grid line, and from there snaps to whichever is nearer: a grid line, or its own offset
/// from the grid a whole number of steps away; so it can be put back on the grid by dragging, or
/// moved keeping its feel. `raw` itself with `free` (Alt held) or a grid that does not snap. The
/// one rule for a note's move, its copy and the drags of its start and its end.
pub(crate) fn magnetic_by(from: f64, raw: f64, grid: TimelineGrid, free: bool) -> f64 {
    if free || !grid.snaps || !raw.is_finite() || !from.is_finite() || raw == 0.0 {
        return raw;
    }
    let target = from + raw;
    let line = grid.place(target, false, false) - from;
    if (grid.place(from, false, false) - from).abs() < ON_GRID {
        return line;
    }
    let first = if raw > 0.0 { grid.next_line(from) } else { grid.previous_line(from) };
    let reached = if raw > 0.0 { target >= first } else { target <= first };
    if !reached {
        return raw;
    }
    let offset = (raw / grid.step).round() * grid.step;
    if (line - raw).abs() <= (offset - raw).abs() { line } else { offset }
}

/// How close to a grid line a start or an end counts as on it, in beats.
const ON_GRID: f64 = 1e-6;

/// The note at `pos`, the topmost (latest) first, and which part of it `pos` is on.
pub(super) fn note_at(layout: &Layout, notes: &[Note], pos: Pos2) -> Option<(usize, Part)> {
    notes.iter().enumerate().rev().find_map(|(i, n)| {
        let rect = layout.note_rect(n).expand2(Vec2::new(0.0, 0.5));
        rect.contains(pos).then(|| (i, Part::of(rect, pos.x)))
    })
}

/// The velocity bar under `pos`: a note's bar can be grabbed anywhere over the note's width in
/// the lane (and a little before its start, where the stem is drawn), the nearest start first,
/// a selected note before the others.
pub(super) fn stem_at(layout: &Layout, notes: &[Note], selected: &BTreeSet<usize>, pos: Pos2) -> Option<usize> {
    let near = |(i, n): &(usize, &Note)| {
        let start = layout.x_of(n.time);
        let end = layout.x_of(n.time + n.duration.max(0.0)).max(start + STEM_REACH);
        (pos.x >= start - STEM_REACH && pos.x <= end).then_some((*i, (start - pos.x).abs()))
    };
    let candidates: Vec<(usize, f32)> = notes.iter().enumerate().filter_map(|e| near(&e)).collect();
    let best = |list: &mut dyn Iterator<Item = (usize, f32)>| list.min_by(|a, b| a.1.total_cmp(&b.1)).map(|c| c.0);
    best(&mut candidates.iter().copied().filter(|(i, _)| selected.contains(i)))
        .or_else(|| best(&mut candidates.iter().copied()))
}

/// The velocity, 1 to 127, at height `y` in the velocity lane: the inverse of [`velocity_height`].
pub(super) fn velocity_at(lane: Rect, y: f32) -> f64 {
    let height = (lane.height() - 4.0).max(1.0);
    let from_bottom = lane.bottom() - 2.0 - y;
    (f64::from(from_bottom / height) * 127.0).round().clamp(1.0, 127.0)
}

pub(super) fn velocity_height(velocity: f32, lane: Rect) -> f32 {
    let v = f32::from(suhina_session::midi_velocity(velocity));
    (lane.height() - 4.0).max(0.0) * v / 127.0
}

/// The cursor a tool shows over the grid: the arrow for Select, and none for the tools whose
/// icon is drawn in its place ([`tool_pointer`]).
pub(super) fn cursor_for(tool: Tool) -> Option<CursorIcon> {
    match tool {
        Tool::Select => None,
        Tool::Draw | Tool::Paint | Tool::Erase | Tool::Slice | Tool::Mute => Some(CursorIcon::None),
    }
}

/// The toolbar icon that stands in for the pointer while a tool is chosen, and its hotspot: the
/// point of the icon that sits on the pointer, as fractions of its square from the top left (the
/// pencil's and the brush's tip, the eraser's rubbing edge, the blade's point, the middle of the
/// mute sign). The windowing layer has no such cursors, so the pointer is hidden and the icon
/// drawn where it is.
pub(super) fn tool_pointer(tool: Tool) -> Option<(IconButton, Vec2)> {
    match tool {
        Tool::Draw => Some((IconButton::Draw, Vec2::new(0.013, 0.948))),
        Tool::Paint => Some((IconButton::Paint, Vec2::new(0.045, 0.913))),
        Tool::Erase => Some((IconButton::Erase, Vec2::new(0.153, 0.847))),
        Tool::Slice => Some((IconButton::Slice, Vec2::new(0.164, 0.093))),
        Tool::Mute => Some((IconButton::Mute, Vec2::splat(0.5))),
        Tool::Select => None,
    }
}

/// The square a tool's icon is drawn in for a pointer at `pos`: `size` wide, its hotspot on `pos`.
pub(super) fn tool_pointer_rect(pos: Pos2, hotspot: Vec2, size: f32) -> Rect {
    Rect::from_min_size(pos - hotspot * size, Vec2::splat(size))
}

/// The theme's own artwork for a tool's toolbar button (`piano.draw` and the others), the role the
/// toolbar paints it with, so the pointer changes with the toolbar's icon.
pub(super) fn tool_role(icon: IconButton) -> String {
    format!("piano.{}", icon.id().trim_start_matches("piano-"))
}

/// A tool's icon in place of the pointer, in `rect`: the toolbar's icon (the theme's when it has
/// one, else the built-in artwork) in the text colour, ringed with a point of the window's colour
/// so it reads over a note as well as over the empty grid.
pub(super) fn draw_tool_pointer(painter: &egui::Painter, icon: IconButton, rect: Rect) {
    let role = tool_role(icon);
    if crate::kit::skin::has_own(&role) {
        let normal = crate::kit::skin::State::Normal;
        for (x, y) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            let ring = rect.translate(Vec2::new(x, y));
            crate::kit::skin::paint(painter, ring, &role, normal, theme::window_background(), 0.0);
        }
        if crate::kit::skin::paint(painter, rect, &role, normal, theme::text_primary(), 0.0) {
            return;
        }
    }
    if let Some(svg) = icon.svg() {
        for (x, y) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            crate::kit::skin::paint_icon(painter, rect.translate(Vec2::new(x, y)), svg, theme::window_background());
        }
        if crate::kit::skin::paint_icon(painter, rect, svg, theme::text_primary()) {
            return;
        }
    }
    icon.draw(painter, rect, theme::text_primary(), rect.width() / TOOL_ICON);
}

/// Beats rounded to a millionth, so a value made of steps such as thirds sends no float noise.
pub(super) fn round_beat(beat: f64) -> f64 {
    (beat * 1e6).round() / 1e6
}

/// How tall the velocity lane is with `room` under the ruler: 72 px, at most 30 % of the room
/// (`wanted`, the height its top edge was dragged to, instead of both), never so tall that fewer
/// than five key rows of `key_height` are left, and left out when that leaves it under 12 px
/// (› Layout in `docs/spec/ui-clip-editor.md`).
pub(super) fn velocity_lane_height(room: f32, key_height: f32, wanted: Option<f32>) -> f32 {
    // At most the default row's height: tall rows are a zoom, and the lane stays through it.
    let most = room - LEAST_KEY_ROWS * key_height.min(DEFAULT_KEY_HEIGHT);
    let lane = wanted.unwrap_or(VELOCITY_LANE.min(room * 0.3)).min(most);
    if lane.is_finite() && lane >= LEAST_LANE { lane } else { 0.0 }
}

/// A newly shown clip: the whole clip fits the `grid`'s width, and its notes are in the middle.
pub(super) fn fit(state: &mut PianoRollState, grid: Rect, clip: &PianoClip, rows: &KeyRows) {
    let width = grid.width().max(1.0);
    state.beat_width = clamp_beat_width(width / clip.length.max(1.0) as f32, &PIANO_ROLL_BEAT_WIDTHS);
    state.scroll.x = 0.0;
    state.scrolled_by_hand = false;
    centre_notes(state, grid.height(), clip, rows);
}

/// Scrolls the key rows so the middle of the clip's lowest and highest note (C3 for an empty
/// clip) is in the middle of a grid `height` tall, counting the rows `rows` shows.
pub(super) fn centre_notes(state: &mut PianoRollState, height: f32, clip: &PianoClip, rows: &KeyRows) {
    let (low, high) = clip.notes.iter().fold((u8::MAX, 0u8), |(lo, hi), n| (lo.min(n.key), hi.max(n.key)));
    let middle = if low <= high { rows.middle_row(low, high) } else { rows.middle_row(60, 60) };
    let height = height.max(1.0);
    state.scroll.y = middle * state.key_height + state.key_height / 2.0 - height / 2.0;
    state.centred_for = height;
}

/// The wheel scrolls, with Ctrl it zooms time and with Ctrl+Shift or Alt key height, around
/// the pointer; the middle button pans. While the right button is held over the grid, a wheel
/// step chooses the next or the previous tool instead, and the toolbar and the cursor follow.
pub(super) fn scroll_and_zoom(ui: &Ui, state: &mut PianoRollState, area: Rect, grid: Rect, inputs: Inputs) {
    let (before, key_height) = (state.scroll.y, state.key_height);
    scroll_and_zoom_by_hand(ui, state, area, grid, inputs);
    if state.scroll.y != before || state.key_height != key_height {
        state.scrolled_by_hand = true;
    }
}

pub(super) fn scroll_and_zoom_by_hand(ui: &Ui, state: &mut PianoRollState, area: Rect, grid: Rect, inputs: Inputs) {
    let pointer = widgets::pointer_over(ui, area);
    let (zoom, wheel) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta()));
    // The right button and the wheel: the next tool for a step one way, the previous for the
    // other. Held over the grid only, and the step scrolls and zooms nothing.
    // One step per wheel event, its own direction, not the smoothed delta: that goes on for a few
    // frames after the wheel stops and would scroll once the button is up.
    let wheel_steps: Vec<f32> = ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::MouseWheel { delta, .. } => Some(if delta.y == 0.0 { delta.x } else { delta.y }),
                _ => None,
            })
            .filter(|step| *step != 0.0)
            .collect()
    });
    if inputs.right && (state.wheel_tools || pointer.is_some_and(|p| grid.contains(p))) {
        state.wheel_tools = true;
        for step in wheel_steps {
            state.right_stepped = true;
            state.tool = step_tool(state.tool, if step > 0.0 { 1 } else { -1 });
        }
        if let Some(cursor) = cursor_for(state.tool) {
            ui.ctx().set_cursor_icon(cursor);
        }
        return;
    }
    if state.wheel_tools {
        // The button is up: the wheel scrolls again once its smoothing has run out.
        if wheel != Vec2::ZERO || zoom != 1.0 {
            return;
        }
        state.wheel_tools = false;
    }
    if let Some(pointer) = pointer {
        let zoom_keys = |state: &mut PianoRollState, factor: f32| {
            let key = (pointer.y - grid.top() + state.scroll.y) / state.key_height;
            state.key_height = (state.key_height * factor).clamp(KEY_HEIGHT_RANGE.0, KEY_HEIGHT_RANGE.1);
            state.scroll.y = key * state.key_height - (pointer.y - grid.top());
        };
        if inputs.alt && zoom == 1.0 {
            // Alt (Option on macOS) and the wheel; some systems turn it into a sideways scroll,
            // so either axis zooms.
            let steps = if wheel.y == 0.0 { wheel.x } else { wheel.y };
            if steps != 0.0 {
                zoom_keys(state, (steps * KEY_ZOOM_PER_POINT).exp());
            }
        } else if zoom != 1.0 {
            if inputs.shift {
                zoom_keys(state, zoom);
            } else {
                let beat = (pointer.x - grid.left() + state.scroll.x) / state.beat_width;
                state.beat_width = clamp_beat_width(state.beat_width * zoom, &PIANO_ROLL_BEAT_WIDTHS);
                state.scroll.x = beat * state.beat_width - (pointer.x - grid.left());
                state.zoom_reach = f64::from((state.scroll.x + grid.width()) / state.beat_width);
            }
        } else {
            state.scroll -= wheel;
        }
    }
    let (middle_pressed, middle_down, delta) = ui.input(|i| {
        (
            i.pointer.button_pressed(PointerButton::Middle),
            i.pointer.button_down(PointerButton::Middle),
            i.pointer.delta(),
        )
    });
    if middle_pressed {
        state.panning = widgets::pointer_over(ui, grid).is_some();
    }
    if !middle_down {
        state.panning = false;
    }
    if state.panning {
        state.scroll -= delta;
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
}

/// How far from the grid's edge the selected notes are kept, in points.
const REVEAL_MARGIN: f32 = 24.0;

/// Scrolls so the selected notes of `clip` are in `grid`, once a key has moved or sized them
/// ([`PianoRollState::reveal_selection`]): the least scroll that brings their span in, with a little
/// room; a span wider or taller than the grid shows its start or its top row.
pub(super) fn reveal_selection(state: &mut PianoRollState, clip: &PianoClip, rows: &KeyRows, grid: Rect) {
    if state.reveal_selection == 0 {
        return;
    }
    state.reveal_selection -= 1;
    let chosen: Vec<&Note> = state.selected.iter().filter_map(|i| clip.notes.get(*i)).collect();
    if chosen.is_empty() {
        return;
    }
    let start = chosen.iter().map(|n| n.time).fold(f64::INFINITY, f64::min) as f32 * state.beat_width;
    let end = chosen.iter().map(|n| n.time + n.duration.max(0.0)).fold(0.0, f64::max) as f32 * state.beat_width;
    state.scroll.x = shown_span(state.scroll.x, grid.width(), start, end);
    let shown_rows: Vec<usize> = chosen.iter().filter_map(|n| rows.row_of(n.key)).collect();
    if let (Some(top), Some(bottom)) = (shown_rows.iter().min(), shown_rows.iter().max()) {
        let (top, bottom) = (*top as f32 * state.key_height, (*bottom + 1) as f32 * state.key_height);
        state.scroll.y = shown_span(state.scroll.y, grid.height(), top, bottom);
        state.scrolled_by_hand = true;
    }
}

/// The scroll that shows `from` to `to` (points along the content) in a view `size` long now at
/// `scroll`: unchanged when it is in view, else the least move, keeping [`REVEAL_MARGIN`] where
/// there is room; a span longer than the view shows its start.
fn shown_span(scroll: f32, size: f32, from: f32, to: f32) -> f32 {
    let margin = REVEAL_MARGIN.min(((size - (to - from)) / 2.0).max(0.0));
    if to - from + 2.0 * margin > size || from - margin < scroll {
        (from - margin).max(0.0)
    } else if to + margin > scroll + size {
        to + margin - size
    } else {
        scroll
    }
}

/// While a drag that moves, sizes or draws notes holds the pointer at or past the grid's edge, the
/// view scrolls that way, faster the further past it is, and the drag's start moves with it so
/// the notes go on following the pointer.
pub(super) fn scroll_at_edges(state: &mut PianoRollState, grid: Rect, pointer: Pos2) {
    let Some(gesture) = state.gesture.as_mut().filter(|g| g.follows_edges()) else { return };
    let edge = 4.0;
    let speed = |past: f32| (past.max(0.0) / 2.0 + 2.0).min(40.0);
    let mut by = Vec2::ZERO;
    if pointer.x > grid.right() - edge {
        by.x = speed(pointer.x - grid.right());
    } else if pointer.x < grid.left() + edge && state.scroll.x > 0.0 {
        by.x = -speed(grid.left() - pointer.x).min(state.scroll.x);
    }
    if pointer.y > grid.bottom() - edge {
        by.y = speed(pointer.y - grid.bottom());
    } else if pointer.y < grid.top() + edge && state.scroll.y > 0.0 {
        by.y = -speed(grid.top() - pointer.y).min(state.scroll.y);
    }
    if by == Vec2::ZERO {
        return;
    }
    gesture.scrolled(by);
    state.scroll += by;
    state.scrolled_by_hand = true;
}
