// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! One selected colour stop, with direct manipulation on a shared gradient ramp.
use super::settings_view::{PageState, ThemeRequest, ThemesView, edit_rgba};
use crate::kit::surface::Backgrounds as _;
use crate::kit::{
    gradient::{Gradient, Stop},
    theme,
};
use egui::{Align2, Pos2, Rect, Sense, Ui, Vec2};

pub(super) fn show(ui: &mut Ui, themes: &ThemesView, state: &mut PageState) -> Option<ThemeRequest> {
    let mut gradients = themes.gradients.clone();
    ui.add_space(8.0);
    ui.label(egui::RichText::new("Surface gradients").strong());
    ui.label("Select a colour stop below the ramp. Drag it to move it.");
    for (name, slot) in
        [("Panel", &mut gradients.panels), ("Track", &mut gradients.tracks), ("Clip", &mut gradients.clips)]
    {
        ui.push_id(("surface-gradient", name), |ui| {
            ui.collapsing(format!("{name} gradient"), |ui| {
                let mut enabled = slot.is_some();
                if ui.checkbox(&mut enabled, format!("Enable {name} gradient")).changed() {
                    *slot = enabled.then(Gradient::default);
                }
                let Some(gradient) = slot else { return };
                if !(2..=8).contains(&gradient.stops.len()) {
                    *gradient = Gradient::default();
                }
                for stop in &mut gradient.stops {
                    stop.position = stop.position.min(100);
                }
                gradient.stops.sort_by_key(|stop| stop.position);
                let selected_id = ui.scope_id().with("selected-stop");
                let mut selected = ui
                    .ctx()
                    .data(|data| data.get_temp::<usize>(selected_id))
                    .unwrap_or(0)
                    .min(gradient.stops.len() - 1);
                let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 58.0), Sense::hover());
                let ramp = Rect::from_min_max(area.min + Vec2::new(9.0, 0.0), area.max - Vec2::new(9.0, 25.0));
                ui.painter().fill_background(ramp, 0.0, theme::panel_background().to_opaque());
                // Positions always run left to right; direction is a separate surface setting.
                let mut preview = gradient.clone();
                preview.angle = 0;
                ui.painter().add(preview.mesh(ramp));
                for index in 0..gradient.stops.len() {
                    let low = index.checked_sub(1).map_or(0, |i| gradient.stops[i].position);
                    let high = gradient.stops.get(index + 1).map_or(100, |stop| stop.position);
                    let stop = &mut gradient.stops[index];
                    let x = ramp.left() + ramp.width() * f32::from(stop.position) / 100.0;
                    let handle = Rect::from_center_size(Pos2::new(x, ramp.bottom() + 12.0), Vec2::new(18.0, 20.0));
                    let response = ui.interact(handle, ui.scope_id().with(("stop", index)), Sense::click_and_drag());
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Select {name} stop {}", index + 1))
                    });
                    if response.clicked() || response.dragged() {
                        selected = index;
                    }
                    if response.dragged()
                        && let Some(pointer) = response.interact_pointer_pos()
                    {
                        stop.position = (((pointer.x - ramp.left()) / ramp.width() * 100.0)
                            .round()
                            .clamp(f32::from(low), f32::from(high))) as u8;
                    }
                    state.theme_colour_held |= response.dragged() || response.is_pointer_button_down_on();
                    let fill = if selected == index {
                        theme::control_selected()
                    } else {
                        theme::control_background().to_opaque()
                    };
                    ui.painter().fill_background(handle, 3.0, fill);
                    ui.painter().text(
                        handle.center(),
                        Align2::CENTER_CENTER,
                        (index + 1).to_string(),
                        theme::mono_font(10.0),
                        theme::text_on(fill),
                    );
                }
                ui.label(format!("Colour stop {} of {}", selected + 1, gradient.stops.len()));
                let low = selected.checked_sub(1).map_or(0, |i| gradient.stops[i].position);
                let high = gradient.stops.get(selected + 1).map_or(100, |stop| stop.position);
                if let Some(stop) = gradient.stops.get_mut(selected) {
                    let mut rgba = stop.rgba();
                    ui.horizontal(|ui| {
                        ui.label("Colour");
                        let popup = ui.auto_id_with("popup");
                        let picker = edit_rgba(ui, &mut rgba, egui::color_picker::Alpha::OnlyBlend);
                        picker.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::Role::ColorWell,
                                true,
                                format!("{name} stop {} colour", selected + 1),
                            )
                        });
                        state.theme_colour_held |=
                            egui::Popup::is_id_open(ui.ctx(), popup) && ui.input(|i| i.pointer.primary_down());
                        ui.label(theme::hex_rgba(rgba));
                    });
                    let position = ui.add(
                        egui::Slider::new(&mut stop.position, low..=high)
                            .suffix(" %")
                            .text(format!("{name} stop {} position", selected + 1)),
                    );
                    let mut opacity = (f32::from(rgba[3]) * 100.0 / 255.0).round() as u8;
                    let alpha = ui.add(
                        egui::Slider::new(&mut opacity, 0..=100)
                            .suffix(" %")
                            .text(format!("{name} stop {} opacity", selected + 1)),
                    );
                    state.theme_colour_held |= position.dragged()
                        || position.is_pointer_button_down_on()
                        || alpha.dragged()
                        || alpha.is_pointer_button_down_on();
                    if alpha.changed() {
                        rgba[3] = (f32::from(opacity) * 2.55).round() as u8;
                    }
                    stop.set_rgba(rgba);
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(gradient.stops.len() < 8, egui::Button::new(format!("Add {name} colour stop")))
                        .clicked()
                    {
                        let gap = gradient
                            .stops
                            .windows(2)
                            .enumerate()
                            .max_by_key(|(_, pair)| pair[1].position.saturating_sub(pair[0].position))
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                        let left = &gradient.stops[gap];
                        let right = &gradient.stops[gap + 1];
                        let stop = Stop {
                            transparent_rgb: None,
                            position: left.position + (right.position - left.position) / 2,
                            color: crate::kit::motion::mix(left.color, right.color, 0.5),
                        };
                        gradient.stops.insert(gap + 1, stop);
                        selected = gap + 1;
                    }
                    if ui
                        .add_enabled(
                            gradient.stops.len() > 2,
                            egui::Button::new(format!("Remove stop {}", selected + 1)),
                        )
                        .clicked()
                    {
                        gradient.stops.remove(selected);
                        selected = selected.min(gradient.stops.len() - 1);
                    }
                });
                ui.separator();
                let direction = ui
                    .add(egui::Slider::new(&mut gradient.angle, 0..=360).suffix("°").text(format!("{name} direction")));
                state.theme_colour_held |= direction.dragged() || direction.is_pointer_button_down_on();
                ui.label(egui::RichText::new("0° left → right · 90° top → bottom").small());
                ui.ctx().data_mut(|data| data.insert_temp(selected_id, selected));
            });
        });
    }
    (gradients != themes.gradients).then_some(ThemeRequest::EditGradients(gradients))
}
