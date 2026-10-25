// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A device's parameters drawn in its panel: cells, knobs, switches and their layout.

use super::*;

pub(super) fn format_value(value: f64) -> String {
    if !value.is_finite() {
        return "-".into();
    }
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// An `.sd` colour.
pub(super) fn rgb([r, g, b]: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(r, g, b)
}

/// One parameter in its cell `row`: its panel colour, its label, its control and its value, as
/// its `ui` fields say. A change is one `device.setParam`.
pub(super) fn parameter_cell(
    ui: &mut Ui,
    painter: &egui::Painter,
    row: Rect,
    parameter: &GraphParameter,
    track: TrackId,
    at: &DeviceAt,
    actions: &mut Vec<Action>,
) {
    let style = &parameter.ui;
    let family = if style.monospace { egui::FontFamily::Monospace } else { egui::FontFamily::Proportional };
    let font = egui::FontId::new(style.font_size, family);
    let foreground = style.text_color.map_or_else(theme::text_secondary, rgb);
    let radius = match style.shape {
        ControlShape::Square => 0.0,
        ControlShape::Pill => 128.0,
        ControlShape::Rounded => style.corner_radius,
    };
    if let Some(color) = style.panel_color {
        painter.rect_filled(row, radius, rgb(color));
    }
    let row = row.shrink(4.0);
    let name_rect = match style.label_position {
        LabelPosition::Left => Rect::from_min_size(row.min, Vec2::new(row.width() * 0.34, row.height())),
        LabelPosition::Top => Rect::from_min_size(row.min, Vec2::new(row.width(), style.font_size + 4.0)),
        LabelPosition::Hidden => Rect::from_min_size(row.min, Vec2::ZERO),
    };
    if style.label_position != LabelPosition::Hidden {
        let align =
            if style.label_position == LabelPosition::Top { Align2::CENTER_CENTER } else { Align2::LEFT_CENTER };
        let name = if style.group.is_empty() {
            parameter.name.clone()
        } else {
            format!("{} / {}", style.group, parameter.name)
        };
        widgets::text_in(painter, name_rect, align, &name, font.clone(), foreground);
    }
    let value_height =
        if style.show_value && style.label_position == LabelPosition::Top { style.font_size + 4.0 } else { 0.0 };
    let value_width = if style.show_value && value_height == 0.0 { (row.width() * 0.24).min(80.0) } else { 0.0 };
    let control_min = match style.label_position {
        LabelPosition::Left => Pos2::new(name_rect.right(), row.top()),
        LabelPosition::Top => Pos2::new(row.left(), name_rect.bottom()),
        LabelPosition::Hidden => row.min,
    };
    let slider_rect =
        Rect::from_min_max(control_min, Pos2::new(row.right() - value_width, row.bottom() - value_height));
    let mut value = parameter.value;
    let response = (parameter.min < parameter.max).then(|| {
        let builder = egui::UiBuilder::new()
            .max_rect(slider_rect)
            .layout(egui::Layout::centered_and_justified(egui::Direction::TopDown));
        ui.scope_builder(builder, |ui| {
            ui.style_mut().override_font_id = Some(font.clone());
            let accent = style.accent.map(rgb);
            let visuals = ui.visuals_mut();
            visuals.override_text_color = Some(foreground);
            if let Some(accent) = accent {
                visuals.selection.bg_fill = accent;
                visuals.widgets.active.bg_fill = accent;
                visuals.widgets.hovered.bg_stroke.color = accent;
                visuals.widgets.active.fg_stroke.color = accent;
            }
            for widget in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active] {
                widget.corner_radius = egui::CornerRadius::same(radius as u8);
                if let Some(background) = style.background {
                    widget.bg_fill = rgb(background);
                    widget.weak_bg_fill = widget.bg_fill;
                }
                if let Some(accent) = accent {
                    widget.fg_stroke.color = accent;
                }
            }
            ui.set_clip_rect(ui.clip_rect().intersect(slider_rect));
            ui.spacing_mut().slider_width = if parameter.control == ParameterControl::VerticalSlider {
                slider_rect.height()
            } else {
                slider_rect.width()
            };
            ui.push_id((&parameter.id, at, track.0), |ui| {
                let previous = value;
                let mut response = parameter_widget(ui, parameter, slider_rect, accent, &mut value);
                if response.double_clicked() {
                    value = parameter.default;
                }
                if value != previous {
                    response.mark_changed();
                }
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, &parameter.name));
                response.on_hover_text(&style.tooltip)
            })
            .inner
        })
        .inner
    });
    if style.show_value {
        let (rect, align) = if value_height > 0.0 {
            (Rect::from_min_max(Pos2::new(row.left(), slider_rect.bottom()), row.max), Align2::CENTER_CENTER)
        } else {
            (Rect::from_min_max(Pos2::new(slider_rect.right(), control_min.y), row.max), Align2::RIGHT_CENTER)
        };
        let color = style.text_color.map_or_else(theme::text_value, rgb);
        widgets::text_in(painter, rect, align, &value_text(parameter, value), font, color);
    }
    if let Some(response) = response.filter(|r| r.changed() || r.drag_stopped()) {
        let continuous = (response.drag_started() || response.dragged()) && !response.drag_stopped();
        actions.push(Action::new(
            "device.setParam",
            json!({ "track": track.0, "device": at.value(), "param": parameter.id, "value": value, "continuous": continuous }),
        ));
    }
}

/// The text beside a parameter's control at `value`: a choice's label, the device's own text
/// for the value it listed, or the number with its unit.
pub(super) fn value_text(parameter: &GraphParameter, value: f64) -> String {
    if matches!(parameter.control, ParameterControl::Choice | ParameterControl::Radio) {
        parameter.ui.options.get(value as usize).cloned().unwrap_or_default()
    } else if !parameter.text.is_empty() && value == parameter.value {
        parameter.text.clone()
    } else {
        format!("{:.*} {}", parameter.ui.decimals, value, parameter.unit).trim().to_owned()
    }
}

/// The control `parameter.control` names, in `rect`, editing `value`.
pub(super) fn parameter_widget(
    ui: &mut Ui,
    parameter: &GraphParameter,
    rect: Rect,
    accent: Option<egui::Color32>,
    value: &mut f64,
) -> egui::Response {
    let style = &parameter.ui;
    match parameter.control {
        ParameterControl::Toggle => {
            let mut checked = *value >= 0.5;
            let response = ui.checkbox(&mut checked, "");
            // Only a click writes: a value automation plays between off and on stays as it is.
            if response.changed() {
                *value = f64::from(checked);
            }
            response
        }
        ParameterControl::Button => {
            let response = ui.selectable_label(*value >= 0.5, &parameter.name);
            if response.clicked() {
                *value = if *value >= 0.5 { 0.0 } else { 1.0 };
            }
            response
        }
        ParameterControl::Choice => {
            egui::ComboBox::from_id_salt("choice")
                .selected_text(style.options.get(*value as usize).map_or("", String::as_str))
                .width(rect.width())
                .show_ui(ui, |ui| {
                    for (i, label) in style.options.iter().enumerate() {
                        ui.selectable_value(&mut *value, i as f64, label);
                    }
                })
                .response
        }
        ParameterControl::Radio => {
            ui.vertical_centered(|ui| {
                for (i, label) in style.options.iter().enumerate() {
                    ui.selectable_value(&mut *value, i as f64, label);
                }
            })
            .response
        }
        ParameterControl::Number => ui.add(
            egui::DragValue::new(value)
                .range(parameter.min..=parameter.max)
                .speed(if style.step > 0.0 {
                    style.step
                } else {
                    (parameter.max - parameter.min) / widgets::PARAMETER_DRAG_PIXELS
                })
                .max_decimals(style.decimals),
        ),
        ParameterControl::Knob | ParameterControl::LogKnob => knob(ui, parameter, rect, accent, value),
        ParameterControl::VerticalSlider => {
            let rect = Rect::from_center_size(rect.center(), Vec2::new(ui.spacing().interact_size.y, rect.height()));
            let mut slider_ui =
                ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Center)));
            slider_ui.add(
                egui::Slider::new(value, parameter.min..=parameter.max)
                    .clamping(egui::SliderClamping::Edits)
                    .vertical()
                    .show_value(false)
                    .trailing_fill(style.accent.is_some())
                    .max_decimals(style.decimals)
                    .step_by(style.step),
            )
        }
        // Edits clamping: the slider rounds a value to its step and decimals only when it is
        // edited, not every time it is drawn, so a value automation plays between steps is not
        // sent back as an edit (which turns the lane off).
        control => ui.add(
            egui::Slider::new(value, parameter.min..=parameter.max)
                .clamping(egui::SliderClamping::Edits)
                .show_value(false)
                .trailing_fill(style.accent.is_some())
                .max_decimals(style.decimals)
                .logarithmic(control == ParameterControl::LogSlider)
                .step_by(if control == ParameterControl::Integer { 1.0 } else { style.step }),
        ),
    }
}

/// A knob in `rect`, logarithmic for a `LogKnob`: dragged up and down (finer with Shift),
/// stepped by the arrow keys once clicked, back to its default on a double click.
pub(super) fn knob(
    ui: &mut Ui,
    parameter: &GraphParameter,
    rect: Rect,
    accent: Option<egui::Color32>,
    value: &mut f64,
) -> egui::Response {
    let style = &parameter.ui;
    let (min, max) = (parameter.min, parameter.max);
    let logarithmic = parameter.control == ParameterControl::LogKnob;
    let size = rect.width().min(rect.height()).max(8.0);
    let (rect, mut response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
    if response.clicked() {
        response.request_focus();
    }
    if response.double_clicked() {
        *value = parameter.default;
    }
    if response.has_focus() {
        let direction = ui.input(|i| {
            i32::from(i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowRight))
                - i32::from(i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowLeft))
        });
        let step = if style.step > 0.0 { style.step } else { (max - min) * 0.01 };
        *value = (*value + f64::from(direction) * step).clamp(min, max);
    }
    let mut position = if logarithmic { (*value / min).ln() / (max / min).ln() } else { (*value - min) / (max - min) };
    if response.dragged() {
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let delta = f64::from(response.drag_delta().y) / widgets::PARAMETER_DRAG_PIXELS * fine;
        position = (position - delta).clamp(0.0, 1.0);
        *value = if logarithmic { min * (max / min).powf(position) } else { min + position * (max - min) };
        response.mark_changed();
    }
    let center = rect.center();
    let radius = size * 9.0 / 22.0;
    let background = style.background.map_or_else(theme::control_background, rgb);
    let border = egui::Stroke::new(1.0, accent.unwrap_or_else(theme::text_hint));
    let painter = ui.painter();
    if style.shape == ControlShape::Square {
        let square = Rect::from_center_size(center, Vec2::splat(radius * 2.0));
        painter.rect_filled(square, 0.0, background);
        painter.rect_stroke(square, 0.0, border, egui::StrokeKind::Inside);
    } else {
        painter.circle_filled(center, radius, background);
        painter.circle_stroke(center, radius, border);
    }
    let angle = (-135.0 + 270.0 * position as f32).to_radians();
    let tip = center + Vec2::new(angle.sin(), -angle.cos()) * (radius - 1.0);
    painter.line_segment([center, tip], egui::Stroke::new(1.5, accent.unwrap_or_else(theme::text_primary)));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::DragValue, true, &parameter.name));
    response
}

/// The Clip / Devices switch in `rect`: two equal parts 1 px apart, the one on light
/// (`docs/design/components.md` › Clip properties). Returns the part clicked, when it was off.
pub(super) fn switch(ui: &Ui, painter: &egui::Painter, rect: Rect, showing: Showing) -> Option<Showing> {
    let part = (rect.width() - 1.0) / 2.0;
    let mut picked = None;
    for (index, (text, value)) in [("Clip", Showing::Clip), ("Devices", Showing::Devices)].into_iter().enumerate() {
        let cell = Rect::from_min_size(
            Pos2::new(rect.left() + index as f32 * (part + 1.0), rect.top()),
            Vec2::new(part, rect.height()),
        );
        let response = widgets::button_area(ui, cell, egui::Id::new(("detail-switch", index)), format!("Show {text}"));
        if response.clicked() && value != showing {
            picked = Some(value);
        }
        let on = motion::toward(ui.ctx(), response.id.with("on"), value == showing, motion::VALUE);
        let idle = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        painter.rect_filled(cell, 0.0, motion::mix(idle, theme::text_value(), on));
        let color = motion::mix(theme::text_secondary(), theme::window_background(), on);
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_SMALL), color);
    }
    picked
}

pub(super) fn parameter_height(parameter: &GraphParameter) -> f32 {
    if let Some([_, height]) = parameter.ui.size {
        return height;
    }
    let extra = if parameter.ui.label_position == LabelPosition::Top { parameter.ui.font_size + 4.0 } else { 0.0 };
    extra
        + if parameter.ui.show_value && parameter.ui.label_position == LabelPosition::Top {
            parameter.ui.font_size + 4.0
        } else {
            0.0
        }
        + match parameter.control {
            ParameterControl::VerticalSlider => 80.0,
            ParameterControl::Knob | ParameterControl::LogKnob => 36.0,
            ParameterControl::Radio => parameter.ui.options.len().max(1) as f32 * (parameter.ui.font_size + 16.0),
            _ => 24.0,
        }
}

/// Where each shown parameter's cell goes: at its own position, or under the one before in a
/// column `width` wide. A cell that would end below `max_height` starts a new column to the
/// right, so a long list widens the panel instead of scrolling (ADR 0021 › Amendment: device
/// panels, 5); `f32::INFINITY` keeps one column.
pub(super) fn parameter_layout(
    parameters: &[GraphParameter],
    width: f32,
    max_height: f32,
    hidden: &[String],
) -> Vec<Rect> {
    let shown = || parameters.iter().filter(|p| !hidden.contains(&p.id));
    let size = |p: &GraphParameter| p.ui.size.unwrap_or([width, parameter_height(p)]);
    // A placed cell pushes down the columns it reaches into, wherever it is in the list.
    let placed: Vec<Rect> = shown()
        .filter_map(|p| p.ui.position.map(|[x, y]| Rect::from_min_size(Pos2::new(x, y), size(p).into())))
        .collect();
    let floor = |column: f32| {
        placed.iter().filter(|r| r.right() > column && r.left() < column + width).fold(0.0f32, |b, r| b.max(r.bottom()))
    };
    let (mut column, mut bottom) = (0.0f32, floor(0.0));
    shown()
        .map(|parameter| {
            let [w, h] = size(parameter);
            let [x, y] = parameter.ui.position.unwrap_or_else(|| {
                // Half a point of slack: heights summed in another order must not open a column.
                if bottom > 0.0 && bottom + h > max_height + 0.5 {
                    column += width;
                    bottom = floor(column);
                }
                let start = [column, bottom];
                bottom += h;
                start
            });
            Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h))
        })
        .collect()
}
