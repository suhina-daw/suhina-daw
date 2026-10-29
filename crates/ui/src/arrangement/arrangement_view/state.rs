// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Arrangement's view state: zoom, scroll, selection, the drag under way and the settings it was given.

use super::*;

/// What a drag in the loop brace row changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum LoopGrab {
    /// Draws a new region from this beat (on the grid) to the pointer.
    New(f64),
    Start,
    End,
    /// Moves the whole region; the pointer was this many beats into it.
    Body(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Control {
    Volume,
    Pan,
}

/// A clip being dragged: where it came from, its unsnapped start and the lane under the pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ClipDrag {
    pub(super) track: TrackId,
    pub(super) clip: ClipId,
    pub(super) start: f64,
    pub(super) target: TrackId,
    /// Where the dragged clip started: the other selected clips move by as much as it does.
    pub(super) origin: f64,
    /// Ctrl (Cmd) was held as the drag began (Shift with Ctrl+drag selecting time): the clips
    /// are copied rather than moved.
    pub(super) copy: bool,
    /// The pointer is in the empty space under the tracks: dropped there, the clips go to a new
    /// track of their kind.
    pub(super) new_track: bool,
}

/// Which of a clip's edges is dragged. Its loop is turned on in its properties (the Loop
/// switch), never by an edge (#700); a looping clip's right edge goes on as far as it is dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EdgeGrab {
    Left,
    Right,
}

/// A clip's edge being dragged: the clip, which grip, and the beat the edge is at so far;
/// the beat it was at and where the button went down, which the edge follows the pointer from.
#[derive(Debug, Clone, Copy)]
pub(super) struct EdgeDrag {
    pub(super) key: (TrackId, ClipId),
    pub(super) grab: EdgeGrab,
    pub(super) raw: f64,
    pub(super) from: f64,
    pub(super) origin: f32,
}

/// Which end of a clip a fade belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FadeEdge {
    In,
    Out,
}

/// A fade handle being dragged: the clip, which fade, and its length so far in beats before
/// snapping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct FadeDrag {
    pub(super) key: (TrackId, ClipId),
    pub(super) edge: FadeEdge,
    pub(super) raw: f64,
    /// The fade's length when the drag started: the other selected clips' fades change by as
    /// much as this one has moved from it (#715).
    pub(super) from: f64,
}

/// A fade's curve handle being dragged: the clip, which fade, the curve so far, and how far
/// below the handle's centre the pointer took hold of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CurveDrag {
    pub(super) key: (TrackId, ClipId),
    pub(super) edge: FadeEdge,
    pub(super) curve: f64,
    pub(super) grab: f32,
}

/// A volume or pan being dragged in a track header: the track whose field took the pointer, which
/// control, the value it had when the drag started and the value it has now, and the tracks the
/// drag moves — the whole selection while the track is part of one, so they move in proportion.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ValueDrag {
    pub(super) track: TrackId,
    pub(super) control: Control,
    pub(super) start: f64,
    pub(super) now: f64,
    pub(super) tracks: Vec<TrackId>,
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
    /// The return tracks' rows are left out (the "Hide returns" button by the ruler).
    pub returns_hidden: bool,
    /// Moved clips stack on others instead of replacing them: audio clips, MIDI clips.
    pub stack_audio: bool,
    pub stack_midi: bool,
    /// How a drag over a clip selects time (Settings › Arrangement › Time selection): with Ctrl
    /// held when true; from the clip's upper half when false. A drag on empty lanes or on the ruler selects time either way.
    pub time_select_ctrl: bool,
    /// Alt held while an audio clip's edge is dragged stretches its audio to the new length
    /// (Settings › Arrangement › Ctrl+Alt-drag on an edge stretches audio).
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
    pub(super) heights: HashMap<TrackId, f32>,
    pub(super) sizes: TrackSizes,
    pub(super) apply_track_default: bool,
    pub(super) apply_return_default: bool,
    pub(super) expanded_heights: HashMap<TrackId, f32>,
    /// The selected clips, in the order they were picked, and the one clicked last: a Shift-click
    /// takes the run of clips from it, and the other views work on it.
    pub(super) selected: Vec<(TrackId, ClipId)>,
    pub(super) anchor: Option<(TrackId, ClipId)>,
    /// Where a drag over empty lanes started: the selection rectangle runs from there to the
    /// pointer until the button is let go.
    pub(super) marquee: Option<Pos2>,
    /// The drag that started `marquee` paints time (a lane's or a clip's upper half) rather than
    /// drawing a rectangle that picks whole clips (the lower half).
    pub(super) marquee_time: bool,
    /// The stacked row a time-painting drag started in: its track, the row and how many.
    pub(super) marquee_row: Option<(TrackId, usize, usize)>,
    /// The scroll when `marquee` was last placed: the view scrolls under a drag past its edge,
    /// and the rectangle's start moves with the lanes (#691).
    pub(super) marquee_scroll: Vec2,
    /// The span the last selection rectangle covered: its tracks and its beats.
    pub(super) time_selection: Option<TimeSelection>,
    /// Where a drag on the ruler started, in beats: it selects time over every track.
    pub(super) ruler_select: Option<f64>,
    /// A clip's edge being dragged.
    pub(super) edge_drag: Option<EdgeDrag>,
    /// An edge just let go of: the clip, where its edges are now and the frames left to wait.
    /// It is drawn there until the project has it, or the frame the button came up showed the
    /// clip as it was before the drag for an instant.
    pub(super) edge_released: Option<((TrackId, ClipId), f64, f64, u8)>,
    /// The beat an empty spot of a lane was right-clicked on, for its menu.
    pub(super) menu_beat: Option<f64>,
    pub(super) clip_drag: Option<ClipDrag>,
    /// The clip a press is held on before the pointer has moved enough for the drag to be
    /// decided: the view may switch under the held press (Tab), and the press then carries it.
    pub(super) pressed_clip: Option<(TrackId, ClipId)>,
    /// The empty space under the tracks this frame, top to bottom: a clip dropped there goes to
    /// a new track.
    pub(super) below_tracks: Option<(f32, f32)>,
    pub(super) value_drag: Option<ValueDrag>,
    /// A clip gain being dragged, and its value so far.
    pub(super) gain_drag: Option<((TrackId, ClipId), f64)>,
    /// The main track's volume being dragged, and its value so far.
    pub(super) main_drag: Option<f64>,
    /// A header's send knob being dragged: the track, the return and the level so far (dB).
    pub(super) send_drag: Option<(TrackId, TrackId, f64)>,
    /// The main row's height, its top edge dragged to change it (a row by default).
    pub(super) main_height: f32,
    pub(super) fade_drag: Option<FadeDrag>,
    pub(super) curve_drag: Option<CurveDrag>,
    /// The beat a clip was last right-clicked on, where its menu's Split here cuts.
    pub(super) split_at: Option<f64>,
    /// Tracks selected by clicking their headers (Ctrl adds, Shift takes a run), for grouping.
    pub selected_tracks: Vec<TrackId>,
    /// A track header was the last thing clicked: Delete removes the selected tracks then
    /// (#690). Picking or clearing clips takes it away.
    pub track_focus: bool,
    /// The track clicked last: where a Shift-click's run of tracks starts.
    pub(super) track_anchor: Option<TrackId>,
    /// The groups whose tracks are folded away, and what each folded row shows.
    pub folds: Folds,
    /// Where the volume, pan and gain fields were drawn last frame: the wheel over one changes its
    /// value instead of scrolling the view, which has to know before they draw themselves again.
    pub(super) value_areas: Vec<Rect>,
    pub(super) send_scroll_areas: Vec<Rect>,
    /// Where the waveform zoom slider was drawn last frame: the wheel over it steps the zoom
    /// and leaves the view where it is.
    pub(super) zoom_area: Option<Rect>,
    /// Volume and pan as knobs instead of fields (Settings › Session and mixer).
    pub knobs: bool,
    /// The track the user last worked on: the header they clicked, or the track of the clip they
    /// selected. A folded group shows that track while it is one of its own.
    pub(super) targeted: Option<TrackId>,
    /// A track header being dragged to a new place, and where it was let go.
    pub(super) track_drag: Option<TrackId>,
    pub(super) track_drop: Option<(TrackId, f32)>,
    /// The middle button is held after being pressed over the timeline: the view follows it.
    pub(super) panning: bool,
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
    /// The clips are picked whole (a click on a name bar, a rectangle, Ctrl+A, a paste): play
    /// starts from the first of them, as the setting says. A click in a clip's body picks it
    /// too, but play starts where it was clicked, as the insert marker shows.
    pub picked_whole: bool,
    /// A clip moves by its name bar and selects time from its body (the default); off
    /// (Lower half), its lower half moves it and its upper half selects time.
    pub name_bar_moves: bool,
    /// The grid's lines show through the clips' bodies, faintly (Settings › Arrangement ›
    /// TimelineGrid in clips; on by default).
    pub grid_in_clips: bool,
    /// How much larger audio clips' waveforms are drawn, 1 … 16: the display only, never the
    /// sound or the clip's gain (the slider by the ruler; Ctrl+Alt+Up and Down).
    pub waveform_zoom: f32,
    pub waveform_appearance: crate::settings::view_settings::WaveformAppearance,
    pub audio_colour_strength: u8,
    pub header_colour_strength: u8,
    /// Where the last click in a lane or a clip's body put the play start, on which track: a
    /// thin line there in that lane while the play start is still there.
    pub insert: Option<(TrackId, f64)>,
    /// A track header was double-clicked this frame: the window shows that track's devices
    /// (`docs/spec/ui-clip-editor.md` › Where it opens).
    pub open_devices: Option<TrackId>,
    /// Automation lanes under the tracks: which are shown, selected points, a drag.
    pub automation: LanesState,
    /// A marker being dragged: its index and where the pointer is, in beats before snapping.
    pub(super) marker_drag: Option<(usize, f64)>,
    /// The beat a right-click on the ruler or the marker row would add a marker at.
    pub(super) marker_menu_beat: f64,
    /// The name being typed for a marker in its menu.
    pub(super) marker_name: Option<(usize, String)>,
    /// A drag in the loop brace row: what it changes and where the pointer is, in beats before
    /// snapping.
    pub(super) loop_drag: Option<(LoopGrab, f64)>,
}

impl Default for ArrangementViewState {
    fn default() -> Self {
        Self {
            scroll: Vec2::ZERO,
            beat_width: DEFAULT_BEAT_WIDTH,
            grid: Quantization::Bars(1),
            headers_left: false,
            returns_hidden: false,
            stack_audio: false,
            stack_midi: false,
            time_select_ctrl: false,
            stretch_alt: true,
            double_click_does_nothing: false,
            ara_tracks: Vec::new(),
            ara_default: None,
            header_width: HEADER_COLUMN,
            heights: HashMap::new(),
            sizes: TrackSizes::default(),
            apply_track_default: false,
            apply_return_default: false,
            expanded_heights: HashMap::new(),
            selected: Vec::new(),
            anchor: None,
            marquee: None,
            marquee_time: false,
            marquee_row: None,
            marquee_scroll: Vec2::ZERO,
            time_selection: None,
            ruler_select: None,
            edge_drag: None,
            edge_released: None,
            menu_beat: None,
            clip_drag: None,
            pressed_clip: None,
            below_tracks: None,
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
            send_scroll_areas: Vec::new(),
            zoom_area: None,
            knobs: false,
            targeted: None,
            track_drag: None,
            track_drop: None,
            panning: false,
            drop: None,
            meters: Meters::default(),
            focus: None,
            clip_clicked: false,
            picked_whole: true,
            name_bar_moves: true,
            grid_in_clips: true,
            waveform_zoom: 1.0,
            waveform_appearance: crate::settings::view_settings::WaveformAppearance::default(),
            audio_colour_strength: 82,
            header_colour_strength: 82,
            insert: None,
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
    /// Painted from a clip in one row of a stack, on its track alone: that row (from 0) of how
    /// many. Consolidate then joins that row's clips only and leaves the other rows be.
    pub row: Option<(usize, usize)>,
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
    pub(super) fn is_selected(&self, key: (TrackId, ClipId)) -> bool {
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
        self.picked_whole = true;
        self.anchor = keys.first().copied();
        self.selected = keys;
    }

    /// Picks the clip at `key` the way a click on it does: Shift takes the rectangle from the last
    /// one picked to this one, the lanes between them (as the view draws them) by the time from
    /// the earlier start to the later end, every clip that touches it (#694); Ctrl (Cmd) adds it
    /// or takes it away, and a plain click leaves it the only one.
    pub(super) fn pick_clip(&mut self, key: (TrackId, ClipId), shift: bool, command: bool, order: &[ClipSpot]) {
        self.track_focus = false;
        let spot = |k: (TrackId, ClipId)| order.iter().find(|s| s.key == k).copied();
        if shift
            && let Some(anchor) = self.anchor
            && let (Some(from), Some(to)) = (spot(anchor), spot(key))
        {
            let rows = from.row.min(to.row)..=from.row.max(to.row);
            let (start, end) = (from.start.min(to.start), from.end.max(to.end));
            self.selected = order
                .iter()
                .filter(|s| rows.contains(&s.row) && s.start < end - 1e-9 && s.end > start + 1e-9)
                .map(|s| s.key)
                .collect();
            if !self.selected.contains(&key) {
                self.selected.push(key);
            }
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
    pub(super) fn pick_track(&mut self, track: TrackId, shift: bool, command: bool, order: &[TrackId]) {
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
    pub(super) fn working_on(&self) -> Option<TrackId> {
        self.targeted.or_else(|| self.anchor.map(|(track, _)| track))
    }

    /// The tracks a header's mixer control acts on: the whole selection while this track is one of
    /// two or more selected, so their volume and pan move in proportion, else this track alone.
    pub(super) fn controlled_tracks(&self, track: TrackId) -> Vec<TrackId> {
        if self.selected_tracks.len() > 1 && self.selected_tracks.contains(&track) {
            self.selected_tracks.clone()
        } else {
            vec![track]
        }
    }

    /// The clip being dragged with the mouse — or the one a press is held on, before the
    /// pointer has moved enough for the drag to be decided: its track and index.
    pub fn dragged_clip(&self) -> Option<(TrackId, ClipId)> {
        self.clip_drag.map(|d| (d.track, d.clip)).or(self.pressed_clip)
    }

    /// Ends a clip's drag without moving it, as when the window carries it to the other view.
    pub fn cancel_clip_drag(&mut self) {
        self.clip_drag = None;
        self.pressed_clip = None;
    }

    /// Forgets a press that was held on a clip whose widget has since gone away (scrolled out
    /// before the release), so it can carry nothing. A live drag is untouched.
    pub fn clear_pressed_clip(&mut self) {
        self.pressed_clip = None;
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
    pub(super) fn zoom_lanes(&mut self, lanes: &[Lane], factor: f32, anchor: f32) {
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
    pub(super) fn shown(&self, key: (TrackId, ClipId), clip: &ClipBox, grid: Option<TimelineGrid>) -> Shown {
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
        } else if let Some(drag) = self.fade_drag.filter(|d| self.is_selected(d.key) && self.is_selected(key)) {
            // Another selected clip's fade dragged: this one's moves by as much (#715).
            if clip.audio {
                let by = drag.raw - drag.from;
                match drag.edge {
                    FadeEdge::In => {
                        shown.fade_in = (shown.fade_in + by).clamp(0.0, (clip.length - shown.fade_out).max(0.0))
                    }
                    FadeEdge::Out => {
                        shown.fade_out = (shown.fade_out + by).clamp(0.0, (clip.length - shown.fade_in).max(0.0))
                    }
                }
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

    pub(super) fn height(&self, lane: &Lane) -> f32 {
        if lane.kind == ColumnKind::Return { self.return_height(lane.track) } else { self.lane_height(lane.track) }
    }

    /// How tall a return track's row is: a row's height until its edge is dragged.
    pub(super) fn return_height(&self, track: TrackId) -> f32 {
        self.heights.get(&track).copied().unwrap_or(f32::from(self.sizes.height(self.sizes.returns)))
    }

    /// Set every track and return to one named size, including folded/hidden rows.
    pub(super) fn resize_all(&mut self, timeline: &Timeline, size: TrackSize) {
        let height = f32::from(self.sizes.height(size));
        for lane in &timeline.lanes {
            self.heights.insert(lane.track, height);
            if !matches!(size, TrackSize::Tiny | TrackSize::Compact) {
                self.expanded_heights.insert(lane.track, height);
            }
        }
    }

    /// Apply named sizes live, retaining manual heights that do not match a preset.
    pub fn set_size_settings(&mut self, sizes: TrackSizes) {
        if sizes == self.sizes {
            return;
        }
        self.apply_track_default |= sizes.tracks != self.sizes.tracks;
        self.apply_return_default |= sizes.returns != self.sizes.returns;
        for height in self.heights.values_mut().chain(self.expanded_heights.values_mut()) {
            if let Some(size) = TrackSize::ALL.into_iter().find(|s| f32::from(self.sizes.height(*s)) == *height) {
                *height = f32::from(sizes.height(size));
            }
        }
        self.sizes = sizes;
    }

    /// A changed default applies to its rows immediately, including a previous Resize all.
    pub(crate) fn apply_size_defaults(&mut self, timeline: &Timeline) {
        if !self.apply_track_default && !self.apply_return_default {
            return;
        }
        for lane in &timeline.lanes {
            if if lane.kind == ColumnKind::Return { self.apply_return_default } else { self.apply_track_default } {
                self.heights.remove(&lane.track);
                self.expanded_heights.remove(&lane.track);
            }
        }
        self.apply_track_default = false;
        self.apply_return_default = false;
    }

    /// Toggle a lane between the smallest row and its last expanded height.
    pub(super) fn toggle_height(&mut self, track: TrackId, height: f32) {
        let compact = f32::from(self.sizes.height(TrackSize::Compact));
        if height > compact {
            self.expanded_heights.insert(track, height);
            self.heights.insert(track, compact);
        } else {
            self.heights.insert(
                track,
                self.expanded_heights.get(&track).copied().unwrap_or(f32::from(self.sizes.height(TrackSize::Small))),
            );
        }
    }

    /// How tall a track's row is: its own height once its edge is dragged or it is resized, else
    /// the theme's (`--suhina-track-height`), else the default size.
    pub fn lane_height(&self, track: TrackId) -> f32 {
        self.heights.get(&track).copied().unwrap_or_else(|| {
            crate::kit::style_class::theme_track_height().unwrap_or(f32::from(self.sizes.height(self.sizes.tracks)))
        })
    }

    /// How tall the view is when it shows only `track` ([`Self::focus`]): the ruler, the loop
    /// brace and marker rows under it, and the whole lane.
    pub fn focus_height(&self, track: TrackId) -> f32 {
        RULER + 1.0 + loop_row_height() + 1.0 + MARKER_ROW + 1.0 + self.lane_height(track) + 1.0
    }
}
