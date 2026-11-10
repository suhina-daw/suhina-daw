// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An audio clip's gain envelope on its waveform in the Arrangement (`docs/spec/clip-panel.md` ›
//! Gain envelope, ADR 0035).
//!
//! 0 dB, the sample as it is, runs across the middle of the clip's body; gain goes up to the
//! ceiling (Settings › Arrangement › Gain envelope) at its top and down to −70 dB at its bottom,
//! even in dB above the middle and by gain below it. With Show gain envelope on, a clip with points shows its line. Edit gain
//! envelope (Alt+E, or the button left of the clip's name) opens one clip for editing: a click on
//! its line adds a point on it, a drag from the line or a point moves one (elsewhere the clip
//! takes the pointer as ever) (snapped to the grids, or free with Free move or Alt), a
//! double-click on a point puts it at 0 dB, Alt and a click or Delete over it removes it, Shift
//! (Ctrl+Shift) and a drag of the line moves all of it (a segment) up or down; each is one
//! `clip.setEnvelope`. The
//! waveform is drawn through the envelope, so it shows what is heard as a point is dragged.

use egui::{Id, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::automation::{Curve, CurveDrag};
use suhina_engine::model::{ClipId, GAIN_POINT_RANGE, GainPoint, db_to_gain, envelope_db_at};

use super::timeline::AudioView;
use crate::kit::theme;
use crate::kit::widgets::Action;

/// The width of a point's dot, as an automation lane's are round, and of the area that takes
/// the pointer around it. Points nearer than a dot's width on screen are one ([`show`]).
const POINT: f32 = 8.0;
const POINT_HIT: f32 = 11.0;
/// Most passes of a loop whose points are drawn; a long loop of a short stretch would draw
/// thousands of dots nobody can tell apart.
const MAX_PASSES: usize = 256;
/// The line is drawn as a point every this many points across.
const STEP: f32 = 2.0;
/// Grid lines closer than this, in points, are left out (every other one, and so on).
const GRID_GAP: f32 = 5.0;
/// How near 0 dB, in points, a dragged point is pulled onto it.
const ZERO_PULL: f32 = 3.0;
/// How near the line, in points, the pointer adds a point or bends a segment.
const LINE_HIT: f32 = 5.0;
/// The bend handle half-way along a segment, a ring (hollow, as a device's curve handle is, so it
/// is not taken for a point): its radius, and the narrowest segment, in points, that has one.
const CURVE_HANDLE: f32 = 3.5;
const CURVE_HANDLE_MIN: f32 = 28.0;
/// Points of drag for one step of a curve drag, as on an automation lane.
const POINTS_PER_STEP: f32 = 1.5;
/// The side of the buttons on the clip: Edit gain envelope by the name, Free move under it.
pub const BUTTON: f32 = 14.0;

/// How the envelope is shown and edited (Settings › Arrangement › Gain envelope).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvelopeSettings {
    /// Clips with points show their envelope while they are not being edited.
    pub show: bool,
    /// The highest gain a point can be dragged to, in dB.
    pub ceiling: f64,
    /// The dB grid points snap to, in dB.
    pub step: f64,
    /// Points move freely, not on the grid, until Free move is switched.
    pub free: bool,
    /// Edit gain envelope opens every audio clip's envelope at once (a mode, Alt+E on and off);
    /// otherwise one clip's at a time, and opening another closes it.
    pub all: bool,
}

impl Default for EnvelopeSettings {
    fn default() -> Self {
        Self { show: true, ceiling: 6.0, step: 1.0, free: false, all: true }
    }
}

fn settings_key() -> Id {
    Id::unique("clip-envelope-settings")
}

/// Hands the settings to the views drawn this frame.
pub fn set_settings(ctx: &egui::Context, settings: EnvelopeSettings) {
    ctx.data_mut(|d| d.insert_temp(settings_key(), settings));
}

/// The settings the window handed over.
pub fn settings(ctx: &egui::Context) -> EnvelopeSettings {
    ctx.data(|d| d.get_temp::<EnvelopeSettings>(settings_key())).unwrap_or_default()
}

fn free_key() -> Id {
    Id::unique("clip-envelope-free")
}

/// Whether points move freely: Free move as last switched, else the setting.
pub fn free_move(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(free_key())).unwrap_or_else(|| settings(ctx).free)
}

/// Switches Free move.
pub fn set_free_move(ctx: &egui::Context, free: bool) {
    ctx.data_mut(|d| d.insert_temp(free_key(), free));
}

/// The window-wide key of what is being edited.
fn editing_key() -> Id {
    Id::unique("clip-envelope-editing")
}

/// Whose gain envelopes are open for editing: every audio clip's (the mode, Settings › Gain
/// envelope › Envelope editing: all clips) or one clip's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Editing {
    All,
    Clip(ClipId),
}

fn editing_state(ctx: &egui::Context) -> Option<Editing> {
    ctx.data(|d| d.get_temp::<Editing>(editing_key()))
}

/// Whether `clip`'s gain envelope is open for editing.
pub fn is_editing(ctx: &egui::Context, clip: ClipId) -> bool {
    match editing_state(ctx) {
        Some(Editing::All) => true,
        Some(Editing::Clip(open)) => open == clip,
        None => false,
    }
}

/// Whether any clip's gain envelope is open for editing.
pub fn any_editing(ctx: &egui::Context) -> bool {
    editing_state(ctx).is_some()
}

/// Opens `clip`'s envelope for editing (or every clip's, with the setting so), or closes them.
pub fn set_editing(ctx: &egui::Context, clip: Option<ClipId>) {
    let all = settings(ctx).all;
    ctx.data_mut(|d| match clip {
        Some(_) if all => {
            d.insert_temp(editing_key(), Editing::All);
        }
        Some(clip) => {
            d.insert_temp(editing_key(), Editing::Clip(clip));
        }
        None => d.remove::<Editing>(editing_key()),
    });
}

/// Edit gain envelope on `clip`, or off when it is on. With the setting on every clip, it is a
/// mode: on for every audio clip, and off again from any of them.
pub fn toggle_editing(ctx: &egui::Context, clip: ClipId) {
    let on = if settings(ctx).all { any_editing(ctx) } else { is_editing(ctx, clip) };
    set_editing(ctx, (!on).then_some(clip));
}

/// The top of the scale for `points`: the ceiling, or higher when a point is above it (set before
/// the ceiling was lowered), so the line is drawn where it is heard.
pub fn top(ceiling: f64, points: &[GainPoint]) -> f64 {
    let highest = points.iter().map(|p| p.db).fold(f64::NEG_INFINITY, f64::max);
    ceiling.max(highest).clamp(0.5, *GAIN_POINT_RANGE.end())
}

/// How far down the lower half `db` (below 0) is, from 0 at the middle to 1 at −70 dB: by the
/// gain it plays at, as the waveform is drawn, so the first few dB of a cut, where most cuts are,
/// take most of the half (−3 dB about 30 %, −6 dB half, −20 dB 90 %).
fn depth(db: f64) -> f64 {
    let floor = db_to_gain(*GAIN_POINT_RANGE.start());
    ((1.0 - db_to_gain(db)) / (1.0 - floor)).clamp(0.0, 1.0)
}

/// The dB `depth` down the lower half, the inverse of [`depth`].
fn depth_db(depth: f64) -> f64 {
    let floor = db_to_gain(*GAIN_POINT_RANGE.start());
    let gain = 1.0 - depth.clamp(0.0, 1.0) * (1.0 - floor);
    (20.0 * gain.max(floor).log10()).max(*GAIN_POINT_RANGE.start())
}

/// The height of `db` in `rect`: 0 dB across the middle, `top` at the top, even in dB above the
/// middle, and −70 dB at the bottom, by the gain below it ([`depth`]).
pub fn y_of(rect: Rect, db: f64, top: f64) -> f32 {
    let middle = rect.center().y;
    let half = rect.height() / 2.0;
    if db >= 0.0 { middle - (db.min(top) / top.max(1e-9)) as f32 * half } else { middle + depth(db) as f32 * half }
}

/// The dB at height `y` in `rect`, the inverse of [`y_of`], kept between −70 dB and `top`.
pub fn db_at(rect: Rect, y: f32, top: f64) -> f64 {
    let half = rect.height() / 2.0;
    if half <= 0.0 {
        return 0.0;
    }
    let t = f64::from((rect.center().y - y) / half).clamp(-1.0, 1.0);
    if t >= 0.0 { t * top } else { depth_db(-t) }
}

/// `db` on the grid of `step` dB, or to a tenth of a dB when free.
pub fn snapped(db: f64, step: f64, free: bool) -> f64 {
    if free || step <= 0.0 { (db * 10.0).round() / 10.0 } else { (db / step).round() * step }
}

/// How the clip maps its own beats (from its start) to its content's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mapping {
    pub offset: f64,
    pub loop_range: Option<(f64, f64)>,
    pub rate: f64,
}

impl Mapping {
    pub fn of(view: &AudioView) -> Self {
        let loop_range = view.loop_range.filter(|(s, e)| s.is_finite() && e.is_finite() && e > s);
        let offset = match loop_range {
            Some((start, end)) if view.offset >= end => start,
            _ => view.offset,
        };
        Self { offset, loop_range, rate: if view.rate > 0.0 { view.rate } else { 1.0 } }
    }

    /// The content beat `elapsed` beats after the clip starts, as the engine plays it.
    pub fn content(&self, elapsed: f64) -> f64 {
        let beat = self.offset + elapsed * self.rate;
        match self.loop_range {
            Some((start, end)) if beat >= end => start + (beat - end).rem_euclid(end - start),
            _ => beat,
        }
    }

    /// Where content beat `beat` plays in a clip `length` beats long, in beats from its start: once,
    /// or on every pass of its loop.
    pub fn places(&self, beat: f64, length: f64) -> Vec<f64> {
        let mut out = Vec::new();
        match self.loop_range {
            None => {
                let at = (beat - self.offset) / self.rate;
                if (0.0..=length).contains(&at) {
                    out.push(at);
                }
            }
            Some((start, end)) => {
                // Before the loop's first end: from the start marker on.
                if beat >= self.offset && beat < end {
                    out.push((beat - self.offset) / self.rate);
                }
                if beat >= start && beat < end {
                    let first = (end - self.offset) / self.rate;
                    let pass = (end - start) / self.rate;
                    for k in 0..MAX_PASSES {
                        let at = first + k as f64 * pass + (beat - start) / self.rate;
                        if at > length {
                            break;
                        }
                        out.push(at);
                    }
                }
            }
        }
        out
    }
}

/// The draft of a point being dragged: the clip, the point's index and its envelope as it is now.
#[derive(Debug, Clone, PartialEq)]
struct Draft {
    clip: ClipId,
    index: usize,
    points: Vec<GainPoint>,
}

fn draft_key() -> Id {
    Id::unique("clip-envelope-draft")
}

/// A part of the envelope being moved up or down as one (Shift and a drag of the line: all of it;
/// Ctrl+Shift: the segment under the pointer), as a spectral editor's clip gain does: the points
/// as they were, which of them move (`None`: all) and the line's level where the drag started.
#[derive(Debug, Clone, PartialEq)]
struct Lift {
    clip: ClipId,
    from: Vec<GainPoint>,
    moved: Option<Vec<usize>>,
    start: f64,
}

fn lift_key() -> Id {
    Id::unique("clip-envelope-lift")
}

/// The points bounding the segment at content beat `beat`: the two around it, or the one at an
/// end when it lies before the first point or after the last.
fn segment_at(points: &[GainPoint], beat: f64) -> Vec<usize> {
    let after = points.partition_point(|p| p.beat <= beat);
    after.checked_sub(1).into_iter().chain((after < points.len()).then_some(after)).collect()
}

/// `from` with the points `moved` (all when `None`) `delta` dB higher, kept in -70 dB and the
/// ceiling (or where a point already was above it).
fn lifted(from: &[GainPoint], moved: Option<&[usize]>, delta: f64, ceiling: f64) -> Vec<GainPoint> {
    let mut points = from.to_vec();
    for (i, p) in points.iter_mut().enumerate() {
        if moved.is_none_or(|m| m.contains(&i)) {
            p.db = (p.db + delta).clamp(*GAIN_POINT_RANGE.start(), ceiling.max(p.db));
        }
    }
    points
}

/// The clip whose envelope line the pointer was pressed on, while a button is down.
fn held_key() -> Id {
    Id::unique("clip-envelope-held")
}

/// A segment's curve being dragged (its bend handle, or Alt and a drag from the line), as on an
/// automation lane (`docs/spec/automation.md` › Dragging a curve): the segment's first point,
/// the drag's paths and how far it has moved.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CurveEdit {
    clip: ClipId,
    index: usize,
    paths: CurveDrag,
    moved: f32,
}

fn curve_key() -> Id {
    Id::unique("clip-envelope-curve")
}

fn draft(ctx: &egui::Context, clip: ClipId) -> Option<Draft> {
    ctx.data(|d| d.get_temp::<Draft>(draft_key())).filter(|d| d.clip == clip)
}

/// `clip`'s envelope as it is drawn now: a point being dragged where it is, else `committed`.
pub fn points_now(ctx: &egui::Context, clip: ClipId, committed: &[GainPoint]) -> Vec<GainPoint> {
    draft(ctx, clip).map_or_else(|| committed.to_vec(), |d| d.points)
}

/// The envelope's gain, as a factor, `elapsed` beats after the clip starts: what the waveform is
/// drawn through. 1 without points.
pub fn gain_at(points: &[GainPoint], mapping: &Mapping, elapsed: f64) -> f32 {
    if points.is_empty() { 1.0 } else { db_to_gain(envelope_db_at(points, mapping.content(elapsed))) as f32 }
}

/// The points as `clip.setEnvelope` takes them: `[beat, dB, …]`.
fn flat(points: &[GainPoint]) -> Vec<f64> {
    points.iter().flat_map(|p| [p.beat, p.db]).collect()
}

fn set(clip: ClipId, points: &[GainPoint]) -> Action {
    // Each point's curve, null for a straight one.
    let curves: Vec<serde_json::Value> = points
        .iter()
        .map(|p| if p.curve.is_straight() { serde_json::Value::Null } else { json!(p.curve.to_array()) })
        .collect();
    let mut params = json!({ "clip": clip.0, "points": flat(points) });
    if curves.iter().any(|c| !c.is_null()) {
        params["curves"] = json!(curves);
    }
    Action::new("clip.setEnvelope", params)
}

/// The least time between two sends of a drag, in seconds: about the engine's glide from one
/// envelope to the next (`ENVELOPE_GLIDE_SECONDS`), so the sound moves on smoothly between them.
const SEND_GAP: f64 = 0.03;

/// When a drag last sent the envelope (the input's time), so the next waits [`SEND_GAP`].
fn sent_key() -> Id {
    Id::unique("clip-envelope-sent")
}

/// The gesture a drag of the envelope sends its frames in: one undo step for the whole drag.
const GESTURE: &str = "clip-envelope";

/// [`set`] as a frame of a drag: the clip plays the envelope as it is dragged, and the frames
/// make one undo step.
fn dragged(clip: ClipId, points: &[GainPoint]) -> Action {
    let mut action = set(clip, points);
    action.params["gesture"] = crate::interaction::commands::gesture(GESTURE);
    action
}

/// `points` with a point at `beat` and `db` put in beat order; a bent segment it lands on is cut
/// there so its two halves keep the curve's shape (`Curve::split`). Returns the points and the
/// new one's index.
fn with_point(points: &[GainPoint], beat: f64, db: f64) -> (Vec<GainPoint>, usize) {
    let mut more = points.to_vec();
    let at = more.partition_point(|p| p.beat <= beat);
    let mut point = GainPoint::new(beat, db);
    if let (Some(k), Some(next)) = (at.checked_sub(1), more.get(at).copied())
        && let Some(prev) = more.get_mut(k)
        && next.beat > prev.beat
        && let Some(split) = prev.curve.split((beat - prev.beat) / (next.beat - prev.beat))
    {
        prev.curve = split.before;
        point.curve = split.after;
    }
    more.insert(at, point);
    (more, at)
}

/// The dB a point dragged to `pointer` gets, `before` being where it was: on the grid (or free),
/// below the ceiling, a tenth of the way with Shift; near the middle, 0 dB itself. Alt holds it
/// off the grid and the middle, as Free move does.
fn dragged_db(ui: &Ui, rect: Rect, pointer: Pos2, before: f64, top: f64) -> f64 {
    let settings = settings(ui.ctx());
    let target = db_at(rect, pointer.y, top);
    let fine = ui.input(|i| i.modifiers.shift);
    let loose = ui.input(|i| i.modifiers.alt);
    // The sample's own level is pulled to, so the line goes back to it exactly.
    if !fine && !loose && (pointer.y - y_of(rect, 0.0, top)).abs() <= ZERO_PULL {
        return 0.0;
    }
    let db = if fine { before + (target - before) * 0.1 } else { target };
    // A point already above the ceiling may stay there, never go higher.
    let ceiling = settings.ceiling.max(before);
    snapped(db, settings.step, fine || loose || free_move(ui.ctx())).clamp(*GAIN_POINT_RANGE.start(), ceiling)
}

/// Where a clip's envelope is drawn: its body `rect` as drawn now (an edge being dragged
/// included), the x of the clip's own start, where its beats are counted from, and the width of
/// a beat, so the line goes on with the passes of a loop as an edge is dragged out.
#[derive(Debug, Clone, Copy)]
pub struct Place {
    pub rect: Rect,
    pub origin: f32,
    pub beat_width: f32,
    /// The ink that reads on the clip's colour, as its name has: white on most, black on a
    /// light one. The line and the points are drawn in it.
    pub ink: egui::Color32,
}

/// Draws `view`'s envelope over the clip at `place`. While `edit` is on, the dB grid shows, the
/// body takes the pointer (a click adds a point, a drag adds one and moves it) and the points can
/// be dragged; `snap` puts a beat from the clip's start on the timeline's grid, unless Free move
/// is on or Alt (or Shift, which also moves a tenth as much) held. What changes is pushed to `actions`. The points' dots are named
/// "Envelope point N of NAME".
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &Ui,
    painter: &egui::Painter,
    place: Place,
    clip: ClipId,
    name: &str,
    view: &AudioView,
    edit: bool,
    snap: &dyn Fn(f64) -> f64,
    actions: &mut Vec<Action>,
) {
    let Place { rect, origin, beat_width, ink: line_ink } = place;
    let length = f64::from((rect.right() - origin) / beat_width.max(1e-3));
    if rect.width() <= 1.0 || rect.height() <= 4.0 || length <= 0.0 {
        return;
    }
    let settings = settings(ui.ctx());
    let points = points_now(ui.ctx(), clip, &view.envelope);
    if !edit && (!settings.show || points.is_empty()) {
        return;
    }
    let mapping = Mapping::of(view);
    let top = top(settings.ceiling, &points);
    let x_of = |elapsed: f64| origin + elapsed as f32 * beat_width;
    let elapsed_at = |x: f32| f64::from((x - origin) / beat_width.max(1e-3)).clamp(0.0, length);
    // On the timeline's grid sideways too, unless the points move freely.
    // Alt frees a point being dragged, as on the piano roll's grid.
    let free = free_move(ui.ctx()) || ui.input(|i| i.modifiers.shift || i.modifiers.alt);
    let placed_at = |x: f32| {
        let elapsed = elapsed_at(x);
        if free { elapsed } else { snap(elapsed).clamp(0.0, length) }
    };
    let ink = theme::accent();
    // Over a halo of the other of black and white, so they read on the waveform too.
    let [r, g, b, _] = line_ink.to_srgba_unmultiplied();
    let light = u16::from(r) + u16::from(g) + u16::from(b) > 3 * 128;
    let halo = if light { egui::Color32::from_black_alpha(150) } else { egui::Color32::from_white_alpha(150) };
    let visible = painter.clip_rect().intersect(rect);

    if edit {
        paint_grid(painter, rect, visible, settings.step, top, ink);
    }
    let mut line = Vec::new();
    let mut x = visible.left();
    while x <= visible.right() {
        let db = envelope_db_at(&points, mapping.content(elapsed_at(x)));
        line.push(Pos2::new(x, y_of(rect, db, top)));
        x += STEP;
    }
    if line.len() >= 2 {
        let width = if edit { 2.5 } else { 2.0 };
        let stroke = if edit { line_ink } else { line_ink.gamma_multiply(0.85) };
        painter.add(egui::Shape::line(line.clone(), Stroke::new(width + 2.5, halo)));
        painter.add(egui::Shape::line(line, Stroke::new(width, stroke)));
    }
    if !edit {
        return;
    }

    // The body only names the envelope; clicks and drags away from the line go to the clip as
    // ever (select, move). On the line: a click adds a point on it, a drag adds one and moves it;
    // with Alt a drag bends the segment under the pointer, as on an automation lane.
    ui.interact(visible, Id::unique(("clip-envelope-area", clip.0)), Sense::hover())
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, format!("Gain envelope of {name}")));
    let any_down = ui.input(|i| i.pointer.any_down());
    if !any_down {
        ui.data_mut(|d| d.remove::<ClipId>(held_key()));
    }
    let mods = ui.input(|i| i.modifiers);
    // Once pressed on the line, the drag goes on wherever the pointer goes.
    let held = any_down && ui.data(|d| d.get_temp::<ClipId>(held_key())) == Some(clip);
    let line_y = |x: f32| y_of(rect, envelope_db_at(&points, mapping.content(elapsed_at(x))), top);
    let near =
        ui.input(|i| i.pointer.hover_pos()).filter(|p| visible.contains(*p) && (p.y - line_y(p.x)).abs() <= LINE_HIT);
    let hit = if held {
        Some(visible)
    } else {
        near.map(|p| {
            Rect::from_center_size(Pos2::new(p.x, line_y(p.x)), Vec2::new(LINE_HIT * 6.0, LINE_HIT * 2.0))
                .intersect(visible)
        })
    };
    let body = hit.map(|hit| {
        ui.interact(hit, Id::unique(("clip-envelope-body", clip.0)), Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
    });
    if body.as_ref().is_some_and(egui::Response::is_pointer_button_down_on) {
        ui.data_mut(|d| d.insert_temp(held_key(), clip));
    }
    let alt = mods.alt;
    // A point added on the line keeps the line's shape: at the level the line has there.
    let add_at = |pointer: Pos2| {
        let beat = mapping.content(placed_at(pointer.x));
        with_point(&points, beat, (envelope_db_at(&points, beat) * 100.0).round() / 100.0)
    };
    // A point already there, within a dot's width on screen: the line takes no second one on it.
    let taken = |x: f32| {
        points.iter().any(|p| mapping.places(p.beat, length).into_iter().any(|at| (x_of(at) - x).abs() < POINT))
    };
    if let Some(body) = &body
        && body.clicked()
        && let Some(pointer) = body.interact_pointer_pos()
    {
        if !alt {
            if !taken(pointer.x) {
                actions.push(set(clip, &add_at(pointer).0));
            }
        } else if mods.shift {
            // Alt+Shift and a click takes the segment's points away.
            let gone = segment_at(&points, mapping.content(elapsed_at(pointer.x)));
            if !gone.is_empty() {
                let kept: Vec<GainPoint> =
                    points.iter().enumerate().filter(|(i, _)| !gone.contains(i)).map(|(_, p)| *p).collect();
                actions.push(set(clip, &kept));
            }
        }
        return;
    }
    if let Some(body) = &body
        && body.drag_started_by(egui::PointerButton::Primary)
        && let Some(pointer) = body.interact_pointer_pos()
    {
        let beat = mapping.content(elapsed_at(pointer.x));
        if alt && !mods.shift {
            let segment = points.partition_point(|p| p.beat <= beat).checked_sub(1).filter(|i| i + 1 < points.len());
            if let Some(index) = segment {
                start_curve(ui, clip, index, &points);
            }
        } else if mods.shift && !alt {
            // Shift moves the whole line up or down, Ctrl+Shift the segment under the pointer; a
            // line without points gets one there to move.
            let from = if points.is_empty() { vec![GainPoint::new(beat, 0.0)] } else { points.clone() };
            let moved = mods.command.then(|| segment_at(&from, beat));
            let start = envelope_db_at(&from, beat);
            ui.data_mut(|d| {
                d.insert_temp(lift_key(), Lift { clip, from: from.clone(), moved, start });
                d.insert_temp(draft_key(), Draft { clip, index: usize::MAX, points: from });
            });
        } else if !alt && !taken(pointer.x) {
            let (more, index) = add_at(pointer);
            ui.data_mut(|d| d.insert_temp(draft_key(), Draft { clip, index, points: more }));
        }
    }
    let mut dragging = None;
    if let Some(body) = &body
        && body.dragged_by(egui::PointerButton::Primary)
    {
        if let Some(lift) = ui.data(|d| d.get_temp::<Lift>(lift_key())).filter(|l| l.clip == clip)
            && let Some(pointer) = body.interact_pointer_pos()
        {
            let raw = db_at(rect, pointer.y, top) - lift.start;
            let loose = alt || free_move(ui.ctx()) || settings.step <= 0.0;
            let delta = if loose { (raw * 10.0).round() / 10.0 } else { (raw / settings.step).round() * settings.step };
            let moved = lifted(&lift.from, lift.moved.as_deref(), delta, settings.ceiling);
            ui.data_mut(|d| d.insert_temp(draft_key(), Draft { clip, index: usize::MAX, points: moved }));
            egui::Tooltip::always_open(
                ui.ctx().clone(),
                ui.layer_id(),
                Id::unique(("clip-envelope-value", clip.0)),
                egui::PopupAnchor::Pointer,
            )
            .gap(12.0)
            .show(|ui| ui.add(egui::Label::new(db_text(delta)).wrap_mode(egui::TextWrapMode::Extend)));
        } else if curve_edit(ui.ctx(), clip).is_some() {
            drag_curve(ui, clip, body.drag_delta().y);
        } else {
            dragging = draft(ui.ctx(), clip).map(|d| d.index);
        }
    }

    // The bend handles, half-way along each sloping segment wide enough, on every pass: a drag
    // bends it, a double-click straightens it.
    for (index, pair) in points.windows(2).enumerate() {
        let [a, b] = pair else { continue };
        if a.db == b.db || b.beat <= a.beat {
            continue;
        }
        let span = (b.beat - a.beat) / mapping.rate;
        for (pass, at) in mapping.places(a.beat, length).into_iter().enumerate() {
            let (xa, xb) = (x_of(at), x_of(at + span));
            if xb - xa < CURVE_HANDLE_MIN || xb < visible.left() || xa > visible.right() {
                continue;
            }
            let mid = (a.beat + b.beat) / 2.0;
            let center = Pos2::new((xa + xb) / 2.0, y_of(rect, envelope_db_at(&points, mid), top));
            let hit = Rect::from_center_size(center, Vec2::splat(POINT_HIT)).intersect(visible);
            if hit.width() <= 0.0 || hit.height() <= 0.0 {
                continue;
            }
            let id = Id::unique(("clip-envelope-curve", clip.0, index, pass));
            let response =
                ui.interact(hit, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeVertical);
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Slider, true, format!("Envelope curve {} of {name}", index + 1))
            });
            if response.double_clicked() {
                let mut straight = points.clone();
                if let Some(p) = straight.get_mut(index) {
                    p.curve = Curve::STRAIGHT;
                }
                ui.data_mut(|d| {
                    d.remove::<Draft>(draft_key());
                    d.remove::<CurveEdit>(curve_key());
                });
                actions.push(set(clip, &straight));
                return;
            }
            if response.drag_started_by(egui::PointerButton::Primary) {
                start_curve(ui, clip, index, &points);
            }
            if response.dragged_by(egui::PointerButton::Primary) {
                drag_curve(ui, clip, response.drag_delta().y);
            }
            let hover = if response.hovered() || response.dragged() { 1.5 } else { 0.0 };
            painter.circle_stroke(center, CURVE_HANDLE + hover, Stroke::new(3.5, halo));
            painter.circle_stroke(center, CURVE_HANDLE + hover, Stroke::new(1.5, line_ink));
        }
    }

    // The points, on every pass they play.
    let mut removed = None;
    let mut zeroed = None;
    for (index, point) in points.iter().enumerate() {
        for (pass, at) in mapping.places(point.beat, length).into_iter().enumerate() {
            let center = Pos2::new(x_of(at), y_of(rect, point.db, top));
            let hit = Rect::from_center_size(center, Vec2::splat(POINT_HIT));
            if !visible.intersects(hit) {
                continue;
            }
            let id = Id::unique(("clip-envelope-point", clip.0, index, pass));
            let response = ui.interact(hit, id, Sense::click_and_drag());
            let response = if response.dragged() {
                response.on_hover_cursor(egui::CursorIcon::Grabbing)
            } else {
                response.on_hover_cursor(egui::CursorIcon::Grab)
            };
            let label = format!("Envelope point {} of {name}", index + 1);
            response.widget_info(|| {
                let mut info = egui::WidgetInfo::labeled(egui::Role::Slider, true, &label);
                info.value = Some(point.db);
                info
            });
            let hovered = response.hovered() || response.dragged() || dragging == Some(index);
            let radius = if hovered { POINT / 2.0 + 1.0 } else { POINT / 2.0 };
            painter.circle_filled(center, radius + 1.5, halo);
            painter.circle_filled(center, radius, if hovered { ink } else { line_ink });
            // Delete over it or Alt and a click takes it away; a double-click puts it back at 0 dB.
            let delete_key = response.hovered() && ui.input(|i| i.key_pressed(egui::Key::Delete));
            if delete_key || (response.clicked() && mods.alt) {
                removed = Some(index);
                continue;
            }
            if response.double_clicked() {
                zeroed = Some(index);
                continue;
            }
            if response.drag_started_by(egui::PointerButton::Primary) {
                ui.data_mut(|d| d.insert_temp(draft_key(), Draft { clip, index, points: points.clone() }));
            }
            // The point the drag holds, by where it is now: passing another changes its index.
            if response.dragged_by(egui::PointerButton::Primary) {
                dragging = draft(ui.ctx(), clip).map(|d| d.index).or(Some(index));
            }
        }
    }
    // The point being dragged follows the pointer: in beat between its neighbours, so the points
    // stay in order, and in dB on the grid below the ceiling.
    if let (Some(index), Some(pointer)) = (dragging, ui.input(|i| i.pointer.latest_pos()))
        && let Some(mut moved) = draft(ui.ctx(), clip).map(|d| d.points)
    {
        let before = moved.get(index).copied().unwrap_or(GainPoint::new(0.0, 0.0));
        let fine = ui.input(|i| i.modifiers.shift);
        let target = mapping.content(placed_at(pointer.x));
        let beat = if fine { before.beat + (target - before.beat) * 0.1 } else { target };
        let db = dragged_db(ui, rect, pointer, before.db, top);
        let index = passed(&mut moved, index, beat.max(0.0), db);
        ui.data_mut(|d| d.insert_temp(draft_key(), Draft { clip, index, points: moved }));
        // Its value beside the pointer while the button is down.
        egui::Tooltip::always_open(
            ui.ctx().clone(),
            ui.layer_id(),
            Id::unique(("clip-envelope-value", clip.0)),
            egui::PopupAnchor::Pointer,
        )
        .gap(12.0)
        .show(|ui| ui.add(egui::Label::new(db_text(db)).wrap_mode(egui::TextWrapMode::Extend)));
    }
    // While the button is down the envelope as dragged is set as it changes, so the clip is heard
    // as it is drawn; the sends are one gesture, one undo step. At most one each SEND_GAP: a new
    // graph a frame (150 a second) came faster than a stretched clip's voice is handed over, and
    // the fades between its voices were cut short, which crackled.
    if let Some(now) = draft(ui.ctx(), clip)
        && ui.input(|i| i.pointer.any_down())
        && now.points != view.envelope
    {
        let time = ui.input(|i| i.time);
        let last = ui.data(|d| d.get_temp::<f64>(sent_key()));
        match last.map(|last| time - last) {
            Some(since) if since < SEND_GAP => ui.ctx().request_repaint_after_secs((SEND_GAP - since) as f32),
            _ => {
                ui.data_mut(|d| d.insert_temp(sent_key(), time));
                actions.push(dragged(clip, &now.points));
            }
        }
    }
    // Once the button is up, what the point or the curve moved to is set, and the draft goes:
    // also for a drag whose point stopped being drawn (its pass moved past the clip's end).
    if let Some(done) = draft(ui.ctx(), clip)
        && !ui.input(|i| i.pointer.any_down())
    {
        ui.data_mut(|d| {
            d.remove::<Draft>(draft_key());
            d.remove::<CurveEdit>(curve_key());
            d.remove::<Lift>(lift_key());
        });
        // A point let go within a dot's width of another's dot, on screen (on any pass of a loop),
        // is one with it.
        let mut kept = done.points;
        let dots = |p: &GainPoint| -> Vec<Pos2> {
            let y = y_of(rect, p.db, top);
            mapping.places(p.beat, length).into_iter().map(|at| Pos2::new(x_of(at), y)).collect()
        };
        if let Some(moved) = kept.get(done.index) {
            let here = dots(moved);
            let on_another = kept.iter().enumerate().filter(|(i, _)| *i != done.index).any(|(_, p)| {
                dots(p).iter().any(|q| here.iter().any(|h| (q.x - h.x).abs() < POINT && (q.y - h.y).abs() < POINT))
            });
            if on_another {
                kept.remove(done.index);
            }
        }
        if kept != view.envelope {
            actions.push(dragged(clip, &kept));
        }
    }
    if let Some(index) = removed {
        let mut kept = points;
        if index < kept.len() {
            kept.remove(index);
        }
        ui.data_mut(|d| d.remove::<Draft>(draft_key()));
        actions.push(set(clip, &kept));
    } else if let Some(index) = zeroed {
        let mut reset = points;
        if let Some(p) = reset.get_mut(index) {
            p.db = 0.0;
        }
        ui.data_mut(|d| d.remove::<Draft>(draft_key()));
        actions.push(set(clip, &reset));
    }
}

/// Moves point `index` of `points` to `beat` and `db`, past the points it is dragged over, as an
/// automation lane's (`docs/spec/automation.md`): the points stay in beat order, and on another
/// point's beat it stays on the side it came from. Returns its index now.
fn passed(points: &mut Vec<GainPoint>, index: usize, beat: f64, db: f64) -> usize {
    if index >= points.len() {
        return index;
    }
    let mut point = points.remove(index);
    let later = beat >= point.beat;
    point.beat = beat;
    point.db = db;
    // Past the points it now lies beyond, never past one on its own beat that it has not crossed.
    let at = if later {
        points.partition_point(|p| p.beat < beat).max(index)
    } else {
        points.partition_point(|p| p.beat <= beat).min(index)
    }
    .min(points.len());
    points.insert(at, point);
    at
}

fn curve_edit(ctx: &egui::Context, clip: ClipId) -> Option<CurveEdit> {
    ctx.data(|d| d.get_temp::<CurveEdit>(curve_key())).filter(|c| c.clip == clip)
}

/// Starts a drag of the curve of the segment from point `index`, the envelope `points` as it is.
fn start_curve(ui: &Ui, clip: ClipId, index: usize, points: &[GainPoint]) {
    let curve = points.get(index).map_or(Curve::STRAIGHT, |p| p.curve);
    let paths = CurveDrag::new(&curve);
    let mut drafted = points.to_vec();
    if let Some(p) = drafted.get_mut(index) {
        p.curve = paths.dragged(0);
    }
    ui.data_mut(|d| {
        d.insert_temp(curve_key(), CurveEdit { clip, index, paths, moved: 0.0 });
        d.insert_temp(draft_key(), Draft { clip, index, points: drafted });
    });
}

/// Moves the curve being dragged by `dy` points of the pointer: up bends the line up, which moves
/// a rising segment's curve earlier and a falling one's later.
fn drag_curve(ui: &Ui, clip: ClipId, dy: f32) {
    let (Some(mut edit), Some(mut drafted)) = (curve_edit(ui.ctx(), clip), draft(ui.ctx(), clip)) else { return };
    let rising = match (drafted.points.get(edit.index), drafted.points.get(edit.index + 1)) {
        (Some(a), Some(b)) => b.db >= a.db,
        _ => true,
    };
    edit.moved += if rising { dy } else { -dy };
    if let Some(p) = drafted.points.get_mut(edit.index) {
        p.curve = edit.paths.dragged((edit.moved / POINTS_PER_STEP).round() as i32);
    }
    ui.data_mut(|d| {
        d.insert_temp(curve_key(), edit);
        d.insert_temp(draft_key(), drafted);
    });
}

/// A point's value as it is shown: "+1.5 dB", "0 dB", "−6 dB".
pub fn db_text(db: f64) -> String {
    let rounded = (db * 10.0).round() / 10.0;
    if rounded == 0.0 {
        "0 dB".into()
    } else if rounded > 0.0 {
        format!("+{} dB", trim(rounded))
    } else {
        format!("\u{2212}{} dB", trim(-rounded))
    }
}

fn trim(value: f64) -> String {
    let text = format!("{value:.1}");
    text.strip_suffix(".0").map_or(text.clone(), str::to_string)
}

/// The dB grid while editing: a faint line every `step` dB (fewer where they would crowd), the
/// 0 dB line stronger.
fn paint_grid(painter: &egui::Painter, rect: Rect, visible: Rect, step: f64, top: f64, ink: egui::Color32) {
    let faint = Stroke::new(1.0, ink.gamma_multiply(0.12));
    let low = *GAIN_POINT_RANGE.start();
    for (from, to, sign) in [(0.0, top, 1.0), (0.0, low, -1.0)] {
        let mut last = y_of(rect, from, top);
        let mut k = 1.0;
        loop {
            let db = from + sign * k * step.max(0.1);
            if (sign > 0.0 && db > to + 1e-9) || (sign < 0.0 && db < to - 1e-9) {
                break;
            }
            let y = y_of(rect, db, top);
            if (y - last).abs() >= GRID_GAP {
                painter.hline(visible.x_range(), y, faint);
                last = y;
            }
            k += 1.0;
        }
    }
    painter.hline(visible.x_range(), y_of(rect, 0.0, top), Stroke::new(1.0, ink.gamma_multiply(0.4)));
}

/// Clear gain envelope in an audio clip's menu: takes all of `envelope`'s points away in one
/// `clip.setEnvelope`, so the clip plays at its own level again; off while it has none.
pub fn clear_item(ui: &mut Ui, clip: ClipId, envelope: &[GainPoint], actions: &mut Vec<Action>) {
    ui.separator();
    if ui.add_enabled(!envelope.is_empty(), egui::Button::new("Clear gain envelope")).clicked() {
        actions.push(set(clip, &[]));
        ui.close();
    }
    grid_menu(ui);
}

/// The gain envelope's grid, its own rather than the Arrangement's: an index into the fixed
/// values (`FIXED_VALUES`), 1/16 until one is picked, kept for every clip until the app quits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EnvelopeGrid(usize);

fn grid_key() -> Id {
    Id::unique("clip-envelope-grid")
}

/// The envelope grid's index into the fixed values.
pub fn grid(ctx: &egui::Context) -> usize {
    ctx.data(|d| d.get_temp::<EnvelopeGrid>(grid_key()))
        .map_or(crate::detail::piano_roll_grid::FIXED_SIXTEENTH, |g| g.0)
}

/// The envelope grid's values: the fixed grid's, and 1/64 for the finest edits of a vocal. Finer
/// than that is Free move: at any zoom a point can be placed, 1/128 is about two pixels apart.
const GRID_VALUES: [&str; 10] = ["8 bars", "4 bars", "2 bars", "1 bar", "1/2", "1/4", "1/8", "1/16", "1/32", "1/64"];

/// The envelope grid's step in beats, at `bar_beats` a bar.
pub fn grid_beats(ctx: &egui::Context, bar_beats: f64) -> f64 {
    match grid(ctx) {
        9 => 1.0 / 16.0,
        index => crate::detail::piano_roll_grid::fixed_value_beats(index, bar_beats),
    }
}

/// Envelope grid in a clip's menu: the values, the one in use checked, and Free move. A pick
/// holds for every clip's envelope from then on, until the app quits.
fn grid_menu(ui: &mut Ui) {
    let current = grid(ui.ctx());
    let free = free_move(ui.ctx());
    if ui.selectable_label(free, "Free move").clicked() {
        set_free_move(ui.ctx(), !free);
        ui.close();
    }
    ui.menu_button("Envelope grid", |ui| {
        for (index, value) in GRID_VALUES.iter().enumerate() {
            let name = value.replace(" bars", " Bars").replace(" bar", " Bar");
            if ui.selectable_label(index == current, name).clicked() {
                ui.data_mut(|d| d.insert_temp(grid_key(), EnvelopeGrid(index)));
                ui.close();
            }
        }
    });
}

/// The Edit gain envelope button at `rect` (left of the clip's name), lit while `on`. Returns
/// whether it was clicked.
pub fn edit_button(ui: &Ui, painter: &egui::Painter, rect: Rect, clip: ClipId, name: &str, on: bool) -> bool {
    let response = ui
        .interact(rect, Id::unique(("clip-envelope-edit", clip.0)), Sense::click())
        .on_hover_text("Edit gain envelope (Alt+E)")
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, format!("Edit gain envelope of {name}"))
    });
    let ink = if on { theme::accent() } else { theme::text_secondary() };
    if on || response.hovered() {
        painter.rect_filled(rect, 2.0, ink.gamma_multiply(0.18));
    }
    // A line with a peak: an envelope.
    let r = rect.shrink(3.0);
    let line = vec![
        Pos2::new(r.left(), r.center().y + 1.0),
        Pos2::new(r.left() + r.width() * 0.35, r.center().y + 1.0),
        Pos2::new(r.center().x + 1.0, r.top()),
        Pos2::new(r.right() - r.width() * 0.2, r.bottom()),
        Pos2::new(r.right(), r.bottom()),
    ];
    painter.add(egui::Shape::line(line, Stroke::new(1.3, ink)));
    response.clicked()
}

/// The Free move button at `rect` (under the clip's name while its envelope is edited): lit while
/// points move freely, off while they snap to the dB grid. Returns whether it was clicked.
pub fn free_button(ui: &Ui, painter: &egui::Painter, rect: Rect, clip: ClipId, free: bool) -> bool {
    let response = ui
        .interact(rect, Id::unique(("clip-envelope-free", clip.0)), Sense::click())
        .on_hover_text(if free { "Free move: points move freely" } else { "Free move: points snap to the dB grid" })
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, free, "Free move"));
    let ink = if free { theme::accent() } else { theme::text_secondary() };
    painter.rect_filled(rect, 2.0, crate::kit::style_class::surface_or(theme::panel_background()).gamma_multiply(0.85));
    if free || response.hovered() {
        painter.rect_filled(rect, 2.0, ink.gamma_multiply(0.18));
    }
    let r = rect.shrink(3.0);
    if free {
        // Arrows up and down: moves anywhere.
        let c = r.center();
        let stroke = Stroke::new(1.3, ink);
        painter.line_segment([Pos2::new(c.x, r.top()), Pos2::new(c.x, r.bottom())], stroke);
        for (y, dy) in [(r.top(), 2.5), (r.bottom(), -2.5)] {
            painter.line_segment([Pos2::new(c.x, y), Pos2::new(c.x - 2.5, y + dy)], stroke);
            painter.line_segment([Pos2::new(c.x, y), Pos2::new(c.x + 2.5, y + dy)], stroke);
        }
    } else {
        // Rungs: steps of the grid.
        for k in 0..3 {
            let y = r.top() + r.height() * (k as f32 + 0.5) / 3.0;
            painter.hline(r.x_range(), y, Stroke::new(1.0, ink));
        }
    }
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_db_is_the_middle_and_the_first_db_of_a_cut_are_wide() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 100.0));
        assert_eq!(y_of(rect, 0.0, 6.0), 50.0);
        assert_eq!(y_of(rect, 6.0, 6.0), 0.0);
        assert_eq!(y_of(rect, 3.0, 6.0), 25.0, "even in dB above the middle");
        assert_eq!(y_of(rect, -70.0, 6.0), 100.0);
        // Below it by gain: a 3 dB cut is near a third of the way down, 6 dB half of it.
        assert!((y_of(rect, -3.0, 6.0) - 64.6).abs() < 0.1, "{}", y_of(rect, -3.0, 6.0));
        assert!((y_of(rect, -6.0, 6.0) - 75.0).abs() < 0.1, "{}", y_of(rect, -6.0, 6.0));
        assert!(y_of(rect, -1.0, 6.0) - 50.0 > 5.0, "a dB is over five points of a 100-point clip");
        for db in [-0.5, -1.0, -6.0, -24.0, -60.0, 2.5] {
            assert!((db_at(rect, y_of(rect, db, 6.0), 6.0) - db).abs() < 1e-3, "{db}");
        }
        assert_eq!(db_at(rect, -10.0, 6.0), 6.0, "kept below the top");
        assert_eq!(db_at(rect, 120.0, 6.0), -70.0, "and above the floor");
    }

    #[test]
    fn the_scale_reaches_a_point_above_the_ceiling() {
        let points = [GainPoint::new(0.0, 12.0)];
        assert_eq!(top(6.0, &points), 12.0);
        assert_eq!(top(6.0, &[]), 6.0);
        assert_eq!(top(24.0, &points), 24.0);
    }

    #[test]
    fn points_snap_to_the_grid_unless_free() {
        assert_eq!(snapped(-3.4, 1.0, false), -3.0);
        assert_eq!(snapped(-3.6, 0.5, false), -3.5);
        assert_eq!(snapped(-3.44, 1.0, true), -3.4);
        assert_eq!(db_text(-3.0), "\u{2212}3 dB");
        assert_eq!(db_text(1.5), "+1.5 dB");
        assert_eq!(db_text(0.04), "0 dB");
    }

    #[test]
    fn a_point_added_on_a_bent_segment_keeps_its_shape() {
        let first = GainPoint { curve: Curve::bend(0.8), ..GainPoint::new(0.0, 0.0) };
        let points = [first, GainPoint::new(4.0, -12.0)];
        let (more, at) = with_point(&points, 1.0, envelope_db_at(&points, 1.0));
        assert_eq!(at, 1);
        for beat in [0.5, 1.5, 3.0] {
            assert!((envelope_db_at(&more, beat) - envelope_db_at(&points, beat)).abs() < 1e-6, "{beat}");
        }
        let action = set(ClipId(1), &more);
        assert_eq!(action.params["curves"].as_array().map(Vec::len), Some(3));
        assert!(set(ClipId(1), &[GainPoint::new(0.0, 0.0)]).params.get("curves").is_none(), "straight: none sent");
    }

    #[test]
    fn a_segment_moves_up_and_down_as_one_and_the_whole_line_with_it() {
        let points = [GainPoint::new(0.0, 0.0), GainPoint::new(2.0, -3.0), GainPoint::new(4.0, 2.0)];
        assert_eq!(segment_at(&points, 1.0), vec![0, 1]);
        assert_eq!(segment_at(&points, 5.0), vec![2], "after the last point: that point alone");
        assert_eq!(segment_at(&[], 1.0), Vec::<usize>::new());
        let db = |p: Vec<GainPoint>| p.iter().map(|p| p.db).collect::<Vec<_>>();
        // Ctrl+Shift: the segment's two points; Shift: all of them.
        assert_eq!(db(lifted(&points, Some(&[0, 1]), -2.0, 6.0)), vec![-2.0, -5.0, 2.0]);
        assert_eq!(db(lifted(&points, None, 3.0, 6.0)), vec![3.0, 0.0, 5.0]);
        // Kept below the ceiling and above −70 dB.
        assert_eq!(db(lifted(&points, None, 5.0, 6.0)), vec![5.0, 2.0, 6.0]);
        assert_eq!(db(lifted(&points, None, -80.0, 6.0)), vec![-70.0, -70.0, -70.0]);
    }

    #[test]
    fn a_dragged_point_passes_the_points_it_is_dragged_over() {
        let at = |beats: &[f64]| beats.iter().map(|&b| GainPoint::new(b, 0.0)).collect::<Vec<_>>();
        let beats = |points: &[GainPoint]| points.iter().map(|p| p.beat).collect::<Vec<_>>();
        // The last point dragged left past the one before it.
        let mut points = at(&[0.0, 2.0, 4.0]);
        let index = passed(&mut points, 2, 1.0, -3.0);
        assert_eq!((index, beats(&points)), (1, vec![0.0, 1.0, 2.0]));
        assert_eq!(points[1].db, -3.0);
        // The first dragged right past both.
        let mut points = at(&[0.0, 2.0, 4.0]);
        let index = passed(&mut points, 0, 5.0, 0.0);
        assert_eq!((index, beats(&points)), (2, vec![2.0, 4.0, 5.0]));
        // Onto another's beat it stays on the side it came from.
        let mut points = at(&[0.0, 2.0, 4.0]);
        assert_eq!(passed(&mut points, 2, 2.0, 0.0), 2, "from the right");
        let mut points = at(&[0.0, 2.0, 4.0]);
        assert_eq!(passed(&mut points, 0, 2.0, 0.0), 0, "from the left");
    }

    #[test]
    fn a_point_is_placed_once_or_on_every_pass_of_the_loop() {
        let once = Mapping { offset: 1.0, loop_range: None, rate: 1.0 };
        assert_eq!(once.places(3.0, 4.0), vec![2.0]);
        assert!(once.places(0.5, 4.0).is_empty(), "before the start marker");
        let looped = Mapping { offset: 0.0, loop_range: Some((0.0, 2.0)), rate: 1.0 };
        assert_eq!(looped.places(0.5, 6.0), vec![0.5, 2.5, 4.5]);
        assert_eq!(looped.content(4.5), 0.5);
        // An unwarped clip an octave up plays its content twice as fast.
        let up = Mapping { offset: 0.0, loop_range: None, rate: 2.0 };
        assert_eq!(up.places(4.0, 4.0), vec![2.0]);
    }
}
