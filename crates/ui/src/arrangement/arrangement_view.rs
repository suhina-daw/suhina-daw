// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Arrangement view: a timeline of lanes, one per track, with the track headers on the right
//! (`docs/design/components.md` › Arrangement view). Track headers show more controls as a lane
//! grows. Like the Launcher view it draws data and turns gestures into [`Action`]s.

use crate::kit::surface::Backgrounds as _;
use std::collections::{HashMap, HashSet};

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use serde_json::{Value, json};
use suhina_engine::model::{ClipId, FadeCurve, FadeShape, Project, Rgb, TrackId};

use crate::arrangement::automation_lanes::{self as automation, LanesState};
use crate::arrangement::timeline::{ClipBox, Folds, Lane, Timeline, inside_lanes};
pub use crate::detail::piano_roll_grid::{GridMode, PianoGrid, Width};
use crate::interaction::colour_picker;
use crate::interaction::commands;
use crate::interaction::drop::{DropProbe, DropTarget};
use crate::interaction::grid::ColumnKind;
use crate::interaction::menus::{self, TrackMenu};
use crate::interaction::nesting::{self, Place};
use crate::interaction::timeline_grid::TimelineGrid;
use crate::kit::meter::{self, Meters};
use crate::kit::motion;
use crate::kit::theme::{self, PAD, ROW};
use crate::kit::waveform;
use crate::kit::widgets::{self, Action, Drag, VOLUME_RANGE, volume_position};

mod clip;
mod header;
mod lane_menu;
mod markers_loop;
mod state;
mod tools;
mod value_controls;

use self::clip::*;

/// The colour Main's automation lanes are drawn in: it has no track colour of its own.
const MAIN_COLOUR: Rgb = Rgb(150, 150, 150);
use self::header::*;
use self::lane_menu::*;
use self::markers_loop::*;
pub use self::state::*;
pub use self::tools::TOOL_ACTIONS;
use self::value_controls::*;
use crate::settings::track_sizes::{TrackSize, TrackSizes};

/// Width of the track header column: the default and how far it can be dragged.
pub const HEADER_COLUMN: f32 = 336.0;
const HEADER_COLUMN_RANGE: (f32, f32) = (160.0, 520.0);

/// The narrowest the header column may be: Suhina's least, or the theme's when it gives one, never
/// past `max_header`, what the view leaves for it.
fn header_least(max_header: f32) -> f32 {
    crate::kit::style_class::theme_header_min_width()
        .unwrap_or(HEADER_COLUMN_RANGE.0)
        .clamp(HEADER_COLUMN_RANGE.0, HEADER_COLUMN_RANGE.1)
        .min(HEADER_COLUMN_RANGE.1.min(max_header))
}
const RULER: f32 = ROW;
#[cfg(test)]
const DEFAULT_LANE: f32 = 40.0;
const MIN_LANE: f32 = 14.0;
const MAX_LANE: f32 = 400.0;
/// How much a point of wheel movement with Alt held grows the lanes (as a power of e).
const LANE_ZOOM_PER_POINT: f32 = 0.004;
/// Heights at which a header shows its next group (components › Track header levels).
const SHOWS_MIXER: f32 = 40.0;
/// A take lane's height under its track (`docs/spec/recording.md` › Take lanes).
const TAKE_ROW: f32 = 36.0;
/// The least and the most a take lane's height may be dragged to (#989).
const TAKE_ROW_RANGE: (f32, f32) = (20.0, 240.0);
const SHOWS_ROUTING: f32 = 70.0;
/// A track that plays notes shows its MIDI input under its routing from this height.
const SHOWS_MIDI_INPUT: f32 = 90.0;
const DROPDOWN: f32 = 15.0;
/// Padding inside a header: top and bottom, and the sides.
const HEADER_PAD_Y: f32 = 4.0;
const HEADER_ROW: f32 = 16.0;
const FIELD: f32 = 16.0;
const PAN_FIELD: f32 = 40.0;
/// The gap between a header's send knobs.
const SEND_GAP: f32 = 3.0;
const SEND_WIDTH: f32 = 64.0;
/// The least width a header's volume bar keeps: narrower, the header stacks it on its own row.
const LEAST_VOLUME: f32 = 72.0;
/// The least room a track's name keeps on a header's lower row before sends are left out.
const LEAST_NAME: f32 = 48.0;
const ON_TOGGLE: f32 = 30.0;
const SOLO: f32 = 26.0;
/// A group's fold triangle at the start of its header row.
const FOLD: f32 = 12.0;
/// How far a track inside a group is indented in its header, a level.
/// The track colour's band down a header's left edge.
const COLOUR_BAND: f32 = 6.0;
/// Points per beat: the default and the zoom range.
const DEFAULT_BEAT_WIDTH: f32 = 8.0;
/// From a whole song on screen to single samples of a short sound: at 4 000 points a beat, a beat
/// at 120 BPM is a quarter of a second, about three samples a point at 48 kHz.
const BEAT_WIDTH_RANGE: (f32, f32) = (1.0, 4000.0);
/// How close grid lines may be drawn, in points: a finer grid coarsens until its lines are at
/// least this far apart (`docs/spec/arrangement-editing.md` › The grid).
const MIN_GRID_SPACING: f32 = 8.0;
/// Bars of empty timeline after the last clip.
const TAIL_BARS: f64 = 16.0;
const CLIP_GAIN_RANGE: (f64, f64) = (-70.0, 24.0);
/// A fade handle: the square drawn at a clip's top corner, and the area that takes the pointer.
const FADE_HANDLE: f32 = 7.0;
const FADE_HANDLE_HIT: f32 = 11.0;
/// A fade's curve handle: a round one half-way along the fade's line, on fades at least this wide.
const CURVE_HANDLE: f32 = 7.0;
const CURVE_HANDLE_MIN_FADE: f32 = 21.0;
/// The gain field at an audio clip's top right, and how far it stays from the clip's right edge
/// so the fade-out handle has room.
const GAIN_FIELD: Vec2 = Vec2::new(44.0, 12.0);
const GAIN_FIELD_INSET: f32 = 12.0;
/// The play start marker on the ruler: a triangle this wide and tall, pointing down.
const MARKER: Vec2 = Vec2::new(9.0, 6.0);
/// The row of arrangement markers under the ruler, and the widest a marker's name tag gets.
const MARKER_ROW: f32 = 16.0;
const MARKER_TAG_MAX: f32 = 120.0;
/// The loop brace row under the ruler, the brace's bar in it, and how near an end of the brace
/// the pointer grabs that end.
const LOOP_ROW: f32 = 14.0;

/// The loop row's height: room for `.loop-region { height }` when the theme makes the bar
/// thicker than the native row allows (up to 40 points).
fn loop_row_height() -> f32 {
    let _style = crate::kit::style_class::enter("loop-region");
    crate::kit::style_class::computed().and_then(|c| c.height).map_or(LOOP_ROW, |h| (h + 6.0).clamp(LOOP_ROW, 40.0))
}
const LOOP_BAR: f32 = 6.0;
const LOOP_GRAB: f32 = 4.0;

/// Subordinate timeline structure to clips while retaining editable theme colours.
fn grid_color(major: bool) -> Color32 {
    motion::mix(
        theme::window_background(),
        if major { theme::grid_major() } else { theme::grid_minor() },
        if major { 0.55 } else { 0.35 },
    )
}

/// The height of a clip's name bar, and the least a clip's height must be to have a body under
/// it for selecting time.
const CLIP_NAME_BAR: f32 = 22.0;
/// The waveform zoom slider's track width, in the header column's corner.
const WAVEFORM_ZOOM_WIDTH: f32 = 72.0;
/// The grid's lines this frame, as (x, on a bar), for the clips' bodies.
const CLIP_GRID: &str = "arrangement-clip-grid";
/// The waveform zoom this frame, for `paint_clip`.
const WAVEFORM_ZOOM: &str = "arrangement-waveform-zoom";
const WAVEFORM_APPEARANCE: &str = "arrangement-waveform-appearance";
const AUDIO_COLOUR_STRENGTH: &str = "arrangement-audio-colour-strength";
/// The waveform zoom's range.
pub const WAVEFORM_ZOOM_RANGE: (f32, f32) = (1.0, 16.0);

/// `zoom` a step of the keys or the wheel up (`steps` > 0) or down, kept in its range.
pub fn waveform_zoom_step(zoom: f32, steps: i32) -> f32 {
    (zoom * 1.25f32.powi(steps)).clamp(WAVEFORM_ZOOM_RANGE.0, WAVEFORM_ZOOM_RANGE.1)
}
const CLIP_BODY_LEAST: f32 = 36.0;

/// `#RRGGBB`, as colour parameters take it.
fn hex(suhina_engine::model::Rgb(r, g, b): suhina_engine::model::Rgb) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// What a clip shows that a drag in progress changes before the project does: its gain and its
/// fades.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shown {
    gain_db: f64,
    fade_in: f64,
    fade_out: f64,
    fade_in_curve: FadeCurve,
    fade_out_curve: FadeCurve,
}

impl Shown {
    fn of(clip: &ClipBox) -> Self {
        Self {
            gain_db: clip.audio_view.as_ref().map_or(0.0, |v| v.gain_db),
            fade_in: clip.fade_in,
            fade_out: clip.fade_out,
            fade_in_curve: clip.fade_in_curve,
            fade_out_curve: clip.fade_out_curve,
        }
    }

    fn curve(&self, edge: FadeEdge) -> FadeCurve {
        match edge {
            FadeEdge::In => self.fade_in_curve,
            FadeEdge::Out => self.fade_out_curve,
        }
    }
}

/// A lane on screen this frame: its track, where it is, and whether it can take notes or audio.
#[derive(Debug, Clone, Copy)]
struct Row {
    /// The lane that owns the row's header.
    track: TrackId,
    /// The track a clip on this row belongs to: the one a folded group's row stands in for.
    clips: TrackId,
    time: Rect,
    clip: Rect,
}

/// One lane the view draws: the lane that owns its header and the lane whose clips it shows. A
/// folded group hides the lanes inside it and takes the clips of the one track it stands in for
/// (`docs/spec/arrangement-editing.md` › Folded groups).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DrawnLane {
    /// Where the lane is in `Timeline::lanes`.
    lane: usize,
    /// Where the lane whose clips the row draws is: the same one, unless a fold stands in.
    clips: usize,
    /// How many lanes are inside this one (0 for a track that holds none).
    children: usize,
    /// Whether this is a group whose tracks are folded away.
    folded: bool,
}

/// The lanes the view draws, in order: every lane, minus those inside a folded group, whose row
/// instead shows the clips of the track the fold stands in for.
fn drawn_lanes(lanes: &[Lane], folds: &Folds) -> Vec<DrawnLane> {
    let mut out: Vec<DrawnLane> = Vec::new();
    // The lanes the last one is inside, innermost last: their depth, and whether they are folded.
    let mut open: Vec<(usize, bool)> = Vec::new();
    for (index, lane) in lanes.iter().enumerate() {
        // A return track is a fixed row along the bottom, not a lane in the list.
        if lane.kind == ColumnKind::Return {
            continue;
        }
        while open.last().is_some_and(|(depth, _)| lane.depth <= *depth) {
            open.pop();
        }
        if open.iter().any(|(_, folded)| *folded) {
            continue;
        }
        let children = inside_lanes(lanes, index);
        let folded = lane.kind == ColumnKind::Group && children > 0 && folds.is_folded(lane.track);
        let clips = match folded.then(|| folds.shown(lane.track)).flatten() {
            Some(show) => lanes.iter().position(|l| l.track == show).unwrap_or(index),
            None => index,
        };
        out.push(DrawnLane { lane: index, clips, children, folded });
        open.push((lane.depth, folded));
    }
    out
}

/// The beat width after zooming by `factor`, within [`BEAT_WIDTH_RANGE`].
fn zoomed(beat_width: f32, factor: f32) -> f32 {
    (beat_width * factor).clamp(BEAT_WIDTH_RANGE.0, BEAT_WIDTH_RANGE.1)
}

/// Zooms by `factor` around the point `offset` points right of the timeline's left edge: the
/// new beat width and horizontal scroll, which keep the beat under that point where it was.
fn zoom_at(beat_width: f32, scroll_x: f32, offset: f32, factor: f32) -> (f32, f32) {
    let beat = (offset + scroll_x) / beat_width;
    let width = zoomed(beat_width, factor);
    (width, beat * width - offset)
}

/// How far the timeline scrolls sideways: to the end of `content_beats`, or as far as a zoom
/// at the pointer reached (`reach` beats at the right edge), in a view `view_width` wide.
pub(crate) fn most_scroll_x(content_beats: f64, reach: f64, beat_width: f32, view_width: f32) -> f32 {
    ((content_beats.max(reach) as f32) * beat_width - view_width).max(0.0)
}

/// The Arrangement's grid keys: snap, narrower, wider, triplets (Ctrl+4, 1, 2, 3 by default).
pub const GRID_KEYS: [&str; 4] =
    ["arrangement.gridSnap", "arrangement.gridNarrow", "arrangement.gridWiden", "arrangement.gridTriplet"];

/// What the Arrangement's grid key `action` makes of `grid`, if it is one.
pub fn grid_key(action: &str, grid: PianoGrid) -> Option<PianoGrid> {
    Some(match action {
        "arrangement.gridNarrow" => grid.narrower(),
        "arrangement.gridWiden" => grid.wider(),
        "arrangement.gridTriplet" => grid.toggle_triplet(),
        "arrangement.gridSnap" => grid.toggle_snap(),
        _ => return None,
    })
}

/// The Arrangement's grid as it is drawn and snapped to at `beat_width` points a beat: an
/// adaptive grid's step for the zoom, or a fixed value's, coarsened until its lines are at least
/// [`MIN_GRID_SPACING`] apart. With snapping off the lines are still drawn and nothing snaps.
pub fn arrangement_grid(chosen: PianoGrid, beat_width: f32, bar_beats: f64) -> TimelineGrid {
    let grid = grid_of_step(chosen.step(beat_width, bar_beats), beat_width, bar_beats);
    TimelineGrid { snaps: grid.snaps && chosen.snap, ..grid }
}

/// A grid of `step` beats at `beat_width` (0 or less: beats and bars by the zoom, no snapping).
pub fn grid_of_step(step: f64, beat_width: f32, bar_beats: f64) -> TimelineGrid {
    TimelineGrid::new(step, beat_width, bar_beats, MIN_GRID_SPACING)
}

/// Where a click `raw` beats into the ruler puts the play start marker: on `grid`, or where it
/// was with `free` (Alt held); never before the start.
fn ruler_beat(raw: f64, grid: TimelineGrid, free: bool) -> f64 {
    let free = free || !grid.snaps;
    let beat = grid.place(raw, free, false);
    // Free clicks keep a thousandth of a beat, as the engine reports it.
    let beat = if free { (beat * 1000.0).round() / 1000.0 } else { beat };
    if beat.is_finite() { beat.max(0.0) } else { 0.0 }
}

/// Where a click `raw` beats into a clip's body puts the play start marker: on the grid line at or
/// before it, never before the clip's start, so a click on the clip never plays from past it.
fn clip_body_beat(raw: f64, clip_start: f64, grid: TimelineGrid, free: bool) -> f64 {
    let free = free || !grid.snaps;
    let beat = if free { (raw * 1000.0).round() / 1000.0 } else { grid.place(raw, false, true) };
    let beat = beat.max(clip_start);
    if beat.is_finite() { beat.max(0.0) } else { 0.0 }
}

fn rgb(color: suhina_engine::model::Rgb) -> Color32 {
    Color32::from_rgb(color.0, color.1, color.2)
}

/// The Arrangement's grid chosen in a menu or its toolbar: the control bar's grid shows the same
/// value when it has it (a fixed grid or none; an adaptive grid is the Arrangement's alone).
pub(crate) fn pick_grid(state: &mut ArrangementViewState, picked: PianoGrid, actions: &mut Vec<Action>) {
    state.grid = picked;
    if let Some(value) = picked.control_bar_value() {
        actions.push(Action::new("launcher.setQuantization", serde_json::json!({ "value": value })));
    }
}

/// Draws the Arrangement view in the space left in `ui` and adds what the user asked for to
/// `actions`.
pub fn show(ui: &mut Ui, timeline: &Timeline, state: &mut ArrangementViewState, actions: &mut Vec<Action>) {
    let _style = crate::kit::style_class::enter("arrangement");
    let mut styled_ui = crate::kit::style_class::ui(ui);
    let ui = &mut *styled_ui;

    // The lanes' menus show the Arrangement's grid and change it.
    if let Some(picked) = state.automation.grid_picked.take() {
        pick_grid(state, picked, actions);
    }
    state.automation.grid = state.grid;
    state.automation.grid_keys = state.grid_keys.clone();
    // A released edge whose trim never came (refused, undone at once) is let go of in a while.
    if let Some(released) = state.edge_released.as_mut() {
        released.3 = released.3.saturating_sub(1);
        if released.3 == 0 {
            state.edge_released = None;
        }
    }
    // Showing one track, the view is not scrolled down; where it was is kept for later.
    let scrolled = state.scroll.y;
    if state.focus.is_some() {
        state.scroll.y = 0.0;
    }
    show_lanes(ui, timeline, state, actions);
    if state.focus.is_some() {
        state.scroll.y = scrolled;
    }
}

fn show_lanes(ui: &mut Ui, timeline: &Timeline, state: &mut ArrangementViewState, actions: &mut Vec<Action>) {
    state.apply_size_defaults(timeline);
    // The track lanes shown: every one but the return tracks, or only the focused one.
    let focus = state.focus;
    let whole = ui.available_rect_before_wrap();
    ui.allocate_rect(whole, Sense::hover());
    // With the piano roll pulled over the whole window the view has no height: it draws nothing
    // and takes no pointer, so its ruler never sits over the splitter and takes its double-click.
    if whole.height() < 1.0 {
        return;
    }
    // The toolbar along the top, when it is on (#991); the view under it.
    let area = if state.toolbar && whole.height() > tools::TOOLBAR * 2.0 {
        let bar = Rect::from_min_size(whole.min, Vec2::new(whole.width(), tools::TOOLBAR));
        tools::toolbar(ui, bar, state);
        Rect::from_min_max(Pos2::new(whole.left(), bar.bottom() + 1.0), whole.max)
    } else {
        whole
    };
    // With the toolbar on, the view says when it has the focus: the tool keys act on it.
    if state.toolbar && state.has_focus {
        ui.painter().rect_stroke(whole, 0.0, Stroke::new(1.0, theme::accent()), StrokeKind::Inside);
    }
    let painter = ui.painter_at(area);
    painter.fill_background(area, 0.0, theme::window_background());
    crate::kit::native_css::background(&painter, area);

    // The header column, on either side, and the timeline beside it.
    let max_header = (area.width() - 120.0).max(HEADER_COLUMN_RANGE.0);
    state.header_width = state.header_width.clamp(header_least(max_header), HEADER_COLUMN_RANGE.1.min(max_header));
    let (headers, time_x) = if state.headers_left {
        let headers = egui::Rangef::new(area.left(), area.left() + state.header_width);
        (headers, egui::Rangef::new(headers.max + 1.0, area.right()))
    } else {
        let headers = egui::Rangef::new(area.right() - state.header_width, area.right());
        (headers, egui::Rangef::new(area.left(), headers.min - 1.0))
    };
    let time = Rect::from_x_y_ranges(time_x, area.y_range());
    let ruler = Rect::from_min_size(time.min, Vec2::new(time.width(), RULER));
    let loop_row =
        Rect::from_min_size(Pos2::new(time.left(), ruler.bottom() + 1.0), Vec2::new(time.width(), loop_row_height()));
    let marker_row =
        Rect::from_min_size(Pos2::new(time.left(), loop_row.bottom() + 1.0), Vec2::new(time.width(), MARKER_ROW));
    // Return tracks and the main track are fixed rows along the bottom.
    let has_returns = timeline.lanes.iter().any(|l| l.kind == ColumnKind::Return);
    let returns: Vec<&Lane> = timeline
        .lanes
        .iter()
        .filter(|l| l.kind == ColumnKind::Return && focus.is_none() && !state.returns_hidden)
        .collect();
    let main_volume_db = timeline.main_volume_db.filter(|_| focus.is_none());
    // The tracks in their groups, and those a folded group hides.
    let places: Vec<Place> = timeline.lanes.iter().filter(|l| l.kind != ColumnKind::Return).map(Lane::place).collect();
    // The lanes the view draws: a folded group stands in for one of its tracks and hides the
    // rest; with the piano roll pulled to the top, the focused track alone.
    let drawn: Vec<DrawnLane> = match focus {
        None => drawn_lanes(&timeline.lanes, &state.folds),
        Some(f) => timeline
            .lanes
            .iter()
            .position(|l| l.track == f && l.kind != ColumnKind::Return)
            .map(|lane| DrawnLane { lane, clips: lane, children: inside_lanes(&timeline.lanes, lane), folded: false })
            .into_iter()
            .collect(),
    };
    // Each return its own height (a row by default, its bottom edge dragged to change it), the
    // main track a row.
    let fixed_height =
        returns.iter().map(|l| state.return_height(l.track) + 1.0 + state.below(l.track, timeline)).sum::<f32>()
            + if main_volume_db.is_some() {
                state.main_height + 1.0 + state.automation.height(TrackId::MAIN, &timeline.automation)
            } else {
                0.0
            };
    // They take at most half the room under the rulers, so the track lanes never vanish under
    // them in a short view.
    let room = (area.bottom() - marker_row.bottom() - 1.0).max(0.0);
    let fixed_top = (area.bottom() - fixed_height.min(room / 2.0)).max(marker_row.bottom() + 1.0);
    let fixed = Rect::from_min_max(Pos2::new(area.left(), fixed_top), area.max);
    let lanes_area =
        Rect::from_min_max(Pos2::new(area.left(), marker_row.bottom() + 1.0), Pos2::new(area.right(), fixed_top - 1.0));
    let bar_beats = if timeline.bar_beats > 0.0 { timeline.bar_beats } else { 4.0 };

    // Zoom around the pointer with Ctrl and the wheel, lane heights with Alt and the wheel; the
    // wheel alone scrolls, over a value field too. Ctrl and the wheel over a value field are the
    // field's (`value_areas`), so the view leaves them alone.
    let fields = std::mem::take(&mut state.value_areas);
    let sends = std::mem::take(&mut state.send_scroll_areas);
    if let Some(pointer) = widgets::pointer_over(ui, area) {
        let over_field = (fields.iter().any(|rect| rect.contains(pointer)) && widgets::wheel_moves_value(ui))
            || (sends.iter().any(|rect| rect.contains(pointer)) && ui.input(|i| i.modifiers.shift))
            || state.zoom_area.is_some_and(|rect| rect.contains(pointer));
        let zoom = ui.input(|i| i.zoom_delta());
        if zoom != 1.0 && time.contains(pointer) && !over_field {
            (state.beat_width, state.scroll.x) =
                zoom_at(state.beat_width, state.scroll.x, pointer.x - time.left(), zoom);
            state.zoom_reach = f64::from((state.scroll.x + time.width()) / state.beat_width);
        }
        let (alt, wheel) = ui.input(|i| (i.modifiers.alt, i.smooth_scroll_delta()));
        if over_field {
            // Nothing: the field the pointer is over took the wheel.
        } else if alt {
            // Some systems turn Alt and the wheel into a sideways scroll; either axis zooms.
            let steps = if wheel.y == 0.0 { wheel.x } else { wheel.y };
            if steps != 0.0 && pointer.y > lanes_area.top() {
                let lanes: Vec<&Lane> = drawn.iter().filter_map(|d| timeline.lanes.get(d.lane)).collect();
                let factor = (steps * LANE_ZOOM_PER_POINT).exp();
                state.zoom_wheel(&lanes, timeline, factor, pointer.y - lanes_area.top());
            }
        } else {
            // Track controls have their own horizontal send navigation. Only the
            // timeline owns horizontal arrangement scrolling; vertical lanes stay linked.
            if time.contains(pointer) {
                state.scroll.x -= wheel.x;
            }
            state.scroll.y -= wheel.y;
        }
    }
    // Free movement with the middle button held: the timeline follows the pointer both ways.
    let (middle_pressed, middle_down, delta) = ui.input(|i| {
        (
            i.pointer.button_pressed(egui::PointerButton::Middle),
            i.pointer.button_down(egui::PointerButton::Middle),
            i.pointer.delta(),
        )
    });
    if middle_pressed {
        state.panning = widgets::pointer_over(ui, time).is_some_and(|p| p.y > ruler.top());
    }
    if !middle_down {
        state.panning = false;
    }
    if state.panning {
        state.scroll -= delta;
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    let content_height: f32 = drawn
        .iter()
        .filter_map(|d| timeline.lanes.get(d.lane))
        .map(|l| state.height(l) + 1.0 + state.below(l.track, timeline))
        .sum();
    let content_beats = timeline.end + TAIL_BARS * bar_beats;
    state.scroll.x =
        state.scroll.x.clamp(0.0, most_scroll_x(content_beats, state.zoom_reach, state.beat_width, time.width()));
    // A track asked to be shown comes into view, its whole row when it fits (#918).
    if let Some(track) = state.reveal.take() {
        let mut top = 0.0;
        for lane in drawn.iter().filter_map(|d| timeline.lanes.get(d.lane)) {
            let height = state.height(lane) + 1.0 + state.below(lane.track, timeline);
            if lane.track == track {
                if top < state.scroll.y {
                    state.scroll.y = top;
                } else if top + height > state.scroll.y + lanes_area.height() {
                    state.scroll.y = top + height - lanes_area.height();
                }
                break;
            }
            top += height;
        }
    }
    state.scroll.y = state.scroll.y.clamp(0.0, (content_height - lanes_area.height()).max(0.0));

    let (scroll_x, beat_width) = (state.scroll.x, state.beat_width);
    let x_of = move |beat: f64| time.left() - scroll_x + beat as f32 * beat_width;
    let first_beat = f64::from(state.scroll.x / state.beat_width);
    // The grid at this zoom: what is drawn is what snaps.
    let grid = arrangement_grid(state.grid, state.beat_width, bar_beats);
    let last_beat = f64::from((state.scroll.x + time.width()) / state.beat_width);

    // Ruler: bar numbers through the time-signature changes, a line at each labelled bar.
    // `.ruler` styles it: its background, `color` for the bar numbers, `border-color` for
    // their lines; `ruler` is its artwork (nine-sliced when the SVG says so).
    let ruler_style = crate::kit::style_class::enter("ruler");
    if !crate::kit::skin::paint(&painter, ruler, "ruler", crate::kit::skin::State::Normal, theme::text_secondary(), 0.0)
    {
        crate::kit::native_css::surface(&painter, ruler, 0.0, theme::panel_background());
    }
    let ruler_ink = crate::kit::style_class::foreground(theme::text_secondary());
    let ruler_line =
        crate::kit::style_class::computed().filter(|c| c.authored.border).map_or_else(theme::border, |c| c.border);
    // `.ruler-number` (its `color`, `font-size`, `display: none`) and `.ruler-tick` (its `color`
    // and `width`, `width: 0` hides it) inside `.ruler`.
    let (number_ink, number_size, number_shown) = {
        let _number = crate::kit::style_class::enter("ruler-number");
        let style = crate::kit::style_class::computed();
        (
            crate::kit::style_class::foreground(ruler_ink),
            style.as_ref().filter(|s| s.authored.font_size).map_or(theme::TEXT_SMALL, |s| s.font_size),
            style.as_ref().is_none_or(|s| s.visible),
        )
    };
    let ruler_tick = {
        let _tick = crate::kit::style_class::enter("ruler-tick");
        let style = crate::kit::style_class::computed();
        let ink = crate::kit::style_class::foreground(ruler_line);
        let width = style.as_ref().and_then(|s| s.width).map_or(1.0, |w| w.clamp(0.0, 4.0));
        egui::Stroke::new(width, if style.as_ref().is_some_and(|s| s.authored.color) { ink } else { ruler_line })
    };
    drop(ruler_style);
    let meter = &timeline.meter;
    let bar_width = bar_beats as f32 * state.beat_width;
    let label_every = [1u32, 2, 4, 8, 16, 32, 64].into_iter().find(|n| *n as f32 * bar_width >= 40.0).unwrap_or(128);
    let first_bar = meter.position(first_beat).bar;
    let last_bar = meter.position(last_beat).bar.saturating_add(1);
    let ruler_painter = painter.with_clip_rect(ruler);
    // The grid covers the tracks and the fixed rows, not the empty space below the tracks.
    let tracks_bottom = (lanes_area.top() - state.scroll.y + content_height).min(lanes_area.bottom());
    state.below_tracks = (lanes_area.bottom() - tracks_bottom > 4.0).then_some((tracks_bottom, lanes_area.bottom()));
    // A clip carried into it: the new track it would make, outlined where it would go.
    if let Some((top, bottom)) = state.below_tracks
        && state.clip_drag.is_some_and(|d| d.new_track)
    {
        let band = Rect::from_x_y_ranges(time.x_range(), egui::Rangef::new(top + 2.0, (top + 42.0).min(bottom)));
        painter.rect_filled(band, 2.0, theme::accent().gamma_multiply(0.12));
        painter.rect_stroke(band, 2.0, theme::hairline(theme::accent()), egui::StrokeKind::Inside);
        widgets::text_in(
            &painter,
            band.shrink2(Vec2::new(8.0, 0.0)),
            Align2::LEFT_CENTER,
            "New track",
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_secondary(),
        );
    }
    let grid_bands =
        [egui::Rangef::new(lanes_area.top(), tracks_bottom), egui::Rangef::new(fixed.top(), fixed.bottom())];
    let grid_painter = painter
        .with_clip_rect(Rect::from_x_y_ranges(time.x_range(), egui::Rangef::new(lanes_area.top(), area.bottom())));
    for bar in first_bar..=last_bar {
        if bar % label_every != 0 {
            continue;
        }
        let x = x_of(meter.bar_start(bar)).round() + 0.5;
        ruler_painter.vline(x, ruler.y_range(), ruler_tick);
        let label = Rect::from_min_max(Pos2::new(x + 3.0, ruler.top()), Pos2::new(x + 40.0, ruler.bottom()));
        if number_shown {
            widgets::text_in(
                &ruler_painter,
                label,
                Align2::LEFT_CENTER,
                &(u64::from(bar) + 1).to_string(),
                theme::mono_font(number_size),
                number_ink,
            );
        }
    }
    // TimelineGrid in the lanes: a line at every step of the grid the control bar's value sets, coarser
    // when zoomed out; bar lines stronger than the lines between them. When the time signature
    // changes, the bars come from the meter instead: a line at every bar and, zoomed in, at
    // every beat.
    // Where the lines are, for the clips to draw them over their bodies too (`paint_clip`).
    let mut grid_xs: Vec<(f32, bool)> = Vec::new();
    if meter.is_constant() {
        let max_lines = (time.width() / MIN_GRID_SPACING).ceil().max(1.0) as usize * 2 + 4;
        for (beat, on_bar) in grid.lines(first_beat, last_beat, max_lines) {
            let x = x_of(beat).round() + 0.5;
            grid_xs.push((x, on_bar));
            let stroke = theme::hairline(grid_color(on_bar));
            for band in grid_bands.iter().filter(|b| b.span() > 0.0) {
                grid_painter.vline(x, *band, stroke);
            }
        }
    } else {
        for bar in first_bar..=last_bar {
            let start = meter.bar_start(bar);
            let this_bar_beats = meter.position(start).bar_beats;
            let x = x_of(start).round() + 0.5;
            if bar_width >= 6.0 || bar % label_every == 0 {
                grid_xs.push((x, true));
            }
            if state.beat_width >= 10.0 {
                for beat in 1..this_bar_beats.ceil() as u32 {
                    grid_xs.push((x_of(start + f64::from(beat)).round() + 0.5, false));
                }
            }
            for band in grid_bands.iter().filter(|b| b.span() > 0.0) {
                if bar_width >= 6.0 || bar % label_every == 0 {
                    grid_painter.vline(x, *band, theme::hairline(grid_color(true)));
                }
                if state.beat_width >= 10.0 {
                    for beat in 1..this_bar_beats.ceil() as u32 {
                        let x = x_of(start + f64::from(beat)).round() + 0.5;
                        grid_painter.vline(x, *band, theme::hairline(grid_color(false)));
                    }
                }
            }
        }
    }
    if !state.grid_in_clips {
        grid_xs.clear();
    }
    ui.ctx().data_mut(|d| {
        d.insert_temp(Id::new(CLIP_GRID), std::sync::Arc::new(grid_xs));
        d.insert_temp(Id::new(WAVEFORM_ZOOM), state.waveform_zoom);
        d.insert_temp(Id::new(WAVEFORM_APPEARANCE), state.waveform_appearance);
        d.insert_temp(Id::new(AUDIO_COLOUR_STRENGTH), state.audio_colour_strength);
    });
    // Tempo and time-signature changes: a line in the accent colour and what changes, after
    // room for the bar number.
    for mark in timeline.marks.iter().filter(|m| m.beat >= first_beat - 64.0 && m.beat <= last_beat) {
        let x = x_of(mark.beat).round() + 0.5;
        ruler_painter.vline(x, ruler.y_range(), theme::hairline(theme::accent()));
        let label = Rect::from_min_max(Pos2::new(x + 30.0, ruler.top()), Pos2::new(x + 140.0, ruler.bottom()));
        widgets::text_in(
            &ruler_painter,
            label,
            Align2::LEFT_CENTER,
            &mark.text,
            theme::mono_font(theme::TEXT_TINY),
            theme::accent(),
        );
    }
    painter.hline(area.x_range(), ruler.bottom() + 0.5, theme::hairline(theme::border()));
    painter.fill_background(loop_row, 0.0, theme::panel_background());
    painter.hline(area.x_range(), loop_row.bottom() + 0.5, theme::hairline(theme::border()));
    painter.fill_background(marker_row, 0.0, theme::panel_background());
    painter.hline(area.x_range(), marker_row.bottom() + 0.5, theme::hairline(theme::border_strong()));

    // A click on the ruler moves the play start marker there, on the clip grid (Alt: free); while
    // playing, playback jumps there at the next launch boundary (`docs/spec/arrangement-editing.md`
    // › Play position).
    // The time selection shows along the ruler as well: which span is selected now.
    if let Some(selection) = &state.time_selection {
        let x = |beat: f64| time.left() + beat as f32 * state.beat_width - state.scroll.x;
        let span = egui::Rangef::new(x(selection.start), x(selection.end)).intersection(time.x_range());
        if span.span() > 0.0 {
            let band = Rect::from_x_y_ranges(span, egui::Rangef::new(ruler.bottom() - 6.0, ruler.bottom()));
            painter.rect_filled(band, 0.0, theme::accent());
            painter.rect_filled(
                Rect::from_x_y_ranges(span, ruler.y_range()),
                0.0,
                theme::accent().gamma_multiply(0.18),
            );
        }
    }
    let ruler_response = ui.interact(ruler, Id::new("arrangement-ruler"), Sense::click_and_drag());
    ruler_response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Ruler"));
    // The ruler is the deliberate seek: while playing, the jump waits for the next boundary of the
    // global launch quantisation, and shows where it goes until then (#657).
    if ruler_response.clicked()
        && let Some(pointer) = ruler_response.interact_pointer_pos()
    {
        let free = ui.input(|i| i.modifiers.alt);
        let raw = f64::from((pointer.x - time.left() + state.scroll.x) / state.beat_width);
        actions.push(Action::new("transport.jump", json!({ "beats": ruler_beat(raw, grid, free) })));
    }
    if let Some(pending) = timeline.pending_jump {
        let x = time.left() + pending as f32 * state.beat_width - state.scroll.x;
        if time.x_range().contains(x) {
            // Blinking while it waits, as the Play button does.
            let on = (ui.input(|i| i.time) * 4.0).floor() as i64 % 2 == 0;
            let colour = if on { theme::accent() } else { theme::accent().gamma_multiply(0.4) };
            painter.vline(x, ruler.y_range(), egui::Stroke::new(2.0, colour));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(125));
        }
    }
    // A drag along the ruler selects that time over every track (on the grid; Alt: free).
    if ruler_response.drag_started_by(egui::PointerButton::Primary)
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        let free = ui.input(|i| i.modifiers.alt);
        let raw = f64::from((origin.x - time.left() + state.scroll.x) / state.beat_width);
        state.ruler_select = Some(ruler_beat(raw, grid, free));
    }
    if let Some(from) = state.ruler_select
        && let Some(pointer) = ui.ctx().pointer_latest_pos()
    {
        let free = ui.input(|i| i.modifiers.alt);
        let raw = f64::from((pointer.x - time.left() + state.scroll.x) / state.beat_width);
        let to = ruler_beat(raw, grid, free);
        let tracks: Vec<TrackId> = timeline.lanes.iter().map(|l| l.track).collect();
        let (start, end) = (from.min(to), from.max(to));
        state.time_selection = (end - start > 1e-6).then_some(TimeSelection { tracks, start, end, row: None });
        if ui.input(|i| !i.pointer.primary_down()) {
            state.ruler_select = None;
            play_from_selection(state, timeline, actions);
        }
    }
    let beat_at = |x: f32, free: bool, state: &ArrangementViewState| {
        ruler_beat(
            f64::from((x - time.left() + state.scroll.x) / state.beat_width),
            arrangement_grid(state.grid, state.beat_width, bar_beats),
            free,
        )
    };
    // A right-click on the ruler or the marker row offers a marker there.
    let row_response = ui.interact(marker_row, Id::new("arrangement-marker-row"), Sense::click());
    row_response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Markers"));
    for response in [&ruler_response, &row_response] {
        if response.secondary_clicked()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let free = ui.input(|i| i.modifiers.alt);
            state.marker_menu_beat = beat_at(pointer.x, free, state);
        }
        response.context_menu(|ui| {
            if ui.button("Add marker here").clicked() {
                actions.push(Action::new("marker.add", json!({ "beat": state.marker_menu_beat })));
                ui.close();
            }
            // Empty time as long as the time selection, at its start; what comes after moves
            // later (#713).
            if let Some(selection) = state.time_selection.as_ref()
                && ui.button("Add section").clicked()
            {
                actions
                    .push(Action::new("range.insertTime", json!({ "start": selection.start, "end": selection.end })));
                ui.close();
            }
        });
    }
    loop_brace(ui, timeline, state, actions, loop_row, &x_of);
    marker_flags(ui, timeline, state, actions, marker_row, &x_of);
    // Back to arrangement, while tracks play launched clips instead of their arrangement
    // clips: it returns every track to its arrangement clips and the lanes light up again.
    // Its icon sits at the ruler's right end, above the tracks it sends back.
    if timeline.overrides {
        let label = "Back to arrangement";
        let side = ruler.height() - 6.0;
        let back = Rect::from_min_size(
            Pos2::new(ruler.right() - 8.0 - side, ruler.center().y - side / 2.0),
            Vec2::splat(side),
        );
        let response = widgets::button_area(ui, back, Id::new("back-to-arrangement"), label).on_hover_text(label);
        if response.clicked() {
            actions.push(Action::new("launcher.backToArrangement", json!({})));
        }
        let idle = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.fill_background(
            back,
            0.0,
            motion::mix(
                idle,
                theme::control_selected(),
                motion::toward(ui.ctx(), response.id.with("on"), false, motion::VALUE),
            ),
        );
        back_glyph(&painter, back, theme::text_primary());
    }
    // The marker: a small triangle hanging from the ruler's lower edge.
    let marker_x = x_of(timeline.start_beat).round() + 0.5;
    if (time.left()..=time.right()).contains(&marker_x) {
        let (top, half) = (ruler.bottom() - MARKER.y, MARKER.x / 2.0);
        ruler_painter.add(egui::Shape::convex_polygon(
            vec![Pos2::new(marker_x - half, top), Pos2::new(marker_x + half, top), Pos2::new(marker_x, ruler.bottom())],
            theme::text_primary(),
            egui::Stroke::NONE,
        ));
    }

    // Header column top, beside the ruler: right-click moves the headers to the other side.
    let corner = Rect::from_x_y_ranges(headers, egui::Rangef::new(ruler.top(), marker_row.bottom()));
    painter.fill_background(corner, 0.0, theme::panel_background());
    let corner_response = ui.interact(corner, Id::new("arrangement-header-corner"), Sense::click());
    corner_response.context_menu(|ui| {
        let text = if state.headers_left { "Track headers on the right" } else { "Track headers on the left" };
        if ui.button(text).clicked() {
            state.headers_left = !state.headers_left;
            ui.close();
        }
    });
    // A named, shared resize control beside returns and waveform zoom.
    let resize_x =
        if state.headers_left && corner.width() >= 210.0 { corner.right() - 102.0 } else { corner.left() + 6.0 };
    let resize = Rect::from_min_size(Pos2::new(resize_x, corner.top() + 4.0), Vec2::new(96.0, 18.0)).intersect(corner);
    let response = widgets::button_area(ui, resize, Id::new("arrangement-resize-all"), "Resize all tracks");
    widgets::subtle_dropdown(ui, &painter, resize, &response, "Resize all", theme::panel_background());
    egui::Popup::menu(&response).show(|ui| {
        for size in TrackSize::ALL {
            let height = f32::from(state.sizes.height(size));
            let selected = timeline.lanes.iter().all(|lane| (state.height(lane) - height).abs() < 0.5);
            if ui.selectable_label(selected, size.name()).clicked() {
                state.resize_all(timeline, size);
                ui.ctx().request_repaint();
                ui.close();
            }
        }
    });
    // The waveform zoom, on the corner's lower line: a waveform mark, a slider and its value.
    // Right draws audio clips' waveforms larger, the sound unchanged; a click puts the handle
    // there, the wheel steps it, a double-click puts it back to ×1.
    let zoom_row = {
        let width = 14.0 + 4.0 + WAVEFORM_ZOOM_WIDTH + 4.0 + 34.0;
        let x = if state.headers_left { corner.left() + 6.0 } else { corner.right() - 6.0 - width };
        Rect::from_min_size(Pos2::new(x, corner.bottom() - 18.0), Vec2::new(width, 14.0)).intersect(corner)
    };
    {
        let mark = Rect::from_min_size(zoom_row.min, Vec2::new(14.0, zoom_row.height()));
        let track = Rect::from_min_size(
            Pos2::new(mark.right() + 4.0, zoom_row.top()),
            Vec2::new(WAVEFORM_ZOOM_WIDTH, zoom_row.height()),
        );
        let value = Rect::from_min_max(Pos2::new(track.right() + 4.0, zoom_row.top()), zoom_row.max);
        let span = (WAVEFORM_ZOOM_RANGE.1 / WAVEFORM_ZOOM_RANGE.0).log2();
        let share_of = |zoom: f32| ((zoom / WAVEFORM_ZOOM_RANGE.0).log2() / span).clamp(0.0, 1.0);
        let zoom_at = |x: f32| {
            let share = ((x - track.left()) / track.width()).clamp(0.0, 1.0);
            WAVEFORM_ZOOM_RANGE.0 * 2f32.powf(share * span)
        };
        let response = ui.interact(
            Rect::from_min_max(mark.min, track.max),
            Id::new("arrangement-waveform-zoom"),
            Sense::click_and_drag(),
        );
        response.widget_info(|| egui::WidgetInfo::slider(true, f64::from(state.waveform_zoom), "Waveform zoom"));
        state.zoom_area = Some(response.rect);
        if response.double_clicked() {
            state.waveform_zoom = 1.0;
        } else if (response.clicked() || response.dragged())
            && let Some(pointer) = response.interact_pointer_pos()
            && pointer.x >= track.left() - 4.0
        {
            state.waveform_zoom = zoom_at(pointer.x);
        }
        if response.hovered() {
            // One step a wheel tick, from the events themselves rather than egui's smoothed scroll.
            let wheel: f32 = ui.input(|i| {
                i.raw
                    .events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::MouseWheel { delta, .. } => Some(delta.y),
                        _ => None,
                    })
                    .sum()
            });
            if wheel != 0.0 {
                state.waveform_zoom = waveform_zoom_step(state.waveform_zoom, if wheel > 0.0 { 1 } else { -1 });
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        let response = response.on_hover_text(format!(
            "Waveform zoom ×{:.1}: audio clips drawn larger, the sound unchanged. Drag, click or scroll; double-click for ×1; Ctrl+Alt+Up and Down",
            state.waveform_zoom
        ));
        let hover = widgets::hover(ui, &response);
        let ink = motion::mix(theme::text_hint(), theme::text_secondary(), hover);
        // The mark: a small waveform, so the slider says what it zooms.
        let c = mark.center();
        let wave: Vec<Pos2> =
            [(-6.0, 0.0), (-4.0, -2.0), (-2.0, 4.0), (0.0, -5.0), (2.0, 3.0), (4.0, -2.0), (6.0, 0.0)]
                .into_iter()
                .map(|(dx, dy)| Pos2::new(c.x + dx, c.y + dy))
                .collect();
        painter.add(egui::Shape::line(wave, egui::Stroke::new(1.2, ink)));
        // The track, filled up to the handle, and the handle.
        let mid = track.center().y;
        let handle_x = track.left() + track.width() * share_of(state.waveform_zoom);
        painter.line_segment(
            [Pos2::new(track.left(), mid), Pos2::new(track.right(), mid)],
            egui::Stroke::new(2.0, theme::control_background()),
        );
        painter.line_segment([Pos2::new(track.left(), mid), Pos2::new(handle_x, mid)], egui::Stroke::new(2.0, ink));
        painter.circle_filled(
            Pos2::new(handle_x, mid),
            4.5,
            motion::mix(theme::text_secondary(), theme::text_value(), hover),
        );
        widgets::text_in(
            &painter,
            value,
            Align2::LEFT_CENTER,
            &format!("×{:.1}", state.waveform_zoom),
            theme::mono_font(theme::TEXT_TINY),
            theme::text_hint(),
        );
    }
    // The return tracks' rows along the bottom go and come back with a button here.
    if has_returns && focus.is_none() {
        let label = if state.returns_hidden { "Show returns" } else { "Hide returns" };
        let font = theme::ui_font(theme::TEXT_SMALL);
        let width = widgets::text_width(ui, label, font.clone()) + 12.0;
        // On the corner's top line, the zoom under it.
        let x = if state.headers_left { corner.left() + 6.0 } else { corner.right() - 6.0 - width };
        let button = Rect::from_min_size(
            Pos2::new(x, corner.top() + if corner.width() < 210.0 { 24.0 } else { 4.0 }),
            Vec2::new(width, 18.0),
        )
        .intersect(corner);
        let response = widgets::button_area(ui, button, Id::new("arrangement-returns-toggle"), label);
        let hover = widgets::hover(ui, &response);
        painter.fill_background(button, 2.0, motion::mix(theme::control_background(), theme::control_hover(), hover));
        widgets::text_in(&painter, button, Align2::CENTER_CENTER, label, font, theme::text_secondary());
        if response.clicked() {
            state.returns_hidden = !state.returns_hidden;
        }
    }
    let edge_x = if state.headers_left { headers.max + 0.5 } else { headers.min - 0.5 };
    painter.vline(edge_x, area.y_range(), theme::hairline(theme::border_strong()));

    // Where every lane is this frame, so a dragged clip can find the lane under the pointer.
    let mut rows = Vec::new();
    let mut y = lanes_area.top() - state.scroll.y;
    for shown in &drawn {
        let Some(lane) = timeline.lanes.get(shown.lane) else { continue };
        let height = state.height(lane);
        let lane_time = Rect::from_x_y_ranges(time.x_range(), egui::Rangef::new(y, y + height));
        let clips = timeline.lanes.get(shown.clips).map_or(lane.track, |l| l.track);
        rows.push(Row { track: lane.track, clips, time: lane_time, clip: lane_time.intersect(lanes_area) });
        y += height + 1.0 + state.below(lane.track, timeline);
    }
    // Where the lanes end, scrolled as they are: how far a drag past the bottom may scroll.
    let lanes_end = y;
    let mut y = fixed.top();
    for lane in &returns {
        let height = state.return_height(lane.track);
        let lane_time = Rect::from_x_y_ranges(time.x_range(), egui::Rangef::new(y, y + height));
        rows.push(Row { track: lane.track, clips: lane.track, time: lane_time, clip: lane_time.intersect(fixed) });
        y += height + 1.0 + state.below(lane.track, timeline);
    }
    let row_of = |track: TrackId| rows.iter().find(|r| r.track == track).copied();
    let row_of_clips = |track: TrackId| rows.iter().find(|r| r.clips == track).copied();
    let lane_at = |pos: Pos2| rows.iter().find(|r| r.clip.y_range().contains(pos.y)).map(|r| r.clips);

    // Group tracks a track can play into.
    let groups: Vec<(TrackId, String)> =
        timeline.lanes.iter().filter(|l| l.kind == ColumnKind::Group).map(|l| (l.track, l.name.clone())).collect();

    // The clips and the tracks in the order the view draws them: what a Shift-click takes its run
    // of clips or of tracks from.
    let mut clip_order: Vec<ClipSpot> = Vec::new();
    let mut track_order: Vec<TrackId> = Vec::new();
    for (row, shown) in drawn.iter().enumerate() {
        if let Some(lane) = timeline.lanes.get(shown.lane) {
            track_order.push(lane.track);
        }
        let Some(clips_lane) = timeline.lanes.get(shown.clips) else { continue };
        clip_order.extend(clips_lane.clips.iter().map(|clip| ClipSpot {
            key: (clips_lane.track, clip.id),
            row,
            start: clip.start,
            end: clip.start + clip.length,
        }));
    }

    // Lanes.
    state.automation.follow_clip(state.selected_clip());
    let geometry = automation::Geometry {
        time: time.x_range(),
        headers,
        visible: lanes_area,
        scroll_x: state.scroll.x,
        beat_width: state.beat_width,
        grid: arrangement_grid(state.grid, state.beat_width, bar_beats),
    };
    let lanes_painter = painter.with_clip_rect(lanes_area);
    let mut track_number = 0;
    // The track the editor pulled to the top edits keeps the number it has in the whole view.
    let focus_number = focus.and_then(|f| {
        drawn_lanes(&timeline.lanes, &state.folds)
            .iter()
            .position(|d| timeline.lanes.get(d.lane).is_some_and(|l| l.track == f))
            .map(|i| i + 1)
    });
    // A drop drawn last frame on its new track has been moved by now.
    state.landing = None;
    let mut ghost = None;
    let mut rows_seen: Vec<(TrackId, Rect)> = Vec::new();
    let mut clip_rects: Vec<((TrackId, ClipId), Rect)> = Vec::new();
    for shown in &drawn {
        let Some(lane) = timeline.lanes.get(shown.lane) else { continue };
        // A folded group's row draws the clips of the track it stands in for, and that track's
        // colour with them, while its header stays the group's.
        let Some(clips_lane) = timeline.lanes.get(shown.clips) else { continue };
        track_number = focus_number.unwrap_or(track_number + 1);
        let Some(row) = row_of(lane.track) else { continue };
        let lane_rect = Rect::from_x_y_ranges(area.x_range(), row.time.y_range());
        let below = state.below(lane.track, timeline);
        if lane_rect.bottom() + below < lanes_area.top() || lane_rect.top() > lanes_area.bottom() {
            continue;
        }
        // A click on an empty spot of a lane clears the clip selection (arrangement-editing ›
        // Selecting clips). A double-click on an empty spot of a lane that holds clips makes a
        // one-bar clip there in the grid cell under the pointer (Alt: where the pointer is) — MIDI
        // on a notes track, audio on one that plays audio. The clips are laid over it, so a
        // click or a double-click on one is the clip's.
        let body = ui.interact(
            row.clip.intersect(lanes_area),
            Id::new(("arrangement-lane", lane.track.0)),
            Sense::click_and_drag(),
        );
        // A folded group's row stands for the track whose clips it shows.
        rows_seen.push((clips_lane.track, row.clip));
        // A drag from an empty spot (#649): from the lane's upper half it paints a time selection
        // over the tracks it crosses, from the lower half it draws a rectangle that picks whole
        // clips (with Ctrl+drag selecting time, Ctrl paints instead).
        // The Draw tool's drag makes a clip as long as itself (#991).
        let drawing = state.tool == ArrangementTool::Draw && (lane.notes || lane.plays_audio);
        let grid_now = arrangement_grid(state.grid, state.beat_width, bar_beats);
        let beat_at = |x: f32, free: bool| {
            grid_now.place(f64::from((x - time.left() + state.scroll.x) / state.beat_width), free, true)
        };
        if drawing
            && body.drag_started_by(egui::PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            state.draw_drag = Some((clips_lane.track, beat_at(origin.x, ui.input(|i| i.modifiers.alt))));
        } else if body.drag_started_by(egui::PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            state.marquee = Some(origin);
            state.marquee_scroll = state.scroll;
            state.marquee_time = state.tool == ArrangementTool::Range
                || if state.time_select_ctrl {
                    ui.input(|i| i.modifiers.command)
                } else {
                    origin.y < row.time.center().y
                };
            state.marquee_row = None;
        }
        if let Some((track, from)) = state.draw_drag.filter(|d| d.0 == clips_lane.track)
            && let Some(pointer) = ui.ctx().pointer_latest_pos()
        {
            let to = beat_at(pointer.x, ui.input(|i| i.modifiers.alt));
            let (start, end) = (from.min(to), from.max(to));
            let shown = Rect::from_x_y_ranges(
                egui::Rangef::new(
                    time.left() + start as f32 * state.beat_width - state.scroll.x,
                    time.left() + end as f32 * state.beat_width - state.scroll.x,
                ),
                row.time.y_range(),
            );
            lanes_painter.rect_filled(shown, 0.0, theme::selection().gamma_multiply(0.25));
            lanes_painter.rect_stroke(shown, 0.0, Stroke::new(1.0, theme::selection()), StrokeKind::Inside);
            if body.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
                state.draw_drag = None;
                if end - start > 1e-6 {
                    let stack = if lane.notes { state.stack_midi } else { state.stack_audio };
                    let params = json!({ "track": track.0, "start": start, "length": end - start, "stack": stack });
                    actions.push(Action::new("clip.create", params));
                }
            }
        }
        if !state.time_select_ctrl
            && state.marquee.is_none()
            && body.hover_pos().is_some_and(|p| p.y < row.time.center().y)
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
        }
        if body.secondary_clicked()
            && let Some(pointer) = body.interact_pointer_pos()
        {
            // On the grid cell under the pointer, as a double-click makes a clip (Alt: free).
            let raw = f64::from((pointer.x - time.left() + state.scroll.x) / state.beat_width);
            let grid = arrangement_grid(state.grid, state.beat_width, bar_beats);
            state.menu_beat = Some(grid.place(raw, ui.input(|i| i.modifiers.alt), true));
        }
        body.context_menu(|ui| {
            let has_takes = timeline.takes.iter().any(|t| t.track == clips_lane.track);
            empty_lane_menu(ui, clips_lane.track, clips_lane.notes, has_takes, state, actions);
        });
        body.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Lane {}", lane.name)));
        if body.clicked() {
            // With an editor pulled to the top the clip stays its: the view would otherwise go
            // back to every track under the pointer, mid-click.
            if focus.is_none() {
                state.clear_selected_clip();
                // The track whose lane was clicked is selected and its devices show (#950).
                state.selected_tracks = vec![clips_lane.track];
                state.devices_clicked = true;
            }
            state.time_selection = None;
            // The click also moves the play start marker there, on the clip grid (Alt: free);
            // while playing, playback goes on where it is and the next start begins there (#657).
            if let Some(pointer) = body.interact_pointer_pos() {
                let free = ui.input(|i| i.modifiers.alt);
                let raw = f64::from((pointer.x - time.left() + state.scroll.x) / state.beat_width);
                let beat = ruler_beat(raw, grid, free);
                state.insert = Some((clips_lane.track, beat));
                actions.push(Action::new("transport.setStart", json!({ "beats": beat })));
            }
        }
        if (lane.notes || lane.plays_audio)
            && !state.double_click_does_nothing
            && body.double_clicked()
            && let Some(pointer) = body.interact_pointer_pos()
        {
            let raw = f64::from((pointer.x - time.left() + state.scroll.x) / state.beat_width);
            let grid = arrangement_grid(state.grid, state.beat_width, bar_beats);
            let start = grid.place(raw, ui.input(|i| i.modifiers.alt), true);
            // The clip's kind follows the track, notes first, as the command decides it, and the
            // stacking setting follows.
            let stack = if lane.notes { state.stack_midi } else { state.stack_audio };
            let params = json!({ "track": lane.track.0, "start": start, "stack": stack });
            actions.push(Action::new("clip.create", params));
        }
        // The track the piano roll edits, alone in the view: lit, so it is clear which it is.
        if focus == Some(lane.track) {
            // It lights the lane over what is painted there, so it blends and never makes it see-through.
            lanes_painter.rect_filled(row.time, 0.0, theme::panel_hover());
            let edge = Rect::from_min_size(row.time.min, Vec2::new(2.0, row.time.height()));
            lanes_painter.rect_filled(edge, 0.0, theme::accent());
        }
        // A folded group's lane: every track inside it, a thin row each with its clips in their
        // colours, so it shows how much is in there and where.
        if shown.folded {
            let inside = inside_lanes(&timeline.lanes, shown.lane);
            let tracks: Vec<&Lane> =
                timeline.lanes.iter().skip(shown.lane + 1).take(inside).filter(|l| !l.clips.is_empty()).collect();
            let count = tracks.len().max(1);
            for (index, inner) in tracks.iter().enumerate() {
                let band = sub_row(row.time, index, count).shrink2(Vec2::new(0.0, 0.5));
                for clip in inner.clips.iter().filter(|c| c.start + c.length >= first_beat && c.start <= last_beat) {
                    let span =
                        egui::Rangef::new(x_of(clip.start), x_of(clip.start + clip.length).max(x_of(clip.start) + 1.0));
                    let mark = Rect::from_x_y_ranges(span, band.y_range()).intersect(lanes_area);
                    if mark.is_positive() {
                        lanes_painter.rect_filled(mark, 1.0, rgb(clip.color));
                    }
                }
            }
        }
        // A lane its stacked clips split into rows: a line between the rows along the whole lane,
        // empty stretches too, so each row reads as one.
        let rows = if shown.folded { 1 } else { clips_lane.clips.iter().map(|c| c.rows).max().unwrap_or(1) };
        for below in 1..rows {
            let y = sub_row(row.time, below, rows).top() - 0.5;
            lanes_painter.hline(row.time.x_range(), y, theme::hairline(theme::border()));
        }
        for clip in clips_lane.clips.iter().filter(|_| !shown.folded) {
            let span = egui::Rangef::new(x_of(clip.start), x_of(clip.start + clip.length));
            // A stacked clip is hit only in its own row, as it is drawn.
            let space = if clip.rows > 1 { sub_row(row.time, clip.row, clip.rows) } else { row.time };
            clip_rects.push(((clips_lane.track, clip.id), Rect::from_x_y_ranges(span, space.y_range())));
            if clip.start + clip.length >= first_beat && clip.start <= last_beat {
                let found = clip_box(
                    ui,
                    &painter,
                    row,
                    clips_lane,
                    clip,
                    &x_of,
                    bar_beats,
                    &lane_at,
                    timeline,
                    &clip_order,
                    state,
                    actions,
                );
                ghost = ghost.or(found);
            }
        }
        for &(track, start, end) in &timeline.recording_spans {
            if track == clips_lane.track {
                let rect = Rect::from_min_max(
                    Pos2::new(x_of(start), row.time.top()),
                    Pos2::new(x_of(end).max(x_of(start) + 2.0), row.time.bottom()),
                )
                .intersect(row.time);
                if rect.is_positive() {
                    lanes_painter.rect_filled(rect, 0.0, theme::recording().gamma_multiply(0.35));
                    lanes_painter.rect_stroke(rect, 0.0, theme::hairline(theme::recording()), egui::StrokeKind::Inside);
                    widgets::text_in(
                        &lanes_painter,
                        rect.shrink(3.0),
                        Align2::LEFT_TOP,
                        "Recording…",
                        theme::ui_font(theme::TEXT_SMALL),
                        theme::text_primary(),
                    );
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
            }
        }
        // The insert marker: where a click in this lane put the play start, while it is there.
        if let Some((track, beat)) = state.insert
            && track == clips_lane.track
            && (timeline.start_beat - beat).abs() < 1e-6
        {
            let x = x_of(beat).round() + 0.5;
            if lanes_area.x_range().contains(x) {
                lanes_painter.vline(x, row.time.y_range(), egui::Stroke::new(1.0, theme::text_primary()));
            }
        }
        let header = Rect::from_x_y_ranges(headers, row.time.y_range());
        let nest = Nest { places: &places, groups: &groups };
        let order = HeaderOrder { children: shown.children, tracks: &track_order };
        track_header(
            ui,
            &lanes_painter,
            header,
            lanes_area,
            lane,
            track_number,
            &nest,
            &order,
            timeline,
            state,
            actions,
        );
        // The track plays launched clips instead of its arrangement clips: the lane dims
        // until Back to arrangement returns it.
        if lane.overrides {
            lanes_painter.rect_filled(lane_rect, 0.0, theme::window_background().gamma_multiply(0.45));
        }
        // A take lane is soloed: the track's own clips are not heard, so they grey (#989).
        if state.take_solo.is_some_and(|(t, _)| t == lane.track) {
            lanes_painter.rect_filled(lane_rect, 0.0, theme::window_background().gamma_multiply(0.6));
        }
        lanes_painter.hline(area.x_range(), lane_rect.bottom() + 0.5, theme::hairline(grid_color(true)));
        // The track's take lanes, under it, then its automation lanes.
        let takes =
            TakeLanes { time: time.x_range(), headers, top: lane_rect.bottom() + 1.0, visible: lanes_area, bar_beats };
        take_lanes(ui, &lanes_painter, &takes, lane, timeline, state, &x_of, actions);
        let lanes = automation::TrackLanes {
            track: lane.track,
            name: &lane.name,
            color: lane.color,
            top: lane_rect.bottom() + 1.0 + state.takes_height(lane.track, timeline),
        };
        automation::show(ui, &lanes_painter, &geometry, &lanes, &timeline.automation, &mut state.automation, actions);
    }
    if state.automation.selected_points().is_some() {
        state.clear_selected_clip();
    }
    // The time selection, lit over its tracks' lanes.
    if let Some(selection) = &state.time_selection {
        // Within the timeline: the track headers beside it are not lit.
        let span = egui::Rangef::new(x_of(selection.start), x_of(selection.end)).intersection(time.x_range());
        for (track, row) in rows_seen.iter().filter(|(t, _)| selection.tracks.contains(t)) {
            if span.span() <= 0.0 {
                break;
            }
            // Plainly over the clips too: the accent, and a line at each edge; in one row of a
            // stack, over that row. The track's take and automation lanes under it are lit too:
            // their automation goes with the clips (#916).
            let under = state.takes_height(*track, timeline) + state.automation.height(*track, &timeline.automation);
            let band = match selection.row {
                Some((at, of)) => sub_row(*row, at, of).y_range(),
                None => egui::Rangef::new(row.top(), row.bottom() + under),
            };
            let lit = Rect::from_x_y_ranges(span, band);
            lanes_painter.rect_filled(lit, 0.0, theme::accent().gamma_multiply(0.38));
            let edge = egui::Stroke::new(1.5, theme::accent());
            lanes_painter.vline(lit.left(), lit.y_range(), edge);
            lanes_painter.vline(lit.right(), lit.y_range(), edge);
        }
    }
    if let Some(origin) = state.marquee.as_mut() {
        *origin += state.marquee_scroll - state.scroll;
        state.marquee_scroll = state.scroll;
    }
    // The selection rectangle: the clips it touches are selected when the button goes up, and
    // the span it covers becomes the time selection. It reaches the clips out of view too: every
    // lane's row is known, drawn or not.
    if let Some(origin) = state.marquee
        && let Some(now) = ui.ctx().pointer_latest_pos()
    {
        let whole = Rect::from_two_pos(origin, now);
        let rect = whole.intersect(lanes_area).intersect(time);
        let done = ui.input(|i| !i.pointer.primary_down());
        if state.marquee_time {
            // Painting time: the selection follows the pointer, on the grid (Alt: free), over the
            // tracks the drag crosses; the clips' selection is left alone.
            let grid = arrangement_grid(state.grid, state.beat_width, bar_beats);
            let free = ui.input(|i| i.modifiers.alt);
            let beat_of =
                |x: f32| grid.place(f64::from((x - time.left() + state.scroll.x) / state.beat_width), free, false);
            let tracks: Vec<TrackId> =
                rows.iter().filter(|r| r.time.y_range().intersects(whole.y_range())).map(|r| r.clips).collect();
            let (start, end) = (beat_of(rect.left()), beat_of(rect.right()));
            // Begun in a row of a stack and kept to its track: that row alone.
            let row = state.marquee_row.filter(|(track, _, _)| tracks == [*track]).map(|(_, row, rows)| (row, rows));
            state.time_selection =
                (end - start > 1e-6 && !tracks.is_empty()).then_some(TimeSelection { tracks, start, end, row });
            if done {
                state.marquee = None;
                if state.time_selection.is_some() {
                    state.selected.clear();
                    state.anchor = None;
                }
                play_from_selection(state, timeline, actions);
            }
        } else {
            // A rectangle that picks the clips it touches, whole.
            lanes_painter.rect_filled(rect, 0.0, theme::accent().gamma_multiply(0.12));
            lanes_painter.rect_stroke(rect, 0.0, theme::hairline(theme::accent()), egui::StrokeKind::Inside);
            if done {
                state.marquee = None;
                state.time_selection = None;
                let picked: Vec<(TrackId, ClipId)> = rows
                    .iter()
                    .filter_map(|row| Some((row, timeline.lanes.iter().find(|l| l.track == row.clips)?)))
                    .flat_map(|(row, lane)| {
                        lane.clips.iter().map(move |clip| {
                            let space = if clip.rows > 1 { sub_row(row.time, clip.row, clip.rows) } else { row.time };
                            ((lane.track, clip.id), clip_rect(space, clip.start, clip.length, &x_of))
                        })
                    })
                    .filter(|(_, r)| r.intersects(whole))
                    .map(|(key, _)| key)
                    .collect();
                if picked.is_empty() {
                    state.selected.clear();
                    state.anchor = None;
                    // Over no clip, the automation points it covers on the lanes under the
                    // tracks are picked (#916); clips picked take their automation along anyway.
                    state.automation.select_inside(whole);
                } else {
                    state.select_clips(picked);
                }
            }
        }
    }

    // A selection rectangle dragged past the lanes' edge scrolls them, the faster the further
    // past (#691); its start moves with the lanes, so it still starts where the drag did.
    if state.marquee.is_some()
        && let Some(now) = ui.ctx().pointer_latest_pos()
    {
        let past = |value: f32, low: f32, high: f32| {
            if value < low {
                -((low - value) / 2.0).clamp(2.0, 30.0)
            } else if value > high {
                ((value - high) / 2.0).clamp(2.0, 30.0)
            } else {
                0.0
            }
        };
        let (dx, dy) = (past(now.x, time.left(), time.right()), past(now.y, lanes_area.top(), lanes_area.bottom()));
        if dx != 0.0 || dy != 0.0 {
            let most_y = (lanes_end - lanes_area.top() + state.scroll.y - lanes_area.height()).max(0.0);
            state.scroll.x = (state.scroll.x + dx).max(0.0);
            state.scroll.y = (state.scroll.y + dy).clamp(0.0, most_y);
            ui.ctx().request_repaint();
        }
    }
    // A track header let go over a lane moves the track (and a group's tracks) there, into the
    // lane's group: above the lane under the pointer, or below it from its lower half; below an
    // open group's header is into that group.
    let tracks: Vec<&Lane> = drawn.iter().filter_map(|d| timeline.lanes.get(d.lane)).collect();
    let target_of = |y: f32| {
        let lane = tracks.iter().find(|l| row_of(l.track).is_some_and(|r| r.time.y_range().contains(y)))?;
        let row = row_of(lane.track)?;
        Some((*lane, y > row.time.center().y))
    };
    if let Some(dragged) = state.track_drag
        && let Some(pointer) = ui.ctx().pointer_interact_pos()
        && let Some((lane, below)) = target_of(pointer.y)
        && let Some(row) = row_of(lane.track)
    {
        let y = if below { row.time.bottom() + 0.5 } else { row.time.top() - 0.5 };
        if lane.track != dragged {
            painter.with_clip_rect(lanes_area).hline(area.x_range(), y, egui::Stroke::new(2.0, theme::accent()));
        }
    }
    if let Some((dragged, y)) = state.track_drop.take()
        && let Some((lane, below)) = target_of(y)
        && let Some(action) = nesting::drop_action(&places, dragged, lane.track, below, &state.folds.groups())
    {
        actions.push(action);
    }

    // Fixed rows: return tracks, then the main track, each a name and a volume.
    painter.hline(area.x_range(), fixed.top() - 0.5, theme::hairline(theme::border_strong()));
    let fixed_painter = painter.with_clip_rect(fixed);
    for lane in &returns {
        let Some(row) = row_of(lane.track) else { continue };
        for clip in &lane.clips {
            if clip.start + clip.length >= first_beat && clip.start <= last_beat {
                let found = clip_box(
                    ui,
                    &painter,
                    row,
                    lane,
                    clip,
                    &x_of,
                    bar_beats,
                    &lane_at,
                    timeline,
                    &clip_order,
                    state,
                    actions,
                );
                ghost = ghost.or(found);
            }
        }
        // A return's header is a track's: volume and pan, mute, solo, its menu, its height.
        let header = Rect::from_x_y_ranges(headers, row.time.y_range());
        let nest = Nest { places: &places, groups: &groups };
        let order = HeaderOrder { children: 0, tracks: &[] };
        track_header(ui, &fixed_painter, header, fixed, lane, 0, &nest, &order, timeline, state, actions);
        // Its automation lanes, under it, as a track's.
        let fixed_geometry = automation::Geometry { visible: fixed, ..geometry };
        let lanes = automation::TrackLanes {
            track: lane.track,
            name: &lane.name,
            color: lane.color,
            top: row.time.bottom() + 1.0,
        };
        automation::show(
            ui,
            &fixed_painter,
            &fixed_geometry,
            &lanes,
            &timeline.automation,
            &mut state.automation,
            actions,
        );
        fixed_painter.hline(area.x_range(), row.time.bottom() + 0.5, theme::hairline(theme::border()));
    }
    if let Some(db) = main_volume_db {
        // Main's lanes under its row, as a track's (#687): the row sits above them.
        let main_lanes = state.automation.height(TrackId::MAIN, &timeline.automation);
        let y = fixed.bottom() - main_lanes - state.main_height;
        let header = Rect::from_x_y_ranges(headers, egui::Rangef::new(y, y + state.main_height));
        // A click on Main's header selects it, so the detail area shows its devices (#687).
        let selected = state.selected_tracks.first() == Some(&TrackId::MAIN);
        if ui.interact(header.intersect(fixed), Id::new("arrangement-main-header"), Sense::click()).clicked() {
            state.selected_tracks = vec![TrackId::MAIN];
            state.focus = None;
            state.track_focus = false;
            // A selected clip would keep its track's devices in the detail area.
            state.clear_selected_clip();
        }
        let fill = if selected { theme::control_active() } else { theme::panel_background() };
        fixed_painter.fill_background(header, 0.0, fill);
        if let Some(probe) = state.drop.as_mut()
            && probe.devices
            && !probe.files
            && !probe.clip
        {
            probe.offer(
                Rect::from_x_y_ranges(area.x_range(), header.y_range()).intersect(fixed),
                DropTarget::Track(TrackId::MAIN),
            );
        }
        // The top edge resizes the row: the fixed rows sit along the bottom, so they grow upward.
        let edge = Rect::from_x_y_ranges(area.x_range(), egui::Rangef::new(y - 1.0, y + 3.0)).intersect(fixed);
        let response = ui
            .interact(edge, Id::new("arrangement-main-resize"), Sense::drag())
            .on_hover_cursor(egui::CursorIcon::ResizeVertical);
        if response.dragged_by(egui::PointerButton::Primary) {
            state.main_height = (state.main_height - response.drag_delta().y).clamp(ROW, MAX_LANE);
        }
        // Lined up with the track names, which start after their colour band; the controls in
        // the row's first line however tall it is.
        let inner = Rect::from_min_max(
            Pos2::new(header.left() + COLOUR_BAND + PAD, header.top() + 2.0),
            Pos2::new(header.right() - PAD, header.top() + ROW - 2.0),
        );
        let font = theme::ui_font(theme::TEXT);
        widgets::text_in(&fixed_painter, inner, Align2::LEFT_CENTER, "Main", font, theme::text_primary());
        // The main volume as a track's: a bar that a drag sets (heard while dragging, one undo
        // step), a double-click back to 0 dB.
        let field = Rect::from_min_max(Pos2::new(inner.left() + inner.width() * 0.45, inner.top()), inner.max);
        let response = widgets::value_area(
            ui,
            field.intersect(fixed),
            Id::new("arrangement-main-volume"),
            "Main volume",
            widgets::format_db(state.main_drag.unwrap_or(db)),
        );
        state.value_areas.push(field.intersect(fixed));
        let mut preview = state.main_drag;
        match widgets::drag_value(ui, &response, &mut preview, db, 0.25, 1.0, VOLUME_RANGE, 0.0) {
            Drag::Moving(v) => {
                let first = state.main_drag.is_none();
                state.main_drag = Some(v);
                actions.push(Action::new("mixer.setMainVolume", json!({ "db": v, "continue": !first })));
            }
            Drag::Done(v) => {
                state.main_drag = None;
                actions.push(Action::new("mixer.setMainVolume", json!({ "db": v, "continue": true })));
            }
            Drag::None => {}
        }
        volume_field(&fixed_painter, field, state.main_drag.unwrap_or(db), widgets::hover(ui, &response));
        let at = lying_meter(field.left() - PAD, inner.center().y);
        let level = state.meters.main;
        meter::show(ui, &fixed_painter, at, "Main", meter::Source::Main, &level, actions);
        // Its automation: the envelope button after the name, and the lanes under the row.
        let button = Rect::from_min_size(
            Pos2::new(at.left() - PAD - ROW + 4.0, inner.top()),
            Vec2::splat((ROW - 4.0).min(inner.height())),
        );
        automation::header_button(
            ui,
            &fixed_painter,
            button.intersect(fixed),
            TrackId::MAIN,
            "Main",
            &mut state.automation,
            &timeline.automation,
        );
        let fixed_geometry = automation::Geometry { visible: fixed, ..geometry };
        let lanes = automation::TrackLanes {
            track: TrackId::MAIN,
            name: "Main",
            color: MAIN_COLOUR,
            top: header.bottom() + 1.0,
        };
        automation::show(
            ui,
            &fixed_painter,
            &fixed_geometry,
            &lanes,
            &timeline.automation,
            &mut state.automation,
            actions,
        );
    }

    // Takes recording into the arrangement: a clip from where the pass began to the playhead,
    // with its notes or its waveform so far, edged in the record colour (#909).
    for take in timeline.live_takes.iter().filter(|t| !t.finished) {
        let Some(row) = row_of_clips(take.track) else { continue };
        let Some(lane) = timeline.lanes.iter().find(|l| l.track == take.track) else { continue };
        paint_live_take(&painter.with_clip_rect(row.clip), row.time, lane, take, &x_of, state.beat_width, timeline);
    }

    // The loop region, while looping is on: a line at its start and its end down every lane, the
    // returns and Main too, so the region reads in the lanes, not only in the brace over them. It
    // follows the brace while it is dragged. `.loop-line` gives its `color` and `width`.
    {
        let free = ui.input(|i| i.modifiers.alt);
        let shown = state
            .loop_drag
            .and_then(|(grab, raw)| markers_loop::dragged_loop(timeline, state, free, grab, raw))
            .or_else(|| timeline.loop_brace.filter(|b| b.enabled).map(|b| (b.start, b.end)));
        if let Some((start, end)) = shown {
            let _line = crate::kit::style_class::enter("loop-line");
            let style = crate::kit::style_class::computed();
            let colour = crate::kit::style_class::foreground(theme::accent());
            let width = style.as_ref().and_then(|s| s.width).map_or(1.5, |w| w.clamp(0.5, 6.0));
            let down = egui::Rangef::new(lanes_area.top(), fixed.bottom());
            let column = Rect::from_x_y_ranges(lanes_area.x_range(), down);
            let line_painter = painter.with_clip_rect(column);
            for beat in [start, end] {
                let x = x_of(beat).round() + 0.5;
                if column.x_range().contains(x) && colour.a() > 0 {
                    line_painter.vline(x, down, egui::Stroke::new(width, colour));
                }
            }
        }
    }

    // A clip dragged onto another lane is drawn there, over everything in that lane.
    if let Some((lane, clip, start)) = ghost
        && let Some(drag) = state.clip_drag.or(state.landing)
        && let Some(row) = row_of_clips(drag.target)
        && let Some(target) = timeline.lanes.iter().find(|l| l.track == drag.target)
    {
        let rect = clip_rect(row.time, start, clip.length, &x_of);
        let lane_look = Lane { color: lane.color, mute: target.mute, ..target.clone() };
        paint_clip(
            &painter.with_clip_rect(row.clip),
            rect,
            &lane_look,
            clip,
            Shown::of(clip),
            start,
            &x_of,
            state.beat_width,
            timeline,
            true,
        );
    }

    // The empty space under the last lane: buttons that add a track, and new tracks for what
    // the browser drops there, at the beat under the pointer.
    let empty =
        Rect::from_min_max(Pos2::new(area.left(), tracks_bottom + 1.0), Pos2::new(area.right(), lanes_area.bottom()));
    let empty_time = Rect::from_x_y_ranges(time.x_range(), empty.y_range());
    if empty_time.is_positive() {
        let space = ui.interact(empty_time, Id::new("arrangement-empty-space"), Sense::click());
        if space.clicked() {
            state.clear_selected_clip();
        }
        let menu = crate::interaction::empty_space::EmptyMenu {
            selection: &state.selected_tracks,
            tracks_copied: state.tracks_copied,
        };
        crate::interaction::empty_space::empty_space_menu(&space, &menu, actions);
    }
    if empty.height() > theme::ROW + 4.0 {
        let buttons = Rect::from_x_y_ranges(time.x_range(), empty.y_range());
        crate::interaction::empty_space::empty_space(ui, &painter, buttons, actions);
    }
    let beat_at =
        |x: f32, free: bool| grid.place(f64::from((x - time.left() + state.scroll.x) / state.beat_width), free, false);
    let free = ui.input(|i| i.modifiers.alt);
    if let Some(probe) = state.drop.as_mut() {
        for row in rows.iter() {
            let Some(lane) = timeline.lanes.iter().find(|l| l.track == row.track) else { continue };
            if lane.kind != ColumnKind::Track {
                // Devices alone land on a group or a return too, on its header or its lane, as
                // on its column in the Launcher view.
                if probe.devices && !probe.files && !probe.clip {
                    let whole = Rect::from_x_y_ranges(area.x_range(), row.clip.y_range());
                    probe.offer(whole, DropTarget::Track(row.track));
                }
                continue;
            }
            if !probe.clip {
                probe.offer(Rect::from_x_y_ranges(headers, row.clip.y_range()), DropTarget::Track(row.track));
            }
            if probe.files {
                let beat = beat_at(probe.pointer.x, free);
                let x = x_of(beat);
                let mark =
                    Rect::from_x_y_ranges(egui::Rangef::new(x, (x + 2.0).min(row.clip.right())), row.clip.y_range());
                if row.clip.contains(probe.pointer) && probe.target.is_none() {
                    probe.target = Some(DropTarget::Lane { track: row.track, beat });
                    probe.area = Some(row.clip);
                    painter.rect_filled(mark, 0.0, theme::accent());
                }
            } else {
                probe.offer(row.clip, DropTarget::Track(row.track));
            }
        }
        if !probe.clip && empty.height() > 0.0 && empty.contains(probe.pointer) {
            let beat = beat_at(probe.pointer.x.max(time.left()), free);
            probe.offer(empty, DropTarget::NewTracks { beat: Some(beat), scene: None });
        }
        if let Some(target_area) = probe.area {
            painter.rect_stroke(
                target_area.shrink(0.5),
                0.0,
                egui::Stroke::new(1.5, theme::accent()),
                egui::StrokeKind::Inside,
            );
            // One track each: the new tracks the other files go onto, below the tracks, from
            // the same beat.
            if probe.separate && matches!(probe.target, Some(DropTarget::Lane { .. })) && empty.height() > 4.0 {
                let x = probe.pointer.x.clamp(time.left(), time.right());
                let below = Rect::from_x_y_ranges(egui::Rangef::new(x, time.right()), empty.y_range());
                painter.rect_filled(below, 0.0, theme::accent().gamma_multiply(0.12));
                painter.rect_stroke(
                    below.shrink(0.5),
                    0.0,
                    egui::Stroke::new(1.0, theme::accent()),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }

    // The header column's inner edge resizes it.
    let edge = Rect::from_x_y_ranges(egui::Rangef::new(edge_x - 3.0, edge_x + 3.0), area.y_range());
    let response = ui
        .interact(edge, Id::new("arrangement-header-edge"), Sense::drag())
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    if response.dragged_by(egui::PointerButton::Primary) {
        let dx = response.drag_delta().x;
        let width = if state.headers_left { state.header_width + dx } else { state.header_width - dx };
        state.header_width = width.clamp(header_least(max_header), HEADER_COLUMN_RANGE.1.min(max_header));
    }

    // Playhead through ruler and lanes.
    if timeline.running || timeline.beat > 0.0 {
        let x = x_of(timeline.beat).round() + 0.5;
        if x >= time.left() && x <= time.right() {
            // `.playhead { color; width }`: its colour and thickness.
            let _style = crate::kit::style_class::enter("playhead");
            let color = crate::kit::style_class::foreground(theme::accent());
            match crate::kit::style_class::computed().and_then(|c| c.width) {
                Some(width) if width > 0.0 => {
                    painter.vline(x, time.y_range(), egui::Stroke::new(width.min(8.0), color));
                }
                Some(_) => {}
                None => {
                    painter.vline(x, time.y_range(), theme::hairline(color));
                }
            }
        }
    }
}

/// A time selection just painted moves the play start marker to its start, so Play starts there
/// (#937); while playing, playback goes on where it is and the next start begins there.
fn play_from_selection(state: &ArrangementViewState, timeline: &Timeline, actions: &mut Vec<Action>) {
    // The ruler reaches past the last beat a start may be on.
    if let Some(start) = state.time_selection.as_ref().map(|s| s.start.min(suhina_engine::MAX_START_BEAT))
        && (start - timeline.start_beat).abs() > 1e-9
    {
        actions.push(Action::new("transport.setStart", json!({ "beats": start })));
    }
}

#[cfg(test)]
#[path = "arrangement_view_tests.rs"]
mod tests;
