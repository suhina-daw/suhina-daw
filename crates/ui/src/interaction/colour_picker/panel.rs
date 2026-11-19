// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The floating picker's controls and bounded colour suggestions. Project edits belong to
//! the app; this panel only reports choices and the lifetime of a pointer gesture.

mod canvas;

use egui::{Id, Key, Sense, Ui, Vec2};
use suhina_engine::model::Rgb;
use suhina_style::value::{hsl_to_srgb, oklch_to_srgb, srgb_to_hsl, srgb_to_oklch};

use super::{color32, hex};
use crate::kit::{element, theme, widgets};

/// Conventional HSV of encoded sRGB, so its hue agrees with numeric HSL. Reuse egui's
/// component conversion, without its linear-light Hsva transfer functions.
#[derive(Clone, Copy)]
pub(super) struct Hsv {
    h: f32,
    s: f32,
    v: f32,
}

impl Hsv {
    fn from_srgb(rgb: [u8; 3]) -> Self {
        let (h, s, v) = egui::ecolor::hsv_from_rgb(rgb.map(|c| f32::from(c) / 255.0));
        Self { h, s, v }
    }

    fn to_srgb(self) -> [u8; 3] {
        egui::ecolor::rgb_from_hsv((self.h, self.s, self.v)).map(|c| (c * 255.0).round().clamp(0.0, 255.0) as u8)
    }
}

pub(crate) const GESTURE: &str = "track-colour-window";

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Edit {
    Colour(Rgb, bool),
    Default,
    Finish,
    Cancel,
}

pub(crate) struct Panel {
    pub expanded: bool,
    pub dragging: bool,
    cancelled_drag: bool,
    pub colour: Rgb,
    hsv: Hsv,
    text: String,
    name: String,
    numbers: [String; 6],
    invalid: bool,
    wheel: bool,
    steps: bool,
    soft: bool,
    random: u64,
    canvas: canvas::Cache,
}

impl Default for Panel {
    fn default() -> Self {
        let colour = suhina_session::colours::MIDI;
        let mut panel = Self {
            expanded: false,
            dragging: false,
            cancelled_drag: false,
            colour,
            hsv: Hsv::from_srgb([colour.0, colour.1, colour.2]),
            text: String::new(),
            name: String::new(),
            numbers: Default::default(),
            invalid: false,
            wheel: false,
            steps: false,
            soft: false,
            random: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64)
                .max(1),
            canvas: Default::default(),
        };
        panel.fields();
        panel
    }
}

impl Panel {
    pub fn discard_drafts(&mut self) {
        self.fields();
    }

    pub fn cancel_pointer(&mut self) {
        self.dragging = false;
        self.cancelled_drag = true;
    }

    /// Follow commands, Undo and selection changes without converting unchanged exact RGB.
    pub fn sync(&mut self, colour: Rgb) {
        if self.colour != colour {
            self.set(colour);
        }
    }

    fn fields(&mut self) {
        self.text = hex(self.colour);
        self.name = super::names::describe(self.colour);
        let Rgb(r, g, b) = self.colour;
        let [h, s, l] = srgb_to_hsl([r, g, b]);
        self.numbers = [
            r.to_string(),
            g.to_string(),
            b.to_string(),
            format!("{h:.1}"),
            format!("{:.1}", s * 100.0),
            format!("{:.1}", l * 100.0),
        ];
        self.invalid = false;
    }

    fn set(&mut self, colour: Rgb) {
        self.colour = colour;
        let mut next = Hsv::from_srgb([colour.0, colour.1, colour.2]);
        if next.s <= f32::EPSILON || next.v <= f32::EPSILON {
            next.h = self.hsv.h;
        }
        if next.v <= f32::EPSILON {
            next.s = self.hsv.s;
        }
        self.hsv = next;
        self.fields();
    }

    fn choose(&mut self, colour: Rgb, exact: bool, edits: &mut Vec<Edit>) {
        if exact {
            self.steps = false;
            self.soft = false;
        }
        self.set(colour);
        edits.push(Edit::Colour(colour, false));
    }

    fn pointer_colour(&mut self, hsv: Hsv, dragging: bool, edits: &mut Vec<Edit>) {
        if self.cancelled_drag {
            return;
        }
        let hsv = constrain(hsv, self.steps, self.soft);
        let [r, g, b] = hsv.to_srgb();
        let colour = Rgb(r, g, b);
        self.hsv = hsv;
        // Hue can change while black/grey without changing RGB: retain it for the next move.
        if colour != self.colour {
            self.colour = colour;
            self.fields();
            edits.push(Edit::Colour(colour, dragging));
        }
        self.dragging |= dragging;
    }

    pub fn show(&mut self, ui: &mut Ui, recent: &[Rgb]) -> Vec<Edit> {
        let mut edits = Vec::new();
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 5.0);
        ui.spacing_mut().button_padding = Vec2::new(5.0, 2.0);
        ui.label("Palette");
        for row in super::swatches().chunks(8) {
            if let Some(colour) = swatch_line(ui, "Palette", row, self.colour) {
                self.choose(colour, true, &mut edits);
            }
        }
        ui.separator();
        ui.label("Related shades");
        if let Some(colour) = swatch_line(ui, "Related shade", &related_shades(self.colour), self.colour) {
            self.choose(colour, true, &mut edits);
        }
        ui.separator();
        ui.label("Recent colours");
        if recent.is_empty() {
            ui.label(egui::RichText::new("Colours you use appear here").small().color(element::caption_ink()));
        } else if let Some(colour) = swatch_line(ui, "Recent colour", &recent[..recent.len().min(8)], self.colour) {
            self.choose(colour, true, &mut edits);
        }
        ui.separator();
        ui.label(format!("Current colour · {}", self.name))
            .on_hover_text("≈ means the nearest named shade. The hex value remains exact.");
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(62.0, 20.0), Sense::hover());
            ui.painter().rect_filled(rect, 0.0, color32(self.colour));
            let id = Id::unique("track-colour-hex");
            let focused = ui.memory(|m| m.has_focus(id) || m.had_focus_last_frame(id));
            let response = widgets::text_input(
                ui,
                egui::TextEdit::singleline(&mut self.text).id(id).desired_width(100.0).char_limit(16),
            );
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, ui.is_enabled(), "Hex colour"));
            if (focused || response.has_focus()) && ui.input(|i| i.key_pressed(Key::Escape)) {
                ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
                self.fields();
                response.surrender_focus();
            } else if response.lost_focus() || (response.has_focus() && ui.input(|i| i.key_pressed(Key::Enter))) {
                if let Some(colour) = suhina_session::colours::parse_hex(self.text.trim()) {
                    if colour != self.colour {
                        self.choose(colour, true, &mut edits);
                    } else {
                        self.fields();
                    }
                } else {
                    self.invalid = true;
                    // Enter normally surrenders text focus. An invalid draft stays editable
                    // so Escape can discard it and the error does not strand the controls.
                    if ui.input(|i| i.key_pressed(Key::Enter)) {
                        response.request_focus();
                    }
                }
            }
            if ui.button("Copy").on_hover_text("Copy exact #RRGGBB colour").clicked() {
                ui.ctx().copy_text(hex(self.colour));
            }
        });
        let title = if self.expanded { "⌄ Fine adjustment" } else { "› Fine adjustment" };
        if ui.selectable_label(self.expanded, title).clicked() {
            self.expanded = !self.expanded;
        }
        if self.expanded {
            self.fine(ui, &mut edits);
        }
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Random").clicked() {
                self.random ^= self.random << 13;
                self.random ^= self.random >> 7;
                self.random ^= self.random << 17;
                let colour = random_colour(self.colour, self.steps, self.soft, self.random);
                self.choose(colour, false, &mut edits);
            }
            if ui.button("Default").on_hover_text("Restore each selected track's default colour").clicked() {
                self.discard_drafts();
                edits.push(Edit::Default);
            }
            let (caption, ink) = if self.invalid {
                ("Enter a valid colour value", element::LABEL.attribute("kind", "error").look().color())
            } else {
                ("Changes apply immediately", element::caption_ink())
            };
            ui.label(egui::RichText::new(caption).small().color(ink));
        });
        if self.dragging && ui.input(|i| i.key_pressed(Key::Escape)) {
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
            self.cancel_pointer();
            edits.clear();
            edits.push(Edit::Cancel);
        } else if self.dragging && !ui.input(|i| i.pointer.any_down()) {
            self.dragging = false;
            edits.push(Edit::Finish);
        }
        if !ui.input(|i| i.pointer.any_down()) {
            self.cancelled_drag = false;
        }
        edits
    }

    fn fine(&mut self, ui: &mut Ui, edits: &mut Vec<Edit>) {
        ui.horizontal(|ui| {
            if ui.selectable_label(!self.wheel, "Field").clicked() {
                self.wheel = false;
            }
            if ui.selectable_label(self.wheel, "Wheel").clicked() {
                self.wheel = true;
            }
        });
        let mut hsv = self.hsv;
        let response = self.canvas.show(ui, &mut hsv, self.wheel, self.steps, self.soft);
        if response.changed() {
            self.pointer_colour(hsv, response.dragged() || self.dragging, edits);
        }
        let mut hue = self.hsv.h * 360.0;
        let response = ui.add(egui::Slider::new(&mut hue, 0.0..=360.0).text("Hue").suffix("°"));
        if response.changed() {
            let mut hsv = self.hsv;
            hsv.h = hue / 360.0;
            self.pointer_colour(hsv, response.dragged() || self.dragging, edits);
        }
        if self.wheel {
            let mut value = self.hsv.v;
            let response = ui.add(egui::Slider::new(&mut value, 0.0..=1.0).text("Value"));
            if response.changed() {
                let mut hsv = self.hsv;
                hsv.v = value;
                self.pointer_colour(hsv, response.dragged() || self.dragging, edits);
            }
        }
        self.numeric_row(ui, false, edits);
        self.numeric_row(ui, true, edits);
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.steps, "Steps")
                .on_hover_text("Pointer selection: 15° hue and 10% saturation/value steps");
            ui.checkbox(&mut self.soft, "Soft colours").on_hover_text(
                "Pointer selection: 25–70% saturation, 45–90% value. Exact entries remain unrestricted.",
            );
        });
    }

    fn numeric_row(&mut self, ui: &mut Ui, hsl: bool, edits: &mut Vec<Edit>) {
        let start = if hsl { 3 } else { 0 };
        let labels = if hsl { ["H", "S", "L"] } else { ["R", "G", "B"] };
        let names = if hsl { ["HSL hue", "HSL saturation", "HSL lightness"] } else { ["Red", "Green", "Blue"] };
        let mut commit = false;
        let mut entered = None;
        let mut cancel = false;
        ui.horizontal(|ui| {
            for (offset, (label, name)) in labels.into_iter().zip(names).enumerate() {
                ui.label(label);
                let Some(text) = self.numbers.get_mut(start + offset) else { continue };
                let id = Id::unique(("track-colour-number", name));
                let focused = ui.memory(|m| m.has_focus(id) || m.had_focus_last_frame(id));
                let response =
                    widgets::text_input(ui, egui::TextEdit::singleline(text).id(id).desired_width(45.0).char_limit(12));
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, ui.is_enabled(), name));
                cancel |= (focused || response.has_focus()) && ui.input(|i| i.key_pressed(Key::Escape));
                commit |= response.lost_focus() || (response.has_focus() && ui.input(|i| i.key_pressed(Key::Enter)));
                if (focused || response.has_focus()) && ui.input(|i| i.key_pressed(Key::Enter)) {
                    entered = Some(id);
                }
                if cancel {
                    response.surrender_focus();
                }
            }
        });
        if cancel {
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
            self.fields();
            return;
        }
        if !commit {
            return;
        }
        let colour = if hsl {
            let values: Option<Vec<f32>> = self.numbers.iter().skip(3).map(|s| s.trim().parse::<f32>().ok()).collect();
            values.and_then(|v| {
                let [h, s, l] = v.as_slice() else { return None };
                hsl_to_srgb([*h, *s / 100.0, *l / 100.0]).map(|[r, g, b]| Rgb(r, g, b))
            })
        } else {
            let values: Option<Vec<u8>> = self.numbers.iter().take(3).map(|s| s.trim().parse::<u8>().ok()).collect();
            values.and_then(|v| {
                let [r, g, b] = v.as_slice() else { return None };
                Some(Rgb(*r, *g, *b))
            })
        };
        if let Some(colour) = colour {
            // Unedited rounded HSL text must never round-trip an exact RGB colour.
            let current = if hsl {
                let [h, s, l] = srgb_to_hsl([self.colour.0, self.colour.1, self.colour.2]);
                [format!("{h:.1}"), format!("{:.1}", s * 100.0), format!("{:.1}", l * 100.0)]
            } else {
                [self.colour.0.to_string(), self.colour.1.to_string(), self.colour.2.to_string()]
            };
            if self.numbers.get(start..start + 3) != Some(current.as_slice()) {
                self.choose(colour, true, edits);
            } else {
                self.fields();
            }
        } else {
            self.invalid = true;
            if let Some(id) = entered {
                ui.memory_mut(|m| m.request_focus(id));
            }
        }
    }
}

fn swatch_line(ui: &mut Ui, label: &str, colours: &[Rgb], current: Rgb) -> Option<Rgb> {
    let mut picked = None;
    ui.horizontal(|ui| {
        for (index, &colour) in colours.iter().enumerate() {
            ui.push_id((label, index, colour.0, colour.1, colour.2), |ui| {
                let (rect, response) = ui.allocate_exact_size(Vec2::new(30.0, 18.0), Sense::click());
                let name = format!("{label} {}", hex(colour));
                response.widget_info(|| {
                    egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), colour == current, &name)
                });
                ui.painter().rect_filled(rect, 0.0, color32(colour));
                if colour == current || response.hovered() || response.has_focus() {
                    ui.painter().rect_stroke(
                        rect.shrink(1.0),
                        0.0,
                        egui::Stroke::new(2.0, theme::text_on(color32(colour))),
                        egui::StrokeKind::Inside,
                    );
                }
                if response.clicked() {
                    picked = Some(colour);
                }
                response.on_hover_ui(|ui| {
                    ui.label(format!("{} · {}", super::names::describe(colour), hex(colour)));
                });
            });
        }
    });
    picked
}

pub(crate) fn related_shades(colour: Rgb) -> [Rgb; 7] {
    let [l, c, h] = srgb_to_oklch([colour.0, colour.1, colour.2]);
    [0.24, 0.16, 0.08, 0.0, -0.08, -0.16, -0.24].map(|offset| {
        if offset == 0.0 {
            return colour;
        }
        oklch_to_srgb([(l + offset).clamp(0.0, 1.0), c, h]).map_or(colour, |[r, g, b]| Rgb(r, g, b))
    })
}

pub(crate) fn remember(recent: &mut Vec<Rgb>, colour: Rgb) {
    recent.retain(|c| *c != colour);
    recent.insert(0, colour);
    recent.truncate(8);
}

fn constrain(mut hsv: Hsv, steps: bool, soft: bool) -> Hsv {
    if steps {
        hsv.h = (hsv.h * 24.0).round() / 24.0;
        hsv.s = (hsv.s * 10.0).round() / 10.0;
        hsv.v = (hsv.v * 10.0).round() / 10.0;
    }
    hsv.h = hsv.h.rem_euclid(1.0);
    hsv.s = hsv.s.clamp(if soft { 0.25 } else { 0.0 }, if soft { 0.7 } else { 1.0 });
    hsv.v = hsv.v.clamp(if soft { 0.45 } else { 0.0 }, if soft { 0.9 } else { 1.0 });
    hsv
}

pub(crate) fn random_colour(current: Rgb, steps: bool, soft: bool, random: u64) -> Rgb {
    let mut choices = Vec::with_capacity(24);
    for colour in super::swatches() {
        let [r, g, b] = constrain(Hsv::from_srgb([colour.0, colour.1, colour.2]), steps, soft).to_srgb();
        let colour = Rgb(r, g, b);
        if colour != current && !choices.contains(&colour) {
            choices.push(colour);
        }
    }
    if choices.is_empty() {
        return current;
    }
    choices.get((random % choices.len() as u64) as usize).copied().unwrap_or(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_and_numeric_hsl_share_hue_without_changing_exact_rgb() {
        for colour in super::super::swatches().into_iter().chain([Rgb(37, 119, 231), Rgb(0, 0, 0), Rgb(255, 255, 255)])
        {
            let rgb = [colour.0, colour.1, colour.2];
            let hsv = Hsv::from_srgb(rgb);
            assert_eq!(hsv.to_srgb(), rgb);
            assert!((hsv.h * 360.0 - srgb_to_hsl(rgb)[0]).abs() < 0.001);
        }
    }

    #[test]
    fn related_shades_keep_exact_source_hue_and_lightness_order() {
        for colour in super::super::swatches().into_iter().chain([Rgb(0, 0, 0), Rgb(255, 255, 255), Rgb(255, 0, 0)]) {
            let shades = related_shades(colour);
            assert_eq!(shades[3], colour);
            let perceptual = shades.map(|c| srgb_to_oklch([c.0, c.1, c.2]));
            assert!(perceptual.windows(2).all(|w| w[0][0] + 0.005 >= w[1][0]));
            let source = srgb_to_oklch([colour.0, colour.1, colour.2]);
            for [_, chroma, hue] in perceptual {
                if source[1] > 0.02 && chroma > 0.02 {
                    let distance = ((hue - source[2] + 180.0).rem_euclid(360.0) - 180.0).abs();
                    assert!(distance < 3.0, "{colour:?}: hue moved by {distance}");
                }
            }
        }
        for level in [0, 40, 128, 255] {
            assert!(
                related_shades(Rgb(level, level, level))
                    .iter()
                    .all(|c| c.0.abs_diff(c.1) <= 1 && c.1.abs_diff(c.2) <= 1)
            );
        }
    }

    #[test]
    fn recent_colours_are_bounded_recency_without_duplicates_and_round_trip() {
        let mut recent = Vec::new();
        for i in 0..100 {
            remember(&mut recent, Rgb(i, 25, 70));
        }
        assert_eq!(recent.len(), 8);
        assert_eq!(recent[0], Rgb(99, 25, 70));
        remember(&mut recent, Rgb(96, 25, 70));
        assert_eq!(recent.len(), 8);
        assert_eq!(recent[0], Rgb(96, 25, 70));
        assert_eq!(recent.iter().filter(|c| **c == Rgb(96, 25, 70)).count(), 1);
        let settings = crate::settings::Settings { recent_colours: recent.clone(), ..Default::default() };
        assert_eq!(crate::settings::Settings::from_text(&settings.to_text()).0.recent_colours, recent);
        for value in ["#gg0000", "#123", "#abé12", "#000000,#123"] {
            assert!(
                crate::settings::Settings::from_text(&format!("recent_colours = {value}\n"))
                    .0
                    .recent_colours
                    .is_empty()
            );
        }
    }

    #[test]
    fn random_candidates_are_different_and_respect_aids_for_fixed_seeds() {
        for current in super::super::swatches() {
            for seed in 0..48 {
                let free = random_colour(current, false, false, seed);
                assert_ne!(free, current);
                assert!(super::super::swatches().contains(&free));
                let soft = random_colour(current, true, true, seed);
                assert_ne!(soft, current);
                let hsv = Hsv::from_srgb([soft.0, soft.1, soft.2]);
                assert!((0.24..=0.71).contains(&hsv.s));
                assert!((0.44..=0.91).contains(&hsv.v));
            }
        }
    }

    #[test]
    fn exact_values_and_black_keep_their_state_across_views_and_aids() {
        let mut panel = Panel::default();
        let exact = Rgb(37, 119, 231);
        panel.set(exact);
        panel.steps = true;
        panel.soft = true;
        panel.wheel = true;
        panel.sync(exact);
        assert_eq!(panel.colour, exact);
        assert_eq!(panel.text, "#2577e7");
        let hue = panel.hsv.h;
        panel.set(Rgb(0, 0, 0));
        assert_eq!(panel.hsv.h, hue);
        let mut edits = Vec::new();
        panel.choose(exact, true, &mut edits);
        assert!(!panel.steps && !panel.soft);
        assert_eq!(edits, [Edit::Colour(exact, false)]);
    }
}
