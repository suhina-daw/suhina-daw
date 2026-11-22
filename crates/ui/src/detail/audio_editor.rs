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
use crate::kit::element::{caption_ink, hint_ink, title_ink};
use crate::kit::theme;
use crate::kit::waveform::{self, Waveforms};
use crate::kit::widgets::Action;

/// Height of the ruler along the top, in points.
pub const RULER_HEIGHT: f32 = 28.0;
/// Height of the marker lane under the ruler: the warp markers' flags and the transients' marks.
pub const MARKER_LANE: f32 = 14.0;
/// The least height the editor is given: a ruler, the marker lane and a waveform worth looking at.
pub const LEAST_HEIGHT: f32 = HEADER_HEIGHT + RULER_HEIGHT + LOOP_ROW + MARKER_LANE + 90.0;
/// Height of the loop row between the ruler and the marker lane: the clip's loop as a bar.
pub const LOOP_ROW: f32 = 10.0;
/// How near the clip start marker, in points, a press in the loop row's lower half takes it.
const LOOP_EDGE: f32 = 5.0;
/// Height of the bar over the ruler: the file's name and format, and the Sample / Envelopes tabs.
pub const HEADER_HEIGHT: f32 = 20.0;
/// How wide the header's Sample / Envelopes tabs are, centered in it.
const TABS_WIDTH: f32 = 150.0;
/// Room left on either side of the file when it is fitted to the area.
const MARGIN: f32 = 8.0;
/// How far the editor's frame stays in from the detail area's right edge, in points.
const FRAME_INSET: f32 = 6.0;
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
/// Beats are numbered (`bar.beat`) once they are this far apart, and sixteenths
/// (`bar.beat.sixteenth`) once they are: one level at a time, each with room around its numbers,
/// rather than every level as soon as its numbers just fit, which wrote them in a crowded row.
const LEVEL_LEAST: f32 = 60.0;
/// How far from a marker's line a pointer still takes hold of it, in points.
const GRAB: f32 = 5.0;
/// Two warp markers stay at least this many beats apart, as a move needs.
const MARKER_GAP: f64 = 1.0 / 64.0;
/// How close to a transient, in points, a new marker goes onto it instead of onto the grid.
const TRANSIENT_GRAB: f32 = 8.0;
/// Files whose transients are found at once, each on a thread of its own.
const TRANSIENT_JOBS: usize = 2;
/// The least the transients on show may be apart on average, in points, for the marker lane to
/// show them.
const TRANSIENT_LEAST: f32 = 12.0;
/// Frames a released marker is still drawn where it was let go while the project takes the move.
const LANDING_FRAMES: u8 = 30;

/// An audio clip as the editor shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioClip {
    /// The params that name the clip in a `clip.*` command: `track` and `clip`, or `scene`.
    pub address: Value,
    /// The clip's id, which `clip.setEnvelope` names it by.
    pub id: ClipId,
    /// The engine's mapping of the clip's content beats to its file.
    pub view: AudioView,
    /// The clip's length in project beats.
    pub length: f64,
    /// Where it starts on the arrangement, in project beats; 0 for a slot's clip, whose beats
    /// count from its launch.
    pub start: f64,
    /// A slot's clip: it plays from its launch for as long as it is not stopped.
    pub endless: bool,
    pub color: Color32,
    /// Beats in a bar.
    pub bar_beats: f64,
    /// The file's name, as the header shows it.
    pub name: String,
    /// The other audio clips picked with it in the Arrangement. Those as long as it, warped, take
    /// every warp marker edit made on it at the same beats, one undo step for all
    /// (`docs/spec/ui-audio-editor.md` › Several clips); the header lists them all.
    pub others: Vec<AudioClip>,
}

/// The editor for an arrangement clip, if it is an audio clip.
pub fn arrangement_clip(project: &Project, track: TrackId, id: ClipId) -> Option<AudioClip> {
    let lane = project.arrangement.lanes.iter().find(|l| l.track == Some(track))?;
    let clip = lane.clips.iter().find(|c| c.id == id)?;
    let mut shown = audio_clip_from(project, track, clip)?;
    shown.address = json!({ "track": track.0, "clip": id.0 });
    shown.start = clip.start;
    Some(shown)
}

/// The editor for a launcher slot's clip (`scene` counts from 0), if it is an audio clip.
pub fn launcher_clip(project: &Project, track: TrackId, scene: usize) -> Option<AudioClip> {
    let clip = project.launcher.columns.iter().find(|c| c.track == track)?.slots.get(scene)?.clip.as_ref()?;
    let mut shown = audio_clip_from(project, track, clip)?;
    shown.address = json!({ "track": track.0, "scene": scene });
    shown.endless = true;
    Some(shown)
}

fn audio_clip_from(project: &Project, track: TrackId, clip: &Clip) -> Option<AudioClip> {
    let content = project.content(clip.content)?;
    let view = audio_view(content, track, clip, project.transport.tempo_bpm)?;
    let position = project.tracks.iter().position(|t| t.id == track)?;
    // The clip's own color when it has one; its track's otherwise (`Clip::color`).
    let color = clip.color.unwrap_or_else(|| {
        project.tracks.get(position).map(|track| track_colour(track, position)).unwrap_or(suhina_session::colours::MIDI)
    });
    let ts = project.transport.time_signature;
    let bar_beats =
        if ts.denominator == 0 { 4.0 } else { f64::from(ts.numerator.max(1)) * 4.0 / f64::from(ts.denominator) };
    Some(AudioClip {
        address: json!({ "track": track.0 }),
        id: clip.id,
        view,
        length: clip.length,
        start: 0.0,
        endless: false,
        color: Color32::from_rgb(color.0, color.1, color.2),
        bar_beats,
        name: content.name.clone(),
        others: Vec::new(),
    })
}

/// `action`, a warp marker edit made on `clip`, with the same edit on each clip picked with it that
/// is as long as it and warped (`AudioClip::others`), all in one gesture (the action's own, else a
/// new one), so the edit is one undo step: a marker moved, removed or added at a beat there is
/// moved, removed or added at that beat in every one, a clip without a marker on that beat getting
/// one first, pinned where its sound is. A move goes to every joined clip or to none: when it
/// would take a marker of one of them onto or past its neighbor (closer than [`MARKER_GAP`]), or
/// a marker it needs added would lie before its file's start, none of them get it, so no clip is
/// left with an added marker its move then refused. Another action, or a clip with none joined,
/// is `action` alone.
pub fn for_joined(clip: &AudioClip, action: Action) -> Vec<Action> {
    const EDITS: [&str; 5] = [
        "clip.addWarpMarker",
        "clip.moveWarpMarker",
        "clip.moveWarpMarkers",
        "clip.removeWarpMarker",
        "clip.removeWarpMarkers",
    ];
    let joined: Vec<&AudioClip> = clip.joined().collect();
    if joined.is_empty() || !EDITS.contains(&action.id) {
        return vec![action];
    }
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let gesture = action.params.get("gesture").cloned().unwrap_or_else(|| {
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        crate::interaction::commands::gesture(format_args!("warp-clips {n}"))
    });
    let own = |params: &Value| {
        let mut params = params.clone();
        params["gesture"] = gesture.clone();
        params
    };
    let mut out = vec![Action::new(action.id, own(&action.params))];
    let number = |key: &str| action.params.get(key).and_then(Value::as_f64);
    let numbers = |key: &str| -> Vec<f64> {
        action
            .params
            .get(key)
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default()
    };
    // A move: the beats it moves and by how much, the same for every clip.
    let moving: Option<(Vec<f64>, f64)> = match action.id {
        "clip.moveWarpMarker" => number("from").zip(number("beat")).map(|(from, to)| (vec![from], to - from)),
        "clip.moveWarpMarkers" => number("by").map(|by| (numbers("beats"), by)),
        _ => None,
    };
    if let Some((beats, by)) = &moving
        && !joined.iter().all(|other| moves_cleanly(&other.view, beats, *by))
    {
        return out;
    }
    for other in joined {
        let has = |beat: f64| other.view.markers.iter().any(|m| (m.0 - beat).abs() < 1e-6);
        let on = |id: &'static str, params: Value| Action::new(id, own(&with_address(&other.address, params)));
        let add = |beat: f64| on("clip.addWarpMarker", json!({ "beat": beat }));
        let count = other.view.markers.len();
        match action.id {
            "clip.addWarpMarker" => out.extend(number("beat").map(add)),
            "clip.moveWarpMarker" => {
                if let (Some(from), Some(to)) = (number("from"), number("beat")) {
                    if !has(from) {
                        out.push(add(from));
                    }
                    out.push(on("clip.moveWarpMarker", json!({ "from": from, "beat": to })));
                }
            }
            "clip.moveWarpMarkers" => {
                let beats = numbers("beats");
                out.extend(beats.iter().copied().filter(|b| !has(*b)).map(add));
                if let Some(by) = number("by") {
                    out.push(on("clip.moveWarpMarkers", json!({ "beats": beats, "by": by })));
                }
            }
            "clip.removeWarpMarker" => {
                if let Some(beat) = number("beat").filter(|b| has(*b) && count > 2) {
                    out.push(on("clip.removeWarpMarker", json!({ "beat": beat })));
                }
            }
            _ => {
                let beats: Vec<f64> = numbers("beats").into_iter().filter(|b| has(*b)).collect();
                if !beats.is_empty() && count.saturating_sub(beats.len()) >= 2 {
                    out.push(on("clip.removeWarpMarkers", json!({ "beats": beats })));
                }
            }
        }
    }
    out
}

/// Whether the warp markers of `view` at `beats` (each added first where it has none) can move by
/// `by` together, as `clip.addWarpMarker` and `clip.moveWarpMarker(s)` take them: an added marker
/// lies where the file is (at or after its start) on markers that rise in seconds, and every marker
/// keeps at least [`MARKER_GAP`] from its neighbors after the move, none passing another.
fn moves_cleanly(view: &AudioView, beats: &[f64], by: f64) -> bool {
    if !by.is_finite() || beats.is_empty() {
        return false;
    }
    let same = |a: f64, b: f64| (a - b).abs() < 1e-6;
    let rising = view.markers.windows(2).all(|w| w[1].0 > w[0].0 && w[1].1 > w[0].1);
    let mut all: Vec<f64> = view.markers.iter().map(|m| m.0).collect();
    for &beat in beats {
        if all.iter().any(|b| same(*b, beat)) {
            continue;
        }
        let seconds = seconds_at(view, beat);
        if !(beat.is_finite() && rising && seconds.is_finite() && seconds >= 0.0) {
            return false;
        }
        all.push(beat);
    }
    all.sort_by(f64::total_cmp);
    let moved: Vec<f64> = all.iter().map(|&b| if beats.iter().any(|m| same(*m, b)) { b + by } else { b }).collect();
    moved.windows(2).all(|w| w[1] - w[0] >= MARKER_GAP - 1e-9)
}

impl AudioClip {
    /// The clips picked with it whose warp markers move with its: as long as it, and warped.
    pub fn joined(&self) -> impl Iterator<Item = &AudioClip> {
        self.others.iter().filter(move |o| (o.length - self.length).abs() < 1e-6 && o.view.markers.len() >= 2)
    }

    /// The content beat that plays at `at` (an arrangement beat, or for a slot's clip the beats
    /// since its launch), through its start, its rate and its loop; `None` outside the clip.
    pub fn content_beat(&self, at: f64) -> Option<f64> {
        let elapsed = at - self.start;
        if !(elapsed >= 0.0 && (self.endless || elapsed < self.length)) {
            return None;
        }
        let mut beat = self.view.offset + elapsed * self.view.rate;
        if let Some((start, end)) = self.view.loop_range
            && end > start
            && beat >= end
        {
            beat = start + (beat - end).rem_euclid(end - start);
        }
        beat.is_finite().then_some(beat)
    }

    /// The arrangement beat at which content beat `beat` first plays, held to the clip's span:
    /// where a click on the waveform puts the playhead. The inverse of [`AudioClip::content_beat`]:
    /// in a looping clip a beat of the loop before the start marker is first played when the loop
    /// comes round to it, after the stretch from the start marker to the loop's end. A beat the
    /// clip never plays (before the start marker without a loop, past the loop's end) is held to
    /// the clip's start or end. `None` for a slot's clip, or a clip with no rate.
    pub fn arrangement_beat(&self, beat: f64) -> Option<f64> {
        let rate = self.view.rate;
        if self.endless || rate.is_nan() || rate <= 0.0 || !beat.is_finite() {
            return None;
        }
        let offset = self.view.offset;
        let elapsed = match self.view.loop_range.filter(|(start, end)| end > start && (*start..*end).contains(&beat)) {
            Some((start, end)) => {
                // Where playback enters the content: a start marker at or past the loop's end turns
                // into the loop at once, as `content_beat` (and the engine) wrap it.
                let from = if offset >= end { start + (offset - end).rem_euclid(end - start) } else { offset };
                let ahead = if beat >= from { beat - from } else { (end - from) + (beat - start) };
                ahead / rate
            }
            None => (beat - offset) / rate,
        };
        let at = self.start + elapsed;
        at.is_finite().then(|| at.clamp(self.start, self.start + self.length))
    }
}

/// A warp marker being dragged: where it was, how far inside the marker the pointer took hold of
/// it, and where it is now, with the other picked markers it carries by as much. The waveform is
/// drawn with them there; the project has them once the button is let go.
#[derive(Debug, Clone, PartialEq)]
struct MarkerDrag {
    from: f64,
    grab: f64,
    to: f64,
    /// The other picked markers moved with it, where they were.
    others: Vec<f64>,
}

impl MarkerDrag {
    /// Where the marker that was at `beat` is now, if the drag moves it.
    fn moves(&self, beat: f64) -> Option<f64> {
        let by = self.to - self.from;
        std::iter::once(self.from).chain(self.others.iter().copied()).find(|b| (b - beat).abs() < 1e-9).map(|b| b + by)
    }

    /// Whether a marker the drag moves is at `beat` now.
    fn lands_on(&self, beat: f64) -> bool {
        let by = self.to - self.from;
        std::iter::once(self.from).chain(self.others.iter().copied()).any(|b| (b + by - beat).abs() < 1e-9)
    }

    /// Every marker the drag moves, where they were, in order.
    fn moved(&self) -> Vec<f64> {
        let mut all: Vec<f64> = std::iter::once(self.from).chain(self.others.iter().copied()).collect();
        all.sort_by(f64::total_cmp);
        all
    }
}

/// Where the `picked` markers are drawn: carried by `drag` while it lasts (they are where it began
/// until it is let go); after the release they are the beats they were let go at already, and
/// are drawn there as they are, never moved by the landing drag a second time.
fn picked_now(picked: &[f64], drag: Option<&MarkerDrag>) -> Vec<f64> {
    picked.iter().map(|&b| drag.and_then(|d| d.moves(b)).unwrap_or(b)).collect()
}

/// The beats of the `markers` inside `span` (its ends included), in order; none without one.
fn markers_in(markers: &[(f64, f64)], span: Option<(f64, f64)>) -> Vec<f64> {
    let Some((start, end)) = span else { return Vec::new() };
    markers.iter().map(|m| m.0).filter(|b| (start - 1e-9..=end + 1e-9).contains(b)).collect()
}

/// How far the markers at the beats `moved` may go together, earlier (the first, at most 0) and
/// later (the second, at least 0): each stays at least [`MARKER_GAP`] from a neighbor that is not
/// moved. `markers` are the clip's, in beat order.
fn group_bounds(markers: &[(f64, f64)], moved: &[f64]) -> (f64, f64) {
    let is_moved = |beat: f64| moved.iter().any(|b| (b - beat).abs() < 1e-9);
    let (mut low, mut high) = (f64::NEG_INFINITY, f64::INFINITY);
    for (j, &(beat, _)) in markers.iter().enumerate() {
        if !is_moved(beat) {
            continue;
        }
        if let Some(&(below, _)) = j.checked_sub(1).and_then(|n| markers.get(n)).filter(|m| !is_moved(m.0)) {
            low = low.max(below + MARKER_GAP - beat);
        }
        if let Some(&(above, _)) = markers.get(j + 1).filter(|m| !is_moved(m.0)) {
            high = high.min(above - MARKER_GAP - beat);
        }
    }
    (low.min(0.0), high.max(0.0))
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
    /// The id of the clip shown: a launcher slot keeps its address when its clip is replaced, and
    /// the clip that took its place starts fitted, with no dialog or drag of the one before.
    shown_id: Option<ClipId>,
    /// The grid marks snap to: the piano roll's kind of grid, changed by the same keys (Ctrl+1
    /// finer, Ctrl+2 coarser, Ctrl+3 triplets, Ctrl+4 snap) and by the clip panel's Grid list.
    grid: PianoGrid,
    drag: Option<MarkerDrag>,
    /// A marker let go, drawn where it was let go until the project has it there (or for
    /// [`LANDING_FRAMES`] frames), so the sound does not flash back to where the drag began.
    landing: Option<(MarkerDrag, u8)>,
    /// The warp markers picked (their content beats, in order): the one clicked, more added with
    /// Ctrl or Shift held, or those in a painted time selection. Delete removes them and a drag of
    /// one moves them all; lit while picked.
    picked: Vec<f64>,
    /// The warp marker under the pointer in the last frame, which Delete removes when none is
    /// picked.
    hovered: Option<f64>,
    reset_open: bool,
    /// The tab on show: the sample (its warp markers) or its gain envelope.
    tab: Tab,
    /// The time selection, in content beats (start before end): painted by a drag on the
    /// waveform; Ctrl+L loops the clip over it.
    span: Option<(f64, f64)>,
    /// Where a drag that paints the time selection began, in content beats, while it lasts.
    painting: Option<f64>,
    /// The middle button went down over the editor and is held: the view follows the pointer.
    panning: bool,
    /// The loop bar being dragged: which part, the content beat the pointer took it at, and the
    /// loop then.
    loop_drag: Option<(LoopPart, f64, (f64, f64))>,
    /// Where the right-click menu was opened, while it is open.
    menu_at: Option<MenuAt>,
    /// What Quantize (Ctrl+U) moves the transients to: one of the piano roll's quantize grids
    /// ([`QUANTIZE_GRIDS`](crate::detail::piano_roll::QUANTIZE_GRIDS)), and how far, 0 to 100
    /// percent.
    quantize_grid: &'static str,
    quantize_amount: f64,
    /// Where the shown clip's file begins and ends in content beats, as last drawn: what Ctrl+A
    /// selects.
    file_span: Option<(f64, f64)>,
    /// Pressed last: the grid keys are the editor's, as the piano roll's are while it has the
    /// keyboard.
    focused: bool,
    /// A click's seek on the ruler or the waveform, waiting out the double-click time.
    waiting: WaitingClick,
}

impl Default for AudioEditorState {
    /// Not fitted yet, on an adaptive grid that snaps.
    fn default() -> Self {
        Self {
            beat_width: 0.0,
            scroll: 0.0,
            shown: None,
            shown_id: None,
            grid: default_grid(),
            drag: None,
            landing: None,
            picked: Vec::new(),
            hovered: None,
            reset_open: false,
            tab: Tab::Sample,
            span: None,
            painting: None,
            panning: false,
            loop_drag: None,
            menu_at: None,
            quantize_grid: "1/16",
            quantize_amount: 100.0,
            file_span: None,
            focused: false,
            waiting: WaitingClick::default(),
        }
    }
}

/// A single click's action held back for the double-click time, so a click that turns out to be
/// the first of a double-click does not do it: egui reports a click on both presses of a
/// double-click, and a double-click on the ruler fits the file and one on the waveform adds a
/// warp marker, neither of which seeks.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct WaitingClick(Option<(f64, Action)>);

impl WaitingClick {
    /// A click at `now`, `count` presses in a row (2 for a double-click, 3 for a triple one): a
    /// single click waits with its action (if any), the one waiting before it sent first; a double
    /// or triple click drops what waits.
    pub(crate) fn click(&mut self, now: f64, count: u32, action: Option<Action>, out: &mut Vec<Action>) {
        if count > 1 {
            self.0 = None;
            return;
        }
        if let Some((_, before)) = self.0.take() {
            out.push(before);
        }
        self.0 = action.map(|action| (now, action));
    }

    /// Sends what waits once `delay` seconds have passed since its click with no second press;
    /// true while something still waits.
    pub(crate) fn tick(&mut self, now: f64, delay: f64, out: &mut Vec<Action>) -> bool {
        match self.0.take() {
            Some((at, action)) if now - at >= delay => {
                out.push(action);
                false
            }
            Some(waiting) => {
                self.0 = Some(waiting);
                true
            }
            None => false,
        }
    }
}

/// How many presses in a row `response`'s click is: 1, 2 for a double-click, 3 for a triple one.
fn presses(response: &egui::Response) -> u32 {
    if response.triple_clicked() {
        3
    } else if response.double_clicked() {
        2
    } else {
        1
    }
}

/// Where the right-click menu was opened: the point clicked on the grid (Alt held: free), where a
/// marker added from the menu goes (on the marker lane, the transient suggested near it), and the
/// marker it was opened on, if any.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MenuAt {
    beat: f64,
    marker_beat: f64,
    on_marker: Option<f64>,
}

/// What the right-click menu reads besides the editor's state.
struct MenuCx<'a> {
    clip: &'a AudioClip,
    at: MenuAt,
    /// The grid's step now, in beats.
    step: f64,
    /// The file's length in beats and the area's width, for Zoom to fit.
    fit: (f64, f32),
    /// The keys, whose labels the entries show.
    keymap: &'a crate::settings::keymap::Keymap,
}

/// The right-click menu's items (`docs/spec/ui-audio-editor.md` › Right-click menu), compact as
/// the piano roll's, each with its key and grayed where there is nothing to act on; what one picks
/// is pushed to `actions` or done to the view.
fn menu_items(ui: &mut Ui, cx: &MenuCx<'_>, state: &mut AudioEditorState, actions: &mut Vec<Action>) {
    use crate::kit::menu;
    let spacing = ui.spacing_mut();
    spacing.item_spacing.y = 0.0;
    spacing.button_padding.y = 1.0;
    spacing.interact_size.y = MENU_ROW;
    let clip = cx.clip;
    let at = cx.at;
    let address = &clip.address;
    let markers = &clip.view.markers;
    let warped = markers.len() >= 2;
    let key = |action: &str| cx.keymap.label(action);
    let pick = |ui: &mut Ui, text: &str, shortcut: Option<String>, enabled: bool| {
        let clicked = menu::action(ui, text, shortcut.as_deref(), enabled).clicked();
        if clicked {
            ui.close();
        }
        clicked
    };
    let line = |ui: &mut Ui| menu::separator(ui);

    // The picked markers (or the one opened on), as Delete takes them.
    let delete = state.delete_key(clip).or_else(|| {
        at.on_marker
            .filter(|_| markers.len() > 2)
            .map(|beat| Action::new("clip.removeWarpMarker", with_address(address, json!({ "beat": beat }))))
    });
    if pick(ui, "Delete", key("selection.delete"), delete.is_some())
        && let Some(action) = delete
    {
        state.picked.clear();
        actions.push(action);
    }
    line(ui);
    let quantize = state.quantize_key(clip);
    if pick(ui, "Quantize", key("pianoRoll.quantize"), quantize.is_some())
        && let Some(action) = quantize
    {
        actions.push(action);
    }
    // The quantize grid and amount, beside the menu: no dialog to open first.
    let (grid, amount) = state.quantize();
    menu::submenu(ui, &format!("Quantize settings: {grid}, {amount:.0} %"), false, |ui| {
        menu::heading(ui, "Grid");
        for g in crate::detail::piano_roll::QUANTIZE_GRIDS {
            if menu::item(ui, g, g == grid).clicked() {
                state.quantize_grid = g;
                ui.close();
            }
        }
        menu::separator(ui);
        menu::heading(ui, "Amount");
        for a in [100.0, 75.0, 50.0, 25.0] {
            if menu::item(ui, &format!("{a:.0} %"), (a - amount).abs() < 1e-9).clicked() {
                state.quantize_amount = a;
                ui.close();
            }
        }
    });
    line(ui);
    if pick(ui, "Add warp marker here", None, warped) {
        actions.push(Action::new("clip.addWarpMarker", with_address(address, json!({ "beat": at.marker_beat }))));
    }
    if pick(ui, "Add warp markers at transients", None, warped) {
        actions.push(Action::new("clip.addWarpMarkersAtTransients", with_address(address, json!({}))));
    }
    if pick(ui, "Reset warp…", None, true) {
        state.reset_open = true;
    }
    line(ui);
    let looped = state.loop_span().is_some();
    if pick(ui, "Loop selection", key("pianoRoll.loop"), looped) {
        actions.extend(state.loop_key(clip));
    }
    if pick(ui, "Set clip start here", None, true) {
        actions.push(Action::new(
            "clip.setStart",
            with_address(address, json!({ "offset": round_beat(at.beat.max(0.0)) })),
        ));
    }
    let new_start = clip.view.loop_range.and_then(|range| loop_start_at(range, at.beat, cx.step));
    if pick(ui, "Set loop start here", None, new_start.is_some())
        && let Some(own) = new_start
    {
        let own = json!({ "start": own.0, "end": own.1 });
        actions.push(Action::new("clip.setLoop", with_address(address, own)));
    }
    let new_end = clip.view.loop_range.and_then(|range| loop_end_at(range, at.beat, cx.step));
    if pick(ui, "Set loop end here", None, new_end.is_some())
        && let Some(own) = new_end
    {
        let own = json!({ "start": own.0, "end": own.1 });
        actions.push(Action::new("clip.setLoop", with_address(address, own)));
    }
    line(ui);
    // An arrangement clip, where the point clicked plays inside it.
    let split = clip.arrangement_beat(at.beat).filter(|s| {
        *s > clip.start + 1e-6 && *s < clip.start + clip.length - 1e-6 && clip.address.get("clip").is_some()
    });
    if pick(ui, "Split clip here", None, split.is_some())
        && let Some(song) = split
    {
        actions.push(Action::new("clip.split", with_address(address, json!({ "at": round_beat(song) }))));
    }
    if pick(ui, "Zoom to fit", None, true) {
        state.fit(cx.fit.0, cx.fit.1);
    }
    line(ui);
    // The grid's entries, as the header's grid list and the piano roll's menu hold them.
    let actions_of_keys =
        ["pianoRoll.gridSnap", "pianoRoll.gridNarrow", "pianoRoll.gridWiden", "pianoRoll.gridTriplet"];
    let keys = crate::interaction::grid_menu::grid_key_labels(cx.keymap, &actions_of_keys);
    if let Some(grid) = crate::interaction::grid_menu::grid_entries(ui, state.grid, &keys, &|_| true) {
        state.grid = grid;
        ui.close();
    }
}

/// The least a loop set from the right-click menu keeps between its ends: a grid `step`, or a 64th
/// of a beat with no grid.
fn menu_loop_gap(step: f64) -> f64 {
    if step > 0.0 && step.is_finite() { step } else { 1.0 / 64.0 }
}

/// The loop *Set loop start here* makes of `range` with the point clicked at `beat`, its ends to a
/// thousandth of a beat: the start there, from beat 0, at least a grid `step` before the end, or
/// as far before it as there is room from beat 0 when the loop is shorter than that. `None` when
/// the point is not before the end by half that gap (the item is grayed).
fn loop_start_at(range: (f64, f64), beat: f64, step: f64) -> Option<(f64, f64)> {
    let (_, end) = range;
    let gap = menu_loop_gap(step).min(end);
    if !(beat.is_finite() && end > 0.0 && beat < end - gap * 0.5) {
        return None;
    }
    let start = round_beat(beat.max(0.0).min(end - gap));
    let end = round_beat(end);
    (end > start).then_some((start, end))
}

/// The loop *Set loop end here* makes of `range` with the point clicked at `beat`, its ends to a
/// thousandth of a beat: the end there, at least a grid `step` after the start. `None` when the
/// point is not after the start by half that gap (the item is grayed).
fn loop_end_at(range: (f64, f64), beat: f64, step: f64) -> Option<(f64, f64)> {
    let (start, _) = range;
    let gap = menu_loop_gap(step);
    if !(beat.is_finite() && beat > start + gap * 0.5) {
        return None;
    }
    let end = round_beat(beat.max(start + gap));
    let start = round_beat(start);
    (end > start).then_some((start, end))
}

/// The height of one of the right-click menu's rows, as the piano roll's.
const MENU_ROW: f32 = 16.0;

/// The part of the loop bar a drag holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopPart {
    Start,
    End,
    Whole,
    /// The clip's start marker, the triangle under the bar.
    Marker,
}

/// The loop `from` becomes when `part` of it is dragged by `by` beats: an end stays at least a
/// grid `step` (or a 64th of a beat) from the other, the whole bar keeps its length.
fn dragged_loop(from: (f64, f64), part: LoopPart, by: f64, step: f64, free: bool) -> (f64, f64) {
    let least = if free || step <= 0.0 { 1.0 / 64.0 } else { step };
    let (start, end) = from;
    match part {
        LoopPart::Start => (snap(start + by, step, free).min(end - least).max(0.0), end),
        LoopPart::End => (start, snap(end + by, step, free).max(start + least)),
        LoopPart::Whole => {
            let moved = snap(start + by, step, free).max(0.0);
            (moved, moved + (end - start))
        }
        // (the start marker is not the loop: the loop stays)
        LoopPart::Marker => from,
    }
}

/// What the editor's tabs show over the same waveform and ruler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// The sample: its warp markers, the time selection, the play start.
    Sample,
    /// The clip's gain envelope, edited on the waveform as on the clip in the Arrangement.
    Envelopes,
}

/// The audio editor's grid until one is picked: adaptive, at the narrow width, snapping.
pub fn default_grid() -> PianoGrid {
    PianoGrid { snap: true, mode: GridMode::Adaptive(Width::Narrow), triplet: false }
}

/// The finest note value an adaptive grid of the audio editor goes to, in beats: a 4096th note.
/// Zoomed in on a few cycles of a wave, a marker is put on a sample or two, so the lines go much
/// finer than the piano roll's and the Arrangement's (to a 128th note).
const EDITOR_FINEST: f64 = 4.0 / 4096.0;
/// How much further apart than the piano roll's an adaptive width keeps the editor's lines: Narrow
/// 18 points rather than 12, so the file fitted to the area is on 32nds (as warping is usually
/// done), not on 64ths that a wide area just had room for.
const EDITOR_SPACING: f64 = 1.5;

/// The step of `grid` at `beat_width` points per beat: a fixed value's, or for an adaptive grid
/// the finest power-of-two note value from a 4096th note to a half note (triplets: two thirds of
/// it), then one bar, two, four and on, whose lines are at least the width's spacing apart.
fn editor_step(grid: PianoGrid, beat_width: f32, bar_beats: f64) -> f64 {
    let GridMode::Adaptive(width) = grid.mode else { return grid.step(beat_width, bar_beats) };
    let bar = if bar_beats > 0.0 { bar_beats } else { 4.0 };
    if !(beat_width.is_finite() && beat_width > 0.0) {
        return bar;
    }
    let (points, spacing) = (f64::from(beat_width), width.spacing() * EDITOR_SPACING);
    let third = if grid.triplet { 2.0 / 3.0 } else { 1.0 };
    let mut value = EDITOR_FINEST;
    while value < bar {
        if value * third * points >= spacing {
            return value * third;
        }
        value *= 2.0;
    }
    let mut step = bar;
    for _ in 0..16 {
        if step * points >= spacing {
            return step;
        }
        step *= 2.0;
    }
    step
}

/// `step` beats as the grid's value is written: `1/16` for a sixteenth (`1/12` for its triplet),
/// `1 Bar`, `2 Bars` and on for whole bars, else the beats.
fn step_label(step: f64, bar_beats: f64) -> String {
    let bar = if bar_beats > 0.0 { bar_beats } else { 4.0 };
    if step >= bar - 1e-9 && on_multiple(step, bar) {
        let bars = (step / bar).round() as i64;
        return if bars == 1 { "1 Bar".to_owned() } else { format!("{bars} Bars") };
    }
    let per_note = 4.0 / step;
    if (per_note - per_note.round()).abs() < 1e-6 && per_note >= 1.0 {
        return format!("1/{}", per_note.round() as i64);
    }
    format!("{step:.3} beats")
}

/// The points per beat at which a file `beats` long fills `width` points, a margin left at each
/// end: the furthest the editor zooms out.
fn fitted_width(beats: f64, width: f32) -> f32 {
    let room = (width - MARGIN * 2.0).max(1.0);
    let fitted = if beats > 0.0 { (f64::from(room) / beats) as f32 } else { 1.0 };
    fitted.clamp(*BEAT_WIDTHS.start(), *BEAT_WIDTHS.end())
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
    /// editor gives up the keyboard, a click waiting out the double-click time is dropped, and the
    /// next clip shown starts fitted. The grid stays.
    pub fn forget_clip(&mut self) {
        self.shown = None;
        self.shown_id = None;
        self.drag = None;
        self.landing = None;
        self.picked.clear();
        self.hovered = None;
        self.reset_open = false;
        self.span = None;
        self.painting = None;
        self.loop_drag = None;
        self.file_span = None;
        self.focused = false;
        self.waiting = WaitingClick::default();
    }

    /// What the loop key (Ctrl+L, the piano roll's) does while the editor has the keyboard: loops
    /// the clip over the time selection and starts it there, so the clip plays that stretch round
    /// and round over its whole length (the start marker left before it played the audio up to
    /// the loop first); pressed again over the same loop, turns it off. With no time selection it
    /// turns the clip's loop on or off. A selection too short to be a loop (it rounds to no length,
    /// or lies wholly before beat 0) leaves the loop as it is. Nothing when the editor shows
    /// another clip.
    pub fn loop_key(&self, clip: &AudioClip) -> Vec<Action> {
        if self.shown.as_ref() != Some(&clip.address) {
            return Vec::new();
        }
        let set_loop = |own: Value| Action::new("clip.setLoop", with_address(&clip.address, own));
        if self.span.is_none() {
            return vec![set_loop(json!({ "loop": clip.view.loop_range.is_none() }))];
        }
        let Some((start, end)) = self.loop_span() else { return Vec::new() };
        let same = clip.view.loop_range.is_some_and(|(s, e)| (s - start).abs() < 1e-6 && (e - end).abs() < 1e-6);
        if same {
            return vec![set_loop(json!({ "loop": false }))];
        }
        vec![
            set_loop(json!({ "start": start, "end": end })),
            Action::new("clip.setStart", with_address(&clip.address, json!({ "offset": start }))),
        ]
    }

    /// The loop the time selection makes, as the commands are given beats: its ends to a
    /// thousandth of a beat, from beat 0 at the earliest (a clip's loop and start are at beat 0 or
    /// later, so a selection painted from before the clip's first beat, where a warp marker puts
    /// the file's start earlier, loops from 0). `None` without a selection, or for one that is
    /// no loop at those beats.
    fn loop_span(&self) -> Option<(f64, f64)> {
        let (start, end) = self.span.map(|(s, e)| (round_beat(s.max(0.0)), round_beat(e)))?;
        (end > start).then_some((start, end))
    }

    /// What Quantize (Ctrl+U, the piano roll's key) does while the editor shows `clip`: moves the
    /// transients in the time selection (none: the whole clip) towards the quantize grid by the
    /// quantize amount (`clip.quantizeWarpMarkers`). Nothing when the editor shows another clip or
    /// the clip is not warped.
    pub fn quantize_key(&self, clip: &AudioClip) -> Option<Action> {
        if self.shown.as_ref() != Some(&clip.address) || clip.view.markers.len() < 2 {
            return None;
        }
        let mut own = json!({ "grid": self.quantize_grid, "amount": self.quantize_amount });
        if let Some((start, end)) = self.span.filter(|(s, e)| e > s) {
            own["start"] = json!(start);
            own["end"] = json!(end);
        }
        Some(Action::new("clip.quantizeWarpMarkers", with_address(&clip.address, own)))
    }

    /// What Select all (Ctrl+A, the piano roll's key) does while the editor has the keyboard: the
    /// time selection becomes the whole file, its warp markers all picked with it. Nothing when
    /// the editor shows another clip or has not drawn it yet.
    pub fn select_all(&mut self, clip: &AudioClip) {
        if self.shown.as_ref() != Some(&clip.address) {
            return;
        }
        if let Some(span) = self.file_span {
            self.span = Some(span);
            self.picked = markers_in(&clip.view.markers, self.span);
        }
    }

    /// The quantize grid and amount, for tests and the menu.
    pub fn quantize(&self) -> (&'static str, f64) {
        (self.quantize_grid, self.quantize_amount)
    }

    /// The tab on show, for tests.
    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// The time selection, for tests.
    pub fn span(&self) -> Option<(f64, f64)> {
        self.span
    }

    /// What Delete does while the editor has the keyboard: removes the warp markers picked (one
    /// command for them all), or the one under the pointer, as a double-click on it does. `None`
    /// when there is none, or when removing them would leave `clip` fewer than the two a warped
    /// clip needs: the key then does nothing, and never acts on the clip behind the editor.
    pub fn delete_key(&self, clip: &AudioClip) -> Option<Action> {
        let markers = &clip.view.markers;
        if self.shown.as_ref() != Some(&clip.address) || markers.len() <= 2 {
            return None;
        }
        let exists = |beat: &f64| markers.iter().any(|m| (m.0 - beat).abs() < 1e-9);
        let picked: Vec<f64> = self.picked.iter().copied().filter(exists).collect();
        if picked.len() > 1 {
            if markers.len() - picked.len() < 2 {
                return None;
            }
            return Some(Action::new(
                "clip.removeWarpMarkers",
                with_address(&clip.address, json!({ "beats": picked })),
            ));
        }
        let beat = picked.first().copied().or(self.hovered).filter(exists)?;
        Some(Action::new("clip.removeWarpMarker", with_address(&clip.address, json!({ "beat": beat }))))
    }

    /// The warp markers picked, for tests.
    pub fn picked(&self) -> &[f64] {
        &self.picked
    }

    /// Whether a warp marker is being dragged, for tests.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Starts on `clip` when it is not the one shown (another address, or another clip in the same
    /// launcher slot) or nothing is fitted yet: its file of `beats` fitted to `width` points, and
    /// nothing of the clip before kept (a drag, the picked markers, the time selection, the reset
    /// dialog, a click waiting out the double-click time, whose seek was a place in that clip).
    fn show_clip(&mut self, clip: &AudioClip, beats: f64, width: f32) {
        if self.shown.as_ref() == Some(&clip.address) && self.shown_id == Some(clip.id) && self.beat_width > 0.0 {
            return;
        }
        self.shown = Some(clip.address.clone());
        self.shown_id = Some(clip.id);
        self.fit(beats, width);
        self.drag = None;
        self.landing = None;
        self.picked.clear();
        self.reset_open = false;
        self.span = None;
        self.painting = None;
        self.loop_drag = None;
        self.menu_at = None;
        self.waiting = WaitingClick::default();
    }

    /// Zooms and scrolls so that the file fills `width` points.
    fn fit(&mut self, beats: f64, width: f32) {
        self.beat_width = fitted_width(beats, width);
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

/// What the editor reads from the window: the decoded waveforms and the waveform color setting.
#[derive(Clone, Copy)]
pub struct Env<'a> {
    pub waveforms: &'a Waveforms,
    /// Settings › Clip editor › Audio editor waveform colour is Theme: the waveform takes the
    /// theme's `clip-waveform` color instead of the clip's.
    pub theme_waveform: bool,
    /// Settings › Clip editor › Audio editor › Drag warp markers by their line: a marker's line down
    /// the waveform takes the pointer as its flag does; off, only the flag on the marker lane does.
    pub line_drag: bool,
    /// Where playback is while the transport runs: an arrangement beat, or for a slot's clip the
    /// beats since its launch ([`AudioClip::content_beat`] takes either).
    pub playhead: Option<f64>,
    /// Where playback starts next, for an arrangement clip: an arrangement beat (a click on the
    /// waveform moves it).
    pub start_marker: Option<f64>,
    /// The keys, whose labels the right-click menu shows.
    pub keymap: &'a crate::settings::keymap::Keymap,
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

/// `view` as seconds are read back to beats ([`beat_at`]): its warp markers that rise in seconds
/// as they do in beats, each kept only when its second is after the last one kept, and a steady
/// rate when fewer than two are left. The engine plays markers that stand still in the file or
/// fall (a project from elsewhere), and [`seconds_at`] draws them as they play; only the way back
/// needs them rising, or a flat end segment would put every second past it on one beat. Borrowed
/// when the markers rise already.
pub fn readable(view: &AudioView) -> std::borrow::Cow<'_, AudioView> {
    use std::borrow::Cow;
    let markers = &view.markers;
    if markers.windows(2).all(|w| w[1].0 > w[0].0 && w[1].1 > w[0].1) {
        return Cow::Borrowed(view);
    }
    let mut kept: Vec<(f64, f64)> = Vec::with_capacity(markers.len());
    for &marker in markers {
        if kept.last().is_none_or(|last| marker.0 > last.0 && marker.1 > last.1) {
            kept.push(marker);
        }
    }
    if kept.len() < 2 {
        kept.clear();
    }
    Cow::Owned(AudioView { markers: kept, ..view.clone() })
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

/// `beat` on the grid with lines every `step` beats; `free` leaves it where it is.
pub fn snap(beat: f64, step: f64, free: bool) -> f64 {
    if free || step.is_nan() || step <= 0.0 || !beat.is_finite() { beat } else { (beat / step).round() * step }
}

/// Where `beat` is as the ruler counts it: `bar.beat.sixteenth`, each from 1. `None` for a beat
/// that is not a number, or so far out that its bar has no number (a ruler or a pointer there
/// writes nothing rather than a wrong count).
pub fn position_label(beat: f64, bar_beats: f64) -> Option<String> {
    let bar_beats = if bar_beats > 0.0 && bar_beats.is_finite() { bar_beats } else { 4.0 };
    let bar = (beat / bar_beats).floor();
    // Past what a whole number of bars holds (or a float stops telling beats apart) there is no
    // count to write.
    if !(bar.is_finite() && bar.abs() < 1e15) {
        return None;
    }
    let in_bar = (beat - bar * bar_beats).clamp(0.0, bar_beats);
    let whole = in_bar.floor();
    let sixteenth = ((in_bar - whole) * 4.0).floor();
    let count = |n: f64| (n as i64).checked_add(1);
    Some(format!("{}.{}.{}", count(bar)?, count(whole)?, count(sixteenth)?))
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

/// `beat` to a thousandth of a beat, as the commands are given beats.
fn round_beat(beat: f64) -> f64 {
    (beat * 1000.0).round() / 1000.0
}

/// `params` of a command on the clip: its address and the command's own.
fn with_address(address: &Value, own: Value) -> Value {
    let mut map: Map<String, Value> = address.as_object().cloned().unwrap_or_default();
    if let Value::Object(own) = own {
        map.extend(own);
    }
    Value::Object(map)
}

/// The color the waveform is drawn in, as Settings › Clip editor › Audio editor waveform colour
/// says: the clip's color (its own, else its track's, as the Arrangement colors the clip), or
/// the theme's: its `clip-waveform` color where it gives one, else a neutral ink that reads on
/// the editor's `panel` (the clip's color there made Theme look the same as Track colour).
fn waveform_ink(theme: bool, clip: Color32, panel: Color32) -> Color32 {
    if theme {
        crate::kit::element::CLIP_WAVEFORM.enter().color_or(theme::text_on(panel.to_opaque()).gamma_multiply(0.7))
    } else {
        clip
    }
}

/// The file seconds of the transients of `file`, found once per file and kept for the window: the
/// marks on the marker lane, and where a new marker goes when it is placed near one. They are
/// found on a thread of their own (a long song takes seconds), none until they are: the window
/// never waits for them, and is drawn again when they are there.
fn transients(ctx: &egui::Context, file: &str, peaks: &waveform::Peaks) -> std::sync::Arc<Vec<f64>> {
    match file_transients(ctx, file, peaks) {
        Transients::Ready(seconds) => seconds,
        Transients::Waiting | Transients::Unavailable => std::sync::Arc::default(),
    }
}

/// Where the transients of a file stand in the window's background analysis.
#[derive(Debug, Clone, PartialEq)]
pub enum Transients {
    /// Found: the file seconds where notes start (`suhina_session::transient_seconds`).
    Ready(std::sync::Arc<Vec<f64>>),
    /// Being found on a thread of their own, or waiting for one; the window is drawn again when
    /// they are there.
    Waiting,
    /// The window holds no samples of the file to look in.
    Unavailable,
}

/// The files' transients found or being found, each under its file and the decoded samples it was
/// found in: two projects with a file of the same name each have their own samples, so their own
/// entry. The condition variable is told whenever a file's are found.
type TransientCache =
    std::sync::Arc<(std::sync::Mutex<std::collections::HashMap<(String, usize), TransientEntry>>, std::sync::Condvar)>;

/// One file's transients: the samples they are (being) found in, and the seconds once found.
#[derive(Debug, Clone)]
struct TransientEntry {
    data: std::sync::Weak<suhina_engine::AudioData>,
    seconds: Option<std::sync::Arc<Vec<f64>>>,
}

fn transient_cache(ctx: &egui::Context) -> TransientCache {
    let id = egui::Id::unique("audio-editor-transients");
    ctx.data_mut(|d| std::sync::Arc::clone(d.get_temp_mut_or_default::<TransientCache>(id)))
}

/// The transients of `file` (its samples in `peaks`), as [`Transients`] says: found once per file
/// and its samples on a thread of their own, at most [`TRANSIENT_JOBS`] at once; a file asked for
/// while that many are being found is started when it is next asked for. The window never waits
/// for them, and is drawn again when they are there.
pub fn file_transients(ctx: &egui::Context, file: &str, peaks: &waveform::Peaks) -> Transients {
    let Some(data) = peaks.data() else { return Transients::Unavailable };
    let cache = transient_cache(ctx);
    let (lock, found) = &*cache;
    let Ok(mut known) = lock.lock() else { return Transients::Unavailable };
    let key = (file.to_owned(), std::sync::Arc::as_ptr(data) as usize);
    // The same samples, not others that came to sit at the same address once these were freed.
    if let Some(entry) = known.get(&key).filter(|e| e.data.upgrade().is_some_and(|d| std::sync::Arc::ptr_eq(&d, data)))
    {
        return match &entry.seconds {
            Some(seconds) => Transients::Ready(std::sync::Arc::clone(seconds)),
            None => Transients::Waiting,
        };
    }
    // At most a couple found at once: browsing long clips quickly started a thread and kept an
    // audio buffer for each. A file over the limit is asked for again when it is next drawn.
    if known.values().filter(|e| e.seconds.is_none()).count() >= TRANSIENT_JOBS {
        return Transients::Waiting;
    }
    // A few files' worth, and none whose samples are gone: the editor shows one clip at a time.
    known.retain(|_, e| e.data.strong_count() > 0);
    if known.len() >= 32 {
        known.retain(|_, e| e.seconds.is_none());
    }
    let entry = TransientEntry { data: std::sync::Arc::downgrade(data), seconds: None };
    known.insert(key.clone(), entry.clone());
    drop(known);
    let (shared, job_key, samples, repaint) =
        (std::sync::Arc::clone(&cache), key.clone(), std::sync::Arc::clone(data), ctx.clone());
    let spawned = std::thread::Builder::new().name("suhina-transients".into()).spawn(move || {
        let seconds = std::sync::Arc::new(suhina_session::transient_seconds(&samples));
        let (lock, found) = &*shared;
        if let Ok(mut known) = lock.lock() {
            known.insert(job_key, TransientEntry { seconds: Some(seconds), ..entry });
        }
        found.notify_all();
        repaint.request_repaint();
    });
    // No thread to be had: no suggestions for the file, rather than a window that waits.
    if spawned.is_err() {
        if let Ok(mut known) = lock.lock()
            && let Some(entry) = known.get_mut(&key)
        {
            entry.seconds = Some(std::sync::Arc::default());
        }
        found.notify_all();
    }
    Transients::Waiting
}

/// Blocks until no file's transients are being found in `ctx`'s window, woken each time one's are
/// found: for tests and tools that must see the background analysis finished before they go on.
pub fn await_transients(ctx: &egui::Context) {
    let cache = transient_cache(ctx);
    let (lock, found) = &*cache;
    let Ok(mut known) = lock.lock() else { return };
    while known.values().any(|e| e.seconds.is_none()) {
        match found.wait(known) {
            Ok(next) => known = next,
            Err(_) => return,
        }
    }
}

/// Where a marker double-clicked on the marker lane at `x` goes: onto the transient suggested
/// nearest to it when one is within [`TRANSIENT_GRAB`] points, else on the grid's line nearest `x`
/// (`free`, Alt held: exactly at `x`, past any transient). Answers the content beat and whether it
/// is a transient's.
fn placed(
    x: f32,
    transients: &[f64],
    view: &AudioView,
    x_of: &impl Fn(f64) -> f32,
    beat_at_x: &impl Fn(f32) -> f64,
    step: f64,
    free: bool,
) -> (f64, bool) {
    if !free {
        let near = transients
            .iter()
            .map(|&seconds| beat_at(view, seconds))
            .filter(|beat| beat.is_finite())
            .map(|beat| (beat, (x_of(beat) - x).abs()))
            .filter(|(_, distance)| *distance <= TRANSIENT_GRAB)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((beat, _)) = near {
            return (beat, true);
        }
    }
    (snap(beat_at_x(x), step, free), false)
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
    let from = actions.len();
    show_one(ui, area, clip, state, env, actions);
    // Clips picked with it that are as long as it take its warp marker edits (› Several clips).
    let made: Vec<Action> = actions.drain(from..).collect();
    for action in made {
        let own = action.params.get("clip") == clip.address.get("clip")
            && action.params.get("track") == clip.address.get("track");
        if own {
            actions.extend(for_joined(clip, action));
        } else {
            actions.push(action);
        }
    }
}

fn show_one(
    ui: &mut Ui,
    area: Rect,
    clip: &AudioClip,
    state: &mut AudioEditorState,
    env: Env<'_>,
    actions: &mut Vec<Action>,
) {
    // A frame round the editor, inset from the detail area's right edge, so the file's end (and
    // its last marker) reads as the end of the editor, not as running on past the window.
    let frame = area;
    let area =
        Rect::from_min_max(area.min, Pos2::new((area.right() - FRAME_INSET).max(area.left() + 1.0), area.bottom()));
    let painter = ui.painter_at(frame);
    let panel = crate::detail::detail_fill("panel");
    painter.rect_filled(frame, 0.0, crate::detail::detail_fill("window"));
    painter.rect_filled(area, 0.0, panel);
    painter.rect_stroke(
        area.shrink(0.5),
        2.0,
        theme::hairline(hint_ink().gamma_multiply(0.5)),
        egui::StrokeKind::Inside,
    );
    let painter = ui.painter_at(area);
    let key = clip.address.to_string();
    let response = ui.interact(area, egui::Id::unique(("audio-editor", &key)), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, "Audio editor"));
    let Some(peaks) = env.waveforms.get(&clip.view.file).filter(|p| p.frames > 0 && p.sample_rate > 0) else {
        hint(&painter, area, "The audio is not loaded");
        return;
    };
    let duration = peaks.frames as f64 / f64::from(peaks.sample_rate);
    // Where the file lies in content beats, and the editor's axis: from content beat 0 (or the
    // start of what the clip plays, or the file's start, whichever is first) to the file's end (or
    // the end of what it plays). The axis is the clip's beats, as the ruler counts them: moving the
    // first warp marker moves the file along it and leaves room before it, rather than moving the
    // ruler under a file pinned to the left edge.
    // Read back from seconds through the markers that rise (`readable`): a project's markers that
    // stand still in the file, or fall, would put the whole file on one beat.
    let inverse = readable(&clip.view);
    let (file_first, file_last) = (beat_at(&inverse, 0.0), beat_at(&inverse, duration));
    state.file_span = (file_first.is_finite() && file_last > file_first).then_some((file_first, file_last));
    if !(file_first.is_finite() && file_last.is_finite() && file_last > file_first) {
        hint(&painter, area, "This audio has no length to show");
        return;
    }
    let (plays_from, plays_to) = played(&clip.view, clip.length);
    let first = file_first.min(0.0).min(if plays_from.is_finite() { plays_from } else { 0.0 });
    let last = file_last.max(if plays_to.is_finite() { plays_to } else { file_last });
    // The bar over the ruler: the file's name and format, and the Sample / Envelopes tabs.
    let header = Rect::from_min_size(area.min, Vec2::new(area.width(), HEADER_HEIGHT));
    // The Envelopes tab is the clip's Edit gain envelope, the same state as the button on the
    // clip in the Arrangement: switching one switches the other, and a clip's envelope is edited
    // in both places at once, its points shared.
    let editing = crate::arrangement::envelope::is_editing(ui.ctx(), clip.id);
    state.tab = if editing { Tab::Envelopes } else { Tab::Sample };
    let (tab, focus) = header_bar(ui, &painter, header, clip, peaks, state.tab);
    actions.extend(focus);
    if let Some(tab) = tab {
        let open = (tab == Tab::Envelopes).then_some(clip.id);
        crate::arrangement::envelope::set_editing(ui.ctx(), open);
        state.tab = tab;
    }
    let tabs_right = header.center().x + TABS_WIDTH / 2.0;
    let envelopes = state.tab == Tab::Envelopes;
    let area = Rect::from_min_max(Pos2::new(area.left(), header.bottom()), area.max);
    let ruler_rect = Rect::from_min_size(area.min, Vec2::new(area.width(), RULER_HEIGHT));
    let loop_row = Rect::from_min_size(Pos2::new(area.left(), ruler_rect.bottom()), Vec2::new(area.width(), LOOP_ROW));
    let lane = Rect::from_min_size(Pos2::new(area.left(), loop_row.bottom()), Vec2::new(area.width(), MARKER_LANE));
    let wave = Rect::from_min_max(Pos2::new(area.left(), lane.bottom()), area.max);
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
    state.show_clip(clip, last - first, area.width());
    // A press here gives the editor the keyboard; one anywhere else takes it away (the clip's panel
    // gives it back, beside it).
    if ui.input(|i| i.pointer.any_pressed()) {
        state.focused = ui.rect_contains_pointer(area);
    }
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
    }
    // The middle button held down over the editor drags the view along, as in the piano roll and
    // the other views.
    let (middle_pressed, middle_down, pointer_delta) = ui.input(|i| {
        (
            i.pointer.button_pressed(egui::PointerButton::Middle),
            i.pointer.button_down(egui::PointerButton::Middle),
            i.pointer.delta(),
        )
    });
    if middle_pressed {
        state.panning = ui.rect_contains_pointer(area);
    }
    if !middle_down {
        state.panning = false;
    }
    if state.panning {
        state.scroll -= pointer_delta.x;
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    // The view spans the file and no more: zoomed out it stops where the file fills the area, and
    // a file that became shorter in beats (a faster tempo, a marker moved in) fills it again.
    let fitted = fitted_width(last - first, area.width());
    if state.beat_width < fitted {
        let anchor = ui.input(|i| i.pointer.hover_pos()).map_or(area.width() / 2.0, |p| p.x - area.left());
        state.zoom(fitted / state.beat_width, anchor);
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
    let chosen = editor_step(state.grid, beat_width, clip.bar_beats);
    let step = TimelineGrid::new(chosen, beat_width, clip.bar_beats, GRID_LEAST).step;
    let free = ui.input(|i| i.modifiers.alt) || !state.grid.snap;
    let found = transients(ui.ctx(), &clip.view.file, peaks);
    // Zoomed out so far that the transients on show would be closer than
    // [`TRANSIENT_LEAST`] points on average, each is a thin line on the lane instead of a mark, and
    // they take no double-click: a marker put down there could not be told from the next.
    let on_show = found.iter().filter(|s| (area.left()..=area.right()).contains(&x_of(beat_at(&inverse, **s)))).count();
    let crowded = on_show > 1 && area.width() / (on_show as f32) < TRANSIENT_LEAST;
    let marks = std::sync::Arc::clone(&found);
    let found = if crowded { std::sync::Arc::new(Vec::new()) } else { found };
    // Where the editor's own gestures (painting the time selection, a click, a double-click) take
    // the pointer: the marker lane and the waveform; on the Envelopes tab the waveform is the
    // envelope's, and only the lane is.
    let takes = move |p: Pos2| lane.contains(p) || (!envelopes && wave.contains(p));

    // A released marker is drawn where it was let go until the project has it there.
    let markers = &clip.view.markers;
    if let Some((landing, frames)) = state.landing.as_mut() {
        let arrived = markers.iter().any(|m| (m.0 - landing.to).abs() < 1e-9);
        *frames = frames.saturating_sub(1);
        if arrived || *frames == 0 || state.drag.is_some() {
            state.landing = None;
        }
    }

    // The warp markers: a drag moves one (on the grid; Alt: free), a double-click takes it away.
    // A double-click on the waveform or the marker lane between them adds one: on the transient
    // near the pointer, else on the grid.
    let editable = markers.len() >= 2;
    let mut hot = None;
    // Where a marker was let go this frame: the project has it there from the next frame on.
    let mut moved_to = None;
    let adding = ui.input(|i| i.modifiers.command || i.modifiers.shift);
    if editable {
        for (i, &(beat, _)) in markers.iter().enumerate() {
            let dragged = state.drag.as_ref().is_some_and(|d| (d.from - beat).abs() < 1e-9);
            let shown = state.drag.as_ref().and_then(|d| d.moves(beat)).unwrap_or(beat);
            let x = x_of(shown);
            // Markers off screen are left out, but never the one being dragged: the pointer may
            // carry it past an edge, and its release must still move it.
            if !dragged && !(area.left() - GRAB..=area.right() + GRAB).contains(&x) {
                continue;
            }
            // A dragged marker past an edge keeps a zone at that edge, so egui goes on reporting
            // its drag and its release.
            let zone_x = if dragged { x.clamp(area.left(), area.right()) } else { x };
            let zone = Rect::from_min_max(
                Pos2::new(zone_x - GRAB, lane.top()),
                Pos2::new(zone_x + GRAB, if env.line_drag { area.bottom() } else { lane.bottom() }),
            )
            .intersect(area);
            let id = egui::Id::unique(("audio-editor-marker", &key, i));
            let marker =
                ui.interact(zone, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            let label = format!("Warp marker {}", i + 1);
            marker.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, &label));
            if marker.hovered() || marker.dragged() {
                hot = Some(i);
            }
            let is_picked = state.picked.iter().any(|b| (b - beat).abs() < 1e-9);
            // A click picks it alone; with Ctrl or Shift held it joins the picked markers, or
            // leaves them when it is one.
            if marker.clicked() {
                if adding {
                    if is_picked {
                        state.picked.retain(|b| (b - beat).abs() >= 1e-9);
                    } else {
                        state.picked.push(beat);
                        state.picked.sort_by(f64::total_cmp);
                    }
                } else {
                    state.picked = vec![beat];
                }
            }
            // Where the button went down, not where the drag was recognised: the marker keeps its
            // place under the pointer. A picked marker carries the other picked ones with it (one
            // not picked joins them with Ctrl or Shift held, else it goes alone).
            if marker.drag_started_by(egui::PointerButton::Primary)
                && let Some(p) = ui.input(|i| i.pointer.press_origin()).or(marker.interact_pointer_pos())
            {
                if !is_picked {
                    if adding {
                        state.picked.push(beat);
                        state.picked.sort_by(f64::total_cmp);
                    } else {
                        state.picked = vec![beat];
                    }
                }
                let others = state.picked.iter().copied().filter(|b| (b - beat).abs() >= 1e-9).collect();
                state.drag = Some(MarkerDrag { from: beat, grab: beat_at_x(p.x) - beat, to: beat, others });
            }
            if marker.dragged_by(egui::PointerButton::Primary)
                && let Some(p) = marker.interact_pointer_pos()
                && let Some(drag) = state.drag.as_mut().filter(|d| (d.from - beat).abs() < 1e-9)
            {
                // Between the neighbors not moved, which none of them passes.
                let (low, high) = group_bounds(markers, &drag.moved());
                let wanted = snap(beat_at_x(p.x) - drag.grab, step, free);
                drag.to = drag.from + (wanted - drag.from).clamp(low, high);
            }
            // Only the drag this marker started: one kept from another clip or project is dropped.
            // The release frame's own move counts: the pointer can go on moving as the button comes
            // up, and the drag above saw it only while the button was down.
            if marker.drag_stopped_by(egui::PointerButton::Primary)
                && let Some(drag) = state.drag.as_mut().filter(|_| dragged)
                && let Some(p) = marker.interact_pointer_pos()
            {
                let (low, high) = group_bounds(markers, &drag.moved());
                let wanted = snap(beat_at_x(p.x) - drag.grab, step, free);
                drag.to = drag.from + (wanted - drag.from).clamp(low, high);
            }
            if marker.drag_stopped_by(egui::PointerButton::Primary)
                && let Some(drag) = state.drag.take()
                && dragged
                && (drag.to - drag.from).abs() > 1e-9
            {
                let by = drag.to - drag.from;
                if drag.others.is_empty() {
                    let own = json!({ "from": drag.from, "beat": drag.to });
                    actions.push(Action::new("clip.moveWarpMarker", with_address(&clip.address, own)));
                } else {
                    let own = json!({ "beats": drag.moved(), "by": by });
                    actions.push(Action::new("clip.moveWarpMarkers", with_address(&clip.address, own)));
                }
                state.picked = drag.moved().iter().map(|b| b + by).collect();
                moved_to = Some(drag.to);
                state.landing = Some((drag, LANDING_FRAMES));
            }
            if marker.secondary_clicked() {
                state.menu_at = Some(MenuAt { beat, marker_beat: beat, on_marker: Some(beat) });
            }
            // Its right-click menu, opened on it (with Remove warp marker).
            if let Some(at) = state.menu_at.filter(|m| m.on_marker.is_some_and(|b| (b - beat).abs() < 1e-9)) {
                let cx = MenuCx { clip, at, step, fit: (last - first, area.width()), keymap: env.keymap };
                let opened = egui::Popup::context_menu(&marker)
                    .layout(egui::Layout::top_down_justified(egui::Align::Min))
                    .style(egui::containers::menu::menu_style)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| menu_items(ui, &cx, state, actions));
                if opened.is_none() && !marker.secondary_clicked() {
                    state.menu_at = None;
                }
            }
            if marker.double_clicked() && markers.len() > 2 {
                state.drag = None;
                state.picked.clear();
                actions
                    .push(Action::new("clip.removeWarpMarker", with_address(&clip.address, json!({ "beat": beat }))));
            }
        }
        if response.double_clicked()
            && hot.is_none()
            && let Some(p) = response.interact_pointer_pos().filter(|p| takes(*p))
        {
            // On the marker lane, onto the transient suggested there, else on the grid; on the
            // waveform, on the grid (Alt: exactly where the double-click is).
            let beat = if lane.contains(p) {
                placed(p.x, &found, &inverse, &x_of, &beat_at_x, step, free).0
            } else {
                snap(beat_at_x(p.x), step, free)
            };
            actions.push(Action::new("clip.addWarpMarker", with_address(&clip.address, json!({ "beat": beat }))));
        }
    }
    // The right-click menu: where it opened is kept while it is open.
    if response.secondary_clicked()
        && let Some(p) = response.interact_pointer_pos()
    {
        let beat = snap(beat_at_x(p.x), step, free);
        let marker_beat = if lane.contains(p) {
            placed(p.x, &found, &inverse, &x_of, &beat_at_x, step, free).0
        } else {
            snap(beat_at_x(p.x), step, free)
        };
        state.menu_at = Some(MenuAt { beat, marker_beat, on_marker: None });
    }
    if let Some(at) = state.menu_at.filter(|m| m.on_marker.is_none()) {
        let cx = MenuCx { clip, at, step, fit: (last - first, area.width()), keymap: env.keymap };
        let opened = egui::Popup::context_menu(&response)
            .layout(egui::Layout::top_down_justified(egui::Align::Min))
            .style(egui::containers::menu::menu_style)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| menu_items(ui, &cx, state, actions));
        if opened.is_none() && !response.secondary_clicked() {
            state.menu_at = None;
        }
    }
    // A click on the waveform away from the markers puts the picked one down; one that is gone
    // (removed, or the project changed under it) is no longer picked.
    // A drag on the waveform or the marker lane paints the time selection, on the grid (Alt:
    // free); the wheel and the ruler scroll.
    if response.drag_started_by(egui::PointerButton::Primary)
        && hot.is_none()
        && let Some(p) = ui.input(|i| i.pointer.press_origin()).filter(|p| takes(*p))
    {
        state.painting = Some(snap(beat_at_x(p.x), step, free));
    }
    if response.dragged_by(egui::PointerButton::Primary)
        && let (Some(from), Some(p)) = (state.painting, response.interact_pointer_pos())
    {
        let to = snap(beat_at_x(p.x), step, free);
        state.span = (to != from).then(|| (from.min(to), from.max(to)));
        // The markers in the time selection are picked with it.
        state.picked = markers_in(markers, state.span);
    }
    if state.painting.is_some() && !ui.input(|i| i.pointer.primary_down()) {
        state.painting = None;
    }
    // Shift and a click carries the time selection to the clicked point.
    let (shift, now) = ui.input(|i| (i.modifiers.shift, i.time));
    if response.clicked()
        && hot.is_none()
        && shift
        && let Some(p) = response.interact_pointer_pos().filter(|p| takes(*p))
    {
        let at = snap(beat_at_x(p.x), step, free);
        state.span = match state.span {
            Some((start, end)) if at < start => Some((at, end)),
            Some((start, _)) => (at > start).then_some((start, at)),
            None => None,
        };
        state.picked = markers_in(markers, state.span);
    }
    if response.clicked() && hot.is_none() && !shift {
        state.picked.clear();
        state.span = None;
        // An arrangement clip's waveform moves the play start marker, on the grid (Alt: free), as
        // a click in a clip's body in the Arrangement does: the next start begins there, and
        // while playing, playback goes on where it is (#657). A slot's clip plays from its launch
        // and has no such place.
        // It waits out the double-click time: a double-click adds a warp marker instead.
        let start = response
            .interact_pointer_pos()
            .filter(|p| !clip.endless && takes(*p))
            .and_then(|p| clip.arrangement_beat(snap(beat_at_x(p.x), step, free)))
            .map(|at| Action::new("transport.setStart", json!({ "beats": round_beat(at) })));
        state.waiting.click(now, presses(&response), start, actions);
    } else if response.double_clicked() || response.triple_clicked() {
        // The second press of a double-click on a marker or with Shift held: no seek either.
        state.waiting.click(now, 2, None, actions);
    }
    // A click on the ruler is the deliberate seek, as on the Arrangement's ruler: playback jumps
    // to where it plays in the arrangement, on the grid (Alt: free), while playing at the next
    // launch boundary (`transport.jump`). A slot's clip plays from its launch and has no such
    // place. It waits out the double-click time: a double-click fits the file instead.
    if ruler_response.clicked() {
        let jump = ruler_response
            .interact_pointer_pos()
            .filter(|_| !clip.endless)
            .and_then(|p| clip.arrangement_beat(snap(beat_at_x(p.x), step, free)))
            .map(|at| Action::new("transport.jump", json!({ "beats": round_beat(at) })));
        state.waiting.click(now, presses(&ruler_response), jump, actions);
    }
    // A click left alone for the double-click time does what it waited to do; the window is
    // drawn again until then.
    let delay = ui.ctx().options(|o| o.input_options.max_double_click_delay);
    if state.waiting.tick(now, delay, actions) {
        ui.ctx().request_repaint();
    }
    // Picked markers that are gone (removed, or the project changed under them) are no longer
    // picked; not while moved ones are on their way to where they were let go.
    if state.drag.is_none() && state.landing.is_none() && moved_to.is_none() {
        state.picked.retain(|b| markers.iter().any(|m| (m.0 - b).abs() < 1e-9));
    }
    state.hovered = hot.and_then(|i| markers.get(i)).map(|m| m.0);
    // The button is up and no marker took the release (its marker is gone, or Warp went off):
    // the drag ends without a move, and the waveform shows the project's markers again.
    if state.drag.is_some() && !ui.input(|i| i.pointer.primary_down()) {
        state.drag = None;
    }

    // The waveform with the marker being dragged (or just let go) where it is now, so the sound
    // moves under it.
    let moving = state.drag.clone().or_else(|| state.landing.as_ref().map(|(drag, _)| drag.clone()));
    let dragged_view = moving.as_ref().map(|drag| {
        let mut moved = clip.view.clone();
        for marker in moved.markers.iter_mut() {
            if let Some(to) = drag.moves(marker.0) {
                marker.0 = to;
            }
        }
        moved
    });
    let view = dragged_view.as_ref().unwrap_or(&clip.view);
    let count = (wave.width().ceil() as usize).min(MAX_COLUMNS);
    let per_point = f64::from(1.0 / beat_width);
    // Drawn through the clip's gain envelope (a point being dragged included), as the Arrangement
    // draws a clip, so the waveform shows what is heard.
    let envelope = crate::arrangement::envelope::points_now(ui.ctx(), clip.id, &clip.view.envelope);
    let columns: Vec<waveform::Column> = (0..count)
        .map(|i| {
            let x = wave.left() + i as f32;
            let beat = beat_at_x(x);
            let (a, b) = (seconds_at(view, beat), seconds_at(view, beat + per_point));
            let gain = if envelope.is_empty() {
                1.0
            } else {
                suhina_engine::model::db_to_gain(suhina_engine::model::envelope_db_at(&envelope, beat)) as f32
            };
            waveform::Column { x: x + 0.5, from: a.min(b), to: a.max(b), gain }
        })
        .collect();
    let shape = waveform::geometry(peaks, wave.shrink2(Vec2::new(0.0, 2.0)), &columns, waveform::scale(ui.ctx()));
    // The grid down the waveform where the file is (past its ends there is nothing to put on a
    // beat): under it in a light ink, and over it as a faint shade, so it reads across a loud, full
    // waveform too, as over a clip's envelope in the Arrangement.
    let visible = (beat_at_x(area.left()), beat_at_x(area.right()));
    // Across the axis: before the file's start too (room a moved first marker leaves), not past
    // the axis's end.
    let over_file = (visible.0.max(first), visible.1.min(last));
    let lines = over_file.1 > over_file.0;
    if lines {
        grid_lines(&painter, wave, clip.bar_beats, &x_of, over_file, step, panel);
    }
    let ink = waveform_ink(env.theme_waveform, clip.color, panel);
    waveform::paint(&painter.with_clip_rect(wave), &shape, waveform::Colors::from(ink));

    // The time selection: tinted over the waveform, its edges drawn, and a bar along the ruler's
    // lower edge, as in the piano roll.
    if let Some((start, end)) = state.span {
        let accent = crate::kit::element::PIANO_TIME_SELECTION.look().accent();
        let (a, b) = (x_of(start), x_of(end));
        let (left, right) = (a.max(wave.left()), b.min(wave.right()));
        if right > left {
            painter.rect_filled(Rect::from_x_y_ranges(left..=right, wave.y_range()), 0.0, accent.gamma_multiply(0.3));
            painter.rect_filled(
                Rect::from_x_y_ranges(left..=right, ruler_rect.bottom() - 3.0..=ruler_rect.bottom()),
                0.0,
                accent,
            );
        }
        for x in [a, b].into_iter().filter(|x| (wave.left()..=wave.right()).contains(x)) {
            painter.vline(x, egui::Rangef::new(lane.top(), wave.bottom()), Stroke::new(1.5, accent));
        }
        let label = format!("Time selection from beat {start:.3} to {end:.3}");
        ui.interact(
            Rect::from_x_y_ranges(left.min(right)..=right.max(left + 1.0), wave.y_range()),
            egui::Id::unique(("audio-editor-span", &key)),
            Sense::hover(),
        )
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, &label));
    }

    // The Envelopes tab: the clip's gain envelope on the waveform, edited as on the clip in the
    // Arrangement (a click on the line adds a point, a drag moves one, a double-click puts it at
    // 0 dB, Alt and a click removes it; each `clip.setEnvelope`). Its points are content beats, so
    // the editor's axis is the envelope's: counted from the file's first beat, at the editor's
    // zoom, snapped to the editor's grid.
    if envelopes {
        let along = AudioView { offset: first, loop_range: None, rate: 1.0, ..clip.view.clone() };
        let place = crate::arrangement::envelope::Place {
            rect: wave,
            origin: x_of(first),
            beat_width,
            ink: theme::text_on(panel.to_opaque()),
            site: "audio-editor",
        };
        let on_grid = move |elapsed: f64| snap(first + elapsed, step, false) - first;
        let name = clip.view.file.rsplit(['/', '\\']).next().unwrap_or("clip").to_owned();
        crate::arrangement::envelope::show(
            ui,
            &painter.with_clip_rect(wave),
            place,
            clip.id,
            &name,
            &along,
            true,
            &on_grid,
            actions,
        );
    }

    // The loop row: the clip's loop as a bar with an arrow at each end, in the clip's color; with
    // the loop off, gray over the part the clip plays. A click switches the loop on or off; a drag
    // of an end moves that end, of the bar the whole loop, on the grid (Alt: free); each is one
    // `clip.setLoop`.
    let loop_on = clip.view.loop_range;
    let bar_now = loop_on.unwrap_or_else(|| played(&clip.view, clip.length));
    let loop_response = ui.interact(loop_row, egui::Id::unique(("audio-editor-loop", &key)), Sense::click_and_drag());
    loop_response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, true, if loop_on.is_some() { "Loop on" } else { "Loop off" })
    });
    let offset = clip.view.offset;
    let part_at = |p: Pos2| {
        let (a, b) = (x_of(bar_now.0), x_of(bar_now.1));
        let x = p.x;
        // The start marker's triangle, in the row's lower half, before the bar's ends.
        if p.y > loop_row.center().y && (x - x_of(offset)).abs() <= LOOP_EDGE {
            Some(LoopPart::Marker)
        } else {
            use crate::arrangement::arrangement_view::{LoopBarPart, loop_bar_part};
            match loop_bar_part(x, a, b)? {
                LoopBarPart::Start => Some(LoopPart::Start),
                LoopBarPart::End => Some(LoopPart::End),
                LoopBarPart::Body => Some(LoopPart::Whole),
            }
        }
    };
    if let Some(p) = loop_response.hover_pos() {
        let icon = match part_at(p) {
            Some(LoopPart::Start | LoopPart::End | LoopPart::Marker) => egui::CursorIcon::ResizeHorizontal,
            // The four-way move arrow over the middle, which a drag moves.
            Some(LoopPart::Whole) => egui::CursorIcon::Move,
            None => egui::CursorIcon::Default,
        };
        ui.ctx().set_cursor_icon(icon);
    }
    if loop_response.drag_started_by(egui::PointerButton::Primary)
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
        && let Some(part) = part_at(p).filter(|part| *part == LoopPart::Marker || loop_on.is_some())
    {
        state.loop_drag = Some((part, beat_at_x(p.x), bar_now));
    }
    // Where the start marker goes as the loop moves: with the loop's start when it is there (as
    // Ctrl+L leaves it), along with a moved loop when it is inside it; one before the loop (a
    // lead-in into it) stays.
    let marker_for = |from: (f64, f64), to: (f64, f64)| {
        let at_start = (offset - from.0).abs() < 1e-6;
        let inside = offset >= from.0 - 1e-6 && offset < from.1;
        let whole = ((to.1 - to.0) - (from.1 - from.0)).abs() < 1e-9;
        if at_start {
            to.0
        } else if inside && whole {
            offset + (to.0 - from.0)
        } else {
            offset
        }
    };
    // The start marker dragged: on the grid (Alt: free), from 0 to a 64th of a beat before the
    // loop's end (or the end of what the clip plays).
    let marker_dragged = |by: f64| {
        let limit = loop_on.map_or(played(&clip.view, clip.length).1, |(_, end)| end) - 1.0 / 64.0;
        snap(offset + by, step, free).clamp(0.0, limit.max(0.0))
    };
    let dragging = state.loop_drag.zip(loop_response.interact_pointer_pos());
    let loop_shown = match dragging {
        Some(((part, grab, from), p)) if part != LoopPart::Marker => {
            Some(dragged_loop(from, part, beat_at_x(p.x) - grab, step, free))
        }
        _ => None,
    };
    let offset_shown = match dragging {
        Some(((LoopPart::Marker, grab, _), p)) => marker_dragged(beat_at_x(p.x) - grab),
        Some(((_, _, from), _)) => loop_shown.map_or(offset, |to| marker_for(from, to)),
        None => offset,
    };
    if loop_response.drag_stopped_by(egui::PointerButton::Primary)
        && let Some((part, grab, from)) = state.loop_drag.take()
        && let Some(p) = loop_response.interact_pointer_pos()
    {
        if part == LoopPart::Marker {
            let to = marker_dragged(beat_at_x(p.x) - grab);
            if (to - offset).abs() > 1e-9 {
                actions.push(Action::new(
                    "clip.setStart",
                    with_address(&clip.address, json!({ "offset": round_beat(to) })),
                ));
            }
        } else {
            let (start, end) = dragged_loop(from, part, beat_at_x(p.x) - grab, step, free);
            state.span = Some((start, end));
            if (start - from.0).abs() > 1e-9 || (end - from.1).abs() > 1e-9 {
                let own = json!({ "start": round_beat(start), "end": round_beat(end) });
                actions.push(Action::new("clip.setLoop", with_address(&clip.address, own)));
                // The start marker with it, where it was at the loop's start or inside the loop:
                // left behind, the clip would play from the old place up to the moved loop first.
                let marker = marker_for(from, (start, end));
                if (marker - offset).abs() > 1e-9 {
                    let own = json!({ "offset": round_beat(marker) });
                    actions.push(Action::new("clip.setStart", with_address(&clip.address, own)));
                }
            }
        }
    }
    if state.loop_drag.is_some() && !ui.input(|i| i.pointer.primary_down()) {
        state.loop_drag = None;
    }
    // A click on the bar selects it: the time selection becomes the loop, the bar lit, ready to be
    // dragged. It never switches the loop (the clip panel's Loop button does); one beside it
    // does nothing.
    if loop_response.clicked()
        && loop_response.interact_pointer_pos().and_then(part_at).is_some_and(|part| part != LoopPart::Marker)
    {
        state.span = Some(bar_now);
        state.picked.clear();
    }
    let bar = loop_shown.unwrap_or(bar_now);
    painter.rect_filled(loop_row, 0.0, crate::detail::detail_fill("window"));
    let (a, b) = (x_of(bar.0), x_of(bar.1));
    let (left, right) = (a.max(loop_row.left()), b.min(loop_row.right()));
    if right > left {
        // Lit while it is the time selection (clicked, or a drag of it under way), as a selected
        // loop brace is.
        let selected = state.loop_drag.is_some()
            || state.span.is_some_and(|(s, e)| (s - bar.0).abs() < 1e-9 && (e - bar.1).abs() < 1e-9);
        let bar_ink = match (loop_on.is_some(), selected) {
            (_, true) => crate::kit::element::PIANO_TIME_SELECTION.look().accent(),
            (true, false) => clip.color,
            (false, false) => hint_ink().gamma_multiply(0.6),
        };
        let body = Rect::from_x_y_ranges(left..=right, loop_row.top() + 2.0..=loop_row.bottom() - 2.0);
        painter.rect_filled(body, 1.0, bar_ink);
        // An arrow pointing in at each end, as a loop brace has.
        let mid = loop_row.center().y;
        for (x, dir) in [(a, 1.0f32), (b, -1.0)] {
            if (loop_row.left()..=loop_row.right()).contains(&x) {
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        Pos2::new(x, loop_row.top() + 1.0),
                        Pos2::new(x + dir * 5.0, mid),
                        Pos2::new(x, loop_row.bottom() - 1.0),
                    ],
                    panel,
                    Stroke::NONE,
                ));
            }
        }
    }

    // The clip's start marker: a triangle pointing right in the row's lower half, where playback
    // of the clip begins (a drag moves it, `clip.setStart`).
    let marker_x = x_of(offset_shown);
    if (loop_row.left()..=loop_row.right()).contains(&marker_x) {
        let top = loop_row.center().y;
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(marker_x, top),
                Pos2::new(marker_x + 6.0, (top + loop_row.bottom()) / 2.0),
                Pos2::new(marker_x, loop_row.bottom()),
            ],
            caption_ink(),
            Stroke::new(1.0, panel),
        ));
        let zone = Rect::from_x_y_ranges(marker_x - 3.0..=marker_x + 6.0, top..=loop_row.bottom());
        let label = format!("Clip start at beat {offset_shown:.3}");
        ui.interact(zone, egui::Id::unique(("audio-editor-clip-start", &key)), Sense::hover())
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, &label));
    }

    // The part the clip does not play is dimmed: from its start marker, or the loop's start when
    // that is earlier, to the loop's end (or where its length reaches), as a drag would leave them.
    // (A clip that does not loop keeps its end marker as its start marker moves.)
    let (from, to) = match loop_shown.or(loop_on) {
        Some((start, end)) => (offset_shown.min(start), end),
        None => (offset_shown, played(&clip.view, clip.length).1),
    };
    let dim = panel.gamma_multiply(0.62);
    let (left, right) = (x_of(from).clamp(wave.left(), wave.right()), x_of(to).clamp(wave.left(), wave.right()));
    for (a, b) in [(wave.left(), left), (right, wave.right())] {
        if b > a {
            painter.rect_filled(Rect::from_x_y_ranges(a..=b, wave.y_range()), 0.0, dim);
        }
    }

    // The loop's edges, down the marker lane and the waveform (where a drag of the bar has them).
    let accent = clip.color;
    if let Some((start, end)) = loop_shown.or(clip.view.loop_range) {
        for beat in [start, end] {
            let x = x_of(beat);
            if (wave.left()..=wave.right()).contains(&x) {
                painter.line_segment([Pos2::new(x, lane.top()), Pos2::new(x, wave.bottom())], Stroke::new(1.0, accent));
            }
        }
    }

    // The marker lane: the transients as small marks where a marker could go (those without one),
    // then the warp markers' flags, each with its line down the waveform (the one the pointer is
    // on, picked or dragged, brighter).
    painter.rect_filled(lane, 0.0, crate::detail::detail_fill("window").gamma_multiply(0.85));
    painter.line_segment([lane.left_bottom(), lane.right_bottom()], theme::hairline(hint_ink().gamma_multiply(0.4)));
    let pointer = ui.input(|i| i.pointer.hover_pos()).filter(|p| area.contains(*p) && state.drag.is_none());
    let inverse_now = readable(view);
    // The transient suggestions answer the pointer on the marker lane only: over the waveform
    // nothing on the lane moves as the pointer does.
    let target = pointer
        .filter(|p| lane.contains(*p) && hot.is_none() && editable)
        .map(|p| placed(p.x, &found, &inverse_now, &x_of, &beat_at_x, step, free));
    if editable {
        for &seconds in marks.iter() {
            let beat = beat_at(&inverse_now, seconds);
            let x = x_of(beat);
            if !(lane.left()..=lane.right()).contains(&x) || view.markers.iter().any(|m| (x_of(m.0) - x).abs() < 3.0) {
                continue;
            }
            if crowded {
                let line = Stroke::new(1.0, hint_ink().gamma_multiply(0.6));
                painter.line_segment([Pos2::new(x, lane.bottom() - 6.0), Pos2::new(x, lane.bottom() - 1.0)], line);
                continue;
            }
            let lit = target.is_some_and(|(b, on)| on && (b - beat).abs() < 1e-9);
            let mark = if lit { title_ink() } else { hint_ink().gamma_multiply(0.8) };
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(x - 3.5, lane.bottom() - 7.0),
                    Pos2::new(x + 3.5, lane.bottom() - 7.0),
                    Pos2::new(x, lane.bottom() - 2.0),
                ],
                mark,
                Stroke::NONE,
            ));
            // The transient a double-click would pin a marker on: a ghost of a marker's flag over
            // its mark, with a faint line down the waveform. The pointer's own line stays with the
            // pointer, so nothing jumps from one transient to the next as the pointer moves.
            if lit {
                let top = lane.top() + 2.0;
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        Pos2::new(x - 4.5, top),
                        Pos2::new(x + 4.5, top),
                        Pos2::new(x + 4.5, lane.bottom() - 4.0),
                        Pos2::new(x, lane.bottom()),
                        Pos2::new(x - 4.5, lane.bottom() - 4.0),
                    ],
                    Color32::TRANSPARENT,
                    Stroke::new(1.0, title_ink().gamma_multiply(0.8)),
                ));
            }
        }
    }
    // Two or more markers picked: a band on the marker lane from the first to the last of them,
    // with a bar along its top joining their flags, so it reads that they go together (where a
    // drag carries them).
    let picked_now = picked_now(&state.picked, state.drag.as_ref());
    if let (true, Some(first), Some(last)) =
        (picked_now.len() > 1, picked_now.iter().copied().reduce(f64::min), picked_now.iter().copied().reduce(f64::max))
    {
        let (left, right) = (x_of(first).max(lane.left()), x_of(last).min(lane.right()));
        if right > left {
            let ink = title_ink();
            painter.rect_filled(Rect::from_x_y_ranges(left..=right, lane.y_range()), 0.0, ink.gamma_multiply(0.12));
            painter.rect_filled(Rect::from_x_y_ranges(left..=right, lane.top()..=lane.top() + 2.0), 0.0, ink);
        }
    }
    for (i, &(beat, _)) in view.markers.iter().enumerate() {
        let x = x_of(beat);
        if !(area.left() - 6.0..=area.right() + 6.0).contains(&x) {
            continue;
        }
        let lit = hot == Some(i)
            || state.picked.iter().any(|b| (b - beat).abs() < 1e-9)
            || moving.as_ref().is_some_and(|d| d.lands_on(beat));
        let color = if lit { title_ink() } else { accent };
        // Its line down the waveform, to put its sound on a beat by eye; it takes the pointer only
        // when Settings say so (`Env::line_drag`), so a drag beside it paints the time selection.
        let width = if lit { 2.0 } else { 1.0 };
        painter.line_segment([Pos2::new(x, lane.bottom()), Pos2::new(x, wave.bottom())], Stroke::new(width, color));
        // The flag: a small block on the lane with a point down to the line.
        let top = lane.top() + 2.0;
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(x - 4.5, top),
                Pos2::new(x + 4.5, top),
                Pos2::new(x + 4.5, lane.bottom() - 4.0),
                Pos2::new(x, lane.bottom()),
                Pos2::new(x - 4.5, lane.bottom() - 4.0),
            ],
            color,
            Stroke::new(1.0, panel),
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

    // Where playback starts next (a click on the waveform moves it), then the playhead while the
    // transport runs: each a line down the waveform with its mark on the marker lane, as in the
    // piano roll, in the clip's content through its start, rate and loop.
    let starts = env.start_marker.and_then(|at| clip.content_beat(at));
    let plays = env.playhead.and_then(|at| clip.content_beat(at));
    for (beat, playing) in [(starts, false), (plays, true)] {
        let Some(beat) = beat else { continue };
        let x = x_of(beat).round() + 0.5;
        if !(wave.left()..=wave.right()).contains(&x) {
            continue;
        }
        let ink = if playing {
            crate::kit::element::PLAYHEAD.attribute("kind", "piano").look().color()
        } else {
            crate::kit::element::START_MARKER.attribute("kind", "piano").look().color()
        };
        let line = if playing { ink } else { ink.gamma_multiply(0.6) };
        painter.vline(x, egui::Rangef::new(lane.top(), wave.bottom()), Stroke::new(1.0, line));
        painter.add(egui::Shape::convex_polygon(
            vec![Pos2::new(x - 4.0, lane.top()), Pos2::new(x + 4.0, lane.top()), Pos2::new(x, lane.top() + 6.0)],
            ink,
            Stroke::NONE,
        ));
        let zone = Rect::from_center_size(Pos2::new(x, wave.center().y), Vec2::new(3.0, wave.height()));
        let (name, label) = if playing {
            ("audio-editor-playhead", format!("Playhead at beat {beat:.3}"))
        } else {
            ("audio-editor-start-marker", format!("Play start at beat {beat:.3}"))
        };
        ui.interact(zone, egui::Id::unique((name, &key)), Sense::hover())
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, &label));
    }

    // The ruler last, with where the pointer is: its line and position follow it exactly, as the
    // transient a double-click would take shows on the marker lane.
    ruler(&painter, ruler_rect, clip.bar_beats, &x_of, visible, step);
    // The editor's tools at the header's right end, right of its tabs: the grid (its value now,
    // as an adaptive grid changes it with the zoom), Snap, Fit, Reset warp and Transients.
    let tools =
        header_tools(ui, &painter, header, tabs_right, state.grid, step_label(step, clip.bar_beats), env.keymap);
    match tools {
        Some(Tool::Fit) => state.fit(last - first, area.width()),
        Some(Tool::Reset) => state.reset_open = true,
        Some(Tool::Transients) => {
            actions.push(Action::new("clip.addWarpMarkersAtTransients", with_address(&clip.address, json!({}))))
        }
        Some(Tool::Snap) => state.grid = state.grid.toggle_snap(),
        Some(Tool::Grid(grid)) => state.grid = grid,
        None => {}
    }
    let readout = pointer.filter(|_| hot.is_none()).map(|p| (p.x, beat_at_x(p.x)));
    if let Some((x, beat)) = readout {
        let line = Stroke::new(1.0, hint_ink().gamma_multiply(0.7));
        painter.line_segment([Pos2::new(x, ruler_rect.bottom()), Pos2::new(x, wave.bottom())], line);
        let text = position_label(beat, clip.bar_beats).unwrap_or_default();
        let chip = Rect::from_center_size(Pos2::new(x, ruler_rect.top() + 9.0), Vec2::new(58.0, 15.0));
        painter.rect_filled(chip, 3.0, crate::detail::detail_fill("control"));
        painter.text(chip.center(), Align2::CENTER_CENTER, text, theme::mono_font(theme::TEXT_TINY + 1.0), title_ink());
    }
    if let Some(drag) = &state.drag {
        let text = position_label(drag.to, clip.bar_beats).unwrap_or_default();
        let chip = Rect::from_center_size(Pos2::new(x_of(drag.to), ruler_rect.top() + 9.0), Vec2::new(58.0, 15.0));
        painter.rect_filled(chip, 3.0, crate::detail::detail_fill("control"));
        painter.text(chip.center(), Align2::CENTER_CENTER, text, theme::mono_font(theme::TEXT_TINY + 1.0), title_ink());
    }

    // A marker the pointer is on, or one being dragged, shows the two-way arrow, set last so no
    // widget drawn over the marker lane (the time selection, the playhead's mark) takes it back.
    if hot.is_some() || state.drag.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
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

/// What the reset dialog's Close button and Escape choose: the markers stay.
impl crate::app::project_dialog::CloseChoice for Reset {
    fn close() -> Self {
        Reset::Keep
    }
}

/// The dialog that asks whether to keep the clip's warp markers or put the audio back on an even
/// grid, a `dialog[name="reset-warp"]` over the window as the other dialogs are, its question
/// wrapped to its width and its three choices along its bottom (`dialog::actions`); `None` while
/// it is up and nothing is chosen. Escape and its Close button keep them.
fn reset_dialog(ctx: &egui::Context, markers: usize) -> Option<Reset> {
    use crate::kit::dialog;
    use crate::kit::widgets;
    const WIDTH: f32 = 404.0;
    let area = ctx.content_rect();
    let question = format!(
        "This clip has {markers} warp markers. Keep them, or reset the warp so the audio runs evenly \
         again, with one marker at each end? The clip's start, length and loop stay where they are."
    );
    let style_class = dialog::enter("reset-warp");
    crate::kit::floating::area(ctx, egui::Id::unique("audio-editor-reset"), crate::kit::floating::Kind::Own)
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .show(ctx, |ui| {
            let mut styled = style_class.ui(ui);
            let ui = &mut *styled;
            ui.set_min_size(area.size());
            let galley = ui.painter().layout(question.clone(), theme::ui_font(theme::TEXT_SMALL), caption_ink(), WIDTH);
            let text_height = galley.size().y;
            let mut closed = None;
            let content = crate::app::project_dialog::panel(
                ui,
                &style_class,
                area,
                Vec2::new(WIDTH, text_height + theme::PAD * 2.0 + dialog::BUTTON_HEIGHT),
                "Reset warp markers",
                &mut closed,
            );
            let painter = ui.painter_at(area);
            let text = Rect::from_min_size(content.min, Vec2::new(content.width(), text_height));
            ui.interact(text, egui::Id::unique("audio-editor-reset-question"), Sense::hover())
                .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &question));
            widgets::paint_text(&painter, text.min, galley, caption_ink());
            // Each choice as wide as its words, named as it reads.
            let choices = [
                ("Keep warp markers", Reset::Keep),
                ("Reset to clip tempo", Reset::Even),
                ("Find tempo again", Reset::Detect),
            ];
            let buttons: Vec<dialog::Action<'_>> = choices
                .iter()
                .map(|(text, _)| {
                    let width = widgets::text_width(ui, text, theme::ui_font(theme::TEXT)) + 20.0;
                    dialog::Action::new(text, text).wide(width.max(dialog::BUTTON_WIDTH))
                })
                .collect();
            let clicked = dialog::actions(ui, &painter, content, &buttons);
            if let Some(&(_, choice)) = clicked.and_then(|index| choices.get(index)) {
                return Some(choice);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                return Some(Reset::Keep);
            }
            closed
        })
        .inner
}

/// The lines of the grid down the waveform, under it: a bar's, a beat's and, closer in, the
/// grid's own, in an ink that reads on `panel`. Nothing is drawn over the waveform, where a drag
/// paints the time selection.
fn grid_lines(
    painter: &egui::Painter,
    wave: Rect,
    bar_beats: f64,
    x_of: &impl Fn(f64) -> f32,
    visible: (f64, f64),
    step: f64,
    panel: Color32,
) {
    let ink = theme::text_on(panel.to_opaque());
    let (bar_ink, beat_ink, fine_ink) = (ink.gamma_multiply(0.34), ink.gamma_multiply(0.18), ink.gamma_multiply(0.09));
    let (from, to) = (visible.0.min(visible.1), visible.0.max(visible.1));
    let (first, last) = ((from / step).floor() as i64, (to / step).ceil() as i64);
    for index in first..=last.min(first.saturating_add(4096)) {
        let beat = index as f64 * step;
        let x = x_of(beat).round() + 0.5;
        let ink = if on_multiple(beat, bar_beats) {
            bar_ink
        } else if on_multiple(beat, 1.0) {
            beat_ink
        } else {
            fine_ink
        };
        painter.vline(x, wave.y_range(), theme::hairline(ink));
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
            if n.rem_euclid(stride) == 0 { n.checked_add(1).map(|n| n.to_string()) } else { None }
        } else if on_beat && beat_width >= LEVEL_LEAST {
            position_label(beat, bar_beats).and_then(|text| text.rsplit_once('.').map(|(a, _)| a.to_owned()))
        } else if !on_beat && on_multiple(beat, 0.25) && beat_width / 4.0 >= LEVEL_LEAST {
            // Only a sixteenth's own mark (not a 32nd's), once sixteenths are as far apart as
            // beats are when they get their numbers.
            position_label(beat, bar_beats)
        } else {
            None
        };
        if let Some(text) = text {
            painter.text(
                // Beside the tick, inside the strip: a bar's tick runs the strip's height, and its
                // number written above the tick was drawn outside the strip, so never seen.
                Pos2::new(x + 3.0, strip.bottom() - tick.min(9.0) - 2.0),
                Align2::LEFT_BOTTOM,
                text,
                font.clone(),
                if on_bar { number } else { hint_ink() },
            );
        }
    }
}

/// The bar over the ruler: the file's name, its sample rate and channels at the left, the Sample /
/// Envelopes tabs in the middle (`editor-tab`, `[checked]` for the one shown, as the Clip / Devices
/// switch is drawn). Returns the tab clicked, when it was not on show.
fn header_bar(
    ui: &Ui,
    painter: &egui::Painter,
    header: Rect,
    clip: &AudioClip,
    peaks: &waveform::Peaks,
    tab: Tab,
) -> (Option<Tab>, Option<Action>) {
    use crate::kit::widgets;
    let mut focus = None;
    painter.rect_filled(header, 0.0, crate::detail::detail_fill("window"));
    painter
        .line_segment([header.left_bottom(), header.right_bottom()], theme::hairline(hint_ink().gamma_multiply(0.5)));
    let name = clip.view.file.rsplit(['/', '\\']).next().unwrap_or_default();
    let channels = peaks.data().map_or(peaks.lanes, |d| usize::from(d.channels));
    let format = format!("{name}  {:.1} kHz · {channels} Ch", f64::from(peaks.sample_rate) / 1000.0);
    let tabs = Rect::from_center_size(header.center(), Vec2::new(TABS_WIDTH, HEADER_HEIGHT - 4.0));
    let text_room = Rect::from_min_max(header.min + Vec2::new(6.0, 0.0), Pos2::new(tabs.left() - 8.0, header.bottom()));
    // Several clips picked: the name is a list of them that picks the one shown (its markers'
    // edits still go to every one as long as it, › Several clips).
    if !clip.others.is_empty() {
        let label = format!("{} samples selected", clip.others.len() + 1);
        let font = theme::ui_font(theme::TEXT_SMALL);
        let width = (widgets::text_width(ui, &label, font.clone()) + 22.0).min(text_room.width());
        let cell = Rect::from_min_size(
            Pos2::new(text_room.left(), header.center().y - (HEADER_HEIGHT - 4.0) / 2.0),
            Vec2::new(width, HEADER_HEIGHT - 4.0),
        );
        let response = widgets::button_area(ui, cell, egui::Id::unique("audio-editor-clips"), label.clone());
        widgets::text_in(painter, cell.shrink2(Vec2::new(4.0, 0.0)), Align2::LEFT_CENTER, &label, font, caption_ink());
        widgets::chevron(painter, Pos2::new(cell.right() - 7.0, cell.center().y), true, caption_ink());
        let rest = Rect::from_min_max(Pos2::new(cell.right() + 8.0, text_room.top()), text_room.max);
        painter.with_clip_rect(rest).text(
            rest.left_center(),
            Align2::LEFT_CENTER,
            &format,
            theme::ui_font(theme::TEXT_SMALL),
            caption_ink(),
        );
        egui::Popup::menu(&response).show(|ui| {
            for one in std::iter::once(clip).chain(clip.others.iter()) {
                let shown = std::ptr::eq(one, clip);
                if crate::kit::menu::item(ui, &one.name, shown).clicked() {
                    if !shown && let (Some(track), Some(id)) = (one.address.get("track"), one.address.get("clip")) {
                        focus = Some(Action::new("view.focusClip", json!({ "track": track, "clip": id })));
                    }
                    ui.close();
                }
            }
        });
    } else {
        // The name is cut at the tabs rather than left out when it is long.
        painter.with_clip_rect(text_room).text(
            text_room.left_center(),
            Align2::LEFT_CENTER,
            &format,
            theme::ui_font(theme::TEXT_SMALL),
            caption_ink(),
        );
    }
    let part = (tabs.width() - 1.0) / 2.0;
    let mut picked = None;
    for (index, (text, value)) in [("Sample", Tab::Sample), ("Envelopes", Tab::Envelopes)].into_iter().enumerate() {
        let cell = Rect::from_min_size(
            Pos2::new(tabs.left() + index as f32 * (part + 1.0), tabs.top()),
            Vec2::new(part, tabs.height()),
        );
        let response =
            widgets::button_area(ui, cell, egui::Id::unique(("audio-editor-tab", index)), format!("Show {text}"));
        if response.clicked() && value != tab {
            picked = Some(value);
        }
        let id = format!("tab-{}", text.to_ascii_lowercase());
        let lit = widgets::lit_toggle_kind(
            ui,
            &response,
            crate::kit::element::EDITOR_TAB.id(&id),
            &[crate::kit::element::Checked],
            value == tab,
            "switch",
        );
        let (fill, color) = lit.colors();
        lit.el.paint_with(painter, cell, crate::kit::element::Native { radius: 0.0, fill });
        drop(lit);
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_SMALL), color);
    }
    (picked, focus)
}

/// A tool of the editor's header picked this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Tool {
    Fit,
    Reset,
    Transients,
    Snap,
    Grid(PianoGrid),
}

/// The editor's tools along `header`'s right end, laid from the right and left out where they
/// would reach the tabs (ending at `tabs_right`): the grid's value (a click opens the grid's
/// entries, as the right-click menu holds them), Snap (lit while on), Fit, Reset warp… and
/// Transients. Each takes the editor's own tab look. The tool picked, if one was.
fn header_tools(
    ui: &Ui,
    painter: &egui::Painter,
    header: Rect,
    tabs_right: f32,
    grid: PianoGrid,
    value: String,
    keymap: &crate::settings::keymap::Keymap,
) -> Option<Tool> {
    use crate::kit::widgets;
    let font = theme::ui_font(theme::TEXT_SMALL);
    let height = HEADER_HEIGHT - 4.0;
    let mut right = header.right() - 4.0;
    let mut picked = None;
    let grid_label = format!("Editor grid: {value}");
    let snap_label = format!("Snap to grid: {}", if grid.snap { "On" } else { "Off" });
    // (shown, accessible name, lit, tool); the grid first, at the right end.
    let tools: [(&str, &str, Option<bool>, Option<Tool>); 5] = [
        (value.as_str(), grid_label.as_str(), None, None),
        ("Snap", snap_label.as_str(), Some(grid.snap), Some(Tool::Snap)),
        ("Reset…", "Reset warp…", None, Some(Tool::Reset)),
        ("Fit", "Fit", None, Some(Tool::Fit)),
        ("Transients", "Add warp markers at transients", None, Some(Tool::Transients)),
    ];
    for (index, (text, name, lit, tool)) in tools.into_iter().enumerate() {
        // The grid's cell has room for its chevron.
        let extra = if tool.is_none() { 14.0 } else { 0.0 };
        let width = widgets::text_width(ui, text, font.clone()) + 14.0 + extra;
        let left = right - width;
        if left < tabs_right + 8.0 {
            break;
        }
        let cell = Rect::from_min_size(Pos2::new(left, header.center().y - height / 2.0), Vec2::new(width, height));
        right = left - 3.0;
        let response = widgets::button_area(ui, cell, egui::Id::unique(("audio-editor-tool", index)), name);
        let id = format!("tool-{index}");
        let on = lit.unwrap_or(false);
        let look = widgets::lit_toggle_kind(
            ui,
            &response,
            crate::kit::element::EDITOR_TAB.id(&id),
            &[crate::kit::element::Checked],
            on,
            "switch",
        );
        let (fill, color) = look.colors();
        look.el.paint_with(painter, cell, crate::kit::element::Native { radius: 2.0, fill });
        drop(look);
        let words = Rect::from_min_max(cell.min, Pos2::new(cell.right() - extra, cell.bottom()));
        widgets::text_in(painter, words, Align2::CENTER_CENTER, text, font.clone(), color);
        match tool {
            Some(tool) => {
                if response.clicked() {
                    picked = Some(tool);
                }
            }
            None => {
                widgets::chevron(painter, Pos2::new(cell.right() - 9.0, cell.center().y), true, color);
                let actions_of_keys =
                    ["pianoRoll.gridSnap", "pianoRoll.gridNarrow", "pianoRoll.gridWiden", "pianoRoll.gridTriplet"];
                let keys = crate::interaction::grid_menu::grid_key_labels(keymap, &actions_of_keys);
                egui::Popup::menu(&response)
                    .layout(egui::Layout::top_down_justified(egui::Align::Min))
                    .style(egui::containers::menu::menu_style)
                    .show(|ui| {
                        if let Some(grid) = crate::interaction::grid_menu::grid_entries(ui, grid, &keys, &|_| true) {
                            picked = Some(Tool::Grid(grid));
                            ui.close();
                        }
                    });
            }
        }
    }
    picked
}

fn hint(painter: &egui::Painter, area: Rect, text: &str) {
    painter.text(area.center(), Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_TINY + 2.0), hint_ink());
}

#[cfg(test)]
#[path = "audio_editor_tests.rs"]
mod tests;
