// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The MIDI tools column beside the note grid: what the tools act on, the quantise block and the
//! note tools, each one `note.*` command over the selection or the whole clip
//! (`docs/spec/ui-clip-editor.md` › Other note tools).

use std::collections::BTreeSet;

use egui::{Align2, Pos2, Rect, Ui, Vec2};
use serde_json::{Value, json};
use suhina_engine::model::Note;

use crate::detail::piano_roll::{PianoRollState, QUANTIZE_GRIDS};
use crate::kit::theme;
use crate::kit::widgets::{self, Action};

/// The column's width, taken from the left of the detail area before the note grid.
pub const TOOLS_WIDTH: f32 = 112.0;

/// The column's width: `.midi-tools { width }` in the theme (80 to 400 points), else [`TOOLS_WIDTH`].
pub fn tools_width() -> f32 {
    let _style = crate::kit::style_class::enter("midi-tools");
    crate::kit::style_class::computed().and_then(|c| c.width).map_or(TOOLS_WIDTH, |w| w.clamp(80.0, 400.0))
}
const PAD: f32 = 6.0;
const ROW: f32 = 20.0;
const PARAM_ROW: f32 = 18.0;
const BUTTON_HEIGHT: f32 = 18.0;
const LABEL: f32 = 10.0;

/// The roots the scale menu offers, by name and pitch class.
pub const ROOTS: [(&str, usize); 12] = [
    ("C", 0),
    ("C#", 1),
    ("D", 2),
    ("D#", 3),
    ("E", 4),
    ("F", 5),
    ("F#", 6),
    ("G", 7),
    ("G#", 8),
    ("A", 9),
    ("A#", 10),
    ("B", 11),
];

/// The scales the Fit to scale menu offers: what the row reads, and the name `note.fitToScale`
/// takes.
pub const SCALES: [(&str, &str); 12] = [
    ("Major", "major"),
    ("Minor", "minor"),
    ("Harmonic minor", "harmonicMinor"),
    ("Melodic minor", "melodicMinor"),
    ("Dorian", "dorian"),
    ("Phrygian", "phrygian"),
    ("Lydian", "lydian"),
    ("Mixolydian", "mixolydian"),
    ("Locrian", "locrian"),
    ("Major pentatonic", "majorPentatonic"),
    ("Minor pentatonic", "minorPentatonic"),
    ("Blues", "blues"),
];

/// What the tools act on: the clip's address for a command, its notes, and the selection (indices
/// into `notes`).
pub struct Tools<'a> {
    pub address: &'a Value,
    pub notes: &'a [Note],
    pub selected: &'a BTreeSet<usize>,
}

impl Tools<'_> {
    /// The params a tool's command takes: the clip's address and, with notes selected, their
    /// indices (`notes` left out means the whole clip, as the commands take it).
    fn params(&self) -> Value {
        let mut params = self.address.clone();
        if !self.selected.is_empty() {
            params["notes"] = json!(self.selected.iter().collect::<Vec<_>>());
        }
        params
    }

    /// What the head says the tools act on.
    fn head(&self) -> &'static str {
        if self.selected.is_empty() { "Whole clip" } else { "Selected notes" }
    }

    /// How many notes the tools act on.
    fn count(&self) -> usize {
        if self.selected.is_empty() { self.notes.len() } else { self.selected.len() }
    }

    /// The count under the head: "4 notes", "1 note", "3 of 12 notes".
    fn count_text(&self) -> String {
        let count = self.count();
        let plural = if count == 1 { "note" } else { "notes" };
        if self.selected.is_empty() {
            format!("{count} {plural}")
        } else {
            format!("{count} of {} notes", self.notes.len())
        }
    }
}

/// The tools' own settings, kept until Suhina closes, as the quantise block's are. Every value is
/// clamped where it is used, so a hand-typed number can never reach a command out of range.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSettings {
    /// Semitones **Transpose** moves the notes by.
    pub transpose: i32,
    /// How far **Humanise** may move a start, in beats.
    pub humanize_time: f64,
    /// How far **Humanise** may move a velocity, in MIDI steps.
    pub humanize_velocity: f64,
    /// The scale's root, as an index into [`ROOTS`].
    pub root: usize,
    /// The scale, as an index into [`SCALES`].
    pub scale: usize,
    /// How far **Randomise** may move a velocity, in MIDI steps.
    pub random_amount: f64,
    /// The velocity **Ramp** starts from and ends on.
    pub ramp_from: i32,
    pub ramp_to: i32,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            transpose: 1,
            humanize_time: 0.02,
            humanize_velocity: 8.0,
            root: 0,
            scale: 0,
            random_amount: 16.0,
            ramp_from: 20,
            ramp_to: 120,
        }
    }
}

/// The height the quantise block needs: its title, the grid, the amount, the ends box and the two
/// buttons.
pub fn quantise_height() -> f32 {
    ROW + PAD + PARAM_ROW * 3.0 + BUTTON_HEIGHT + PAD
}

/// Draws the MIDI tools column at the left of `area` and returns the rest, where the note grid
/// goes. Nothing is drawn when `area` is too small to hold the column, so a short or narrow detail
/// area gets the whole space for the grid.
pub fn show(ui: &mut Ui, area: Rect, tools: &Tools<'_>, state: &mut PianoRollState, actions: &mut Vec<Action>) -> Rect {
    let width = tools_width();
    if area.width() < width + 80.0 || area.height() < ROW + PARAM_ROW {
        return area;
    }
    let column = Rect::from_min_max(area.min, Pos2::new(area.left() + width, area.bottom()));
    ui.allocate_rect(column, egui::Sense::hover());
    let painter = ui.painter_at(column);
    // `.midi-tools` styles the column: its background and, through the scope, its controls' text.
    let _style = crate::kit::style_class::enter("midi-tools");
    crate::kit::native_css::surface(&painter, column, 0.0, theme::panel_background());
    painter.line_segment(
        [Pos2::new(column.right() - 0.5, column.top()), Pos2::new(column.right() - 0.5, column.bottom())],
        egui::Stroke::new(1.0, theme::border()),
    );

    // The head says what the tools act on, and stays put while the tools scroll under it.
    let head = Rect::from_min_size(column.min, Vec2::new(column.width(), ROW));
    widgets::text_in(
        &painter,
        head.shrink2(Vec2::new(PAD, 0.0)),
        Align2::LEFT_CENTER,
        tools.head(),
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_primary(),
    );
    let count = Rect::from_min_size(Pos2::new(column.left(), head.bottom()), Vec2::new(column.width(), LABEL + 2.0));
    let count_text = tools.count_text();
    widgets::text_in(
        &painter,
        count.shrink2(Vec2::new(PAD, 0.0)),
        Align2::LEFT_CENTER,
        &count_text,
        theme::ui_font(theme::TEXT_SMALL),
        theme::text_hint(),
    );
    // The head and the count are named for a reader and a test, as the piano roll's parts are.
    let head_label = tools.head();
    ui.interact(head, egui::Id::new("piano-roll-tools-head"), egui::Sense::hover())
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, head_label));
    ui.interact(count, egui::Id::new("piano-roll-tools-count"), egui::Sense::hover())
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &count_text));

    let body = Rect::from_min_max(Pos2::new(column.left(), count.bottom() + PAD), column.max);
    // In a short column the tools have no room under the head: they are left out, not squeezed.
    let body_fits = body.height() >= PARAM_ROW;
    ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
        if !body_fits {
            return;
        }
        // A button or field too narrow for its text shortens it; nothing wraps onto more lines.
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        // `.midi-tools { --suhina-gap: 2px }` packs the rows closer (0 to 16 points), and
        // `.midi-tools .button { height }` sets the buttons' and fields' height.
        if let Some(gap) = crate::kit::style_class::property("--suhina-gap").and_then(|v| px_value(&v)) {
            ui.spacing_mut().item_spacing.y = gap.clamp(0.0, 16.0);
        }
        let button_height = {
            let _button = crate::kit::style_class::enter("button");
            crate::kit::style_class::computed().and_then(|c| c.height)
        };
        if let Some(height) = button_height {
            ui.spacing_mut().interact_size.y = height.clamp(10.0, 40.0);
        }
        egui::ScrollArea::vertical().id_salt("piano-roll-midi-tools").show(ui, |ui| {
            if state.quantize_open {
                quantise_block(ui, tools, state, actions);
            }
            note_tools(ui, tools, state, actions);
        });
    });
    // Escape closes the quantise block, as it closes the panel it replaces (Escape also clears the
    // note selection in the piano roll; both happen, as both are what the key means there).
    if state.quantize_open && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.quantize_open = false;
    }
    Rect::from_min_max(Pos2::new(column.right() + 1.0, area.top()), area.max)
}

/// The quantise controls, at the top of the column while they are open: Grid, Amount, "Move ends
/// too", **Quantise notes** and **Close** (› Quantise in `docs/spec/ui-clip-editor.md`). The
/// settings are kept for Ctrl+U; **Quantise notes** adds `note.quantize` to `actions`.
fn quantise_block(ui: &mut Ui, tools: &Tools<'_>, state: &mut PianoRollState, actions: &mut Vec<Action>) {
    let small = theme::ui_font(theme::TEXT_SMALL);
    let mono = theme::mono_font(theme::TEXT_SMALL);
    heading(ui, "Quantise", theme::text_primary());
    let mut settings = state.quantize;
    let shown = settings.grid;
    let menu = crate::kit::widgets::combo_box(
        ui,
        egui::ComboBox::from_id_salt("piano-roll-quantise-grid")
            .width(64.0)
            .selected_text(egui::RichText::new(shown).font(mono.clone())),
        |ui| {
            for grid in QUANTIZE_GRIDS {
                ui.selectable_value(&mut settings.grid, grid, egui::RichText::new(grid).font(mono.clone()));
            }
        },
    );
    menu.response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Quantise grid {shown}")));
    // `.midi-tools .slider { --suhina-appearance: knob }` draws the amount as a knob, dragged up
    // or down; `#quantise-amount` selects it alone.
    let slider_style = crate::kit::style_class::enter_named("slider", "quantise-amount");
    let slider = if crate::kit::style_class::knob_preference(false) {
        let size = crate::kit::style_class::computed().and_then(|c| c.height).map_or(32.0, |h| h.clamp(16.0, 96.0));
        let (rect, response) = ui.allocate_exact_size(Vec2::new(size * 2.4, size), egui::Sense::drag());
        if response.dragged() {
            settings.amount = (settings.amount - f64::from(response.drag_delta().y) * 0.5).clamp(0.0, 100.0).round();
        }
        let text = format!("{} %", settings.amount);
        let ink = crate::kit::theme::text_secondary();
        let pointer = crate::kit::theme::text_primary();
        let right = crate::kit::widgets::ValuePosition::Right;
        let position = (settings.amount / 100.0) as f32;
        crate::kit::widgets::knob_with_value(
            ui.painter(),
            rect,
            position,
            pointer,
            Some(&response),
            &text,
            ink,
            right,
            10.0,
        );
        response
    } else {
        ui.add_sized(
            Vec2::new(ui.available_width().min(150.0), 16.0),
            egui::Slider::new(&mut settings.amount, 0.0..=100.0).suffix(" %").integer().max_decimals(0),
        )
    };
    drop(slider_style);
    slider.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, format!("Amount {} %", settings.amount))
    });
    ui.checkbox(&mut settings.ends, egui::RichText::new("Move ends too").font(small.clone()));
    state.quantize = settings;
    let apply = ui
        .add(
            egui::Button::new(egui::RichText::new("Quantise notes").font(small.clone()))
                .min_size(Vec2::new(0.0, ui.spacing().interact_size.y)),
        )
        .on_hover_text("Quantise the selected notes, or every note")
        .clicked();
    // Its accessible label is the whole name, as the block had in the clip column.
    let close = ui.add(
        egui::Button::new(egui::RichText::new("Close").font(small.clone()))
            .min_size(Vec2::new(0.0, ui.spacing().interact_size.y)),
    );
    close.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close quantise"));
    if close.on_hover_text("Close the quantise controls").clicked() {
        state.quantize_open = false;
    }
    if apply {
        let mut params = tools.params();
        params["grid"] = json!(state.quantize.grid);
        params["amount"] = json!(state.quantize.amount);
        params["ends"] = json!(state.quantize.ends);
        state.replacing(&mut params);
        actions.push(Action::new("note.quantize", params));
    }
    ui.add_space(PAD);
}

/// The note tools: Transpose, Humanise, Fit to scale, Invert, Reverse, Legato, ×2 and ÷2, and the
/// velocity tools. Each button runs one command over what the head names.
fn note_tools(ui: &mut Ui, tools: &Tools<'_>, state: &mut PianoRollState, actions: &mut Vec<Action>) {
    let small = theme::ui_font(theme::TEXT_SMALL);
    let mut settings = state.tools.clone();

    heading(ui, "Transpose", theme::text_secondary());
    let transpose = crate::kit::widgets::number_input(
        ui,
        Some(Vec2::new(ui.available_width().min(64.0), 16.0)),
        egui::DragValue::new(&mut settings.transpose).range(-24..=24).speed(1.0).prefix(""),
    );
    transpose.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::DragValue,
            true,
            format!("Transpose semitones {}", settings.transpose),
        )
    });
    if button(ui, "Transpose", &small) {
        let mut params = tools.params();
        params["semitones"] = json!(settings.transpose);
        actions.push(Action::new("note.move", params));
    }

    ui.add_space(PAD);
    heading(ui, "Humanise", theme::text_secondary());
    let time = crate::kit::widgets::number_input(
        ui,
        Some(Vec2::new(ui.available_width().min(64.0), 16.0)),
        egui::DragValue::new(&mut settings.humanize_time).range(0.0..=1.0).speed(0.01).max_decimals(3).prefix("Time "),
    );
    time.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::DragValue,
            true,
            format!("Humanise time {:.3}", settings.humanize_time),
        )
    });
    let velocity = crate::kit::widgets::number_input(
        ui,
        Some(Vec2::new(ui.available_width().min(64.0), 16.0)),
        egui::DragValue::new(&mut settings.humanize_velocity).range(0.0..=127.0).speed(1.0).prefix("Velocity "),
    );
    velocity.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::DragValue,
            true,
            format!("Humanise velocity {:.0}", settings.humanize_velocity),
        )
    });
    if button(ui, "Humanise", &small) {
        let mut params = tools.params();
        params["time"] = json!(settings.humanize_time.clamp(0.0, 1.0));
        params["velocity"] = json!(settings.humanize_velocity.clamp(0.0, 127.0));
        // No seed: a fresh draw each press, so pressing it twice gives two different results.
        actions.push(Action::new("note.humanize", params));
    }

    ui.add_space(PAD);
    heading(ui, "Fit to scale", theme::text_secondary());
    let root = ROOTS.get(settings.root).map_or("C", |(name, _)| *name);
    let mono = theme::mono_font(theme::TEXT_SMALL);
    let menu = crate::kit::widgets::combo_box(
        ui,
        egui::ComboBox::from_id_salt("piano-roll-tools-root")
            .width(56.0)
            .selected_text(egui::RichText::new(root).font(mono.clone())),
        |ui| {
            for (index, (name, _)) in ROOTS.iter().enumerate() {
                ui.selectable_value(&mut settings.root, index, egui::RichText::new(*name).font(mono.clone()));
            }
        },
    );
    menu.response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Tools root {root}")));
    let scale = SCALES.get(settings.scale).map_or("Major", |(name, _)| *name);
    let menu = crate::kit::widgets::combo_box(
        ui,
        egui::ComboBox::from_id_salt("piano-roll-tools-scale")
            .width(96.0)
            .selected_text(egui::RichText::new(scale).font(mono.clone())),
        |ui| {
            for (index, (name, _)) in SCALES.iter().enumerate() {
                ui.selectable_value(&mut settings.scale, index, egui::RichText::new(*name).font(mono.clone()));
            }
        },
    );
    menu.response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Tools scale {scale}")));
    if button(ui, "Fit to scale", &small) {
        let mut params = tools.params();
        params["root"] = json!(root);
        params["scale"] = json!(SCALES.get(settings.scale).map_or("major", |(_, id)| *id));
        actions.push(Action::new("note.fitToScale", params));
    }

    ui.add_space(PAD);
    if button(ui, "Invert", &small) {
        actions.push(Action::new("note.invert", tools.params()));
    }
    if button(ui, "Reverse", &small) {
        actions.push(Action::new("note.reverse", tools.params()));
    }
    if button(ui, "Legato", &small) {
        let mut params = tools.params();
        state.replacing(&mut params);
        actions.push(Action::new("note.legato", params));
    }
    ui.horizontal(|ui| {
        let small = small.clone();
        // Two halves of the column's width, so the second is never pushed off its edge.
        let half = ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).max(8.0);
        let button = |ui: &mut Ui, name: &str, font: &egui::FontId| {
            ui.add(
                egui::Button::new(egui::RichText::new(name).font(font.clone()))
                    .min_size(Vec2::new(half, ui.spacing().interact_size.y)),
            )
            .clicked()
        };
        if button(ui, "×2", &small) {
            let mut params = tools.params();
            params["factor"] = json!(2.0);
            actions.push(Action::new("note.stretch", params));
        }
        if button(ui, "÷2", &small) {
            let mut params = tools.params();
            params["factor"] = json!(0.5);
            actions.push(Action::new("note.stretch", params));
        }
    });

    ui.add_space(PAD);
    heading(ui, "Velocity", theme::text_secondary());
    let amount = crate::kit::widgets::number_input(
        ui,
        Some(Vec2::new(ui.available_width().min(64.0), 16.0)),
        egui::DragValue::new(&mut settings.random_amount).range(0.0..=127.0).speed(1.0).prefix("Amount "),
    );
    amount.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::DragValue,
            true,
            format!("Randomise amount {:.0}", settings.random_amount),
        )
    });
    if button(ui, "Randomise", &small) {
        let mut params = tools.params();
        params["amount"] = json!(settings.random_amount.clamp(0.0, 127.0));
        actions.push(Action::new("note.randomizeVelocity", params));
    }
    let from = crate::kit::widgets::number_input(
        ui,
        Some(Vec2::new(ui.available_width().min(64.0), 16.0)),
        egui::DragValue::new(&mut settings.ramp_from).range(1..=127).speed(1.0).prefix("From "),
    );
    from.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, format!("Ramp from {}", settings.ramp_from))
    });
    let to = crate::kit::widgets::number_input(
        ui,
        Some(Vec2::new(ui.available_width().min(64.0), 16.0)),
        egui::DragValue::new(&mut settings.ramp_to).range(1..=127).speed(1.0).prefix("To "),
    );
    to.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, format!("Ramp to {}", settings.ramp_to))
    });
    if button(ui, "Ramp", &small) {
        let mut params = tools.params();
        params["from"] = json!(settings.ramp_from.clamp(1, 127));
        params["to"] = json!(settings.ramp_to.clamp(1, 127));
        actions.push(Action::new("note.rampVelocity", params));
    }

    state.tools = settings;
}

/// One 18 px button of the column, with its name as its accessible label.
fn button(ui: &mut Ui, name: &str, font: &egui::FontId) -> bool {
    ui.add(
        egui::Button::new(egui::RichText::new(name).font(font.clone()))
            .min_size(Vec2::new(ui.available_width().min(tools_width() - PAD * 2.0), ui.spacing().interact_size.y)),
    )
    .clicked()
}

/// A group heading in the column, drawn and not named: the button under it carries the name, so a
/// reader and a test find one node for "Transpose" and not two.
fn heading(ui: &mut Ui, text: &str, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), LABEL + 3.0), egui::Sense::hover());
    widgets::text_in(ui.painter(), rect, Align2::LEFT_CENTER, text, theme::ui_font(theme::TEXT_SMALL), color);
}

/// `12px` as 12.0.
fn px_value(value: &str) -> Option<f32> {
    value.trim().strip_suffix("px").and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite())
}
