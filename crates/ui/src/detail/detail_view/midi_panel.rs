// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A MIDI clip's panel in the clip column (`docs/spec/ui-clip-editor.md` › Clip column and › Note
//! tools): the Clip part it shares with an audio clip's panel with what the tools act on in
//! figures under it, and beside them (under them when the detail area is stacked) the tools part
//! with its Notes and Transform tabs. Every tool is one `note.*` command over the selection or the
//! whole clip; `midi-tools` styles the tools part.

use super::clip_panel::{
    CLIP_PART, Change, FIELD, Field, GAP, NAME, TOGGLE, Text, bar_of, button_face, clip_part, disabled_face, drop_down,
    name_over, number_field, plain_number, set_button, small_button, toggle, whole,
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
/// The line under the Transform tab's list that says what the chosen transformation does.
const HINT: f32 = 12.0;
const CHIP: f32 = 18.0;
const INFO_ROW: f32 = 14.0;
const RANGE_BAR: f32 = 10.0;
/// The space between the buttons of the Notes tab's grid.
const BUTTON_GAP: f32 = 4.0;
/// The quantise amount's id to the theme, and its knob's height where the theme sets none.
const QUANTISE_AMOUNT: &str = "quantise-amount";
const KNOB: f32 = 32.0;
/// A check box's side.
const CHECK: f32 = 12.0;

/// The tools part's spacing: the room between its rows, between the rows of the Notes tab's
/// buttons, and the height of its fields and buttons and of its switches and Apply.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ToolsSpacing {
    pub gap: f32,
    pub button_gap: f32,
    pub field: f32,
    pub toggle: f32,
    /// How tall the quantise amount's knob is, when the theme draws the amount as a knob
    /// (`midi-tools slider { appearance: knob }`, or `#quantise-amount`).
    pub knob: Option<f32>,
}

/// The tools part's spacing as the theme sets it: `midi-tools { gap }` the room between
/// its rows (0 to 16 points), `midi-tools button { height }` its fields', buttons', switches' and
/// Apply's height (10 to 40 points); else the panel's own. `slider { appearance: knob }`
/// inside it (`#quantise-amount` for the one slider) draws the quantise amount as a knob, as tall
/// as the slider's `height` (16 to 96 points, 32 by default).
pub(crate) fn tools_spacing() -> ToolsSpacing {
    let el = crate::kit::element::MIDI_TOOLS.enter();
    let gap = el
        .style()
        .and_then(|style| style.points(suhina_style::property::Property::RowGap))
        .filter(|v| v.is_finite())
        .map(|v| v.clamp(0.0, 16.0));
    let height = {
        let button = crate::kit::element::BUTTON.enter();
        button.style().and_then(|c| c.height).filter(|h| h.is_finite()).map(|h| h.clamp(10.0, 40.0))
    };
    let knob = {
        let slider = crate::kit::element::SLIDER.id(QUANTISE_AMOUNT).enter();
        crate::kit::style_class::knob_preference(&slider, false).then(|| {
            slider.style().and_then(|c| c.height).filter(|h| h.is_finite()).map_or(KNOB, |h| h.clamp(16.0, 96.0))
        })
    };
    ToolsSpacing {
        gap: gap.unwrap_or(GAP),
        button_gap: gap.unwrap_or(BUTTON_GAP),
        field: height.unwrap_or(FIELD),
        toggle: height.unwrap_or(TOGGLE),
        knob,
    }
}

/// How tall the tools part is under its tab row: its taller tab, Notes (Transpose, the buttons and
/// the Swing row) or Transform (the list, the hint, a row of settings, a switch and Apply; the
/// taller with the quantise amount's knob).
fn tools_body(s: ToolsSpacing) -> f32 {
    let notes = NAME + s.field + s.gap + s.field * 3.0 + s.button_gap * 2.0 + s.gap + NAME + s.field;
    let settings = s.knob.map_or(s.field, |knob| knob.max(s.field));
    let transform = s.field + s.gap + HINT + s.gap + NAME + settings + s.gap + s.toggle + s.gap + s.toggle;
    notes.max(transform)
}

/// A name and a field under it, twice side by side, `field` tall.
fn pair(part: Rect, top: f32, field: f32) -> [(Rect, Rect); 2] {
    let half = (part.width() - GAP) / 2.0;
    [0.0, 1.0].map(|i| {
        let left = part.left() + i * (half + GAP);
        let name = Rect::from_min_size(Pos2::new(left, top), Vec2::new(half, NAME));
        let field = Rect::from_min_size(Pos2::new(left, top + NAME), Vec2::new(half, field));
        (name, field)
    })
}
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
    let tools = if midi && tools {
        let s = tools_spacing();
        // The part's border and padding round what it holds (#1336).
        let [top, _, bottom, _] = crate::kit::element::MIDI_TOOLS.enter().insets();
        14.0 + TAB_ROW + s.gap + tools_body(s) + top + bottom
    } else {
        0.0
    };
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

/// The Swing row at the foot of the Notes tab (#1221, `docs/spec/swing.md`): its name says whose swing the
/// clip plays, the **Global** switch makes it follow the project's swing or play its own again, and
/// the slider sets the swing it plays: the project's while it follows it (so every clip that
/// follows changes with it), else its own. Left out whole when it does not fit. Returns where it
/// ends.
#[allow(clippy::too_many_arguments)]
fn swing_row(
    ui: &Ui,
    painter: &egui::Painter,
    part: Rect,
    top: f32,
    address: &serde_json::Value,
    panel: &AudioPanel,
    field: f32,
    actions: &mut Vec<Action>,
) {
    if top + NAME + field > part.bottom() + 0.5 {
        return;
    }
    let on_clip = |more: serde_json::Value| {
        let mut params = address.clone();
        if let (Some(map), serde_json::Value::Object(more)) = (params.as_object_mut(), more) {
            map.extend(more);
        }
        params
    };
    let global = !panel.swing.own;
    let percent = (panel.swing.effective(panel.global_swing) * 100.0).round();
    let name = Rect::from_min_size(Pos2::new(part.left(), top), Vec2::new(part.width(), NAME));
    name_over(painter, name, if global { "Swing (global)" } else { "Swing (this clip)" }, Align2::LEFT_BOTTOM);
    widgets::text_in(
        painter,
        name.shrink2(Vec2::new(0.0, 2.0)),
        Align2::RIGHT_BOTTOM,
        &format!("{percent} %"),
        theme::mono_font(theme::TEXT_TINY),
        theme::text_value(),
    );
    let switch = Rect::from_min_size(Pos2::new(part.left(), top + NAME), Vec2::new(52.0, field));
    if toggle(ui, painter, switch, "Use global swing", "Global", global) {
        actions.push(Action::new("clip.setSwing", on_clip(json!({ "global": !global }))));
    }
    let slider =
        Rect::from_min_max(Pos2::new(switch.right() + 4.0, switch.top()), Pos2::new(part.right(), switch.bottom()));
    let label = if global { "Swing (global)" } else { "Swing (this clip)" };
    let id = Id::unique(("clip-panel-swing", panel.clip.0));
    if let Some((amount, dragged)) = widgets::percent_slider(ui, painter, slider, id, label, percent, !global) {
        // The frames of one drag are one gesture, one undo step.
        let mut params = json!({ "amount": amount });
        if dragged {
            params["gesture"] = crate::interaction::commands::gesture("clip-panel-swing");
        }
        let action = if global {
            Action::new("transport.setSwing", params)
        } else {
            Action::new("clip.setSwing", on_clip(params))
        };
        actions.push(action);
    }
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
    let MidiTools { tools, state, .. } = midi;
    let spacing = tools_spacing();
    // `midi-tools` styles the part: its background and, through the scope, its controls' text.
    let el = crate::kit::element::MIDI_TOOLS.enter();
    let [top, right, bottom, left] = el.insets();
    if part.height() < TAB_ROW + top + bottom || part.width() < 40.0 + left + right {
        return;
    }
    // What it holds goes in its content box, inside its border and padding (#1336).
    let part = el
        .paint_with(painter, part, crate::kit::element::Native { radius: 0.0, fill: theme::panel_background() })
        .content;
    let tab_row = Rect::from_min_size(part.min, Vec2::new(part.width(), TAB_ROW));
    tab_row_of(ui, painter, tab_row, state);
    let body = Rect::from_min_max(Pos2::new(part.left(), tab_row.bottom() + spacing.gap), part.max);
    // A time selection with no note in it gives the tools nothing to act on.
    let before = actions.len();
    let notes_end = match state.tools_tab {
        ToolsTab::Notes => Some(notes_tab(ui, painter, body, panel, tools, state, spacing, actions)),
        ToolsTab::Transform => {
            transform_tab(ui, painter, body, tools, state, spacing, actions);
            None
        }
    };
    if tools.nothing {
        actions.truncate(before);
    }
    // The clip's swing, under the Notes tab's tools: the clip's own, so it works whatever the
    // tools act on.
    if let Some(end) = notes_end {
        swing_row(ui, painter, body, end + spacing.gap, tools.address, panel, spacing.field, actions);
    }
    // Escape leaves Quantise for the Notes tab, as it closed the quantise block before (Escape also
    // clears the note selection in the piano roll; both happen, as both are what the key means).
    if state.quantize_open() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.set_quantize_open(false);
    }
}

/// The tab row: the Notes and Transform tabs (the one shown `text.primary` over a 2 px `accent`
/// line, the other `text.hint`), each as wide as its word, in one CSS row (#1372).
fn tab_row_of(ui: &Ui, painter: &egui::Painter, row: Rect, state: &mut PianoRollState) {
    use crate::kit::native_css::row::{self, Bar, Item};
    let tabs = [(ToolsTab::Notes, "Notes"), (ToolsTab::Transform, "Transform")];
    let element = crate::kit::element::TAB_ROW.enter();
    element.paint(painter, row);
    painter.hline(row.x_range(), row.bottom() - 0.5, theme::hairline(theme::border()));
    let font = theme::ui_font(theme::TEXT_SMALL);
    let ids = tabs.map(|(_, text)| format!("tab-{}", text.to_ascii_lowercase()));
    let items: Vec<_> = tabs
        .iter()
        .zip(&ids)
        .map(|((tab, text), id)| {
            Item::named("editor-tab", id)
                .flag("checked", state.tools_tab == *tab)
                .width(widgets::text_width(ui, text, font.clone()) + 16.0)
        })
        .collect();
    let Some(laid) = row::row(&Bar::of(&element, row.size()), &items) else { return };
    for (index, ((tab, text), id)) in tabs.into_iter().zip(&ids).enumerate() {
        let Some(cell) = laid.at(index, row.min) else { continue };
        // A tab taller than the row paints and takes input only inside it, not over the tools.
        let painter = &painter.with_clip_rect(painter.clip_rect().intersect(row));
        let hit = cell.intersect(row);
        let response = widgets::button_area(ui, hit, Id::unique(("midi-tools-tab", text)), format!("{text} tab"));
        // `editor-tab` (`[checked]` for the one shown), `#tab-notes` and `#tab-transform`.
        let on = state.tools_tab == tab;
        let el = crate::kit::element::EDITOR_TAB
            .id(id)
            .attribute("kind", id)
            .response(&response)
            .attr(crate::kit::element::Checked, on)
            .enter();
        let ink = if on {
            theme::text_primary()
        } else {
            motion::mix(theme::text_hint(), theme::text_secondary(), widgets::hover(ui, &response))
        };
        let ink = el.own_color_or(ink);
        // The tab's box is painted, and its word lies in the content box, inside its border and
        // padding.
        let content = el.paint(painter, cell).content;
        widgets::text_in(painter, content, Align2::CENTER_CENTER, text, font.clone(), ink);
        if on {
            let line = Rect::from_min_max(
                Pos2::new(cell.left() + 4.0, cell.bottom() - 2.0),
                Pos2::new(cell.right() - 4.0, cell.bottom()),
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
    // A `button` to the theme, as the column's buttons were.
    let ink = if enabled {
        let fill = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        button_face(crate::kit::element::BUTTON, painter, rect, &response, false, fill, true)
            .unwrap_or_else(theme::text_primary)
    } else {
        disabled_face(painter, rect);
        theme::text_hint()
    };
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

/// A setting of a transformation that is on or off: a check box and its name after it, so it
/// does not read as a button beside Apply under it. The box is a `button` to the theme
/// (`[checked]` while on); the whole row takes the click. Named "`name`: On" or "Off", and
/// showing `name`, or `short` where the part is too narrow for it, so the name is not cut.
/// Returns whether it was clicked.
fn check_box(ui: &Ui, painter: &egui::Painter, row: Rect, name: &str, short: &str, on: bool) -> bool {
    let state = if on { "On" } else { "Off" };
    let response = widgets::button_area(ui, row, Id::unique(("midi-tools-check", name)), format!("{name}: {state}"));
    let side = CHECK.min(row.height());
    let mark = Rect::from_min_size(Pos2::new(row.left(), row.center().y - side / 2.0), Vec2::splat(side));
    let (fill, ink) = widgets::toggle_colors(ui, &response, on);
    let ink = button_face(crate::kit::element::BUTTON, painter, mark, &response, on, fill, !on).unwrap_or(ink);
    if on {
        let at = |x: f32, y: f32| Pos2::new(mark.left() + side * x, mark.top() + side * y);
        let line = egui::Stroke::new(1.5, ink);
        painter.line_segment([at(0.24, 0.52), at(0.42, 0.7)], line);
        painter.line_segment([at(0.42, 0.7), at(0.76, 0.3)], line);
    }
    let text = Rect::from_min_max(Pos2::new(mark.right() + 6.0, row.top()), row.max);
    let colour = motion::mix(theme::text_secondary(), theme::text_primary(), widgets::hover(ui, &response));
    let font = theme::ui_font(theme::TEXT_SMALL);
    let shown = check_text(name, short, text.width(), |t| widgets::text_width(ui, t, font.clone()));
    widgets::text_in(painter, text, Align2::LEFT_CENTER, shown, font, colour);
    response.clicked()
}

/// The text a check box shows in `room` points: its name, or the short one where the name is
/// wider than that.
pub(super) fn check_text<'a>(name: &'a str, short: &'a str, room: f32, width: impl Fn(&str) -> f32) -> &'a str {
    if width(name) <= room { name } else { short }
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
    s: ToolsSpacing,
    actions: &mut Vec<Action>,
) -> f32 {
    let left = body;
    let enabled = tools.count() > 0;
    let mut y = left.top();
    let fits = |y: f32, height: f32| y + height <= body.bottom() + 0.5;
    // Transpose: its semitones (0 until chosen), and Apply at the right of its name, greyed out
    // while they are 0, as there is nothing to move.
    if fits(y, NAME + s.field) {
        let name = Rect::from_min_size(Pos2::new(left.left(), y), Vec2::new(left.width(), NAME));
        name_over(painter, name, "Transpose", Align2::LEFT_BOTTOM);
        let moves = enabled && state.tools.transpose.clamp(-24, 24) != 0;
        if small_button(ui, painter, set_button(name), "Apply", "Transpose", moves) {
            let mut params = tools.params();
            params["semitones"] = json!(state.tools.transpose.clamp(-24, 24));
            state.replacing(&mut params);
            actions.push(Action::new("note.move", params));
        }
        let field = Rect::from_min_size(Pos2::new(left.left(), y + NAME), Vec2::new(left.width(), s.field));
        let shown = |v: f64| format!("{} st", signed(v));
        let value = f64::from(state.tools.transpose);
        if let Some(v) = setting_field(ui, painter, field, "Transpose by", value, (-24.0, 24.0), &shown) {
            state.tools.transpose = v.round() as i32;
        }
        y += NAME + s.field + s.gap;
    }
    // The one-click tools, two to a row.
    let cell = (left.width() - BUTTON_GAP) / 2.0;
    let rows: [[&str; 2]; 3] = [["Fit to scale", "Invert"], ["Reverse", "Legato"], ["×2", "÷2"]];
    for pair_of in rows {
        if !fits(y, s.field) {
            break;
        }
        for (column, name) in pair_of.into_iter().enumerate() {
            let rect = Rect::from_min_size(
                Pos2::new(left.left() + column as f32 * (cell + BUTTON_GAP), y),
                Vec2::new(cell, s.field),
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
        y += s.field + s.button_gap;
    }
    y - s.button_gap
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
    painter.rect_filled(bar, 3.0, theme::control_background());
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
#[allow(clippy::too_many_arguments)]
fn transform_tab(
    ui: &mut Ui,
    painter: &egui::Painter,
    body: Rect,
    tools: &Tools<'_>,
    state: &mut PianoRollState,
    s: ToolsSpacing,
    actions: &mut Vec<Action>,
) {
    if body.height() < s.field {
        return;
    }
    let chosen = state.transform;
    let list = Rect::from_min_size(body.min, Vec2::new(body.width(), s.field));
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
    let mut y = body.top() + s.field + s.gap;
    // What it does, in a line.
    let hint = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), HINT));
    if hint.bottom() <= body.bottom() + 0.5 {
        widgets::text_in(
            painter,
            hint,
            Align2::LEFT_CENTER,
            chosen.hint(),
            theme::ui_font(theme::TEXT_TINY),
            theme::text_hint(),
        );
        y = hint.bottom() + s.gap;
    }
    let settings_fit = y + NAME + s.field <= body.bottom() + 0.5;
    let settings = &mut state.tools;
    let mut params = tools.params();
    match chosen {
        Transform::Humanise => {
            if settings_fit {
                let [(time_name, time_field), (velocity_name, velocity_field)] = pair(body, y, s.field);
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
                y += NAME + s.field + s.gap;
            }
            params["ms"] = json!(settings.humanize_ms.clamp(0.0, 50.0));
            params["velocity"] = json!(settings.humanize_velocity.clamp(0.0, 127.0));
        }
        Transform::Quantise => {
            let mut quantize = state.quantize;
            if settings_fit {
                let [(grid_name, grid_field), (amount_name, amount_field)] = pair(body, y, s.field);
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
                if let Some(size) = s.knob {
                    let knob =
                        Rect::from_min_size(amount_field.min, Vec2::new(amount_field.width(), size.max(s.field)));
                    quantize.amount = amount_knob(ui, painter, knob, quantize.amount);
                } else {
                    let percent = |v: f64| format!("{v} %");
                    if let Some(v) = setting_field(
                        ui,
                        painter,
                        amount_field,
                        "Quantise amount",
                        quantize.amount,
                        (0.0, 100.0),
                        &percent,
                    ) {
                        quantize.amount = v;
                    }
                }
                y += NAME + s.knob.map_or(s.field, |knob| knob.max(s.field)) + s.gap;
                if y + s.toggle <= body.bottom() + 0.5 {
                    let row = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), s.toggle));
                    y += s.toggle + s.gap;
                    explain(ui, row, "ends", "Move ends too: each note's end goes to the grid as its start does");
                    if check_box(ui, painter, row, "Move ends too", "Ends too", quantize.ends) {
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
                let [(name, field), _] = pair(body, y, s.field);
                name_over(painter, name, "Velocity ±", Align2::LEFT_BOTTOM);
                let steps = |v: f64| format!("{v}");
                if let Some(v) =
                    setting_field(ui, painter, field, "Randomise amount", settings.random_amount, (0.0, 127.0), &steps)
                {
                    settings.random_amount = v;
                }
                y += NAME + s.field + s.gap;
            }
            params["amount"] = json!(settings.random_amount.clamp(0.0, 127.0));
        }
        Transform::RampVelocity => {
            if settings_fit {
                let [(from_name, from_field), (to_name, to_field)] = pair(body, y, s.field);
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
                y += NAME + s.field + s.gap;
            }
            params["from"] = json!(settings.ramp_from.clamp(1, 127));
            params["to"] = json!(settings.ramp_to.clamp(1, 127));
        }
    }
    // Apply, at the bottom, says what it does and to what: "Humanise 3 notes", "Quantise clip".
    if y + s.toggle > body.bottom() + 0.5 {
        return;
    }
    let apply_text = format!("{} {}", chosen.verb(), tools.target());
    let apply = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), s.toggle));
    let enabled = tools.count() > 0;
    let response =
        widgets::button_area(ui, apply, Id::unique("midi-tools-apply"), &apply_text).on_hover_text(chosen.hint());
    let ink = if enabled {
        let (fill, ink) = widgets::toggle_colors(ui, &response, true);
        button_face(crate::kit::element::BUTTON, painter, apply, &response, false, fill, false).unwrap_or(ink)
    } else {
        disabled_face(painter, apply);
        theme::text_hint()
    };
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

/// The quantise amount as a knob, where the theme draws it as one (`midi-tools slider
/// { appearance: knob }`, or `#quantise-amount`): dragged up for more, half a percent a
/// point, its value where `value-position` puts it (at its right unless it says). Named
/// "Quantise amount: `n` %" as the field is. Returns the amount.
fn amount_knob(ui: &Ui, painter: &egui::Painter, rect: Rect, amount: f64) -> f64 {
    let response = ui.interact(rect, Id::unique("midi-tools-quantise-amount"), Sense::drag());
    let amount = if response.dragged() {
        (amount - f64::from(response.drag_delta().y) * 0.5).clamp(0.0, 100.0).round()
    } else {
        amount
    };
    let text = format!("{amount} %");
    let label = format!("Quantise amount: {text}");
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::Slider, true, &label);
        info.current_text_value = Some(text.clone());
        info
    });
    let slider = crate::kit::element::SLIDER.id(QUANTISE_AMOUNT).response(&response).enter();
    widgets::knob_with_value(
        painter,
        rect,
        &slider,
        (amount / 100.0) as f32,
        theme::text_primary(),
        Some(&response),
        &text,
        theme::text_secondary(),
        widgets::ValuePosition::Right,
        10.0,
    );
    amount
}
