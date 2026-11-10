// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An audio clip's panel in the clip column (`docs/spec/clip-panel.md`): the Clip, Audio, Gain and
//! Pitch groups, each control the kind that fits its value. Every change is the clip's command,
//! one undo step; every number can be typed.

use super::*;
use crate::arrangement::Dragged;
use crate::interaction::colour_picker;
use crate::kit::widgets::Drag;
use suhina_engine::model::{CLIP_TEMPO_RANGE, ROOT_NAMES, ScaleMode, TempoSource};

/// The gain fader's cap, and the pitch knob.
const FADER_WIDTH: f32 = 18.0;
const KNOB: f32 = 34.0;
/// The gain fader's range in dB: below its bottom is silence.
const GAIN_RANGE: (f64, f64) = (-70.0, 24.0);
const SEMITONES: (f64, f64) = (-48.0, 48.0);
const CENTS: (f64, f64) = (-50.0, 50.0);

/// How often a drag sends its value while it moves, in seconds: the project follows a few times a
/// second, in one undo step, as the tempo field's drag does (#916).
const SEND_EVERY: f64 = 0.1;

/// A value a control sets, and whether it is part of a drag (`more`): the calls of one drag carry
/// one `gesture`, so they make one undo step that ends when the button comes up (#1146).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Change {
    pub value: f64,
    pub more: bool,
}

/// What a drag sends: while it moves, at most every [`SEND_EVERY`]; when it ends, always. `id` is
/// the control's, under which the time of its last send is kept.
fn send(ui: &Ui, id: Id, value: f64, done: bool) -> Option<Change> {
    let key = id.with("sent");
    let sent = ui.data(|d| d.get_temp::<f64>(key));
    if done {
        ui.data_mut(|d| d.remove::<f64>(key));
        return Some(Change { value, more: sent.is_some() });
    }
    let now = ui.input(|i| i.time);
    if sent.is_none_or(|at| now - at >= SEND_EVERY) {
        ui.data_mut(|d| d.insert_temp(key, now));
        return Some(Change { value, more: true });
    }
    None
}

/// A bar of the project in beats, never 0.
fn bar_of(panel: &AudioPanel) -> f64 {
    if panel.bar_beats.is_finite() && panel.bar_beats > 0.0 { panel.bar_beats } else { 4.0 }
}

/// A content beat as `bar.beat.sixteenth`, counted from 1 (a position), or from 0 (a length).
pub(super) fn bars_text(beats: f64, bar: f64, from_one: bool) -> String {
    let quarters = (beats.max(0.0) * 4.0 + 1e-6).floor();
    let per_bar = (bar * 4.0).max(1.0);
    let bars = (quarters / per_bar).floor();
    let rest = quarters - bars * per_bar;
    let one = if from_one { 1 } else { 0 };
    format!("{}.{}.{}", bars as i64 + one, (rest / 4.0).floor() as i64 + one, (rest % 4.0) as i64 + one)
}

/// `bar.beat.sixteenth` typed, as beats: missing parts are the first (a position) or none (a
/// length); "3" is bar 3. `None` for what is not that.
pub(super) fn bars_parse(text: &str, bar: f64, from_one: bool) -> Option<f64> {
    let one = if from_one { 1.0 } else { 0.0 };
    let parts: Vec<f64> = text.trim().split('.').map(|p| p.trim().parse::<f64>()).collect::<Result<_, _>>().ok()?;
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|p| !p.is_finite()) {
        return None;
    }
    let at = |i: usize| parts.get(i).copied().unwrap_or(one) - one;
    Some((at(0) * bar + at(1) + at(2) * 0.25).max(0.0))
}

/// The places of a `bar.beat.sixteenth` text a drag steps: each number and its step in beats.
fn bars_places(text: &str, bar: f64) -> Vec<(usize, usize, f64)> {
    let mut out = Vec::new();
    let mut from = 0;
    for (index, part) in text.split('.').enumerate() {
        let len = part.chars().count();
        let step = [bar, 1.0, 0.25].get(index).copied().unwrap_or(0.25);
        out.push((from, from + len, step));
        from += len + 1;
    }
    out
}

/// The places of a tempo's text, as the project's tempo field has them (#829): the integer part
/// 1 BPM, each decimal a tenth of the one before.
fn tempo_places(text: &str) -> Vec<(usize, usize, f64)> {
    let dot = text.find('.').unwrap_or(text.len());
    let mut out = vec![(0, dot, 1.0)];
    for decimal in 1..=3 {
        if dot + decimal < text.len() + 1 && dot + decimal <= text.chars().count() {
            out.push((dot + decimal, dot + decimal + 1, 10f64.powi(-(decimal as i32))));
        }
    }
    out
}

/// A number typed, with a unit after it ("96 BPM", "-6 dB") left off.
fn plain_number(text: &str) -> Option<f64> {
    let number: String =
        text.trim().chars().take_while(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | ',')).collect();
    number.replace(',', ".").parse::<f64>().ok().filter(|n| n.is_finite())
}

/// Semitones as the knob says them.
fn semitones_text(st: f64) -> String {
    let st = st.round();
    if st > 0.0 { format!("+{st} st") } else { format!("{st} st") }
}

fn gain_text(db: f64) -> String {
    format!("{} dB", widgets::format_db(db))
}

/// What a tempo's source reads as under the field.
fn source_text(source: Option<TempoSource>, warped: bool) -> &'static str {
    match (source, warped) {
        (_, false) => "not warped",
        (Some(TempoSource::Metadata), _) => "from metadata",
        (Some(TempoSource::Name), _) => "from the file name",
        (Some(TempoSource::Detected), _) => "detected",
        (Some(TempoSource::Manual), _) => "set by hand",
        (None, true) => "",
    }
}

/// A transpose split into whole semitones and cents (−50 … +50).
pub(super) fn split_transpose(transpose: f64) -> (f64, f64) {
    let semitones = transpose.round();
    let cents = ((transpose - semitones) * 100.0).round().clamp(CENTS.0, CENTS.1);
    (semitones, cents)
}

/// Where a gain sits on the fader, 0 … 1: 0 dB half-way, 0 … +24 dB over the upper half, −24 …
/// 0 dB over the next three tenths, −70 … −24 dB over the lowest fifth.
pub(super) fn fader_position(db: f64) -> f64 {
    let db = if db.is_finite() { db.clamp(GAIN_RANGE.0, GAIN_RANGE.1) } else { GAIN_RANGE.0 };
    if db >= 0.0 {
        0.5 + 0.5 * db / GAIN_RANGE.1
    } else if db >= -24.0 {
        0.2 + 0.3 * (db + 24.0) / 24.0
    } else {
        0.2 * (db - GAIN_RANGE.0) / (-24.0 - GAIN_RANGE.0)
    }
}

/// The gain at fader position `t`, the inverse of [`fader_position`].
pub(super) fn fader_db(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t >= 0.5 {
        (t - 0.5) / 0.5 * GAIN_RANGE.1
    } else if t >= 0.2 {
        -24.0 + (t - 0.2) / 0.3 * 24.0
    } else {
        GAIN_RANGE.0 + t / 0.2 * (-24.0 - GAIN_RANGE.0)
    }
}

/// How a number field takes its value.
#[derive(Clone, Copy)]
struct Field {
    range: (f64, f64),
    per_point: f64,
    per_tick: f64,
    /// Rounds a dragged value to its step.
    snap: fn(f64) -> f64,
    /// Where its text sits: at the right for a position or a tempo, centred under a fader or knob.
    align: Align2,
    /// Drawn faint: a value that does not apply now (the loop's, while the loop is off).
    dim: bool,
}

fn tenth(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
fn whole(v: f64) -> f64 {
    v.round()
}
fn thousandth(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// A field's surface. Fields keep the panel's gaps between them, so none touch (the grey box of
/// #1190).
fn field_surface(ui: &Ui, painter: &egui::Painter, rect: Rect, response: &egui::Response) {
    let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, response));
    painter.rect_filled(rect, 3.0, fill);
}

/// The places of a text a drag steps: each a range of characters and its step.
type Places<'a> = dyn Fn(&str) -> Vec<(usize, usize, f64)> + 'a;

/// How a field reads typed text and which places of its text a drag steps.
struct Text<'a> {
    shown: &'a dyn Fn(f64) -> String,
    parse: &'a dyn Fn(&str) -> Option<f64>,
    /// The places of the text, each a range of characters and its step: a drag steps the place
    /// under the pointer where it was pressed, as the project's tempo field does (#829). `None`
    /// steps the whole value by `per_point`.
    places: Option<&'a Places<'a>>,
}

/// A number field in `rect`: dragged up and down (the value follows while it moves, in one undo
/// step), and on a click a text field with the number selected (Enter sets it, Escape or a click
/// elsewhere leaves it; the arrows step it while it is open). Named "`name`: `shown`".
#[allow(clippy::too_many_arguments)]
fn number_field(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    id: Id,
    name: &str,
    value: f64,
    text: Text<'_>,
    field: Field,
) -> Option<Change> {
    let edit_id = id.with("typing");
    if let Some(mut typed) = ui.data(|d| d.get_temp::<String>(edit_id)) {
        // The arrows step the value while the text field is open, as they step every value
        // control (#707): taken before the field moves its cursor with them.
        let steps = ui.input_mut(|i| {
            let up = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                || i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowUp);
            let down = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                || i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowDown);
            i32::from(up) - i32::from(down)
        });
        if steps != 0 {
            let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
            let from = (text.parse)(&typed).unwrap_or(value);
            let next =
                (field.snap)(from + f64::from(steps) * field.per_tick * fine).clamp(field.range.0, field.range.1);
            let shown =
                (text.shown)(next).split_whitespace().next().unwrap_or_default().trim_start_matches('+').to_string();
            ui.data_mut(|d| d.insert_temp(edit_id, shown));
            return Some(Change { value: next, more: false });
        }
        let edit = egui::TextEdit::singleline(&mut typed)
            .id(id.with("text"))
            .font(theme::mono_font(theme::TEXT_SMALL))
            .align(egui::Align2::RIGHT_CENTER);
        let response = widgets::text_input_at(ui, rect, edit);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, format!("{name} value")));
        if ui.data(|d| d.get_temp::<bool>(id.with("started"))).is_none() {
            response.request_focus();
            // The number is selected, so typing replaces it.
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                let all = egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(typed.chars().count()),
                );
                state.cursor.set_char_range(Some(all));
                state.store(ui.ctx(), response.id);
            }
            ui.data_mut(|d| d.insert_temp(id.with("started"), true));
        }
        let entered = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if response.lost_focus() || entered || escaped {
            ui.data_mut(|d| {
                d.remove::<String>(edit_id);
                d.remove::<bool>(id.with("started"));
            });
            if entered && !escaped {
                return (text.parse)(&typed)
                    .map(|n| Change { value: n.clamp(field.range.0, field.range.1), more: false });
            }
            return None;
        }
        ui.data_mut(|d| d.insert_temp(edit_id, typed));
        return None;
    }
    let preview_id = id.with("preview");
    let grab_id = id.with("grab");
    let mut preview = ui.data(|d| d.get_temp::<f64>(preview_id));
    let grab = ui.data(|d| d.get_temp::<(f64, f64)>(grab_id));
    // A value dragged by a place snaps to that place's steps from where the drag began.
    let snapped = |v: f64| match grab {
        Some((step, from)) if step > 0.0 => thousandth(from + ((v - from) / step).round() * step),
        _ => (field.snap)(v),
    };
    let shown = (text.shown)(preview.map_or(value, snapped));
    let response = ui.interact(rect, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeVertical);
    let label = format!("{name}: {shown}");
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::Slider, true, &label);
        info.current_text_value = Some(shown.clone());
        info
    });
    field_surface(ui, painter, rect, &response);
    let text_area = rect.shrink2(Vec2::new(PAD / 2.0, 0.0));
    let mono = theme::mono_font(theme::TEXT_SMALL);
    let ink = if field.dim { theme::text_hint() } else { theme::text_value() };
    widgets::text_in(painter, text_area, field.align, &shown, mono.clone(), ink);
    // One click opens the text field (a drag moves the value instead): a double-click was
    // needed before, and a click that only took the keys was easy to miss.
    if response.clicked() {
        let start =
            (text.shown)(value).split_whitespace().next().unwrap_or_default().trim_start_matches('+').to_string();
        ui.data_mut(|d| {
            d.insert_temp(edit_id, start);
            d.remove::<f64>(preview_id);
            d.remove::<(f64, f64)>(grab_id);
        });
        return None;
    }
    if !value.is_finite() {
        return None;
    }
    // The place under the pointer, or the one the drag holds, underlined.
    let places = text.places.map(|places| places(&shown)).unwrap_or_default();
    let char_width = widgets::text_width(ui, "0", mono);
    let len = shown.chars().count();
    let place_at = |x: f32| {
        let from_right = if char_width > 0.0 { ((text_area.right() - x) / char_width).floor().max(0.0) } else { 0.0 };
        let index = len.saturating_sub(from_right as usize + 1);
        places.iter().find(|(from, to, _)| index >= *from && index < *to).or(places.first()).copied()
    };
    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        && let Some((_, _, step)) = place_at(origin.x)
    {
        ui.data_mut(|d| d.insert_temp(grab_id, (step, value)));
    }
    let held = ui.data(|d| d.get_temp::<(f64, f64)>(grab_id)).map(|(step, _)| step);
    let underlined = match held {
        Some(step) => places.iter().find(|(_, _, s)| (*s - step).abs() < 1e-12).copied(),
        None => response.hover_pos().and_then(|p| place_at(p.x)),
    };
    if let Some((from, to, _)) = underlined {
        let right = text_area.right();
        let span = egui::Rangef::new(right - (len - from) as f32 * char_width, right - (len - to) as f32 * char_width);
        let ink = if held.is_some() { theme::accent() } else { theme::text_secondary() };
        painter.hline(span, rect.bottom() - 2.0, egui::Stroke::new(1.0, ink));
    }
    // Four points of drag a step of the place grabbed; the arrow keys the hovered place's step.
    let (per_point, per_tick) = match held.or(underlined.map(|(_, _, s)| s)) {
        Some(step) if !places.is_empty() => (step / 4.0, step),
        _ => (field.per_point, field.per_tick),
    };
    let drag = widgets::drag_value(ui, &response, &mut preview, value, per_point, per_tick, field.range, value);
    match drag {
        Drag::Moving(v) => {
            ui.data_mut(|d| d.insert_temp(preview_id, v));
            // The pointer hides while it drags and comes back on the field, as on every value
            // control.
            widgets::value_drag_returns_to(ui.ctx(), response.rect.center());
            send(ui, id, snapped(v).clamp(field.range.0, field.range.1), false)
        }
        Drag::Done(v) | Drag::Set(v) => {
            let v = snapped(v).clamp(field.range.0, field.range.1);
            ui.data_mut(|d| {
                d.remove::<f64>(preview_id);
                d.remove::<(f64, f64)>(grab_id);
            });
            let change = send(ui, id, v, true);
            change.filter(|c| c.more || (c.value - value).abs() > 1e-9)
        }
        Drag::None => None,
    }
}

/// The gesture the clip panel's drags send, one drag at a time.
const GESTURE: &str = "clip-panel";

/// `params` with the drag's `gesture` when `change` is part of a drag.
fn with_more(mut params: serde_json::Value, change: Change) -> serde_json::Value {
    if change.more
        && let Some(map) = params.as_object_mut()
    {
        map.insert("gesture".into(), crate::interaction::commands::gesture(GESTURE));
    }
    params
}

/// The clip's name in the title row; a double-click opens a text field to rename it
/// (`clip.rename`): Enter keeps the name, Escape or a click elsewhere leaves it.
pub(super) fn clip_name(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    name: &str,
    at: ClipRef,
    actions: &mut Vec<Action>,
) {
    let id = Id::unique("clip-panel-name");
    let editing = id.with("typing");
    if let Some(mut text) = ui.data(|d| d.get_temp::<String>(editing)) {
        let edit = egui::TextEdit::singleline(&mut text).id(id.with("text")).font(theme::ui_font(theme::TEXT));
        let response = widgets::text_input_at(ui, rect, edit);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Clip name"));
        if ui.data(|d| d.get_temp::<bool>(id.with("started"))).is_none() {
            response.request_focus();
            ui.data_mut(|d| d.insert_temp(id.with("started"), true));
        }
        let entered = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if response.lost_focus() || entered || escaped {
            ui.data_mut(|d| {
                d.remove::<String>(editing);
                d.remove::<bool>(id.with("started"));
            });
            if entered && !escaped && text != name {
                actions.push(Action::new("clip.rename", on_clip(at, json!({ "name": text }))));
            }
        } else {
            ui.data_mut(|d| d.insert_temp(editing, text));
        }
        return;
    }
    let response = ui.interact(rect, id, Sense::click()).on_hover_text("Double-click to rename");
    let label = format!("Clip name: {name}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    widgets::text_in(painter, rect, Align2::LEFT_CENTER, name, theme::ui_font(theme::TEXT), theme::text_primary());
    if response.double_clicked() {
        ui.data_mut(|d| d.insert_temp(editing, name.to_string()));
    }
}

/// A small button, as the kit draws one; returns whether it was clicked.
fn small_button(ui: &Ui, painter: &egui::Painter, rect: Rect, text: &str, label: &str, enabled: bool) -> bool {
    let response = widgets::button_area(ui, rect, Id::unique(("clip-panel-button", label)), label);
    let fill = if enabled {
        motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response))
    } else {
        crate::kit::style_class::surface_or(theme::panel_background())
    };
    painter.rect_filled(rect, 3.0, fill);
    painter.rect_stroke(rect, 3.0, theme::hairline(theme::border()), egui::StrokeKind::Inside);
    let ink = if enabled { theme::text_primary() } else { theme::text_hint() };
    let size = if rect.height() < 15.0 { theme::TEXT_TINY } else { theme::TEXT_SMALL };
    widgets::text_in(painter, rect, Align2::CENTER_CENTER, text, theme::ui_font(size), ink);
    enabled && response.clicked()
}

/// A switch the width of its part, showing `text`, lit while on; named "`name`: On" or "Off".
fn toggle(ui: &Ui, painter: &egui::Painter, rect: Rect, name: &str, text: &str, on: bool) -> bool {
    let state = if on { "On" } else { "Off" };
    let response = widgets::button_area(ui, rect, Id::unique(("clip-panel-switch", name)), format!("{name}: {state}"));
    let (fill, ink) = widgets::toggle_colors(ui, &response, on);
    painter.rect_filled(rect, 3.0, fill);
    if !on {
        painter.rect_stroke(rect, 3.0, theme::hairline(theme::border()), egui::StrokeKind::Inside);
    }
    widgets::text_in(painter, rect, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_SMALL), ink);
    response.clicked()
}

/// A name over a control: the name at the left of `rect`, small.
fn name_over(painter: &egui::Painter, rect: Rect, text: &str, align: Align2) {
    widgets::text_in(painter, rect, align, text, theme::ui_font(theme::TEXT_TINY), theme::text_secondary());
}

/// A drop-down field: its value and a chevron; a click opens its list right under it, as wide as
/// the field, the value in it lit. The list's entries are buttons named as they read; `None` is a
/// line between entries. Returns the index of the entry picked.
#[allow(clippy::too_many_arguments)]
fn drop_down(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    id: Id,
    name: &str,
    shown: &str,
    entries: &[Option<(&str, bool)>],
    list_width: f32,
) -> Option<usize> {
    let response = widgets::button_area(ui, rect, id, format!("{name}: {shown}"));
    let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
    painter.rect_filled(rect, 3.0, fill);
    let text = Rect::from_min_max(rect.min + Vec2::new(6.0, 0.0), rect.max - Vec2::new(14.0, 0.0));
    widgets::text_in(painter, text, Align2::LEFT_CENTER, shown, theme::ui_font(theme::TEXT_SMALL), theme::text_value());
    widgets::chevron(painter, Pos2::new(rect.right() - 8.0, rect.center().y), false, theme::text_secondary());
    let mut picked = None;
    egui::Popup::menu(&response).width(list_width).gap(2.0).align(egui::RectAlign::BOTTOM_START).show(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(0.0, 1.0);
        for (index, entry) in entries.iter().enumerate() {
            let Some((label, selected)) = entry else {
                let (line, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 5.0), Sense::hover());
                ui.painter().hline(line.x_range(), line.center().y, theme::hairline(theme::border()));
                continue;
            };
            let (row, item) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::click());
            item.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, *selected, *label));
            let lit = *selected || item.hovered();
            if lit {
                let fill = if *selected { theme::control_selected() } else { theme::control_hover() };
                ui.painter().rect_filled(row, 2.0, fill);
            }
            let ink = if *selected { theme::text_on(theme::control_selected()) } else { theme::text_primary() };
            let inset = row.shrink2(Vec2::new(6.0, 0.0));
            widgets::text_in(ui.painter(), inset, Align2::LEFT_CENTER, label, theme::ui_font(theme::TEXT_SMALL), ink);
            if item.clicked() {
                picked = Some(index);
                ui.close();
            }
        }
    });
    picked
}

/// The widths and gaps of the panel's parts in a row, as the design has them: Clip, Audio, then
/// Gain and Pitch.
const CLIP_PART: f32 = 156.0;
const AUDIO_PART: f32 = 112.0;
const GAIN_PART: f32 = 128.0;
const PART_GAP: f32 = 12.0;
/// The panel's width in a row: its three parts, their gaps and its padding.
pub(super) const ROW_WIDTH: f32 = CLIP_PART + AUDIO_PART + GAIN_PART + PART_GAP * 2.0 + 16.0;
/// A name over a field, a field, a toggle, and the space between controls.
const NAME: f32 = 14.0;
const FIELD: f32 = 18.0;
const TOGGLE: f32 = 20.0;
const GAP: f32 = 6.0;
/// The height of the Gain and Pitch part when the panel is stacked.
const STACKED_GAIN: f32 = 150.0;

/// How tall the panel is with its parts one under another (the detail area stacked).
pub(super) fn stacked_height(_info: &ClipInfo) -> f32 {
    let clip = (NAME + FIELD) * 3.0 + TOGGLE + GAP * 3.0;
    let audio = TOGGLE + FIELD + (NAME + FIELD + 4.0 + FIELD + 3.0 + 12.0) + TOGGLE + GAP * 3.0;
    clip + 14.0 + audio + 14.0 + STACKED_GAIN + 16.0
}

/// The title row of an audio clip's panel: its colour (a click opens the palette), its name (a
/// double-click renames it) and its length in beats.
pub(super) fn title_row(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    info: &ClipInfo,
    at: ClipRef,
    panel: &AudioPanel,
    actions: &mut Vec<Action>,
) {
    let swatch = Rect::from_center_size(Pos2::new(rect.left() + 8.0 + 6.0, rect.center().y), Vec2::splat(12.0));
    let colour = panel.color.map_or(theme::text_hint(), colour_picker::color32);
    painter.rect_filled(swatch, 2.0, colour);
    painter.rect_stroke(swatch, 2.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
    // `clip.setColor` names arrangement clips: a slot's clip shows its colour without the palette.
    if !matches!(at, ClipRef::Arrangement(..)) {
        title_rest(ui, painter, rect, swatch, info, at, panel, actions);
        return;
    }
    let response = widgets::button_area(ui, swatch, Id::unique(("clip-panel-colour", panel.clip.0)), "Clip colour");
    egui::Popup::menu(&response).show(|ui| {
        if let Some(picked) = colour_picker::swatch_row(ui, panel.color) {
            let params = json!({ "clip": panel.clip.0, "color": colour_picker::hex(picked.colour) });
            actions.push(Action::new("clip.setColor", params));
            picked.close_menu(ui);
        }
        if ui.button("Track colour").clicked() {
            actions.push(Action::new("clip.setColor", json!({ "clip": panel.clip.0, "color": "none" })));
            ui.close();
        }
    });
    title_rest(ui, painter, rect, swatch, info, at, panel, actions);
}

/// The title row after the swatch: the clip's name and its length.
#[allow(clippy::too_many_arguments)]
fn title_rest(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    swatch: Rect,
    info: &ClipInfo,
    at: ClipRef,
    panel: &AudioPanel,
    actions: &mut Vec<Action>,
) {
    let length = format!("{} beats", (panel.length_beats * 100.0).round() / 100.0);
    let mono = theme::mono_font(theme::TEXT_TINY);
    let length_width = widgets::text_width(ui, &length, mono.clone());
    let right =
        Rect::from_min_max(Pos2::new(rect.right() - 8.0 - length_width, rect.top()), rect.max - Vec2::new(8.0, 0.0));
    widgets::text_in(painter, right, Align2::RIGHT_CENTER, &length, mono, theme::text_hint());
    let name =
        Rect::from_min_max(Pos2::new(swatch.right() + 8.0, rect.top()), Pos2::new(right.left() - 8.0, rect.bottom()));
    if info.others.is_empty() {
        clip_name(ui, painter, name, &info.name, at, actions);
    } else {
        let text = format!("{} selected", info.others.len() + 1);
        widgets::text_in(painter, name, Align2::LEFT_CENTER, &text, theme::ui_font(theme::TEXT), theme::text_primary());
    }
}

/// The audio clip's parts in `area` under the title row: side by side in a row, one under another
/// when `stacked` (`docs/spec/clip-panel.md` › Layout).
#[allow(clippy::too_many_arguments)]
pub(super) fn audio_panel(
    ui: &mut Ui,
    painter: &egui::Painter,
    area: Rect,
    info: &ClipInfo,
    at: ClipRef,
    panel: &AudioPanel,
    stacked: bool,
    actions: &mut Vec<Action>,
) {
    let area = area.shrink(8.0);
    if stacked {
        let mut top = area.top();
        let part =
            |top: f32, height: f32| Rect::from_min_size(Pos2::new(area.left(), top), Vec2::new(area.width(), height));
        top = clip_part(ui, painter, part(top, area.height()), info, at, panel, actions) + 14.0;
        top = audio_part(ui, painter, part(top, area.bottom() - top), info, at, panel, actions) + 14.0;
        let gain = part(top, STACKED_GAIN.min(area.bottom() - top));
        gain_and_pitch(ui, painter, gain, info, at, panel, true, actions);
        return;
    }
    let column =
        |left: f32, width: f32| Rect::from_min_max(Pos2::new(left, area.top()), Pos2::new(left + width, area.bottom()));
    let clip = column(area.left(), CLIP_PART);
    let audio = column(clip.right() + PART_GAP, AUDIO_PART);
    let gain = column(audio.right() + PART_GAP, GAIN_PART);
    clip_part(ui, painter, clip, info, at, panel, actions);
    audio_part(ui, painter, audio, info, at, panel, actions);
    gain_and_pitch(ui, painter, gain, info, at, panel, false, actions);
}

/// Two fields side by side with their names over them, each with a Set button at the right of its
/// name when the playhead is known; returns the rects of the two fields.
fn pair(part: Rect, top: f32) -> [(Rect, Rect); 2] {
    let half = (part.width() - GAP) / 2.0;
    [0.0, 1.0].map(|i| {
        let left = part.left() + i * (half + GAP);
        let name = Rect::from_min_size(Pos2::new(left, top), Vec2::new(half, NAME));
        let field = Rect::from_min_size(Pos2::new(left, top + NAME), Vec2::new(half, FIELD));
        (name, field)
    })
}

/// The Set button at the right of a name row.
fn set_button(name: Rect) -> Rect {
    Rect::from_min_max(Pos2::new(name.right() - 24.0, name.top()), Pos2::new(name.right(), name.bottom() - 1.0))
}

/// The Clip part: the start and end markers, the loop with its position and length, and the
/// scale. Returns where it ends.
fn clip_part(
    ui: &mut Ui,
    painter: &egui::Painter,
    part: Rect,
    info: &ClipInfo,
    at: ClipRef,
    panel: &AudioPanel,
    actions: &mut Vec<Action>,
) -> f32 {
    let clip = panel.clip;
    let bar = bar_of(panel);
    let quarter = |v: f64| (v * 4.0).round() / 4.0;
    let markers = Field {
        range: (0.0, 120_000.0),
        per_point: 0.0625,
        per_tick: 0.25,
        snap: |v| (v * 4.0).round() / 4.0,
        align: Align2::RIGHT_CENTER,
        dim: false,
    };
    let position = |v: f64| bars_text(v, bar, true);
    let parse_position = |t: &str| bars_parse(t, bar, true);
    let length_shown = |v: f64| bars_text(v, bar, false);
    let parse_length = |t: &str| bars_parse(t, bar, false);
    let places = |t: &str| bars_places(t, bar);
    let as_position = || Text { shown: &position, parse: &parse_position, places: Some(&places) };
    // An arrangement clip's Set buttons stay where they are, greyed while the playhead is outside
    // the clip, so they do not come and go as a loop passes the clip's end.
    let set_to = |ui: &Ui, painter: &egui::Painter, name: Rect, what: &str| {
        if !panel.set_buttons {
            return None;
        }
        let label = format!("Set {what} to the playhead");
        let clicked = small_button(ui, painter, set_button(name), "Set", &label, panel.playhead.is_some());
        panel.playhead.filter(|_| clicked)
    };
    let mut y = part.top();
    // Start and End: where in its audio the clip plays from and to.
    let [(start_name, start_field), (end_name, end_field)] = pair(part, y);
    name_over(painter, start_name, "Start", Align2::LEFT_BOTTOM);
    name_over(painter, end_name, "End", Align2::LEFT_BOTTOM);
    let start_range = (0.0, (info.markers.1 - 0.25).max(0.0));
    let end_range = (info.markers.0 + 0.25, 120_000.0);
    let id = Id::unique(("clip-panel-field", "Start marker", clip.0));
    let start = number_field(
        ui,
        painter,
        start_field,
        id,
        "Start marker",
        info.markers.0,
        as_position(),
        Field { range: start_range, ..markers },
    )
    .or_else(|| {
        set_to(ui, painter, start_name, "Start marker")
            .map(|p| Change { value: quarter(p).clamp(start_range.0, start_range.1), more: false })
    });
    if let Some(change) = start {
        actions.push(Action::new("clip.setStart", with_more(on_clip(at, json!({ "offset": change.value })), change)));
    }
    let id = Id::unique(("clip-panel-field", "End marker", clip.0));
    let end = number_field(
        ui,
        painter,
        end_field,
        id,
        "End marker",
        info.markers.1,
        as_position(),
        Field { range: end_range, ..markers },
    )
    .or_else(|| {
        set_to(ui, painter, end_name, "End marker")
            .map(|p| Change { value: quarter(p).clamp(end_range.0, end_range.1), more: false })
    });
    if let Some(change) = end {
        let params = on_clip(at, json!({ "length": change.value - info.markers.0 }));
        actions.push(Action::new("clip.setLength", with_more(params, change)));
    }
    y += NAME + FIELD + GAP;
    // The loop switch, the width of the part.
    let switch = Rect::from_min_size(Pos2::new(part.left(), y), Vec2::new(part.width(), TOGGLE));
    if toggle(ui, painter, switch, "Loop", "Loop", info.looping) {
        for other in &info.others {
            actions.push(Action::new("clip.setLoop", json!({ "clip": other, "loop": !info.looping })));
        }
        actions.push(Action::new("clip.setLoop", on_clip(at, json!({ "loop": !info.looping }))));
    }
    y += TOGGLE + GAP;
    // Position and Length: dimmed while the loop is off; a change turns it on.
    let (start, end) = info.loop_range.unwrap_or(info.markers);
    let length = (end - start).max(0.25);
    let [(position_name, position_field), (length_name, length_field)] = pair(part, y);
    name_over(painter, position_name, "Position", Align2::LEFT_BOTTOM);
    name_over(painter, length_name, "Length", Align2::LEFT_BOTTOM);
    let dim = info.loop_range.is_none();
    let position_range = (0.0, 120_000.0 - length);
    let id = Id::unique(("clip-panel-field", "Loop position", clip.0));
    let moved = number_field(
        ui,
        painter,
        position_field,
        id,
        "Loop position",
        start,
        as_position(),
        Field { range: position_range, dim, ..markers },
    )
    .or_else(|| {
        set_to(ui, painter, position_name, "Loop position")
            .map(|p| Change { value: quarter(p).clamp(position_range.0, position_range.1), more: false })
    });
    if let Some(change) = moved {
        // The position moves the loop whole.
        let params = on_clip(at, json!({ "start": change.value, "end": change.value + length }));
        actions.push(Action::new("clip.setLoop", with_more(params, change)));
    }
    let length_range = (0.25, 120_000.0 - start);
    let id = Id::unique(("clip-panel-field", "Loop length", clip.0));
    let text = Text { shown: &length_shown, parse: &parse_length, places: Some(&places) };
    let resized = number_field(
        ui,
        painter,
        length_field,
        id,
        "Loop length",
        length,
        text,
        Field { range: length_range, dim, ..markers },
    )
    .or_else(|| {
        set_to(ui, painter, length_name, "Loop length")
            .map(|p| Change { value: quarter(p - start).clamp(length_range.0, length_range.1), more: false })
    });
    if let Some(change) = resized {
        let params = on_clip(at, json!({ "start": start, "end": start + change.value }));
        actions.push(Action::new("clip.setLoop", with_more(params, change)));
    }
    y += NAME + FIELD + GAP;
    // The scale: its root and its mode, each a list under its field.
    let name = Rect::from_min_size(Pos2::new(part.left(), y), Vec2::new(part.width(), NAME));
    name_over(painter, name, "Scale", Align2::LEFT_BOTTOM);
    let root_rect = Rect::from_min_size(Pos2::new(part.left(), y + NAME), Vec2::new(52.0, FIELD));
    let mode_rect = Rect::from_min_max(
        Pos2::new(root_rect.right() + 4.0, root_rect.top()),
        Pos2::new(part.right(), root_rect.bottom()),
    );
    let root = panel.scale.map(|s| usize::from(s.root()));
    let root_shown = root.and_then(|r| ROOT_LABELS.get(r)).copied().unwrap_or("—");
    let roots: Vec<Option<(&str, bool)>> =
        ROOT_LABELS.iter().enumerate().map(|(i, name)| Some((*name, root == Some(i)))).collect();
    let id = Id::unique(("clip-panel-scale-root", panel.clip.0));
    if let Some(picked) =
        drop_down(ui, painter, root_rect, id, "Scale root", root_shown, &roots, root_rect.width() + 20.0)
    {
        let name = ROOT_NAMES.get(picked).copied().unwrap_or("C");
        actions.push(Action::new("clip.setScale", on_clip(at, json!({ "root": name }))));
    }
    let mode = panel.scale.map(|s| s.mode);
    let mode_shown = mode.map_or("None", |m| m.label());
    let mut modes: Vec<Option<(&str, bool)>> = vec![Some(("None", mode.is_none())), None];
    modes.extend(ScaleMode::ALL.iter().map(|m| Some((m.label(), mode == Some(*m)))));
    let id = Id::unique(("clip-panel-scale-mode", panel.clip.0));
    if let Some(picked) =
        drop_down(ui, painter, mode_rect, id, "Scale", mode_shown, &modes, mode_rect.width() + root_rect.width() + 4.0)
    {
        let params = match picked.checked_sub(2).and_then(|i| ScaleMode::ALL.get(i)) {
            Some(mode) => json!({ "mode": mode.name() }),
            None => json!({ "off": true }),
        };
        actions.push(Action::new("clip.setScale", on_clip(at, params)));
    }
    y + NAME + FIELD
}

/// The roots as the Scale list shows them.
const ROOT_LABELS: [&str; 12] = ["C", "C#/Db", "D", "D#/Eb", "E", "F", "F#/Gb", "G", "G#/Ab", "A", "A#/Bb", "B"];

/// The Audio part: warp, the warp mode, the clip's tempo and the gain envelope's switch. Returns
/// where it ends.
fn audio_part(
    ui: &mut Ui,
    painter: &egui::Painter,
    part: Rect,
    info: &ClipInfo,
    at: ClipRef,
    panel: &AudioPanel,
    actions: &mut Vec<Action>,
) -> f32 {
    let clip = panel.clip;
    let row = |y: f32, height: f32| Rect::from_min_size(Pos2::new(part.left(), y), Vec2::new(part.width(), height));
    let mut y = part.top();
    if toggle(ui, painter, row(y, TOGGLE), "Warp", "Warp", info.warped) {
        actions.push(Action::new("clip.setAudio", on_clip(at, json!({ "warp": !info.warped }))));
    }
    y += TOGGLE + GAP;
    // The warp mode: a list of the six under the field.
    let shown = WARP_MODES.iter().find(|(m, _, _)| *m == info.stretch).map_or("Smooth", |(_, _, s)| s);
    let entries: Vec<Option<(&str, bool)>> =
        WARP_MODES.iter().map(|(m, _, text)| Some((*text, *m == info.stretch))).collect();
    let field = row(y, FIELD);
    let id = Id::unique(("clip-panel-mode", panel.clip.0));
    if let Some(picked) = drop_down(ui, painter, field, id, "Mode", shown, &entries, field.width())
        && let Some((mode, name, _)) = WARP_MODES.get(picked)
        && *mode != info.stretch
    {
        actions.push(Action::new("clip.setAudio", on_clip(at, json!({ "mode": name }))));
    }
    y += FIELD + GAP;
    // The tempo: its name with Find, the field, ÷2 and ×2, and where it came from.
    let name = row(y, NAME);
    name_over(painter, name, "Tempo", Align2::LEFT_BOTTOM);
    let find = set_button(name);
    let response = widgets::button_area(ui, find, Id::unique(("clip-panel-find", panel.clip.0)), "Find tempo");
    let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
    painter.rect_filled(find, 3.0, fill);
    painter.rect_stroke(find, 3.0, theme::hairline(theme::border()), egui::StrokeKind::Inside);
    widgets::text_in(
        painter,
        find,
        Align2::CENTER_CENTER,
        "Find",
        theme::ui_font(theme::TEXT_TINY),
        theme::text_primary(),
    );
    let mut set: Option<(f64, TempoSource, bool)> = None;
    let open = egui::Popup::menu(&response).show(|ui| {
        // Wide enough for "File name: 120.00" on one line: the menu took the button's narrow
        // width and broke every entry over two lines.
        ui.set_min_width(140.0);
        let names = [
            (TempoSource::Metadata, "Metadata"),
            (TempoSource::Name, "File name"),
            (TempoSource::Detected, "Detected"),
        ];
        match &panel.candidates {
            None => {
                ui.label("Looking…");
            }
            Some(found) => {
                for ((source, text), bpm) in names.into_iter().zip(found.iter()) {
                    let shown = bpm.map_or_else(|| format!("{text}: none"), |b| format!("{text}: {b:.2}"));
                    let button = egui::Button::new(egui::RichText::new(shown)).wrap_mode(egui::TextWrapMode::Extend);
                    if ui.add_enabled(bpm.is_some(), button).clicked() {
                        set = bpm.map(|b| (b, source, false));
                        ui.close();
                    }
                }
            }
        }
    });
    // The detail area works the sources out while the menu is open (they read the file).
    let wanted = open.is_some().then_some(panel.clip);
    ui.data_mut(|d| match wanted {
        Some(clip) => {
            d.insert_temp(find_tempo_key(), clip);
        }
        None => {
            if d.get_temp::<ClipId>(find_tempo_key()) == Some(panel.clip) {
                d.remove::<ClipId>(find_tempo_key());
            }
            // Asked again on the next opening: the file or the project may have changed since.
            d.remove::<[Option<f64>; 3]>(tempo_candidates_key(panel.clip));
        }
    });
    y += NAME;
    let tempo_field = Field {
        range: (*CLIP_TEMPO_RANGE.start(), *CLIP_TEMPO_RANGE.end()),
        per_point: 0.25,
        per_tick: 1.0,
        snap: thousandth,
        align: Align2::RIGHT_CENTER,
        dim: false,
    };
    let id = Id::unique(("clip-panel-field", "Tempo", clip.0));
    let value = panel.tempo.unwrap_or(f64::NAN);
    let shown_tempo = |v: f64| if v.is_finite() { format!("{v:.3}") } else { "—".to_string() };
    let text = Text { shown: &shown_tempo, parse: &plain_number, places: Some(&tempo_places) };
    let typed_tempo = number_field(ui, painter, row(y, FIELD), id, "Tempo", value, text, tempo_field);
    if let Some(change) = typed_tempo {
        set = Some((change.value, TempoSource::Manual, change.more));
    }
    // The tempo as dragged so far, for the Arrangement to draw the clip at on every frame; kept
    // after the drag until the project has it, so the clip does not flash back for a frame.
    let dragged = ui.data(|d| d.get_temp::<f64>(id.with("preview"))).map(thousandth);
    publish_preview(ui, panel.clip, dragged, typed_tempo.map(|c| c.value), value, Dragged::Tempo);
    y += FIELD + 4.0;
    let half = (part.width() - 4.0) / 2.0;
    let halve = Rect::from_min_size(Pos2::new(part.left(), y), Vec2::new(half, FIELD));
    let double = Rect::from_min_size(Pos2::new(part.left() + half + 4.0, y), Vec2::new(half, FIELD));
    let known = panel.tempo.is_some();
    if small_button(ui, painter, halve, "÷2", "Halve the tempo", known) {
        set = panel.tempo.map(|t| (t / 2.0, TempoSource::Manual, false));
    }
    if small_button(ui, painter, double, "×2", "Double the tempo", known) {
        set = panel.tempo.map(|t| (t * 2.0, TempoSource::Manual, false));
    }
    y += FIELD + 3.0;
    let source = source_text(panel.tempo_source, info.warped);
    widgets::text_in(
        painter,
        row(y, 12.0),
        Align2::RIGHT_CENTER,
        source,
        theme::ui_font(theme::TEXT_TINY),
        theme::text_hint(),
    );
    y += 12.0 + GAP;
    if let Some((bpm, source, more)) = set {
        let bpm = bpm.clamp(*CLIP_TEMPO_RANGE.start(), *CLIP_TEMPO_RANGE.end());
        let mut params = json!({ "bpm": bpm, "source": source.name() });
        if more {
            params["gesture"] = crate::interaction::commands::gesture(GESTURE);
        }
        actions.push(Action::new("clip.setTempo", on_clip(at, params)));
    }
    // Edit gain envelope: opens the envelope on the clip's waveform in the Arrangement for
    // editing, as Alt+E and the button by the clip's name do.
    if matches!(at, ClipRef::Arrangement(..)) {
        let on = crate::arrangement::envelope::is_editing(ui.ctx(), panel.clip);
        if toggle(ui, painter, row(y, TOGGLE), "Envelope", "Edit gain envelope", on) {
            crate::arrangement::envelope::toggle_editing(ui.ctx(), panel.clip);
        }
    }
    y + TOGGLE
}

/// The key the tempo sources of `clip` are kept under while its Find menu is open.
pub fn tempo_candidates_key(clip: ClipId) -> Id {
    Id::unique(("clip-tempo-candidates", clip.0))
}

/// The key under which the clip panel asks for the tempo sources of a clip (its Find menu open).
pub fn find_tempo_key() -> Id {
    Id::unique("clip-panel-find-tempo")
}

/// The Gain and Pitch part: the fader and the knob side by side; centred in the part when
/// `stacked`.
#[allow(clippy::too_many_arguments)]
fn gain_and_pitch(
    ui: &mut Ui,
    painter: &egui::Painter,
    part: Rect,
    info: &ClipInfo,
    at: ClipRef,
    panel: &AudioPanel,
    stacked: bool,
    actions: &mut Vec<Action>,
) {
    // As tall as the other parts, however tall the detail area is: the fader does not stretch to
    // the area's bottom.
    let part = Rect::from_min_size(part.min, Vec2::new(part.width(), part.height().min(STACKED_GAIN)));
    let width = 52.0 + 12.0 + 64.0;
    let left = if stacked { part.center().x - width / 2.0 } else { part.left() };
    let gain = Rect::from_min_max(Pos2::new(left, part.top()), Pos2::new(left + 52.0, part.bottom()));
    let pitch = Rect::from_min_max(
        Pos2::new(gain.right() + 12.0, part.top()),
        Pos2::new(gain.right() + 12.0 + 64.0, part.bottom()),
    );
    gain_part(ui, painter, gain, info, at, panel, actions);
    pitch_part(ui, painter, pitch, info, at, panel.clip, actions);
}

fn gain_part(
    ui: &mut Ui,
    painter: &egui::Painter,
    part: Rect,
    info: &ClipInfo,
    at: ClipRef,
    panel: &AudioPanel,
    actions: &mut Vec<Action>,
) {
    let clip = panel.clip;
    name_over(painter, Rect::from_min_size(part.min, Vec2::new(part.width(), NAME)), "Gain", Align2::CENTER_BOTTOM);
    let db = info.gain_db.filter(|d| d.is_finite()).unwrap_or(GAIN_RANGE.0).max(GAIN_RANGE.0);
    // The fader between the name and the readout, its scale at its left: 24, 0 half-way, -inf.
    let field = Rect::from_min_max(Pos2::new(part.left(), part.bottom() - FIELD), part.max);
    let area =
        Rect::from_min_max(Pos2::new(part.left(), part.top() + NAME + 8.0), Pos2::new(part.right(), field.top() - 9.0));
    let track =
        Rect::from_min_max(Pos2::new(area.left() + 25.0, area.top()), Pos2::new(area.left() + 43.0, area.bottom()));
    let id = Id::unique(("clip-panel-fader", panel.clip.0));
    let response = ui
        .interact(track.expand2(Vec2::new(0.0, 4.0)), id, Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::ResizeVertical);
    let preview_id = id.with("preview");
    let preview = ui.data(|d| d.get_temp::<f64>(preview_id));
    let shown_db = preview.map_or(db, tenth);
    let label = format!("Gain fader: {}", gain_text(shown_db));
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::Slider, true, &label);
        info.value = Some(shown_db);
        info
    });
    // Clicked, it takes the keys: a number typed and Enter set it, the arrows step it by 1 dB
    // (Shift: 0.1), as every value control does (#707). Dragged, the gain follows as it moves, in
    // one undo step; a double-click puts it at 0 dB.
    let (steps, fine_keys, typed) = widgets::value_keys(ui, &response);
    let mut set: Option<Change> = None;
    if let Some(number) = typed.as_deref().and_then(plain_number) {
        set = Some(Change { value: number.clamp(GAIN_RANGE.0, GAIN_RANGE.1), more: false });
    } else if steps != 0 {
        let step = f64::from(steps) * if fine_keys { 0.1 } else { 1.0 };
        set = Some(Change { value: tenth(db + step).clamp(GAIN_RANGE.0, GAIN_RANGE.1), more: false });
    } else if response.double_clicked() {
        ui.data_mut(|d| d.remove::<f64>(preview_id));
        set = Some(Change { value: 0.0, more: false });
    } else if response.dragged_by(egui::PointerButton::Primary) {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let delta = -f64::from(widgets::value_drag_delta(ui, &response).y) / f64::from(track.height()) * fine;
        let from = fader_position(preview.unwrap_or(db));
        let next = fader_db(from + delta);
        ui.data_mut(|d| d.insert_temp(preview_id, next));
        // Hidden while it drags, the pointer comes back on the cap where the drag left it.
        let cap_y = track.bottom() - fader_position(next) as f32 * track.height();
        widgets::value_drag_returns_to(ui.ctx(), Pos2::new(track.center().x, cap_y));
        set = send(ui, id, tenth(next), false);
    } else if response.drag_stopped_by(egui::PointerButton::Primary) {
        set = preview.and_then(|v| send(ui, id, tenth(v), true));
        ui.data_mut(|d| d.remove::<f64>(preview_id));
    }
    let y_of = |db: f64| track.bottom() - fader_position(db) as f32 * track.height();
    let scale = theme::ui_font(theme::TEXT_TINY);
    for (text, at) in [("24", track.top()), ("0", y_of(0.0)), ("-inf", track.bottom())] {
        let mark = Rect::from_min_max(Pos2::new(area.left(), at - 6.0), Pos2::new(track.left() - 2.0, at + 6.0));
        widgets::text_in(painter, mark, Align2::LEFT_CENTER, text, scale.clone(), theme::text_hint());
    }
    let slot = Rect::from_center_size(track.center(), Vec2::new(4.0, track.height()));
    painter.rect_filled(slot, 2.0, theme::control_background());
    painter.hline(
        egui::Rangef::new(track.left() - 1.0, track.right() + 1.0),
        y_of(0.0),
        theme::hairline(theme::text_hint()),
    );
    let cap = Rect::from_center_size(Pos2::new(track.center().x, y_of(shown_db)), Vec2::new(FADER_WIDTH, 8.0));
    let hover = widgets::hover(ui, &response);
    crate::kit::mixer_controls::fader_cap(painter, cap, hover, &response);
    // Under it the readout, typed.
    let gain_field = Field {
        range: GAIN_RANGE,
        per_point: 0.1,
        per_tick: 1.0,
        snap: tenth,
        align: Align2::CENTER_CENTER,
        dim: false,
    };
    let text = Text { shown: &gain_text, parse: &plain_number, places: None };
    if let Some(change) =
        number_field(ui, painter, field, Id::unique(("clip-panel-field", "Gain", clip.0)), "Gain", db, text, gain_field)
    {
        set = Some(change);
    }
    if let Some(change) = set {
        let db = commands::exact_db_param(change.value, GAIN_RANGE.0);
        actions.push(Action::new("clip.setGain", with_more(on_clip(at, json!({ "db": db })), change)));
    }
}

/// Leaves the value being dragged (`dragged`), or just sent at the drag's end (`sent`), for the
/// Arrangement to draw `clip` at on every frame ([`crate::arrangement::TempoPreview`]); held until
/// the project has it (`now`), or for a second.
fn publish_preview(ui: &Ui, clip: ClipId, dragged: Option<f64>, sent: Option<f64>, now: f64, kind: fn(f64) -> Dragged) {
    use crate::arrangement::TempoPreview;
    let key = crate::arrangement::tempo_preview_key();
    let value = |p: &TempoPreview| match p.value {
        Dragged::Tempo(v) | Dragged::Transpose(v) => v,
    };
    let held = ui
        .data(|d| d.get_temp::<TempoPreview>(key))
        .filter(|p| p.clip == clip && std::mem::discriminant(&p.value) == std::mem::discriminant(&kind(0.0)));
    let preview = match (dragged, sent) {
        (Some(v), _) => Some(TempoPreview { clip, value: kind(v), frames: 60 }),
        (None, Some(v)) if held.is_some() => Some(TempoPreview { clip, value: kind(v), frames: 60 }),
        (None, _) => held
            .filter(|p| p.frames > 0 && (value(p) - now).abs() > 1e-6)
            .map(|p| TempoPreview { frames: p.frames - 1, ..p }),
    };
    ui.data_mut(|d| match preview {
        Some(p) => {
            d.insert_temp(key, p);
        }
        None if held.is_some() => d.remove::<TempoPreview>(key),
        None => {}
    });
}

fn pitch_part(
    ui: &mut Ui,
    painter: &egui::Painter,
    part: Rect,
    info: &ClipInfo,
    at: ClipRef,
    clip: ClipId,
    actions: &mut Vec<Action>,
) {
    name_over(painter, Rect::from_min_size(part.min, Vec2::new(part.width(), NAME)), "Pitch", Align2::CENTER_BOTTOM);
    let (semitones, cents) = split_transpose(info.transpose);
    // The knob: whole semitones, dragged up and down, following as it moves; clicked, it takes a
    // number typed and the arrows; a double-click puts it at 0.
    let center = Pos2::new(part.center().x, part.top() + NAME + 4.0 + KNOB / 2.0);
    let rect = Rect::from_center_size(center, Vec2::splat(KNOB));
    let id = Id::unique(("clip-panel-knob", "pitch", clip.0));
    let response = ui.interact(rect, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeVertical);
    let preview_id = id.with("preview");
    let mut preview = ui.data(|d| d.get_temp::<f64>(preview_id));
    let shown = preview.map_or(semitones, whole);
    let label = format!("Pitch: {}", semitones_text(shown));
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::Slider, true, &label);
        info.value = Some(shown);
        info
    });
    let mut set_semitones: Option<Change> = None;
    let mut set_cents: Option<Change> = None;
    let mut sent: Option<f64> = None;
    match widgets::drag_value(ui, &response, &mut preview, semitones, 0.25, 1.0, SEMITONES, 0.0) {
        Drag::Moving(v) => {
            ui.data_mut(|d| d.insert_temp(preview_id, v));
            widgets::value_drag_returns_to(ui.ctx(), center);
            set_semitones = send(ui, id, whole(v), false);
        }
        Drag::Done(v) | Drag::Set(v) => {
            ui.data_mut(|d| d.remove::<f64>(preview_id));
            set_semitones = send(ui, id, whole(v), true);
        }
        Drag::None => {}
    }
    let position = ((shown - SEMITONES.0) / (SEMITONES.1 - SEMITONES.0)) as f32;
    widgets::dial(painter, center, KNOB, position, Some(&response));
    // Under it the semitones and the cents side by side, typed, their units under them.
    let top = rect.bottom() + 6.0;
    let half = (part.width() - 4.0) / 2.0;
    let st = Rect::from_min_size(Pos2::new(part.left(), top), Vec2::new(half, FIELD));
    let ct = Rect::from_min_size(Pos2::new(st.right() + 4.0, top), Vec2::new(half, FIELD));
    for (rect, unit) in [(st, "st"), (ct, "ct")] {
        let under = Rect::from_min_size(Pos2::new(rect.left(), rect.bottom() + 1.0), Vec2::new(rect.width(), 11.0));
        widgets::text_in(
            painter,
            under,
            Align2::CENTER_TOP,
            unit,
            theme::ui_font(theme::TEXT_TINY),
            theme::text_hint(),
        );
    }
    let centred = |range, per_point| Field {
        range,
        per_point,
        per_tick: 1.0,
        snap: whole,
        align: Align2::CENTER_CENTER,
        dim: false,
    };
    let text = Text { shown: &signed, parse: &plain_number, places: None };
    let id = Id::unique(("clip-panel-field", "Semitones", clip.0));
    if let Some(change) = number_field(ui, painter, st, id, "Semitones", semitones, text, centred(SEMITONES, 0.25)) {
        set_semitones = Some(change);
    }
    let text = Text { shown: &signed, parse: &plain_number, places: None };
    let id = Id::unique(("clip-panel-field", "Cents", clip.0));
    if let Some(change) = number_field(ui, painter, ct, id, "Cents", cents, text, centred(CENTS, 0.5)) {
        set_cents = Some(change);
    }
    if let Some(change) = set_semitones.or(set_cents) {
        let st = set_semitones.map_or(semitones, |c| c.value);
        let ct = set_cents.map_or(cents, |c| c.value);
        let transpose = (((st + ct / 100.0) * 100.0).round() / 100.0).clamp(SEMITONES.0, SEMITONES.1);
        sent = Some(transpose);
        if change.more || (transpose - info.transpose).abs() > 1e-9 {
            let params = on_clip(at, json!({ "transpose": transpose }));
            actions.push(Action::new("clip.setAudio", with_more(params, change)));
        }
    }
    // An unwarped clip's pitch changes its length: the Arrangement draws it at the pitch as
    // dragged on every frame, as for the tempo.
    if !info.warped {
        let dragged = ui.data(|d| d.get_temp::<f64>(preview_id)).map(|v| whole(v) + cents / 100.0);
        publish_preview(ui, clip, dragged, sent, info.transpose, Dragged::Transpose);
    }
}

/// A whole number with its sign: `+3`, `0`, `-12`.
fn signed(value: f64) -> String {
    let value = value.round();
    if value > 0.0 { format!("+{value}") } else { format!("{value}") }
}
