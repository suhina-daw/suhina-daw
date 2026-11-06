// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An audio clip's gain envelope on its waveform in the Arrangement (`docs/spec/clip-panel.md` ›
//! Gain envelope, ADR 0035).
//!
//! 0 dB, the sample as it is, runs across the middle of the clip's body; gain goes up to the
//! ceiling (Settings › Arrangement › Gain envelope) at its top and down to −70 dB at its bottom,
//! each half even in dB. With Show gain envelope on, a clip with points shows its line. Edit gain
//! envelope (Alt+E, or the button left of the clip's name) opens one clip for editing: a click on
//! its body adds a point, a drag moves one (snapped to the dB grid, or free with Free move), a
//! double-click on a point or Delete over it removes it; each is one `clip.setEnvelope`. The
//! waveform is drawn through the envelope, so it shows what is heard as a point is dragged.

use egui::{Id, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{ClipId, GAIN_POINT_RANGE, GainPoint, db_to_gain, envelope_db_at};

use super::timeline::AudioView;
use crate::kit::theme;
use crate::kit::widgets::Action;

/// The side of a point's square, and of the area that takes the pointer around it.
const POINT: f32 = 6.0;
const POINT_HIT: f32 = 11.0;
/// Most passes of a loop whose points are drawn; a long loop of a short stretch would draw
/// thousands of squares nobody can tell apart.
const MAX_PASSES: usize = 256;
/// The line is drawn as a point every this many points across.
const STEP: f32 = 2.0;
/// Grid lines closer than this, in points, are left out (every other one, and so on).
const GRID_GAP: f32 = 5.0;
/// How near 0 dB, in points, a dragged point is pulled onto it.
const ZERO_PULL: f32 = 5.0;
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
}

impl Default for EnvelopeSettings {
    fn default() -> Self {
        Self { show: true, ceiling: 6.0, step: 1.0, free: false }
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

/// The window-wide key of the clip whose envelope is being edited.
fn editing_key() -> Id {
    Id::unique("clip-envelope-editing")
}

/// The arrangement clip whose gain envelope is being edited, if one is.
pub fn editing(ctx: &egui::Context) -> Option<ClipId> {
    ctx.data(|d| d.get_temp::<ClipId>(editing_key()))
}

/// Opens `clip`'s envelope for editing, or closes it.
pub fn set_editing(ctx: &egui::Context, clip: Option<ClipId>) {
    ctx.data_mut(|d| {
        if let Some(clip) = clip {
            d.insert_temp(editing_key(), clip);
        } else {
            d.remove::<ClipId>(editing_key());
        }
    });
}

/// Edit gain envelope on `clip`, or off when it is on.
pub fn toggle_editing(ctx: &egui::Context, clip: ClipId) {
    let on = editing(ctx) == Some(clip);
    set_editing(ctx, (!on).then_some(clip));
}

/// The top of the scale for `points`: the ceiling, or higher when a point is above it (set before
/// the ceiling was lowered), so the line is drawn where it is heard.
pub fn top(ceiling: f64, points: &[GainPoint]) -> f64 {
    let highest = points.iter().map(|p| p.db).fold(f64::NEG_INFINITY, f64::max);
    ceiling.max(highest).clamp(0.5, *GAIN_POINT_RANGE.end())
}

/// The height of `db` in `rect`: 0 dB across the middle, `top` at the top and −70 dB at the
/// bottom, each half even in dB.
pub fn y_of(rect: Rect, db: f64, top: f64) -> f32 {
    let low = *GAIN_POINT_RANGE.start();
    let middle = rect.center().y;
    let half = rect.height() / 2.0;
    if db >= 0.0 {
        middle - (db.min(top) / top.max(1e-9)) as f32 * half
    } else {
        middle + (db.max(low) / low) as f32 * half
    }
}

/// The dB at height `y` in `rect`, the inverse of [`y_of`], kept between −70 dB and `top`.
pub fn db_at(rect: Rect, y: f32, top: f64) -> f64 {
    let low = *GAIN_POINT_RANGE.start();
    let half = rect.height() / 2.0;
    if half <= 0.0 {
        return 0.0;
    }
    let t = f64::from((rect.center().y - y) / half).clamp(-1.0, 1.0);
    if t >= 0.0 { t * top } else { -t * low }
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
    Action::new("clip.setEnvelope", json!({ "clip": clip.0, "points": flat(points) }))
}

/// The dB a point dragged to `pointer` gets, `before` being where it was: on the grid (or free),
/// below the ceiling, a tenth of the way with Shift; near the middle, 0 dB itself.
fn dragged_db(ui: &Ui, rect: Rect, pointer: Pos2, before: f64, top: f64) -> f64 {
    let settings = settings(ui.ctx());
    let target = db_at(rect, pointer.y, top);
    let fine = ui.input(|i| i.modifiers.shift);
    // The sample's own level is pulled to, so the line goes back to it exactly.
    if !fine && (pointer.y - y_of(rect, 0.0, top)).abs() <= ZERO_PULL {
        return 0.0;
    }
    let db = if fine { before + (target - before) * 0.1 } else { target };
    // A point already above the ceiling may stay there, never go higher.
    let ceiling = settings.ceiling.max(before);
    snapped(db, settings.step, fine || free_move(ui.ctx())).clamp(*GAIN_POINT_RANGE.start(), ceiling)
}

/// Where a clip's envelope is drawn: its body `rect` as drawn now (an edge being dragged
/// included), the x of the clip's own start, where its beats are counted from, and the width of
/// a beat, so the line goes on with the passes of a loop as an edge is dragged out.
#[derive(Debug, Clone, Copy)]
pub struct Place {
    pub rect: Rect,
    pub origin: f32,
    pub beat_width: f32,
}

/// Draws `view`'s envelope over the clip at `place`. While `edit` is on, the dB grid shows, the
/// body takes the pointer (a click adds a point, a drag adds one and moves it) and the points can
/// be dragged; `snap` puts a beat from the clip's start on the timeline's grid, unless Free move
/// is on or Shift held. What changes is pushed to `actions`. The points' squares are named
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
    let Place { rect, origin, beat_width } = place;
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
    let free = free_move(ui.ctx()) || ui.input(|i| i.modifiers.shift);
    let placed_at = |x: f32| {
        let elapsed = elapsed_at(x);
        if free { elapsed } else { snap(elapsed).clamp(0.0, length) }
    };
    let ink = theme::accent();
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
        let width = if edit { 1.5 } else { 1.0 };
        let stroke = if edit { ink } else { ink.gamma_multiply(0.7) };
        painter.add(egui::Shape::line(line, Stroke::new(width, stroke)));
    }
    if !edit {
        return;
    }

    // The body: a click adds a point, a drag adds one and moves it.
    let body = ui
        .interact(visible, Id::unique(("clip-envelope-body", clip.0)), Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    body.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, true, format!("Gain envelope of {name}")));
    let add_at = |pointer: Pos2| {
        let beat = mapping.content(placed_at(pointer.x));
        let db = dragged_db(ui, rect, pointer, 0.0, top);
        let mut more = points.clone();
        let at = more.partition_point(|p| p.beat <= beat);
        more.insert(at, GainPoint { beat, db });
        (more, at)
    };
    if body.clicked()
        && let Some(pointer) = body.interact_pointer_pos()
    {
        actions.push(set(clip, &add_at(pointer).0));
        return;
    }
    if body.drag_started_by(egui::PointerButton::Primary)
        && let Some(pointer) = body.interact_pointer_pos()
    {
        let (more, index) = add_at(pointer);
        ui.data_mut(|d| d.insert_temp(draft_key(), Draft { clip, index, points: more }));
    }
    let mut dragging = None;
    if body.dragged_by(egui::PointerButton::Primary) {
        dragging = draft(ui.ctx(), clip).map(|d| d.index);
    }

    // The points, on every pass they play.
    let mut removed = None;
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
            let square = Rect::from_center_size(center, Vec2::splat(if hovered { POINT + 2.0 } else { POINT }));
            painter.rect_filled(square, 0.0, if hovered { ink } else { theme::panel_background() });
            painter.rect_stroke(square, 0.0, Stroke::new(1.0, ink), StrokeKind::Inside);
            let delete_key = response.hovered() && ui.input(|i| i.key_pressed(egui::Key::Delete));
            if response.double_clicked() || delete_key {
                removed = Some(index);
                continue;
            }
            if response.drag_started_by(egui::PointerButton::Primary) {
                ui.data_mut(|d| d.insert_temp(draft_key(), Draft { clip, index, points: points.clone() }));
            }
            if response.dragged_by(egui::PointerButton::Primary) {
                dragging = Some(index);
            }
        }
    }
    // The point being dragged follows the pointer: in beat between its neighbours, so the points
    // stay in order, and in dB on the grid below the ceiling.
    if let (Some(index), Some(pointer)) = (dragging, ui.input(|i| i.pointer.latest_pos()))
        && let Some(mut moved) = draft(ui.ctx(), clip).map(|d| d.points)
    {
        let before = moved.get(index).copied().unwrap_or(GainPoint { beat: 0.0, db: 0.0 });
        let fine = ui.input(|i| i.modifiers.shift);
        let target = mapping.content(placed_at(pointer.x));
        let beat = if fine { before.beat + (target - before.beat) * 0.1 } else { target };
        let db = dragged_db(ui, rect, pointer, before.db, top);
        let low = index.checked_sub(1).and_then(|i| moved.get(i)).map_or(0.0, |p| p.beat);
        let high = moved.get(index + 1).map_or(f64::INFINITY, |p| p.beat);
        if let Some(p) = moved.get_mut(index) {
            p.beat = beat.clamp(low, high.max(low));
            p.db = db;
        }
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
    // Once the button is up, what the point moved to is set, and the draft goes: also for a drag
    // whose point stopped being drawn (its pass moved past the clip's end).
    if let Some(done) = draft(ui.ctx(), clip)
        && !ui.input(|i| i.pointer.any_down())
    {
        ui.data_mut(|d| d.remove::<Draft>(draft_key()));
        actions.push(set(clip, &done.points));
    }
    if let Some(index) = removed {
        let mut kept = points;
        if index < kept.len() {
            kept.remove(index);
        }
        ui.data_mut(|d| d.remove::<Draft>(draft_key()));
        actions.push(set(clip, &kept));
    }
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
    painter.rect_filled(rect, 2.0, theme::panel_background().gamma_multiply(0.85));
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
    fn zero_db_is_the_middle_and_each_half_is_even() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 100.0));
        assert_eq!(y_of(rect, 0.0, 6.0), 50.0);
        assert_eq!(y_of(rect, 6.0, 6.0), 0.0);
        assert_eq!(y_of(rect, 3.0, 6.0), 25.0);
        assert_eq!(y_of(rect, -70.0, 6.0), 100.0);
        assert_eq!(y_of(rect, -35.0, 6.0), 75.0);
        assert!((db_at(rect, y_of(rect, -6.0, 6.0), 6.0) + 6.0).abs() < 1e-4);
        assert!((db_at(rect, y_of(rect, 2.5, 6.0), 6.0) - 2.5).abs() < 1e-4);
        assert_eq!(db_at(rect, -10.0, 6.0), 6.0, "kept below the top");
    }

    #[test]
    fn the_scale_reaches_a_point_above_the_ceiling() {
        let points = [GainPoint { beat: 0.0, db: 12.0 }];
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
