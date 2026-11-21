// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The record controls ([`docs/spec/recording.md`](../../../../docs/spec/recording.md) › In the
//! window): the control bar's record button (`transport.record`) and count-in menu
//! (`transport.setCountIn`), and the record button an armed track's empty slots show
//! (`clip.record`; a click on a slot that records ends its take with `clip.launch`).

use egui::{Align2, Id, Painter, Pos2, Rect, Stroke, Ui, Vec2};
use serde_json::json;

use crate::kit::element::{BUTTON, Checked, POPUP, Recording, SLIDER, SLIDER_FILL, SLIDER_HANDLE, SLIDER_RAIL};
use crate::kit::widgets::{self, Action, Lit};
use crate::kit::{menu, motion, theme};

/// The count-in menu's accessible name.
pub const COUNT_IN_LABEL: &str = "Count-in";
/// The count-in menu's entries: bars, and what it shows.
const COUNT_INS: [(u8, &str); 4] = [(0, "Off"), (1, "1 bar"), (2, "2 bars"), (4, "4 bars")];

/// The control bar's record button in `rect`: lit while the arrangement records; a click
/// starts or ends recording into the arrangement.
pub fn record_button(ui: &Ui, painter: &Painter, rect: Rect, recording: bool, actions: &mut Vec<Action>) {
    let label = if recording { "Stop recording" } else { "Record into the arrangement" };
    let response = widgets::button_area(ui, rect, Id::unique("record"), label);
    if response.clicked() {
        actions.push(Action::new("transport.record", json!({ "on": !recording })));
    }
    // `button#record`, `[checked][recording]` while it records: the sheet's `button[kind="record"]`
    // gives its fill and ring at rest and lit. The dot's ink is the lit one, at rest too.
    let dot = BUTTON.id("record").attribute("kind", "record").attr(Checked, true).attr(Recording, true).look().color();
    let Lit { el, toggle, lit } =
        widgets::lit_toggle_kind(ui, &response, BUTTON.id("record"), &[Checked, Recording], recording, "record");
    let (fill, ring) = toggle.at(lit);
    widgets::toggle_surface(painter, rect, &el, &response, fill, toggle.image(lit), 0.0);
    if crate::kit::skin::paint(
        painter,
        rect,
        "record",
        crate::kit::skin::State::response(&response, recording),
        el.color_or(dot),
        0.0,
    ) {
        return;
    }
    painter.circle_stroke(rect.center(), 4.5, Stroke::new(1.2, ring));
    painter.circle_filled(rect.center(), 4.5, dot.gamma_multiply(0.35 + 0.65 * lit));
}

/// The metronome's level in dB, lowest and highest, as its slider sets it.
pub const METRONOME_RANGE: (f64, f64) = (-60.0, 6.0);
/// The level slider's height.
const LEVEL_SLIDER: f32 = 128.0;

/// The metronome's switch in `rect`: lit while it clicks; a click turns it on or off, held and
/// dragged it sets its level ([`metronome_level`]), the right-click menu picks its sound and
/// where it plays ([`metronome_menu`], `transport.setMetronome`).
pub fn metronome_button(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    metronome: &suhina_session::MetronomeState,
    route: suhina_engine::ClickRoute,
    actions: &mut Vec<Action>,
) {
    let (on, db) = (metronome.on, metronome.db);
    let label = if on { "Metronome off" } else { "Metronome on" };
    let response = ui.interact(rect, Id::unique("metronome"), egui::Sense::click_and_drag()).on_hover_text(
        "Metronome: a click on every beat while playing. Hold and drag up or down: its level. Right-click: its sound and where it plays",
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if response.clicked() {
        actions.push(Action::new("transport.setMetronome", json!({ "on": !on })));
    }
    metronome_menu_on(&response, metronome, route, actions);
    // `button#metronome-toggle`, `[checked]` while it clicks: the sheet's `button[kind="metronome"]`.
    // The glyph keeps reading on a lit fill the sheet gives (`widgets::lit_toggle_kind`).
    let Lit { el, toggle, lit } =
        widgets::lit_toggle_kind(ui, &response, BUTTON.id("metronome-toggle"), &[Checked], on, "metronome");
    let (fill, ink) = toggle.at(lit);
    widgets::toggle_surface(painter, rect, &el, &response, fill, toggle.image(lit), 0.0);
    metronome_glyph(painter, rect, ink);
    drop(el);
    // The level's slider is a popup of its own (`#metronome-level`), not part of the button.
    metronome_level(ui, &response, rect, (on, db), actions);
}

/// The metronome's right-click menu on a button's `response`. As it opens, the library's
/// metronome sounds are looked for again (`transport.metronomeSounds`), so a folder added since
/// is in it from the next frame.
pub fn metronome_menu_on(
    response: &egui::Response,
    metronome: &suhina_session::MetronomeState,
    route: suhina_engine::ClickRoute,
    actions: &mut Vec<Action>,
) {
    if response.secondary_clicked() {
        actions.push(Action::new("transport.metronomeSounds", json!({})));
    }
    response.context_menu(|ui| metronome_menu(ui, metronome, route, actions));
}

/// The metronome's right-click menu: its sound and the bar's, each by family, then where its
/// click joins the output (`transport.setMetronome`), the current ones marked. Its rows are the
/// kit's ([`crate::kit::menu`]), so the theme styles them.
pub fn metronome_menu(
    ui: &mut Ui,
    metronome: &suhina_session::MetronomeState,
    route: suhina_engine::ClickRoute,
    actions: &mut Vec<Action>,
) {
    use suhina_engine::ClickRoute;
    let on = metronome.on;
    menu::submenu(ui, "Sound", false, |ui| {
        if let Some(id) = sound_choices(ui, metronome, Some(metronome.sound.as_str())) {
            actions.push(Action::new("transport.setMetronome", json!({ "on": on, "sound": id })));
            ui.close();
        }
    })
    .on_hover_text("The sound every beat plays");
    menu::submenu(ui, "Bar sound", false, |ui| {
        let higher = metronome.accent.is_none();
        if menu::item(ui, "Beat sound, higher", higher)
            .on_hover_text("The first beat of a bar plays the beat's sound, higher")
            .clicked()
            && !higher
        {
            actions.push(Action::new("transport.setMetronome", json!({ "on": on, "accent": "higher" })));
            ui.close();
        }
        menu::separator(ui);
        if let Some(id) = sound_choices(ui, metronome, metronome.accent.as_deref()) {
            actions.push(Action::new("transport.setMetronome", json!({ "on": on, "accent": id })));
            ui.close();
        }
    })
    .on_hover_text("The sound the first beat of a bar plays");
    menu::separator(ui);
    menu::heading(ui, "Plays");
    for choice in ClickRoute::ALL {
        let (name, hint) = match choice {
            ClickRoute::PreFader => {
                ("Pre-fader", "Past the main channel: its devices, fader and mute leave the click alone")
            }
            ClickRoute::PostFader => ("Post-fader", "Past the main channel's devices, at its fader and mute"),
            ClickRoute::Cue => ("Cue output only", "Only on the cue output, when it has outputs of its own"),
            ClickRoute::ThroughMain => ("Through Main", "Into the main channel: its devices, fader and mute apply"),
        };
        if menu::item(ui, name, choice == route).on_hover_text(hint).clicked() && choice != route {
            actions.push(Action::new("transport.setMetronome", json!({ "on": on, "route": choice.name() })));
            ui.close();
        }
    }
}

/// A submenu for each family of the metronome's sounds, the one holding `current` marked with a
/// dot and `current` chosen in it; the id of the sound clicked, when it is another.
fn sound_choices(ui: &mut Ui, metronome: &suhina_session::MetronomeState, current: Option<&str>) -> Option<String> {
    let mut picked = None;
    for family in metronome.families.iter() {
        let holds = family.sounds.iter().any(|(id, _)| Some(id.as_str()) == current);
        let hint = match &family.folder {
            Some(folder) => format!("Sound files in the library's Sounds/Metronome/{folder}"),
            None => "Built into Suhina".to_string(),
        };
        menu::submenu(ui, &family.name, holds, |ui| {
            for (id, name) in &family.sounds {
                let here = Some(id.as_str()) == current;
                if menu::item(ui, name, here).clicked() && !here {
                    picked = Some(id.clone());
                }
            }
        })
        .on_hover_text(hint);
    }
    menu::separator(ui);
    menu::hint(ui, "Your own: a folder of sound files in the library's Sounds/Metronome");
    picked
}

/// While the metronome's button (`response`, in `button`) is held: its level's slider beside it,
/// above, or below where there is no room above (#684). Dragged, the pointer's height on the
/// slider is the level, so the whole range is in reach in one drag; a press that does not move
/// changes nothing, so a click still only turns it on or off.
pub fn metronome_level(
    ui: &Ui,
    response: &egui::Response,
    button: Rect,
    (on, db): (bool, f64),
    actions: &mut Vec<Action>,
) {
    if !response.is_pointer_button_down_on() {
        return;
    }
    let (low, high) = METRONOME_RANGE;
    let screen = ui.ctx().content_rect();
    let above = button.top() - screen.top() >= LEVEL_SLIDER + 2.0;
    let slider = if above {
        Rect::from_min_max(
            Pos2::new(button.left(), button.top() - 2.0 - LEVEL_SLIDER),
            Pos2::new(button.right(), button.top() - 2.0),
        )
    } else {
        Rect::from_min_max(
            Pos2::new(button.left(), button.bottom() + 2.0),
            Pos2::new(button.right(), button.bottom() + 2.0 + LEVEL_SLIDER),
        )
    };
    let track = Rect::from_min_max(
        Pos2::new(slider.center().x - 2.0, slider.top() + 18.0),
        Pos2::new(slider.center().x + 2.0, slider.bottom() - 6.0),
    );
    let mut level = if db.is_finite() { db.clamp(low, high) } else { low };
    if response.dragged()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let at = f64::from(((track.bottom() - pointer.y) / track.height()).clamp(0.0, 1.0));
        let next = ((low + at * (high - low)) * 2.0).round() / 2.0;
        if (next - level).abs() > 1e-9 {
            level = next;
            actions.push(Action::new("transport.setMetronome", json!({ "on": on, "db": next })));
        }
    }
    let over = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, response.id.with("level")));
    // `popup#metronome-level`, with a `slider` and its rail, fill and handle in it.
    let popup = POPUP.id("metronome-level").attribute("kind", "level").enter();
    over.rect_filled(slider, 0.0, popup.background());
    over.rect_stroke(slider, 0.0, theme::hairline(popup.border()), egui::StrokeKind::Inside);
    let el = SLIDER.enter();
    over.rect_filled(track, 0.0, SLIDER_RAIL.look().background());
    let position = ((level - low) / (high - low)) as f32;
    let y = track.bottom() - track.height() * position;
    let fill = SLIDER_FILL.look().background();
    over.rect_filled(Rect::from_min_max(Pos2::new(track.left(), y), track.max), 0.0, fill);
    over.rect_filled(
        Rect::from_center_size(Pos2::new(track.center().x, y), Vec2::new(18.0, 5.0)),
        0.0,
        SLIDER_HANDLE.look().background(),
    );
    widgets::text_in(
        &over,
        Rect::from_min_max(slider.min, Pos2::new(slider.right(), slider.top() + 16.0)),
        Align2::CENTER_CENTER,
        &format!("{level:.0}"),
        theme::mono_font(theme::TEXT_SMALL),
        el.color(),
    );
}

/// A small metronome: its body and its arm, centered in `rect`.
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
        let response = crate::kit::widgets::combo_box(
            ui,
            egui::ComboBox::from_id_salt("count-in")
                .width(rect.width())
                .selected_text(egui::RichText::new(shown).font(mono.clone()).color(crate::kit::element::value_ink())),
            |ui| {
                for (value, name) in COUNT_INS {
                    ui.selectable_value(&mut chosen, value, egui::RichText::new(name).font(mono.clone()));
                }
            },
        )
        .response;
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, COUNT_IN_LABEL));
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
    let response = widgets::button_area(ui, button, Id::unique(("slot-record", track, number)), label);
    if response.clicked() {
        let id = if recording { "clip.launch" } else { "clip.record" };
        actions.push(Action::new(id, json!({ "track": track, "scene": number.saturating_sub(1) })));
    }
    let pulse = if recording { motion::pulse(ui.ctx()) } else { 1.0 };
    // `slot-record`, `[recording]` while its slot records: its `color` for the dot and the word,
    // its `background-color` over the slot while it records.
    let el = crate::kit::element::SLOT_RECORD.response(&response).attr(Recording, recording).enter();
    if recording {
        painter.rect_filled(rect, 0.0, el.background());
        widgets::text_in(
            painter,
            rect.shrink2(Vec2::new(width, 0.0)),
            Align2::LEFT_CENTER,
            "Recording",
            theme::ui_font(theme::TEXT_SMALL),
            el.color(),
        );
    }
    let color = el.hover_fade(widgets::hover(ui, &response), crate::kit::element::Look::color);
    let radius = (rect.height() * 0.2).clamp(2.5, 4.0);
    painter.circle_filled(button.center(), radius, color.gamma_multiply(pulse));
}
