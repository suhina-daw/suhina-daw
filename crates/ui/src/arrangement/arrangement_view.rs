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
use crate::interaction::drop::{DropProbe, DropTarget};
use crate::interaction::grid::ColumnKind;
use crate::interaction::menus::{self, TrackMenu};
use crate::interaction::nesting::{self, Place};
use crate::kit::meter::{self, Meters};
use crate::kit::motion;
use crate::kit::theme::{self, PAD, ROW};
use crate::kit::waveform;
use crate::kit::widgets::{self, Action, Drag, VOLUME_RANGE, volume_position};

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
/// The gap between a header's send knobs, and the least width its volume bar keeps beside them.
const SEND_GAP: f32 = 3.0;
const LEAST_VOLUME: f32 = 60.0;
const ON_TOGGLE: f32 = 30.0;
const SOLO: f32 = 18.0;
/// A group's fold triangle at the start of its header row.
const FOLD: f32 = 12.0;
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
/// The coarsest grid, in bars, however far the view is zoomed out.
const MAX_GRID_BARS: f64 = 1024.0;
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

/// What a drag in the loop brace row changes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum LoopGrab {
    /// Draws a new region from this beat (on the grid) to the pointer.
    New(f64),
    Start,
    End,
    /// Moves the whole region; the pointer was this many beats into it.
    Body(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Volume,
    Pan,
}

/// A clip being dragged: where it came from, its unsnapped start and the lane under the pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ClipDrag {
    track: TrackId,
    clip: ClipId,
    start: f64,
    target: TrackId,
    /// Where the dragged clip started: the other selected clips move by as much as it does.
    origin: f64,
    /// Ctrl (Cmd) was held as the drag began (Shift with Ctrl+drag selecting time): the clips
    /// are copied rather than moved.
    copy: bool,
}

/// Which grip of a clip's edges is dragged: its left or right edge, or the loop handle at its
/// lower right, which turns the loop on as it makes the clip longer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeGrab {
    Left,
    Right,
    Loop,
}

/// A clip's edge being dragged: the clip, which grip, and the beat the edge is at so far.
#[derive(Debug, Clone, Copy)]
struct EdgeDrag {
    key: (TrackId, ClipId),
    grab: EdgeGrab,
    raw: f64,
}

/// Where a clip's edges are while `drag` moves one: on the grid (free with `free`), kept at
/// least a grid step (or a 16th of a beat) long, its left edge no earlier than its content's
/// start and its right edge no later than its audio's end, as `clip.trim` keeps them; the loop
/// handle has no end limit.
fn edge_extent(clip: &ClipBox, drag: EdgeDrag, grid: &Grid, free: bool) -> (f64, f64) {
    let end = clip.start + clip.length;
    // A clip already shorter than a grid step keeps at least its length: a coarse grid (1 bar)
    // does not stretch a short clip just because its edge was touched.
    let least = if free || !grid.snaps { 1.0 / 16.0 } else { grid.step.min(clip.length).max(1.0 / 16.0) };
    let start_edge = if drag.grab == EdgeGrab::Left { clip.start } else { end };
    // Not moved yet (a press on the edge): nothing changes.
    if (drag.raw - start_edge).abs() < 1e-9 {
        return (clip.start, end);
    }
    let beat = grid.place(drag.raw, free);
    match drag.grab {
        EdgeGrab::Left => (beat.max(clip.earliest_start).min(end - least).max(0.0), end),
        EdgeGrab::Right => {
            let to = beat.max(clip.start + least);
            (clip.start, clip.latest_end.map_or(to, |last| to.min(last.max(clip.start + least))))
        }
        EdgeGrab::Loop => (clip.start, beat.max(clip.start + least)),
    }
}

/// The height of a clip's name bar, and the least a clip's height must be to have a body under
/// it for selecting time.
const CLIP_NAME_BAR: f32 = 16.0;
const CLIP_BODY_LEAST: f32 = 36.0;

/// `#RRGGBB`, as colour parameters take it.
fn hex(suhina_engine::model::Rgb(r, g, b): suhina_engine::model::Rgb) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// Which end of a clip a fade belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FadeEdge {
    In,
    Out,
}

/// A fade handle being dragged: the clip, which fade, and its length so far in beats before
/// snapping.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FadeDrag {
    key: (TrackId, ClipId),
    edge: FadeEdge,
    raw: f64,
}

/// A fade's curve handle being dragged: the clip, which fade, the curve so far, and how far
/// below the handle's centre the pointer took hold of it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CurveDrag {
    key: (TrackId, ClipId),
    edge: FadeEdge,
    curve: f64,
    grab: f32,
}

/// A volume or pan being dragged in a track header: the track whose field took the pointer, which
/// control, the value it had when the drag started and the value it has now, and the tracks the
/// drag moves — the whole selection while the track is part of one, so they move in proportion.
#[derive(Debug, Clone, PartialEq)]
struct ValueDrag {
    track: TrackId,
    control: Control,
    start: f64,
    now: f64,
    tracks: Vec<TrackId>,
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

/// What the view keeps between frames: scrolling, zoom, lane heights, the header column, the
/// selected clip and whatever is being dragged.
#[derive(Debug)]
pub struct ArrangementViewState {
    pub scroll: Vec2,
    /// Points per beat.
    pub beat_width: f32,
    /// The value chosen in the control bar, which the grid is drawn from and clips snap to
    /// (`launcher.setQuantization`); the window sets it before each frame.
    pub grid: Quantization,
    /// Track headers on the left of the timeline instead of the right.
    pub headers_left: bool,
    /// Moved clips stack on others instead of replacing them: audio clips, MIDI clips.
    pub stack_audio: bool,
    pub stack_midi: bool,
    /// How a drag over a clip selects time (Settings › Arrangement › Time selection): with Ctrl
    /// held when true; from the clip's upper half when false. A drag on empty lanes or on the ruler selects time either way.
    pub time_select_ctrl: bool,
    /// Alt held while an audio clip's edge is dragged stretches its audio to the new length
    /// (Settings › Arrangement › Alt-drag on an edge stretches audio).
    pub stretch_alt: bool,
    /// A double-click on an empty spot of a lane does nothing (Settings › Arrangement ›
    /// Double-click makes a clip is off); by default it makes an empty clip of the track's kind.
    pub double_click_does_nothing: bool,
    /// The tracks whose plug-in edits clips (ARA), with that plug-in's name: their audio
    /// clips' menus offer to open the clip in it. The window sets it before each frame.
    pub ara_tracks: Vec<(TrackId, String)>,
    /// The installed plug-in that edits clips, by name: the menu of an audio clip on any other
    /// track offers to open it there, which adds it to the track first.
    pub ara_default: Option<String>,
    pub header_width: f32,
    heights: HashMap<TrackId, f32>,
    /// The selected clips, in the order they were picked, and the one clicked last: a Shift-click
    /// takes the run of clips from it, and the other views work on it.
    selected: Vec<(TrackId, ClipId)>,
    anchor: Option<(TrackId, ClipId)>,
    /// Where a drag over empty lanes started: the selection rectangle runs from there to the
    /// pointer until the button is let go.
    marquee: Option<Pos2>,
    /// The drag that started `marquee` paints time (a lane's or a clip's upper half, as Studio
    /// One's range does) rather than drawing a rectangle that picks whole clips (the lower half).
    marquee_time: bool,
    /// The span the last selection rectangle covered: its tracks and its beats.
    time_selection: Option<TimeSelection>,
    /// Where a drag on the ruler started, in beats: it selects time over every track.
    ruler_select: Option<f64>,
    /// A clip's edge being dragged.
    edge_drag: Option<EdgeDrag>,
    /// The beat an empty spot of a lane was right-clicked on, for its menu.
    menu_beat: Option<f64>,
    clip_drag: Option<ClipDrag>,
    value_drag: Option<ValueDrag>,
    /// A clip gain being dragged, and its value so far.
    gain_drag: Option<((TrackId, ClipId), f64)>,
    /// The main track's volume being dragged, and its value so far.
    main_drag: Option<f64>,
    /// A header's send knob being dragged: the track, the return and the level so far (dB).
    send_drag: Option<(TrackId, TrackId, f64)>,
    /// The main row's height, its top edge dragged to change it (a row by default).
    main_height: f32,
    fade_drag: Option<FadeDrag>,
    curve_drag: Option<CurveDrag>,
    /// The beat a clip was last right-clicked on, where its menu's Split here cuts.
    split_at: Option<f64>,
    /// Tracks selected by clicking their headers (Ctrl adds, Shift takes a run), for grouping.
    pub selected_tracks: Vec<TrackId>,
    /// A track header was the last thing clicked: Delete removes the selected tracks then
    /// (#690). Picking or clearing clips takes it away.
    pub track_focus: bool,
    /// The track clicked last: where a Shift-click's run of tracks starts.
    track_anchor: Option<TrackId>,
    /// The groups whose tracks are folded away, and what each folded row shows.
    pub folds: Folds,
    /// Where the volume, pan and gain fields were drawn last frame: the wheel over one changes its
    /// value instead of scrolling the view, which has to know before they draw themselves again.
    value_areas: Vec<Rect>,
    /// Volume and pan as knobs instead of fields (Settings › Session and mixer).
    pub knobs: bool,
    /// The track the user last worked on: the header they clicked, or the track of the clip they
    /// selected. A folded group shows that track while it is one of its own.
    targeted: Option<TrackId>,
    /// A track header being dragged to a new place, and where it was let go.
    track_drag: Option<TrackId>,
    track_drop: Option<(TrackId, f32)>,
    /// The middle button is held after being pressed over the timeline: the view follows it.
    panning: bool,
    /// A drag from the browser over the window: the view says which target is under the
    /// pointer (`docs/spec/browser.md` › Dragging into the project).
    pub drop: Option<DropProbe>,
    /// The engine's levels this frame, for the headers' meters.
    pub meters: Meters,
    /// The piano roll is pulled to the top: the view shows only this track, highlighted, and
    /// no return tracks or main track (`docs/spec/ui-clip-editor.md` › Pulled to the top).
    pub focus: Option<TrackId>,
    /// A clip was clicked this frame (picked, even when it was selected already): the window
    /// opens a clip editor closed by hand again (`docs/spec/ui-clip-editor.md` › Least height
    /// and closing).
    pub clip_clicked: bool,
    /// A track header was double-clicked this frame: the window shows that track's devices
    /// (`docs/spec/ui-clip-editor.md` › Where it opens).
    pub open_devices: Option<TrackId>,
    /// Automation lanes under the tracks: which are shown, selected points, a drag.
    pub automation: LanesState,
    /// A marker being dragged: its index and where the pointer is, in beats before snapping.
    marker_drag: Option<(usize, f64)>,
    /// The beat a right-click on the ruler or the marker row would add a marker at.
    marker_menu_beat: f64,
    /// The name being typed for a marker in its menu.
    marker_name: Option<(usize, String)>,
    /// A drag in the loop brace row: what it changes and where the pointer is, in beats before
    /// snapping.
    loop_drag: Option<(LoopGrab, f64)>,
}

impl Default for ArrangementViewState {
    fn default() -> Self {
        Self {
            scroll: Vec2::ZERO,
            beat_width: DEFAULT_BEAT_WIDTH,
            grid: Quantization::Bars(1),
            headers_left: false,
            stack_audio: false,
            stack_midi: false,
            time_select_ctrl: false,
            stretch_alt: true,
            double_click_does_nothing: false,
            ara_tracks: Vec::new(),
            ara_default: None,
            header_width: HEADER_COLUMN,
            heights: HashMap::new(),
            selected: Vec::new(),
            anchor: None,
            marquee: None,
            marquee_time: false,
            time_selection: None,
            ruler_select: None,
            edge_drag: None,
            menu_beat: None,
            clip_drag: None,
            value_drag: None,
            gain_drag: None,
            main_drag: None,
            send_drag: None,
            main_height: ROW,
            fade_drag: None,
            curve_drag: None,
            split_at: None,
            selected_tracks: Vec::new(),
            track_focus: false,
            track_anchor: None,
            folds: Folds::default(),
            value_areas: Vec::new(),
            knobs: false,
            targeted: None,
            track_drag: None,
            track_drop: None,
            panning: false,
            drop: None,
            meters: Meters::default(),
            focus: None,
            clip_clicked: false,
            open_devices: None,
            automation: LanesState::default(),
            marker_drag: None,
            marker_menu_beat: 0.0,
            marker_name: None,
            loop_drag: None,
        }
    }
}

/// A span of beats over some tracks, the selection rectangle's.
#[derive(Debug, Clone, PartialEq)]
pub struct TimeSelection {
    pub tracks: Vec<TrackId>,
    pub start: f64,
    pub end: f64,
}

impl ArrangementViewState {
    /// The span the last selection rectangle covered, if one did.
    pub fn time_selection(&self) -> Option<&TimeSelection> {
        self.time_selection.as_ref()
    }

    /// Sets or clears the time selection: after a duplicate it moves onto the copy.
    pub fn set_time_selection(&mut self, selection: Option<TimeSelection>) {
        self.time_selection = selection;
    }

    /// The clip the views work on: the one clicked last.
    pub fn selected_clip(&self) -> Option<(TrackId, ClipId)> {
        self.anchor
    }

    /// The selected clips, in the order they were picked.
    pub fn selected_clips(&self) -> &[(TrackId, ClipId)] {
        &self.selected
    }

    /// Whether the clip at `key` is selected.
    fn is_selected(&self, key: (TrackId, ClipId)) -> bool {
        self.selected.contains(&key)
    }

    pub fn clear_selected_clip(&mut self) {
        self.selected.clear();
        self.anchor = None;
        self.track_focus = false;
    }

    /// Selects every clip in `keys`, as Ctrl+A does.
    pub fn select_clips(&mut self, keys: Vec<(TrackId, ClipId)>) {
        self.track_focus = false;
        self.anchor = keys.first().copied();
        self.selected = keys;
    }

    /// Picks the clip at `key` the way a click on it does: Shift takes the run of clips between
    /// the last one picked and this one (in the order the lanes draw them), Ctrl (Cmd) adds it or
    /// takes it away, and a plain click leaves it the only one.
    fn pick_clip(&mut self, key: (TrackId, ClipId), shift: bool, command: bool, order: &[(TrackId, ClipId)]) {
        self.track_focus = false;
        if shift
            && let Some(anchor) = self.anchor
            && let (Some(from), Some(to)) =
                (order.iter().position(|k| *k == anchor), order.iter().position(|k| *k == key))
        {
            let (from, to) = if from <= to { (from, to) } else { (to, from) };
            self.selected = order.get(from..=to).map(<[(TrackId, ClipId)]>::to_vec).unwrap_or_default();
            return;
        }
        if command {
            match self.selected.iter().position(|k| *k == key) {
                Some(at) => {
                    self.selected.remove(at);
                    if self.anchor == Some(key) {
                        self.anchor = self.selected.last().copied();
                    }
                }
                None => {
                    self.selected.push(key);
                    self.anchor = Some(key);
                }
            }
            return;
        }
        self.selected = vec![key];
        self.anchor = Some(key);
    }

    /// Picks the track `track` the way a click on its header does: Shift takes the run of tracks
    /// between the last one picked and this one, Ctrl (Cmd) adds it or takes it away.
    fn pick_track(&mut self, track: TrackId, shift: bool, command: bool, order: &[TrackId]) {
        if shift
            && let Some(anchor) = self.track_anchor
            && let (Some(from), Some(to)) =
                (order.iter().position(|t| *t == anchor), order.iter().position(|t| *t == track))
        {
            let (from, to) = if from <= to { (from, to) } else { (to, from) };
            self.selected_tracks = order.get(from..=to).map(<[TrackId]>::to_vec).unwrap_or_default();
            return;
        }
        if command {
            match self.selected_tracks.iter().position(|t| *t == track) {
                Some(at) => {
                    self.selected_tracks.remove(at);
                    if self.track_anchor == Some(track) {
                        self.track_anchor = self.selected_tracks.last().copied();
                    }
                }
                None => {
                    self.selected_tracks.push(track);
                    self.track_anchor = Some(track);
                }
            }
            return;
        }
        self.selected_tracks = vec![track];
        self.track_anchor = Some(track);
    }

    /// Brings the folds in line with the project and the track being worked on. The window calls
    /// this before it builds the timeline, so a folded group's row shows the right track.
    pub fn reconcile(&mut self, project: &Project) {
        self.folds.reconcile(project, self.working_on());
    }

    /// Folds a group's tracks away, as the group a Stems file arrives in does.
    pub fn fold_group(&mut self, group: TrackId) {
        self.folds.fold(group);
    }

    /// The track the user is working on: the last header they clicked, or the track of the
    /// selected clip.
    fn working_on(&self) -> Option<TrackId> {
        self.targeted.or_else(|| self.anchor.map(|(track, _)| track))
    }

    /// The tracks a header's mixer control acts on: the whole selection while this track is one of
    /// two or more selected, so their volume and pan move in proportion, else this track alone.
    fn controlled_tracks(&self, track: TrackId) -> Vec<TrackId> {
        if self.selected_tracks.len() > 1 && self.selected_tracks.contains(&track) {
            self.selected_tracks.clone()
        } else {
            vec![track]
        }
    }

    /// The clip being dragged with the mouse, if any: its track and index.
    pub fn dragged_clip(&self) -> Option<(TrackId, ClipId)> {
        self.clip_drag.map(|d| (d.track, d.clip))
    }

    /// Ends a clip's drag without moving it, as when the window carries it to the other view.
    pub fn cancel_clip_drag(&mut self) {
        self.clip_drag = None;
    }

    /// Selects a clip: its track and id.
    pub fn select_clip(&mut self, track: TrackId, clip: ClipId) {
        self.select_clips(vec![(track, clip)]);
    }

    /// Sets a lane's height, as dragging its lower edge does.
    pub fn set_lane_height(&mut self, track: TrackId, height: f32) {
        self.heights.insert(track, height.clamp(MIN_LANE, MAX_LANE));
    }

    /// Scales every track lane's height by `factor`, keeping the content `anchor` points below
    /// the top of the lanes where it is, as Alt and the wheel do.
    fn zoom_lanes(&mut self, lanes: &[Lane], factor: f32, anchor: f32) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let lanes: Vec<&Lane> = lanes.iter().filter(|l| l.kind != ColumnKind::Return).collect();
        // Every lane already at the limit it is zoomed towards: nothing moves.
        let stuck = lanes.iter().all(|l| {
            let h = self.height(l);
            (factor > 1.0 && h >= MAX_LANE) || (factor < 1.0 && h <= MIN_LANE)
        });
        if stuck {
            return;
        }
        let content = |state: &Self| lanes.iter().map(|l| state.height(l) + 1.0).sum::<f32>();
        let before = content(self);
        for lane in &lanes {
            let height = self.height(lane) * factor;
            self.heights.insert(lane.track, height.clamp(MIN_LANE, MAX_LANE));
        }
        let ratio = if before > 0.0 { content(self) / before } else { 1.0 };
        self.scroll.y = ((self.scroll.y + anchor) * ratio - anchor).max(0.0);
    }

    /// What the clip at `key` shows: the project's gain and fades, or the value of a drag in
    /// progress on it (fade ends on `grid`, or free when it is `None`).
    fn shown(&self, key: (TrackId, ClipId), clip: &ClipBox, grid: Option<Grid>) -> Shown {
        let mut shown = Shown::of(clip);
        if let Some((k, v)) = self.gain_drag
            && k == key
        {
            shown.gain_db = v;
        }
        if let Some(drag) = self.fade_drag.filter(|d| d.key == key) {
            match drag.edge {
                FadeEdge::In => shown.fade_in = fade_length(drag.raw, FadeEdge::In, clip, shown.fade_out, grid),
                FadeEdge::Out => shown.fade_out = fade_length(drag.raw, FadeEdge::Out, clip, shown.fade_in, grid),
            }
        }
        if let Some(drag) = self.curve_drag.filter(|d| d.key == key) {
            match drag.edge {
                FadeEdge::In => shown.fade_in_curve.curve = drag.curve,
                FadeEdge::Out => shown.fade_out_curve.curve = drag.curve,
            }
        }
        shown
    }

    fn height(&self, lane: &Lane) -> f32 {
        if lane.kind == ColumnKind::Return { self.return_height(lane.track) } else { self.lane_height(lane.track) }
    }

    /// How tall a return track's row is: a row's height until its edge is dragged.
    fn return_height(&self, track: TrackId) -> f32 {
        self.heights.get(&track).copied().unwrap_or(SHOWS_MIXER)
    }

    /// How tall `track`'s lane is.
    pub fn lane_height(&self, track: TrackId) -> f32 {
        self.heights.get(&track).copied().unwrap_or(DEFAULT_LANE)
    }

    /// How tall the view is when it shows only `track` ([`Self::focus`]): the ruler, the loop
    /// brace and marker rows under it, and the whole lane.
    pub fn focus_height(&self, track: TrackId) -> f32 {
        RULER + 1.0 + LOOP_ROW + 1.0 + MARKER_ROW + 1.0 + self.lane_height(track) + 1.0
    }
}

/// The beat width after zooming by `factor`, within [`BEAT_WIDTH_RANGE`].
fn zoomed(beat_width: f32, factor: f32) -> f32 {
    (beat_width * factor).clamp(BEAT_WIDTH_RANGE.0, BEAT_WIDTH_RANGE.1)
}

/// The Arrangement view's grid at one zoom: the lines drawn and the beats things snap to
/// (`docs/spec/arrangement-editing.md` › The grid).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    /// Beats between lines inside a bar; a whole number of bars when it is a bar or more.
    pub step: f64,
    /// Beats in a bar: lines start again at every bar line.
    pub bar_beats: f64,
    /// Whether clips, drops, fades and ruler clicks snap to the lines (not with `none`).
    pub snaps: bool,
}

impl Grid {
    /// The grid for the value `chosen` in the control bar, at `beat_width` points a beat with
    /// bars of `bar_beats` beats. Lines are the chosen step apart, or coarser where they would be
    /// closer than [`MIN_GRID_SPACING`]: a straight step doubles up to the beat, a triplet goes
    /// to the beat, then the beat goes to the bar and bars double. `none` draws beats and bars
    /// by the zoom alone and does not snap.
    pub fn new(chosen: Quantization, beat_width: f32, bar_beats: f64) -> Self {
        let bar_beats = if bar_beats.is_finite() && bar_beats > 0.0 { bar_beats } else { 4.0 };
        let chosen_step = chosen.step(bar_beats);
        let snaps = chosen_step > 0.0;
        let mut step = if snaps { chosen_step } else { 1.0 };
        // A note value longer than a bar that is not a whole number of bars (a half note in
        // 3/8) is drawn on the bar lines instead, so lines stay on the bar.
        if step > bar_beats + EPSILON && !is_whole(step / bar_beats) {
            step = bar_beats;
        }
        let points = f64::from(beat_width.max(f32::MIN_POSITIVE));
        let min = f64::from(MIN_GRID_SPACING);
        while step * points < min && step < MAX_GRID_BARS * bar_beats {
            step = if step >= bar_beats - EPSILON {
                step * 2.0
            } else if step < 1.0 - EPSILON && is_whole((1.0 / step).log2()) {
                // A straight note value finer than a beat: 1/32, 1/16, 1/8, 1/4.
                (step * 2.0).min(1.0)
            } else if step < 1.0 - EPSILON {
                // A triplet finer than a beat goes to the beat, whose lines are on the bar.
                1.0
            } else {
                bar_beats
            };
            // A bar shorter than a beat (2/8): the beat is coarser than the bar, so go by bars.
            if step > bar_beats + EPSILON && !is_whole(step / bar_beats) {
                step = bar_beats;
            }
        }
        Self { step, bar_beats, snaps }
    }

    /// Whether the step is one bar or more, so lines are only on bar lines.
    fn in_bars(&self) -> bool {
        self.step >= self.bar_beats - EPSILON
    }

    /// The grid line nearest to `beat` (never before the start), or `beat` itself with `free`
    /// (Alt held) or a grid that does not snap.
    pub fn place(&self, beat: f64, free: bool) -> f64 {
        if !beat.is_finite() {
            return 0.0;
        }
        let beat = beat.max(0.0);
        if free || !self.snaps || self.step <= 0.0 {
            return beat;
        }
        if self.in_bars() {
            return (beat / self.step).round() * self.step;
        }
        // Inside a bar: lines from the bar line on, and the next bar line.
        let bar_start = (beat / self.bar_beats).floor() * self.bar_beats;
        let next_bar = bar_start + self.bar_beats;
        let on_step = bar_start + ((beat - bar_start) / self.step).round() * self.step;
        if on_step >= next_bar - EPSILON || (next_bar - beat) < (beat - on_step).abs() { next_bar } else { on_step }
    }

    /// The start of the grid cell `beat` is in (never before the start): the line at or before
    /// it, counted from its bar line as `place` counts, so a step that does not fill the bar (a
    /// two-beat step in 3/4) starts again at each bar. `beat` itself with `free` or no snapping.
    pub fn cell(&self, beat: f64, free: bool) -> f64 {
        if !beat.is_finite() {
            return 0.0;
        }
        let beat = beat.max(0.0);
        if free || !self.snaps || self.step <= 0.0 {
            return beat;
        }
        if self.in_bars() {
            return (beat / self.step + EPSILON).floor() * self.step;
        }
        let bar_start = (beat / self.bar_beats + EPSILON).floor() * self.bar_beats;
        bar_start + ((beat - bar_start) / self.step + EPSILON).floor() * self.step
    }

    /// The beats of the lines from `first` to `last` (both included), each with whether it is a
    /// bar line.
    /// At most `limit` lines, so a bad zoom can never draw without end.
    pub fn lines(&self, first: f64, last: f64, limit: usize) -> Vec<(f64, bool)> {
        let mut lines = Vec::new();
        if !(first.is_finite() && last.is_finite()) || self.step <= 0.0 {
            return lines;
        }
        let first = first.max(0.0);
        if self.in_bars() {
            let mut n = (first / self.step).floor();
            while lines.len() < limit {
                let beat = n * self.step;
                if beat > last {
                    break;
                }
                lines.push((beat, true));
                n += 1.0;
            }
            return lines;
        }
        let mut bar = (first / self.bar_beats).floor();
        'bars: while lines.len() < limit {
            let bar_start = bar * self.bar_beats;
            if bar_start > last {
                break;
            }
            if bar_start >= first - EPSILON {
                lines.push((bar_start, true));
            }
            let mut k = ((first - bar_start) / self.step).ceil().max(1.0);
            loop {
                let beat = bar_start + k * self.step;
                if beat >= bar_start + self.bar_beats - EPSILON {
                    break;
                }
                if lines.len() >= limit || beat > last {
                    break 'bars;
                }
                lines.push((beat, false));
                k += 1.0;
            }
            bar += 1.0;
        }
        lines
    }
}

/// Beats closer than this are the same grid line.
const EPSILON: f64 = 1e-9;

fn is_whole(value: f64) -> bool {
    value.is_finite() && (value - value.round()).abs() < 1e-6
}

/// Where a click `raw` beats into the ruler puts the play start marker: on `grid`, or where it
/// was with `free` (Alt held); never before the start.
fn ruler_beat(raw: f64, grid: Grid, free: bool) -> f64 {
    let free = free || !grid.snaps;
    let beat = grid.place(raw, free);
    // Free clicks keep a thousandth of a beat, as the engine reports it.
    let beat = if free { (beat * 1000.0).round() / 1000.0 } else { beat };
    if beat.is_finite() { beat.max(0.0) } else { 0.0 }
}

/// A fade's length after a drag of `raw` beats: its end on the grid (unless `grid` is `None`,
/// Alt held, or a grid that does not snap), and no longer than the clip leaves beside the other
/// fade. No fade and all the room
/// there is snap too, so a clip that does not start or end on the grid can still be faded over
/// its whole length.
fn fade_length(raw: f64, edge: FadeEdge, clip: &ClipBox, other: f64, grid: Option<Grid>) -> f64 {
    if !raw.is_finite() {
        return 0.0;
    }
    let end = clip.start + clip.length;
    let room = (clip.length - other).max(0.0);
    let snapped = match grid.filter(|g| g.snaps) {
        Some(g) => {
            let on_grid = match edge {
                FadeEdge::In => g.place(clip.start + raw, false) - clip.start,
                FadeEdge::Out => end - g.place(end - raw, false),
            };
            [0.0, room].into_iter().fold(on_grid, |best, v| if (v - raw).abs() < (best - raw).abs() { v } else { best })
        }
        None => raw,
    };
    if snapped.is_finite() { snapped.clamp(0.0, room) } else { 0.0 }
}

/// The gain a clip's fades give `beats` into a clip `length` beats long, each fade following its
/// shape and curve, 0 … 1 (`docs/spec/arrangement-editing.md` › Fades).
fn fade_gain(beats: f64, length: f64, shown: &Shown) -> f32 {
    let fade_in = if shown.fade_in > 0.0 { shown.fade_in_curve.gain(beats / shown.fade_in) } else { 1.0 };
    let fade_out =
        if shown.fade_out > 0.0 { shown.fade_out_curve.gain((length - beats) / shown.fade_out) } else { 1.0 };
    (fade_in.clamp(0.0, 1.0) * fade_out.clamp(0.0, 1.0)) as f32
}

/// What a fade shape is called in its menu.
fn shape_label(shape: FadeShape) -> &'static str {
    match shape {
        FadeShape::Linear => "Linear",
        FadeShape::EqualPower => "Equal power",
        FadeShape::Fast => "Fast",
        FadeShape::Slow => "Slow",
        FadeShape::SCurve => "S-curve",
    }
}

/// The curve that puts a fade's curve handle at height `y` in the clip `rect`: the handle sits on
/// the fade's line half-way through it, so it follows the pointer up and down until the curve
/// reaches −1 or +1.
fn curve_at(rect: Rect, shape: FadeShape, y: f32) -> f64 {
    let (top, bottom) = (rect.top() + 2.0, rect.bottom());
    let gain = if bottom > top { f64::from((bottom - y) / (bottom - top)) } else { f64::NAN };
    FadeCurve::through(shape, gain)
}

/// Most points a fade's line is drawn with, however wide the fade is on screen.
const FADE_LINE_POINTS: usize = 256;

/// The points of a fade's line in the clip `rect`: from the silent bottom corner to full level
/// at the top, following `curve`, from `from` to `to` (left to right; `to` is the silent end for
/// a fade-out). Sampled every 2 points, at most [`FADE_LINE_POINTS`].
fn fade_line(rect: Rect, edge: FadeEdge, from: f32, to: f32, curve: FadeCurve) -> Vec<Pos2> {
    let (top, bottom) = (rect.top() + 2.0, rect.bottom());
    let width = (to - from).max(0.0);
    let steps = ((width / 2.0).ceil() as usize).clamp(1, FADE_LINE_POINTS - 1);
    (0..=steps)
        .map(|i| {
            let x = from + width * i as f32 / steps as f32;
            let along = if width > 0.0 { f64::from((x - from) / width) } else { 1.0 };
            let t = if edge == FadeEdge::In { along } else { 1.0 - along };
            Pos2::new(x, bottom - curve.gain(t) as f32 * (bottom - top))
        })
        .collect()
}

/// Where a fade's curve handle sits: on its line half-way through the fade.
fn curve_handle(rect: Rect, edge: FadeEdge, fade: f32, curve: FadeCurve) -> Pos2 {
    let (top, bottom) = (rect.top() + 2.0, rect.bottom());
    let x = match edge {
        FadeEdge::In => rect.left() + fade / 2.0,
        FadeEdge::Out => rect.right() - fade / 2.0,
    };
    Pos2::new(x, bottom - curve.gain(0.5) as f32 * (bottom - top))
}

fn rgb(color: suhina_engine::model::Rgb) -> Color32 {
    Color32::from_rgb(color.0, color.1, color.2)
}

/// Draws the Arrangement view in the space left in `ui` and adds what the user asked for to
/// `actions`.
pub fn show(ui: &mut Ui, timeline: &Timeline, state: &mut ArrangementViewState, actions: &mut Vec<Action>) {
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
    let returns: Vec<&Lane> =
        timeline.lanes.iter().filter(|l| l.kind == ColumnKind::Return && focus.is_none()).collect();
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
    // wheel alone scrolls. The wheel over a value field is the field's (`value_areas`), so the
    // view leaves it alone.
    let fields = std::mem::take(&mut state.value_areas);
    if let Some(pointer) = widgets::pointer_over(ui, area) {
        let over_field = fields.iter().any(|rect| rect.contains(pointer));
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
    let grid = Grid::new(state.grid, state.beat_width, bar_beats);
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
    // Grid in the lanes: a line at every step of the grid the control bar's value sets, coarser
    // when zoomed out; bar lines stronger than the lines between them. When the time signature
    // changes, the bars come from the meter instead: a line at every bar and, zoomed in, at
    // every beat.
    if meter.is_constant() {
        let max_lines = (time.width() / MIN_GRID_SPACING).ceil().max(1.0) as usize * 2 + 4;
        for (beat, on_bar) in grid.lines(first_beat, last_beat, max_lines) {
            let x = x_of(beat).round() + 0.5;
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
        state.time_selection = (end - start > 1e-6).then_some(TimeSelection { tracks, start, end });
        if ui.input(|i| !i.pointer.primary_down()) {
            state.ruler_select = None;
        }
    }
    let beat_at = |x: f32, free: bool, state: &ArrangementViewState| {
        ruler_beat(
            f64::from((x - time.left() + state.scroll.x) / state.beat_width),
            Grid::new(state.grid, state.beat_width, bar_beats),
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
    let mut clip_order: Vec<(TrackId, ClipId)> = Vec::new();
    let mut track_order: Vec<TrackId> = Vec::new();
    for shown in &drawn {
        if let Some(lane) = timeline.lanes.get(shown.lane) {
            track_order.push(lane.track);
        }
        let Some(clips_lane) = timeline.lanes.get(shown.clips) else { continue };
        clip_order.extend(clips_lane.clips.iter().map(|clip| (clips_lane.track, clip.id)));
    }

    // Lanes.
    state.automation.follow_clip(state.selected_clip());
    let geometry = automation::Geometry {
        time: time.x_range(),
        headers,
        visible: lanes_area,
        scroll_x: state.scroll.x,
        beat_width: state.beat_width,
        grid: {
            let grid = Grid::new(state.grid, state.beat_width, bar_beats);
            if grid.snaps { grid.step } else { 0.0 }
        },
    };
    let lanes_painter = painter.with_clip_rect(lanes_area);
    let mut track_number = 0;
    let mut ghost = None;
    let mut rows_seen: Vec<(TrackId, Rect)> = Vec::new();
    let mut clip_rects: Vec<((TrackId, ClipId), Rect)> = Vec::new();
    for shown in &drawn {
        let Some(lane) = timeline.lanes.get(shown.lane) else { continue };
        // A folded group's row draws the clips of the track it stands in for, and that track's
        // colour with them, while its header stays the group's.
        let Some(clips_lane) = timeline.lanes.get(shown.clips) else { continue };
        track_number += 1;
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
            state.marquee_time =
                if state.time_select_ctrl { ui.input(|i| i.modifiers.command) } else { origin.y < row.time.center().y };
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
            let grid = Grid::new(state.grid, state.beat_width, bar_beats);
            state.menu_beat = Some(grid.cell(raw, ui.input(|i| i.modifiers.alt)));
        }
        body.context_menu(|ui| {
            empty_lane_menu(ui, clips_lane.track, clips_lane.notes, state, actions);
        });
        body.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Lane {}", lane.name)));
        if body.clicked() {
            state.clear_selected_clip();
            state.time_selection = None;
            // The click also moves the play start marker there, on the clip grid (Alt: free), as
            // a click on the ruler does; while playing, playback jumps there.
            if let Some(pointer) = body.interact_pointer_pos() {
                let free = ui.input(|i| i.modifiers.alt);
                let raw = f64::from((pointer.x - time.left() + state.scroll.x) / state.beat_width);
                actions.push(Action::new("transport.setPosition", json!({ "beats": ruler_beat(raw, grid, free) })));
            }
        }
        if (lane.notes || lane.plays_audio)
            && !state.double_click_does_nothing
            && body.double_clicked()
            && let Some(pointer) = body.interact_pointer_pos()
        {
            let raw = f64::from((pointer.x - time.left() + state.scroll.x) / state.beat_width);
            let grid = Grid::new(state.grid, state.beat_width, bar_beats);
            let start = grid.cell(raw, ui.input(|i| i.modifiers.alt));
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
        for clip in &clips_lane.clips {
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
    // The time selection, lit over its tracks' lanes.
    if let Some(selection) = &state.time_selection {
        // Within the timeline: the track headers beside it are not lit.
        let span = egui::Rangef::new(x_of(selection.start), x_of(selection.end)).intersection(time.x_range());
        for (_, row) in rows_seen.iter().filter(|(t, _)| selection.tracks.contains(t)) {
            if span.span() <= 0.0 {
                break;
            }
            // Plainly over the clips too: the accent, and a line at each edge.
            let lit = Rect::from_x_y_ranges(span, row.y_range());
            lanes_painter.rect_filled(lit, 0.0, theme::accent().gamma_multiply(0.38));
            let edge = egui::Stroke::new(1.5, theme::accent());
            lanes_painter.vline(lit.left(), lit.y_range(), edge);
            lanes_painter.vline(lit.right(), lit.y_range(), edge);
        }
    }
    // The selection rectangle: the clips it touches are selected when the button goes up, and
    // the span it covers becomes the time selection.
    if let Some(origin) = state.marquee
        && let Some(now) = ui.ctx().pointer_latest_pos()
    {
        let rect = Rect::from_two_pos(origin, now).intersect(lanes_area).intersect(time);
        let done = ui.input(|i| !i.pointer.primary_down());
        if state.marquee_time {
            // Painting time: the selection follows the pointer, on the grid (Alt: free), over the
            // tracks the drag crosses; the clips' selection is left alone.
            let grid = Grid::new(state.grid, state.beat_width, bar_beats);
            let free = ui.input(|i| i.modifiers.alt);
            let beat_of = |x: f32| grid.place(f64::from((x - time.left() + state.scroll.x) / state.beat_width), free);
            let tracks: Vec<TrackId> =
                rows_seen.iter().filter(|(_, r)| r.y_range().intersects(rect.y_range())).map(|(t, _)| *t).collect();
            let (start, end) = (beat_of(rect.left()), beat_of(rect.right()));
            state.time_selection =
                (end - start > 1e-6 && !tracks.is_empty()).then_some(TimeSelection { tracks, start, end });
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
                let picked: Vec<(TrackId, ClipId)> =
                    clip_rects.iter().filter(|(_, r)| r.intersects(rect)).map(|(key, _)| *key).collect();
                if picked.is_empty() {
                    state.selected.clear();
                    state.anchor = None;
                } else {
                    state.select_clips(picked);
                }
            }
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
        let mut preview = state.main_drag;
        match widgets::drag_value(ui, &response, &mut preview, db, 0.25, 1.0, VOLUME_RANGE, 0.0) {
            Drag::Moving(v) => {
                let first = state.main_drag.is_none();
                state.main_drag = Some(v);
                actions.push(Action::new("main.setVolume", json!({ "db": v, "continue": !first })));
            }
            Drag::Done(v) => {
                state.main_drag = None;
                actions.push(Action::new("main.setVolume", json!({ "db": v, "continue": true })));
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
        |x: f32, free: bool| grid.place(f64::from((x - time.left() + state.scroll.x) / state.beat_width), free);
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

/// The markers in the row under the ruler: each a line at its beat with its name beside it. A
/// click jumps there, a drag moves it (on the clip grid; Alt: free), a right-click renames or
/// deletes it (`docs/spec/arrangement-editing.md` › Markers).
fn marker_flags(
    ui: &Ui,
    timeline: &Timeline,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
    row: Rect,
    x_of: &dyn Fn(f64) -> f32,
) {
    let painter = ui.painter_at(row);
    let font = theme::ui_font(theme::TEXT_SMALL);
    let shown_beat = |state: &ArrangementViewState, index: usize, beat: f64| match state.marker_drag {
        Some((i, raw)) if i == index => {
            let free = ui.input(|input| input.modifiers.alt);
            let bar_beats = if timeline.bar_beats > 0.0 { timeline.bar_beats } else { 4.0 };
            ruler_beat(raw, Grid::new(state.grid, state.beat_width, bar_beats), free)
        }
        _ => beat,
    };
    for (n, marker) in timeline.markers.iter().enumerate() {
        let beat = shown_beat(state, marker.index, marker.beat);
        let x = x_of(beat).round() + 0.5;
        // The tag ends where the next marker starts, so names never cover each other.
        let next = timeline.markers.get(n + 1).map_or(f32::INFINITY, |m| x_of(shown_beat(state, m.index, m.beat)));
        let width = (widgets::text_width(ui, &marker.name, font.clone()) + 2.0 * 3.0 + 2.0)
            .clamp(MARKER.x, MARKER_TAG_MAX)
            .min((next - x - 1.0).max(MARKER.x));
        let tag = Rect::from_min_max(Pos2::new(x, row.top() + 1.0), Pos2::new(x + width, row.bottom() - 1.0));
        if tag.right() < row.left() || tag.left() > row.right() {
            continue;
        }
        let id = Id::new(("arrangement-marker", marker.index));
        let response = ui.interact(tag.expand2(Vec2::new(2.0, 0.0)), id, Sense::click_and_drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Marker {}", marker.name))
        });
        let fill = if state.marker_drag.is_some_and(|(i, _)| i == marker.index) {
            theme::control_active()
        } else if response.hovered() {
            theme::control_hover()
        } else {
            theme::control_background()
        };
        painter.rect_filled(tag, 0.0, fill);
        painter.vline(x, row.y_range(), theme::hairline(theme::text_secondary()));
        widgets::text_in(
            &painter,
            tag.shrink2(Vec2::new(3.0, 0.0)).translate(Vec2::new(1.0, 0.0)),
            Align2::LEFT_CENTER,
            &marker.name,
            font.clone(),
            theme::text_value(),
        );

        if response.clicked() {
            // A click loops from the marker to the next one and jumps there, so a section is
            // looped with one click; the last marker only jumps.
            if let Some(next) = timeline.markers[n + 1..].iter().find(|m| m.beat > marker.beat) {
                actions.push(Action::new(
                    "transport.setLoop",
                    json!({ "start": marker.beat, "end": next.beat, "enabled": true }),
                ));
            }
            actions.push(Action::new("marker.jump", json!({ "index": marker.index })));
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            state.marker_drag = Some((marker.index, marker.beat));
        }
        if response.dragged_by(egui::PointerButton::Primary)
            && let Some(pointer) = response.interact_pointer_pos()
            && let Some((i, raw)) = state.marker_drag.as_mut()
            && *i == marker.index
        {
            // Where the pointer is, in beats, unsnapped: snapping happens as it is shown.
            *raw = f64::from((pointer.x - row.left() + state.scroll.x) / state.beat_width);
        }
        if response.drag_stopped() && state.marker_drag.is_some_and(|(i, _)| i == marker.index) {
            let to = shown_beat(state, marker.index, marker.beat);
            state.marker_drag = None;
            if to != marker.beat {
                actions.push(Action::new("marker.move", json!({ "index": marker.index, "beat": to })));
            }
        }
        if response.secondary_clicked() {
            state.marker_name = Some((marker.index, marker.name.clone()));
        }
        response.context_menu(|ui| {
            let Some((index, name)) = state.marker_name.as_mut().filter(|(i, _)| *i == marker.index) else {
                ui.close();
                return;
            };
            let index = *index;
            let field = ui.add(egui::TextEdit::singleline(name).hint_text("Marker name").desired_width(140.0));
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Rename").clicked() || entered {
                let name = name.trim().to_string();
                if !name.is_empty() && name != marker.name {
                    actions.push(Action::new("marker.rename", json!({ "index": index, "name": name })));
                }
                ui.close();
            }
            if ui.button("Jump to marker").clicked() {
                actions.push(Action::new("marker.jump", json!({ "index": index })));
                ui.close();
            }
            if ui.button("Delete marker").clicked() {
                actions.push(Action::new("marker.delete", json!({ "index": index })));
                ui.close();
            }
        });
    }
    // A drag that lost its marker (the project changed under it) ends.
    if state.marker_drag.is_some() && !ui.input(|i| i.pointer.primary_down()) {
        state.marker_drag = None;
    }
}

/// The loop region as the row under the ruler draws it while `state.loop_drag` changes it, on the
/// ruler's grid (Alt: free). `None` while a new region is no wider than a grid step.
fn dragged_loop(
    timeline: &Timeline,
    state: &ArrangementViewState,
    free: bool,
    grab: LoopGrab,
    raw: f64,
) -> Option<(f64, f64)> {
    let bar_beats = if timeline.bar_beats > 0.0 { timeline.bar_beats } else { 4.0 };
    let snap = |beat: f64| ruler_beat(beat, Grid::new(state.grid, state.beat_width, bar_beats), free);
    let brace = timeline.loop_brace.map(|b| (b.start, b.end));
    let (start, end) = match (grab, brace) {
        (LoopGrab::New(anchor), _) => {
            let to = snap(raw);
            (anchor.min(to), anchor.max(to))
        }
        (LoopGrab::Start, Some((_, end))) => (snap(raw).min(end), end),
        (LoopGrab::End, Some((start, _))) => (start, snap(raw).max(start)),
        (LoopGrab::Body(into), Some((start, end))) => {
            let to = snap((raw - into).max(0.0));
            (to, to + (end - start))
        }
        _ => return None,
    };
    (end > start && start >= 0.0 && end <= suhina_engine::MAX_START_BEAT).then_some((start, end))
}

/// The loop brace in its row under the ruler: a bar from the loop start to the loop end, bright
/// while looping is on. Dragging on the row draws a new region and turns looping on; dragging an
/// end of the brace moves that end, dragging its middle moves it (on the ruler's grid; Alt:
/// free); a click on it turns looping on or off (`docs/spec/arrangement-editing.md` › Loop region).
fn loop_brace(
    ui: &Ui,
    timeline: &Timeline,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
    row: Rect,
    x_of: &dyn Fn(f64) -> f32,
) {
    let painter = ui.painter_at(row);
    let free = ui.input(|i| i.modifiers.alt);
    let raw_at = |x: f32, state: &ArrangementViewState| f64::from((x - row.left() + state.scroll.x) / state.beat_width);
    let response = ui.interact(row, Id::new("arrangement-loop-row"), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Loop brace"));

    // Which part of the brace is under `x`.
    let brace_x = timeline.loop_brace.map(|b| (x_of(b.start), x_of(b.end)));
    let part = |x: f32| -> Option<LoopGrab> {
        let (left, right) = brace_x?;
        if (x - left).abs() <= LOOP_GRAB {
            Some(LoopGrab::Start)
        } else if (x - right).abs() <= LOOP_GRAB {
            Some(LoopGrab::End)
        } else if (left..=right).contains(&x) {
            Some(LoopGrab::Body(0.0))
        } else {
            None
        }
    };
    if let Some(pointer) = response.hover_pos() {
        match part(pointer.x) {
            Some(LoopGrab::Start | LoopGrab::End) => ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal),
            Some(_) => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
            None => {}
        }
    }

    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(origin) = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos())
    {
        let raw = raw_at(origin.x, state);
        let bar_beats = if timeline.bar_beats > 0.0 { timeline.bar_beats } else { 4.0 };
        let grab = match (part(origin.x), timeline.loop_brace) {
            (Some(LoopGrab::Body(_)), Some(brace)) => LoopGrab::Body(raw - brace.start),
            (Some(grab), _) => grab,
            (None, _) => LoopGrab::New(ruler_beat(raw, Grid::new(state.grid, state.beat_width, bar_beats), free)),
        };
        state.loop_drag = Some((grab, raw));
    }
    if response.dragged_by(egui::PointerButton::Primary)
        && let Some(pointer) = response.interact_pointer_pos()
        && state.loop_drag.is_some()
    {
        let at = raw_at(pointer.x, state);
        if let Some((_, raw)) = state.loop_drag.as_mut() {
            *raw = at;
        }
    }
    let dragged = state.loop_drag.and_then(|(grab, raw)| dragged_loop(timeline, state, free, grab, raw));
    if response.drag_stopped()
        && let Some((grab, _)) = state.loop_drag.take()
        && let Some((start, end)) = dragged
    {
        let unchanged = timeline.loop_brace.is_some_and(|b| b.start == start && b.end == end);
        match grab {
            // A new region is meant to be heard: it turns looping on.
            LoopGrab::New(_) => {
                actions.push(Action::new("transport.setLoop", json!({ "start": start, "end": end, "enabled": true })));
            }
            _ if !unchanged => actions.push(Action::new("transport.setLoop", json!({ "start": start, "end": end }))),
            _ => {}
        }
    }
    if response.clicked()
        && let Some(pointer) = response.interact_pointer_pos()
        && part(pointer.x).is_some()
    {
        actions.push(Action::new("transport.setLoopEnabled", json!({})));
    }
    response.context_menu(|ui| {
        let on = timeline.loop_brace.is_some_and(|b| b.enabled);
        if ui.button(if on { "Turn loop off" } else { "Turn loop on" }).clicked() {
            actions.push(Action::new("transport.setLoopEnabled", json!({ "enabled": !on })));
            ui.close();
        }
    });
    // A drag that lost its row (the view changed under it) ends.
    if state.loop_drag.is_some() && !ui.input(|i| i.pointer.primary_down()) {
        state.loop_drag = None;
    }

    let shown = dragged.or_else(|| timeline.loop_brace.map(|b| (b.start, b.end)));
    let Some((start, end)) = shown else { return };
    let enabled = timeline.loop_brace.is_none_or(|b| b.enabled) || state.loop_drag.is_some();
    let (left, right) = (x_of(start).round(), x_of(end).round());
    let bar = Rect::from_min_max(
        Pos2::new(left, row.center().y - LOOP_BAR / 2.0),
        Pos2::new(right.max(left + 1.0), row.center().y + LOOP_BAR / 2.0),
    );
    let (fill, edge) = if enabled {
        (theme::control_active(), theme::text_secondary())
    } else {
        (theme::control_background(), theme::text_hint())
    };
    painter.rect_filled(bar, 0.0, fill);
    painter.rect_stroke(bar, 0.0, theme::hairline(edge), StrokeKind::Inside);
}

/// The Back to arrangement glyph: an arrow curving over the top from the right to the left, its
/// head pointing down the way the curve arrives — the launch returns to the arrangement it left.
fn back_glyph(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = egui::Stroke::new(1.1, color);
    let r = rect.height() * 0.42;
    let cx = rect.center().x;
    let cy = rect.top() + (rect.height() + r) / 2.0;
    let mut points = Vec::with_capacity(9);
    for step in 0u32..=8 {
        let angle = std::f32::consts::PI * (1.0 - step as f32 / 8.0);
        points.push(Pos2::new(cx + r * angle.cos(), cy - r * angle.sin()));
    }
    painter.add(egui::Shape::line(points, stroke));
    // The head at the left end, opening against the curve's arrival: pointing down.
    let tip = Pos2::new(cx - r, cy);
    painter.add(egui::Shape::line(vec![tip + Vec2::new(-2.6, -3.2), tip, tip + Vec2::new(2.6, -3.2)], stroke));
}

/// The part of a lane a clip in sub-row `row` of `rows` is drawn in.
fn sub_row(lane_time: Rect, row: usize, rows: usize) -> Rect {
    let rows = rows.max(1) as f32;
    let height = lane_time.height() / rows;
    let top = lane_time.top() + height * row as f32;
    Rect::from_x_y_ranges(lane_time.x_range(), egui::Rangef::new(top.round(), (top + height).round() - 1.0))
}

fn clip_rect(lane_time: Rect, start: f64, length: f64, x_of: &dyn Fn(f64) -> f32) -> Rect {
    let left = x_of(start).round();
    Rect::from_min_max(
        Pos2::new(left, lane_time.top()),
        Pos2::new((x_of(start + length).round() - 1.0).max(left + 1.0), lane_time.bottom()),
    )
}

/// One arrangement clip: draws it and moves it when dragged, along its lane or onto another.
/// Returns the clip and its start when it is being dragged onto another lane, to be drawn there.
#[allow(clippy::too_many_arguments)]
fn clip_box<'a>(
    ui: &Ui,
    painter: &egui::Painter,
    row: Row,
    lane: &'a Lane,
    clip: &'a ClipBox,
    x_of: &dyn Fn(f64) -> f32,
    bar_beats: f64,
    lane_at: &dyn Fn(Pos2) -> Option<TrackId>,
    timeline: &Timeline,
    clip_order: &[(TrackId, ClipId)],
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) -> Option<(&'a Lane, &'a ClipBox, f64)> {
    let key = (lane.track, clip.id);
    let mine = |drag: &ClipDrag| (drag.track, drag.clip) == key;
    // A moved clip starts on a grid line, or where it is let go with Alt held.
    let grid = Grid::new(state.grid, state.beat_width, bar_beats);
    let free = ui.input(|i| i.modifiers.alt);
    let snapped = |raw: f64| grid.place(raw, free);
    let at = match state.clip_drag {
        Some(drag) if mine(&drag) => snapped(drag.start),
        // Another clip of the selection is dragged: this one moves by as much.
        Some(drag) if state.is_selected(key) && state.is_selected((drag.track, drag.clip)) => {
            (clip.start + snapped(drag.start) - drag.origin).max(0.0)
        }
        _ => clip.start,
    };
    // Where it was last frame takes the pointer; it is drawn where this frame's drag puts it.
    let space = if clip.rows > 1 { sub_row(row.time, clip.row, clip.rows) } else { row.time };
    let visible = clip_rect(space, at, clip.length, x_of).intersect(row.clip);
    if visible.width() <= 0.0 || visible.height() <= 0.0 {
        return None;
    }
    let bar = (clip.start / bar_beats).floor() as u64 + 1;
    let name = if clip.name.is_empty() { "clip" } else { clip.name.as_str() };
    let response = ui.interact(visible, Id::new(("arrangement-clip", clip.id.0)), Sense::click_and_drag());
    // The cursor says what a drag does: the lower half moves the clip (a hand; with the copy key
    // held, the copy cursor), the upper half selects time (a text cursor).
    let copy_key = ui.input(|i| if state.time_select_ctrl { i.modifiers.shift } else { i.modifiers.command });
    let in_body = |y: f32| !state.time_select_ctrl && clip_grab_area(visible).is_some_and(|grab| y < grab.top());
    if state.clip_drag.is_some_and(|d| mine(&d) && (d.copy || copy_key)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
    } else if response.hover_pos().is_some_and(|p| in_body(p.y)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(if copy_key { egui::CursorIcon::Copy } else { egui::CursorIcon::Grab });
    }
    let label = format!("Clip {name} on {} at bar {bar}", lane.name);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let stack = if clip.audio { state.stack_audio } else { state.stack_midi };
    // A right-click remembers the beat under the pointer, for the menu's Split here.
    if response.secondary_clicked()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let raw = clip.start + f64::from((pointer.x - x_of(clip.start)) / state.beat_width);
        let on_grid = snapped(raw);
        let free = ui.input(|i| i.modifiers.alt) || on_grid <= clip.start || on_grid >= clip.start + clip.length;
        state.split_at = Some(if free { raw } else { on_grid });
    }
    let split_at = state.split_at;
    // The menu acts on the whole selection when this clip is part of it, else on this clip alone.
    let menu_clips: Vec<(TrackId, ClipId)> = if state.is_selected(key) { state.selected.clone() } else { vec![key] };
    // The menu stays open on a click inside: the rename's field and button live here. When it
    // is not open, its rename's draft dies: a reopen starts from the name as it is now.
    if !response.context_menu_opened() {
        ui.ctx().data_mut(|d| d.remove::<String>(Id::new(("clip-rename", key.1.0))));
    }
    egui::Popup::context_menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        menus::clip_menu(ui, &menu_clips, stack, clip.enabled, &clip.name, actions);
        let ara = state
            .ara_tracks
            .iter()
            .find(|(t, _)| *t == lane.track)
            .map(|(_, name)| name.as_str())
            .or(state.ara_default.as_deref());
        // Consolidate joins every clip of the menu's on this track, from the first start to the
        // last end, so two selected clips become one.
        let joined = menu_clips
            .iter()
            .filter(|(track, _)| *track == lane.track)
            .filter_map(|(_, id)| lane.clips.iter().find(|c| c.id == *id))
            .fold((clip.start, clip.start + clip.length), |(s, e), c| (s.min(c.start), e.max(c.start + c.length)));
        menus::clip_edit_menu(ui, lane.track, clip.id, (clip.start, clip.length), joined, split_at, actions);
        if clip.audio
            && let Some(name) = ara
        {
            menus::ara_menu(ui, lane.track, clip.id, name, actions);
        }
    });
    let (shift, command) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
    // A drag that selects time rather than moving the clip: with Ctrl held, or from the clip's
    // lower half on a lane tall enough to have one, as the setting says.
    let selects_time = response.drag_started_by(egui::PointerButton::Primary)
        && ui.input(|i| i.pointer.press_origin()).is_some_and(|origin| {
            if state.time_select_ctrl {
                command
            } else {
                clip_grab_area(visible).is_some_and(|grab| origin.y < grab.top())
            }
        });
    if selects_time {
        state.marquee = ui.input(|i| i.pointer.press_origin());
        state.marquee_time = true;
    } else if response.clicked() || response.drag_started_by(egui::PointerButton::Primary) {
        // Clips picked by a click or moved: the time selection gives way to them.
        state.time_selection = None;
    }
    if response.clicked() {
        state.pick_clip(key, shift, command, clip_order);
        state.clip_clicked = true;
        // A click on a clip also moves the play position there, on the grid (Alt: free), as a
        // click on an empty lane does; an ARA plug-in's editor follows it (#717).
        if let Some(pointer) = response.interact_pointer_pos() {
            let free = ui.input(|i| i.modifiers.alt);
            let raw = f64::from((pointer.x - x_of(0.0)) / state.beat_width);
            actions.push(Action::new("transport.setPosition", json!({ "beats": ruler_beat(raw, grid, free) })));
        }
    } else if response.drag_started_by(egui::PointerButton::Primary) && !selects_time && !state.is_selected(key) {
        // Pressing a clip that is already selected keeps the selection, so a drag can start there.
        state.pick_clip(key, shift, command, clip_order);
    }
    if response.clicked() || (response.drag_started_by(egui::PointerButton::Primary) && !selects_time) {
        // The track being worked on: a folded group shows this track while it is one of its own.
        state.targeted = Some(lane.track);
    }
    if response.drag_started_by(egui::PointerButton::Primary) && !selects_time {
        state.clip_drag = Some(ClipDrag {
            track: lane.track,
            clip: clip.id,
            start: clip.start,
            target: lane.track,
            origin: clip.start,
            copy: copy_key,
        });
    }
    if response.dragged_by(egui::PointerButton::Primary)
        && let Some(drag) = state.clip_drag.as_mut()
        && (drag.track, drag.clip) == key
    {
        drag.start += f64::from(response.drag_delta().x / state.beat_width);
        if let Some(target) = response.interact_pointer_pos().and_then(lane_at) {
            drag.target = target;
        }
    }
    if response.drag_stopped_by(egui::PointerButton::Primary)
        && let Some(drag) = state.clip_drag.take()
        && (drag.track, drag.clip) == key
    {
        let start = snapped(drag.start);
        let stack = (clip.audio && state.stack_audio) || (!clip.audio && state.stack_midi);
        let moved = drag.target != lane.track || (start - clip.start).abs() > 1e-9;
        // Copied: the clips stay, and copies (each playing a content of its own) go where they
        // were dropped; a selection is copied whole.
        let copying = drag.copy || copy_key;
        if copying {
            let chosen: Vec<(TrackId, ClipId)> =
                if state.is_selected(key) && state.selected.len() > 1 { state.selected.clone() } else { vec![key] };
            let clips: Vec<Value> = chosen.iter().map(|(_, id)| json!({ "clip": id.0 })).collect();
            let mut params = json!({ "clips": clips, "beats": start - clip.start, "copy": true });
            if drag.target != lane.track && chosen.iter().all(|(track, _)| *track == lane.track) {
                params["to_track"] = json!(drag.target.0);
                params["color"] = json!(hex(clip.color));
            }
            if stack {
                params["stack"] = json!(true);
            }
            if moved {
                actions.push(Action::new("clip.moveMany", params));
            }
        } else if state.selected.len() > 1 && state.is_selected(key) {
            // The whole selection moves by as much, at once; to another track only when all of
            // it is on this one.
            let clips: Vec<Value> = state.selected.iter().map(|(_, id)| json!({ "clip": id.0 })).collect();
            let mut params = json!({ "clips": clips, "beats": start - clip.start });
            if drag.target != lane.track && state.selected.iter().all(|(track, _)| *track == lane.track) {
                params["to_track"] = json!(drag.target.0);
                params["color"] = json!(hex(lane.color));
            }
            if stack {
                params["stack"] = json!(true);
            }
            if moved {
                actions.push(Action::new("clip.moveMany", params));
            }
        } else {
            let mut params = json!({ "clip": clip.id.0, "start": start });
            if drag.target != lane.track {
                params["to_track"] = json!(drag.target.0);
                params["color"] = json!(hex(clip.color));
            }
            if stack {
                params["stack"] = json!(true);
            }
            if moved {
                actions.push(Action::new("clip.move", params));
            }
        }
        // Drawn where it was dropped until the project has moved it.
        let rect = clip_rect(space, start, clip.length, x_of);
        let shown = Shown::of(clip);
        paint_clip(
            &painter.with_clip_rect(row.clip),
            rect,
            lane,
            clip,
            shown,
            start,
            x_of,
            state.beat_width,
            timeline,
            true,
        );
        return None;
    }

    // What the clip shows: the project's values, or those of a drag in progress, so the
    // waveform follows the gain and the fades while they are dragged.
    let fade_grid = (!free).then_some(grid);
    let mut shown = state.shown(key, clip, fade_grid);

    // An audio clip big enough shows its gain, which a vertical drag changes.
    let mut gain_paint = None;
    if let Some(view) = &clip.audio_view
        && state.clip_drag.is_none_or(|d| !mine(&d))
    {
        let rect = clip_rect(space, clip.start, clip.length, x_of);
        let field = Rect::from_min_size(
            Pos2::new(rect.right() - GAIN_FIELD_INSET - GAIN_FIELD.x, rect.top() + 3.0),
            GAIN_FIELD,
        );
        if rect.width() >= 80.0 && rect.height() >= 30.0 && field.intersects(row.clip) {
            let label = format!("Gain of {name} on {}", lane.name);
            state.value_areas.push(field.intersect(row.clip));
            let response = widgets::value_area(
                ui,
                field.intersect(row.clip),
                Id::new(("clip-gain", clip.id.0)),
                &label,
                widgets::format_db(shown.gain_db),
            );
            let mut preview = match state.gain_drag {
                Some((k, v)) if k == key => Some(v),
                _ => None,
            };
            let mut committed = None;
            match widgets::drag_value(ui, &response, &mut preview, view.gain_db, 0.1, 1.0, CLIP_GAIN_RANGE, 0.0) {
                Drag::Moving(v) => state.gain_drag = Some((key, v)),
                Drag::Done(v) => {
                    state.gain_drag = None;
                    committed = Some(v);
                    let db = if v <= CLIP_GAIN_RANGE.0 { json!("-inf") } else { json!((v * 10.0).round() / 10.0) };
                    actions.push(Action::new("clip.setGain", json!({ "clip": clip.id.0, "db": db })));
                }
                Drag::None => {}
            }
            shown.gain_db = match state.gain_drag {
                Some((k, v)) if k == key => v,
                _ => committed.unwrap_or(view.gain_db),
            };
            gain_paint = Some((field, shown.gain_db, widgets::hover(ui, &response)));
        }
    }

    // A clip's edges: the left or right edge dragged moves it (on the grid; Shift: free), the
    // content staying where it plays; with Alt an audio clip's audio stretches to the new length
    // instead. The handle at the lower right turns the loop on and makes the clip longer
    // (`clip.trim`, `clip.stretch`; `docs/spec/arrangement-editing.md` › Clip edges). An
    // audio clip keeps its top corners for the fade handles.
    let edge_rect = clip_rect(space, clip.start, clip.length, x_of);
    if state.clip_drag.is_none_or(|d| !mine(&d)) && edge_rect.width() >= 12.0 {
        const ZONE: f32 = 5.0;
        const CORNER: f32 = 10.0;
        let top = edge_rect.top() + if clip.audio { FADE_HANDLE + 2.0 } else { 2.0 };
        let (shift, alt) = ui.input(|i| (i.modifiers.shift, i.modifiers.alt));
        for grab in [EdgeGrab::Left, EdgeGrab::Right, EdgeGrab::Loop] {
            let hit = match grab {
                EdgeGrab::Left => Rect::from_min_max(
                    Pos2::new(edge_rect.left(), top),
                    Pos2::new(edge_rect.left() + ZONE, edge_rect.bottom()),
                ),
                EdgeGrab::Right => Rect::from_min_max(
                    Pos2::new(edge_rect.right() - ZONE, top),
                    Pos2::new(edge_rect.right(), edge_rect.bottom() - CORNER),
                ),
                EdgeGrab::Loop => Rect::from_min_max(
                    Pos2::new(edge_rect.right() - CORNER, edge_rect.bottom() - CORNER),
                    edge_rect.max,
                ),
            }
            .intersect(row.clip);
            if hit.width() <= 0.0 || hit.height() <= 0.0 {
                continue;
            }
            let id = Id::new(("clip-edge", clip.id.0, grab as u8));
            let response = ui.interact(hit, id, Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            let what = match grab {
                EdgeGrab::Left => "Left edge",
                EdgeGrab::Right => "Right edge",
                EdgeGrab::Loop => "Loop handle",
            };
            let label = format!("{what} of {name} on {}", lane.name);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, &label));
            let mine_edge = |d: &EdgeDrag| d.key == key && d.grab == grab;
            if response.drag_started_by(egui::PointerButton::Primary) {
                let raw = if grab == EdgeGrab::Left { clip.start } else { clip.start + clip.length };
                state.edge_drag = Some(EdgeDrag { key, grab, raw });
            }
            if response.dragged_by(egui::PointerButton::Primary)
                && let Some(drag) = state.edge_drag.as_mut().filter(|d| mine_edge(d))
            {
                drag.raw += f64::from(response.drag_delta().x / state.beat_width);
            }
            if response.drag_stopped_by(egui::PointerButton::Primary)
                && let Some(drag) = state.edge_drag.take_if(|d| mine_edge(d))
            {
                let (from, to) = edge_extent(clip, drag, &grid, shift);
                let beat = if grab == EdgeGrab::Left { from } else { to };
                let stretch = alt && state.stretch_alt && clip.audio && grab != EdgeGrab::Loop;
                let mut params = json!({ "clip": clip.id.0 });
                params[if grab == EdgeGrab::Left { "start" } else { "end" }] = json!(beat);
                if grab == EdgeGrab::Loop {
                    params["loop"] = json!(true);
                }
                if (clip.audio && state.stack_audio) || (!clip.audio && state.stack_midi) {
                    params["stack"] = json!(true);
                }
                let old = if grab == EdgeGrab::Left { clip.start } else { clip.start + clip.length };
                if (beat - old).abs() > 1e-9 || grab == EdgeGrab::Loop {
                    actions.push(Action::new(if stretch { "clip.stretch" } else { "clip.trim" }, params));
                }
            }
        }
    }

    // Fade handles at the top corners of an audio clip, while it is hovered or selected: a
    // horizontal drag sets the fade (its end on the grid, free with Alt), a double-click
    // removes it, a right-click chooses its shape.
    let mut handles = Vec::new();
    let mut curves = Vec::new();
    let rect = clip_rect(space, clip.start, clip.length, x_of);
    let selected = state.is_selected(key);
    let fading = state.fade_drag.is_some_and(|d| d.key == key) || state.curve_drag.is_some_and(|d| d.key == key);
    let pointer_in = widgets::pointer_over(ui, visible).is_some();
    let fits = rect.width() >= 3.0 * FADE_HANDLE && rect.height() >= 2.0 * FADE_HANDLE;
    if clip.audio && fits && state.clip_drag.is_none_or(|d| !mine(&d)) && (pointer_in || selected || fading) {
        let grid = fade_grid;
        for edge in [FadeEdge::In, FadeEdge::Out] {
            let (current, now, other) = match edge {
                FadeEdge::In => (clip.fade_in, shown.fade_in, shown.fade_out),
                FadeEdge::Out => (clip.fade_out, shown.fade_out, shown.fade_in),
            };
            let handle = fade_handle(rect, edge, now as f32 * state.beat_width);
            let hit = handle.expand((FADE_HANDLE_HIT - FADE_HANDLE) / 2.0).intersect(row.clip);
            if hit.width() <= 0.0 || hit.height() <= 0.0 {
                continue;
            }
            let which = if edge == FadeEdge::In { "Fade-in" } else { "Fade-out" };
            let label = format!("{which} of {name} on {}", lane.name);
            let id = Id::new(("clip-fade", clip.id.0, edge == FadeEdge::In));
            let response =
                ui.interact(hit, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            response.widget_info(|| {
                let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, &label);
                info.value = Some(now);
                info
            });
            let mine_fade = |d: &FadeDrag| d.key == key && d.edge == edge;
            let mut done = None;
            if response.double_clicked() {
                state.fade_drag = None;
                done = Some(0.0);
            } else {
                if response.drag_started_by(egui::PointerButton::Primary) {
                    state.fade_drag = Some(FadeDrag { key, edge, raw: current });
                }
                if response.dragged_by(egui::PointerButton::Primary)
                    && let Some(drag) = state.fade_drag.as_mut().filter(|d| mine_fade(d))
                {
                    let dx = f64::from(response.drag_delta().x / state.beat_width);
                    drag.raw += if edge == FadeEdge::In { dx } else { -dx };
                }
                if response.drag_stopped_by(egui::PointerButton::Primary)
                    && let Some(drag) = state.fade_drag.take_if(|d| mine_fade(d))
                {
                    done = Some(fade_length(drag.raw, edge, clip, other, grid));
                }
            }
            let length = match (done, state.fade_drag) {
                (Some(v), _) => v,
                (None, Some(d)) if mine_fade(&d) => fade_length(d.raw, edge, clip, other, grid),
                _ => now,
            };
            if let Some(v) = done
                && (v - current).abs() > 1e-9
            {
                let param = if edge == FadeEdge::In { "in" } else { "out" };
                let mut params = json!({ "clip": clip.id.0 });
                params[param] = json!((v * 1e6).round() / 1e6);
                actions.push(Action::new("clip.setFades", params));
            }
            match edge {
                FadeEdge::In => shown.fade_in = length,
                FadeEdge::Out => shown.fade_out = length,
            }
            // A right-click chooses the fade's shape.
            let shape = shown.curve(edge).shape;
            response.context_menu(|ui| {
                for choice in FadeShape::ALL {
                    if ui.radio(choice == shape, shape_label(choice)).clicked() {
                        let param = if edge == FadeEdge::In { "inShape" } else { "outShape" };
                        let mut params = json!({ "clip": clip.id.0 });
                        params[param] = json!(choice.name());
                        actions.push(Action::new("clip.setFades", params));
                        ui.close();
                    }
                }
            });
            handles.push((edge, widgets::hover(ui, &response)));

            // The curve handle, half-way along a fade wide enough: a vertical drag bends the
            // curve, a double-click sets it back to 0.
            let fade_width = length as f32 * state.beat_width;
            if fade_width < CURVE_HANDLE_MIN_FADE || state.fade_drag.is_some_and(|d| mine_fade(&d)) {
                continue;
            }
            let current_curve = shown.curve(edge);
            let center = curve_handle(rect, edge, fade_width, current_curve);
            let hit = Rect::from_center_size(center, Vec2::splat(FADE_HANDLE_HIT)).intersect(row.clip);
            if hit.width() <= 0.0 || hit.height() <= 0.0 {
                continue;
            }
            let label = format!("{which} curve of {name} on {}", lane.name);
            let id = Id::new(("clip-fade-curve", clip.id.0, edge == FadeEdge::In));
            let response =
                ui.interact(hit, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeVertical);
            response.widget_info(|| {
                let mut info = egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, &label);
                info.value = Some(current_curve.curve);
                info
            });
            let mine_curve = |d: &CurveDrag| d.key == key && d.edge == edge;
            let mut done = None;
            if response.double_clicked() {
                state.curve_drag = None;
                done = Some(0.0);
            } else {
                if response.drag_started_by(egui::PointerButton::Primary) {
                    // Where the button went down, not where the drag was recognised: the handle
                    // keeps its place under the pointer.
                    let pressed = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos());
                    let grab = pressed.map_or(0.0, |p| p.y - center.y);
                    state.curve_drag = Some(CurveDrag { key, edge, curve: current_curve.curve, grab });
                }
                if response.dragged_by(egui::PointerButton::Primary)
                    && let Some(pointer) = response.interact_pointer_pos()
                    && let Some(drag) = state.curve_drag.as_mut().filter(|d| mine_curve(d))
                {
                    drag.curve = curve_at(rect, current_curve.shape, pointer.y - drag.grab);
                }
                if response.drag_stopped_by(egui::PointerButton::Primary)
                    && let Some(drag) = state.curve_drag.take_if(|d| mine_curve(d))
                {
                    done = Some(drag.curve);
                }
            }
            let stored = match edge {
                FadeEdge::In => clip.fade_in_curve.curve,
                FadeEdge::Out => clip.fade_out_curve.curve,
            };
            if let Some(v) = done {
                let v = (v * 1000.0).round() / 1000.0;
                if (v - stored).abs() > 1e-9 {
                    let param = if edge == FadeEdge::In { "inCurve" } else { "outCurve" };
                    let mut params = json!({ "clip": clip.id.0 });
                    params[param] = json!(v);
                    actions.push(Action::new("clip.setFades", params));
                }
                match edge {
                    FadeEdge::In => shown.fade_in_curve.curve = v,
                    FadeEdge::Out => shown.fade_out_curve.curve = v,
                }
            }
            curves.push((edge, widgets::hover(ui, &response)));
        }
    }

    // Copying (Ctrl held): the clip stays where it is drawn, and its copy shows where it lands.
    let copying = state.clip_drag.is_some_and(|d| d.copy || copy_key) && (at - clip.start).abs() > 1e-9;
    let paint_at = |start: f64| {
        paint_clip(
            &painter.with_clip_rect(row.clip),
            clip_rect(space, start, clip.length, x_of),
            lane,
            clip,
            shown,
            start,
            x_of,
            state.beat_width,
            timeline,
            selected,
        );
    };
    let result = match state.clip_drag {
        Some(drag) if mine(&drag) && drag.target != lane.track => {
            if drag.copy || copy_key {
                paint_at(clip.start);
            }
            Some((lane, clip, snapped(drag.start)))
        }
        _ if copying => {
            paint_at(clip.start);
            paint_at(at);
            None
        }
        _ => {
            // Where the drag puts it (the dragged clip and the rest of its selection alike).
            let start = at;
            // An edge being dragged: the clip is drawn at the length it has so far, its content
            // where it plays.
            let shift = ui.input(|i| i.modifiers.shift);
            let (rect, clip_area) = match state.edge_drag.filter(|d| d.key == key) {
                Some(edge) => {
                    let (from, to) = edge_extent(clip, edge, &grid, shift);
                    let rect = clip_rect(space, from, to - from, x_of);
                    (rect, row.clip.intersect(rect))
                }
                None => (clip_rect(space, start, clip.length, x_of), row.clip),
            };
            paint_clip(
                &painter.with_clip_rect(clip_area),
                rect,
                lane,
                clip,
                shown,
                start,
                x_of,
                state.beat_width,
                timeline,
                selected,
            );
            None
        }
    };
    // The gain sits on top of the clip.
    if let Some((field, db, hover)) = gain_paint {
        let gain_painter = painter.with_clip_rect(row.clip);
        let fill = motion::mix(theme::control_background(), theme::control_hover(), hover);
        gain_painter.rect_filled(field, 0.0, fill);
        let text = format!("{} dB", widgets::format_db(db));
        let font = theme::mono_font(theme::TEXT_TINY);
        widgets::text_in(
            &gain_painter,
            field.shrink2(Vec2::new(3.0, 0.0)),
            Align2::RIGHT_CENTER,
            &text,
            font,
            theme::text_value(),
        );
    }
    // The handles sit on top of everything on the clip, where the fades now end.
    let handle_painter = painter.with_clip_rect(row.clip);
    for (edge, hover) in handles {
        let length = if edge == FadeEdge::In { shown.fade_in } else { shown.fade_out };
        let handle = fade_handle(rect, edge, length as f32 * state.beat_width);
        let fill = motion::mix(theme::text_secondary(), theme::text_primary(), hover);
        handle_painter.rect_filled(handle, 0.0, fill);
        handle_painter.rect_stroke(handle, 0.0, theme::hairline(theme::window_background()), StrokeKind::Outside);
    }
    for (edge, hover) in curves {
        let length = if edge == FadeEdge::In { shown.fade_in } else { shown.fade_out };
        let center = curve_handle(rect, edge, length as f32 * state.beat_width, shown.curve(edge));
        let fill = motion::mix(theme::text_secondary(), theme::text_primary(), hover);
        handle_painter.circle(center, CURVE_HANDLE / 2.0, fill, theme::hairline(theme::window_background()));
    }
    result
}

/// The square of a fade handle: at the clip's top corner with no fade, at the fade's end along
/// the top edge with one, `fade` points from the clip's edge; always inside the clip.
fn fade_handle(clip: Rect, edge: FadeEdge, fade: f32) -> Rect {
    let fade = if fade.is_finite() { fade.max(0.0) } else { 0.0 };
    let left = match edge {
        FadeEdge::In => (clip.left() + fade).min(clip.right() - FADE_HANDLE),
        FadeEdge::Out => (clip.right() - fade - FADE_HANDLE).max(clip.left()),
    };
    Rect::from_min_size(Pos2::new(left.round(), clip.top()), Vec2::splat(FADE_HANDLE))
}

fn view_rect(painter: &egui::Painter) -> Rect {
    painter.clip_rect()
}

/// Draws the notes an ARA plug-in found in an audio clip drawn at beat `start`: bars from each
/// note's start to its end, its pitch between the clip's lowest and highest (a row spare at each
/// side), more opaque the louder. Times come in seconds from the clip's start and go to beats
/// through the tempo map; only the notes in view, and in the clip, are drawn.
#[allow(clippy::too_many_arguments)]
fn paint_ara_notes(
    painter: &egui::Painter,
    rect: Rect,
    marks: Rect,
    notes: &suhina_session::ClipNotes,
    tempo: &suhina_engine::model::TempoMap,
    start: f64,
    length: f64,
    x_of: &dyn Fn(f64) -> f32,
    beat_width: f32,
    ink: Color32,
) {
    let view = painter.clip_rect().intersect(rect);
    if !view.is_positive() {
        return;
    }
    let at = tempo.time_at(start);
    let beat_at_x = |x: f32| start + f64::from((x - x_of(start)) / beat_width);
    let (first, last) = (tempo.time_at(beat_at_x(view.left())) - at, tempo.time_at(beat_at_x(view.right())) - at);
    let end = tempo.time_at(start + length) - at;
    let rows = f32::from(notes.high - notes.low) + 3.0;
    let height = (marks.height() / rows).clamp(2.0, 6.0);
    let painter = painter.with_clip_rect(view);
    for note in notes.notes.iter().filter(|n| n.start + n.length >= first.max(0.0) && n.start <= last.min(end)) {
        let from = x_of(tempo.beat_at(at + note.start.max(0.0)));
        let to = x_of(tempo.beat_at(at + (note.start + note.length).min(end))).max(from + 1.0);
        let row = f32::from(note.pitch - notes.low) + 1.0;
        let y = marks.bottom() - (row + 0.5) / rows * marks.height();
        let bar = Rect::from_min_max(Pos2::new(from, y - height / 2.0), Pos2::new(to, y + height / 2.0));
        painter.rect_filled(bar, 1.0, ink.gamma_multiply(0.45 + 0.55 * note.volume));
    }
}

/// `colour` more saturated and a little brighter, for a selected track.
fn saturated(colour: Color32) -> Color32 {
    let mut hsva = egui::ecolor::Hsva::from(colour);
    hsva.s = (hsva.s * 1.3).min(1.0);
    hsva.v = (hsva.v * 1.12).min(1.0);
    hsva.into()
}

/// Draws a clip's body, colour strip, name and the marks of its notes (only those in view).
#[allow(clippy::too_many_arguments)]
fn paint_clip(
    painter: &egui::Painter,
    rect: Rect,
    lane: &Lane,
    clip: &ClipBox,
    shown: Shown,
    start: f64,
    x_of: &dyn Fn(f64) -> f32,
    beat_width: f32,
    timeline: &Timeline,
    selected: bool,
) {
    let view = painter.clip_rect();
    if !rect.intersects(view) {
        return;
    }
    // The clip in its colour (its own, or its track's): the name bar in full colour, the body a
    // shade of it, and its notes or waveform in ink that reads on both. Off or muted, it greys.
    let off = lane.mute || !clip.enabled;
    let base = if off { motion::mix(rgb(clip.color), theme::text_hint(), 0.7) } else { rgb(clip.color) };
    let body = motion::mix(theme::window_background(), base, if selected { 0.85 } else { 0.62 });
    painter.rect_filled(rect, 0.0, body);
    let bar = clip_name_bar(rect);
    if let Some(bar) = bar {
        painter.rect_filled(bar, 0.0, base);
    } else {
        painter.rect_filled(Rect::from_min_size(rect.min, Vec2::new(rect.width(), 2.0)), 0.0, base);
    }
    let ink = ink_on(body);
    // Notes an ARA plug-in found in an audio clip go over its waveform, which dims under them
    // so both read (ADR 0028, #720).
    let ara_notes = timeline.ara_notes.get(&clip.id).filter(|_| clip.audio).zip(timeline.tempo.as_ref());
    let wave_ink = if ara_notes.is_some() { motion::mix(body, ink, 0.4) } else { ink };
    let dim = |_: Color32| wave_ink;
    let color = ink;
    let text_color = ink_on(if bar.is_some() { base } else { body });
    // The name's place, for a rename in place (Ctrl+R).
    let strip = Rect::from_min_max(
        Pos2::new(rect.left().max(view.left()), rect.top()),
        Pos2::new(rect.right().min(view.right()), rect.top() + 18.0),
    );
    let pass = painter.ctx().cumulative_pass_nr();
    painter.ctx().data_mut(|d| {
        d.insert_temp(Id::new((crate::app::NAME_RECT_CLIP, u64::from(clip.id.0))), (strip, pass));
    });
    let has_name = rect.height() >= 20.0;
    if has_name {
        // The name stays in view while the clip's start is scrolled off to the left.
        let left = rect.left().max(view.left()) + 4.0;
        let name_rect =
            Rect::from_min_max(Pos2::new(left, rect.top() + 3.0), Pos2::new(rect.right() - 2.0, rect.top() + 15.0));
        widgets::text_in(
            painter,
            name_rect,
            Align2::LEFT_CENTER,
            &clip.name,
            theme::ui_font(theme::TEXT_SMALL),
            text_color,
        );
    }
    let marks = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top() + if has_name && rect.height() >= 30.0 { 17.0 } else { 4.0 }),
        Pos2::new(rect.right(), rect.bottom() - 3.0),
    );
    if let Some(view) = clip.audio_view.as_ref().filter(|_| marks.height() >= 4.0)
        && let Some(peaks) = timeline.waveforms.get(&view.file)
    {
        // One column per point in view: the stretch of the file it shows, scaled by the clip's
        // gain and its fades, drawn as `docs/spec/waveforms.md` says, inside the clip.
        let gain = if shown.gain_db.is_finite() { 10f32.powf(shown.gain_db as f32 / 20.0) } else { 0.0 };
        let from = rect.left().max(view_rect(painter).left()).floor();
        let to = rect.right().min(view_rect(painter).right()).ceil();
        let per_point = 1.0 / f64::from(beat_width);
        let count = (to - from).max(0.0) as usize;
        let columns: Vec<waveform::Column> = (0..count)
            .map(|i| {
                let x = from + i as f32;
                let elapsed = f64::from(x - rect.left()) * per_point;
                let gain = gain * fade_gain(elapsed + per_point / 2.0, clip.length, &shown);
                let (a, b) = (view.seconds(elapsed), view.seconds(elapsed + per_point));
                waveform::Column { x: x + 0.5, from: a.min(b), to: a.max(b), gain }
            })
            .collect();
        let shape = waveform::geometry(peaks, marks, &columns, waveform::scale(painter.ctx()));
        waveform::paint(
            &painter.with_clip_rect(marks.intersect(painter.clip_rect())),
            &shape,
            waveform::Colors::from(dim(color)),
        );
    } else if clip.audio {
        painter.hline(marks.x_range(), marks.center().y.round() + 0.5, theme::hairline(dim(color)));
    } else if !clip.notes.is_empty() && marks.height() >= 4.0 {
        let low = clip.notes.iter().map(|n| n.key).min().unwrap_or(0);
        let high = clip.notes.iter().map(|n| n.key).max().unwrap_or(low);
        let rows = f32::from(high - low) + 1.0;
        // Only the notes in view are drawn; a long looped clip has thousands.
        let first = f64::from((view.left() - x_of(start)) / beat_width) - 1.0;
        let last = f64::from((view.right() - x_of(start)) / beat_width) + 1.0;
        for note in clip.notes.iter().filter(|n| n.start + n.length >= first && n.start <= last) {
            let x = x_of(start + note.start);
            let width = (note.length as f32 * beat_width - 1.0).max(1.0);
            let row = f32::from(note.key - low);
            let y = marks.bottom() - (row + 0.5) / rows * marks.height();
            painter.rect_filled(Rect::from_min_size(Pos2::new(x, y - 1.0), Vec2::new(width, 2.0)), 0.0, dim(color));
        }
    }
    if let Some((notes, tempo)) = ara_notes
        && marks.height() >= 4.0
    {
        paint_ara_notes(painter, rect, marks, notes, tempo, start, clip.length, x_of, beat_width, ink);
    }
    // A fade is a line over the waveform, from the clip's bottom corner to where it ends at
    // the top, following its shape and curve.
    if clip.audio {
        let fade_stroke = theme::hairline(dim(theme::text_secondary()));
        if shown.fade_in > 0.0 {
            let end = (rect.left() + shown.fade_in as f32 * beat_width).min(rect.right());
            painter.line(fade_line(rect, FadeEdge::In, rect.left(), end, shown.fade_in_curve), fade_stroke);
        }
        if shown.fade_out > 0.0 {
            let from = (rect.right() - shown.fade_out as f32 * beat_width).max(rect.left());
            painter.line(fade_line(rect, FadeEdge::Out, from, rect.right(), shown.fade_out_curve), fade_stroke);
        }
    }
    if selected {
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(2.0, theme::text_primary()), StrokeKind::Inside);
    }
}

/// The part of a clip a drag moves it from: its lower half, on a lane tall enough to have an
/// upper half for painting time; `None` when the whole clip moves it.
fn clip_grab_area(rect: Rect) -> Option<Rect> {
    (rect.height() >= CLIP_BODY_LEAST).then(|| Rect::from_min_max(Pos2::new(rect.left(), rect.center().y), rect.max))
}

/// The bar along a clip's top that carries its name, on a lane tall enough to have a body under
/// it (a title bar); the drag that moves the clip takes the whole upper half.
fn clip_name_bar(rect: Rect) -> Option<Rect> {
    (rect.height() >= CLIP_BODY_LEAST).then(|| Rect::from_min_size(rect.min, Vec2::new(rect.width(), CLIP_NAME_BAR)))
}

/// Text and marks that read on `background`: dark on a light colour, light on a dark one.
fn ink_on(background: Color32) -> Color32 {
    let [r, g, b, _] = background.to_array();
    let light = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
    if light > 150.0 { Color32::from_black_alpha(200) } else { Color32::from_white_alpha(225) }
}

/// The output dropdown: "→ Main" or a group; picking one runs `track.setOutput`.
fn routing(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    lane: &Lane,
    groups: &[(TrackId, String)],
    actions: &mut Vec<Action>,
) {
    let current = lane.output.and_then(|t| groups.iter().find(|(g, _)| *g == t)).map_or("Main", |(_, name)| name);
    let label = Rect::from_min_size(rect.min, Vec2::new(18.0, rect.height()));
    widgets::text_in(painter, label, Align2::LEFT_CENTER, "→", theme::ui_font(theme::TEXT_SMALL), theme::text_value());
    let field = Rect::from_min_max(Pos2::new(label.right(), rect.top()), rect.max);
    let mut picked = None;
    let response =
        widgets::button_area(ui, field, Id::new(("output", lane.track.0)), format!("{} output: {current}", lane.name));
    widgets::dropdown(ui, painter, field, &response, current);
    egui::Popup::menu(&response).show(|ui| {
        let text = |t: &str| egui::RichText::new(t).font(theme::ui_font(theme::TEXT_SMALL));
        if ui.selectable_label(lane.output.is_none(), text("Main")).clicked() && lane.output.is_some() {
            picked = Some(serde_json::Value::from("main"));
        }
        for (group, name) in groups.iter().filter(|(g, _)| *g != lane.track) {
            if ui.selectable_label(lane.output == Some(*group), text(name)).clicked() && lane.output != Some(*group) {
                picked = Some(serde_json::Value::from(group.0));
            }
        }
    });
    if let Some(to) = picked {
        actions.push(Action::new("track.setOutput", json!({ "track": lane.track.0, "to": to })));
    }
}

/// The MIDI input dropdown (ADR 0017): "MIDI" and what the track hears while armed; its menu
/// runs `track.setMidiInput`.
fn midi_input(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    lane: &Lane,
    ports: &[String],
    actions: &mut Vec<Action>,
) {
    let current = menus::midi_input_text(&lane.midi_input);
    let label = Rect::from_min_size(rect.min, Vec2::new(28.0, rect.height()));
    widgets::text_in(
        painter,
        label,
        Align2::LEFT_CENTER,
        "MIDI",
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_value(),
    );
    let field = Rect::from_min_max(Pos2::new(label.right(), rect.top()), rect.max);
    let response = widgets::button_area(
        ui,
        field,
        Id::new(("midi-input", lane.track.0)),
        format!("{} MIDI input: {current}", lane.name),
    );
    widgets::dropdown(ui, painter, field, &response, &current);
    egui::Popup::menu(&response).show(|ui| menus::midi_input_choices(ui, lane.track, &lane.midi_input, ports, actions));
}

/// The audio input dropdown (ADR 0017): "In" and the track's input channels and monitoring;
/// its menu runs `track.setAudioInput`.
fn audio_input(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    lane: &Lane,
    channels: u16,
    actions: &mut Vec<Action>,
) {
    let current = menus::audio_input_text(&lane.audio_input);
    let label = Rect::from_min_size(rect.min, Vec2::new(28.0, rect.height()));
    widgets::text_in(
        painter,
        label,
        Align2::LEFT_CENTER,
        "Audio",
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_value(),
    );
    let field = Rect::from_min_max(Pos2::new(label.right(), rect.top()), rect.max);
    let response = widgets::button_area(
        ui,
        field,
        Id::new(("audio-input", lane.track.0)),
        format!("{} audio input: {current}", lane.name),
    );
    widgets::dropdown(ui, painter, field, &response, &current);
    egui::Popup::menu(&response)
        .show(|ui| menus::audio_input_choices(ui, lane.track, &lane.audio_input, channels, actions));
}

/// Width of the meter lying in a return or main row, left of its volume.
const LYING_METER: f32 = 48.0;

/// Where a return or main row shows its meter: lying down, ending at `right`, centred on `y`.
fn lying_meter(right: f32, y: f32) -> Rect {
    Rect::from_min_max(Pos2::new(right - LYING_METER, y - meter::WIDTH / 2.0), Pos2::new(right, y + meter::WIDTH / 2.0))
}

/// A header's colour band: the track's colour down its left edge, the full height of the header
/// (dimmed while the track is off). A click opens the palette; gives the band's rectangle.
fn colour_band(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    clip: Rect,
    lane: &Lane,
    id: Id,
    actions: &mut Vec<Action>,
) -> Rect {
    let band = Rect::from_min_size(rect.min, Vec2::new(COLOUR_BAND, rect.height()));
    painter.rect_filled(band, 0.0, colour_picker::fill(lane.color, lane.mute));
    if band.intersects(clip) {
        colour_picker::area(ui, band.intersect(clip), id.with("colour"), lane.track, &lane.name, lane.color, actions);
    }
    band
}

/// Where a header stands among the drawn lanes: how many lanes are inside it (a group's fold
/// triangle shows when there are any) and the tracks in the order drawn, for a Shift-click's run.
struct HeaderOrder<'a> {
    children: usize,
    tracks: &'a [TrackId],
}

/// The track list's groups, for a header's menu.
struct Nest<'a> {
    /// Every track but the returns, in track order.
    places: &'a [Place],
    /// The group tracks, with their names.
    groups: &'a [(TrackId, String)],
}

#[allow(clippy::too_many_arguments)]
fn track_header(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    lanes_area: Rect,
    lane: &Lane,
    track_number: usize,
    nest: &Nest<'_>,
    order: &HeaderOrder<'_>,
    timeline: &Timeline,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) {
    let id = Id::new(("arrangement-header", lane.track.0));
    let clip = rect.intersect(lanes_area);

    // The header itself: click selects (Ctrl adds, Shift takes a run), right-click opens the
    // track's menu, a drag moves the track. Its controls, added after it, take their own clicks.
    let body = ui.interact(clip, id.with("body"), Sense::click_and_drag());
    body.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Track {}", lane.name)));
    if body.clicked() {
        state.targeted = Some(lane.track);
        let (shift, command) = ui.input(|i| (i.modifiers.shift, i.modifiers.command));
        state.pick_track(lane.track, shift, command, order.tracks);
        // The track, not a clip, is what Delete acts on now (#690); a clip on it stays selected
        // for the detail area.
        state.track_focus = true;
    }
    // A double-click shows the track's devices in the detail area.
    if body.double_clicked() {
        state.open_devices = Some(lane.track);
    }
    if body.drag_started_by(egui::PointerButton::Primary) {
        state.track_drag = Some(lane.track);
    }
    if body.drag_stopped_by(egui::PointerButton::Primary)
        && let Some(dragged) = state.track_drag.take()
        && let Some(pointer) = body.interact_pointer_pos().or_else(|| ui.ctx().pointer_latest_pos())
    {
        state.track_drop = Some((dragged, pointer.y));
    }
    let selection = state.selected_tracks.clone();
    body.context_menu(|ui| {
        let into: Vec<(TrackId, String)> = nesting::move_targets(nest.places, lane.track)
            .into_iter()
            .filter_map(|g| nest.groups.iter().find(|(t, _)| *t == g).cloned())
            .collect();
        let menu = TrackMenu {
            track: lane.track,
            name: &lane.name,
            colour: lane.color,
            is_group: lane.kind == ColumnKind::Group,
            is_return: lane.kind == ColumnKind::Return,
            selection: &selection,
            into: &into,
            out: nesting::out_of_group(nest.places, lane.track),
            midi: lane.plays_notes.then_some((&lane.midi_input, timeline.midi_ports.as_slice())),
            audio: lane.plays_audio.then_some((&lane.audio_input, timeline.input_channels)),
        };
        menus::track_menu(ui, &menu, actions);
    });
    let selected = state.selected_tracks.contains(&lane.track) || state.focus == Some(lane.track);
    // The header in its track's colour; a selected track's is brighter and more saturated, with
    // an outline, so the selection reads at a glance (#690).
    let header_fill = if selected {
        motion::mix(theme::panel_background(), saturated(rgb(lane.color)), 0.85)
    } else {
        motion::mix(theme::panel_background(), rgb(lane.color), 0.6)
    };
    painter.rect_filled(rect, 0.0, header_fill);
    if selected {
        painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.5, saturated(rgb(lane.color))), StrokeKind::Inside);
    }
    // The track's colour runs down the header's left edge; a click there opens the palette.
    let band = colour_band(ui, painter, rect, clip, lane, id, actions);
    // The meter stands at the right edge; the rows keep clear of it.
    let meter_x = rect.right() - PAD - meter::WIDTH;
    let inner = Rect::from_min_max(
        Pos2::new(band.right() + PAD + lane.depth as f32 * 8.0, rect.top() + HEADER_PAD_Y),
        Pos2::new(meter_x - PAD, rect.bottom() - HEADER_PAD_Y),
    );
    let meter_bottom =
        if rect.height() >= SHOWS_MIXER { inner.top() + HEADER_ROW + PAD + FIELD } else { inner.top() + HEADER_ROW };
    let at = Rect::from_min_max(
        Pos2::new(meter_x, inner.top()),
        Pos2::new(meter_x + meter::WIDTH, meter_bottom.min(inner.bottom())),
    );
    let level = state.meters.track(lane.track);
    meter::show(ui, painter, at.intersect(clip), &lane.name, meter::Source::Track(lane.track), &level, actions);

    // Row 1: a group's fold triangle, track on, solo, cue, arm, name. Mixer controls act on the whole
    // selection while this track is part of one.
    let controlled: Vec<u32> = state.controlled_tracks(lane.track).iter().map(|t| t.0).collect();
    let fold = (lane.kind == ColumnKind::Group && order.children > 0)
        .then(|| Rect::from_min_size(inner.min, Vec2::new(FOLD, HEADER_ROW)));
    if let Some(fold) = fold {
        let folded = state.folds.is_folded(lane.track);
        let label = if folded { format!("Unfold {}", lane.name) } else { format!("Fold {}", lane.name) };
        let response = widgets::button_area(ui, fold.intersect(clip), id.with("fold"), label);
        let color = motion::mix(theme::text_hint(), theme::text_primary(), widgets::hover(ui, &response));
        let center = fold.center();
        let (wide, tall) = (7.0, 4.0);
        // Pointing right while the group is folded, down while it is open.
        let points = if folded {
            vec![
                Pos2::new(center.x - tall / 2.0, center.y - wide / 2.0),
                Pos2::new(center.x - tall / 2.0, center.y + wide / 2.0),
                Pos2::new(center.x + tall / 2.0, center.y),
            ]
        } else {
            vec![
                Pos2::new(center.x - wide / 2.0, center.y - tall / 2.0),
                Pos2::new(center.x + wide / 2.0, center.y - tall / 2.0),
                Pos2::new(center.x, center.y + tall / 2.0),
            ]
        };
        painter.add(egui::Shape::convex_polygon(points, color, egui::Stroke::NONE));
        if response.clicked() {
            state.folds.toggle(lane.track);
            // What the lanes are changes without the project changing: draw the next frame.
            ui.ctx().request_repaint();
        }
    }
    let first_x = fold.map_or(inner.left(), |f| f.right() + 2.0);
    let row = Rect::from_min_max(Pos2::new(first_x, inner.top()), Pos2::new(inner.right(), inner.top() + HEADER_ROW));
    let toggle = Rect::from_min_size(row.min, Vec2::new(ON_TOGGLE, HEADER_ROW));
    let solo = Rect::from_min_size(Pos2::new(toggle.right() + 2.0, row.top()), Vec2::new(SOLO, HEADER_ROW));
    let cue = Rect::from_min_size(Pos2::new(solo.right() + 2.0, row.top()), Vec2::new(SOLO, HEADER_ROW));
    // A track that can be armed has its arm button after cue, as wide.
    let arm =
        lane.armable.then(|| Rect::from_min_size(Pos2::new(cue.right() + 2.0, row.top()), Vec2::new(SOLO, HEADER_ROW)));
    // The automation button at the end of the row shows and hides the track's lanes.
    let lanes_button = Rect::from_min_size(Pos2::new(row.right() - SOLO, row.top()), Vec2::new(SOLO, HEADER_ROW));
    if lanes_button.intersects(clip) {
        automation::header_button(
            ui,
            painter,
            lanes_button,
            lane.track,
            &lane.name,
            &mut state.automation,
            &timeline.automation,
        );
    }
    let name = Rect::from_min_max(
        Pos2::new(arm.unwrap_or(cue).right() + PAD, row.top()),
        Pos2::new(lanes_button.left() - 4.0, row.bottom()),
    );
    let name_color = if lane.mute { theme::text_hint() } else { theme::text_primary() };
    widgets::text_in(painter, name, Align2::LEFT_CENTER, &lane.name, theme::ui_font(theme::TEXT), name_color);
    if solo.intersects(clip)
        && let Some(action) =
            widgets::solo_button(ui, painter, solo, id.with("solo"), &controlled, &lane.name, lane.solo)
    {
        actions.push(action);
    }
    if cue.intersects(clip)
        && let Some(action) = widgets::cue_button(ui, painter, cue, id.with("cue"), lane.track.0, &lane.name, lane.cue)
    {
        actions.push(action);
    }
    if let Some(arm) = arm.filter(|a| a.intersects(clip))
        && let Some(action) = widgets::arm_button(
            ui,
            painter,
            arm,
            id.with("arm"),
            lane.track.0,
            &lane.name,
            lane.armed,
            lane.plays_notes,
        )
    {
        actions.push(action);
    }
    if toggle.intersects(clip) {
        let label = if lane.mute { format!("Turn {} on", lane.name) } else { format!("Turn {} off", lane.name) };
        let response = widgets::button_area(ui, toggle.intersect(clip), id.with("on"), label);
        if response.clicked() {
            actions.push(Action::new("track.setMute", json!({ "tracks": controlled, "mute": !lane.mute })));
        }
        let (fill, text) = widgets::toggle_colors(ui, &response, !lane.mute);
        painter.rect_filled(toggle, 0.0, fill);
        let number = if lane.kind == ColumnKind::Return {
            lane.name.chars().next().map(String::from).unwrap_or_default()
        } else {
            track_number.to_string()
        };
        widgets::text_in(painter, toggle, Align2::CENTER_CENTER, &number, theme::mono_font(theme::TEXT_SMALL), text);
    }

    // Row 2, once the lane is tall enough: volume and pan.
    if rect.height() >= SHOWS_MIXER {
        let top = row.bottom() + PAD;
        let pan = Rect::from_min_max(Pos2::new(inner.right() - PAN_FIELD, top), Pos2::new(inner.right(), top + FIELD));
        // The sends, a knob per return before the pan, as many as leave the volume bar its room
        // (Ableton Live's track header has them too).
        let step = FIELD + SEND_GAP;
        let room = ((pan.left() - 4.0 - inner.left() - LEAST_VOLUME) / step).floor().max(0.0) as usize;
        let shown = lane.sends.len().min(room);
        let sends_left = pan.left() - 4.0 - step * shown as f32;
        let volume = Rect::from_min_max(Pos2::new(inner.left(), top), Pos2::new(sends_left, top + FIELD));
        value_control(ui, painter, volume, clip, lane, Control::Volume, false, id, state, actions);
        for (index, send) in lane.sends.iter().take(shown).enumerate() {
            let knob =
                Rect::from_min_size(Pos2::new(sends_left + SEND_GAP + step * index as f32, top), Vec2::splat(FIELD));
            send_knob(ui, painter, knob, clip, lane, send, id, state, actions);
        }
        value_control(ui, painter, pan, clip, lane, Control::Pan, false, id, state, actions);
    }

    // Row 3, taller still: where the track's audio goes.
    if rect.height() >= SHOWS_ROUTING {
        let top = row.bottom() + PAD + FIELD + PAD + 4.0;
        let line = Rect::from_min_max(Pos2::new(inner.left(), top), Pos2::new(inner.right(), top + DROPDOWN));
        if line.intersects(clip) {
            routing(ui, painter, line, lane, nest.groups, actions);
        }
        // Taller still, on a track that plays notes: which controllers it hears when armed.
        let input = line.translate(Vec2::new(0.0, DROPDOWN + PAD));
        if rect.height() >= SHOWS_MIDI_INPUT && input.intersects(clip) {
            if lane.plays_notes {
                midi_input(ui, painter, input, lane, &timeline.midi_ports, actions);
            } else if lane.plays_audio {
                audio_input(ui, painter, input, lane, timeline.input_channels, actions);
            }
        }
    }

    // The bottom edge resizes the lane; a return's top edge, as the fixed rows sit along the
    // bottom and grow upward.
    let upward = lane.kind == ColumnKind::Return;
    let edge = if upward {
        Rect::from_min_max(Pos2::new(rect.left(), rect.top() - 1.0), Pos2::new(rect.right(), rect.top() + 3.0))
    } else {
        Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 3.0), Pos2::new(rect.right(), rect.bottom() + 2.0))
    };
    if edge.intersects(lanes_area) {
        let response = ui
            .interact(edge.intersect(lanes_area), id.with("resize"), Sense::drag())
            .on_hover_cursor(egui::CursorIcon::ResizeVertical);
        if response.dragged_by(egui::PointerButton::Primary) {
            let dy = response.drag_delta().y;
            let height = (state.height(lane) + if upward { -dy } else { dy }).clamp(MIN_LANE, MAX_LANE);
            state.heights.insert(lane.track, height);
        }
    }
}

/// A header's send knob to one return: dragged up or down for the level (`track.setSend`, sent
/// when let go), double-click for none, right-click for before or after the fader
/// (`track.setSendMode`), as the Session view's strip's.
#[allow(clippy::too_many_arguments)]
fn send_knob(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    clip: Rect,
    lane: &Lane,
    send: &crate::interaction::grid::SendKnob,
    id: Id,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) {
    if !rect.intersects(clip) {
        return;
    }
    let track = lane.track;
    let mode = if send.pre_fader { "pre" } else { "post" };
    let response = widgets::value_area(
        ui,
        rect.intersect(clip),
        id.with(("send", send.target.0)),
        format!("{} send to {}", lane.name, send.name),
        format!("{} {mode}", widgets::format_db(send.level_db)),
    );
    let mut preview = match state.send_drag {
        Some((t, r, v)) if t == track && r == send.target => Some(v),
        _ => None,
    };
    let level = if send.level_db.is_finite() { send.level_db } else { VOLUME_RANGE.0 };
    let mut committed = None;
    match widgets::drag_value(ui, &response, &mut preview, level, 0.25, 1.0, VOLUME_RANGE, VOLUME_RANGE.0) {
        Drag::Moving(v) => state.send_drag = Some((track, send.target, v)),
        Drag::Done(v) => {
            state.send_drag = None;
            committed = Some(v);
            let db = if v <= VOLUME_RANGE.0 { json!("-inf") } else { json!((v * 10.0).round() / 10.0) };
            actions
                .push(Action::new("track.setSend", json!({ "track": track.0, "return": send.target.0, "level": db })));
        }
        Drag::None => {}
    }
    response.context_menu(|ui| {
        for (pre, label) in [(true, "Before the fader (pre)"), (false, "After the fader (post)")] {
            if ui.radio(send.pre_fader == pre, label).clicked() {
                let mode = if pre { "pre" } else { "post" };
                actions.push(Action::new(
                    "track.setSendMode",
                    json!({ "track": track.0, "return": send.target.0, "mode": mode }),
                ));
                ui.close();
            }
        }
    });
    let shown = match state.send_drag {
        Some((t, r, v)) if t == track && r == send.target => v,
        _ => committed.unwrap_or(level),
    };
    let position = ((shown - VOLUME_RANGE.0) / (VOLUME_RANGE.1 - VOLUME_RANGE.0)) as f32;
    let pointer = motion::mix(theme::text_value(), theme::text_primary(), widgets::hover(ui, &response));
    widgets::knob(painter, rect.center(), FIELD, position, pointer);
    // The return's letter in the knob, and a dot under it when the send is before the fader.
    let letter = send.name.chars().next().map(String::from).unwrap_or_default();
    widgets::text_in(
        painter,
        rect,
        Align2::CENTER_CENTER,
        &letter,
        theme::mono_font(theme::TEXT_SMALL - 2.0),
        theme::text_hint(),
    );
    if send.pre_fader {
        painter.circle_filled(Pos2::new(rect.center().x, rect.bottom() + 1.5), 1.5, theme::text_value());
    }
}

#[allow(clippy::too_many_arguments)]
fn value_control(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    clip: Rect,
    lane: &Lane,
    control: Control,
    plain: bool,
    id: Id,
    state: &mut ArrangementViewState,
    actions: &mut Vec<Action>,
) {
    let track = lane.track;
    let (value, per_point, per_tick, range, default) = match control {
        Control::Volume => (lane.volume_db, 0.25, 1.0, VOLUME_RANGE, 0.0),
        Control::Pan => (lane.pan, 0.01, 0.02, (-1.0, 1.0), 0.0),
    };
    // The tracks this drag moves: one track, or the whole selection it is part of.
    let tracks = state.controlled_tracks(track);
    let mut preview = state.value_drag.as_ref().filter(|d| d.track == track && d.control == control).map(|d| d.now);
    // Another track of the same selection moves with the drag, by the same amount: what it shows
    // is its own value moved by the drag's offset, before the command lands.
    let dragged = match state.value_drag.as_ref() {
        Some(drag) if drag.control == control && drag.tracks.contains(&track) => value + (drag.now - drag.start),
        _ => value,
    };
    let shown = preview.unwrap_or(dragged);
    let mut hover = 0.0;
    let mut committed = None;
    if rect.intersects(clip) {
        // Where the field is, so the wheel over it moves the value instead of scrolling the view.
        state.value_areas.push(rect);
        let (label, text) = match control {
            Control::Volume => (format!("{} volume", lane.name), widgets::format_db(shown)),
            Control::Pan => (format!("{} pan", lane.name), widgets::format_pan(shown)),
        };
        let response = widgets::value_area(ui, rect.intersect(clip), id.with(label.as_str()), &label, text);
        hover = widgets::hover(ui, &response);
        // One track hears every frame of a drag: each sets the value, and all but the first
        // continue its undo step, so the drag is one step. A selection of several moves together
        // when the drag ends, by the same amount (`value_action`).
        let continued = preview.is_some();
        let one = tracks.len() == 1;
        let set = |v: f64| match control {
            Control::Volume => {
                let db = if v <= VOLUME_RANGE.0 { json!("-inf") } else { json!((v * 10.0).round() / 10.0) };
                Action::new("track.setVolume", json!({ "tracks": [track.0], "db": db, "continue": continued }))
            }
            Control::Pan => Action::new(
                "track.setPan",
                json!({ "tracks": [track.0], "pan": (v * 100.0).round() / 100.0, "continue": continued }),
            ),
        };
        match widgets::drag_value(ui, &response, &mut preview, value, per_point, per_tick, range, default) {
            Drag::Moving(v) => {
                state.value_drag = Some(ValueDrag { track, control, start: value, now: v, tracks: tracks.clone() });
                if one {
                    actions.push(set(v));
                }
            }
            Drag::Done(v) => {
                state.value_drag = None;
                committed = Some(v);
                actions.push(if one { set(v) } else { value_action(control, &tracks, value, v) });
            }
            Drag::None => {}
        }
    }
    // A drag (and the value it commits) follows the pointer directly and keeps the animation
    // there, so nothing glides back afterwards; other changes (an undo, say) glide.
    let anim = id.with(("shown", control as u8));
    let dragged = match state.value_drag.as_ref() {
        Some(drag) if drag.control == control && drag.tracks.contains(&track) => Some(drag.now),
        _ => committed,
    };
    let shown = match dragged {
        Some(v) => {
            motion::jump(ui.ctx(), anim, v as f32);
            v
        }
        None if value.is_finite() => f64::from(motion::value(ui.ctx(), anim, value as f32, motion::VALUE)),
        None => value,
    };
    match control {
        Control::Volume if plain => volume_text(painter, rect, shown, hover),
        Control::Volume if state.knobs => volume_knob(painter, rect, shown, hover),
        Control::Volume => volume_field(painter, rect, shown, hover),
        Control::Pan if state.knobs => pan_knob(painter, rect, shown),
        Control::Pan => pan_field(painter, rect, shown, hover),
    }
}

/// A volume as a small knob with its value beside it, for Settings › Session and mixer ›
/// Mixer controls: Knob.
fn volume_knob(painter: &egui::Painter, rect: Rect, db: f64, hover: f32) {
    let size = rect.height().min(theme::KNOB);
    let center = Pos2::new(rect.left() + size / 2.0 + 2.0, rect.center().y);
    let pointer = motion::mix(theme::text_value(), theme::text_primary(), hover);
    widgets::knob(painter, center, size, volume_position(db), pointer);
    let text = Rect::from_min_max(Pos2::new(center.x + size / 2.0 + 4.0, rect.top()), rect.max);
    widgets::text_in(
        painter,
        text,
        Align2::RIGHT_CENTER,
        &widgets::format_db(db),
        theme::mono_font(theme::TEXT_SMALL),
        theme::text_primary(),
    );
}

/// A pan as a knob in the middle of its field, with the value at its right.
fn pan_knob(painter: &egui::Painter, rect: Rect, pan: f64) {
    let size = rect.height().min(theme::KNOB);
    let center = Pos2::new(rect.left() + size / 2.0 + 2.0, rect.center().y);
    widgets::knob(painter, center, size, ((pan + 1.0) / 2.0) as f32, theme::text_primary());
    let text = Rect::from_min_max(Pos2::new(center.x + size / 2.0 + 4.0, rect.top()), rect.max);
    widgets::text_in(
        painter,
        text,
        Align2::RIGHT_CENTER,
        &widgets::format_pan(pan),
        theme::mono_font(theme::TEXT_SMALL),
        theme::text_secondary(),
    );
}

/// The command a finished volume or pan drag runs: one track is set to the value the drag reached,
/// and a selection of several moves together by the same amount, keeping their differences. A move
/// that cannot be counted in dB (a track at silence, whose value is -inf) is set outright.
fn value_action(control: Control, tracks: &[TrackId], from: f64, to: f64) -> Action {
    let ids: Vec<u32> = tracks.iter().map(|t| t.0).collect();
    let one = tracks.len() == 1;
    let delta = to - from;
    match control {
        Control::Volume if !one && delta.is_finite() => {
            let db = if to <= VOLUME_RANGE.0 { -200.0 } else { (delta * 10.0).round() / 10.0 };
            Action::new("track.adjustVolume", json!({ "tracks": ids, "db": db }))
        }
        Control::Volume => {
            let db = if to <= VOLUME_RANGE.0 { json!("-inf") } else { json!((to * 10.0).round() / 10.0) };
            Action::new("track.setVolume", json!({ "tracks": ids, "db": db }))
        }
        Control::Pan if !one && delta.is_finite() => {
            Action::new("track.adjustPan", json!({ "tracks": ids, "pan": (delta * 100.0).round() / 100.0 }))
        }
        Control::Pan => Action::new("track.setPan", json!({ "tracks": ids, "pan": (to * 100.0).round() / 100.0 })),
    }
}

/// A volume field: fill from the left up to the level, a tick at its end, the value on the right.
fn volume_field(painter: &egui::Painter, rect: Rect, db: f64, hover: f32) {
    painter.rect_filled(rect, 0.0, motion::mix(theme::control_background(), theme::control_hover(), hover));
    let x = rect.left() + rect.width() * volume_position(db);
    painter.rect_filled(Rect::from_min_max(rect.min, Pos2::new(x, rect.bottom())), 0.0, theme::control_playing());
    let tick = x.round() - 0.5;
    painter.vline(tick, egui::Rangef::new(rect.top(), rect.top() + 2.0), theme::hairline(theme::text_value()));
    painter.vline(tick, egui::Rangef::new(rect.bottom() - 2.0, rect.bottom()), theme::hairline(theme::text_value()));
    let text = rect.shrink2(Vec2::new(4.0, 0.0));
    widgets::text_in(
        painter,
        text,
        Align2::RIGHT_CENTER,
        &widgets::format_db(db),
        theme::mono_font(theme::TEXT_SMALL),
        theme::text_primary(),
    );
}

/// A volume as a plain value, for the compact return and main rows.
fn volume_text(painter: &egui::Painter, rect: Rect, db: f64, hover: f32) {
    let color = motion::mix(theme::text_value(), theme::text_primary(), hover);
    widgets::text_in(
        painter,
        rect,
        Align2::RIGHT_CENTER,
        &widgets::format_db(db),
        theme::mono_font(theme::TEXT_SMALL),
        color,
    );
}

/// A pan field: a bar along the bottom from the centre, a centre tick, the value in the middle.
fn pan_field(painter: &egui::Painter, rect: Rect, pan: f64, hover: f32) {
    painter.rect_filled(rect, 0.0, motion::mix(theme::pan_background(), theme::panel_hover(), hover));
    let centre = rect.center().x.round() - 0.5;
    painter.vline(centre, egui::Rangef::new(rect.top(), rect.top() + 2.0), theme::hairline(theme::pan_centre()));
    painter.vline(centre, egui::Rangef::new(rect.bottom() - 2.0, rect.bottom()), theme::hairline(theme::pan_centre()));
    let to = rect.center().x + rect.width() / 2.0 * pan.clamp(-1.0, 1.0) as f32;
    let bar =
        Rect::from_min_max(Pos2::new(centre.min(to), rect.bottom() - 2.0), Pos2::new(centre.max(to), rect.bottom()));
    painter.rect_filled(bar, 0.0, theme::text_value());
    widgets::text_in(
        painter,
        rect,
        Align2::CENTER_CENTER,
        &widgets::format_pan(pan),
        theme::mono_font(theme::TEXT_SMALL),
        theme::text_primary(),
    );
}

/// The menu of an empty spot of a lane: a new empty MIDI clip where it was right-clicked (on a
/// notes track), and, with a time selection, looping it and consolidating it on this track.
fn empty_lane_menu(ui: &mut Ui, track: TrackId, notes: bool, state: &ArrangementViewState, actions: &mut Vec<Action>) {
    ui.set_min_width(180.0);
    let beat = state.menu_beat.unwrap_or(0.0);
    if ui.add_enabled(notes, egui::Button::new("Insert empty MIDI clip")).clicked() {
        let stack = state.stack_midi;
        actions.push(Action::new("clip.create", json!({ "track": track.0, "start": beat, "stack": stack })));
        ui.close();
    }
    let selection = state.time_selection.as_ref();
    ui.separator();
    if ui
        .add_enabled(
            selection.is_some(),
            egui::Button::new("Loop selection").shortcut_text(crate::settings::keymap::shown("Ctrl+L")),
        )
        .clicked()
        && let Some(s) = selection
    {
        actions.push(Action::new("transport.setLoop", json!({ "start": s.start, "end": s.end, "enabled": true })));
        ui.close();
    }
    let here = selection.filter(|s| s.tracks.contains(&track));
    if ui
        .add_enabled(
            here.is_some(),
            egui::Button::new("Consolidate selection").shortcut_text(crate::settings::keymap::shown("Ctrl+J")),
        )
        .clicked()
        && let Some(s) = here
    {
        actions.push(Action::new("clip.consolidate", json!({ "track": track.0, "start": s.start, "end": s.end })));
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_reaches_ten_times_further_in_than_before_and_stops() {
        let mut width = DEFAULT_BEAT_WIDTH;
        for _ in 0..100 {
            width = zoomed(width, 1.5);
        }
        assert_eq!(width, 4000.0);
        for _ in 0..100 {
            width = zoomed(width, 1.0 / 1.5);
        }
        assert_eq!(width, 1.0);
    }

    #[test]
    fn volume_sits_at_85_percent_for_0_db() {
        assert_eq!(volume_position(f64::NEG_INFINITY), 0.0);
        assert_eq!(volume_position(-70.0), 0.0);
        assert!((volume_position(0.0) - 0.85).abs() < 1e-6);
        assert!((volume_position(6.0) - 1.0).abs() < 1e-6);
        assert!(volume_position(-6.0) < volume_position(-3.0));
    }

    #[test]
    fn alt_and_the_wheel_zoom_every_lane_within_its_limits() {
        let timeline = Timeline::build(&crate::demo::demo_project(), None);
        let tracks: Vec<&Lane> = timeline.lanes.iter().filter(|l| l.kind != ColumnKind::Return).collect();
        let mut state = ArrangementViewState::default();
        state.set_lane_height(tracks[0].track, 100.0);
        state.zoom_lanes(&timeline.lanes, 2.0, 0.0);
        assert_eq!(state.height(tracks[0]), 200.0);
        assert!(tracks[1..].iter().all(|l| state.height(l) == DEFAULT_LANE * 2.0));
        // The content under the pointer stays put: what was 10 points down is now 20.
        state.scroll.y = 0.0;
        state.zoom_lanes(&timeline.lanes, 0.5, 10.0);
        assert_eq!(state.scroll.y, 0.0);
        state.scroll.y = 40.0;
        state.zoom_lanes(&timeline.lanes, 2.0, 10.0);
        assert!((state.scroll.y - 90.0).abs() < 1.0, "{}", state.scroll.y);
        for _ in 0..50 {
            state.zoom_lanes(&timeline.lanes, 0.5, 0.0);
        }
        assert!(tracks.iter().all(|l| state.height(l) == MIN_LANE));
        state.zoom_lanes(&timeline.lanes, f32::NAN, 0.0);
        assert!(tracks.iter().all(|l| state.height(l) == MIN_LANE));
    }

    fn audio_clip() -> ClipBox {
        let timeline = Timeline::build(&crate::demo::demo_project(), None);
        let clip = timeline.lanes.iter().flat_map(|l| &l.clips).next().cloned().unwrap();
        ClipBox { start: 2.0, length: 8.0, fade_in: 1.0, fade_out: 0.0, audio: true, ..clip }
    }

    #[test]
    fn a_folded_group_hides_its_tracks_and_shows_the_one_it_stands_in_for() {
        let project = crate::demo::grouped_project();
        let timeline = Timeline::build(&project, None);
        let track = |name: &str| timeline.lanes.iter().position(|l| l.name == name).unwrap();
        let (group, drums, bass) = (track("Group"), track("Drums"), track("Bass"));
        let open = drawn_lanes(&timeline.lanes, &Folds::default());
        assert_eq!(
            open.iter().map(|d| d.lane).collect::<Vec<_>>(),
            [group, drums, bass, track("Keys"), track("Vox")],
            "a project with no folds draws every lane, and returns are not lanes"
        );
        assert_eq!(open[0].children, 2, "the group holds Drums and Bass");
        assert!(!open[0].folded);

        let mut folds = Folds::default();
        folds.fold(timeline.lanes[group].track);
        folds.reconcile(&project, None);
        let folded = drawn_lanes(&timeline.lanes, &folds);
        assert_eq!(folded.iter().map(|d| d.lane).collect::<Vec<_>>(), [group, track("Keys"), track("Vox")]);
        assert!(folded[0].folded);
        assert_eq!(folded[0].clips, drums, "its row shows the first track inside it");
        assert_eq!(folded[0].children, 2);

        // Working on Bass (its header, or its clip) turns the group's row to it.
        folds.reconcile(&project, Some(timeline.lanes[bass].track));
        assert_eq!(drawn_lanes(&timeline.lanes, &folds)[0].clips, bass);
    }

    #[test]
    fn a_fold_inside_a_fold_hides_exactly_what_it_covers() {
        let lane = |track: u32, kind: ColumnKind, depth: usize| Lane {
            track: TrackId(track),
            name: format!("T{track}"),
            color: suhina_engine::model::Rgb(0, 0, 0),
            kind,
            notes: false,
            depth,
            parent: None,
            position: track as usize,
            mute: false,
            solo: false,
            cue: false,
            armable: false,
            armed: false,
            midi_input: Default::default(),
            plays_notes: false,
            plays_audio: false,
            audio_input: Default::default(),
            volume_db: 0.0,
            pan: 0.0,
            sends: Vec::new(),
            output: None,
            column: None,
            overrides: false,
            clips: Vec::new(),
        };
        // An outer group (10) holding a group (11) that holds a track (12), and a track (13)
        // after the outer one.
        let lanes = [
            lane(10, ColumnKind::Group, 0),
            lane(11, ColumnKind::Group, 1),
            lane(12, ColumnKind::Track, 2),
            lane(13, ColumnKind::Track, 0),
        ];
        let drawn = |folds: &Folds| drawn_lanes(&lanes, folds).iter().map(|d| d.lane).collect::<Vec<_>>();
        assert_eq!(drawn(&Folds::default()), [0, 1, 2, 3]);
        let mut outer = Folds::default();
        outer.fold(TrackId(10));
        assert_eq!(drawn(&outer), [0, 3], "an outer fold hides the group inside it and its track");
        assert_eq!(drawn_lanes(&lanes, &outer)[0].children, 2, "the whole family is inside it");
        let mut inner = Folds::default();
        inner.fold(TrackId(11));
        assert_eq!(drawn(&inner), [0, 1, 3], "an inner fold hides only its own track");
        // A track is not a group: asking to fold one changes nothing.
        let mut not_a_group = Folds::default();
        not_a_group.fold(TrackId(13));
        assert_eq!(drawn(&not_a_group), [0, 1, 2, 3]);
    }

    #[test]
    fn a_dragged_gain_or_fade_is_what_the_clip_shows() {
        let clip = audio_clip();
        let key = (TrackId(1), ClipId(1));
        let mut state = ArrangementViewState::default();
        let linear = FadeCurve::default();
        assert_eq!(
            state.shown(key, &clip, Some(beats(1.0))),
            Shown { gain_db: 0.0, fade_in: 1.0, fade_out: 0.0, fade_in_curve: linear, fade_out_curve: linear }
        );
        // A curve being dragged bends that fade only.
        state.curve_drag = Some(CurveDrag { key, edge: FadeEdge::In, curve: 0.4, grab: 0.0 });
        assert_eq!(state.shown(key, &clip, Some(beats(1.0))).fade_in_curve.curve, 0.4);
        assert_eq!(state.shown(key, &clip, Some(beats(1.0))).fade_out_curve.curve, 0.0);
        state.curve_drag = None;
        // The waveform is drawn with the gain being dragged, not the one in the project.
        state.gain_drag = Some((key, -12.0));
        assert_eq!(state.shown(key, &clip, Some(beats(1.0))).gain_db, -12.0);
        assert_eq!(
            state.shown((TrackId(1), ClipId(2)), &clip, Some(beats(1.0))).gain_db,
            0.0,
            "only on the dragged clip"
        );
        // A fade being dragged: on the grid, or free with Alt.
        state.fade_drag = Some(FadeDrag { key, edge: FadeEdge::Out, raw: 2.6 });
        assert_eq!(state.shown(key, &clip, Some(beats(1.0))).fade_out, 3.0);
        assert!((state.shown(key, &clip, None).fade_out - 2.6).abs() < 1e-9);
    }

    #[test]
    fn a_fade_ends_on_the_grid_and_leaves_room_for_the_other() {
        let clip = audio_clip();
        // The fade-in's end snaps to the grid in project beats: the clip starts at beat 2.
        assert_eq!(fade_length(1.3, FadeEdge::In, &clip, 0.0, Some(beats(4.0))), 2.0);
        assert_eq!(fade_length(1.3, FadeEdge::In, &clip, 0.0, None), 1.3);
        // The fade-out's start snaps: the clip ends at beat 10.
        assert_eq!(fade_length(1.3, FadeEdge::Out, &clip, 0.0, Some(beats(4.0))), 2.0);
        // Never below 0, never past the other fade.
        assert_eq!(fade_length(-5.0, FadeEdge::In, &clip, 0.0, None), 0.0);
        assert_eq!(fade_length(7.5, FadeEdge::In, &clip, 3.0, None), 5.0);
        assert_eq!(fade_length(f64::NAN, FadeEdge::In, &clip, 0.0, None), 0.0);
        // The clip's own ends snap too: from beat 10 back to beat 2.6, nearer the clip's start
        // (beat 2) than the grid line at beat 4, the fade-out takes the whole clip, and the
        // fade-in from beat 2 to 9.5 takes the room the fade-out leaves.
        assert_eq!(fade_length(7.4, FadeEdge::Out, &clip, 0.0, Some(beats(4.0))), 8.0);
        assert_eq!(fade_length(7.4, FadeEdge::Out, &clip, 1.0, Some(beats(4.0))), 7.0);
        assert_eq!(fade_length(7.5, FadeEdge::In, &clip, 0.0, Some(beats(4.0))), 8.0);
        assert_eq!(fade_length(0.4, FadeEdge::Out, &clip, 0.0, Some(beats(4.0))), 0.0);
        // Nearer the grid line, the grid wins.
        assert_eq!(fade_length(6.5, FadeEdge::Out, &clip, 0.0, Some(beats(4.0))), 6.0);
    }

    #[test]
    fn fade_handles_follow_the_fade_and_stay_inside_the_clip() {
        let clip = Rect::from_min_size(Pos2::new(100.0, 10.0), Vec2::new(80.0, 40.0));
        assert_eq!(fade_handle(clip, FadeEdge::In, 0.0).min, Pos2::new(100.0, 10.0));
        assert_eq!(fade_handle(clip, FadeEdge::Out, 0.0).max.x, 180.0);
        assert_eq!(fade_handle(clip, FadeEdge::In, 20.0).min.x, 120.0);
        assert_eq!(fade_handle(clip, FadeEdge::Out, 500.0).min.x, 100.0);
        assert_eq!(fade_handle(clip, FadeEdge::In, f32::NAN).min.x, 100.0);
        // Linear: half-way into the fade-in, half the level; full in between the fades.
        let linear = FadeCurve::default();
        let shown = Shown { gain_db: 0.0, fade_in: 1.0, fade_out: 2.0, fade_in_curve: linear, fade_out_curve: linear };
        assert_eq!(fade_gain(0.5, 8.0, &shown), 0.5);
        assert_eq!(fade_gain(4.0, 8.0, &shown), 1.0);
        assert_eq!(fade_gain(7.0, 8.0, &shown), 0.5);
        // Other shapes and curves follow the spec's formula.
        let slow = FadeCurve { shape: FadeShape::Slow, curve: 0.0 };
        let shown = Shown { fade_in_curve: slow, fade_out_curve: FadeCurve { curve: 1.0, ..slow }, ..shown };
        assert_eq!(fade_gain(0.5, 8.0, &shown), 0.25);
        assert!((fade_gain(7.0, 8.0, &shown) - 0.25f32.powf(0.25)).abs() < 1e-6);
    }

    /// A fade's line runs from the silent bottom corner to full level at the top along its
    /// shape, and the curve handle sits on it half-way; dragging up bends towards +1 (#197).
    #[test]
    fn fade_lines_and_curve_handles_follow_the_shape() {
        let clip = Rect::from_min_size(Pos2::new(100.0, 10.0), Vec2::new(80.0, 42.0));
        let (top, bottom) = (12.0, 52.0);
        for shape in FadeShape::ALL {
            let curve = FadeCurve { shape, curve: 0.0 };
            let line = fade_line(clip, FadeEdge::In, 100.0, 140.0, curve);
            assert_eq!(line.first().copied(), Some(Pos2::new(100.0, bottom)), "{shape:?}");
            assert_eq!(line.last().copied(), Some(Pos2::new(140.0, top)), "{shape:?}");
            let out = fade_line(clip, FadeEdge::Out, 150.0, 180.0, curve);
            assert_eq!((out[0], out[out.len() - 1]), (Pos2::new(150.0, top), Pos2::new(180.0, bottom)), "{shape:?}");
            let handle = curve_handle(clip, FadeEdge::In, 40.0, curve);
            assert_eq!(handle.x, 120.0);
            assert!((handle.y - (bottom - curve.gain(0.5) as f32 * 40.0)).abs() < 1e-4, "{shape:?}");
        }
        assert_eq!(curve_handle(clip, FadeEdge::Out, 30.0, FadeCurve::default()).x, 165.0);
        // The clip's height is the whole range: half of it up is +1 from 0.
        // The handle follows the pointer: the curve set by a height puts the handle there.
        for shape in FadeShape::ALL {
            let unbent = curve_handle(clip, FadeEdge::In, 40.0, FadeCurve { shape, curve: 0.0 }).y;
            for y in [unbent - 6.0, unbent - 1.0, unbent, unbent + 1.0, unbent + 6.0] {
                let curve = FadeCurve { shape, curve: curve_at(clip, shape, y) };
                if (-1.0..=1.0).contains(&curve.curve) && curve.curve.abs() < 0.999 {
                    let at = curve_handle(clip, FadeEdge::In, 40.0, curve).y;
                    assert!((at - y).abs() < 0.01, "{shape:?}: handle at {at}, pointer at {y}");
                }
            }
            assert_eq!(curve_at(clip, shape, -500.0), 1.0, "{shape:?}: kept within -1 … +1");
            assert_eq!(curve_at(clip, shape, 500.0), -1.0, "{shape:?}");
        }
        assert!(curve_at(clip, FadeShape::Linear, f32::NAN).is_finite());
        // However wide the fade, its line has a bounded number of points.
        assert!(fade_line(clip, FadeEdge::In, 100.0, 1_000_000.0, FadeCurve::default()).len() <= FADE_LINE_POINTS);
        assert_eq!(shape_label(FadeShape::SCurve), "S-curve");
    }

    /// A grid of `step` beats at a zoom where its lines are far enough apart, in 4/4.
    fn beats(step: f64) -> Grid {
        Grid::new(Quantization::Beats(step), 1000.0, 4.0)
    }

    /// The grid step for the control bar's `value`, at `beat_width` points a beat.
    fn step(value: &str, beat_width: f32, bar_beats: f64) -> f64 {
        let chosen = suhina_session::parse_quantization(value).unwrap_or(Quantization::None);
        Grid::new(chosen, beat_width, bar_beats).step
    }

    #[test]
    fn each_choice_is_its_own_step_when_zoomed_in() {
        // 400 points a beat: every choice is far enough apart to be drawn as it is.
        let steps = [
            ("8 bars", 32.0),
            ("4 bars", 16.0),
            ("2 bars", 8.0),
            ("1 bar", 4.0),
            ("1/2", 2.0),
            ("1/4", 1.0),
            ("1/8", 0.5),
            ("1/16", 0.25),
            ("1/32", 0.125),
        ];
        for (value, beats) in steps {
            assert_eq!(step(value, 400.0, 4.0), beats, "{value}");
        }
        for (value, beats) in [("1/2T", 4.0 / 3.0), ("1/4T", 2.0 / 3.0), ("1/8T", 1.0 / 3.0), ("1/16T", 1.0 / 6.0)] {
            assert!((step(value, 400.0, 4.0) - beats).abs() < 1e-12, "{value}");
        }
        // Bars follow the time signature: a bar of 3/4 is three beats.
        assert_eq!(step("1 bar", 400.0, 3.0), 3.0);
        assert_eq!(step("2 bars", 400.0, 3.0), 6.0);
        assert!(Grid::new(Quantization::Beats(0.25), 400.0, 4.0).snaps);
    }

    #[test]
    fn a_straight_grid_coarsens_by_doubling_then_beats_then_bars() {
        // 1/16 is 0.25 beats: four lines a beat from 32 points a beat on.
        assert_eq!(step("1/16", 32.0, 4.0), 0.25);
        // Closer than 8 points: eighths, then quarters (the beat).
        assert_eq!(step("1/16", 31.0, 4.0), 0.5);
        assert_eq!(step("1/16", 16.0, 4.0), 0.5);
        assert_eq!(step("1/16", 15.0, 4.0), 1.0);
        assert_eq!(step("1/16", 8.0, 4.0), 1.0);
        // A beat under 8 points: the bar, then 2, 4, 8 bars.
        assert_eq!(step("1/16", 7.0, 4.0), 4.0);
        assert_eq!(step("1/16", 2.0, 4.0), 4.0);
        assert_eq!(step("1/16", 1.9, 4.0), 8.0);
        assert_eq!(step("1/16", 1.0, 4.0), 8.0);
        assert_eq!(step("1/16", 0.5, 4.0), 16.0);
        // In 3/4 the beat goes to a bar of three beats, then six.
        assert_eq!(step("1/8", 7.0, 3.0), 3.0);
        assert_eq!(step("1/8", 2.0, 3.0), 6.0);
        // A chosen value is never drawn finer than it is: 2 bars stays 2 bars zoomed in.
        assert_eq!(step("2 bars", 4000.0, 4.0), 8.0);
        assert_eq!(step("8 bars", 1.0, 4.0), 32.0);
        // A half note in 3/4 is not on the bar: once too narrow it goes to the bar.
        assert_eq!(step("1/2", 5.0, 3.0), 2.0);
        assert_eq!(step("1/2", 3.0, 3.0), 3.0);
    }

    #[test]
    fn a_triplet_grid_coarsens_to_the_beat_so_lines_stay_on_the_bar() {
        // 1/16T is a sixth of a beat: drawn from 48 points a beat on.
        assert!((step("1/16T", 48.0, 4.0) - 1.0 / 6.0).abs() < 1e-12);
        // Narrower: straight to the beat, never to a third of a beat that would leave the bar.
        assert_eq!(step("1/16T", 47.0, 4.0), 1.0);
        assert_eq!(step("1/8T", 20.0, 4.0), 1.0);
        assert_eq!(step("1/4T", 11.0, 3.0), 1.0);
        assert_eq!(step("1/4T", 7.0, 3.0), 3.0);
        // 1/2T (4/3 beats) goes to the bar.
        assert_eq!(step("1/2T", 5.0, 4.0), 4.0);
    }

    #[test]
    fn none_draws_beats_and_bars_by_the_zoom_and_does_not_snap() {
        let grid = Grid::new(Quantization::None, 400.0, 4.0);
        assert_eq!((grid.step, grid.snaps), (1.0, false));
        assert_eq!(step("none", 7.0, 4.0), 4.0);
        assert_eq!(grid.place(2.1337, false), 2.1337);
        assert_eq!(ruler_beat(2.1337, grid, false), 2.134);
        let clip = audio_clip();
        assert_eq!(fade_length(1.3, FadeEdge::In, &clip, 0.0, Some(grid)), 1.3);
    }

    #[test]
    fn places_snap_to_the_lines_inside_each_bar_unless_free() {
        let sixteenths = beats(0.25);
        assert_eq!(sixteenths.place(2.13, false), 2.25);
        assert_eq!(sixteenths.place(2.13, true), 2.13);
        assert_eq!(sixteenths.place(-1.0, false), 0.0);
        assert_eq!(sixteenths.place(f64::NAN, false), 0.0);
        // Triplet lines start again at each bar line: in 3/4 a half-note triplet (4/3 beats)
        // has lines at 0, 4/3 and 8/3, then the bar line at 3.
        let grid = Grid::new(Quantization::Beats(4.0 / 3.0), 400.0, 3.0);
        assert!((grid.place(1.5, false) - 4.0 / 3.0).abs() < 1e-12);
        assert_eq!(grid.place(2.9, false), 3.0);
        assert!((grid.place(4.4, false) - (3.0 + 4.0 / 3.0)).abs() < 1e-12);
        // Bars: on whole steps from the start.
        let two_bars = Grid::new(Quantization::Bars(2), 8.0, 4.0);
        assert_eq!(two_bars.place(11.0, false), 8.0);
        assert_eq!(two_bars.place(13.0, false), 16.0);
    }

    #[test]
    fn a_cell_starts_at_the_line_before_counted_from_its_bar() {
        // In 3/4 a two-beat step has lines at 0 and 2 in each bar, then the bar line at 3: a
        // click at 3.1 is in the cell that starts on the bar line, not at beat 2.
        let grid = Grid::new(Quantization::Beats(2.0), 400.0, 3.0);
        assert_eq!(grid.cell(3.1, false), 3.0);
        assert_eq!(grid.cell(2.9, false), 2.0);
        assert_eq!(grid.cell(5.5, false), 5.0);
        assert_eq!(grid.cell(3.1, true), 3.1);
        assert_eq!(grid.cell(-1.0, false), 0.0);
        // Bars: whole steps from the start.
        let two_bars = Grid::new(Quantization::Bars(2), 8.0, 4.0);
        assert_eq!(two_bars.cell(15.9, false), 8.0);
    }

    #[test]
    fn lines_are_drawn_at_every_step_with_bar_lines_marked() {
        assert_eq!(
            beats(1.0).lines(3.5, 6.0, 100),
            vec![(4.0, true), (5.0, false), (6.0, false)],
            "the bar before the view starts the lines"
        );
        let in_3_4 = Grid::new(Quantization::Beats(4.0 / 3.0), 400.0, 3.0);
        let lines: Vec<f64> = in_3_4.lines(0.0, 3.0, 100).into_iter().map(|(b, _)| (b * 1000.0).round()).collect();
        assert_eq!(lines, vec![0.0, 1333.0, 2667.0, 3000.0]);
        let two_bars = Grid::new(Quantization::Bars(2), 8.0, 4.0);
        assert_eq!(two_bars.lines(1.0, 17.0, 100), vec![(0.0, true), (8.0, true), (16.0, true)]);
        // Never more than asked for.
        assert_eq!(beats(0.125).lines(0.0, 1e9, 50).len(), 50);
        assert!(beats(1.0).lines(f64::NAN, 4.0, 10).is_empty());
    }

    #[test]
    fn a_ruler_click_lands_on_the_grid_unless_alt_is_held() {
        // 1 bar at 8 points a beat: the grid is a bar.
        let bars = Grid::new(Quantization::Bars(1), 8.0, 4.0);
        assert_eq!(ruler_beat(17.9, bars, false), 16.0);
        assert_eq!(ruler_beat(18.1, bars, false), 20.0);
        // 1/16 at 60 points a beat: sixteenths.
        let sixteenths = Grid::new(Quantization::Beats(0.25), 60.0, 4.0);
        assert_eq!(ruler_beat(2.13, sixteenths, false), 2.25);
        assert_eq!(ruler_beat(2.1337, sixteenths, true), 2.134);
        assert_eq!(ruler_beat(-3.0, bars, true), 0.0);
        assert_eq!(ruler_beat(f64::NAN, bars, false), 0.0);
    }
}
