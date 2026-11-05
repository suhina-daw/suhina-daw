// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An audio clip's gain envelope on its waveform in the Arrangement (`docs/spec/clip-panel.md` ›
//! Gain envelope, ADR 0035): drawn as a line from −70 dB at the clip's bottom to +24 dB at its top,
//! its points as small squares, while the clip panel's Envelope switch shows it. A double-click on
//! the clip adds a point, a point is dragged to move it, and a double-click on it (or Delete over
//! it) removes it; each is one `clip.setEnvelope`.

use egui::{Id, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use serde_json::json;
use suhina_engine::model::{ClipId, GAIN_POINT_RANGE, GainPoint, envelope_db_at};

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

/// The window-wide key of the clip whose envelope is shown.
fn shown_key() -> Id {
    Id::unique("clip-envelope-shown")
}

/// The arrangement clip whose gain envelope is shown for editing, if one is.
pub fn shown(ctx: &egui::Context) -> Option<ClipId> {
    ctx.data(|d| d.get_temp::<ClipId>(shown_key()))
}

/// Shows `clip`'s envelope for editing, or none.
pub fn set_shown(ctx: &egui::Context, clip: Option<ClipId>) {
    ctx.data_mut(|d| {
        if let Some(clip) = clip {
            d.insert_temp(shown_key(), clip);
        } else {
            d.remove::<ClipId>(shown_key());
        }
    });
}

/// The height of `db` in `rect`: −70 dB at its bottom, +24 dB at its top.
pub fn y_of(rect: Rect, db: f64) -> f32 {
    let (low, high) = (*GAIN_POINT_RANGE.start(), *GAIN_POINT_RANGE.end());
    let t = ((db.clamp(low, high) - low) / (high - low)) as f32;
    rect.bottom() - t * rect.height()
}

/// The dB at height `y` in `rect`, the inverse of [`y_of`], kept in the range.
pub fn db_at(rect: Rect, y: f32) -> f64 {
    let (low, high) = (*GAIN_POINT_RANGE.start(), *GAIN_POINT_RANGE.end());
    let t = if rect.height() > 0.0 { f64::from((rect.bottom() - y) / rect.height()) } else { 0.0 };
    (low + t.clamp(0.0, 1.0) * (high - low)).clamp(low, high)
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

/// The points as `clip.setEnvelope` takes them: `[beat, dB, …]`.
fn flat(points: &[GainPoint]) -> Vec<f64> {
    points.iter().flat_map(|p| [p.beat, p.db]).collect()
}

fn set(clip: ClipId, points: &[GainPoint]) -> Action {
    Action::new("clip.setEnvelope", json!({ "clip": clip.0, "points": flat(points) }))
}

/// Draws `view`'s envelope over the clip in `rect` (`length` beats from its start) and takes the
/// pointer on its points; `added` is where a double-click on the clip's body was, to add a point.
/// What changes is pushed to `actions`. The points' squares are named "Envelope point N of NAME".
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    length: f64,
    clip: ClipId,
    name: &str,
    view: &AudioView,
    added: Option<Pos2>,
    actions: &mut Vec<Action>,
) {
    if rect.width() <= 1.0 || rect.height() <= 4.0 || length <= 0.0 {
        return;
    }
    let mapping = Mapping::of(view);
    let draft = ui.data(|d| d.get_temp::<Draft>(draft_key())).filter(|d| d.clip == clip);
    let points = draft.as_ref().map_or_else(|| view.envelope.clone(), |d| d.points.clone());
    let x_of = |elapsed: f64| rect.left() + (elapsed / length) as f32 * rect.width();
    let elapsed_at = |x: f32| f64::from((x - rect.left()) / rect.width()) * length;
    let ink = theme::accent();

    // The 0 dB line, faint, and the envelope over it.
    let zero = y_of(rect, 0.0);
    painter.hline(rect.x_range(), zero, Stroke::new(1.0, ink.gamma_multiply(0.25)));
    let visible = painter.clip_rect().intersect(rect);
    let mut line = Vec::new();
    let mut x = visible.left();
    while x <= visible.right() {
        let db = envelope_db_at(&points, mapping.content(elapsed_at(x)));
        line.push(Pos2::new(x, y_of(rect, db)));
        x += STEP;
    }
    if line.len() >= 2 {
        painter.add(egui::Shape::line(line, Stroke::new(1.5, ink)));
    }

    // The points, on every pass they play.
    let mut removed = None;
    for (index, point) in points.iter().enumerate() {
        for (pass, at) in mapping.places(point.beat, length).into_iter().enumerate() {
            let center = Pos2::new(x_of(at), y_of(rect, point.db));
            let hit = Rect::from_center_size(center, Vec2::splat(POINT_HIT));
            if !visible.intersects(hit) {
                continue;
            }
            let id = Id::unique(("clip-envelope-point", clip.0, index, pass));
            let response = ui.interact(hit, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::Grab);
            let label = format!("Envelope point {} of {name}", index + 1);
            response.widget_info(|| {
                let mut info = egui::WidgetInfo::labeled(egui::Role::Slider, true, &label);
                info.value = Some(point.db);
                info
            });
            let hovered = response.hovered() || response.dragged();
            let square = Rect::from_center_size(center, Vec2::splat(if hovered { POINT + 2.0 } else { POINT }));
            painter.rect_filled(square, 0.0, if hovered { ink } else { theme::panel_background() });
            painter.rect_stroke(square, 0.0, Stroke::new(1.0, ink), StrokeKind::Inside);
            let delete_key = hovered && ui.input(|i| i.key_pressed(egui::Key::Delete));
            if response.double_clicked() || delete_key {
                removed = Some(index);
                continue;
            }
            if response.dragged_by(egui::PointerButton::Primary)
                && let Some(pointer) = response.interact_pointer_pos()
            {
                let fine = ui.input(|i| i.modifiers.shift);
                let mut moved = points.clone();
                let before = moved.get(index).copied().unwrap_or(*point);
                let target_beat = mapping.content(elapsed_at(pointer.x).clamp(0.0, length));
                let target_db = db_at(rect, pointer.y);
                let (beat, db) = if fine {
                    (before.beat + (target_beat - before.beat) * 0.1, before.db + (target_db - before.db) * 0.1)
                } else {
                    (target_beat, target_db)
                };
                // Kept between its neighbours, so the points stay in order.
                let low = index.checked_sub(1).and_then(|i| moved.get(i)).map_or(0.0, |p| p.beat);
                let high = moved.get(index + 1).map_or(f64::INFINITY, |p| p.beat);
                if let Some(p) = moved.get_mut(index) {
                    p.beat = beat.clamp(low, high.max(low));
                    p.db = (db * 10.0).round() / 10.0;
                }
                ui.data_mut(|d| d.insert_temp(draft_key(), Draft { clip, index, points: moved }));
            }
            if response.drag_stopped_by(egui::PointerButton::Primary) {
                if let Some(done) = ui.data(|d| d.get_temp::<Draft>(draft_key())).filter(|d| d.clip == clip) {
                    actions.push(set(clip, &done.points));
                }
                ui.data_mut(|d| d.remove::<Draft>(draft_key()));
            }
        }
    }
    // A drag whose point stopped being drawn (its pass moved past the clip's end) never sees its
    // drag stop: once the button is up, what it moved to is set, and the draft goes.
    if let Some(done) = ui.data(|d| d.get_temp::<Draft>(draft_key())).filter(|d| d.clip == clip)
        && !ui.input(|i| i.pointer.any_down())
    {
        ui.data_mut(|d| d.remove::<Draft>(draft_key()));
        actions.push(set(clip, &done.points));
    }
    if let Some(index) = removed {
        let mut kept = points.clone();
        if index < kept.len() {
            kept.remove(index);
        }
        ui.data_mut(|d| d.remove::<Draft>(draft_key()));
        actions.push(set(clip, &kept));
        return;
    }
    if let Some(pointer) = added.filter(|p| rect.contains(*p)) {
        let beat = mapping.content(elapsed_at(pointer.x));
        let db = (db_at(rect, pointer.y) * 10.0).round() / 10.0;
        let mut more = points;
        let at = more.partition_point(|p| p.beat <= beat);
        more.insert(at, GainPoint { beat, db });
        actions.push(set(clip, &more));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decibels_map_to_heights_and_back() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 94.0));
        assert_eq!(y_of(rect, 24.0), 0.0);
        assert_eq!(y_of(rect, -70.0), 94.0);
        assert!((db_at(rect, y_of(rect, -6.0)) + 6.0).abs() < 1e-4);
        assert_eq!(db_at(rect, -10.0), 24.0, "kept in the range");
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
