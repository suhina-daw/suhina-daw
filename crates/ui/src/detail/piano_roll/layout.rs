// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where things are in the piano roll: keys and beats to points and back, the grid, what is under the pointer, scroll and zoom.

use super::*;

/// Where things are this frame.
#[derive(Debug, Clone, Copy)]
pub(super) struct Layout {
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
        self.grid.left() - self.scroll.x + beat as f32 * self.beat_width
    }

    pub(super) fn beat_at(&self, x: f32) -> f64 {
        f64::from((x - self.grid.left() + self.scroll.x) / self.beat_width)
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

/// The grid a grid key (Ctrl+1 to Ctrl+4 by default) makes of `clip`'s; `None` when the grid
/// follows the control bar and it has no such value.
pub(super) fn grid_key(action: &str, clip: &PianoClip) -> Option<PianoGrid> {
    let grid = clip.grid_menu;
    let picked = match action {
        "pianoRoll.gridNarrow" => grid.narrower(),
        "pianoRoll.gridWiden" => grid.wider(),
        "pianoRoll.gridTriplet" => grid.toggle_triplet(),
        "pianoRoll.gridSnap" => grid.toggle_snap(),
        _ => return None,
    };
    (!clip.grid_follows || picked.control_bar_value().is_some()).then_some(picked)
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

/// How much a drag of the grabbed note's right end by `raw` beats changes the selection's
/// lengths: the end on the grid unless `free`, and no note shorter than [`MIN_LENGTH`].
pub fn resize_by(raw: f64, grabbed: &Note, resized: &[Note], grid: TimelineGrid, free: bool) -> f64 {
    let end = grabbed.time + grabbed.duration;
    let mut new_end = grid.place(end + raw, free, false);
    if grid.snaps && !free && new_end <= grabbed.time + EPSILON {
        // Snapped onto or before its start: the end goes to the first line after the start.
        new_end = grid.next_line(grabbed.time);
    }
    let shortest = resized.iter().map(|n| n.duration).fold(f64::INFINITY, f64::min);
    let by = new_end - end;
    let by = if shortest.is_finite() { by.max(MIN_LENGTH - shortest) } else { by };
    if by.is_finite() { by } else { 0.0 }
}

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

/// The cursor a tool shows over the grid. Erase hides the pointer and draws the eraser button's
/// shape in its place (`show`), since the windowing layer has no eraser cursor.
pub(super) fn cursor_for(tool: Tool) -> Option<CursorIcon> {
    match tool {
        Tool::Select | Tool::Mute => None,
        Tool::Draw | Tool::Paint => Some(CursorIcon::Crosshair),
        Tool::Erase => Some(CursorIcon::None),
        Tool::Slice => Some(CursorIcon::Text),
    }
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
    let most = room - LEAST_KEY_ROWS * key_height;
    let lane = wanted.unwrap_or(VELOCITY_LANE.min(room * 0.3)).min(most);
    if lane.is_finite() && lane >= LEAST_LANE { lane } else { 0.0 }
}

/// A newly shown clip: the whole clip fits the `grid`'s width, and its notes are in the middle.
pub(super) fn fit(state: &mut PianoRollState, grid: Rect, clip: &PianoClip, rows: &KeyRows) {
    let width = grid.width().max(1.0);
    state.beat_width = (width / clip.length.max(1.0) as f32).clamp(BEAT_WIDTH_RANGE.0, BEAT_WIDTH_RANGE.1);
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
                state.beat_width = (state.beat_width * zoom).clamp(BEAT_WIDTH_RANGE.0, BEAT_WIDTH_RANGE.1);
                state.scroll.x = beat * state.beat_width - (pointer.x - grid.left());
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
