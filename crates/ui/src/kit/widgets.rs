// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Small drawing helpers and the controls the views share: text that fits a cell, the value
//! field drag, the knob.

use crate::kit::surface::Backgrounds as _;
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

/// `value` to `decimals` places, without the minus sign of a negative that rounds to zero.
pub(crate) fn fixed(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    match text.strip_prefix('-') {
        Some(rest) if !rest.bytes().any(|b| matches!(b, b'1'..=b'9')) => rest.to_string(),
        _ => text,
    }
}

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
    paint_text(painter, pos, galley, color);
}

pub fn paint_text(painter: &Painter, pos: Pos2, galley: Arc<Galley>, color: Color32) {
    super::native_css::paint_text(painter, pos, galley, color);
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

/// Shared track-button surface. Colors and focus outlines follow the active theme.
/// Geometry stays inside the caller's hit target, including compact mixer strips.
pub fn track_button_face(_ui: &Ui, painter: &Painter, rect: Rect, response: &Response, fill: Color32) {
    let _style = crate::kit::style_class::control("button", response);
    let radius = super::style_class::radius((rect.height() * 0.25).min(5.0));
    button_surface(painter, rect, response, false, fill, radius);
    if response.has_focus() || response.hovered() {
        let edge = if response.has_focus() { theme::accent() } else { theme::border_strong() };
        painter.rect_stroke(rect, radius, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
    }
}

/// CSS paint remains above the SVG body, below the control's glyph and label.
pub fn button_surface(painter: &Painter, rect: Rect, response: &Response, on: bool, fill: Color32, radius: f32) {
    let _style = super::style_class::control("button", response);
    super::native_css::mark(super::native_css::HostState::Checked, on);
    // A shape the sheet gives the button wins over the artwork Suhina ships, not over the theme's own.
    let shaped = super::style_class::computed()
        .is_some_and(|style| style.authored.background || style.authored.radius || style.authored.border_width);
    let role = super::skin::button_role();
    if !shaped || role != "button" || super::skin::has_own("button") {
        let fill = super::style_class::background(fill);
        let radius = super::style_class::radius(radius);
        if super::skin::button(painter, rect, super::skin::State::response(response, on), fill, radius) {
            // The artwork cast the shadows already, in its own shape.
            super::native_css::gradients_rounded(painter, rect, radius);
            return;
        }
    }
    super::native_css::surface(painter, rect, radius, fill);
}

/// A track's number, `.track-number`: its own `color`, `font-size` and `display: none`, and
/// `:muted` while the track is off.
pub fn track_number_text(painter: &Painter, rect: Rect, number: &str, ink: Color32, muted: bool) {
    let _style = super::style_class::enter("track-number");
    super::native_css::mark(super::native_css::HostState::Muted, muted);
    let computed = super::style_class::computed();
    if computed.as_ref().is_some_and(|c| !c.visible) {
        return;
    }
    let size = computed.as_ref().filter(|c| c.authored.font_size).map_or(theme::TEXT_SMALL, |c| c.font_size);
    let ink = super::style_class::foreground(ink);
    text_in(painter, rect, Align2::CENTER_CENTER, number, theme::mono_font(size), ink);
}

/// Where a knob's value goes, from `--suhina-value-position` on the control in scope: `right`
/// (beside it, the default), `left`, `top`, `bottom`, `inside` (over its middle) or `none`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValuePosition {
    Right,
    Left,
    Top,
    Bottom,
    Inside,
    None,
}

impl ValuePosition {
    /// The position the sheet gives the control in scope.
    pub fn current() -> Self {
        Self::or(Self::Right)
    }

    /// The position the sheet gives the control in scope, or `default`.
    pub fn or(default: Self) -> Self {
        match super::style_class::property("--suhina-value-position").as_deref().map(str::trim) {
            Some("right") => Self::Right,
            Some("left") => Self::Left,
            Some("top") => Self::Top,
            Some("bottom") => Self::Bottom,
            Some("inside" | "center" | "centre") => Self::Inside,
            Some("none" | "hidden") => Self::None,
            _ => default,
        }
    }
}

/// The smallest knob drawn with its value above or below it.
const MIN_STACKED_KNOB: f32 = 10.0;

/// A knob in `rect` at `position` (0 to 1) with its value `text` where the sheet puts it
/// ([`ValuePosition`], `default` where it says nothing), in `ink` unless the sheet's `color` says
/// otherwise, at its `font-size` (`font_size` where it says none).
#[allow(clippy::too_many_arguments)]
pub fn knob_with_value(
    painter: &Painter,
    rect: Rect,
    position: f32,
    pointer: Color32,
    response: Option<&Response>,
    text: &str,
    ink: Color32,
    default: ValuePosition,
    font_size: f32,
) {
    let computed = super::style_class::computed();
    let size_text = computed.as_ref().filter(|c| c.authored.font_size).map_or(font_size, |c| c.font_size);
    let ink = super::style_class::foreground(ink);
    let line = size_text + 3.0;
    let place = match ValuePosition::or(default) {
        // A row too low for a knob with its value above or below it keeps the value beside it,
        // so the knob stays big enough to see and use (a pan in a track's header).
        ValuePosition::Top | ValuePosition::Bottom if rect.height() - line < MIN_STACKED_KNOB => ValuePosition::Right,
        place => place,
    };
    let text = &formatted_value(text);
    let (knob_area, text_area, align) = match place {
        ValuePosition::Right | ValuePosition::Left => {
            let size = rect.height().min(theme::KNOB);
            if place == ValuePosition::Right {
                let knob =
                    Rect::from_min_size(Pos2::new(rect.left() + 2.0, rect.center().y - size / 2.0), Vec2::splat(size));
                (knob, Rect::from_min_max(Pos2::new(knob.right() + 4.0, rect.top()), rect.max), Align2::RIGHT_CENTER)
            } else {
                let knob = Rect::from_min_size(
                    Pos2::new(rect.right() - 2.0 - size, rect.center().y - size / 2.0),
                    Vec2::splat(size),
                );
                (knob, Rect::from_min_max(rect.min, Pos2::new(knob.left() - 4.0, rect.bottom())), Align2::LEFT_CENTER)
            }
        }
        ValuePosition::Top | ValuePosition::Bottom => {
            let size = (rect.height() - line).min(rect.width()).max(4.0);
            let (knob_y, text_rect) = if place == ValuePosition::Top {
                (rect.bottom() - size, Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + line)))
            } else {
                (rect.top(), Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - line), rect.max))
            };
            let knob = Rect::from_min_size(Pos2::new(rect.center().x - size / 2.0, knob_y), Vec2::splat(size));
            (knob, text_rect, Align2::CENTER_CENTER)
        }
        ValuePosition::Inside | ValuePosition::None => {
            let size = rect.height().min(rect.width());
            (Rect::from_center_size(rect.center(), Vec2::splat(size)), rect, Align2::CENTER_CENTER)
        }
    };
    knob(painter, knob_area.center(), knob_area.width(), position, pointer, response);
    if place != ValuePosition::None && text_area.is_positive() {
        text_in(painter, text_area, align, text, theme::mono_font(size_text), ink);
    }
}

/// Fill and text of a track-on toggle: light when on, dark when off, a step brighter hovered.
pub fn toggle_colors(ui: &Ui, response: &Response, on: bool) -> (Color32, Color32) {
    let hover = hover(ui, response);
    let lit = motion::toward(ui.ctx(), response.id.with("on"), on, motion::VALUE);
    let off_fill = motion::mix(theme::control_background(), theme::control_hover(), hover);
    let off_text = motion::mix(theme::text_hint(), theme::text_secondary(), hover);
    let on_fill = motion::mix(theme::control_selected(), Color32::WHITE, 0.2 * hover);
    // The inverse ink, unless a variant's lit fill leaves it unreadable (white on a light fill).
    let on_text = if theme::contrast(on_fill, theme::text_inverse()) < 3.0 {
        theme::text_on(on_fill)
    } else {
        theme::text_inverse()
    };
    (motion::mix(off_fill, on_fill, lit), motion::mix(off_text, on_text, lit))
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
    if super::control_layout::placement("solo").hidden {
        return None;
    }
    let rect = super::control_layout::place("solo", rect, painter.clip_rect());
    let label = if solo { format!("End solo on {name}") } else { format!("Solo {name}") };
    let response = button_area(ui, rect, id, label);
    let _style = super::style_class::enter_named_response("button", "solo", &response);
    super::native_css::mark(super::native_css::HostState::Checked, solo);
    let lit = motion::toward(ui.ctx(), response.id.with("on"), solo, motion::VALUE);
    let off = motion::mix(theme::control_background(), theme::control_hover(), hover(ui, &response));
    track_button_face(ui, painter, rect, &response, motion::mix(off, theme::accent(), lit));
    let text = super::style_class::foreground(motion::mix(theme::text_secondary(), theme::text_inverse(), lit));
    if !super::skin::paint(painter, rect, "solo", super::skin::State::response(&response, solo), text, 0.0) {
        text_in(painter, rect, Align2::CENTER_CENTER, "S", theme::ui_font(theme::TEXT_SMALL), text);
    }
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
    if super::control_layout::placement("cue").hidden {
        return None;
    }
    let rect = super::control_layout::place("cue", rect, painter.clip_rect());
    let label = if cue { format!("Stop cueing {name}") } else { format!("Cue {name} on the headphones") };
    let response = button_area(ui, rect, id, label);
    let _style = super::style_class::enter_named_response("button", "cue", &response);
    super::native_css::mark(super::native_css::HostState::Checked, cue);
    let (fill, text) = toggle_colors(ui, &response, cue);
    let text = super::style_class::foreground(text);
    track_button_face(ui, painter, rect, &response, fill);
    if !super::skin::paint(painter, rect, "cue", super::skin::State::response(&response, cue), text, 0.0) {
        headphones(painter, rect, text);
    }
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
    if super::control_layout::placement("arm").hidden {
        return None;
    }
    let rect = super::control_layout::place("arm", rect, painter.clip_rect());
    let label = if armed { format!("Disarm {name}") } else { format!("Arm {name} for recording") };
    let response = button_area(ui, rect, id, label).on_hover_text("Make this track ready to record");
    let _style = super::style_class::enter_named_response("button", "arm", &response);
    super::native_css::mark(super::native_css::HostState::Checked, armed);
    super::native_css::mark(super::native_css::HostState::Armed, armed);
    let lit = motion::toward(ui.ctx(), response.id.with("on"), armed, motion::VALUE);
    let off = motion::mix(theme::control_background(), theme::control_hover(), hover(ui, &response));
    track_button_face(ui, painter, rect, &response, motion::mix(off, theme::recording_background(), lit));
    let radius = (rect.height().min(rect.width()) * 0.22).clamp(2.5, 4.0);
    let ring = super::style_class::foreground(motion::mix(theme::text_secondary(), theme::recording(), lit));
    if !super::skin::paint(painter, rect, "arm", super::skin::State::response(&response, armed), ring, 0.0) {
        painter.circle_stroke(rect.center(), radius, Stroke::new(1.2, ring));
        if lit > 0.0 {
            painter.circle_filled(rect.center(), radius, theme::recording().gamma_multiply(lit));
        }
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
    form_dropdown(ui, painter, rect, response, text);
}

/// Text fields have no enclosing box; their underline is painted after interaction.
pub fn input_frame() -> egui::Frame {
    egui::Frame::NONE.inner_margin(2.0)
}

/// A text edit with the same transparent surface and focus underline in every view.
pub fn text_input(ui: &mut Ui, edit: egui::TextEdit<'_>) -> Response {
    let _style_class = crate::kit::style_class::enter("text-input");
    let mut styled_ui = crate::kit::style_class::ui(ui);
    let ui = &mut *styled_ui;
    let response = ui.add(edit.frame(input_frame()));
    input_line(ui, &response);
    response
}

/// The fixed-rectangle counterpart used by browser rows and project dialogs.
pub fn text_input_at(ui: &mut Ui, rect: Rect, edit: egui::TextEdit<'_>) -> Response {
    let _style_class = crate::kit::style_class::enter("text-input");
    let mut styled_ui = crate::kit::style_class::ui(ui);
    let ui = &mut *styled_ui;
    let response = ui.put(rect, edit.frame(input_frame()));
    input_line(ui, &response);
    response
}

/// Closed choice fields show their surrounding surface; their menus retain normal styling.
pub fn combo_box<R>(
    ui: &mut Ui,
    combo: egui::ComboBox,
    contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let _style_class = super::style_class::enter("dropdown");
    let mut styled_ui = super::style_class::ui(ui);
    let ui = &mut *styled_ui;
    let menu_style = ui.style().clone();
    ui.scope(|ui| {
        transparent_control(ui);
        let response = combo.show_ui(ui, |ui| {
            ui.set_style(menu_style);
            contents(ui)
        });
        input_line(ui, &response.response);
        response
    })
    .inner
}

fn transparent_control(ui: &mut Ui) {
    let visuals = &mut ui.style_mut().visuals;
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.bg_fill = Color32::TRANSPARENT;
        widget.weak_bg_fill = Color32::TRANSPARENT;
        widget.bg_stroke = Stroke::NONE;
    }
}

/// Draggable/typed numeric fields share the form underline without changing interaction.
pub fn number_input(ui: &mut Ui, size: Option<egui::Vec2>, number: egui::DragValue<'_>) -> Response {
    let _style_class = super::style_class::enter("value");
    let mut styled_ui = super::style_class::ui(ui);
    let ui = &mut *styled_ui;
    ui.scope(|ui| {
        transparent_control(ui);
        let response = if let Some(size) = size { ui.add_sized(size, number) } else { ui.add(number) };
        input_line(ui, &response);
        response
    })
    .inner
}

pub fn number_input_at(ui: &mut Ui, rect: Rect, number: egui::DragValue<'_>) -> Response {
    let _style_class = super::style_class::enter("value");
    let mut styled_ui = super::style_class::ui(ui);
    let ui = &mut *styled_ui;
    ui.scope(|ui| {
        transparent_control(ui);
        let response = ui.put(rect, number);
        input_line(ui, &response);
        response
    })
    .inner
}

/// A quiet underline, stronger on hover and accented while editing.
pub fn input_line(ui: &Ui, response: &Response) {
    let amount = hover(ui, response);
    let color = if response.has_focus() {
        theme::accent()
    } else {
        motion::mix(theme::border(), theme::border_strong(), amount)
    };
    ui.painter().hline(response.rect.x_range(), response.rect.bottom() - 0.5, theme::hairline(color));
}

/// A form field whose surrounding surface shows through, with only an underline.
pub fn form_dropdown(ui: &Ui, painter: &Painter, rect: Rect, response: &Response, text: &str) {
    let _style_class = crate::kit::style_class::enter_response("dropdown", response);
    let hover = hover(ui, response);
    let border = if response.has_focus() {
        theme::accent()
    } else {
        motion::mix(theme::border(), theme::border_strong(), hover)
    };
    painter.hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline(border));
    dropdown_contents(painter, rect, text, theme::text_value(), theme::text_hint());
}

/// A secondary routing field: quiet until hovered, with the same menu and hit area.
pub fn subtle_dropdown(ui: &Ui, painter: &Painter, rect: Rect, response: &Response, text: &str, background: Color32) {
    let amount = hover(ui, response);
    let mut surface = background;
    if amount > 0.0 || response.has_focus() {
        surface = motion::mix(background, theme::control_hover(), amount.max(0.3));
    }
    let line = if response.has_focus() {
        theme::accent()
    } else {
        motion::mix(background, theme::text_on(background), 0.25 + 0.25 * amount)
    };
    painter.hline(rect.x_range(), rect.bottom() - 0.5, theme::hairline(line));
    let ink = theme::text_on(surface);
    dropdown_contents(painter, rect, text, ink, motion::mix(surface, ink, 0.65));
}

fn dropdown_contents(painter: &Painter, rect: Rect, text: &str, ink: Color32, hint: Color32) {
    let inner = rect.shrink2(Vec2::new(4.0, 0.0));
    let chevron = Rect::from_center_size(Pos2::new(inner.right() - 3.0, rect.center().y), Vec2::new(6.0, 4.0));
    painter.add(egui::Shape::convex_polygon(
        vec![chevron.left_top(), chevron.right_top(), chevron.center_bottom()],
        hint,
        Stroke::NONE,
    ));
    let label = Rect::from_min_max(inner.min, Pos2::new(chevron.left() - 4.0, inner.bottom()));
    text_in(painter, label, Align2::LEFT_CENTER, text, theme::ui_font(theme::TEXT_SMALL), ink);
}

/// What a value control did this frame.
pub enum Drag {
    None,
    /// Being dragged: the value to show.
    Moving(f64),
    /// Released, reset by a double-click or moved by the wheel: the value to commit.
    Done(f64),
}

/// Whether the wheel over a value field changes its value this frame: with Ctrl/Cmd held. Without it
/// the wheel scrolls the view under the field (`docs/spec/arrangement-editing.md`).
pub fn wheel_moves_value(ui: &Ui) -> bool {
    ui.input(|i| (i.modifiers.ctrl || i.modifiers.command) && !i.modifiers.alt)
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
        suhina_plugins::trace!("value control takes the keys: {:?}", response.id);
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
        suhina_plugins::trace!("value control lets the keys go: a press elsewhere");
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
        suhina_plugins::trace!("value control: Enter with {typed:?} typed");
        ctx.data_mut(|d| {
            d.remove::<(egui::Id, u64)>(key);
            d.remove::<String>(typed_id);
        });
        return (0, fine, Some(typed));
    }
    ctx.data_mut(|d| d.insert_temp(typed_id, typed.clone()));
    // Lit while it takes the keys; what is typed shows over it, on a layer above the window's
    // own, as a theme's field drawn after this would hide it (#916: the typed number did not
    // show until Enter set it).
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, response.id.with("typed-layer")));
    // Keep the target ring outside the surface: custom value painters run after key handling
    // and would cover an inset rectangle, leaving only sharp yellow corners behind.
    let radius = if response.rect.width() > response.rect.height() * 4.0 { response.rect.height() * 0.5 } else { 4.0 };
    painter.rect_stroke(response.rect, radius, Stroke::new(1.0, theme::text_secondary()), egui::StrokeKind::Outside);
    if !typed.is_empty() {
        painter.fill_background(response.rect.shrink(1.0), 0.0, theme::control_active());
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
    drag_value_axis(ui, response, preview, value, per_point, per_tick, range, default, false)
}

/// A number typed into a value control, in the control's range. A pan (-1 to 1) shows and is
/// typed on its own scale, 50L to 50R ("-50" is all the way left, "25" half right), as
/// [`format_pan`] shows it, so a number past 1 is read so.
fn typed_value(number: f64, range: (f64, f64)) -> f64 {
    let pan = range == (-1.0, 1.0);
    let number = if pan && number.abs() > 1.0 { number / 50.0 } else { number };
    number.clamp(range.0, range.1)
}

/// The same keyboard, wheel, reset and undo protocol with an explicit drag axis.
#[allow(clippy::too_many_arguments)]
pub fn drag_value_axis(
    ui: &Ui,
    response: &Response,
    preview: &mut Option<f64>,
    value: f64,
    per_point: f64,
    per_tick: f64,
    range: (f64, f64),
    default: f64,
    horizontal: bool,
) -> Drag {
    let _style_class = crate::kit::style_class::enter_response("value", response);
    // Keys to the control clicked last: the arrows step it, Enter sets what was typed.
    let (steps, fine, typed) = value_keys(ui, response);
    if let Some(typed) = typed
        && let Ok(number) = typed.replace(',', ".").trim().parse::<f64>()
        && number.is_finite()
    {
        *preview = None;
        return Drag::Done(typed_value(number, range));
    }
    if steps != 0 {
        let from = if value.is_finite() { value } else { range.0 };
        let step = f64::from(steps) * per_tick * if fine { 0.1 } else { 1.0 };
        *preview = None;
        return Drag::Done((from + step).clamp(range.0, range.1));
    }
    // Ctrl/Cmd and the wheel over the control move it, as a drag would; the caller commits what it is
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
        let delta = if horizontal { response.drag_delta().x } else { -response.drag_delta().y };
        let next = (start + f64::from(delta) * per_point * fine).clamp(range.0, range.1);
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
/// The artwork roles of a knob's body and pointer: a pan's and a send's (a return's level) knob
/// take `pan.knob.*` and `send.knob.*` when the theme has them, in the Launcher and the
/// Arrangement alike, so they can differ from the devices' knobs; otherwise every knob shares
/// `knob.*`.
pub fn knob_roles() -> (&'static str, &'static str) {
    [("pan", "pan.knob.body", "pan.knob.pointer"), ("send", "send.knob.body", "send.knob.pointer")]
        .into_iter()
        .find(|(class, body, _)| super::style_class::within(class) && super::skin::has_own(body))
        .map_or(("knob.body", "knob.pointer"), |(_, body, tip)| (body, tip))
}

pub fn knob(painter: &Painter, center: Pos2, size: f32, position: f32, pointer: Color32, response: Option<&Response>) {
    let _style = response
        .map(|response| crate::kit::style_class::enter_response("knob", response))
        .unwrap_or_else(|| crate::kit::style_class::enter("knob"));
    let state =
        response.map(|response| super::skin::State::response(response, false)).unwrap_or(super::skin::State::Normal);
    let pointer = super::style_class::foreground(pointer);
    let radius = size * 9.0 / 22.0;
    let rect = Rect::from_center_size(center, Vec2::splat(size));
    let (body, tip_role) = knob_roles();
    if !super::skin::paint(painter, rect, body, state, pointer, 0.0) {
        painter.circle_filled(center, radius, theme::control_background());
        painter.circle_stroke(center, radius, theme::hairline(theme::text_hint()));
    }
    super::native_css::background_rounded(painter, rect, size * 0.5);
    let angle = (-135.0 + 270.0 * position.clamp(0.0, 1.0)).to_radians();
    let tip = center + Vec2::new(angle.sin(), -angle.cos()) * (radius - 1.0);
    if !super::skin::paint(painter, rect, tip_role, state, pointer, angle) {
        painter.line_segment([center, tip], Stroke::new(1.5, pointer));
    }
}

/// A volume in dB as the mixer shows it.
pub fn format_db(db: f64) -> String {
    if db.is_finite() && db > -70.0 { format!("{db:.1}") } else { "-inf".into() }
}

/// A value's text as the sheet formats it: `--suhina-value-decimals: 0 | 1 | 2` rounds its
/// number, and `--suhina-value-unit: " dB"` follows it (quoted; at most 8 characters). Text
/// with no number (`-inf`, `C`) only takes the unit when it ends in a digit.
pub fn formatted_value(text: &str) -> String {
    let decimals = super::style_class::property("--suhina-value-decimals")
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|d| *d <= 2);
    let unit = super::style_class::property("--suhina-value-unit")
        .map(|v| v.trim().trim_matches(['"', '\'']).chars().take(8).collect::<String>());
    let mut out = match (decimals, text.trim().parse::<f64>()) {
        (Some(decimals), Ok(number)) if number.is_finite() => format!("{number:.decimals$}"),
        _ => text.to_owned(),
    };
    if let Some(unit) = unit
        && out.ends_with(|c: char| c.is_ascii_digit())
    {
        out.push_str(&unit);
    }
    out
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
    let _style = super::style_class::enter_response("button", &response);
    let fill = motion::mix(theme::control_background(), theme::control_hover(), hover(ui, &response));
    let radius = super::style_class::radius(0.0);
    button_surface(painter, rect, &response, false, fill, radius);
    painter.rect_stroke(rect, radius, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
    let small = theme::ui_font(theme::TEXT_SMALL);
    text_in(painter, rect, Align2::CENTER_CENTER, text, small, theme::text_primary());
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pan_and_a_send_take_their_own_knob_artwork_and_other_knobs_do_not() {
        let svg =
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10"/></svg>"#;
        let mut skin = super::super::skin::Skin::default();
        skin.assets.insert("pan.knob.body".into(), svg.into());
        skin.assets.insert("send.knob.body".into(), svg.into());
        super::super::skin::configure(&skin);
        assert_eq!(knob_roles().0, "knob.body", "a device's knob");
        {
            let _pan = super::super::style_class::enter("pan");
            assert_eq!(knob_roles(), ("pan.knob.body", "pan.knob.pointer"));
        }
        {
            let _send = super::super::style_class::enter("send");
            assert_eq!(knob_roles(), ("send.knob.body", "send.knob.pointer"));
        }
        super::super::skin::configure(&super::super::skin::Skin::default());
        let _pan = super::super::style_class::enter("pan");
        assert_eq!(knob_roles().0, "knob.body", "without the theme's own artwork");
    }

    #[test]
    fn active_toggle_text_stays_visible_over_transparent_or_gradient_panels() {
        let original = theme::palette();
        let mut palette = original;
        palette.window_background = Color32::TRANSPARENT;
        palette.text_value = Color32::from_rgb(220, 230, 220);
        let ctx = egui::Context::default();
        theme::set_palette(&ctx, palette);
        motion::set_enabled(&ctx, false);
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let rect = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::splat(24.0));
            let response = button_area(ui, rect, Id::new("transparent-toggle"), "Track on");
            let (fill, text) = toggle_colors(ui, &response, true);
            assert_eq!(text.a(), 255);
            assert!(theme::contrast(fill, text) >= 4.5);
        });
        output.textures_delta.clear();
        theme::set_palette(&ctx, original);
    }

    #[test]
    fn shared_form_controls_keep_surfaces_transparent_and_do_not_restyle_buttons() {
        let ctx = egui::Context::default();
        let mut text = String::from("Project name");
        let mut number = 12;
        let mut fields = Vec::new();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let original = ui.visuals().widgets.inactive.bg_fill;
            fields.push(text_input(ui, egui::TextEdit::singleline(&mut text)).rect);
            fields.push(number_input(ui, None, egui::DragValue::new(&mut number)).rect);
            fields.push(
                combo_box(ui, egui::ComboBox::from_id_salt("shared-choice").selected_text("One"), |ui| {
                    ui.label("Two");
                })
                .response
                .rect,
            );
            assert_eq!(ui.visuals().widgets.inactive.bg_fill, original);
        });
        for rect in fields {
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::LineSegment { points, stroke } if points[0].y == rect.bottom() - 0.5 && points[0].y == points[1].y && stroke.width > 0.0)));
            assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Rect(shape) if rect.contains_rect(shape.rect) && (shape.fill.a() > 0 || shape.stroke.width > 0.0))));
        }
        output.textures_delta.clear();
    }

    #[test]
    fn form_dropdown_has_an_underline_without_a_box_or_fill() {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::new(20.0, 20.0), Vec2::new(180.0, 18.0));
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let response = button_area(ui, rect, Id::new("form-dropdown-test"), "Choice");
            form_dropdown(ui, ui.painter(), rect, &response, "Every 5 minutes");
        });
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::LineSegment { points, stroke } if points[0].y == rect.bottom() - 0.5
                && points[0].y == points[1].y && stroke.width > 0.0)));
        assert!(
            !output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(shape) if shape.rect == rect))
        );
        output.textures_delta.clear();
    }

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
