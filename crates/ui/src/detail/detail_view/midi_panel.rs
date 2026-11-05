// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A MIDI clip's panel in the clip column (`docs/spec/ui-clip-editor.md` › Clip column and › Note
//! tools): the Clip part it shares with an audio clip's panel, and beside it (under it when the
//! detail area is stacked) the tools part with its Notes and Transform tabs. Every tool is one
//! `note.*` command over the selection or the whole clip; `.midi-tools` styles the tools part.

use super::clip_panel::{
    CLIP_PART, Change, FIELD, Field, GAP, NAME, PART_GAP, TOGGLE, Text, bar_of, clip_part, drop_down, name_over,
    number_field, pair, plain_number, set_button, small_button, toggle, whole,
};
use super::*;
use crate::detail::midi_tools::{Tools, ToolsTab, Transform, tools_width};
use crate::detail::piano_roll::{PianoRollState, QUANTIZE_GRIDS};
use suhina_engine::model::{ROOT_NAMES, ScaleMode};

/// What a MIDI clip's panel needs from the piano roll to show its tools: what they act on, and the
/// piano roll's state, which keeps their settings, tab and transformation.
pub struct MidiTools<'a, 'b> {
    pub tools: &'a Tools<'b>,
    pub state: &'a mut PianoRollState,
}

/// The tab row at the top of the tools part, a row of the selection's figures, and the bar under
/// the velocity figures.
const TAB_ROW: f32 = 22.0;
const INFO_ROW: f32 = 14.0;
const RANGE_BAR: f32 = 10.0;
/// The space between the buttons of the Notes tab's grid.
const BUTTON_GAP: f32 = 4.0;
/// How tall the tools part is under its tab row: the tallest of its tabs.
const TOOLS_BODY: f32 = NAME + FIELD + GAP + FIELD * 3.0 + BUTTON_GAP * 2.0;

/// The panel's width in a row: the Clip part, and the tools part when it is shown.
pub(super) fn row_width(tools: bool) -> f32 {
    let tools = if tools { PART_GAP + tools_width() } else { 0.0 };
    CLIP_PART + tools + 16.0
}

/// How tall the panel is with its parts one under another (the detail area stacked), under its
/// title row.
pub(super) fn stacked_height(tools: bool) -> f32 {
    let clip = (NAME + FIELD) * 3.0 + TOGGLE + GAP * 3.0;
    let tools = if tools { 14.0 + TAB_ROW + GAP + TOOLS_BODY } else { 0.0 };
    clip + tools + 16.0
}

/// The MIDI clip's parts in `area` under the title row: side by side in a row, one under the other
/// when `stacked`. Without `midi` (the clip's editor is not shown) only the Clip part is drawn.
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
    if stacked {
        let clip = Rect::from_min_max(area.min, Pos2::new(area.right(), area.bottom()));
        let end = clip_part(ui, painter, clip, info, at, panel, actions);
        if let Some(midi) = midi {
            let top = end + 14.0;
            let part = Rect::from_min_max(Pos2::new(area.left(), top), area.max);
            tools_part(ui, painter, part, panel, midi, actions);
        }
        return;
    }
    let clip = Rect::from_min_max(area.min, Pos2::new(area.left() + CLIP_PART, area.bottom()));
    clip_part(ui, painter, clip, info, at, panel, actions);
    if let Some(midi) = midi {
        let left = clip.right() + PART_GAP;
        let right = (left + tools_width()).min(area.right());
        let part = Rect::from_min_max(Pos2::new(left, area.top()), Pos2::new(right, area.bottom()));
        tools_part(ui, painter, part, panel, midi, actions);
    }
}

/// The tools part: its tab row with what the tools act on, and the tab shown.
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
    let MidiTools { tools, state } = midi;
    // `.midi-tools` styles the part: its background and, through the scope, its controls' text.
    let _style = crate::kit::style_class::enter("midi-tools");
    crate::kit::native_css::surface(painter, part, 0.0, theme::panel_background());
    let tab_row = Rect::from_min_size(part.min, Vec2::new(part.width(), TAB_ROW));
    tab_row_of(ui, painter, tab_row, tools, state);
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

/// The tab row: the Notes and Transform tabs at the left (the one shown `text.primary` over a 2 px
/// `accent` line, the other `text.hint`), and at the right a chip that says what the tools act on.
fn tab_row_of(ui: &Ui, painter: &egui::Painter, row: Rect, tools: &Tools<'_>, state: &mut PianoRollState) {
    painter.hline(row.x_range(), row.bottom() - 0.5, theme::hairline(theme::border()));
    let font = theme::ui_font(theme::TEXT_SMALL);
    let mut x = row.left();
    for (tab, text) in [(ToolsTab::Notes, "Notes"), (ToolsTab::Transform, "Transform")] {
        let width = widgets::text_width(ui, text, font.clone()) + 16.0;
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
    // The chip: what the tools act on and how many notes that is, lit while notes are selected.
    let head = tools.head();
    let count = tools.count_text();
    let tiny = theme::ui_font(theme::TEXT_TINY);
    let mono = theme::mono_font(theme::TEXT_TINY);
    let head_width = widgets::text_width(ui, head, tiny.clone());
    let count_width = widgets::text_width(ui, &count, mono.clone());
    let chip_width = (head_width + count_width + 18.0).min((row.right() - x - 4.0).max(0.0));
    if chip_width < 24.0 {
        return;
    }
    let chip = Rect::from_center_size(
        Pos2::new(row.right() - chip_width / 2.0, row.center().y - 1.0),
        Vec2::new(chip_width, 16.0),
    );
    let selected = !tools.selected.is_empty();
    let fill = if selected { theme::control_selected() } else { theme::control_background() };
    let ink = if selected { theme::text_on(theme::control_selected()) } else { theme::text_secondary() };
    painter.fill_background(chip, 3.0, fill);
    let inner = chip.shrink2(Vec2::new(6.0, 0.0));
    let head_rect = Rect::from_min_max(inner.min, Pos2::new(inner.left() + head_width, inner.bottom()));
    let count_rect = Rect::from_min_max(Pos2::new(head_rect.right() + 6.0, inner.top()), inner.max);
    widgets::text_in(painter, head_rect, Align2::LEFT_CENTER, head, tiny, ink);
    widgets::text_in(painter, count_rect, Align2::RIGHT_CENTER, &count, mono, ink);
    // The head and the count are named for a reader and a test, as the piano roll's parts are.
    ui.interact(head_rect, Id::unique("piano-roll-tools-head"), Sense::hover())
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, head));
    ui.interact(count_rect, Id::unique("piano-roll-tools-count"), Sense::hover())
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &count));
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

/// The Notes tab: Transpose and the one-click tools at the left, what is selected (or the whole
/// clip) at the right.
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
    let half = (body.width() - PART_GAP) / 2.0;
    let left = Rect::from_min_size(body.min, Vec2::new(half, body.height()));
    let right = Rect::from_min_max(Pos2::new(left.right() + PART_GAP, body.top()), body.max);
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
            let id = match name {
                "Fit to scale" => "note.fitToScale",
                "Invert" => "note.invert",
                "Reverse" => "note.reverse",
                "Legato" => {
                    params["until"] = json!(tools.legato_until(state.span_end()));
                    state.replacing(&mut params);
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
    figures(ui, painter, right, panel, tools);
}

/// Fit to scale for a clip with no scale: a list of the scales, each with its roots; picking one
/// sets the clip's scale (`clip.setScale`) and fits the notes to it (`note.fitToScale`).
fn fit_to_a_new_scale(response: &egui::Response, tools: &Tools<'_>, actions: &mut Vec<Action>) {
    egui::Popup::menu(response).show(|ui| {
        ui.label(egui::RichText::new("Set the clip's scale").font(theme::ui_font(theme::TEXT_TINY)));
        for mode in ScaleMode::ALL {
            ui.menu_button(mode.label(), |ui| {
                for (index, root) in ROOT_LABELS.iter().enumerate() {
                    if ui.button(*root).clicked() {
                        let name = ROOT_NAMES.get(index).copied().unwrap_or("C");
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

/// The roots as the lists show them.
const ROOT_LABELS: [&str; 12] = ["C", "C#/Db", "D", "D#/Eb", "E", "F", "F#/Gb", "G", "G#/Ab", "A", "A#/Bb", "B"];

/// What the tools act on, in figures: how many notes, their keys, where they start and end, and
/// their velocities as numbers and a bar of 1 to 127. Each row is named "`Name`: `value`" for a
/// reader and a test; rows that do not fit are left out whole.
fn figures(ui: &Ui, painter: &egui::Painter, part: Rect, panel: &AudioPanel, tools: &Tools<'_>) {
    let all: std::collections::BTreeSet<usize> = (0..tools.notes.len()).collect();
    let acted = if tools.selected.is_empty() { &all } else { tools.selected };
    let info = SelectionInfo::of(tools.notes, acted, bar_of(panel));
    let total = tools.notes.len();
    let notes = match (tools.selected.is_empty(), info.muted) {
        (true, _) => total.to_string(),
        (false, 0) => format!("{} of {total}", info.selected),
        (false, muted) => format!("{} of {total}, {muted} muted", info.selected),
    };
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
    let rows = [("Notes", Some(notes)), ("Pitch", pitch), ("From", info.from.clone()), ("To", info.to.clone())];
    let mut y = part.top();
    for (name, value) in rows {
        let Some(value) = value else { continue };
        if y + INFO_ROW > part.bottom() + 0.5 {
            return;
        }
        figure(
            ui,
            painter,
            Rect::from_min_size(Pos2::new(part.left(), y), Vec2::new(part.width(), INFO_ROW)),
            name,
            &value,
        );
        y += INFO_ROW;
    }
    let (Some(velocity), Some(low), Some(high)) = (velocity, info.low_velocity, info.high_velocity) else { return };
    if y + 4.0 + INFO_ROW + RANGE_BAR > part.bottom() + 0.5 {
        return;
    }
    y += 4.0;
    figure(
        ui,
        painter,
        Rect::from_min_size(Pos2::new(part.left(), y), Vec2::new(part.width(), INFO_ROW)),
        "Velocity",
        &velocity,
    );
    y += INFO_ROW;
    let bar = Rect::from_min_size(Pos2::new(part.left(), y), Vec2::new(part.width(), RANGE_BAR));
    painter.fill_background(bar, 3.0, theme::control_background());
    let x_of = |v: u8| bar.left() + 2.0 + (f32::from(v.saturating_sub(1)) / 126.0) * (bar.width() - 4.0);
    let span = Rect::from_min_max(
        Pos2::new(x_of(low), bar.top() + 3.0),
        Pos2::new(x_of(high).max(x_of(low) + 3.0), bar.bottom() - 3.0),
    );
    painter.rect_filled(span, 2.0, theme::accent());
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

/// The Transform tab: the list of transformations and Apply on one line, a line on what the chosen
/// one does, and its settings.
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
    // Apply, at the right, says what it does and to what: "Humanise 3 notes", "Quantise clip".
    let apply_text = format!("{} {}", chosen.verb(), tools.target());
    let font = theme::ui_font(theme::TEXT_SMALL);
    let apply_width = (widgets::text_width(ui, &apply_text, font.clone()) + 16.0).min(body.width() / 2.0);
    let apply = Rect::from_min_max(
        Pos2::new(body.right() - apply_width, body.top()),
        Pos2::new(body.right(), body.top() + FIELD),
    );
    let list = Rect::from_min_max(body.min, Pos2::new((apply.left() - 8.0).max(body.left()), body.top() + FIELD));
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
    widgets::text_in(painter, apply, Align2::CENTER_CENTER, &apply_text, font, ink);
    let applied = enabled && response.clicked();
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
                    explain(ui, row, "chords", "Notes that start together move together");
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
            }
            params["from"] = json!(settings.ramp_from.clamp(1, 127));
            params["to"] = json!(settings.ramp_to.clamp(1, 127));
        }
    }
    if applied {
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
