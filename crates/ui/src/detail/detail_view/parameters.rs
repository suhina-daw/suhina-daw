// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A device's parameters drawn in its panel: cells, knobs, switches and their layout.

use super::*;

pub(super) fn format_value(value: f64) -> String {
    if !value.is_finite() {
        return "-".into();
    }
    let text = widgets::fixed(value, 2);
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
    let themed = themed_ui(parameter);
    let style = &themed;
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
    // `.device .value { width }` sets the value's column; it never takes more than half the row.
    let sheet_value = {
        let _value = crate::kit::style_class::enter("value");
        crate::kit::style_class::computed().and_then(|c| c.width)
    };
    let value_width = if style.show_value && value_height == 0.0 {
        sheet_value.map_or((row.width() * 0.24).min(80.0), |w| w.clamp(0.0, row.width() / 2.0))
    } else {
        0.0
    };
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
        format!("{} {}", widgets::fixed(value, parameter.ui.decimals), parameter.unit).trim().to_owned()
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
            widgets::combo_box(
                ui,
                egui::ComboBox::from_id_salt("choice")
                    .selected_text(style.options.get(*value as usize).map_or("", String::as_str))
                    .width(rect.width()),
                |ui| {
                    for (i, label) in style.options.iter().enumerate() {
                        ui.selectable_value(&mut *value, i as f64, label);
                    }
                },
            )
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
        ParameterControl::Number => widgets::number_input(
            ui,
            None,
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
        // `.slider { --suhina-appearance: knob }` draws a slider as a knob.
        ParameterControl::Slider | ParameterControl::LogSlider | ParameterControl::Integer if slider_as_knob() => {
            knob(ui, parameter, rect, accent, value)
        }
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
        control if themed_slider() => themed_slider_widget(ui, parameter, rect, control, value),
        // Edits clamping: the slider rounds a value to its step and decimals only when it is
        // edited, not every time it is drawn, so a value automation plays between steps is not
        // sent back as an edit (which turns the lane off).
        control => {
            let step = if control == ParameterControl::Integer { 1.0 } else { style.step };
            let slider = ui.add(
                egui::Slider::new(value, parameter.min..=parameter.max)
                    .clamping(egui::SliderClamping::Edits)
                    .show_value(false)
                    .trailing_fill(style.accent.is_some())
                    .max_decimals(style.decimals)
                    .logarithmic(control == ParameterControl::LogSlider)
                    .step_by(step),
            );
            // Over the slider, which only draws: a drag holds the pointer, as a knob's does.
            let hit = ui.interact(slider.rect, slider.id.with("drag"), Sense::click_and_drag());
            let log = control == ParameterControl::LogSlider && parameter.min > 0.0;
            let mut slider = slider.union(hit.clone());
            if slider_gesture(ui, &hit, slider.rect, slider.rect.height(), parameter, log, step, value) {
                slider.mark_changed();
            }
            slider
        }
    }
}

/// Whether the theme draws parameter sliders as knobs (`.slider { --suhina-appearance: knob }`).
pub(crate) fn slider_as_knob() -> bool {
    let _style = crate::kit::style_class::enter("slider");
    crate::kit::style_class::knob_preference(false)
}

/// A click or drag on a horizontal slider in `rect`, `handle` wide, as a fader's: a click sets the
/// value there; a drag from the handle moves it as far as the pointer moves, the pointer hidden and
/// held, so past either end it waits and turns back at once, and on release the pointer is on the
/// handle (Shift: ten times finer). Whether the value changed.
#[allow(clippy::too_many_arguments)]
fn slider_gesture(
    ui: &Ui,
    hit: &egui::Response,
    rect: Rect,
    handle: f32,
    parameter: &GraphParameter,
    logarithmic: bool,
    step: f64,
    value: &mut f64,
) -> bool {
    let (min, max) = (parameter.min, parameter.max);
    let travel = (rect.width() - handle).max(1.0);
    let at_pointer = |x: f32| f64::from(((x - rect.left() - handle / 2.0) / travel).clamp(0.0, 1.0));
    let held_id = hit.id.with("position");
    let dragging = hit.dragged_by(egui::PointerButton::Primary);
    let position = if dragging {
        let current = if logarithmic {
            (*value / min).ln() / (max / min).ln()
        } else {
            (*value - min) / (max - min).max(f64::EPSILON)
        };
        let held = ui.ctx().data(|d| d.get_temp::<f64>(held_id)).filter(|_| !hit.drag_started());
        let from = held.unwrap_or_else(|| {
            let press = ui.input(|i| i.pointer.press_origin()).map_or(rect.center().x, |p| p.x);
            let on_handle = (f64::from(press - rect.left() - handle / 2.0) / f64::from(travel) - current).abs()
                * f64::from(travel)
                <= f64::from(handle);
            if on_handle { current.clamp(0.0, 1.0) } else { at_pointer(press) }
        });
        let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let moved = f64::from(widgets::value_drag_delta(ui, hit).x / travel) * fine;
        let position = (from + moved).clamp(0.0, 1.0);
        ui.ctx().data_mut(|d| d.insert_temp(held_id, position));
        let back = rect.left() + handle / 2.0 + travel * position as f32;
        widgets::value_drag_returns_to(ui.ctx(), Pos2::new(back, rect.center().y));
        position
    } else if hit.clicked() {
        let Some(pointer) = hit.interact_pointer_pos() else {
            return false;
        };
        at_pointer(pointer.x)
    } else {
        return false;
    };
    let mut next = if logarithmic { min * (max / min).powf(position) } else { min + position * (max - min) };
    if step > 0.0 {
        next = (min + ((next - min) / step).round() * step).clamp(min, max);
    }
    let changed = next != *value;
    *value = next;
    changed
}

/// Whether the theme gives sliders parts of their own: `.slider-rail`, `.slider-fill` or
/// `.slider-handle` with a background, or `slider.rail` / `slider.fill` / `slider.handle` artwork.
pub(crate) fn themed_slider() -> bool {
    let _style = crate::kit::style_class::enter("slider");
    ["slider-rail", "slider-fill", "slider-handle"].into_iter().any(|class| {
        let _part = crate::kit::style_class::enter(class);
        crate::kit::style_class::computed().is_some_and(|c| c.authored.background || c.has_gradient())
    }) || ["slider.rail", "slider.fill", "slider.handle"].into_iter().any(crate::kit::skin::has_own)
}

/// A parameter slider drawn from the theme's parts: the rail (`.slider-rail`, `height` its
/// thickness), the fill up to the value (`.slider-fill`) and the handle (`.slider-handle`,
/// `width`), each also as artwork. Dragged sideways, back to its default on a double click.
fn themed_slider_widget(
    ui: &mut Ui,
    parameter: &GraphParameter,
    rect: Rect,
    control: ParameterControl,
    value: &mut f64,
) -> egui::Response {
    use crate::kit::skin::{self, State};
    let (min, max) = (parameter.min, parameter.max);
    let logarithmic = control == ParameterControl::LogSlider && min > 0.0 && max > min;
    let height = ui.spacing().interact_size.y.min(rect.height()).max(8.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(rect.width(), height), Sense::click_and_drag());
    let _style = crate::kit::style_class::enter_response("slider", &response);
    let to_position = |v: f64| {
        if logarithmic { (v / min).ln() / (max / min).ln() } else { (v - min) / (max - min).max(f64::EPSILON) }
    };
    let handle_width = {
        let _handle = crate::kit::style_class::enter("slider-handle");
        crate::kit::style_class::computed().and_then(|c| c.width).unwrap_or(8.0).clamp(2.0, rect.width().max(2.0))
    };
    let travel = (rect.width() - handle_width).max(1.0);
    let step = if control == ParameterControl::Integer { 1.0 } else { parameter.ui.step };
    slider_gesture(ui, &response, rect, handle_width, parameter, logarithmic, step, value);
    let position = to_position(*value).clamp(0.0, 1.0) as f32;
    let state = State::response(&response, false);
    let painter = ui.painter_at(rect);
    let rail = {
        let _rail = crate::kit::style_class::enter("slider-rail");
        let thickness = crate::kit::native_css::thickness(4.0).clamp(1.0, rect.height());
        let rail = Rect::from_center_size(rect.center(), Vec2::new(rect.width(), thickness));
        if !(skin::has_own("slider.rail") && skin::paint(&painter, rail, "slider.rail", state, theme::text_hint(), 0.0))
        {
            crate::kit::native_css::surface(&painter, rail, thickness / 2.0, theme::control_background());
        }
        rail
    };
    let at = rect.left() + handle_width / 2.0 + travel * position;
    {
        let _fill = crate::kit::style_class::enter("slider-fill");
        let fill = Rect::from_min_max(rail.min, Pos2::new(at, rail.bottom()));
        if fill.width() >= 1.0
            && !(skin::has_own("slider.fill")
                && skin::paint(&painter, fill, "slider.fill", state, theme::accent(), 0.0))
        {
            crate::kit::native_css::surface(&painter, fill, rail.height() / 2.0, theme::control_selected());
        }
    }
    {
        let _handle = crate::kit::style_class::enter_response("slider-handle", &response);
        let handle = Rect::from_center_size(Pos2::new(at, rect.center().y), Vec2::new(handle_width, rect.height()));
        if !(skin::has_own("slider.handle")
            && skin::paint(&painter, handle, "slider.handle", state, theme::text_primary(), 0.0))
        {
            crate::kit::native_css::surface(&painter, handle, 2.0, theme::text_primary());
        }
    }
    response
}

/// A knob in `rect`, logarithmic for a `LogKnob`: dragged up and down (finer with Shift, with the pointer hidden),
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
    // `.device .knob { width }` (or height) sets the knob's size, within its cell; with
    // `--suhina-param-row` taller rows give it room. Its `accent-color` colours its ring and
    // pointer, unless the device gives an accent of its own.
    let (themed, sheet_accent) = {
        let _knob = crate::kit::style_class::enter("knob");
        let computed = crate::kit::style_class::computed();
        (computed.as_ref().and_then(|c| c.width.or(c.height)), computed.filter(|c| c.authored.accent).map(|c| c.accent))
    };
    let accent = accent.or(sheet_accent);
    let room = rect.width().min(rect.height()).max(8.0);
    let size = themed.map_or(room, |s| s.clamp(8.0, room));
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
        let delta = f64::from(widgets::value_drag_delta(ui, &response).y) / widgets::PARAMETER_DRAG_PIXELS * fine;
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
        painter.fill_background(square, 0.0, background);
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
        // `.editor-tab` (`:checked` for the one shown), `#tab-clip` and `#tab-devices`.
        let _style = crate::kit::style_class::enter_named_response(
            "editor-tab",
            &format!("tab-{}", text.to_ascii_lowercase()),
            &response,
        );
        crate::kit::native_css::mark(crate::kit::native_css::HostState::Checked, value == showing);
        let on = motion::toward(ui.ctx(), response.id.with("on"), value == showing, motion::VALUE);
        let idle = motion::mix(theme::control_background(), theme::control_hover(), widgets::hover(ui, &response));
        crate::kit::native_css::surface(painter, cell, 0.0, motion::mix(idle, theme::control_selected(), on));
        let color =
            crate::kit::style_class::foreground(motion::mix(theme::text_secondary(), theme::text_inverse(), on));
        widgets::text_in(painter, cell, Align2::CENTER_CENTER, text, theme::ui_font(theme::TEXT_SMALL), color);
    }
    picked
}

pub(super) fn parameter_height(parameter: &GraphParameter) -> f32 {
    let themed = themed_ui(parameter);
    let parameter = &GraphParameter { ui: themed, ..parameter.clone() };
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
/// panels, 5); `f32::INFINITY` keeps one column. The theme may divide a column into cells side by
/// side, each as tall as it says (`--suhina-param-columns`, `--suhina-param-row`).
pub(super) fn parameter_layout(
    parameters: &[GraphParameter],
    width: f32,
    max_height: f32,
    hidden: &[String],
) -> Vec<Rect> {
    let (slots, row) = crate::kit::style_class::theme_param_grid();
    let cell = width / slots as f32;
    let shown = || parameters.iter().filter(|p| !hidden.contains(&p.id) && !p.is_readout());
    let placed_size = |p: &GraphParameter| p.ui.size.unwrap_or([width, parameter_height(p)]);
    // A placed cell pushes down the columns it reaches into, wherever it is in the list.
    let placed: Vec<Rect> = shown()
        .filter_map(|p| p.ui.position.map(|[x, y]| Rect::from_min_size(Pos2::new(x, y), placed_size(p).into())))
        .collect();
    let floor = |column: f32| {
        placed.iter().filter(|r| r.right() > column && r.left() < column + width).fold(0.0f32, |b, r| b.max(r.bottom()))
    };
    let (mut column, mut right, mut bottom) = (0.0f32, 0.0f32, floor(0.0));
    let (mut slot, mut line_top) = (0usize, 0.0f32);
    shown()
        .map(|parameter| {
            if let Some([x, y]) = parameter.ui.position {
                return Rect::from_min_size(Pos2::new(x, y), placed_size(parameter).into());
            }
            let height = row.map_or_else(|| parameter_height(parameter), |row| row.max(parameter_height(parameter)));
            let [w, h] = parameter.ui.size.unwrap_or([cell, height]);
            if slot == 0 {
                line_top = bottom;
                // Half a point of slack: heights summed in another order must not open a column.
                // Past every placed cell the floor is zero, so this ends. A cell wider than its
                // column keeps the next one clear of it.
                while line_top > 0.0 && line_top + h > max_height + 0.5 {
                    column = right.max(column + width);
                    right = column;
                    bottom = floor(column);
                    line_top = bottom;
                }
            }
            let rect = Rect::from_min_size(Pos2::new(column + cell * slot as f32, line_top), Vec2::new(w, h));
            bottom = bottom.max(rect.bottom());
            right = right.max(rect.right());
            slot = (slot + 1) % slots;
            rect
        })
        .collect()
}

/// Which column of `width` a cell starting at `left` is in.
fn column_of(left: f32, width: f32) -> f32 {
    (left / width.max(1.0) + 0.001).floor()
}

/// Whether `graph`'s list gets room and a line between its columns: a generated list, not one with a
/// canvas or cells it places itself, which keep their own positions.
pub(super) fn list_spreads(graph: &GraphBox) -> bool {
    graph.canvas.is_none() && graph.parameters.iter().all(|p| p.ui.position.is_none())
}

/// Moves each cell by the room before its column, a theme's cells inside a column staying together.
/// Returns where the lines between the columns go.
pub(super) fn spread_columns(cells: &mut [Rect], width: f32) -> Vec<f32> {
    let mut columns = 0.0f32;
    for cell in cells.iter_mut() {
        let column = column_of(cell.left(), width);
        columns = columns.max(column);
        *cell = cell.translate(Vec2::new(column * super::LIST_GAP, 0.0));
    }
    (1..=columns as u32).map(|k| k as f32 * width + (k as f32 - 0.5) * super::LIST_GAP).collect()
}

/// The stock controls of `graph` from the top left of the part under its nodes. With a canvas, the
/// list flows to the canvas's right and a cell the file places itself stays under the canvas, where
/// its position has always been measured from.
pub(super) fn list_cells(graph: &GraphBox, width: f32, max_height: f32) -> Vec<Rect> {
    let Some(canvas) = &graph.canvas else {
        return parameter_layout(&graph.parameters, width, max_height, &[]);
    };
    let shown = || graph.parameters.iter().filter(|p| !canvas.hidden_parameters.contains(&p.id) && !p.is_readout());
    let mut skipped = canvas.hidden_parameters.clone();
    skipped.extend(shown().filter(|p| p.ui.position.is_some()).map(|p| p.id.clone()));
    let mut flow = parameter_layout(&graph.parameters, width, max_height, &skipped).into_iter();
    shown()
        .filter_map(|p| match p.ui.position {
            Some([x, y]) => {
                let size = p.ui.size.unwrap_or([width, parameter_height(p)]);
                Some(Rect::from_min_size(Pos2::new(x, y + canvas.size[1]), size.into()))
            }
            None => flow.next().map(|cell| cell.translate(Vec2::new(canvas.size[0], 0.0))),
        })
        .collect()
}

/// A parameter's look with the theme's label position (`--suhina-param-label`) over its own.
fn themed_ui(parameter: &GraphParameter) -> suhina_device_sdk::ParameterUi {
    let mut ui = parameter.ui.clone();
    ui.label_position = match crate::kit::style_class::theme_param_label() {
        Some("top") => LabelPosition::Top,
        Some("hidden") => LabelPosition::Hidden,
        Some(_) => LabelPosition::Left,
        None => ui.label_position,
    };
    ui
}
