// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The record controls ([`docs/spec/recording.md`](../../../../docs/spec/recording.md) › In the
//! window): the control bar's record button (`transport.record`) and count-in menu
//! (`transport.setCountIn`), and the record button an armed track's empty slots show
//! (`clip.record`; a click on a slot that records ends its take with `clip.launch`).

use egui::{Align2, Id, Painter, Rect, Stroke, Ui, Vec2};
use serde_json::json;

use crate::kit::widgets::{self, Action};
use crate::kit::{motion, theme};

/// The count-in menu's accessible name.
pub const COUNT_IN_LABEL: &str = "Count-in";
/// The count-in menu's entries: bars, and what it shows.
const COUNT_INS: [(u8, &str); 4] = [(0, "Off"), (1, "1 bar"), (2, "2 bars"), (4, "4 bars")];

/// The control bar's record button in `rect`: lit while the arrangement records; a click
/// starts or ends recording into the arrangement.
pub fn record_button(ui: &Ui, painter: &Painter, rect: Rect, recording: bool, actions: &mut Vec<Action>) {
    let label = if recording { "Stop recording" } else { "Record into the arrangement" };
    let response = widgets::button_area(ui, rect, Id::new("record"), label);
    if response.clicked() {
        actions.push(Action::new("transport.record", json!({ "on": !recording })));
    }
    let lit = motion::toward(ui.ctx(), response.id.with("on"), recording, motion::VALUE);
    let hover = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
    painter.rect_filled(rect, 0.0, motion::mix(hover, theme::recording_background(), lit));
    let ring = motion::mix(theme::text_primary(), theme::recording(), lit);
    painter.circle_stroke(rect.center(), 4.5, Stroke::new(1.2, ring));
    painter.circle_filled(rect.center(), 4.5, theme::recording().gamma_multiply(0.35 + 0.65 * lit));
}

/// The metronome's switch in `rect`: lit while it clicks; a click turns it on or off, the
/// right-click menu picks its sound (`transport.setMetronome`).
pub fn metronome_button(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    (on, _, sound): (bool, f64, suhina_engine::ClickSound),
    actions: &mut Vec<Action>,
) {
    let label = if on { "Metronome off" } else { "Metronome on" };
    let response = widgets::button_area(ui, rect, Id::new("metronome"), label)
        .on_hover_text("Metronome: a click on every beat while playing. Right-click: its sound");
    if response.clicked() {
        actions.push(Action::new("transport.setMetronome", json!({ "on": !on })));
    }
    response.context_menu(|ui| {
        for choice in suhina_engine::ClickSound::ALL {
            let name = match choice {
                suhina_engine::ClickSound::Beep => "Beep",
                suhina_engine::ClickSound::Wood => "Wood",
                suhina_engine::ClickSound::Soft => "Soft",
            };
            if ui.selectable_label(choice == sound, name).clicked() && choice != sound {
                actions.push(Action::new("transport.setMetronome", json!({ "on": on, "sound": choice.name() })));
                ui.close();
            }
        }
    });
    let lit = motion::toward(ui.ctx(), response.id.with("on"), on, motion::VALUE);
    let hover = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
    painter.rect_filled(rect, 0.0, motion::mix(hover, theme::control_active(), lit));
    metronome_glyph(painter, rect, motion::mix(theme::text_primary(), theme::accent(), lit));
}

/// A small metronome: its body and its arm, centred in `rect`.
pub fn metronome_glyph(painter: &Painter, rect: Rect, color: egui::Color32) {
    let c = rect.center();
    let body =
        vec![c + Vec2::new(-4.5, 5.5), c + Vec2::new(-1.5, -5.5), c + Vec2::new(1.5, -5.5), c + Vec2::new(4.5, 5.5)];
    painter.add(egui::Shape::closed_line(body, Stroke::new(1.2, color)));
    painter.line_segment([c + Vec2::new(0.0, 3.5), c + Vec2::new(3.5, -4.0)], Stroke::new(1.2, color));
}

/// The count-in menu in `rect`: the bars recording counts in when it starts the transport.
pub fn count_in_menu(ui: &mut Ui, rect: Rect, bars: u8, actions: &mut Vec<Action>) {
    let mono = theme::mono_font(theme::TEXT);
    let mut chosen = bars;
    let shown = COUNT_INS.iter().find(|(b, _)| *b == bars).map_or("Off", |(_, name)| name);
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        let response = egui::ComboBox::from_id_salt("count-in")
            .width(rect.width())
            .selected_text(egui::RichText::new(shown).font(mono.clone()).color(theme::text_value()))
            .show_ui(ui, |ui| {
                for (value, name) in COUNT_INS {
                    ui.selectable_value(&mut chosen, value, egui::RichText::new(name).font(mono.clone()));
                }
            })
            .response;
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, COUNT_IN_LABEL));
        response.on_hover_text("Count-in: clicks before recording starts the transport");
    });
    if chosen != bars {
        actions.push(Action::new("transport.setCountIn", json!({ "bars": chosen })));
    }
}

/// The record button at the left of an armed track's empty slot `rect` (scene `number`, from
/// 1), drawn over the slot; `recording` while this slot records. A click starts the take, or
/// ends it.
#[allow(clippy::too_many_arguments)]
pub fn slot_record(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    width: f32,
    track: u32,
    track_name: &str,
    number: usize,
    recording: bool,
    actions: &mut Vec<Action>,
) {
    let button = Rect::from_min_size(rect.min, Vec2::new(width.min(rect.width()), rect.height()));
    let label = if recording {
        format!("End the recording on {track_name} (scene {number})")
    } else {
        format!("Record on {track_name} (scene {number})")
    };
    let response = widgets::button_area(ui, button, Id::new(("slot-record", track, number)), label);
    if response.clicked() {
        let id = if recording { "clip.launch" } else { "clip.record" };
        actions.push(Action::new(id, json!({ "track": track, "scene": number })));
    }
    let pulse = if recording { motion::pulse(ui.ctx()) } else { 1.0 };
    if recording {
        painter.rect_filled(rect, 0.0, theme::recording_background());
        widgets::text_in(
            painter,
            rect.shrink2(Vec2::new(width, 0.0)),
            Align2::LEFT_CENTER,
            "Recording",
            theme::ui_font(theme::TEXT_SMALL),
            theme::recording(),
        );
    }
    let hover = widgets::hover(ui, &response);
    let color = motion::mix(theme::text_secondary(), theme::recording(), hover.max(f32::from(u8::from(recording))));
    let radius = (rect.height() * 0.2).clamp(2.5, 4.0);
    painter.circle_filled(button.center(), radius, color.gamma_multiply(pulse));
}
