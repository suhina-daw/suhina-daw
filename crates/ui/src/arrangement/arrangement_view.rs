// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Arrangement view: a timeline of lanes, one per track, with the track headers on the right
//! (`docs/design/components.md` › Arrangement view). Track headers show more controls as a lane
//! grows. Like the Session view it draws data and turns gestures into [`Action`]s.

use std::collections::HashMap;

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, StrokeKind, Ui, Vec2};
use serde_json::{Value, json};
use suhina_engine::Quantization;
use suhina_engine::model::{ClipId, FadeCurve, FadeShape, Project, TrackId};

use crate::arrangement::automation_lanes::{self as automation, LanesState};
use crate::arrangement::timeline::{ClipBox, Folds, Lane, Timeline, inside_lanes};
use crate::interaction::colour_picker;
use crate::interaction::commands;
use crate::interaction::drop::{DropProbe, DropTarget};
use crate::interaction::grid::ColumnKind;
use crate::interaction::menus::{self, TrackMenu};
use crate::interaction::nesting::{self, Place};
use crate::interaction::timeline_grid::{self, TimelineGrid};
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
mod value_controls;

use self::clip::*;
use self::header::*;
use self::lane_menu::*;
use self::markers_loop::*;
pub use self::state::*;
use self::value_controls::*;

/// Width of the track header column: the default and how far it can be dragged.
pub const HEADER_COLUMN: f32 = 232.0;
const HEADER_COLUMN_RANGE: (f32, f32) = (160.0, 520.0);
const RULER: f32 = ROW;
const DEFAULT_LANE: f32 = 48.0;
const MIN_LANE: f32 = 20.0;
const MAX_LANE: f32 = 400.0;
/// How much a point of wheel movement with Alt held grows the lanes (as a power of e).
const LANE_ZOOM_PER_POINT: f32 = 0.004;
/// Heights at which a header shows its next group (components › Track header levels).
const SHOWS_MIXER: f32 = 48.0;
const SHOWS_ROUTING: f32 = 70.0;
/// A track that plays notes shows its MIDI input under its routing from this height.
const SHOWS_MIDI_INPUT: f32 = 90.0;
const DROPDOWN: f32 = 15.0;
/// Padding inside a header: top and bottom, and the sides.
const HEADER_PAD_Y: f32 = 5.0;
const HEADER_ROW: f32 = 15.0;
const FIELD: f32 = 16.0;
const PAN_FIELD: f32 = 46.0;
/// The gap between a header's send knobs.
const SEND_GAP: f32 = 3.0;
/// The least width a header's volume bar keeps: narrower, the header stacks it on its own row.
const LEAST_VOLUME: f32 = 56.0;
/// The least room a track's name keeps on a header's lower row before sends are left out.
const LEAST_NAME: f32 = 48.0;
const ON_TOGGLE: f32 = 30.0;
const SOLO: f32 = 18.0;
/// A group's fold triangle at the start of its header row.
const FOLD: f32 = 12.0;
/// How far a track inside a group is indented in its header, a level.
const INDENT: f32 = 16.0;
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
const LOOP_BAR: f32 = 6.0;
const LOOP_GRAB: f32 = 4.0;

/// The height of a clip's name bar, and the least a clip's height must be to have a body under
/// it for selecting time.
const CLIP_NAME_BAR: f32 = 16.0;
/// The waveform zoom slider's track width, in the header column's corner.
const WAVEFORM_ZOOM_WIDTH: f32 = 72.0;
/// The grid's lines this frame, as (x, on a bar), for the clips' bodies.
const CLIP_GRID: &str = "arrangement-clip-grid";
/// The waveform zoom this frame, for `paint_clip`.
const WAVEFORM_ZOOM: &str = "arrangement-waveform-zoom";
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

/// The Arrangement view's grid for the value `chosen` in the control bar, at `beat_width` points
/// a beat with bars of `bar_beats` beats (`docs/spec/arrangement-editing.md` › The grid): lines
/// at least [`MIN_GRID_SPACING`] apart; `none` draws beats and bars by the zoom alone and does not
/// snap.
pub fn arrangement_grid(chosen: Quantization, beat_width: f32, bar_beats: f64) -> TimelineGrid {
    TimelineGrid::new(chosen.step(timeline_grid::bar_length(bar_beats)), beat_width, bar_beats, MIN_GRID_SPACING)
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

fn rgb(color: suhina_engine::model::Rgb) -> Color32 {
    Color32::from_rgb(color.0, color.1, color.2)
}

/// Draws the Arrangement view in the space left in `ui` and adds what the user asked for to
/// `actions`.
pub fn show(ui: &mut Ui, timeline: &Timeline, state: &mut ArrangementViewState, actions: &mut Vec<Action>) {
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
    // The track lanes shown: every one but the return tracks, or only the focused one.
    let focus = state.focus;
    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, Sense::hover());
    // With the piano roll pulled over the whole window the view has no height: it draws nothing
    // and takes no pointer, so its ruler never sits over the splitter and takes its double-click.
    if area.height() < 1.0 {
        return;
    }
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, theme::window_background());

    // The header column, on either side, and the timeline beside it.
    let max_header = (area.width() - 120.0).max(HEADER_COLUMN_RANGE.0);
    state.header_width = state.header_width.clamp(HEADER_COLUMN_RANGE.0, HEADER_COLUMN_RANGE.1.min(max_header));
    let (headers, time_x) = if state.headers_left {
        let headers = egui::Rangef::new(area.left(), area.left() + state.header_width);
        (headers, egui::Rangef::new(headers.max + 1.0, area.right()))
    } else {
        let headers = egui::Rangef::new(area.right() - state.header_width, area.right());
        (headers, egui::Rangef::new(area.left(), headers.min - 1.0))
    };
    let time = Rect::from_x_y_ranges(time_x, area.y_range());
    let ruler = Rect::from_min_size(time.min, Vec2::new(time.width(), RULER));
    let loop_row = Rect::from_min_size(Pos2::new(time.left(), ruler.bottom() + 1.0), Vec2::new(time.width(), LOOP_ROW));
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
    let fixed_height = returns
        .iter()
        .map(|l| state.return_height(l.track) + 1.0 + state.automation.height(l.track, &timeline.automation))
        .sum::<f32>()
        + if main_volume_db.is_some() { state.main_height + 1.0 } else { 0.0 };
    // They take at most half the room under the rulers, so the track lanes never vanish under
    // them in a short view.
    let room = (area.bottom() - marker_row.bottom() - 1.0).max(0.0);
    let fixed_top = (area.bottom() - fixed_height.min(room / 2.0)).max(marker_row.bottom() + 1.0);
    let fixed = Rect::from_min_max(Pos2::new(area.left(), fixed_top), area.max);
    let lanes_area =
        Rect::from_min_max(Pos2::new(area.left(), marker_row.bottom() + 1.0), Pos2::new(area.right(), fixed_top - 1.0));
    let bar_beats = if timeline.bar_beats > 0.0 { timeline.bar_beats } else { 4.0 };

    // Zoom around the pointer with Ctrl and the wheel, lane heights with Alt and the wheel; the
    // wheel alone scrolls, over a value field too. Alt and the wheel over a value field are the
    // field's (`value_areas`), so the view leaves them alone.
    let fields = std::mem::take(&mut state.value_areas);
    if let Some(pointer) = widgets::pointer_over(ui, area) {
        let over_field = (fields.iter().any(|rect| rect.contains(pointer)) && widgets::wheel_moves_value(ui))
            || state.zoom_area.is_some_and(|rect| rect.contains(pointer));
        let zoom = ui.input(|i| i.zoom_delta());
        if zoom != 1.0 && time.contains(pointer) && !over_field {
            let beat = (pointer.x - time.left() + state.scroll.x) / state.beat_width;
            state.beat_width = zoomed(state.beat_width, zoom);
            state.scroll.x = beat * state.beat_width - (pointer.x - time.left());
        }
        let (alt, wheel) = ui.input(|i| (i.modifiers.alt, i.smooth_scroll_delta()));
        if over_field {
            // Nothing: the field the pointer is over took the wheel.
        } else if alt {
            // Some systems turn Alt and the wheel into a sideways scroll; either axis zooms.
            let steps = if wheel.y == 0.0 { wheel.x } else { wheel.y };
            if steps != 0.0 && pointer.y > lanes_area.top() {
                state.zoom_lanes(&timeline.lanes, (steps * LANE_ZOOM_PER_POINT).exp(), pointer.y - lanes_area.top());
            }
        } else {
            state.scroll -= wheel;
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
    let content_width = ((timeline.end + TAIL_BARS * bar_beats) as f32) * state.beat_width;
    let content_height: f32 = drawn
        .iter()
        .filter_map(|d| timeline.lanes.get(d.lane))
        .map(|l| state.height(l) + 1.0 + state.automation.height(l.track, &timeline.automation))
        .sum();
    state.scroll.x = state.scroll.x.clamp(0.0, (content_width - time.width()).max(0.0));
    state.scroll.y = state.scroll.y.clamp(0.0, (content_height - lanes_area.height()).max(0.0));

    let (scroll_x, beat_width) = (state.scroll.x, state.beat_width);
    let x_of = move |beat: f64| time.left() - scroll_x + beat as f32 * beat_width;
    let first_beat = f64::from(state.scroll.x / state.beat_width);
    // The grid at this zoom: what is drawn is what snaps.
    let grid = arrangement_grid(state.grid, state.beat_width, bar_beats);
    let last_beat = f64::from((state.scroll.x + time.width()) / state.beat_width);

    // Ruler: bar numbers through the time-signature changes, a line at each labelled bar.
    painter.rect_filled(ruler, 0.0, theme::panel_background());
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
        ruler_painter.vline(x, ruler.y_range(), theme::hairline(theme::border()));
        let label = Rect::from_min_max(Pos2::new(x + 3.0, ruler.top()), Pos2::new(x + 40.0, ruler.bottom()));
        widgets::text_in(
            &ruler_painter,
            label,
            Align2::LEFT_CENTER,
            &(u64::from(bar) + 1).to_string(),
            theme::mono_font(theme::TEXT_SMALL),
            theme::text_secondary(),
        );
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
            let stroke = theme::hairline(if on_bar { theme::border() } else { theme::panel_background() });
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
                    grid_painter.vline(x, *band, theme::hairline(theme::border()));
                }
                if state.beat_width >= 10.0 {
                    for beat in 1..this_bar_beats.ceil() as u32 {
                        let x = x_of(start + f64::from(beat)).round() + 0.5;
                        grid_painter.vline(x, *band, theme::hairline(theme::panel_background()));
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
    painter.rect_filled(loop_row, 0.0, theme::panel_background());
    painter.hline(area.x_range(), loop_row.bottom() + 0.5, theme::hairline(theme::border()));
    painter.rect_filled(marker_row, 0.0, theme::panel_background());
    painter.hline(area.x_range(), marker_row.bottom() + 0.5, theme::hairline(theme::border_strong()));

    // A click on the ruler moves the play start marker there, on the clip grid (Alt: free); while
    // playing, playback jumps there (`docs/spec/arrangement-editing.md` › Play position).
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
    if ruler_response.clicked()
        && let Some(pointer) = ruler_response.interact_pointer_pos()
    {
        let free = ui.input(|i| i.modifiers.alt);
        let raw = f64::from((pointer.x - time.left() + state.scroll.x) / state.beat_width);
        actions.push(Action::new("transport.setPosition", json!({ "beats": ruler_beat(raw, grid, free) })));
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
        painter.rect_filled(
            back,
            0.0,
            motion::mix(
                idle,
                theme::text_value(),
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
    painter.rect_filled(corner, 0.0, theme::panel_background());
    let corner_response = ui.interact(corner, Id::new("arrangement-header-corner"), Sense::click());
    corner_response.context_menu(|ui| {
        let text = if state.headers_left { "Track headers on the right" } else { "Track headers on the left" };
        if ui.button(text).clicked() {
            state.headers_left = !state.headers_left;
            ui.close();
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
        let button = Rect::from_min_size(Pos2::new(x, corner.top() + 4.0), Vec2::new(width, 18.0)).intersect(corner);
        let response = widgets::button_area(ui, button, Id::new("arrangement-returns-toggle"), label);
        let hover = widgets::hover(ui, &response);
        painter.rect_filled(button, 2.0, motion::mix(theme::control_background(), theme::control_hover(), hover));
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
        y += height + 1.0 + state.automation.height(lane.track, &timeline.automation);
    }
    // Where the lanes end, scrolled as they are: how far a drag past the bottom may scroll.
    let lanes_end = y;
    let mut y = fixed.top();
    for lane in &returns {
        let height = state.return_height(lane.track);
        let lane_time = Rect::from_x_y_ranges(time.x_range(), egui::Rangef::new(y, y + height));
        rows.push(Row { track: lane.track, clips: lane.track, time: lane_time, clip: lane_time.intersect(fixed) });
        y += height + 1.0 + state.automation.height(lane.track, &timeline.automation);
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
        let below = state.automation.height(lane.track, &timeline.automation);
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
        if body.drag_started_by(egui::PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            state.marquee = Some(origin);
            state.marquee_scroll = state.scroll;
            state.marquee_time =
                if state.time_select_ctrl { ui.input(|i| i.modifiers.command) } else { origin.y < row.time.center().y };
            state.marquee_row = None;
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
            empty_lane_menu(ui, clips_lane.track, clips_lane.notes, state, actions);
        });
        body.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Lane {}", lane.name)));
        if body.clicked() {
            // With an editor pulled to the top the clip stays its: the view would otherwise go
            // back to every track under the pointer, mid-click.
            if focus.is_none() {
                state.clear_selected_clip();
            }
            state.time_selection = None;
            // The click also moves the play start marker there, on the clip grid (Alt: free), as
            // a click on the ruler does; while playing, playback jumps there.
            if let Some(pointer) = body.interact_pointer_pos() {
                let free = ui.input(|i| i.modifiers.alt);
                let raw = f64::from((pointer.x - time.left() + state.scroll.x) / state.beat_width);
                let beat = ruler_beat(raw, grid, free);
                state.insert = Some((clips_lane.track, beat));
                actions.push(Action::new("transport.setPosition", json!({ "beats": beat })));
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
        lanes_painter.hline(area.x_range(), lane_rect.bottom() + 0.5, theme::hairline(theme::border_strong()));
        // The track's automation lanes, under it.
        let lanes = automation::TrackLanes {
            track: lane.track,
            name: &lane.name,
            color: lane.color,
            top: lane_rect.bottom() + 1.0,
        };
        automation::show(ui, &lanes_painter, &geometry, &lanes, &timeline.automation, &mut state.automation, actions);
    }
    if state.automation.selected_points().is_some() {
        state.clear_selected_clip();
    }
    // The loop region, while looping is on: a line at its start and its end down every lane, so
    // the region reads in the lanes too, not only in the brace over them.
    if let Some(brace) = timeline.loop_brace.filter(|b| b.enabled) {
        let stroke = egui::Stroke::new(1.5, theme::accent());
        for beat in [brace.start, brace.end] {
            let x = x_of(beat).round() + 0.5;
            if lanes_area.x_range().contains(x) {
                lanes_painter.vline(x, lanes_area.y_range(), stroke);
            }
        }
    }
    // The time selection, lit over its tracks' lanes.
    if let Some(selection) = &state.time_selection {
        // Within the timeline: the track headers beside it are not lit.
        let span = egui::Rangef::new(x_of(selection.start), x_of(selection.end)).intersection(time.x_range());
        for (_, row) in rows_seen.iter().filter(|(t, _)| selection.tracks.contains(t)) {
            if span.span() <= 0.0 {
                break;
            }
            // Plainly over the clips too: the accent, and a line at each edge; in one row of a
            // stack, over that row.
            let band = match selection.row {
                Some((at, of)) => sub_row(*row, at, of).y_range(),
                None => row.y_range(),
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
        let y = fixed.bottom() - state.main_height;
        let header = Rect::from_x_y_ranges(headers, egui::Rangef::new(y, fixed.bottom()));
        fixed_painter.rect_filled(header, 0.0, theme::panel_background());
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
    }

    // A clip dragged onto another lane is drawn there, over everything in that lane.
    if let Some((lane, clip, start)) = ghost
        && let Some(drag) = state.clip_drag
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
        crate::interaction::empty_space::empty_space_menu(&space, actions);
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
        state.header_width = width.clamp(HEADER_COLUMN_RANGE.0, HEADER_COLUMN_RANGE.1.min(max_header));
    }

    // Playhead through ruler and lanes.
    if timeline.running || timeline.beat > 0.0 {
        let x = x_of(timeline.beat).round() + 0.5;
        if x >= time.left() && x <= time.right() {
            painter.vline(x, time.y_range(), theme::hairline(theme::accent()));
        }
    }
}

#[cfg(test)]
#[path = "arrangement_view_tests.rs"]
mod tests;
