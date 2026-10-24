// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll: the MIDI clip editor in the detail area (`docs/spec/ui-clip-editor.md`). A key
//! column, a ruler, the note grid and a velocity lane. Every edit is a `note.*` command; the view
//! keeps only what is on screen: zoom, scroll, the selected notes and a gesture in progress.

use std::collections::BTreeSet;

use egui::{Align2, Color32, CursorIcon, Id, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use serde_json::{Value, json};
use suhina_engine::Quantization;
use suhina_engine::model::{Clip, ClipId, LaneData, Note, Project, Scale, TrackId};

use crate::detail::midi_tools::ToolSettings;
use crate::detail::piano_roll_ghosts::{self as ghost_view, GhostSource, Ghosts};
use crate::detail::piano_roll_grid::{FIXED_VALUES, GridMode, PianoGrid, Width};
use crate::detail::piano_roll_ruler::{self as ruler_view, ClipShape, RulerState};
use crate::detail::piano_roll_scale::{self as scale_view, Fold, KeyRows};
use crate::interaction::grid::default_track_colour;
use crate::interaction::timeline_grid::TimelineGrid;
use crate::kit::theme;
use crate::kit::widgets::{self, Action};
use crate::settings::ToolbarSize;
use crate::settings::workflow::Workflow;

/// The gestures of [#351](https://github.com/suhina-daw/suhina-daw/issues/351): the left edge,
/// copying by dragging, velocity on a note and as a line, the lane's height and the audition.
#[path = "piano_roll_gestures.rs"]
mod gestures;
use gestures::Part;

mod clip;
mod drawing;
mod edits;
mod layout;
mod state;
mod toolbar;

pub use self::clip::*;
use self::drawing::*;
use self::edits::*;
pub use self::layout::*;
pub use self::state::*;
pub(crate) use self::toolbar::*;

pub use gestures::start_by;

/// Height of the toolbar, width of the key column, height of the ruler and the default height of
/// the velocity lane.
const TOOLBAR: f32 = 22.0;
const KEYS: f32 = 36.0;
const RULER: f32 = 24.0;
const VELOCITY_LANE: f32 = 72.0;
/// The velocity lane is left out under this height, and leaves at least this many key rows.
const LEAST_LANE: f32 = 12.0;
const LEAST_KEY_ROWS: f32 = 5.0;
/// The least height of the detail area while it shows the piano roll: the toolbar, the ruler and
/// five key rows at the default row height, and beside them the clip column's title and its five
/// rows (`docs/spec/ui-clip-editor.md` › Least height and closing).
pub const LEAST_HEIGHT: f32 = 120.0;
/// The toolbar's icon size and the space between its buttons, in points.
const TOOL_ICON: f32 = 14.0;
const TOOL_GAP: f32 = 3.0;
/// Key row heights and points per beat: the default and the zoom ranges.
const DEFAULT_KEY_HEIGHT: f32 = 12.0;
const KEY_HEIGHT_RANGE: (f32, f32) = (6.0, 32.0);
const BEAT_WIDTH_RANGE: (f32, f32) = (4.0, 400.0);
/// How much one point of Alt and the wheel scales key rows.
const KEY_ZOOM_PER_POINT: f32 = 0.004;
/// Grid lines closer than this, in points, are thinned out.
const MIN_LINE_SPACING: f32 = 6.0;
/// The most grid lines drawn in a frame, so a bad zoom can never draw without end.
const MAX_LINES: usize = 4000;
/// The most notes one command takes, so a stroke never builds one it cannot run.
const MAX_NOTES_PER_COMMAND: usize = suhina_session::MAX_NOTES_PER_COMMAND;
/// How close to a note's start or end, in points, the pointer resizes it instead of moving it.
const EDGE: f32 = 5.0;
/// How close to a velocity stem, in points, the pointer grabs it.
const STEM_REACH: f32 = 4.0;
/// The shortest note a resize leaves, in beats.
const MIN_LENGTH: f64 = 0.01;
/// The length of a note drawn without a grid, in beats, and its velocity.
const FREE_LENGTH: f64 = 0.25;
const NEW_VELOCITY: u8 = 100;
const EPSILON: f64 = 1e-9;
/// Two clicks this close in time (seconds) and space (points) are a double-click
/// (› Double-click in `docs/spec/ui-clip-editor.md`).
const DOUBLE_CLICK_TIME: f64 = 0.3;
const DOUBLE_CLICK_DISTANCE: f32 = 6.0;

const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// The name of MIDI key `key`, with 60 as C3.
pub fn key_name(key: u8) -> String {
    let name = NAMES.get(usize::from(key % 12)).copied().unwrap_or("?");
    format!("{name}{}", i32::from(key / 12) - 2)
}

fn is_black(key: u8) -> bool {
    matches!(key % 12, 1 | 3 | 6 | 8 | 10)
}

/// The modifiers and buttons this frame, read once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Inputs {
    shift: bool,
    alt: bool,
    command: bool,
    /// The right mouse button is held: with the wheel it chooses the tools.
    right: bool,
}

/// What gestures in one clip work with: where things are, the clip and its grid.
struct Gestures<'a> {
    layout: &'a Layout,
    clip: &'a PianoClip,
    grid: TimelineGrid,
}

impl Gestures<'_> {
    /// `note.delete` for one note, clearing the selection as the erase click does.
    fn delete_note(&self, state: &mut PianoRollState, actions: &mut Vec<Action>, index: usize) {
        state.selected.clear();
        let mut params = self.clip.address.clone();
        params["notes"] = json!([index]);
        actions.push(Action::new("note.delete", params));
    }

    /// Adds one note where `pos` is, `note.add` for it, and selects it.
    fn add_note(&self, state: &mut PianoRollState, actions: &mut Vec<Action>, pos: Pos2, free: bool) {
        if let Some(note) = state.new_note(self.layout, self.clip, self.grid, pos, free) {
            actions.push(Action::new("note.add", add_params(self.clip, &[note])));
            // `note.add` appends: the new note is the next index.
            state.selected = BTreeSet::from([self.clip.notes.len()]);
            state.audition_keys([note.key]);
        }
    }

    /// The notes a Shift+Draw stroke has drawn so far, one step on from the last, up to where
    /// the pointer is: the first starts where the press was, in the row it was in, and each is as
    /// long as the note last drawn or clicked. A stroke adds at most as many notes as one command
    /// takes, and never past the furthest beat.
    fn draw_run(&self, state: &mut PianoRollState, now: Pos2, free: bool) {
        let Some(first) = state.stroke.first().copied() else { return };
        let length = if first.duration > 0.0 { first.duration } else { self.grid.step.max(MIN_LENGTH) };
        let end = if free {
            self.layout.beat_at(now.x).max(0.0)
        } else {
            nearest_line(self.grid, self.layout.beat_at(now.x))
        };
        let mut time = first.time;
        while time + length <= end + EPSILON {
            let taken = state.stroke.iter().any(|n| (n.time - time).abs() < EPSILON)
                || self.clip.notes.iter().any(|n| n.key == first.key && (n.time - time).abs() < EPSILON);
            if !taken && state.stroke.len() < MAX_NOTES_PER_COMMAND {
                state.stroke.push(Note { time, ..first });
            }
            time += length;
            if time > f64::from(BEAT_LIMIT) {
                break;
            }
        }
    }
}

/// Draws the piano roll for `clip` in `area` and adds the edits it makes, and the key column's
/// `preview.*` notes, to `actions`.
pub fn show(
    ui: &mut Ui,
    area: Rect,
    clip: &PianoClip,
    state: &mut PianoRollState,
    cx: Context<'_>,
    actions: &mut Vec<Action>,
) {
    let Context { options, keymap, copied, playhead } = cx;
    let scale = options.toolbar_size.scale();
    let fresh = state.shown.as_ref() != Some(&clip.address);
    if fresh {
        state.shown = Some(clip.address.clone());
        state.selected.clear();
        state.gesture = None;
        state.playing_key = None;
    }
    // Keys run before the piano roll is drawn, and their commands after it: notes a paste or a
    // duplicate selected this frame exist from the next frame on.
    if !std::mem::take(&mut state.selecting_new) {
        state.selected.retain(|i| *i < clip.notes.len());
    }
    // The toolbar along the top; the editor under it. The toolbar's height follows Settings ›
    // Clip editor › Piano roll › Toolbar size, and never takes more than the room the editor has.
    let band = (TOOLBAR * scale).min(area.height());
    let toolbar_rect = Rect::from_min_max(area.min, Pos2::new(area.right(), area.top() + band));
    let whole = area;
    let area = Rect::from_min_max(Pos2::new(area.left(), toolbar_rect.bottom()), area.max);
    let lane_height = velocity_lane_height(area.height() - RULER, state.key_height, state.lane_wanted);
    let keys = Rect::from_min_max(
        Pos2::new(area.left(), area.top() + RULER),
        Pos2::new(area.left() + KEYS, area.bottom() - lane_height),
    );
    let ruler = Rect::from_min_max(Pos2::new(keys.right(), area.top()), Pos2::new(area.right(), keys.top()));
    let grid_rect = Rect::from_min_max(Pos2::new(keys.right(), keys.top()), Pos2::new(area.right(), keys.bottom()));
    let lane = Rect::from_min_max(Pos2::new(keys.right(), keys.bottom()), area.max);
    toolbar(ui, toolbar_rect, ToolbarCx { clip, keymap, copied, scale }, state, actions);
    if grid_rect.width() <= 1.0 || grid_rect.height() <= 1.0 {
        return;
    }
    // A newly shown clip fits the width with its notes in the middle; until the rows are scrolled
    // by hand, a change in height centres the notes again, so they are in view at any height.
    let rows = scale_view::rows(clip, state.fold);
    if fresh {
        fit(state, grid_rect, clip, &rows);
    } else if !state.scrolled_by_hand && (grid_rect.height() - state.centred_for).abs() > 0.5 {
        centre_notes(state, grid_rect.height(), clip, &rows);
    }

    let inputs = Inputs {
        alt: ui.input(|i| i.modifiers.alt),
        shift: ui.input(|i| i.modifiers.shift),
        command: ui.input(|i| i.modifiers.command || i.modifiers.ctrl),
        right: ui.input(|i| i.pointer.button_down(PointerButton::Secondary)),
    };
    if ui.input(|i| i.pointer.any_pressed()) {
        state.focused = widgets::pointer_over(ui, whole).is_some();
    }
    // A fresh press of the right button: whether the last hold stepped the tools with the wheel
    // says nothing about this one.
    if ui.input(|i| i.pointer.button_pressed(PointerButton::Secondary)) {
        state.right_stepped = false;
    }
    scroll_and_zoom(ui, state, area, grid_rect, inputs);
    // The ruler: its loop handles and markers, and dragging it to zoom and scroll time.
    let shape = ruler_view::interact(ui, ruler, grid_rect.left(), clip, state, inputs.alt, actions);
    state.view_width = grid_rect.width();
    let rows_height = rows.len() as f32 * state.key_height;
    state.scroll.y = state.scroll.y.clamp(0.0, (rows_height - grid_rect.height()).max(0.0));
    let width = ((clip.length + clip.bar_beats * 4.0) as f32 * state.beat_width).max(grid_rect.width());
    state.scroll.x = state.scroll.x.clamp(0.0, (width - grid_rect.width()).max(0.0));
    let layout = Layout {
        keys,
        ruler,
        grid: grid_rect,
        lane,
        scroll: state.scroll,
        beat_width: state.beat_width,
        key_height: state.key_height,
        rows,
    };
    let grid = note_grid(clip, state.beat_width);
    let gesture = Gestures { layout: &layout, clip, grid };

    // The grid's gestures: one widget for the whole grid, so a note under the pointer and the
    // empty grid around it are told apart by where the press was.
    let response = ui.interact(grid_rect, Id::new("piano-roll-grid"), Sense::click_and_drag());
    let pointer = response.interact_pointer_pos().or_else(|| ui.ctx().pointer_hover_pos());
    let mut eraser = None;
    if let Some(pos) = ui.ctx().pointer_hover_pos().filter(|p| grid_rect.contains(*p)) {
        let over = note_at(&layout, &clip.notes, pos);
        let cursor = match (&state.gesture, over, state.tool) {
            (Some(Gesture::Resize { .. } | Gesture::ResizeStart { .. }), _, _) => Some(CursorIcon::ResizeHorizontal),
            (Some(Gesture::Move { copy: true, .. }), _, _) => Some(CursorIcon::Copy),
            (Some(Gesture::Move { .. }), _, _) => Some(CursorIcon::Grabbing),
            (Some(Gesture::NoteVelocity { .. }), _, _) => Some(CursorIcon::ResizeVertical),
            (Some(Gesture::Slice { .. }), _, _) | (None, _, Tool::Slice) => Some(CursorIcon::Text),
            // There is no eraser cursor in the windowing layer, so the pointer is hidden and the
            // eraser button's shape is drawn where the pointer is.
            (Some(Gesture::Erase), _, _) | (None, _, Tool::Erase) => {
                eraser = Some(pos);
                Some(CursorIcon::None)
            }
            (None, Some((_, Part::Start | Part::End)), _) => Some(CursorIcon::ResizeHorizontal),
            (None, Some((_, Part::Body)), Tool::Select | Tool::Draw) if ui.input(|i| i.modifiers.command) => {
                Some(CursorIcon::Copy)
            }
            (Some(Gesture::Draw { .. } | Gesture::Paint), _, _) | (None, None, Tool::Draw | Tool::Paint) => {
                Some(CursorIcon::Crosshair)
            }
            _ => None,
        };
        if let Some(cursor) = cursor {
            ui.ctx().set_cursor_icon(cursor);
        }
    }
    if std::env::var("SUHINA_TRACE_CLICK").is_ok() {
        eprintln!(
            "click? {} dbl {} pos {pointer:?} over {:?} tool {:?} selected {:?}",
            response.clicked(),
            response.double_clicked(),
            pointer.map(|p| note_at(&layout, &clip.notes, p)),
            state.tool,
            state.selected
        );
    }
    let workflow = options.workflow;
    if response.clicked()
        && let Some(pos) = pointer
    {
        // The piano roll counts its own double-clicks: two clicks close in time and space, and a
        // third starts a new pair (› Double-click).
        let time = ui.input(|i| i.time);
        let double = state
            .last_click
            .is_some_and(|(at, from)| time - at < DOUBLE_CLICK_TIME && from.distance(pos) < DOUBLE_CLICK_DISTANCE);
        state.last_click = if double { None } else { Some((time, pos)) };
        let added = state.click_added.take();
        let over = note_at(&layout, &clip.notes, pos);
        if double {
            second_click(&gesture, state, actions, (over.map(|(i, _)| i), added), pos, inputs, workflow);
        } else {
            single_click(&gesture, state, actions, over.map(|(i, _)| i), pos, inputs, workflow);
        }
    }
    // The right button: the Mouse-first switches first, then the Draw-first preset's delete in
    // Draw and Paint (› Keymap presets).
    let right_deletes = workflow.right_click_deletes(matches!(state.tool, Tool::Draw | Tool::Paint));
    if response.clicked_by(PointerButton::Secondary)
        && !state.right_stepped
        && !state.right_playing
        && !(workflow.alt_right_plays && inputs.alt)
        && let Some(pos) = pointer
        && let Some((index, _)) = note_at(&layout, &clip.notes, pos)
        && right_deletes
    {
        gesture.delete_note(state, actions, index);
    }
    // The right button: the Mouse-first switches, on the press, so a press that never moves (a
    // click) does the same job as a stroke (› Keymap presets).
    let right_pressed = ui.input(|i| i.pointer.button_pressed(PointerButton::Secondary))
        && widgets::pointer_over(ui, grid_rect).is_some();
    if right_pressed {
        let from = pointer.unwrap_or(grid_rect.center());
        state.erasing.clear();
        state.muting.clear();
        if workflow.alt_right_plays && inputs.alt {
            // Alt (Option) and the right button: the clip plays from the point under the pointer
            // and stops when the button is let go. It is the transport, not an edit.
            if let Some(at) = clip.placement.arrangement_beat(layout.beat_at(from.x)) {
                actions.push(Action::new("transport.setPosition", json!({ "beats": round_beat(at) })));
                actions.push(Action::new("transport.play", json!({})));
                state.right_playing = true;
            }
        } else if workflow.shift_right_splits && inputs.shift {
            state.gesture = Some(Gesture::Slice { from });
        } else if right_deletes {
            state.gesture = Some(Gesture::Erase);
            if let Some((index, _)) = note_at(&layout, &clip.notes, from) {
                state.erasing.insert(index);
            }
        }
    }
    if response.drag_started_by(PointerButton::Primary) {
        let from = ui.input(|i| i.pointer.press_origin()).or(pointer).unwrap_or(grid_rect.center());
        state.stroke.clear();
        state.erasing.clear();
        state.muting.clear();
        state.gesture = Some(match (note_at(&layout, &clip.notes, from), state.tool) {
            (_, Tool::Erase) => Gesture::Erase,
            (_, Tool::Slice) => Gesture::Slice { from },
            // Mute: the state the stroke gives every note it touches is the first note's, flipped.
            (Some((index, _)), Tool::Mute) => {
                state.mute_target = clip.notes.get(index).is_some_and(|note| !note.muted);
                state.muting.insert(index);
                Gesture::Mute
            }
            (None, Tool::Mute) => Gesture::Mute,
            // Shift and Draw repeat the last note's length along the row the drag started in:
            // the first note now, the rest as the drag reaches ([`Gestures::draw_run`]); the
            // whole stroke is one `note.add` when the drag ends.
            (None, Tool::Draw) if inputs.shift => {
                if let Some(note) = state.new_note(&layout, clip, grid, from, inputs.alt) {
                    state.stroke.push(note);
                }
                Gesture::DrawRun
            }
            (None, Tool::Draw) => Gesture::Draw { from },
            (_, Tool::Paint) => {
                state.stroke.extend(state.new_note(&layout, clip, grid, from, inputs.alt));
                Gesture::Paint
            }
            // Left click draws: with the Mouse-first switch on, the Select tool draws a note on an
            // empty spot the way Draw does (› Keymap presets).
            (None, Tool::Select) if workflow.empty_click_draws => Gesture::Draw { from },
            (Some((grabbed, part)), _) => {
                if !state.selected.contains(&grabbed) {
                    if !(inputs.shift || inputs.command) {
                        state.selected.clear();
                    }
                    state.selected.insert(grabbed);
                }
                // The ends resize; on the body Alt changes the velocity and Ctrl copies
                // (› Moving, › Resizing, › Velocity).
                match part {
                    Part::End => Gesture::Resize { from, grabbed },
                    Part::Start => Gesture::ResizeStart { from, grabbed },
                    Part::Body if inputs.alt => Gesture::NoteVelocity { from, grabbed },
                    Part::Body => Gesture::Move { from, grabbed, copy: inputs.command },
                }
            }
            (None, Tool::Select) => Gesture::Select { from, add: inputs.shift || inputs.command },
        });
    }

    // The velocity lane's gestures.
    let lane_response =
        (lane.height() > 4.0).then(|| ui.interact(lane, Id::new("piano-roll-velocity"), Sense::click_and_drag()));
    if let Some(lane_response) = &lane_response {
        let at = lane_response.interact_pointer_pos();
        if lane_response.clicked()
            && let Some(pos) = at
        {
            match stem_at(&layout, &clip.notes, &BTreeSet::new(), pos) {
                Some(index) if inputs.shift || inputs.command => {
                    if !state.selected.remove(&index) {
                        state.selected.insert(index);
                    }
                }
                Some(index) => state.selected = BTreeSet::from([index]),
                None => {}
            }
        }
        if lane_response.drag_started_by(PointerButton::Primary) {
            let from = ui.input(|i| i.pointer.press_origin()).or(at).unwrap_or(lane.center());
            // A drag that grabs no bar (or any drag with Alt) draws a line (› Velocity).
            let grabbed = if inputs.alt { None } else { stem_at(&layout, &clip.notes, &state.selected, from) };
            if let Some(grabbed) = grabbed {
                if !state.selected.contains(&grabbed) {
                    state.selected = BTreeSet::from([grabbed]);
                }
                state.gesture = Some(Gesture::Velocity { from, grabbed });
            } else {
                state.gesture = Some(Gesture::VelocityLine { from });
            }
        }
    }
    // The lane's top edge: dragging it makes the lane taller or shorter (› Layout).
    let room = area.height() - RULER;
    gestures::lane_edge(ui, &layout, room, state);

    // The key column: pressing a key plays it through the track's instrument while it is held,
    // and Ctrl+click or Ctrl+drag selects the notes of that pitch or those pitches.
    key_column(ui, &keys, &layout, clip, state, inputs, actions);

    // The drag's effect, shown while it lasts and made a command when it ends. The pointer
    // position is the interaction's own, so the frame a drag ends still has one.
    let now = ui.input(|i| i.pointer.latest_pos()).or_else(|| ui.ctx().pointer_hover_pos()).or(pointer);
    let dragging = response.dragged_by(PointerButton::Primary)
        || response.dragged_by(PointerButton::Secondary)
        || lane_response.as_ref().is_some_and(|r| r.dragged_by(PointerButton::Primary));
    let stopped = response.drag_stopped_by(PointerButton::Primary)
        || response.drag_stopped_by(PointerButton::Secondary)
        || lane_response.as_ref().is_some_and(|r| r.drag_stopped_by(PointerButton::Primary));
    if dragging && let Some(now) = now {
        match state.gesture {
            Some(Gesture::Paint) => paint(state, &layout, clip, now),
            Some(Gesture::DrawRun) => gesture.draw_run(state, now, inputs.alt),
            Some(Gesture::Erase) => {
                if let Some((index, _)) = note_at(&layout, &clip.notes, now) {
                    state.erasing.insert(index);
                }
            }
            Some(Gesture::Mute) => {
                if let Some((index, _)) = note_at(&layout, &clip.notes, now) {
                    if state.muting.is_empty() {
                        // The first note the stroke touches decides what the whole stroke does.
                        state.mute_target = clip.notes.get(index).is_some_and(|note| !note.muted);
                    }
                    state.muting.insert(index);
                }
            }
            _ => {}
        }
    }
    let chosen: Vec<Note> = state.selected.iter().filter_map(|i| clip.notes.get(*i).copied()).collect();
    let preview = match (state.gesture.as_ref(), now) {
        (Some(Gesture::Draw { from }), Some(now)) if dragging || stopped => {
            Some(Effect::Draw(drawn_note(state, &layout, clip, grid, *from, now, inputs.alt)))
        }
        (Some(Gesture::VelocityLine { from }), Some(now)) if dragging || stopped => {
            Some(gestures::velocity_line(&layout, &clip.notes, *from, now))
        }
        (Some(gesture), Some(now)) if dragging || stopped => {
            Some(effect(gesture, now, &layout, clip, &chosen, grid, inputs.alt))
        }
        _ => None,
    };
    // Letting the right button go stops the clip the Mouse-first switch was playing. It watches
    // the button itself, so a press that never moved stops it too.
    if ui.input(|i| i.pointer.button_released(PointerButton::Secondary)) && std::mem::take(&mut state.right_playing) {
        actions.push(Action::new("transport.stop", json!({})));
    }
    let velocity_label = match (&state.gesture, &preview) {
        (Some(Gesture::NoteVelocity { grabbed, .. }), Some(Effect::Velocity(by))) => Some((*grabbed, *by)),
        _ => None,
    };
    if stopped {
        if let (Some(effect), Some(gesture)) = (preview.clone(), state.gesture.take()) {
            commit(&gesture, effect, clip, state, actions, &layout);
        }
        state.gesture = None;
    } else if !dragging && !ui.input(|i| i.pointer.any_down()) {
        state.gesture = None;
        state.stroke.clear();
        state.erasing.clear();
        state.muting.clear();
    }

    let shape = shape.unwrap_or(clip.shape);
    draw(ui, &layout, clip, state, grid, preview, shape.played());
    if let Some((grabbed, by)) = velocity_label {
        gestures::draw_velocity_label(ui, &layout, clip, grabbed, by);
    }
    let axis = ruler_view::Axis { origin: grid_rect.left() - state.scroll.x, beat_width: state.beat_width };
    ruler_view::draw(ui, ruler, &axis, &shape);
    // The eraser under the pointer: the windowing layer has no eraser cursor, so the pointer is
    // hidden and the Erase button's shape is drawn where it is.
    if let Some(pos) = eraser {
        let painter = ui.painter_at(whole);
        IconButton::Erase.draw(
            &painter,
            Rect::from_center_size(pos, Vec2::splat(TOOL_ICON * scale)),
            theme::text_primary(),
            scale,
        );
    }
    // The playhead: where playback is in the clip's content (through its start, offset and loop),
    // over the ruler, the grid and the velocity lane, as the other views show it.
    if let Some(beat) = playhead.and_then(|at| clip.placement.content_beat(at)) {
        draw_playhead(ui, &layout, beat);
    }
    for_readers(ui, &layout, clip, state);
    scale_view::key_readers(ui, layout.keys, clip, &layout.rows, |key| layout.y_of(key), layout.key_height);
    // Notes the audition sounds go on now and off a moment later.
    let track = clip.address.get("track").and_then(Value::as_u64);
    state.audition_frame(ui, track, actions);
}

#[cfg(test)]
#[path = "piano_roll_tests.rs"]
mod tests;
