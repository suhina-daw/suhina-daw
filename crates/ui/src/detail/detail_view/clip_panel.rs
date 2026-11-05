// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An audio clip's panel in the clip column (`docs/spec/clip-panel.md`): the Clip, Audio, Gain and
//! Pitch groups, each control the kind that fits its value. Every change is the clip's command,
//! one undo step; every number can be typed.

use super::*;
use crate::interaction::colour_picker;
use crate::kit::widgets::Drag;
use suhina_engine::model::{CLIP_TEMPO_RANGE, TempoSource};

/// A row of the panel, and the gap a field keeps above and below inside it, so no two fields
/// touch (the grey box of #1190).
const ROW_HEIGHT: f32 = 18.0;
const FIELD_GAP: f32 = 2.0;
/// A group's heading.
const HEADING: f32 = 14.0;
/// The Set button beside a marker or loop field, and the ÷2 / ×2 buttons.
const SET_WIDTH: f32 = 28.0;
const HALVE_WIDTH: f32 = 22.0;
/// The gain fader's track and cap, and the pitch knob.
const FADER_WIDTH: f32 = 18.0;
const FADER_HEIGHT: f32 = 100.0;
const KNOB: f32 = 34.0;
/// In a row the panel's three parts (Clip; Audio; Gain and Pitch) are side by side, each this wide.
pub(super) const PART_WIDTH: f32 = 184.0;
/// The gain fader's range in dB: below its bottom is silence.
const GAIN_RANGE: (f64, f64) = (-70.0, 24.0);
const SEMITONES: (f64, f64) = (-48.0, 48.0);
const CENTS: (f64, f64) = (-50.0, 50.0);

/// The beat count a marker or loop field shows.
fn beats(value: f64) -> String {
    beats_text(value)
}

/// Semitones as the knob's readout shows them.
fn semitones_text(st: f64) -> String {
    let st = st.round();
    if st > 0.0 { format!("+{st} st") } else { format!("{st} st") }
}

fn cents_text(cents: f64) -> String {
    let cents = cents.round();
    if cents > 0.0 { format!("+{cents} ct") } else { format!("{cents} ct") }
}

fn gain_text(db: f64) -> String {
    format!("{} dB", widgets::format_db(db))
}

fn tempo_text(bpm: Option<f64>) -> String {
    bpm.map_or_else(|| "—".to_string(), |bpm| format!("{bpm:.2} BPM"))
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

/// Where a gain sits on the fader, 0 … 1: −24 … +24 dB over its upper three quarters (0 dB at
/// 0.625), −70 … −24 dB over its lowest quarter.
pub(super) fn fader_position(db: f64) -> f64 {
    let db = if db.is_finite() { db.clamp(GAIN_RANGE.0, GAIN_RANGE.1) } else { GAIN_RANGE.0 };
    if db >= -24.0 { 0.25 + 0.75 * (db + 24.0) / 48.0 } else { 0.25 * (db - GAIN_RANGE.0) / (-24.0 - GAIN_RANGE.0) }
}

/// The gain at fader position `t`, the inverse of [`fader_position`].
pub(super) fn fader_db(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t >= 0.25 { -24.0 + (t - 0.25) / 0.75 * 48.0 } else { GAIN_RANGE.0 + t / 0.25 * (-24.0 - GAIN_RANGE.0) }
}

/// How a number field takes its value.
#[derive(Clone, Copy)]
struct Field {
    range: (f64, f64),
    per_point: f64,
    per_tick: f64,
    /// Rounds a dragged value to its step.
    snap: fn(f64) -> f64,
    horizontal: bool,
}

fn quarter(v: f64) -> f64 {
    (v * 4.0).round() / 4.0
}
fn tenth(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
fn whole(v: f64) -> f64 {
    v.round()
}
fn hundredth(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// A field's surface: inset by [`FIELD_GAP`], so fields one under another never touch.
fn field_surface(ui: &Ui, painter: &egui::Painter, rect: Rect, response: &egui::Response) {
    let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, response));
    painter.fill_background(rect, 3.0, fill);
}

/// A number field in `rect`: dragged, stepped with the arrows, typed after a click and Enter, and
/// on a double-click a text field (Enter sets it, Escape or a click elsewhere leaves it). Named
/// "`name`: `shown`". Returns the value to set, when one is set.
#[allow(clippy::too_many_arguments)]
fn number_field(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    id: Id,
    name: &str,
    value: f64,
    shown: &dyn Fn(f64) -> String,
    field: Field,
) -> Option<f64> {
    let rect = rect.shrink2(Vec2::new(0.0, FIELD_GAP));
    let edit_id = id.with("typing");
    if let Some(mut text) = ui.data(|d| d.get_temp::<String>(edit_id)) {
        let edit = egui::TextEdit::singleline(&mut text)
            .id(id.with("text"))
            .font(theme::mono_font(theme::TEXT_SMALL))
            .align(egui::Align2::RIGHT_CENTER);
        let response = widgets::text_input_at(ui, rect, edit);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, format!("{name} value")));
        let started = ui.data(|d| d.get_temp::<bool>(id.with("started"))).is_none();
        if started {
            response.request_focus();
            // The number is selected, so typing replaces it.
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                let all = egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(text.chars().count()),
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
                // A unit typed after the number ("96 BPM", "-6 dB") is left off.
                let number: String = text
                    .trim()
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | ','))
                    .collect();
                return number
                    .replace(',', ".")
                    .parse::<f64>()
                    .ok()
                    .filter(|n| n.is_finite())
                    .map(|n| n.clamp(field.range.0, field.range.1));
            }
            return None;
        }
        ui.data_mut(|d| d.insert_temp(edit_id, text));
        return None;
    }
    let preview_id = id.with("preview");
    let mut preview = ui.data(|d| d.get_temp::<f64>(preview_id));
    let text = shown(preview.map_or(value, field.snap));
    let response = ui.interact(rect, id, Sense::click_and_drag()).on_hover_cursor(if field.horizontal {
        egui::CursorIcon::ResizeHorizontal
    } else {
        egui::CursorIcon::ResizeVertical
    });
    let label = format!("{name}: {text}");
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::Slider, true, &label);
        info.current_text_value = Some(text.clone());
        info
    });
    field_surface(ui, painter, rect, &response);
    widgets::text_in(
        painter,
        rect.shrink2(Vec2::new(PAD / 2.0, 0.0)),
        Align2::RIGHT_CENTER,
        &text,
        theme::mono_font(theme::TEXT_SMALL),
        theme::text_value(),
    );
    if response.double_clicked() {
        let typed = shown(value).split_whitespace().next().unwrap_or_default().trim_start_matches('+').to_string();
        ui.data_mut(|d| {
            d.insert_temp(edit_id, typed);
            d.remove::<f64>(preview_id);
        });
        return None;
    }
    let drag = widgets::drag_value_axis(
        ui,
        &response,
        &mut preview,
        value,
        field.per_point,
        field.per_tick,
        field.range,
        value,
        field.horizontal,
    );
    match drag {
        Drag::Moving(v) => {
            ui.data_mut(|d| d.insert_temp(preview_id, v));
            None
        }
        Drag::Done(v) | Drag::Set(v) => {
            ui.data_mut(|d| d.remove::<f64>(preview_id));
            let v = (field.snap)(v).clamp(field.range.0, field.range.1);
            ((v - value).abs() > 1e-9).then_some(v)
        }
        Drag::None => None,
    }
}

/// A small button; returns whether it was clicked.
fn small_button(ui: &Ui, painter: &egui::Painter, rect: Rect, text: &str, label: &str, enabled: bool) -> bool {
    let rect = rect.shrink2(Vec2::new(1.0, FIELD_GAP));
    let response = widgets::button_area(ui, rect, Id::unique(("clip-panel-button", label)), label);
    let fill = if enabled {
        motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response))
    } else {
        theme::panel_background()
    };
    painter.fill_background(rect, 3.0, fill);
    painter.rect_stroke(rect, 3.0, theme::hairline(theme::border()), egui::StrokeKind::Inside);
    let ink = if enabled { theme::text_primary() } else { theme::text_hint() };
    widgets::text_in(painter, rect, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_SMALL), ink);
    enabled && response.clicked()
}

/// A switch: "`name`: On" or "Off", a click turns it.
fn switch_row(ui: &Ui, painter: &egui::Painter, rect: Rect, name: &str, on: bool) -> bool {
    let rect = rect.shrink2(Vec2::new(0.0, FIELD_GAP));
    let text = if on { "On" } else { "Off" };
    let response = widgets::button_area(ui, rect, Id::unique(("clip-panel-switch", name)), format!("{name}: {text}"));
    let (fill, ink) = widgets::toggle_colors(ui, &response, on);
    painter.fill_background(rect, 3.0, fill);
    widgets::text_in(painter, rect, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_SMALL), ink);
    response.clicked()
}

/// A label at the left of a row.
fn label(painter: &egui::Painter, row: Rect, text: &str) {
    widgets::text_in(
        painter,
        row,
        Align2::LEFT_CENTER,
        text,
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_secondary(),
    );
}

/// A read-only row: its label, and its value at the right, named "`name`: `value`" for readers.
fn info_row(ui: &Ui, painter: &egui::Painter, row: Rect, name: &str, value: &str) {
    let response = ui.interact(row, Id::unique(("clip-panel-info", name)), Sense::hover());
    let named = format!("{name}: {value}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &named));
    label(painter, row, name);
    widgets::text_in(
        painter,
        row,
        Align2::RIGHT_CENTER,
        value,
        theme::mono_font(theme::TEXT_SMALL),
        theme::text_value(),
    );
}

/// Lays rows out from the top of a part, and stops at its bottom.
struct Rows {
    left: f32,
    right: f32,
    y: f32,
    bottom: f32,
}

impl Rows {
    fn heading(&mut self, painter: &egui::Painter, text: &str) -> bool {
        if self.y + HEADING > self.bottom + 0.5 {
            return false;
        }
        let rect = Rect::from_min_max(Pos2::new(self.left, self.y), Pos2::new(self.right, self.y + HEADING));
        widgets::text_in(
            painter,
            rect,
            Align2::LEFT_BOTTOM,
            text,
            theme::ui_font(theme::TEXT_TINY),
            theme::text_hint(),
        );
        self.y += HEADING;
        true
    }

    /// The next row, or none when it would not fit whole.
    fn next(&mut self, height: f32) -> Option<Rect> {
        if self.y + height > self.bottom + 0.5 {
            return None;
        }
        let rect = Rect::from_min_max(Pos2::new(self.left, self.y), Pos2::new(self.right, self.y + height));
        self.y += height;
        Some(rect)
    }
}

/// A row's value part (after the label), less `buttons` points at its right for buttons.
fn value_part(row: Rect, buttons: f32) -> (Rect, Rect) {
    let value = Rect::from_min_max(Pos2::new(row.left() + LABEL_WIDTH + 8.0, row.top()), row.max);
    let field = Rect::from_min_max(value.min, Pos2::new(value.right() - buttons, value.bottom()));
    let rest = Rect::from_min_max(Pos2::new(field.right(), value.top()), value.max);
    (field, rest)
}

/// How tall the panel is with its groups one under another (the detail area stacked).
pub(super) fn stacked_height(info: &ClipInfo) -> f32 {
    let markers = if info.looping { 1.0 } else { 2.0 };
    let loop_rows = if info.loop_range.is_some() { 2.0 } else { 0.0 };
    // Colour, track, start, length, the markers, loop, its rows, two fades.
    let clip = HEADING + ROW_HEIGHT * (4.0 + markers + 1.0 + loop_rows + 2.0);
    let audio = HEADING + ROW_HEIGHT * 4.0;
    let block = HEADING + FADER_HEIGHT;
    clip + PAD + audio + PAD + block * 2.0
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

/// The audio clip's groups in `area` under the title row; `stacked` keeps them in one column.
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
    let rows_in = |part: Rect| Rows { left: part.left(), right: part.right(), y: part.top(), bottom: part.bottom() };
    if !stacked && area.width() >= PART_WIDTH * 3.0 {
        let width = (area.width() - PAD * 2.0) / 3.0;
        let part = |i: f32| {
            let left = area.left() + i * (width + PAD);
            Rect::from_min_max(Pos2::new(left, area.top()), Pos2::new(left + width, area.bottom()))
        };
        clip_group(ui, painter, &mut rows_in(part(0.0)), info, at, panel, actions);
        audio_group(ui, painter, &mut rows_in(part(1.0)), info, at, panel, actions);
        gain_and_pitch(ui, painter, &mut rows_in(part(2.0)), info, at, panel, actions);
        return;
    }
    let mut rows = rows_in(area);
    clip_group(ui, painter, &mut rows, info, at, panel, actions);
    rows.y += PAD;
    audio_group(ui, painter, &mut rows, info, at, panel, actions);
    rows.y += PAD;
    gain_and_pitch(ui, painter, &mut rows, info, at, panel, actions);
}

/// The Clip group: colour, where it is, its markers and its loop.
fn clip_group(
    ui: &mut Ui,
    painter: &egui::Painter,
    rows: &mut Rows,
    info: &ClipInfo,
    at: ClipRef,
    panel: &AudioPanel,
    actions: &mut Vec<Action>,
) {
    if !rows.heading(painter, "CLIP") {
        return;
    }
    // Colour: a swatch; a click opens the palette.
    if let Some(row) = rows.next(ROW_HEIGHT) {
        label(painter, row, "Colour");
        let (field, _) = value_part(row, 0.0);
        let swatch = Rect::from_min_size(
            Pos2::new(field.right() - field.height() + FIELD_GAP, field.top() + FIELD_GAP),
            Vec2::splat(field.height() - FIELD_GAP * 2.0),
        );
        let response = widgets::button_area(ui, swatch, Id::unique(("clip-panel-colour", panel.clip.0)), "Clip colour");
        let colour = panel.color.map_or(theme::text_hint(), colour_picker::color32);
        painter.rect_filled(swatch, 2.0, colour);
        painter.rect_stroke(swatch, 2.0, theme::hairline(theme::border_strong()), egui::StrokeKind::Inside);
        egui::Popup::menu(&response).show(|ui| {
            if let Some(picked) = colour_picker::swatch_row(ui, panel.color) {
                let params = json!({ "clip": panel.clip.0, "color": colour_picker::hex(picked) });
                actions.push(Action::new("clip.setColor", params));
                ui.close();
            }
            if ui.button("Track colour").clicked() {
                actions.push(Action::new("clip.setColor", json!({ "clip": panel.clip.0, "color": "none" })));
                ui.close();
            }
        });
    }
    for (name, value) in [
        ("Track", info.track.as_str()),
        (if info.scene { "Scene" } else { "Start" }, info.start.as_str()),
        ("Length", info.length.as_str()),
    ] {
        let Some(row) = rows.next(ROW_HEIGHT) else { return };
        info_row(ui, painter, row, name, value);
    }
    let set_width = if panel.playhead.is_some() { SET_WIDTH } else { 0.0 };
    let markers = Field { range: (0.0, 120_000.0), per_point: 0.0625, per_tick: 0.25, snap: quarter, horizontal: true };
    // The start marker, and the end marker of a clip that does not loop.
    let mut marker_rows = vec![("Start marker", info.markers.0, (0.0, (info.markers.1 - 0.25).max(0.0)))];
    if !info.looping {
        marker_rows.push(("End marker", info.markers.1, (info.markers.0 + 0.25, 120_000.0)));
    }
    for (name, value, range) in marker_rows {
        let Some(row) = rows.next(ROW_HEIGHT) else { return };
        label(painter, row, name);
        let (field, rest) = value_part(row, set_width);
        let id = Id::unique(("clip-panel-field", name));
        let set = number_field(ui, painter, field, id, name, value, &beats, Field { range, ..markers });
        let at_playhead = panel
            .playhead
            .filter(|_| small_button(ui, painter, rest, "Set", &format!("Set {name} to the playhead"), true));
        if let Some(v) = set.or(at_playhead.map(|p| quarter(p).clamp(range.0, range.1))) {
            actions.push(if name == "Start marker" {
                Action::new("clip.setStart", on_clip(at, json!({ "offset": v })))
            } else {
                Action::new("clip.setLength", on_clip(at, json!({ "length": v - info.markers.0 })))
            });
        }
    }
    // The loop: its switch, and where it starts and how long it is.
    let Some(row) = rows.next(ROW_HEIGHT) else { return };
    label(painter, row, "Loop");
    let (field, _) = value_part(row, 0.0);
    if switch_row(ui, painter, field, "Loop", info.looping) {
        for other in &info.others {
            actions.push(Action::new("clip.setLoop", json!({ "clip": other, "loop": !info.looping })));
        }
        actions.push(Action::new("clip.setLoop", on_clip(at, json!({ "loop": !info.looping }))));
    }
    if let Some((start, end)) = info.loop_range {
        let length = end - start;
        for name in ["Loop position", "Loop length"] {
            let Some(row) = rows.next(ROW_HEIGHT) else { return };
            label(painter, row, name);
            let (field, rest) = value_part(row, set_width);
            let id = Id::unique(("clip-panel-field", name));
            let (value, range) = if name == "Loop position" {
                (start, (0.0, 120_000.0 - length.max(0.25)))
            } else {
                (length, (0.25, 120_000.0 - start))
            };
            let set = number_field(ui, painter, field, id, name, value, &beats, Field { range, ..markers });
            let at_playhead = panel
                .playhead
                .filter(|_| small_button(ui, painter, rest, "Set", &format!("Set {name} to the playhead"), true));
            let from_playhead =
                at_playhead.map(|p| if name == "Loop position" { quarter(p) } else { quarter(p - start).max(0.25) });
            if let Some(v) = set.or(from_playhead) {
                let v = v.clamp(range.0, range.1);
                // The position moves the loop whole; the length moves its end.
                let params = if name == "Loop position" {
                    json!({ "start": v, "end": v + length })
                } else {
                    json!({ "start": start, "end": start + v })
                };
                actions.push(Action::new("clip.setLoop", on_clip(at, params)));
            }
        }
    }
    let fade = |b: f64| if b > 0.0 { beats(b) } else { "-".into() };
    for (name, value) in [("Fade in", fade(info.fades.0)), ("Fade out", fade(info.fades.1))] {
        let Some(row) = rows.next(ROW_HEIGHT) else { return };
        info_row(ui, painter, row, name, &value);
    }
}

/// The Audio group: the warp switch, the warp mode and the clip's tempo.
fn audio_group(
    ui: &mut Ui,
    painter: &egui::Painter,
    rows: &mut Rows,
    info: &ClipInfo,
    at: ClipRef,
    panel: &AudioPanel,
    actions: &mut Vec<Action>,
) {
    if !rows.heading(painter, "AUDIO") {
        return;
    }
    let Some(row) = rows.next(ROW_HEIGHT) else { return };
    label(painter, row, "Warp");
    let (field, _) = value_part(row, 0.0);
    if switch_row(ui, painter, field, "Warp", info.warped) {
        actions.push(Action::new("clip.setAudio", on_clip(at, json!({ "warp": !info.warped }))));
    }
    // The warp mode: a drop-down of the six.
    let Some(row) = rows.next(ROW_HEIGHT) else { return };
    label(painter, row, "Mode");
    let (field, _) = value_part(row, 0.0);
    let field = field.shrink2(Vec2::new(0.0, FIELD_GAP));
    let shown = WARP_MODES.iter().find(|(m, _, _)| *m == info.stretch).map_or("Smooth", |(_, _, s)| s);
    let response =
        widgets::button_area(ui, field, Id::unique(("clip-panel-mode", panel.clip.0)), format!("Mode: {shown}"));
    widgets::dropdown(ui, painter, field, &response, shown);
    egui::Popup::menu(&response).show(|ui| {
        for (mode, name, text) in WARP_MODES {
            if ui.selectable_label(mode == info.stretch, text).clicked() {
                if mode != info.stretch {
                    actions.push(Action::new("clip.setAudio", on_clip(at, json!({ "mode": name }))));
                }
                ui.close();
            }
        }
    });
    // The tempo, halved or doubled, and where it came from.
    let Some(row) = rows.next(ROW_HEIGHT) else { return };
    label(painter, row, "Tempo");
    let (field, rest) = value_part(row, HALVE_WIDTH * 2.0);
    let tempo_field = Field {
        range: (*CLIP_TEMPO_RANGE.start(), *CLIP_TEMPO_RANGE.end()),
        per_point: 0.1,
        per_tick: 0.1,
        snap: hundredth,
        horizontal: true,
    };
    let id = Id::unique(("clip-panel-field", "Tempo"));
    let value = panel.tempo.unwrap_or(f64::NAN);
    let shown_tempo = |v: f64| tempo_text(v.is_finite().then_some(v));
    let mut set = number_field(ui, painter, field, id, "Tempo", value, &shown_tempo, tempo_field)
        .map(|bpm| (bpm, TempoSource::Manual));
    let (halve, double) = (
        Rect::from_min_size(rest.min, Vec2::new(HALVE_WIDTH, rest.height())),
        Rect::from_min_size(rest.min + Vec2::new(HALVE_WIDTH, 0.0), Vec2::new(HALVE_WIDTH, rest.height())),
    );
    let known = panel.tempo.is_some();
    if small_button(ui, painter, halve, "÷2", "Halve the tempo", known) {
        set = panel.tempo.map(|t| (t / 2.0, TempoSource::Manual));
    }
    if small_button(ui, painter, double, "×2", "Double the tempo", known) {
        set = panel.tempo.map(|t| (t * 2.0, TempoSource::Manual));
    }
    // Under it: where the tempo came from, and Find, which lists what each source says.
    if let Some(row) = rows.next(ROW_HEIGHT) {
        let (field, rest) = value_part(row, SET_WIDTH + 8.0);
        let source = source_text(panel.tempo_source, info.warped);
        widgets::text_in(
            painter,
            field,
            Align2::RIGHT_CENTER,
            source,
            theme::ui_font(theme::TEXT_TINY),
            theme::text_hint(),
        );
        let find = rest.shrink2(Vec2::new(1.0, FIELD_GAP));
        let response = widgets::button_area(ui, find, Id::unique(("clip-panel-find", panel.clip.0)), "Find tempo");
        painter.fill_background(
            find,
            3.0,
            motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response)),
        );
        widgets::text_in(
            painter,
            find,
            Align2::CENTER_CENTER,
            "Find",
            theme::ui_font(theme::TEXT_SMALL),
            theme::text_primary(),
        );
        let open = egui::Popup::menu(&response).show(|ui| {
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
                        if ui.add_enabled(bpm.is_some(), egui::Button::new(shown)).clicked() {
                            set = bpm.map(|b| (b, source));
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
            }
        });
    }
    if let Some((bpm, source)) = set {
        let bpm = bpm.clamp(*CLIP_TEMPO_RANGE.start(), *CLIP_TEMPO_RANGE.end());
        actions.push(Action::new("clip.setTempo", on_clip(at, json!({ "bpm": bpm, "source": source.name() }))));
    }
}

/// The key under which the clip panel asks for the tempo sources of a clip (its Find menu open).
pub fn find_tempo_key() -> Id {
    Id::unique("clip-panel-find-tempo")
}

/// The Gain and Pitch groups: the fader and the knob, side by side when there is room.
fn gain_and_pitch(
    ui: &mut Ui,
    painter: &egui::Painter,
    rows: &mut Rows,
    info: &ClipInfo,
    at: ClipRef,
    panel: &AudioPanel,
    actions: &mut Vec<Action>,
) {
    // As tall as there is room for, between three rows (the readouts and the switch) and a full
    // fader.
    let least = HEADING + ROW_HEIGHT * 3.0 + 4.0;
    let block = (rows.bottom - rows.y).clamp(least, HEADING + FADER_HEIGHT);
    let wide = rows.right - rows.left >= 2.0 * (FADER_WIDTH + 70.0);
    let (gain_part, pitch_part) = if wide {
        let middle = (rows.left + rows.right) / 2.0;
        let Some(both) = rows.next(block) else { return };
        (
            Rect::from_min_max(both.min, Pos2::new(middle - PAD / 2.0, both.bottom())),
            Rect::from_min_max(Pos2::new(middle + PAD / 2.0, both.top()), both.max),
        )
    } else {
        let Some(gain) = rows.next(block) else { return };
        let pitch = rows.next(block);
        (gain, pitch.unwrap_or(Rect::NOTHING))
    };
    gain_group(ui, painter, gain_part, info, at, panel, actions);
    if pitch_part.is_positive() {
        pitch_group(ui, painter, pitch_part, info, at, actions);
    }
}

fn heading_in(painter: &egui::Painter, part: Rect, text: &str) -> Rect {
    let rect = Rect::from_min_size(part.min, Vec2::new(part.width(), HEADING));
    widgets::text_in(painter, rect, Align2::LEFT_BOTTOM, text, theme::ui_font(theme::TEXT_TINY), theme::text_hint());
    Rect::from_min_max(Pos2::new(part.left(), part.top() + HEADING), part.max)
}

fn gain_group(
    ui: &mut Ui,
    painter: &egui::Painter,
    part: Rect,
    info: &ClipInfo,
    at: ClipRef,
    panel: &AudioPanel,
    actions: &mut Vec<Action>,
) {
    let body = heading_in(painter, part, "GAIN");
    let db = info.gain_db.filter(|d| d.is_finite()).unwrap_or(GAIN_RANGE.0).max(GAIN_RANGE.0);
    // The fader: dragged up and down, one undo step when let go; a double-click puts it at 0 dB.
    let track = Rect::from_min_max(
        Pos2::new(body.left() + 4.0, body.top() + 4.0),
        Pos2::new(body.left() + 4.0 + FADER_WIDTH, body.bottom() - 4.0),
    );
    let id = Id::unique(("clip-panel-fader", panel.clip.0));
    let response = ui.interact(track, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeVertical);
    let preview_id = id.with("preview");
    let preview = ui.data(|d| d.get_temp::<f64>(preview_id));
    let shown_db = preview.map_or(db, tenth);
    let label = format!("Gain fader: {}", gain_text(shown_db));
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::Slider, true, &label);
        info.value = Some(shown_db);
        info
    });
    let mut set = None;
    if response.double_clicked() {
        ui.data_mut(|d| d.remove::<f64>(preview_id));
        set = Some(0.0);
    } else if response.dragged_by(egui::PointerButton::Primary) {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let delta = -f64::from(widgets::value_drag_delta(ui, &response).y) / f64::from(track.height()) * fine;
        let from = fader_position(preview.unwrap_or(db));
        ui.data_mut(|d| d.insert_temp(preview_id, fader_db(from + delta)));
    } else if response.drag_stopped_by(egui::PointerButton::Primary) {
        set = preview.map(tenth);
        ui.data_mut(|d| d.remove::<f64>(preview_id));
    }
    // Drawn: the slot, the 0 dB mark, the cap.
    let slot = Rect::from_center_size(track.center(), Vec2::new(4.0, track.height()));
    painter.fill_background(slot, 2.0, theme::control_background());
    let y_of = |db: f64| track.bottom() - fader_position(db) as f32 * track.height();
    painter.hline(track.x_range(), y_of(0.0), theme::hairline(theme::text_hint()));
    let cap = Rect::from_center_size(Pos2::new(track.center().x, y_of(shown_db)), Vec2::new(FADER_WIDTH, 8.0));
    let hover = widgets::hover(ui, &response);
    crate::kit::mixer_controls::fader_cap(painter, cap, hover, crate::kit::skin::State::response(&response, false));
    // Beside it, the readout (typed), and the envelope switch under it.
    let side = Rect::from_min_max(
        Pos2::new(track.right() + PAD, body.top()),
        Pos2::new(body.right(), body.top() + ROW_HEIGHT),
    );
    let gain_field = Field { range: GAIN_RANGE, per_point: 0.1, per_tick: 1.0, snap: tenth, horizontal: false };
    if let Some(v) =
        number_field(ui, painter, side, Id::unique(("clip-panel-field", "Gain")), "Gain", db, &gain_text, gain_field)
    {
        set = Some(v);
    }
    if let Some(v) = set.filter(|v| (v - db).abs() > 1e-9 || info.gain_db.is_none_or(|g| !g.is_finite())) {
        let db = commands::exact_db_param(v, GAIN_RANGE.0);
        actions.push(Action::new("clip.setGain", on_clip(at, json!({ "db": db }))));
    }
    let envelope = side.translate(Vec2::new(0.0, ROW_HEIGHT + 2.0));
    let arranged = matches!(at, ClipRef::Arrangement(..));
    let on = arranged && crate::arrangement::envelope::shown(ui.ctx()) == Some(panel.clip);
    if arranged && switch_row(ui, painter, envelope, "Envelope", on) {
        crate::arrangement::envelope::set_shown(ui.ctx(), (!on).then_some(panel.clip));
    }
    if panel.envelope_points > 0 {
        let count = envelope.translate(Vec2::new(0.0, ROW_HEIGHT));
        let text = if panel.envelope_points == 1 {
            "1 point".to_string()
        } else {
            format!("{} points", panel.envelope_points)
        };
        widgets::text_in(
            painter,
            count,
            Align2::CENTER_CENTER,
            &text,
            theme::ui_font(theme::TEXT_TINY),
            theme::text_hint(),
        );
    }
}

fn pitch_group(
    ui: &mut Ui,
    painter: &egui::Painter,
    part: Rect,
    info: &ClipInfo,
    at: ClipRef,
    actions: &mut Vec<Action>,
) {
    let body = heading_in(painter, part, "PITCH");
    let (semitones, cents) = split_transpose(info.transpose);
    // The knob: whole semitones, dragged up and down; a double-click puts it at 0.
    let center = Pos2::new(body.left() + KNOB / 2.0 + 4.0, body.top() + KNOB / 2.0 + 4.0);
    let rect = Rect::from_center_size(center, Vec2::splat(KNOB));
    let id = Id::unique(("clip-panel-knob", "pitch"));
    let response = ui.interact(rect, id, Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::ResizeVertical);
    let preview_id = id.with("preview");
    let preview = ui.data(|d| d.get_temp::<f64>(preview_id));
    let shown = preview.map_or(semitones, whole);
    let label = format!("Pitch: {}", semitones_text(shown));
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::Slider, true, &label);
        info.value = Some(shown);
        info
    });
    let mut set_semitones = None;
    let mut set_cents = None;
    if response.double_clicked() {
        ui.data_mut(|d| d.remove::<f64>(preview_id));
        set_semitones = Some(0.0);
    } else if response.dragged_by(egui::PointerButton::Primary) {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let delta = -f64::from(widgets::value_drag_delta(ui, &response).y) * 0.25 * fine;
        let next = (preview.unwrap_or(semitones) + delta).clamp(SEMITONES.0, SEMITONES.1);
        ui.data_mut(|d| d.insert_temp(preview_id, next));
    } else if response.drag_stopped_by(egui::PointerButton::Primary) {
        set_semitones = preview.map(whole);
        ui.data_mut(|d| d.remove::<f64>(preview_id));
    }
    let position = ((shown - SEMITONES.0) / (SEMITONES.1 - SEMITONES.0)) as f32;
    widgets::knob(painter, center, KNOB, position, theme::accent(), Some(&response));
    // Beside it the semitones typed, and under them the cents.
    let side =
        Rect::from_min_max(Pos2::new(rect.right() + PAD, body.top()), Pos2::new(body.right(), body.top() + ROW_HEIGHT));
    let st_field = Field { range: SEMITONES, per_point: 0.25, per_tick: 1.0, snap: whole, horizontal: false };
    if let Some(v) = number_field(
        ui,
        painter,
        side,
        Id::unique(("clip-panel-field", "Semitones")),
        "Semitones",
        semitones,
        &semitones_text,
        st_field,
    ) {
        set_semitones = Some(v);
    }
    let cents_row = side.translate(Vec2::new(0.0, ROW_HEIGHT + 2.0));
    let cents_field = Field { range: CENTS, per_point: 0.5, per_tick: 1.0, snap: whole, horizontal: false };
    if let Some(v) = number_field(
        ui,
        painter,
        cents_row,
        Id::unique(("clip-panel-field", "Cents")),
        "Cents",
        cents,
        &cents_text,
        cents_field,
    ) {
        set_cents = Some(v);
    }
    if set_semitones.is_some() || set_cents.is_some() {
        let transpose = set_semitones.unwrap_or(semitones) + set_cents.unwrap_or(cents) / 100.0;
        let transpose = (transpose * 100.0).round() / 100.0;
        if (transpose - info.transpose).abs() > 1e-9 {
            actions.push(Action::new("clip.setAudio", on_clip(at, json!({ "transpose": transpose }))));
        }
    }
}
