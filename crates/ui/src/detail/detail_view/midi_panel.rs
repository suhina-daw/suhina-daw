// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A MIDI clip's panel in the clip column (`docs/spec/ui-clip-editor.md` › Clip column and › Note
//! tools): the Clip part it shares with an audio clip's panel with what the tools act on in
//! figures under it, and beside them (under them when the detail area is stacked) the tools part
//! with its Notes and Transform tabs. Every tool is one `note.*` command over the selection or the
//! whole clip; `.midi-tools` styles the tools part.

use super::clip_panel::{
    CLIP_PART, Change, FIELD, Field, GAP, NAME, TOGGLE, Text, bar_of, clip_part, drop_down, name_over, number_field,
    pair, plain_number, set_button, small_button, toggle, whole,
};
use super::*;
use crate::detail::midi_tools::{Tools, ToolsTab, Transform, tools_width};
use crate::detail::piano_roll::{PianoRollState, QUANTIZE_GRIDS};
use suhina_engine::model::{ROOT_NAMES, ScaleMode};

/// What a MIDI clip's panel needs from the piano roll: what the tools act on, the piano roll's
/// state (which keeps their settings, tab and transformation), and whether the tools part shows
/// (the toolbar's three-slider button). The figures under the Clip part show either way.
pub struct MidiTools<'a, 'b> {
    pub tools: &'a Tools<'b>,
    pub state: &'a mut PianoRollState,
    pub show_tools: bool,
}

/// The tab row at the top of the tools part, the chip at the top of the figures, a row of the
/// figures, and the bar under the velocity figures.
const TAB_ROW: f32 = 22.0;
const CHIP: f32 = 18.0;
const INFO_ROW: f32 = 14.0;
const RANGE_BAR: f32 = 10.0;
/// The space between the buttons of the Notes tab's grid.
const BUTTON_GAP: f32 = 4.0;
/// How tall the tools part is under its tab row: its taller tab, Transform (the list, the hint,
/// two fields, a switch and Apply).
const TOOLS_BODY: f32 = FIELD + GAP + 12.0 + GAP + NAME + FIELD + GAP + TOGGLE + GAP + TOGGLE;
/// How tall the figures are at most: the chip, Pitch, From, To, Muted, Velocity and its bar.
const FIGURES: f32 = CHIP + GAP + INFO_ROW * 4.0 + 4.0 + INFO_ROW + RANGE_BAR;
/// The room between the Clip part and the figures under it.
const FIGURES_GAP: f32 = 12.0;

/// The panel's width in a row: the Clip part with the figures under it, and the tools part beside
/// them when it is shown.
pub(super) fn row_width(tools: bool) -> f32 {
    let tools = if tools { TOOLS_GAP + tools_width() } else { 0.0 };
    CLIP_PART + tools + 16.0
}

/// The room between the Clip part and the tools part: a little less than the audio panel's, so
/// the panel is no wider than the clip column and the tools column were together and the piano
/// roll's toolbar keeps its menus at 1280 points.
const TOOLS_GAP: f32 = 8.0;

/// How tall the panel is with its parts one under another (the detail area stacked), under its
/// title row: the Clip part, and with `midi` the figures and, with `tools`, the tools part.
pub(super) fn stacked_height(midi: bool, tools: bool) -> f32 {
    let clip = (NAME + FIELD) * 3.0 + TOGGLE + GAP * 3.0;
    let figures = if midi { FIGURES_GAP + FIGURES } else { 0.0 };
    let tools = if midi && tools { 14.0 + TAB_ROW + GAP + TOOLS_BODY } else { 0.0 };
    clip + figures + tools + 16.0
}

/// The MIDI clip's parts in `area` under the title row: the Clip part with the figures under it,
/// and the tools part beside them in a row, or all one under another when `stacked`. Without
/// `midi` (the clip's editor is not shown) only the Clip part is drawn.
#[allow(clippy::too_many_arguments)]
pub(super) fn midi_panel(
    ui: &mut Ui,
    painter: &egui::Painter,
    area: Rect,
    info: &ClipInfo,
    at: ClipRef,
    panel: &AudioPanel,
    stacked: bool,
    midi: Option<MidiTools<'_, '_>>,
    actions: &mut Vec<Action>,
) {
    let area = area.shrink(8.0);
    let clip =
        if stacked { area } else { Rect::from_min_max(area.min, Pos2::new(area.left() + CLIP_PART, area.bottom())) };
    let end = clip_part(ui, painter, clip, info, at, panel, actions);
    let Some(midi) = midi else { return };
    // The figures stay under the Clip part at any height, their rows left out whole where they do
    // not fit, so the panel keeps its width while the detail area is resized.
    let figures_top = end + FIGURES_GAP;
    let figures_end = if figures_top < clip.bottom() {
        let part = Rect::from_min_max(Pos2::new(clip.left(), figures_top), clip.max);
        figures(ui, painter, part, panel, midi.tools)
    } else {
        figures_top
    };
    if !midi.show_tools {
        return;
    }
    let part = if stacked {
        Rect::from_min_max(Pos2::new(area.left(), figures_end + 14.0), area.max)
    } else {
        let left = clip.right() + TOOLS_GAP;
        Rect::from_min_max(
            Pos2::new(left, area.top()),
            Pos2::new((left + tools_width()).min(area.right()), area.bottom()),
        )
    };
    tools_part(ui, painter, part, panel, midi, actions);
}

/// The tools part: its tab row, and the tab shown.
fn tools_part(
    ui: &mut Ui,
    painter: &egui::Painter,
    part: Rect,
    panel: &AudioPanel,
    midi: MidiTools<'_, '_>,
    actions: &mut Vec<Action>,
) {
    if part.height() < TAB_ROW || part.width() < 40.0 {
        return;
    }
    let MidiTools { tools, state, .. } = midi;
    // `.midi-tools` styles the part: its background and, through the scope, its controls' text.
    let _style = crate::kit::style_class::enter("midi-tools");
    crate::kit::native_css::surface(painter, part, 0.0, theme::panel_background());
    let tab_row = Rect::from_min_size(part.min, Vec2::new(part.width(), TAB_ROW));
    tab_row_of(ui, painter, tab_row, state);
    let body = Rect::from_min_max(Pos2::new(part.left(), tab_row.bottom() + GAP), part.max);
    // A time selection with no note in it gives the tools nothing to act on.
    let before = actions.len();
    match state.tools_tab {
        ToolsTab::Notes => notes_tab(ui, painter, body, panel, tools, state, actions),
        ToolsTab::Transform => transform_tab(ui, painter, body, tools, state, actions),
    }
    if tools.nothing {
        actions.truncate(before);
    }
    // Escape leaves Quantise for the Notes tab, as it closed the quantise block before (Escape also
    // clears the note selection in the piano roll; both happen, as both are what the key means).
    if state.quantize_open() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.set_quantize_open(false);
    }
}

/// The tab row: the Notes and Transform tabs (the one shown `text.primary` over a 2 px `accent`
/// line, the other `text.hint`).
fn tab_row_of(ui: &Ui, painter: &egui::Painter, row: Rect, state: &mut PianoRollState) {
    painter.hline(row.x_range(), row.bottom() - 0.5, theme::hairline(theme::border()));
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mut x = row.left();
    for (tab, text) in [(ToolsTab::Notes, "Notes"), (ToolsTab::Transform, "Transform")] {
        let width = (widgets::text_width(ui, text, font.clone()) + 16.0).min((row.right() - x).max(0.0));
        let cell = Rect::from_min_max(Pos2::new(x, row.top()), Pos2::new(x + width, row.bottom()));
        x = cell.right() + 2.0;
        let response = widgets::button_area(ui, cell, Id::unique(("midi-tools-tab", text)), format!("{text} tab"));
        // `.editor-tab` (`:checked` for the one shown), `#tab-notes` and `#tab-transform`.
        let _style = crate::kit::style_class::enter_named_response(
            "editor-tab",
            &format!("tab-{}", text.to_ascii_lowercase()),
            &response,
        );
        let on = state.tools_tab == tab;
        crate::kit::native_css::mark(crate::kit::native_css::HostState::Checked, on);
        let ink = if on {
            theme::text_primary()
        } else {
            motion::mix(theme::text_hint(), theme::text_secondary(), widgets::hover(ui, &response))
        };
        let ink = crate::kit::style_class::foreground(ink);
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, text, font.clone(), ink);
        if on {
            let line = Rect::from_min_max(
                Pos2::new(cell.left() + 4.0, row.bottom() - 2.0),
                Pos2::new(cell.right() - 4.0, row.bottom()),
            );
            painter.rect_filled(line, 0.0, theme::accent());
        }
        if response.clicked() && !on {
            state.tools_tab = tab;
        }
    }
}

/// A button of the tools, drawn as the panel's small buttons are; returns its response.
fn tool_button(ui: &Ui, painter: &egui::Painter, rect: Rect, text: &str, label: &str, enabled: bool) -> egui::Response {
    let response = widgets::button_area(ui, rect, Id::unique(("midi-tools-button", label)), label);
    let fill = if enabled {
        motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response))
    } else {
        theme::panel_background()
    };
    painter.fill_background(rect, 3.0, fill);
    painter.rect_stroke(rect, 3.0, theme::hairline(theme::border()), egui::StrokeKind::Inside);
    let ink = if enabled { theme::text_primary() } else { theme::text_hint() };
    widgets::text_in(painter, rect, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_SMALL), ink);
    response
}

/// A whole number with its sign: `+3`, `0`, `-12`.
fn signed(value: f64) -> String {
    let value = value.round();
    if value > 0.0 { format!("+{value}") } else { format!("{value}") }
}

/// A field of the tools that sets one of their settings (no command): dragged, stepped or typed.
#[allow(clippy::too_many_arguments)]
fn setting_field(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    name: &str,
    value: f64,
    range: (f64, f64),
    shown: &dyn Fn(f64) -> String,
) -> Option<f64> {
    let field = Field { range, per_point: 0.25, per_tick: 1.0, snap: whole, align: Align2::RIGHT_CENTER, dim: false };
    let parse = |t: &str| plain_number(t);
    let text = Text { shown, parse: &parse, places: None };
    let id = Id::unique(("midi-tools-field", name));
    number_field(ui, painter, rect, id, name, value, text, field).map(|Change { value, .. }| value)
}

/// A hover area over a control's name, with a sentence on what the control does.
fn explain(ui: &Ui, rect: Rect, id: &str, text: &str) {
    let _ = ui.interact(rect, Id::unique(("midi-tools-hint", id)), Sense::hover()).on_hover_text(text);
}

/// The Notes tab: Transpose and the one-click tools, two to a row.
#[allow(clippy::too_many_arguments)]
fn notes_tab(
    ui: &mut Ui,
    painter: &egui::Painter,
    body: Rect,
    panel: &AudioPanel,
    tools: &Tools<'_>,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
) {
    let left = body;
    let enabled = tools.count() > 0;
    let mut y = left.top();
    let fits = |y: f32, height: f32| y + height <= body.bottom() + 0.5;
    // Transpose: its semitones, and Apply at the right of its name.
    if fits(y, NAME + FIELD) {
        let name = Rect::from_min_size(Pos2::new(left.left(), y), Vec2::new(left.width(), NAME));
        name_over(painter, name, "Transpose", Align2::LEFT_BOTTOM);
        if small_button(ui, painter, set_button(name), "Apply", "Transpose", enabled) {
            let mut params = tools.params();
            params["semitones"] = json!(state.tools.transpose.clamp(-24, 24));
            state.replacing(&mut params);
            actions.push(Action::new("note.move", params));
        }
        let field = Rect::from_min_size(Pos2::new(left.left(), y + NAME), Vec2::new(left.width(), FIELD));
        let shown = |v: f64| format!("{} st", signed(v));
        let value = f64::from(state.tools.transpose);
        if let Some(v) = setting_field(ui, painter, field, "Transpose by", value, (-24.0, 24.0), &shown) {
            state.tools.transpose = v.round() as i32;
        }
        y += NAME + FIELD + GAP;
    }
    // The one-click tools, two to a row.
    let cell = (left.width() - BUTTON_GAP) / 2.0;
    let rows: [[&str; 2]; 3] = [["Fit to scale", "Invert"], ["Reverse", "Legato"], ["×2", "÷2"]];
    for pair_of in rows {
        if !fits(y, FIELD) {
            break;
        }
        for (column, name) in pair_of.into_iter().enumerate() {
            let rect = Rect::from_min_size(
                Pos2::new(left.left() + column as f32 * (cell + BUTTON_GAP), y),
                Vec2::new(cell, FIELD),
            );
            let response = tool_button(ui, painter, rect, name, name, enabled);
            if name == "Fit to scale" && enabled && panel.scale.is_none() {
                // No scale on the clip: the list of scales and roots; picking one sets the clip's
                // scale and fits the notes to it.
                fit_to_a_new_scale(&response, tools, actions);
                continue;
            }
            if !(enabled && response.clicked()) {
                continue;
            }
            let mut params = tools.params();
            // With stacked notes off, every tool's notes replace what they land on (› Allow
            // stacked notes), as a move's do.
            state.replacing(&mut params);
            let id = match name {
                "Fit to scale" => "note.fitToScale",
                "Invert" => "note.invert",
                "Reverse" => "note.reverse",
                "Legato" => {
                    params["until"] = json!(tools.legato_until(state.span_end()));
                    "note.legato"
                }
                "×2" => {
                    params["factor"] = json!(2.0);
                    "note.stretch"
                }
                _ => {
                    params["factor"] = json!(0.5);
                    "note.stretch"
                }
            };
            actions.push(Action::new(id, params));
        }
        y += FIELD + BUTTON_GAP;
    }
}

/// Fit to scale for a clip with no scale: a list of the scales, each with its roots; picking one
/// sets the clip's scale (`clip.setScale`) and fits the notes to it (`note.fitToScale`).
fn fit_to_a_new_scale(response: &egui::Response, tools: &Tools<'_>, actions: &mut Vec<Action>) {
    egui::Popup::menu(response).show(|ui| {
        ui.label(egui::RichText::new("Set the clip's scale").font(theme::ui_font(theme::TEXT_TINY)));
        for mode in ScaleMode::ALL {
            ui.menu_button(mode.label(), |ui| {
                for (index, name) in ROOT_NAMES.iter().enumerate() {
                    let root = crate::detail::piano_roll::pitch_class_name(u8::try_from(index).unwrap_or(0));
                    if ui.button(root).clicked() {
                        let mut scale = tools.address.clone();
                        scale["root"] = json!(name);
                        scale["mode"] = json!(mode.name());
                        actions.push(Action::new("clip.setScale", scale));
                        let mut params = tools.params();
                        params["root"] = json!(name);
                        params["scale"] = json!(mode.name());
                        actions.push(Action::new("note.fitToScale", params));
                        ui.close();
                    }
                }
            });
        }
    });
}

/// What the tools act on, in figures, under the Clip part: a chip with what they act on and how
/// many notes that is (lit while notes are selected), then the notes' keys, where they start and
/// end, how many are muted, and their velocities as numbers and a bar of 1 to 127. Each row is
/// named "`Name`: `value`" for a reader and a test; rows that do not fit are left out whole.
/// Returns where the figures end.
fn figures(ui: &Ui, painter: &egui::Painter, part: Rect, panel: &AudioPanel, tools: &Tools<'_>) -> f32 {
    let mut y = part.top();
    if y + CHIP > part.bottom() + 0.5 {
        return y;
    }
    chip(ui, painter, Rect::from_min_size(part.min, Vec2::new(part.width(), CHIP)), tools);
    y += CHIP + GAP;
    let all: std::collections::BTreeSet<usize> = (0..tools.notes.len()).collect();
    let acted = if tools.selected.is_empty() { &all } else { tools.selected };
    let info = SelectionInfo::of(tools.notes, acted, bar_of(panel));
    let key_name = crate::detail::piano_roll::key_name;
    let pitch = match (info.low_key, info.high_key) {
        (Some(low), Some(high)) if low == high => Some(key_name(low)),
        (Some(low), Some(high)) => Some(format!("{}–{}", key_name(low), key_name(high))),
        _ => None,
    };
    let velocity = match (info.low_velocity, info.high_velocity) {
        (Some(low), Some(high)) if low == high => Some(low.to_string()),
        (Some(low), Some(high)) => Some(format!("{low}–{high}")),
        _ => None,
    };
    let muted = (info.muted > 0).then(|| info.muted.to_string());
    let rows = [("Pitch", pitch), ("From", info.from.clone()), ("To", info.to.clone()), ("Muted", muted)];
    for (name, value) in rows {
        let Some(value) = value else { continue };
        if y + INFO_ROW > part.bottom() + 0.5 {
            return y;
        }
        let row = Rect::from_min_size(Pos2::new(part.left(), y), Vec2::new(part.width(), INFO_ROW));
        figure(ui, painter, row, name, &value);
        y += INFO_ROW;
    }
    let (Some(velocity), Some(low), Some(high)) = (velocity, info.low_velocity, info.high_velocity) else { return y };
    if y + 4.0 + INFO_ROW + RANGE_BAR > part.bottom() + 0.5 {
        return y;
    }
    y += 4.0;
    let row = Rect::from_min_size(Pos2::new(part.left(), y), Vec2::new(part.width(), INFO_ROW));
    figure(ui, painter, row, "Velocity", &velocity);
    y += INFO_ROW;
    let bar = Rect::from_min_size(Pos2::new(part.left(), y), Vec2::new(part.width(), RANGE_BAR));
    painter.fill_background(bar, 3.0, theme::control_background());
    let x_of = |v: u8| bar.left() + 2.0 + (f32::from(v.saturating_sub(1)) / 126.0) * (bar.width() - 4.0);
    let span = Rect::from_min_max(
        Pos2::new(x_of(low), bar.top() + 3.0),
        Pos2::new(x_of(high).max(x_of(low) + 3.0), bar.bottom() - 3.0),
    );
    painter.rect_filled(span, 2.0, theme::accent());
    bar.bottom()
}

/// The head of the figures: what the tools act on (**Whole clip**, **Selection**, **No notes in
/// time**) at the left, how many notes that is at the right, and a hairline under them, as a
/// section's title. A selection lights the head in the accent; it is a heading, not a button.
fn chip(ui: &Ui, painter: &egui::Painter, rect: Rect, tools: &Tools<'_>) {
    let head = tools.head();
    let count = tools.count_text();
    let selected = !tools.selected.is_empty();
    let (head_ink, count_ink) =
        if selected { (theme::accent(), theme::text_primary()) } else { (theme::text_hint(), theme::text_secondary()) };
    let tiny = theme::ui_font(theme::TEXT_TINY);
    let mono = theme::mono_font(theme::TEXT_TINY);
    let count_width = widgets::text_width(ui, &count, mono.clone());
    let count_rect = Rect::from_min_max(Pos2::new(rect.right() - count_width, rect.top()), rect.max);
    let head_rect = Rect::from_min_max(rect.min, Pos2::new(count_rect.left() - 6.0, rect.bottom()));
    widgets::text_in(painter, head_rect, Align2::LEFT_CENTER, head, tiny, head_ink);
    widgets::text_in(painter, count_rect, Align2::RIGHT_CENTER, &count, mono, count_ink);
    let line = rect.bottom() - 0.5;
    painter
        .line_segment([Pos2::new(rect.left(), line), Pos2::new(rect.right(), line)], theme::hairline(theme::border()));
    // The head and the count are named for a reader and a test, as the piano roll's parts are.
    ui.interact(head_rect, Id::unique("piano-roll-tools-head"), Sense::hover())
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, head));
    ui.interact(count_rect, Id::unique("piano-roll-tools-count"), Sense::hover())
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &count));
}

/// One row of [`figures`]: its name at the left, its value at the right.
fn figure(ui: &Ui, painter: &egui::Painter, row: Rect, name: &str, value: &str) {
    let response = ui.interact(row, Id::unique(("midi-tools-figure", name)), Sense::hover());
    let label = format!("{name}: {value}");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &label));
    widgets::text_in(
        painter,
        row,
        Align2::LEFT_CENTER,
        name,
        theme::ui_font(theme::TEXT_TINY),
        theme::text_secondary(),
    );
    widgets::text_in(
        painter,
        row,
        Align2::RIGHT_CENTER,
        value,
        theme::mono_font(theme::TEXT_SMALL),
        theme::text_value(),
    );
}

/// The Transform tab, one under another: the list of transformations, a line on what the chosen one
/// does, its settings, and Apply, which says what it does and to what ("Humanise 3 notes").
fn transform_tab(
    ui: &mut Ui,
    painter: &egui::Painter,
    body: Rect,
    tools: &Tools<'_>,
    state: &mut PianoRollState,
    actions: &mut Vec<Action>,
) {
    if body.height() < FIELD {
        return;
    }
    let chosen = state.transform;
    let list = Rect::from_min_size(body.min, Vec2::new(body.width(), FIELD));
    let entries: Vec<Option<(&str, bool)>> = Transform::ALL
        .iter()
        .flat_map(|t| {
            let line = (*t == Transform::RandomiseVelocity).then_some(None);
            line.into_iter().chain([Some((t.name(), *t == chosen))])
        })
        .collect();
    let id = Id::unique("midi-tools-transform");
    if let Some(index) =
        drop_down(ui, painter, list, id, "Transformation", chosen.name(), &entries, list.width().max(150.0))
    {
        let picked = entries.iter().take(index + 1).flatten().count().checked_sub(1);
        if let Some(t) = picked.and_then(|i| Transform::ALL.get(i)) {
            state.transform = *t;
        }
    }
    let mut y = body.top() + FIELD + GAP;
    // What it does, in a line.
    let hint = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), 12.0));
    if hint.bottom() <= body.bottom() + 0.5 {
        widgets::text_in(
            painter,
            hint,
            Align2::LEFT_CENTER,
            chosen.hint(),
            theme::ui_font(theme::TEXT_TINY),
            theme::text_hint(),
        );
        y = hint.bottom() + GAP;
    }
    let settings_fit = y + NAME + FIELD <= body.bottom() + 0.5;
    let settings = &mut state.tools;
    let mut params = tools.params();
    match chosen {
        Transform::Humanise => {
            if settings_fit {
                let [(time_name, time_field), (velocity_name, velocity_field)] = pair(body, y);
                name_over(painter, time_name, "Timing ±", Align2::LEFT_BOTTOM);
                name_over(painter, velocity_name, "Velocity ±", Align2::LEFT_BOTTOM);
                explain(ui, time_name, "timing", "How far a note may start earlier or later, in milliseconds");
                explain(ui, velocity_name, "velocity", "How far a note's velocity may move up or down");
                let ms = |v: f64| format!("{v} ms");
                if let Some(v) =
                    setting_field(ui, painter, time_field, "Timing", settings.humanize_ms, (0.0, 50.0), &ms)
                {
                    settings.humanize_ms = v;
                }
                let steps = |v: f64| format!("{v}");
                if let Some(v) = setting_field(
                    ui,
                    painter,
                    velocity_field,
                    "Velocity",
                    settings.humanize_velocity,
                    (0.0, 127.0),
                    &steps,
                ) {
                    settings.humanize_velocity = v;
                }
                y += NAME + FIELD + GAP;
                if y + TOGGLE <= body.bottom() + 0.5 {
                    let row = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), TOGGLE));
                    y += TOGGLE + GAP;
                    explain(
                        ui,
                        row,
                        "chords",
                        "Notes that start within 30 ms of each other move together, as one chord",
                    );
                    if toggle(
                        ui,
                        painter,
                        row,
                        "Keep chords together",
                        "Keep chords together",
                        settings.humanize_chords,
                    ) {
                        settings.humanize_chords = !settings.humanize_chords;
                    }
                }
            }
            params["ms"] = json!(settings.humanize_ms.clamp(0.0, 50.0));
            params["velocity"] = json!(settings.humanize_velocity.clamp(0.0, 127.0));
            params["chords"] = json!(settings.humanize_chords);
        }
        Transform::Quantise => {
            let mut quantize = state.quantize;
            if settings_fit {
                let [(grid_name, grid_field), (amount_name, amount_field)] = pair(body, y);
                name_over(painter, grid_name, "Grid", Align2::LEFT_BOTTOM);
                name_over(painter, amount_name, "Amount", Align2::LEFT_BOTTOM);
                let grids: Vec<Option<(&str, bool)>> =
                    QUANTIZE_GRIDS.iter().map(|g| Some((*g, *g == quantize.grid))).collect();
                let id = Id::unique("midi-tools-quantise-grid");
                if let Some(index) =
                    drop_down(ui, painter, grid_field, id, "Quantise grid", quantize.grid, &grids, grid_field.width())
                    && let Some(grid) = QUANTIZE_GRIDS.get(index)
                {
                    quantize.grid = grid;
                }
                let percent = |v: f64| format!("{v} %");
                if let Some(v) =
                    setting_field(ui, painter, amount_field, "Quantise amount", quantize.amount, (0.0, 100.0), &percent)
                {
                    quantize.amount = v;
                }
                y += NAME + FIELD + GAP;
                if y + TOGGLE <= body.bottom() + 0.5 {
                    let row = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), TOGGLE));
                    y += TOGGLE + GAP;
                    if toggle(ui, painter, row, "Move ends too", "Move ends too", quantize.ends) {
                        quantize.ends = !quantize.ends;
                    }
                }
            }
            state.quantize = quantize;
            params["grid"] = json!(quantize.grid);
            params["amount"] = json!(quantize.amount);
            params["ends"] = json!(quantize.ends);
            state.replacing(&mut params);
        }
        Transform::RandomiseVelocity => {
            if settings_fit {
                let [(name, field), _] = pair(body, y);
                name_over(painter, name, "Velocity ±", Align2::LEFT_BOTTOM);
                let steps = |v: f64| format!("{v}");
                if let Some(v) =
                    setting_field(ui, painter, field, "Randomise amount", settings.random_amount, (0.0, 127.0), &steps)
                {
                    settings.random_amount = v;
                }
                y += NAME + FIELD + GAP;
            }
            params["amount"] = json!(settings.random_amount.clamp(0.0, 127.0));
        }
        Transform::RampVelocity => {
            if settings_fit {
                let [(from_name, from_field), (to_name, to_field)] = pair(body, y);
                name_over(painter, from_name, "From", Align2::LEFT_BOTTOM);
                name_over(painter, to_name, "To", Align2::LEFT_BOTTOM);
                let steps = |v: f64| format!("{v}");
                let from = f64::from(settings.ramp_from);
                if let Some(v) = setting_field(ui, painter, from_field, "Ramp from", from, (1.0, 127.0), &steps) {
                    settings.ramp_from = v.round() as i32;
                }
                let to = f64::from(settings.ramp_to);
                if let Some(v) = setting_field(ui, painter, to_field, "Ramp to", to, (1.0, 127.0), &steps) {
                    settings.ramp_to = v.round() as i32;
                }
                y += NAME + FIELD + GAP;
            }
            params["from"] = json!(settings.ramp_from.clamp(1, 127));
            params["to"] = json!(settings.ramp_to.clamp(1, 127));
        }
    }
    // Apply, at the bottom, says what it does and to what: "Humanise 3 notes", "Quantise clip".
    if y + TOGGLE > body.bottom() + 0.5 {
        return;
    }
    let apply_text = format!("{} {}", chosen.verb(), tools.target());
    let apply = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), TOGGLE));
    let enabled = tools.count() > 0;
    let response =
        widgets::button_area(ui, apply, Id::unique("midi-tools-apply"), &apply_text).on_hover_text(chosen.hint());
    let (fill, ink) = if enabled {
        widgets::toggle_colors(ui, &response, true)
    } else {
        (theme::panel_background(), theme::text_hint())
    };
    painter.fill_background(apply, 3.0, fill);
    if !enabled {
        painter.rect_stroke(apply, 3.0, theme::hairline(theme::border()), egui::StrokeKind::Inside);
    }
    widgets::text_in(painter, apply, Align2::CENTER_CENTER, &apply_text, theme::ui_font(theme::TEXT_SMALL), ink);
    if enabled && response.clicked() {
        // No seed for Humanise and Randomise: a fresh draw each press.
        let id = match chosen {
            Transform::Quantise => "note.quantize",
            Transform::Humanise => "note.humanize",
            Transform::RandomiseVelocity => "note.randomizeVelocity",
            Transform::RampVelocity => "note.rampVelocity",
        };
        actions.push(Action::new(id, params));
    }
}
