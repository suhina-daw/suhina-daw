// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Small drawing helpers and the controls the views share: text that fits a cell, the value
//! field drag, the knob.

use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align2, Color32, FontId, Galley, Id, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType,
};
use serde_json::Value;

use crate::kit::{motion, theme};

/// The arrow of a row that opens: pointing right while closed, down while open.
pub fn chevron(painter: &Painter, center: Pos2, open: bool, color: Color32) {
    let points = if open {
        vec![center + Vec2::new(-3.0, -1.5), center + Vec2::new(0.0, 1.5), center + Vec2::new(3.0, -1.5)]
    } else {
        vec![center + Vec2::new(-1.5, -3.0), center + Vec2::new(1.5, 0.0), center + Vec2::new(-1.5, 3.0)]
    };
    painter.add(egui::Shape::line(points, Stroke::new(1.1, color)));
}

/// Where the pointer is, when it is over `rect` and no layer is on top of `ui`'s there (a
/// dialog, a menu, a tooltip): what a view that reads the wheel, a pinch or a press itself may
/// act on, so they reach only the dialog while one is open.
pub fn pointer_over(ui: &Ui, rect: Rect) -> Option<Pos2> {
    if ui.rect_contains_pointer(rect) { ui.ctx().pointer_hover_pos() } else { None }
}

/// Normalized device parameter range traversed by a vertical drag.
pub(crate) const PARAMETER_DRAG_PIXELS: f64 = 400.;

/// A command for the session to run, made by a view: the same ids the CLI and MCP use.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub id: &'static str,
    pub params: Value,
}

impl Action {
    pub fn new(id: &'static str, params: Value) -> Self {
        Self { id, params }
    }

    /// An action for a command named at run time (from the keymap); unknown ids run nothing.
    pub fn new_owned(id: &str, params: Value) -> Self {
        let known = suhina_session::commands()
            .iter()
            .map(|c| c.id)
            .chain(suhina_session::project_commands().iter().map(|c| c.id))
            .find(|known| *known == id)
            .unwrap_or("command.list");
        Self { id: known, params }
    }
}

/// One line of text cut to `max_width` with an ellipsis.
pub fn fitted(painter: &Painter, text: &str, font: FontId, color: Color32, max_width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap = TextWrapping::truncate_at_width(max_width.max(0.0));
    painter.layout_job(job)
}

/// How wide `text` is in `font`.
pub fn text_width(ui: &Ui, text: &str, font: FontId) -> f32 {
    ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, Color32::WHITE).size().x)
}

/// Draws one line of text in `rect`, cut to its width, aligned horizontally by `align` and
/// centred vertically.
pub fn text_in(painter: &Painter, rect: Rect, align: Align2, text: &str, font: FontId, color: Color32) {
    let galley = fitted(painter, text, font, color, rect.width());
    let pos = align.align_size_within_rect(galley.size(), rect).min;
    painter.galley(pos, galley, color);
}

/// Makes `rect` clickable, with a label for screen readers and tests.
pub fn button_area(ui: &Ui, rect: Rect, id: egui::Id, label: impl ToString) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    let label = label.to_string();
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
    response
}

/// How hovered (or dragged) a control is, 0 … 1, fading in and out.
pub fn hover(ui: &Ui, response: &Response) -> f32 {
    motion::toward(ui.ctx(), response.id.with("hover"), response.hovered() || response.dragged(), motion::HOVER)
}

/// Fill and text of a track-on toggle: light when on, dark when off, a step brighter hovered.
pub fn toggle_colors(ui: &Ui, response: &Response, on: bool) -> (Color32, Color32) {
    let hover = hover(ui, response);
    let lit = motion::toward(ui.ctx(), response.id.with("on"), on, motion::VALUE);
    let off_fill = motion::mix(theme::control_background(), theme::control_hover(), hover);
    let off_text = motion::mix(theme::text_hint(), theme::text_secondary(), hover);
    let on_fill = motion::mix(theme::text_value(), theme::text_primary(), hover);
    (motion::mix(off_fill, on_fill, lit), motion::mix(off_text, theme::window_background(), lit))
}

/// A track's solo button: draws it in `rect` and returns the command a click asks for. A click
/// solos the tracks named in `tracks` alone — one track, or the whole selection the clicked one
/// is part of; with Ctrl (Cmd) held they are added to the tracks already soloed.
pub fn solo_button(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    id: egui::Id,
    tracks: &[u32],
    name: &str,
    solo: bool,
) -> Option<Action> {
    let label = if solo { format!("End solo on {name}") } else { format!("Solo {name}") };
    let response = button_area(ui, rect, id, label);
    let lit = motion::toward(ui.ctx(), response.id.with("on"), solo, motion::VALUE);
    let off = motion::mix(theme::control_background(), theme::control_hover(), hover(ui, &response));
    painter.rect_filled(rect, 0.0, motion::mix(off, theme::accent(), lit));
    let text = motion::mix(theme::text_secondary(), theme::window_background(), lit);
    text_in(painter, rect, Align2::CENTER_CENTER, "S", theme::ui_font(theme::TEXT_SMALL), text);
    if !response.clicked() {
        return None;
    }
    let adding = ui.input(|i| i.modifiers.command);
    Some(Action::new(
        "track.setSolo",
        serde_json::json!({ "tracks": tracks, "solo": !solo, "exclusive": !solo && !adding }),
    ))
}

/// A track's cue button (headphones; a C read as part of the pan field beside it, which shows C
/// for centre, #744): draws it in `rect` and returns the command a click asks for,
/// which turns the track's cue on or off, so it is also heard on the cue output (headphones).
pub fn cue_button(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    id: egui::Id,
    track: u32,
    name: &str,
    cue: bool,
) -> Option<Action> {
    let label = if cue { format!("Stop cueing {name}") } else { format!("Cue {name} on the headphones") };
    let response = button_area(ui, rect, id, label);
    let (fill, text) = toggle_colors(ui, &response, cue);
    painter.rect_filled(rect, 0.0, fill);
    headphones(painter, rect, text);
    response.clicked().then(|| Action::new("track.setCue", serde_json::json!({ "track": track, "cue": !cue })))
}

/// A pair of headphones in the middle of `rect`, in `color`: the band an arc over the top, a cup
/// at each of its ends. Sized from the rectangle's height, so it reads at a button's 15 points.
pub fn headphones(painter: &Painter, rect: Rect, color: Color32) {
    let size = rect.height().min(rect.width()) * 0.62;
    let radius = size * 0.45;
    let center = rect.center() + egui::vec2(0.0, size * 0.12);
    let band: Vec<Pos2> = (0..=12)
        .map(|i| {
            let angle = std::f32::consts::PI * (1.0 + i as f32 / 12.0);
            center + egui::vec2(angle.cos() * radius, angle.sin() * radius)
        })
        .collect();
    painter.add(egui::Shape::line(band, egui::Stroke::new((size * 0.13).max(1.0), color)));
    let cup = egui::vec2(size * 0.26, size * 0.42);
    for side in [-1.0f32, 1.0] {
        let at = center + egui::vec2(side * radius, cup.y * 0.35);
        painter.rect_filled(Rect::from_center_size(at, cup), size * 0.06, color);
    }
}

/// A track's arm button (components › Arm): a ring off, a filled red dot on its dark red
/// background when armed. Returns the command a click asks for: a click arms the track alone;
/// with Ctrl (Cmd) held it is added to the tracks armed already. Arming a track that plays
/// notes enables recording and, in Auto monitoring, hears its MIDI input.
pub fn arm_button(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    id: egui::Id,
    track: u32,
    name: &str,
    armed: bool,
) -> Option<Action> {
    let label = if armed { format!("Disarm {name}") } else { format!("Arm {name} for recording") };
    let response = button_area(ui, rect, id, label).on_hover_text("Make this track ready to record");
    let lit = motion::toward(ui.ctx(), response.id.with("on"), armed, motion::VALUE);
    let off = motion::mix(theme::control_background(), theme::control_hover(), hover(ui, &response));
    painter.rect_filled(rect, 0.0, motion::mix(off, theme::recording_background(), lit));
    let radius = (rect.height().min(rect.width()) * 0.22).clamp(2.5, 4.0);
    let ring = motion::mix(theme::text_secondary(), theme::recording(), lit);
    painter.circle_stroke(rect.center(), radius, Stroke::new(1.2, ring));
    if lit > 0.0 {
        painter.circle_filled(rect.center(), radius, theme::recording().gamma_multiply(lit));
    }
    if !response.clicked() {
        return None;
    }
    let adding = ui.input(|i| i.modifiers.command);
    Some(Action::new(
        "track.setArm",
        serde_json::json!({ "track": track, "armed": !armed, "exclusive": !armed && !adding }),
    ))
}

/// A dropdown field (components › Dropdown): the current choice and a chevron; the caller opens
/// its menu from `response`.
pub fn dropdown(ui: &Ui, painter: &Painter, rect: Rect, response: &Response, text: &str) {
    let fill = motion::mix(theme::control_background(), theme::control_hover(), hover(ui, response));
    painter.rect_filled(rect, 0.0, fill);
    let inner = rect.shrink2(Vec2::new(4.0, 0.0));
    let chevron = Rect::from_center_size(Pos2::new(inner.right() - 3.0, rect.center().y), Vec2::new(6.0, 4.0));
    painter.add(egui::Shape::convex_polygon(
        vec![chevron.left_top(), chevron.right_top(), chevron.center_bottom()],
        theme::text_hint(),
        Stroke::NONE,
    ));
    let label = Rect::from_min_max(inner.min, Pos2::new(chevron.left() - 4.0, inner.bottom()));
    text_in(painter, label, Align2::LEFT_CENTER, text, theme::ui_font(theme::TEXT_SMALL), theme::text_value());
}

/// What a value control did this frame.
pub enum Drag {
    None,
    /// Being dragged: the value to show.
    Moving(f64),
    /// Released, reset by a double-click or moved by the wheel: the value to commit.
    Done(f64),
}

/// Whether the wheel over a value field changes its value this frame: with Alt held. Without it
/// the wheel scrolls the view under the field (`docs/spec/arrangement-editing.md`).
pub fn wheel_moves_value(ui: &Ui) -> bool {
    ui.input(|i| i.modifiers.alt)
}

/// The points one tick of a wheel scrolls, which is one step of a value control (egui reports a
/// line of a wheel as this many points on most platforms).
const WHEEL_TICK: f64 = 50.0;

/// How far the wheel turned this frame, in points, read from the events themselves rather than
/// from egui's smoothed scroll, so one tick of a wheel is one step and not a run of them.
fn wheel_points(ui: &Ui) -> f32 {
    ui.input(|i| {
        i.raw
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel { unit, delta, .. } => {
                    // Some systems turn Alt and the wheel into a sideways scroll.
                    let turned = if delta.y == 0.0 { delta.x } else { delta.y };
                    Some(match unit {
                        egui::MouseWheelUnit::Point => turned,
                        egui::MouseWheelUnit::Line => turned * WHEEL_TICK as f32,
                        egui::MouseWheelUnit::Page => turned * 400.0,
                    })
                }
                _ => None,
            })
            .sum()
    })
}

/// The value control clicked or dragged last, and the frame it was drawn in: the arrow keys and
/// typed digits go to it (#707) until a click elsewhere, Enter or Escape.
const ACTIVE_VALUE: &str = "active-value";

/// Whether a value control takes this frame's keys: an arrow, a digit, Enter, Escape or
/// Backspace while one is active. The window's shortcuts wait for this frame then, so a digit
/// typed into it does not pick a tool; Ctrl+Z and Space still reach them.
pub fn value_active(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    let active = ctx
        .data(|d| d.get_temp::<(egui::Id, u64)>(egui::Id::new(ACTIVE_VALUE)))
        .is_some_and(|(_, drawn)| pass.saturating_sub(drawn) <= 2);
    active
        && ctx.input(|i| {
            i.events.iter().any(|event| match event {
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    !modifiers.command
                        && matches!(
                            key,
                            egui::Key::ArrowUp
                                | egui::Key::ArrowDown
                                | egui::Key::Enter
                                | egui::Key::Escape
                                | egui::Key::Backspace
                        )
                }
                egui::Event::Text(text) => text.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | '.' | ',')),
                _ => false,
            })
        })
}

/// The keys an active value control takes this frame: arrow steps (up is +1), and Enter with
/// the number typed into it. Ctrl (Cmd) with a key leaves it to the window's shortcuts, as
/// [`value_active`] says.
fn value_keys(ui: &Ui, response: &Response) -> (i32, bool, Option<String>) {
    let ctx = ui.ctx();
    let key = egui::Id::new(ACTIVE_VALUE);
    let typed_id = response.id.with("typed");
    let pass = ctx.cumulative_pass_nr();
    if response.clicked() || response.drag_started_by(egui::PointerButton::Primary) {
        ctx.data_mut(|d| {
            d.insert_temp(key, (response.id, pass));
            d.remove::<String>(typed_id);
        });
    }
    let active = ctx.data(|d| d.get_temp::<(egui::Id, u64)>(key)).is_some_and(|(id, _)| id == response.id);
    if !active {
        return (0, false, None);
    }
    // A press anywhere else lets it go.
    if ui.input(|i| i.pointer.any_pressed()) && !response.hovered() {
        ctx.data_mut(|d| {
            d.remove::<(egui::Id, u64)>(key);
            d.remove::<String>(typed_id);
        });
        return (0, false, None);
    }
    ctx.data_mut(|d| d.insert_temp(key, (response.id, pass)));
    let mut typed = ctx.data(|d| d.get_temp::<String>(typed_id)).unwrap_or_default();
    let (mut steps, mut fine, mut enter, mut escape) = (0i32, false, false, false);
    ctx.input_mut(|i| {
        i.events.retain(|event| match event {
            egui::Event::Key { key: egui::Key::ArrowUp, pressed: true, modifiers, .. } if !modifiers.command => {
                steps += 1;
                fine |= modifiers.shift;
                false
            }
            egui::Event::Key { key: egui::Key::ArrowDown, pressed: true, modifiers, .. } if !modifiers.command => {
                steps -= 1;
                fine |= modifiers.shift;
                false
            }
            egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers, .. } if !modifiers.command => {
                enter = true;
                false
            }
            egui::Event::Key { key: egui::Key::Escape, pressed: true, modifiers, .. } if !modifiers.command => {
                escape = true;
                false
            }
            egui::Event::Key { key: egui::Key::Backspace, pressed: true, modifiers, .. } if !modifiers.command => {
                typed.pop();
                false
            }
            egui::Event::Text(text) if text.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | '.' | ',')) => {
                typed.push_str(text);
                false
            }
            _ => true,
        });
    });
    if escape {
        ctx.data_mut(|d| {
            d.remove::<(egui::Id, u64)>(key);
            d.remove::<String>(typed_id);
        });
        return (0, false, None);
    }
    if enter {
        ctx.data_mut(|d| {
            d.remove::<(egui::Id, u64)>(key);
            d.remove::<String>(typed_id);
        });
        return (0, fine, Some(typed));
    }
    ctx.data_mut(|d| d.insert_temp(typed_id, typed.clone()));
    // Lit while it takes the keys; what is typed shows over it.
    let painter = ui.painter_at(response.rect);
    painter.rect_stroke(response.rect, 0.0, Stroke::new(1.0, theme::accent()), egui::StrokeKind::Inside);
    if !typed.is_empty() {
        painter.rect_filled(response.rect.shrink(1.0), 0.0, theme::control_active());
        painter.text(
            response.rect.center(),
            egui::Align2::CENTER_CENTER,
            &typed,
            theme::mono_font(theme::TEXT_SMALL),
            theme::text_value(),
        );
    }
    (steps, fine, None)
}

/// A number changed by dragging vertically (up raises it), by the wheel over it and by a
/// double-click; clicked, the arrow keys step it (Shift: ten times finer) and a number typed and
/// Enter set it (#707). `per_point` is the change per point of drag and `per_tick` the change
/// per wheel tick or arrow key; Shift makes both ten times finer.
#[allow(clippy::too_many_arguments)]
pub fn drag_value(
    ui: &Ui,
    response: &Response,
    preview: &mut Option<f64>,
    value: f64,
    per_point: f64,
    per_tick: f64,
    range: (f64, f64),
    default: f64,
) -> Drag {
    // Keys to the control clicked last: the arrows step it, Enter sets what was typed.
    let (steps, fine, typed) = value_keys(ui, response);
    if let Some(typed) = typed
        && let Ok(number) = typed.replace(',', ".").trim().parse::<f64>()
        && number.is_finite()
    {
        *preview = None;
        return Drag::Done(number.clamp(range.0, range.1));
    }
    if steps != 0 {
        let from = if value.is_finite() { value } else { range.0 };
        let step = f64::from(steps) * per_tick * if fine { 0.1 } else { 1.0 };
        *preview = None;
        return Drag::Done((from + step).clamp(range.0, range.1));
    }
    // Alt and the wheel over the control move it, as a drag would; the caller commits what it is
    // given. The wheel alone scrolls the view, so scrolling past a field never changes it.
    // Scrolling up raises the value, as dragging up does; Shift too, ten times finer.
    if response.hovered() && wheel_moves_value(ui) {
        let points = wheel_points(ui);
        if points != 0.0 {
            let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
            // A positive delta moves the content down: the wheel turned up, which raises it.
            let step = f64::from(points) / WHEEL_TICK * per_tick * fine;
            *preview = None;
            return Drag::Done((value + step).clamp(range.0, range.1));
        }
    }
    if response.double_clicked() {
        *preview = None;
        return Drag::Done(default);
    }
    if response.drag_started_by(egui::PointerButton::Primary) {
        *preview = Some(if value.is_finite() { value } else { range.0 });
    }
    if response.dragged_by(egui::PointerButton::Primary) {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let start = preview.unwrap_or(value);
        let next = (start - f64::from(response.drag_delta().y) * per_point * fine).clamp(range.0, range.1);
        *preview = Some(next);
        return Drag::Moving(next);
    }
    if response.drag_stopped_by(egui::PointerButton::Primary) {
        return match preview.take() {
            Some(done) => Drag::Done(done),
            None => Drag::None,
        };
    }
    Drag::None
}

/// Makes `rect` a draggable value with a label for screen readers and tests.
pub fn value_area(ui: &Ui, rect: Rect, id: egui::Id, label: impl ToString, value: impl ToString) -> Response {
    let response = ui.interact(rect, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeVertical);
    let (label, value) = (label.to_string(), value.to_string());
    response.widget_info(|| {
        let mut info = WidgetInfo::labeled(WidgetType::DragValue, true, &label);
        info.current_text_value = Some(value.clone());
        info
    });
    response
}

/// The knob: a small circle with a pointer from the centre, −135° … +135° of travel for
/// `position` 0 … 1 (components › Mixer strip).
pub fn knob(painter: &Painter, center: Pos2, size: f32, position: f32, pointer: Color32) {
    let radius = size * 9.0 / 22.0;
    painter.circle_filled(center, radius, theme::control_background());
    painter.circle_stroke(center, radius, theme::hairline(theme::text_hint()));
    let angle = (-135.0 + 270.0 * position.clamp(0.0, 1.0)).to_radians();
    let tip = center + Vec2::new(angle.sin(), -angle.cos()) * (radius - 1.0);
    painter.line_segment([center, tip], Stroke::new(1.5, pointer));
}

/// A volume in dB as the mixer shows it.
pub fn format_db(db: f64) -> String {
    if db.is_finite() && db > -70.0 { format!("{db:.1}") } else { "-inf".into() }
}

/// A pan position as the mixer shows it: 50L … C … 50R.
pub fn format_pan(pan: f64) -> String {
    let amount = (pan * 50.0).round() as i64;
    match amount {
        0 => "C".into(),
        a if a < 0 => format!("{}L", -a),
        a => format!("{a}R"),
    }
}

pub(crate) const VOLUME_RANGE: (f64, f64) = (-70.0, 6.0);

/// Where a volume sits on a field, 0 … 1, as the volume fader places it: 0 dB at 85 %, 40 dB
/// per travel down to -18 dB, faster below (components › Value field;
/// `suhina_model::automation::volume_position`).
pub fn volume_position(db: f64) -> f32 {
    suhina_engine::model::automation::volume_position(db) as f32
}

pub(crate) fn play_glyph(painter: &egui::Painter, center: Pos2, playing: bool, color: Color32, size: f32) {
    if playing {
        painter.rect_filled(Rect::from_center_size(center, Vec2::splat(size)), 0.0, color);
    } else {
        let s = size / 7.0;
        painter.add(egui::Shape::convex_polygon(
            vec![
                center + Vec2::new(-3.0 * s, -4.0 * s),
                center + Vec2::new(4.0 * s, 0.0),
                center + Vec2::new(-3.0 * s, 4.0 * s),
            ],
            color,
            Stroke::NONE,
        ));
    }
}

pub(crate) fn button(ui: &Ui, painter: &egui::Painter, rect: Rect, text: &str, label: &str) -> bool {
    let response = button_area(ui, rect, Id::new(("dialog-button", label)), label);
    let fill = motion::mix(theme::control_background(), theme::control_hover(), hover(ui, &response));
    painter.rect_filled(rect, 0.0, fill);
    painter.rect_stroke(rect, 0.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
    let small = theme::ui_font(theme::TEXT_SMALL);
    text_in(painter, rect, Align2::CENTER_CENTER, text, small, theme::text_primary());
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_read_like_the_mixer() {
        assert_eq!(format_db(0.0), "0.0");
        assert_eq!(format_db(-6.02), "-6.0");
        assert_eq!(format_db(f64::NEG_INFINITY), "-inf");
        assert_eq!(format_db(-80.0), "-inf");
        assert_eq!(format_pan(0.0), "C");
        assert_eq!(format_pan(-0.3), "15L");
        assert_eq!(format_pan(1.0), "50R");
    }
}
