// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The audio editor: the detail area's view of an audio clip (`docs/spec/ui-audio-editor.md`). It
//! draws the clip's whole file across the content's beats, as large as the area is, under a ruler
//! that counts bars, beats and sixteenths, with the part the clip plays lit, the loop's edges and
//! the clip's warp markers. Warp markers are added, moved and removed here, each a command
//! (`clip.addWarpMarker`, `clip.moveWarpMarker`, `clip.removeWarpMarker`, `clip.resetWarp`); the
//! view keeps only what is on screen: zoom and scroll, a marker being dragged and the reset dialog.

use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use serde_json::{Map, Value, json};
use suhina_engine::model::{Clip, ClipId, Project, TrackId};

use crate::arrangement::timeline::{AudioView, audio_view};
use crate::detail::piano_roll_grid::{GridMode, PianoGrid, Width};
use crate::interaction::grid::track_colour;
use crate::interaction::timeline_grid::TimelineGrid;
use crate::kit::element::{GRID_LINE, caption_ink, hint_ink, title_ink};
use crate::kit::theme;
use crate::kit::waveform::{self, Waveforms};
use crate::kit::widgets::Action;
use crate::settings::view_settings::{WaveformAppearance, WaveformColour};

/// Height of the ruler along the top, in points.
pub const RULER_HEIGHT: f32 = 28.0;
/// The least height the editor is given: a ruler and a waveform worth looking at.
pub const LEAST_HEIGHT: f32 = RULER_HEIGHT + 90.0;
/// Room left on either side of the file when it is fitted to the area.
const MARGIN: f32 = 8.0;
/// How much a drag on the ruler zooms: the zoom changes by this much (as an exponent) per point.
const RULER_ZOOM_PER_POINT: f32 = 0.012;
/// Zoom limits, in points per beat.
const BEAT_WIDTHS: std::ops::RangeInclusive<f32> = 0.05..=20_000.0;
/// Most pixel columns one frame lays out, whatever the area's width.
const MAX_COLUMNS: usize = 8192;
/// Grid lines (and so snap points) are never closer than this, in points.
const GRID_LEAST: f32 = 10.0;
/// A label is only written when its marks are at least this far apart, in points.
const LABEL_LEAST: f32 = 32.0;
/// How far from a marker's line a pointer still takes hold of it, in points.
const GRAB: f32 = 5.0;
/// Two warp markers stay at least this many beats apart, as a move needs.
const MARKER_GAP: f64 = 1.0 / 64.0;

/// An audio clip as the editor shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioClip {
    /// The params that name the clip in a `clip.*` command: `track` and `clip`, or `scene`.
    pub address: Value,
    /// The engine's mapping of the clip's content beats to its file.
    pub view: AudioView,
    /// The clip's length in project beats.
    pub length: f64,
    pub color: Color32,
    /// Beats in a bar.
    pub bar_beats: f64,
}

/// The editor for an arrangement clip, if it is an audio clip.
pub fn arrangement_clip(project: &Project, track: TrackId, id: ClipId) -> Option<AudioClip> {
    let lane = project.arrangement.lanes.iter().find(|l| l.track == Some(track))?;
    let clip = lane.clips.iter().find(|c| c.id == id)?;
    let mut shown = audio_clip_from(project, track, clip)?;
    shown.address = json!({ "track": track.0, "clip": id.0 });
    Some(shown)
}

/// The editor for a launcher slot's clip (`scene` counts from 0), if it is an audio clip.
pub fn launcher_clip(project: &Project, track: TrackId, scene: usize) -> Option<AudioClip> {
    let clip = project.launcher.columns.iter().find(|c| c.track == track)?.slots.get(scene)?.clip.as_ref()?;
    let mut shown = audio_clip_from(project, track, clip)?;
    shown.address = json!({ "track": track.0, "scene": scene });
    Some(shown)
}

fn audio_clip_from(project: &Project, track: TrackId, clip: &Clip) -> Option<AudioClip> {
    let content = project.content(clip.content)?;
    let view = audio_view(content, clip, project.transport.tempo_bpm)?;
    let position = project.tracks.iter().position(|t| t.id == track)?;
    // The clip's own colour when it has one; its track's otherwise (`Clip::color`).
    let color = clip.color.unwrap_or_else(|| {
        project.tracks.get(position).map(|track| track_colour(track, position)).unwrap_or(suhina_session::colours::MIDI)
    });
    let ts = project.transport.time_signature;
    let bar_beats =
        if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) };
    Some(AudioClip {
        address: json!({ "track": track.0 }),
        view,
        length: clip.length,
        color: Color32::from_rgb(color.0, color.1, color.2),
        bar_beats,
    })
}

/// A warp marker being dragged: where it was, how far inside the marker the pointer took hold of
/// it, and where it is now. The waveform is drawn with it there; the project has it once the
/// button is let go.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MarkerDrag {
    from: f64,
    grab: f64,
    to: f64,
}

/// What the editor keeps between frames: how far it is zoomed in and scrolled, for the clip it
/// shows (another clip starts fitted), its grid, a marker being dragged, and whether the reset
/// dialog is up.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioEditorState {
    /// Points per content beat; 0 until the first frame fits the file to the area.
    beat_width: f32,
    /// Points the file's first beat is left of the area's left edge.
    scroll: f32,
    /// The clip the zoom belongs to.
    shown: Option<Value>,
    /// The grid marks snap to: the piano roll's kind of grid, changed by the same keys (Ctrl+1
    /// finer, Ctrl+2 coarser, Ctrl+3 triplets, Ctrl+4 snap) and by the clip panel's Grid list.
    grid: PianoGrid,
    drag: Option<MarkerDrag>,
    /// The warp marker clicked last (its content beat), which Delete removes; lit while picked.
    picked: Option<f64>,
    /// The warp marker under the pointer in the last frame, which Delete removes when none is
    /// picked.
    hovered: Option<f64>,
    reset_open: bool,
    /// Pressed last: the grid keys are the editor's, as the piano roll's are while it has the
    /// keyboard.
    focused: bool,
}

impl Default for AudioEditorState {
    /// Not fitted yet, on an adaptive grid that snaps.
    fn default() -> Self {
        Self {
            beat_width: 0.0,
            scroll: 0.0,
            shown: None,
            grid: default_grid(),
            drag: None,
            picked: None,
            hovered: None,
            reset_open: false,
            focused: false,
        }
    }
}

/// The audio editor's grid until one is picked: adaptive, at the medium width, snapping.
pub fn default_grid() -> PianoGrid {
    PianoGrid { snap: true, mode: GridMode::Adaptive(Width::Medium), triplet: false }
}

impl AudioEditorState {
    /// The grid marks snap to.
    pub fn grid(&self) -> PianoGrid {
        self.grid
    }

    /// Picks the grid marks snap to.
    pub fn set_grid(&mut self, grid: PianoGrid) {
        self.grid = grid;
    }

    /// Whether the editor has the keyboard (it, or its clip's panel, was pressed last).
    pub fn focused(&self) -> bool {
        self.focused
    }

    /// Gives the editor the keyboard, or takes it away.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Lets go of the clip shown: a marker drag ends without a move, the reset dialog closes, the
    /// editor gives up the keyboard, and the next clip shown starts fitted. The grid stays.
    pub fn forget_clip(&mut self) {
        self.shown = None;
        self.drag = None;
        self.picked = None;
        self.hovered = None;
        self.reset_open = false;
        self.focused = false;
    }

    /// What Delete does while the editor has the keyboard: removes the warp marker picked last,
    /// or the one under the pointer, as a double-click on it does. `None` when there is none, or
    /// when `clip` keeps only the two a warped clip needs: the key then does nothing, and never
    /// acts on the clip behind the editor.
    pub fn delete_key(&self, clip: &AudioClip) -> Option<Action> {
        if self.shown.as_ref() != Some(&clip.address) || clip.view.markers.len() <= 2 {
            return None;
        }
        let beat = self.picked.or(self.hovered)?;
        clip.view.markers.iter().find(|m| (m.0 - beat).abs() < 1e-9)?;
        Some(Action::new("clip.removeWarpMarker", with_address(&clip.address, json!({ "beat": beat }))))
    }

    /// The warp marker picked last, for tests.
    pub fn picked(&self) -> Option<f64> {
        self.picked
    }

    /// Whether a warp marker is being dragged, for tests.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Zooms and scrolls so that the file fills `width` points.
    fn fit(&mut self, beats: f64, width: f32) {
        let room = (width - MARGIN * 2.0).max(1.0);
        let fitted = if beats > 0.0 { (f64::from(room) / beats) as f32 } else { 1.0 };
        self.beat_width = fitted.clamp(*BEAT_WIDTHS.start(), *BEAT_WIDTHS.end());
        self.scroll = -MARGIN;
    }

    /// Zooms by `factor` keeping the beat under `anchor` points from the area's left where it is.
    fn zoom(&mut self, factor: f32, anchor: f32) {
        if !(factor.is_finite() && factor > 0.0) || self.beat_width <= 0.0 {
            return;
        }
        let beat = (self.scroll + anchor) / self.beat_width;
        self.beat_width = (self.beat_width * factor).clamp(*BEAT_WIDTHS.start(), *BEAT_WIDTHS.end());
        self.scroll = beat * self.beat_width - anchor;
    }

    /// The points per beat now, for tests.
    pub fn beat_width(&self) -> f32 {
        self.beat_width
    }

    /// Whether the reset dialog is up, for tests.
    pub fn reset_open(&self) -> bool {
        self.reset_open
    }
}

/// What the editor reads from the window: the decoded waveforms and the waveform colour setting.
#[derive(Clone, Copy)]
pub struct Env<'a> {
    pub waveforms: &'a Waveforms,
    pub appearance: WaveformAppearance,
}

/// File seconds at content beat `beat`: the clip's warp markers joined by straight lines, the
/// first and last segments carried on past the ends, as the engine plays them; without two
/// markers a steady rate.
pub fn seconds_at(view: &AudioView, beat: f64) -> f64 {
    let markers = &view.markers;
    if markers.len() < 2 {
        return beat * view.seconds_per_beat;
    }
    let last = markers.len() - 1;
    let i = markers.partition_point(|m| m.0 <= beat).clamp(1, last);
    match (markers.get(i - 1), markers.get(i)) {
        (Some(&(b0, s0)), Some(&(b1, s1))) if b1 > b0 => s0 + (beat - b0) * (s1 - s0) / (b1 - b0),
        (Some(&(_, s0)), _) => s0,
        _ => 0.0,
    }
}

/// The content beat of file second `seconds`: the inverse of [`seconds_at`].
pub fn beat_at(view: &AudioView, seconds: f64) -> f64 {
    let markers = &view.markers;
    if markers.len() < 2 {
        return if view.seconds_per_beat > 0.0 { seconds / view.seconds_per_beat } else { 0.0 };
    }
    let last = markers.len() - 1;
    let i = markers.partition_point(|m| m.1 <= seconds).clamp(1, last);
    match (markers.get(i - 1), markers.get(i)) {
        (Some(&(b0, s0)), Some(&(b1, s1))) if s1 > s0 => b0 + (seconds - s0) * (b1 - b0) / (s1 - s0),
        (Some(&(b0, _)), _) => b0,
        _ => 0.0,
    }
}

/// The content beats the clip plays: from where it starts to where its loop ends, or to where its
/// length reaches, a loop's own start counting when it lies before the start.
pub fn played(view: &AudioView, length: f64) -> (f64, f64) {
    match view.loop_range {
        Some((start, end)) => (view.offset.min(start), end),
        None => (view.offset, view.offset + length * view.rate),
    }
}

/// The grid a grid key makes of `grid` (Ctrl+1 to Ctrl+4 by default, the piano roll's keys):
/// finer, coarser, triplets on or off, snapping on or off. `None` for another action.
pub fn grid_key(action: &str, grid: PianoGrid) -> Option<PianoGrid> {
    Some(match action {
        "pianoRoll.gridNarrow" => grid.narrower(),
        "pianoRoll.gridWiden" => grid.wider(),
        "pianoRoll.gridTriplet" => grid.toggle_triplet(),
        "pianoRoll.gridSnap" => grid.toggle_snap(),
        _ => return None,
    })
}

/// What the clip panel's buttons ask of the audio editor (`docs/spec/clip-panel.md` › Warp mode
/// settings): fit the file, open the Reset warp dialog, place warp markers on the file's
/// transients, or use another grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Request {
    Fit,
    Reset,
    Transients,
    Grid(PianoGrid),
}

fn requests_id() -> egui::Id {
    egui::Id::unique("audio-editor-requests")
}

fn shown_id() -> egui::Id {
    egui::Id::unique("audio-editor-shown")
}

/// Asks the audio editor for `request`; it is done when the editor is next drawn.
pub fn ask(ctx: &egui::Context, request: Request) {
    ctx.data_mut(|d| {
        let list = d.get_temp_mut_or_default::<Vec<Request>>(requests_id());
        if list.len() < 16 {
            list.push(request);
        }
    });
}

/// The grid of the audio editor drawn in the last frame or this one, for the clip panel's Grid
/// list; `None` while no audio editor is on show, and the panel leaves out its editor buttons.
pub fn shown_grid(ctx: &egui::Context) -> Option<PianoGrid> {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<(u64, PianoGrid)>(shown_id()))
        .filter(|(drawn, _)| drawn.saturating_add(1) >= pass)
        .map(|(_, grid)| grid)
}

/// `beat` on the grid with lines every `step` beats; `free` leaves it where it is.
pub fn snap(beat: f64, step: f64, free: bool) -> f64 {
    if free || step.is_nan() || step <= 0.0 || !beat.is_finite() { beat } else { (beat / step).round() * step }
}

/// Where `beat` is as the ruler counts it: `bar.beat.sixteenth`, each from 1.
pub fn position_label(beat: f64, bar_beats: f64) -> String {
    let bar_beats = if bar_beats > 0.0 { bar_beats } else { 4.0 };
    let bar = (beat / bar_beats).floor();
    let in_bar = beat - bar * bar_beats;
    let whole = in_bar.floor();
    let sixteenth = ((in_bar - whole) * 4.0).floor();
    format!("{}.{}.{}", bar as i64 + 1, whole as i64 + 1, sixteenth as i64 + 1)
}

/// Whether `beat` is (within a hair of) a whole number of `unit`s.
fn on_multiple(beat: f64, unit: f64) -> bool {
    unit > 0.0 && (beat / unit - (beat / unit).round()).abs() < 1e-6
}

/// Every how many bars a bar's number is written, so numbers are at least [`LABEL_LEAST`] apart.
fn label_stride(bar_width: f32) -> i64 {
    let mut stride = 1;
    while bar_width * (stride as f32) < LABEL_LEAST && stride < 1 << 30 {
        stride *= 2;
    }
    stride
}

/// `params` of a command on the clip: its address and the command's own.
fn with_address(address: &Value, own: Value) -> Value {
    let mut map: Map<String, Value> = address.as_object().cloned().unwrap_or_default();
    if let Value::Object(own) = own {
        map.extend(own);
    }
    Value::Object(map)
}

/// The colour the waveform is drawn in, as Settings › Waveform colour says: the clip's own (Auto),
/// the theme's `clip-waveform` colour where it gives one and the clip's otherwise (Theme), or the
/// light, dark or custom ink.
fn waveform_ink(appearance: WaveformAppearance, clip: Color32) -> Color32 {
    match appearance.mode {
        WaveformColour::Auto => clip,
        WaveformColour::Theme => crate::kit::element::CLIP_WAVEFORM.enter().color_or(clip),
        _ => appearance.ink(clip),
    }
}

/// Draws the editor for `clip` in `area`; the edits it makes go to `actions`. A file whose waveform
/// is not there (not decoded yet) shows a line saying so.
pub fn show(
    ui: &mut Ui,
    area: Rect,
    clip: &AudioClip,
    state: &mut AudioEditorState,
    env: Env<'_>,
    actions: &mut Vec<Action>,
) {
    let painter = ui.painter_at(area);
    let panel = crate::detail::detail_fill("panel");
    painter.rect_filled(area, 0.0, panel);
    let key = clip.address.to_string();
    let response = ui.interact(area, egui::Id::unique(("audio-editor", &key)), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, "Audio editor"));
    let Some(peaks) = env.waveforms.get(&clip.view.file).filter(|p| p.frames > 0 && p.sample_rate > 0) else {
        hint(&painter, area, "The audio is not loaded");
        return;
    };
    let duration = peaks.frames as f64 / f64::from(peaks.sample_rate);
    let (first, last) = (beat_at(&clip.view, 0.0), beat_at(&clip.view, duration));
    if !(first.is_finite() && last.is_finite() && last > first) {
        hint(&painter, area, "This audio has no length to show");
        return;
    }
    let ruler_rect = Rect::from_min_size(area.min, Vec2::new(area.width(), RULER_HEIGHT));
    let wave = Rect::from_min_max(Pos2::new(area.left(), ruler_rect.bottom()), area.max);
    if wave.height() < 4.0 || wave.width() < 4.0 {
        return;
    }
    // The ruler's own response, over the area's: a double-click on it fits the file again.
    // Hovering it shows a magnifier: drag up to zoom in, down to zoom out (around where the drag
    // began), sideways to scroll.
    let ruler_response =
        ui.interact(ruler_rect, egui::Id::unique(("audio-editor-ruler", &key)), Sense::click_and_drag());
    // The windowing layer has no magnifier cursor everywhere, so the pointer is hidden and the
    // piano roll's magnifier is drawn where it is.
    // It is there on hover and gone while the button is held, as on the piano roll's strip.
    if ruler_response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::None);
    } else if ruler_response.hovered()
        && let Some(pos) = ui.input(|i| i.pointer.latest_pos())
    {
        crate::detail::piano_roll::draw_magnifier(ui, pos);
    }
    // A clip not shown before starts with its file fitted to the area.
    if state.shown.as_ref() != Some(&clip.address) || state.beat_width <= 0.0 {
        state.shown = Some(clip.address.clone());
        state.fit(last - first, area.width());
        state.drag = None;
        state.picked = None;
        state.reset_open = false;
    }
    // A press here gives the editor the keyboard; one anywhere else takes it away (the clip's panel
    // gives it back, beside it).
    if ui.input(|i| i.pointer.any_pressed()) {
        state.focused = ui.rect_contains_pointer(area);
    }
    // What the clip panel's buttons asked for since the last frame.
    let asked = ui.ctx().data_mut(|d| d.remove_temp::<Vec<Request>>(requests_id())).unwrap_or_default();
    for request in asked {
        match request {
            Request::Fit => state.fit(last - first, area.width()),
            Request::Reset => state.reset_open = true,
            Request::Transients => {
                actions.push(Action::new("clip.addWarpMarkersAtTransients", with_address(&clip.address, json!({}))))
            }
            Request::Grid(grid) => state.grid = grid,
        }
    }
    let pass = ui.ctx().cumulative_pass_nr();
    let grid_now = state.grid;
    ui.ctx().data_mut(|d| d.insert_temp(shown_id(), (pass, grid_now)));
    if ruler_response.double_clicked() {
        state.fit(last - first, area.width());
    }
    // Zoom with Ctrl and the wheel (or a pinch), scroll with the wheel, drag the empty waveform to pan.
    if ui.rect_contains_pointer(area) {
        let (zoom, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
        if zoom != 1.0
            && let Some(pointer) = ui.input(|i| i.pointer.hover_pos())
        {
            state.zoom(zoom, pointer.x - area.left());
        }
        state.scroll -= if scroll.x != 0.0 { scroll.x } else { scroll.y };
    }
    if ruler_response.dragged_by(egui::PointerButton::Primary) {
        let delta = ruler_response.drag_delta();
        let anchor = ui.input(|i| i.pointer.press_origin()).map_or(area.width() / 2.0, |p| p.x - area.left());
        state.zoom((-delta.y * RULER_ZOOM_PER_POINT).exp(), anchor);
        state.scroll -= delta.x;
    } else if response.dragged_by(egui::PointerButton::Primary) && !ruler_response.is_pointer_button_down_on() {
        state.scroll -= response.drag_delta().x;
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    // The file's first beat may sit a margin left of the area and its last a margin right of it.
    let total = ((last - first) as f32 * state.beat_width).max(0.0);
    let most = (total + MARGIN * 2.0 - area.width()).max(0.0);
    state.scroll = state.scroll.clamp(-MARGIN, -MARGIN + most);
    let (beat_width, scroll) = (state.beat_width, state.scroll);
    let beat_at_x = move |x: f32| first + f64::from(x - area.left() + scroll) / f64::from(beat_width);
    let x_of = move |beat: f64| area.left() + ((beat - first) as f32) * beat_width - scroll;
    // The grid's lines: the grid picked, made coarser where they would crowd; marks snap to them
    // unless snapping is off (Ctrl+4) or Alt is held.
    let chosen = state.grid.step(beat_width, clip.bar_beats);
    let step = TimelineGrid::new(chosen, beat_width, clip.bar_beats, GRID_LEAST).step;
    let free = ui.input(|i| i.modifiers.alt) || !state.grid.snap;

    // The warp markers: a drag moves one (on the grid; Alt: free), a double-click takes it away.
    // A double-click on the waveform between them adds one where the sound is.
    let markers = &clip.view.markers;
    let editable = markers.len() >= 2;
    let mut hot = None;
    // Where a marker was let go this frame: the project has it there from the next frame on.
    let mut moved_to = None;
    if editable {
        for (i, &(beat, _)) in markers.iter().enumerate() {
            let dragged = state.drag.is_some_and(|d| (d.from - beat).abs() < 1e-9);
            let shown = if dragged { state.drag.map_or(beat, |d| d.to) } else { beat };
            let x = x_of(shown);
            // Markers off screen are left out, but never the one being dragged: the pointer may
            // carry it past an edge, and its release must still move it.
            if !dragged && !(area.left() - GRAB..=area.right() + GRAB).contains(&x) {
                continue;
            }
            // A dragged marker past an edge keeps a zone at that edge, so egui goes on reporting
            // its drag and its release.
            let zone_x = if dragged { x.clamp(area.left(), area.right()) } else { x };
            let zone =
                Rect::from_min_max(Pos2::new(zone_x - GRAB, area.top()), Pos2::new(zone_x + GRAB, area.bottom()))
                    .intersect(area);
            let id = egui::Id::unique(("audio-editor-marker", &key, i));
            let marker =
                ui.interact(zone, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            let label = format!("Warp marker {}", i + 1);
            marker.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, &label));
            if marker.hovered() || marker.dragged() {
                hot = Some(i);
            }
            if marker.clicked() {
                state.picked = Some(beat);
            }
            // Where the button went down, not where the drag was recognised: the marker keeps its
            // place under the pointer.
            if marker.drag_started_by(egui::PointerButton::Primary)
                && let Some(p) = ui.input(|i| i.pointer.press_origin()).or(marker.interact_pointer_pos())
            {
                state.drag = Some(MarkerDrag { from: beat, grab: beat_at_x(p.x) - beat, to: beat });
                state.picked = Some(beat);
            }
            if marker.dragged_by(egui::PointerButton::Primary)
                && let Some(p) = marker.interact_pointer_pos()
                && let Some(drag) = state.drag.as_mut().filter(|d| (d.from - beat).abs() < 1e-9)
            {
                // Between its neighbours, which it never passes.
                let low = i.checked_sub(1).and_then(|n| markers.get(n)).map_or(f64::NEG_INFINITY, |m| m.0 + MARKER_GAP);
                let high = markers.get(i + 1).map_or(f64::INFINITY, |m| m.0 - MARKER_GAP);
                let wanted = snap(beat_at_x(p.x) - drag.grab, step, free);
                drag.to = if low <= high { wanted.clamp(low, high) } else { drag.from };
            }
            // Only the drag this marker started: one kept from another clip or project is dropped.
            if marker.drag_stopped_by(egui::PointerButton::Primary)
                && let Some(drag) = state.drag.take()
                && dragged
                && (drag.to - drag.from).abs() > 1e-9
            {
                let own = json!({ "from": drag.from, "beat": drag.to });
                actions.push(Action::new("clip.moveWarpMarker", with_address(&clip.address, own)));
                state.picked = Some(drag.to);
                moved_to = Some(drag.to);
            }
            if marker.double_clicked() && markers.len() > 2 {
                state.drag = None;
                state.picked = None;
                actions
                    .push(Action::new("clip.removeWarpMarker", with_address(&clip.address, json!({ "beat": beat }))));
            }
        }
        if response.double_clicked()
            && let Some(p) = response.interact_pointer_pos().filter(|p| wave.contains(*p))
        {
            let beat = snap(beat_at_x(p.x), step, free);
            actions.push(Action::new("clip.addWarpMarker", with_address(&clip.address, json!({ "beat": beat }))));
        }
    }
    // A click on the waveform away from the markers puts the picked one down; one that is gone
    // (removed, or the project changed under it) is no longer picked.
    if response.clicked() && hot.is_none() {
        state.picked = None;
    }
    if state.drag.is_none()
        && state.picked.is_some_and(|b| moved_to != Some(b) && !markers.iter().any(|m| (m.0 - b).abs() < 1e-9))
    {
        state.picked = None;
    }
    state.hovered = hot.and_then(|i| markers.get(i)).map(|m| m.0);
    // The button is up and no marker took the release (its marker is gone, or Warp went off):
    // the drag ends without a move, and the waveform shows the project's markers again.
    if state.drag.is_some() && !ui.input(|i| i.pointer.primary_down()) {
        state.drag = None;
    }

    // The waveform with the marker being dragged where it is now, so the sound moves under it.
    let dragged_view = state.drag.map(|drag| {
        let mut moved = clip.view.clone();
        if let Some(marker) = moved.markers.iter_mut().find(|m| (m.0 - drag.from).abs() < 1e-9) {
            marker.0 = drag.to;
        }
        moved
    });
    let view = dragged_view.as_ref().unwrap_or(&clip.view);
    let count = (wave.width().ceil() as usize).min(MAX_COLUMNS);
    let per_point = f64::from(1.0 / beat_width);
    let columns: Vec<waveform::Column> = (0..count)
        .map(|i| {
            let x = wave.left() + i as f32;
            let beat = beat_at_x(x);
            let (a, b) = (seconds_at(view, beat), seconds_at(view, beat + per_point));
            waveform::Column { x: x + 0.5, from: a.min(b), to: a.max(b), gain: 1.0 }
        })
        .collect();
    let shape = waveform::geometry(peaks, wave.shrink2(Vec2::new(0.0, 2.0)), &columns, waveform::scale(ui.ctx()));
    // The grid lies under the waveform, which shows through it.
    grid_lines(&painter, wave, clip.bar_beats, &x_of, (beat_at_x(area.left()), beat_at_x(area.right())), step);
    let ink = waveform_ink(env.appearance, clip.color);
    waveform::paint(&painter.with_clip_rect(wave), &shape, waveform::Colors::from(ink));

    // The part the clip does not play is dimmed.
    let (from, to) = played(&clip.view, clip.length);
    let dim = panel.gamma_multiply(0.62);
    let (left, right) = (x_of(from).clamp(wave.left(), wave.right()), x_of(to).clamp(wave.left(), wave.right()));
    for (a, b) in [(wave.left(), left), (right, wave.right())] {
        if b > a {
            painter.rect_filled(Rect::from_x_y_ranges(a..=b, wave.y_range()), 0.0, dim);
        }
    }

    // The loop's edges, then the warp markers (the one the pointer is on, or dragged, brighter).
    let accent = clip.color;
    if let Some((start, end)) = clip.view.loop_range {
        for beat in [start, end] {
            let x = x_of(beat);
            if (wave.left()..=wave.right()).contains(&x) {
                painter.line_segment([Pos2::new(x, wave.top()), Pos2::new(x, wave.bottom())], Stroke::new(1.0, accent));
            }
        }
    }
    for (i, &(beat, _)) in view.markers.iter().enumerate() {
        let x = x_of(beat);
        if !(area.left() - 4.0..=area.right() + 4.0).contains(&x) {
            continue;
        }
        let lit = hot == Some(i) || state.picked.is_some_and(|b| (b - beat).abs() < 1e-9);
        let width = if lit { 2.0 } else { 1.0 };
        painter.line_segment(
            [Pos2::new(x, ruler_rect.bottom() - 6.0), Pos2::new(x, wave.bottom())],
            Stroke::new(width, if lit { title_ink() } else { accent }),
        );
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(x - 5.0, ruler_rect.bottom() - 10.0),
                Pos2::new(x + 5.0, ruler_rect.bottom() - 10.0),
                Pos2::new(x, ruler_rect.bottom() - 1.0),
            ],
            if lit { title_ink() } else { accent },
            Stroke::NONE,
        ));
    }
    if !editable {
        let note = "Warp is off: turn Warp on in the clip panel to place warp markers";
        painter.text(
            Pos2::new(wave.left() + 8.0, wave.top() + 6.0),
            Align2::LEFT_TOP,
            note,
            theme::ui_font(theme::TEXT_SMALL),
            hint_ink(),
        );
    }

    // The ruler last, over the markers' flags, with where the pointer is.
    let pointer = ui.input(|i| i.pointer.hover_pos()).filter(|p| area.contains(*p) && state.drag.is_none());
    ruler(&painter, ruler_rect, clip.bar_beats, &x_of, (beat_at_x(area.left()), beat_at_x(area.right())), step);
    let readout = pointer.map(|p| (p.x, snap(beat_at_x(p.x), step, free)));
    if let Some((x, beat)) = readout.filter(|_| hot.is_none()) {
        let line = Stroke::new(1.0, hint_ink().gamma_multiply(0.7));
        painter.line_segment([Pos2::new(x, ruler_rect.bottom()), Pos2::new(x, wave.bottom())], line);
        let text = position_label(beat, clip.bar_beats);
        let chip = Rect::from_center_size(Pos2::new(x, ruler_rect.top() + 9.0), Vec2::new(58.0, 15.0));
        painter.rect_filled(chip, 3.0, crate::detail::detail_fill("control"));
        painter.text(chip.center(), Align2::CENTER_CENTER, text, theme::mono_font(theme::TEXT_TINY + 1.0), title_ink());
    }
    if let Some(drag) = state.drag {
        let text = position_label(drag.to, clip.bar_beats);
        let chip = Rect::from_center_size(Pos2::new(x_of(drag.to), ruler_rect.top() + 9.0), Vec2::new(58.0, 15.0));
        painter.rect_filled(chip, 3.0, crate::detail::detail_fill("control"));
        painter.text(chip.center(), Align2::CENTER_CENTER, text, theme::mono_font(theme::TEXT_TINY + 1.0), title_ink());
    }

    if state.reset_open {
        match reset_dialog(ui.ctx(), markers.len()) {
            Some(Reset::Keep) => state.reset_open = false,
            Some(Reset::Even) => {
                state.reset_open = false;
                actions.push(Action::new("clip.resetWarp", with_address(&clip.address, json!({}))));
            }
            Some(Reset::Detect) => {
                state.reset_open = false;
                actions.push(Action::new("clip.resetWarp", with_address(&clip.address, json!({ "detect": true }))));
            }
            None => {}
        }
    }
}

/// What the reset dialog's buttons choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reset {
    /// Leave the warp markers as they are.
    Keep,
    /// Two markers at the clip's tempo.
    Even,
    /// Two markers at the tempo found again in the file.
    Detect,
}

/// The dialog that asks whether to keep the clip's warp markers or put the audio back on an even
/// grid; `None` while it is up and nothing is chosen. Escape or a click outside keeps them.
fn reset_dialog(ctx: &egui::Context, markers: usize) -> Option<Reset> {
    use crate::kit::widgets::{self, button};
    const WIDTH: f32 = 380.0;
    const BUTTON_HEIGHT: f32 = 18.0;
    let modal = crate::kit::floating::modal(ctx, egui::Id::unique("audio-editor-reset")).show(ctx, |ui| {
        ui.set_width(WIDTH);
        // The title and the question are the kit's text (golden rule 15), the question wrapped to
        // the dialog's width.
        let lines = [
            ("Reset warp markers".to_owned(), theme::ui_font(theme::TEXT + 2.0), title_ink()),
            (
                format!(
                    "This clip has {markers} warp markers. Keep them, or reset the warp so the audio runs evenly                      again, with one marker at each end? The clip's start, length and loop stay where they are."
                ),
                theme::ui_font(theme::TEXT_SMALL),
                caption_ink(),
            ),
        ];
        for (index, (text, font, ink)) in lines.into_iter().enumerate() {
            if index > 0 {
                ui.add_space(6.0);
            }
            let galley = ui.painter().layout(text.clone(), font, ink, WIDTH);
            let (rect, response) = ui.allocate_exact_size(Vec2::new(WIDTH, galley.size().y), Sense::hover());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &text));
            widgets::paint_text(ui.painter(), rect.min, galley, ink);
        }
        ui.add_space(10.0);
        // The three choices are the kit's buttons, side by side, each as wide as its words.
        let choices =
            [("Keep warp markers", Reset::Keep), ("Reset to clip tempo", Reset::Even), ("Find tempo again", Reset::Detect)];
        let font = theme::ui_font(theme::TEXT);
        let widths: Vec<f32> = choices
            .iter()
            .map(|(text, _)| ui.painter().layout_no_wrap((*text).to_owned(), font.clone(), title_ink()).size().x + 20.0)
            .collect();
        let gap = 6.0;
        let row_width = widths.iter().sum::<f32>() + gap * (widths.len() as f32 - 1.0);
        let (row, _) = ui.allocate_exact_size(Vec2::new(row_width.max(WIDTH), BUTTON_HEIGHT), Sense::hover());
        let painter = ui.painter().clone();
        let mut left = row.left();
        let mut choice = None;
        for ((text, pick), width) in choices.into_iter().zip(widths) {
            let rect = Rect::from_min_size(Pos2::new(left, row.top()), Vec2::new(width, BUTTON_HEIGHT));
            if button(ui, &painter, rect, text, text) {
                choice = Some(pick);
            }
            left += width + gap;
        }
        choice
    });
    modal.inner.or_else(|| modal.should_close().then_some(Reset::Keep))
}

/// The lines of the grid down the waveform: a bar's, a beat's and, closer in, the grid's own.
fn grid_lines(
    painter: &egui::Painter,
    wave: Rect,
    bar_beats: f64,
    x_of: &impl Fn(f64) -> f32,
    visible: (f64, f64),
    step: f64,
) {
    let bar_ink = GRID_LINE.attribute("kind", "bar").look().color();
    let beat_ink = GRID_LINE.attribute("kind", "beat").look().color();
    let bar_ink = if bar_ink.a() == 0 { hint_ink().gamma_multiply(0.5) } else { bar_ink };
    let beat_ink = if beat_ink.a() == 0 { hint_ink().gamma_multiply(0.3) } else { beat_ink };
    let (from, to) = (visible.0.min(visible.1), visible.0.max(visible.1));
    let (first, last) = ((from / step).floor() as i64, (to / step).ceil() as i64);
    for index in first..=last.min(first.saturating_add(4096)) {
        let beat = index as f64 * step;
        let x = x_of(beat);
        let ink = if on_multiple(beat, bar_beats) {
            bar_ink
        } else if on_multiple(beat, 1.0) {
            beat_ink
        } else {
            beat_ink.gamma_multiply(0.5)
        };
        painter.line_segment([Pos2::new(x, wave.top()), Pos2::new(x, wave.bottom())], Stroke::new(1.0, ink));
    }
}

/// The ruler: bar numbers (every few bars when they would crowd), beat numbers (`bar.beat`) once
/// beats are far enough apart, and sixteenths (`bar.beat.sixteenth`) when zoomed in far enough,
/// with a tick for every line of the grid.
fn ruler(
    painter: &egui::Painter,
    strip: Rect,
    bar_beats: f64,
    x_of: &impl Fn(f64) -> f32,
    visible: (f64, f64),
    step: f64,
) {
    let bar = if bar_beats > 0.0 { bar_beats } else { 4.0 };
    painter.rect_filled(strip, 0.0, crate::detail::detail_fill("window"));
    let line = hint_ink().gamma_multiply(0.6);
    painter.line_segment([strip.left_bottom(), strip.right_bottom()], theme::hairline(line));
    let number = caption_ink();
    let font = theme::mono_font(theme::TEXT_TINY + 1.0);
    let bar_width = (x_of(bar) - x_of(0.0)).abs();
    let beat_width = (x_of(1.0) - x_of(0.0)).abs();
    let stride = label_stride(bar_width);
    let (from, to) = (visible.0.min(visible.1), visible.0.max(visible.1));
    let (first, last) = ((from / step).floor() as i64, (to / step).ceil() as i64);
    for index in first..=last.min(first.saturating_add(4096)) {
        let beat = index as f64 * step;
        let x = x_of(beat);
        let (on_bar, on_beat) = (on_multiple(beat, bar), on_multiple(beat, 1.0));
        let tick = if on_bar {
            strip.height() - 4.0
        } else if on_beat {
            9.0
        } else {
            5.0
        };
        painter.line_segment(
            [Pos2::new(x, strip.bottom() - tick), Pos2::new(x, strip.bottom())],
            theme::hairline(if on_bar { number } else { line }),
        );
        let text = if on_bar {
            let n = (beat / bar).round() as i64;
            (n.rem_euclid(stride) == 0).then(|| (n + 1).to_string())
        } else if on_beat {
            (beat_width >= LABEL_LEAST).then(|| {
                position_label(beat, bar_beats).rsplit_once('.').map(|(a, _)| a.to_owned()).unwrap_or_default()
            })
        } else {
            (step <= 0.25 + 1e-9 && f64::from(beat_width) * step >= f64::from(LABEL_LEAST) + 4.0)
                .then(|| position_label(beat, bar_beats))
        };
        if let Some(text) = text {
            painter.text(
                Pos2::new(x + 3.0, strip.bottom() - tick - 2.0),
                Align2::LEFT_BOTTOM,
                text,
                font.clone(),
                if on_bar { number } else { hint_ink() },
            );
        }
    }
}

fn hint(painter: &egui::Painter, area: Rect, text: &str) {
    painter.text(area.center(), Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_TINY + 2.0), hint_ink());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(markers: &[(f64, f64)]) -> AudioView {
        AudioView {
            file: "a.wav".into(),
            offset: 0.0,
            loop_range: None,
            markers: markers.to_vec(),
            seconds_per_beat: 0.5,
            rate: 1.0,
            gain_db: 0.0,
            envelope: Vec::new(),
        }
    }

    #[test]
    fn an_unwarped_clip_plays_a_steady_rate() {
        let v = view(&[]);
        assert_eq!(seconds_at(&v, 4.0), 2.0);
        assert_eq!(beat_at(&v, 2.0), 4.0);
    }

    #[test]
    fn markers_join_by_straight_lines_and_the_ends_carry_on() {
        // 2 beats take 1 s, then 2 beats take 2 s.
        let v = view(&[(0.0, 0.0), (2.0, 1.0), (4.0, 3.0)]);
        assert_eq!(seconds_at(&v, 1.0), 0.5);
        assert_eq!(seconds_at(&v, 3.0), 2.0);
        assert_eq!(seconds_at(&v, 6.0), 5.0, "the last segment carries on");
        assert_eq!(seconds_at(&v, -2.0), -1.0, "the first segment carries on");
        for beat in [-2.0, 0.5, 2.0, 3.5, 6.0] {
            assert!((beat_at(&v, seconds_at(&v, beat)) - beat).abs() < 1e-9, "{beat}");
        }
    }

    #[test]
    fn markers_that_stand_still_in_the_file_do_not_divide_by_zero() {
        let v = view(&[(0.0, 1.0), (2.0, 1.0)]);
        assert!(seconds_at(&v, 1.0).is_finite());
        assert!(beat_at(&v, 1.0).is_finite());
    }

    #[test]
    fn the_played_part_runs_to_the_loops_end_or_the_clips_length() {
        let mut v = view(&[]);
        v.offset = 2.0;
        assert_eq!(played(&v, 4.0), (2.0, 6.0));
        v.rate = 2.0;
        assert_eq!(played(&v, 4.0), (2.0, 10.0));
        v.loop_range = Some((1.0, 5.0));
        assert_eq!(played(&v, 4.0), (1.0, 5.0));
    }

    #[test]
    fn fitting_fills_the_area_and_zooming_keeps_the_beat_under_the_pointer() {
        let mut state = AudioEditorState::default();
        state.fit(16.0, 816.0);
        assert!((state.beat_width() - 50.0).abs() < 1e-3);
        let before = (state.scroll + 100.0) / state.beat_width;
        state.zoom(2.0, 100.0);
        let after = (state.scroll + 100.0) / state.beat_width;
        assert!((before - after).abs() < 1e-4);
        assert!((state.beat_width() - 100.0).abs() < 1e-3);
    }

    #[test]
    fn a_zoom_that_is_not_a_number_changes_nothing() {
        let mut state = AudioEditorState::default();
        state.fit(8.0, 100.0);
        let kept = state.clone();
        state.zoom(f32::NAN, 10.0);
        state.zoom(-1.0, 10.0);
        assert_eq!(state, kept);
    }

    #[test]
    fn the_grid_starts_adaptive_and_the_grid_keys_change_it() {
        let grid = AudioEditorState::default().grid();
        assert!(grid.is_adaptive() && grid.snap);
        let finer = grid_key("pianoRoll.gridNarrow", grid);
        assert!(finer.is_some_and(|g| g != grid), "Ctrl+1 makes it finer");
        assert_eq!(finer.and_then(|g| grid_key("pianoRoll.gridWiden", g)), Some(grid), "Ctrl+2 back");
        assert_eq!(grid_key("pianoRoll.gridSnap", grid).map(|g| g.snap), Some(false), "Ctrl+4 stops snapping");
        assert_eq!(grid_key("pianoRoll.gridTriplet", grid).map(|g| g.triplet), Some(true));
        assert_eq!(grid_key("pianoRoll.zoomClip", grid), None);
        // A fixed grid as fine as 1/16 at a zoom where its lines would crowd is drawn coarser.
        let sixteenth = PianoGrid::from_value("1/16");
        let step = TimelineGrid::new(sixteenth.step(4.0, 4.0), 4.0, 4.0, GRID_LEAST).step;
        assert!(f64::from(4.0_f32) * step >= f64::from(GRID_LEAST), "{step}");
    }

    #[test]
    fn marks_snap_to_the_grid_unless_free() {
        assert_eq!(snap(2.3, 0.25, false), 2.25);
        assert_eq!(snap(2.4, 0.25, false), 2.5);
        assert_eq!(snap(2.3, 0.25, true), 2.3);
        assert_eq!(snap(2.3, 0.0, false), 2.3);
        assert!(snap(f64::NAN, 0.25, false).is_nan());
    }

    #[test]
    fn the_ruler_counts_bars_beats_and_sixteenths_from_one() {
        assert_eq!(position_label(0.0, 4.0), "1.1.1");
        assert_eq!(position_label(5.5, 4.0), "2.2.3");
        assert_eq!(position_label(3.99, 4.0), "1.4.4");
        assert_eq!(position_label(9.0, 3.0), "4.1.1");
        assert_eq!(position_label(-0.25, 4.0), "0.4.4", "before the first bar it counts back");
    }

    #[test]
    fn a_bar_number_is_written_every_few_bars_when_bars_crowd() {
        assert_eq!(label_stride(100.0), 1);
        assert_eq!(label_stride(20.0), 2);
        assert_eq!(label_stride(3.0), 16);
        assert_eq!(label_stride(0.0), 1 << 30);
    }

    #[test]
    fn the_waveform_colour_setting_says_what_ink_the_waveform_is_drawn_in() {
        let clip = Color32::from_rgb(10, 120, 200);
        let appearance = |mode| WaveformAppearance { mode, custom: Color32::from_rgb(1, 2, 3) };
        assert_eq!(waveform_ink(appearance(WaveformColour::Auto), clip), clip, "the clip's own colour");
        assert_eq!(waveform_ink(appearance(WaveformColour::Custom), clip), Color32::from_rgb(1, 2, 3));
        assert_eq!(waveform_ink(appearance(WaveformColour::Light), clip), Color32::from_rgb(240, 237, 232));
        assert_eq!(waveform_ink(appearance(WaveformColour::Dark), clip), Color32::from_rgb(32, 31, 29));
        // On Theme, with no theme rule for `clip-waveform`, it is the clip's colour too.
        assert_eq!(waveform_ink(appearance(WaveformColour::Theme), clip), clip);
    }

    #[test]
    fn a_marker_command_names_the_clip_and_adds_its_own_params() {
        let params = with_address(&json!({ "track": 3, "clip": 7 }), json!({ "beat": 2.5 }));
        assert_eq!(params, json!({ "track": 3, "clip": 7, "beat": 2.5 }));
    }
}
