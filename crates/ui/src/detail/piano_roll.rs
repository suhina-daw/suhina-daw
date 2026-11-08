// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The piano roll: the MIDI clip editor in the detail area (`docs/spec/ui-clip-editor.md`). A key
//! column, a ruler, the note grid and a velocity lane. Every edit is a `note.*` command; the view
//! keeps only what is on screen: zoom, scroll, the selected notes and a gesture in progress.

use crate::kit::surface::Backgrounds as _;
use std::collections::BTreeSet;

use egui::{Align2, Color32, CursorIcon, Id, PointerButton, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use serde_json::{Value, json};
use suhina_engine::model::{Clip, ClipId, LaneData, Note, NoteId, Notes, Project, Scale, TrackId};

use crate::detail::detail_view::ClipRef;
use crate::detail::midi_tools::{ToolSettings, ToolsTab, Transform};
use crate::detail::piano_roll_ghosts::{self as ghost_view, GhostSource, Ghosts};
use crate::detail::piano_roll_grid::{FIXED_VALUES, GridMode, PianoGrid, Width};
use crate::detail::piano_roll_ruler::{self as ruler_view, ClipShape, RulerState};
use crate::detail::piano_roll_scale::{self as scale_view, Fold, KeyRows};
use crate::interaction::grid::track_colour;
use crate::interaction::timeline_grid::TimelineGrid;
use crate::kit::theme;
use crate::kit::time_axis::{PIANO_ROLL_BEAT_WIDTHS, clamp_beat_width};
use crate::kit::widgets::{self, Action};
use crate::settings::ToolbarSize;
use crate::settings::workflow::Workflow;

/// The gestures of [#351](https://github.com/suhina-daw/suhina-daw/issues/351): the left edge,
/// copying by dragging, velocity on a note and as a line, the lane's height and the audition.
#[path = "piano_roll_gestures.rs"]
mod gestures;
use gestures::Part;

mod clip;
mod clip_strip;
mod drawing;
mod edits;
mod keys;
mod layout;
mod span;
mod state;
mod toolbar;
pub(crate) use toolbar::{grid_entries, grid_key_labels, menu_arrow};

pub use self::clip::*;
#[cfg(test)]
pub(crate) use self::drawing::part_style;
use self::drawing::*;
use self::edits::*;
pub use self::keys::ViewPick;
use self::keys::*;
pub use self::layout::*;
use self::span::*;
pub use self::state::*;
pub use self::toolbar::IconButton;
pub(crate) use self::toolbar::*;

pub use gestures::start_by;

/// Height of the toolbar, widths of the scale strip and the key column, how far the toolbar's
/// buttons start from the left, height of the ruler and the default height of the velocity lane.
const TOOLBAR: f32 = 22.0;
const SCALE_STRIP: f32 = 26.0;
const KEYS: f32 = 60.0;
const TOOLBAR_INSET: f32 = 4.0;
const RULER: f32 = 36.0;
const VELOCITY_LANE: f32 = 72.0;
/// The velocity lane is left out under this height, and leaves at least this many key rows.
const LEAST_LANE: f32 = 12.0;
const LEAST_KEY_ROWS: f32 = 5.0;
/// The least height of the detail area while it shows the piano roll: the toolbar, the ruler and
/// five key rows at the default row height, and beside them the clip column's title and its five
/// rows (`docs/spec/ui-clip-editor.md` › Least height and closing).
pub const LEAST_HEIGHT: f32 = 132.0;
/// The height of a row of the strip that shows several picked clips, which the least height grows by.
pub const CLIP_STRIP: f32 = 16.0;
/// The toolbar's icon size and the space between its buttons, in points.
const TOOL_ICON: f32 = 14.0;
const TOOL_GAP: f32 = 3.0;
/// The size of a tool's icon drawn in place of the pointer over the grid, about a system cursor's.
const TOOL_POINTER: f32 = 18.0;
/// Key row heights and points per beat: the default and the zoom ranges.
const DEFAULT_KEY_HEIGHT: f32 = 12.0;
const KEY_HEIGHT_RANGE: (f32, f32) = (6.0, 160.0);
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
const FLAT_NAMES: [&str; 12] = ["C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B"];

thread_local! {
    /// How black keys are named on this thread (the window's): the setting, given each frame.
    static SPELLING: std::cell::Cell<crate::settings::NoteSpelling> =
        const { std::cell::Cell::new(crate::settings::NoteSpelling::Sharps) };
}

/// Names black keys as sharps or flats from now on, wherever a note or a scale's root is named:
/// the Settings' *Note names*, given each frame.
pub fn set_note_spelling(spelling: crate::settings::NoteSpelling) {
    SPELLING.with(|s| s.set(spelling));
}

thread_local! {
    /// The scale a clip's Scale switch turns on when the clip has none of its own (the setting).
    static DEFAULT_SCALE: std::cell::Cell<(u8, suhina_engine::model::ScaleMode)> =
        const { std::cell::Cell::new((0, suhina_engine::model::ScaleMode::Major)) };
}

/// The Settings' *Default scale*, given each frame: its root (0 is C) and its mode.
pub fn set_default_scale(root: u8, mode: suhina_engine::model::ScaleMode) {
    DEFAULT_SCALE.with(|s| s.set((root % 12, mode)));
}

/// The scale a clip's Scale switch turns on when the clip has none of its own.
pub fn default_scale() -> (u8, suhina_engine::model::ScaleMode) {
    DEFAULT_SCALE.with(std::cell::Cell::get)
}

/// A clip's address as the piano roll names it (`{"track", "clip"}` or `{"track", "scene"}`):
/// the key the Scale switch keeps a clip's scale under.
pub(crate) fn clip_address(at: ClipRef) -> Value {
    match at {
        ClipRef::Arrangement(track, clip) => json!({ "track": track.0, "clip": clip.0 }),
        ClipRef::Slot(track, scene) => json!({ "track": track.0, "scene": scene }),
    }
}

/// Where the Scale switch keeps the scale a clip had when it was switched off, by its address, for
/// as long as Suhina is open.
fn kept_scale_id(address: &Value) -> Id {
    Id::unique(("scale-switch-kept", address.to_string()))
}

/// The scale a clip's Scale switch turns on: the clip's own from before it was switched off, or
/// Settings' Default scale. The clip panel shows it while the clip has none.
pub(crate) fn switch_scale(ctx: &egui::Context, address: &Value) -> (u8, suhina_engine::model::ScaleMode) {
    ctx.data(|d| d.get_temp::<(u8, suhina_engine::model::ScaleMode)>(kept_scale_id(address)))
        .unwrap_or_else(default_scale)
}

/// The `clip.setScale` parameters the Scale switch sends (the clip panel's and the toolbar's are one
/// switch) for a clip whose scale is `scale`: off when it has one, which is kept to turn on again;
/// else the switch's scale ([`switch_scale`]). Without the clip's address.
pub(crate) fn scale_switch_params(ctx: &egui::Context, address: &Value, scale: Option<Scale>) -> Value {
    match scale {
        Some(scale) => {
            ctx.data_mut(|d| d.insert_temp(kept_scale_id(address), (scale.root(), scale.mode)));
            json!({ "off": true })
        }
        None => {
            let (root, mode) = switch_scale(ctx, address);
            let root = suhina_engine::model::ROOT_NAMES.get(usize::from(root)).copied().unwrap_or("C");
            json!({ "root": root, "mode": mode.name() })
        }
    }
}

/// The name of pitch class `class` (0 is C), as a sharp or a flat by the setting.
pub fn pitch_class_name(class: u8) -> &'static str {
    let names = match SPELLING.with(std::cell::Cell::get) {
        crate::settings::NoteSpelling::Sharps => &NAMES,
        crate::settings::NoteSpelling::Flats => &FLAT_NAMES,
    };
    names.get(usize::from(class % 12)).copied().unwrap_or("?")
}

/// The name of MIDI key `key`, with 60 as C3.
pub fn key_name(key: u8) -> String {
    format!("{}{}", pitch_class_name(key % 12), i32::from(key / 12) - 2)
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
            actions.push(state.add_action(self.clip, &[note]));
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
    let _style_class = crate::kit::style_class::enter("piano-roll");
    let mut styled_ui = crate::kit::style_class::ui(ui);
    let ui = &mut *styled_ui;
    let Context { options, keymap, copied, playhead, start_marker, seeks } = cx;
    let scale = options.toolbar_size.scale();
    let fresh = state.shown.as_ref() != Some(&clip.address);
    let sent_from = actions.len();
    if fresh {
        state.shown = Some(clip.address.clone());
        state.selected.clear();
        state.selected_ids = None;
        state.gesture = None;
        state.playing_key = None;
        state.key_live = None;
        state.clear_span();
        // The scroll reach a zoom gave the last clip is not this one's.
        state.zoom_reach = 0.0;
    }
    // Keys run before the piano roll is drawn, and their commands after it: notes a paste or a
    // duplicate selected this frame exist from the next frame on.
    let selecting_new = std::mem::take(&mut state.selecting_new);
    if !selecting_new {
        // A copy taken back brings back the selection before it, by index; any other selection
        // follows its notes when another writer moved them (#1182).
        if !state.copy_taken_back(&clip.notes) {
            state.follow_selection(&clip.notes);
        }
        state.settle_selection(&clip.notes);
    }
    // The toolbar along the top; the editor under it. The toolbar's height follows Settings ›
    // Clip editor › Piano roll › Toolbar size, and never takes more than the room the editor has.
    // A theme's taller tool buttons (`.piano-tool { height }`) grow the band to hold them.
    let tool_height = {
        let _style = crate::kit::style_class::enter("piano-tool");
        crate::kit::style_class::computed().and_then(|c| c.height).map_or(0.0, |h| h.clamp(8.0, 96.0) + 6.0)
    };
    let band = (TOOLBAR * scale).max(tool_height).min(area.height());
    let toolbar_rect = Rect::from_min_max(area.min, Pos2::new(area.right(), area.top() + band));
    let whole = area;
    let area = Rect::from_min_max(Pos2::new(area.left(), toolbar_rect.bottom()), area.max);
    // Several clips picked: their strip, over the ruler (› Several clips picked).
    let clip_row = (clip.ghosts.picked.len() > 1).then(|| {
        Rect::from_min_max(
            area.min,
            Pos2::new(area.right(), area.top() + (CLIP_STRIP * clip.ghosts.strip_rows() as f32).min(area.height())),
        )
    });
    let area = clip_row.map_or(area, |row| Rect::from_min_max(Pos2::new(area.left(), row.bottom()), area.max));
    let lane_height = velocity_lane_height(area.height() - RULER, state.key_height, state.lane_wanted);
    let strip = Rect::from_min_max(
        Pos2::new(area.left(), area.top() + RULER),
        Pos2::new(area.left() + SCALE_STRIP, area.bottom() - lane_height),
    );
    let keys =
        Rect::from_min_max(Pos2::new(strip.right(), strip.top()), Pos2::new(strip.right() + KEYS, strip.bottom()));
    let ruler = Rect::from_min_max(Pos2::new(keys.right(), area.top()), Pos2::new(area.right(), keys.top()));
    let grid_rect = Rect::from_min_max(Pos2::new(keys.right(), keys.top()), Pos2::new(area.right(), keys.bottom()));
    let lane = Rect::from_min_max(Pos2::new(keys.right(), keys.bottom()), area.max);
    toolbar(ui, toolbar_rect, ToolbarCx { clip, keymap, copied, scale }, state, actions);
    if grid_rect.width() <= 1.0 || grid_rect.height() <= 1.0 {
        state.remember_selection(&clip.notes, selecting_new || sent_edit(&actions[sent_from..]));
        return;
    }
    // A newly shown clip fits the width with its notes in the middle; until the rows are scrolled
    // by hand, a change in height centres the notes again, so they are in view at any height.
    let rows = scale_view::rows(clip, state.fold_for(clip));
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
    state.magnifier = false;
    strip_zoom(ui, strip, grid_rect, state);
    // The ruler: its loop handles and markers, and dragging it to zoom and scroll time.
    let shape = ruler_view::interact(ui, ruler, grid_rect.left(), clip, state, inputs.alt, seeks, actions);
    state.view_width = grid_rect.width();
    let rows_height = rows.len() as f32 * state.key_height;
    state.scroll.y = state.scroll.y.clamp(0.0, (rows_height - grid_rect.height()).max(0.0));
    // The view goes on past the clip over the ghost notes, so the other picked clips' notes can
    // be reached (#1054).
    let ghosts_end = clip.ghosts.notes.iter().map(|n| n.start + n.length).fold(0.0, f64::max);
    let most = crate::arrangement::arrangement_view::most_scroll_x(
        clip.length.max(ghosts_end) + clip.bar_beats * 4.0,
        state.zoom_reach,
        state.beat_width,
        grid_rect.width(),
    );
    state.scroll.x = state.scroll.x.clamp(0.0, most);
    let layout = Layout {
        strip,
        keys,
        ruler,
        grid: grid_rect,
        lane,
        scroll: state.scroll,
        beat_width: state.beat_width,
        key_height: state.key_height,
        rows,
    };
    if let Some(row) = clip_row {
        clip_strip::show(ui, row, &layout, clip, state, actions);
    }
    let grid = note_grid(clip, state.beat_width);
    let gesture = Gestures { layout: &layout, clip, grid };

    // The grid's gestures: one widget for the whole grid, so a note under the pointer and the
    // empty grid around it are told apart by where the press was.
    let response = ui.interact(grid_rect, Id::unique("piano-roll-grid"), Sense::click_and_drag());
    let pointer = response.interact_pointer_pos().or_else(|| ui.ctx().pointer_hover_pos());
    // The tool whose icon stands in for the pointer, and where: the windowing layer has no pencil,
    // brush, eraser, blade or mute cursor, so the pointer is hidden and the icon is drawn there.
    // Not where a menu or a window covers the grid: the pointer is that one's there.
    let mut tool_icon = None;
    if let Some(pos) =
        ui.ctx().pointer_hover_pos().filter(|p| grid_rect.contains(*p) && ui.rect_contains_pointer(grid_rect))
    {
        let over = note_at(&layout, &clip.notes, pos);
        let mut drawn = |tool: Tool| {
            tool_icon = tool_pointer(tool).map(|icon| (icon, pos));
            cursor_for(tool)
        };
        let cursor = match (&state.gesture, over, state.tool) {
            (Some(Gesture::Resize { .. } | Gesture::ResizeStart { .. }), _, _) => Some(CursorIcon::ResizeHorizontal),
            (Some(Gesture::Move { copy: true, .. }), _, _) => Some(CursorIcon::Copy),
            (Some(Gesture::Move { .. }), _, _) => Some(CursorIcon::Grabbing),
            (Some(Gesture::NoteVelocity { .. }), _, _) => Some(CursorIcon::ResizeVertical),
            // Erase, Slice, Mute and Paint act on whatever they touch, a note's edge included.
            (Some(Gesture::Slice { .. }), _, _) | (None, _, Tool::Slice) => drawn(Tool::Slice),
            (Some(Gesture::Erase), _, _) | (None, _, Tool::Erase) => drawn(Tool::Erase),
            (Some(Gesture::Mute), _, _) | (None, _, Tool::Mute) => drawn(Tool::Mute),
            (Some(Gesture::Paint), _, _) | (None, _, Tool::Paint) => drawn(Tool::Paint),
            (Some(Gesture::Draw { .. } | Gesture::DrawRun), _, Tool::Draw) => drawn(Tool::Draw),
            // A note drawn with the Select tool (Left click draws).
            (Some(Gesture::Draw { .. } | Gesture::DrawRun), _, _) => Some(CursorIcon::Crosshair),
            // Draw resizes and moves a note it presses on, as Select does.
            (None, Some((_, Part::Start | Part::End)), _) => Some(CursorIcon::ResizeHorizontal),
            (None, Some((_, Part::Body)), Tool::Select | Tool::Draw) if ui.input(|i| i.modifiers.command) => {
                Some(CursorIcon::Copy)
            }
            (None, _, Tool::Draw) => drawn(Tool::Draw),
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
    // A ghost note: the clip it comes from. One of the clips picked takes the focus, with a click on
    // its faint note too; any other opens with a double-click of the right button.
    if let Some(pos) = pointer.filter(|pos| note_at(&layout, &clip.notes, *pos).is_none()) {
        let ghost = || ghost_view::ghost_at(&clip.ghosts, pos, |g| layout.note_rect(&g.note()));
        let clicked = response.clicked() && state.tool == Tool::Select && !workflow.empty_click_draws;
        if (clicked || response.double_clicked_by(PointerButton::Secondary))
            && let Some(from) = ghost().filter(|from| clip.ghosts.is_picked(*from))
        {
            actions.extend(clip_strip::focus_action(from));
        } else if response.double_clicked_by(PointerButton::Secondary) {
            state.open_ghost = ghost();
        }
    }
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
        } else if state.tool == Tool::Slice {
            // The Slice tool's right button cuts and deletes the shorter piece of each note.
            state.gesture = Some(Gesture::Slice { from, delete: true, by_shift: false });
        } else if workflow.shift_right_splits && inputs.shift {
            state.gesture = Some(Gesture::Slice { from, delete: false, by_shift: true });
        } else if right_deletes {
            state.gesture = Some(Gesture::Erase);
            if let Some((index, _)) = note_at(&layout, &clip.notes, from) {
                state.erasing.insert(index);
            }
        }
    }
    // A press on a note that is not selected selects it at once, as the press goes down, not when
    // the button is let go (with Shift or Ctrl it joins the selection); a press on a selected note
    // keeps the selection, so it can be dragged, and its click picks the note alone (› Selecting).
    if ui.input(|i| i.pointer.button_pressed(PointerButton::Primary))
        && matches!(state.tool, Tool::Select | Tool::Draw | Tool::Paint)
        && let Some(pos) = widgets::pointer_over(ui, grid_rect)
    {
        state.press_added = None;
        if let Some((index, _)) = note_at(&layout, &clip.notes, pos)
            && !state.selected.contains(&index)
        {
            if inputs.shift || inputs.command {
                state.selected.insert(index);
                state.press_added = Some(index);
            } else {
                state.selected = BTreeSet::from([index]);
                if let Some(note) = clip.notes.get(index) {
                    state.remember(note);
                }
            }
        }
    }
    // The audition starts on the press: a note pressed, or the note a press is about to draw, sounds
    // for as long as the button is held (› Playing the keys).
    if ui.input(|i| i.pointer.button_pressed(PointerButton::Primary))
        && let Some(pos) = widgets::pointer_over(ui, grid_rect)
    {
        let sounded = match (note_at(&layout, &clip.notes, pos), state.tool) {
            (_, Tool::Erase | Tool::Mute | Tool::Slice) => None,
            (Some((index, _)), _) => clip.notes.get(index).map(|n| n.key),
            (None, Tool::Draw | Tool::Paint) => state.new_note(&layout, clip, grid, pos, inputs.alt).map(|n| n.key),
            (None, Tool::Select) if workflow.empty_click_draws => {
                state.new_note(&layout, clip, grid, pos, inputs.alt).map(|n| n.key)
            }
            (None, Tool::Select) => None,
        };
        if let Some(key) = sounded {
            state.audition_keys([key]);
            state.audition_press = true;
        }
    }
    if response.drag_started_by(PointerButton::Primary) {
        let from = ui.input(|i| i.pointer.press_origin()).or(pointer).unwrap_or(grid_rect.center());
        state.stroke.clear();
        state.erasing.clear();
        state.muting.clear();
        state.gesture = Some(match (note_at(&layout, &clip.notes, from), state.tool) {
            (_, Tool::Erase) => Gesture::Erase,
            (_, Tool::Slice) => Gesture::Slice { from, delete: false, by_shift: false },
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
            (None, Tool::Select) => {
                let add = inputs.shift || inputs.command;
                // Adding goes on from what is selected in time.
                if !add {
                    state.clear_span();
                }
                Gesture::Select { from, add }
            }
        });
    }

    // The velocity lane's gestures.
    let lane_response =
        (lane.height() > 4.0).then(|| ui.interact(lane, Id::unique("piano-roll-velocity"), Sense::click_and_drag()));
    if let Some(lane_response) = &lane_response {
        let at = lane_response.interact_pointer_pos();
        if lane_response.clicked()
            && let Some(pos) = at
        {
            match stem_at(&layout, &clip.notes, &BTreeSet::new(), pos) {
                Some(index) if inputs.shift || inputs.command => {
                    if !state.selected.remove(&index) {
                        state.selected.insert(index);
                        state.audition_keys(clip.notes.get(index).map(|n| n.key));
                    }
                }
                Some(index) => {
                    state.selected = BTreeSet::from([index]);
                    state.audition_keys(clip.notes.get(index).map(|n| n.key));
                }
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

    // The key column: a press or a drag selects the notes of those keys and plays the key under
    // the pointer through the track's instrument; Shift adds a range, and Ctrl selects in silence.
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
    // Ctrl (Cmd) pressed or let go while notes are dragged switches between moving and copying
    // them; what it is when the button is let go decides (› Moving).
    if dragging && let Some(Gesture::Move { copy, .. }) = &mut state.gesture {
        *copy = inputs.command;
    }
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
            Some(effect(gesture, now, &layout, clip, &chosen, grid, inputs))
        }
        _ => None,
    };
    // While a note is dragged, the keys it is on sound; the keys that already sound go on.
    if dragging {
        match preview {
            Some(Effect::Move(_, keys, rows)) => {
                state.audition_keys(chosen.iter().map(|n| edits::moved_key(&layout.rows, n.key, keys, rows)));
            }
            Some(Effect::Draw(Some(note))) => state.audition_keys([note.key]),
            Some(Effect::Select(rect)) => state.audition_swept(&layout, clip, rect),
            _ => {}
        }
    }
    // Letting the right button go stops the clip the Mouse-first switch was playing. It watches
    // the button itself, so a press that never moved stops it too.
    if ui.input(|i| i.pointer.button_released(PointerButton::Secondary)) && std::mem::take(&mut state.right_playing) {
        actions.push(Action::new("transport.stop", json!({})));
    }
    let selecting = match &preview {
        Some(Effect::Select(rect)) if dragging => Some(*rect),
        _ => None,
    };
    let velocity_label = match (&state.gesture, &preview) {
        (Some(Gesture::NoteVelocity { grabbed, .. }), Some(Effect::Velocity(by))) => Some((*grabbed, *by)),
        _ => None,
    };
    if stopped {
        // A right-button hold that chose tools with the wheel edits nothing when it is let go.
        let stepped = state.right_stepped && response.drag_stopped_by(PointerButton::Secondary);
        if let (Some(effect), Some(gesture)) = (preview.clone(), state.gesture.take())
            && !stepped
        {
            commit(&gesture, effect, clip, state, actions, &layout);
        }
        state.gesture = None;
        state.erasing.clear();
    } else if !dragging && !ui.input(|i| i.pointer.any_down()) {
        state.gesture = None;
        state.stroke.clear();
        state.erasing.clear();
        state.muting.clear();
    }

    let shape = shape.unwrap_or(clip.shape);
    draw(ui, &layout, clip, state, grid, preview, shape.played());
    draw_strip(ui, &layout, state);
    corner(
        ui,
        Rect::from_min_max(Pos2::new(strip.left(), ruler.top()), Pos2::new(keys.right(), ruler.bottom())),
        state,
        keymap,
        scale,
    );
    if let Some((grabbed, by)) = velocity_label {
        gestures::draw_velocity_label(ui, &layout, clip, grabbed, by);
    }
    let axis = ruler_view::Axis { origin: grid_rect.left() - state.scroll.x, beat_width: state.beat_width };
    ruler_view::draw(ui, ruler, &axis, &shape);
    draw_span_bar(ui, &layout, clip, state, selecting);
    if let Some(((icon, hotspot), pos)) = tool_icon {
        draw_tool_pointer(&ui.painter_at(whole), icon, tool_pointer_rect(pos, hotspot, TOOL_POINTER));
    }
    if state.magnifier
        && let Some(pos) = ui.input(|i| i.pointer.latest_pos())
    {
        draw_magnifier(ui, pos);
    }
    // The playhead: where playback is in the clip's content (through its start, offset and loop),
    // over the ruler, the grid and the velocity lane, as the other views show it.
    // Where playback starts next, for an arrangement clip: a click on the ruler moves it (#951).
    if let Some(beat) = start_marker.and_then(|at| clip.placement.content_beat(at)) {
        drawing::draw_start_marker(ui, &layout, beat);
    }
    if let Some(beat) = playhead.and_then(|at| clip.placement.content_beat(at)) {
        draw_playhead(ui, &layout, beat);
    }
    for_readers(ui, &layout, clip, state);
    scale_view::key_readers(ui, layout.keys, clip, &layout.rows, |key| layout.y_of(key), layout.key_height);
    // Notes the audition sounds go on now and off a moment later.
    let track = clip.address.get("track").and_then(Value::as_u64);
    state.settle_frame(clip, actions);
    state.audition_frame(ui, track, actions);
    state.remember_selection(&clip.notes, selecting_new || sent_edit(&actions[sent_from..]));
}

/// Whether `sent` holds an edit of the clip's notes: the piano roll set the selection for the
/// notes as they are after it (#1182).
fn sent_edit(sent: &[Action]) -> bool {
    sent.iter().any(|a| a.id.starts_with("note."))
}

#[cfg(test)]
#[path = "piano_roll_tests.rs"]
mod tests;
