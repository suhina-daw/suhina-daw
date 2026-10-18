// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! SDK drawing programs are evaluated only on the UI thread.
use crate::detail::detail_view::GraphParameter;
use crate::kit::widgets::Action;
use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use serde_json::json;
use std::{cell::RefCell, sync::Arc};
use suhina_device_sdk::{DeviceGraph, ui::*};
use suhina_engine::model::TrackId;

pub(crate) struct Parsed {
    pub graph: DeviceGraph,
    pub canvas: Option<Arc<DeviceUi>>,
}
thread_local! {static GRAPHS:RefCell<Vec<(Vec<u8>,Arc<Parsed>)>>=const{RefCell::new(Vec::new())};}
pub(crate) fn graph(bytes: &[u8]) -> Option<Arc<Parsed>> {
    if let Some(found) =
        GRAPHS.with(|cache| cache.borrow().iter().find(|(key, _)| key == bytes).map(|(_, v)| v.clone()))
    {
        return Some(found);
    }
    let mut graph = DeviceGraph::from_sd(std::str::from_utf8(bytes).ok()?).ok()?;
    let canvas = graph.ui.take().map(Arc::new);
    let parsed = Arc::new(Parsed { graph, canvas });
    GRAPHS.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.push((bytes.to_vec(), parsed.clone()));
        while cache.len() > 64 || cache.iter().map(|(b, _)| b.len()).sum::<usize>() > 16 * 1024 * 1024 {
            cache.remove(0);
        }
    });
    Some(parsed)
}
#[derive(Clone)]
struct Loaded {
    source: Arc<DeviceUi>,
    palette: WidgetPalette,
    images: Vec<Result<egui::TextureHandle, String>>,
    controls: Vec<Vec<Draw>>,
}
fn rgba(color: egui::Color32) -> [u8; 4] {
    [color.r(), color.g(), color.b(), color.a()]
}
fn accent_text(accent: egui::Color32) -> [u8; 4] {
    let luminance = 0.2126 * f32::from(accent.r()) + 0.7152 * f32::from(accent.g()) + 0.0722 * f32::from(accent.b());
    if luminance > 145.0 { [29, 28, 26, 255] } else { [255, 255, 255, 255] }
}
fn images(ctx: &egui::Context, source: &Arc<DeviceUi>) -> Arc<Loaded> {
    let colors = crate::kit::theme::palette();
    let palette = WidgetPalette {
        text: rgba(colors.text_primary),
        panel: rgba(colors.panel_background),
        control: rgba(colors.control_background),
        border: rgba(colors.border),
        accent: rgba(colors.accent),
        accent_text: accent_text(colors.accent),
    };
    let key = egui::Id::new("sd-image-cache");
    let mut cache = ctx.data_mut(|d| d.get_temp::<Vec<Arc<Loaded>>>(key)).unwrap_or_default();
    if let Some(found) = cache.iter().find(|v| Arc::ptr_eq(&v.source, source) && v.palette == palette) {
        return found.clone();
    }
    let images = source
        .assets
        .iter()
        .map(|a| {
            a.decode_rgba()
                .map(|(pixels, size)| {
                    ctx.load_texture(
                        &a.id,
                        egui::ColorImage::from_rgba_unmultiplied(size, &pixels),
                        egui::TextureOptions {
                            wrap_mode: egui::TextureWrapMode::Repeat,
                            ..egui::TextureOptions::LINEAR
                        },
                    )
                })
                .map_err(|e| e.to_string())
        })
        .collect();
    let controls = source.controls.iter().map(|c| c.drawings_with_palette(&palette).into_owned()).collect();
    let loaded = Arc::new(Loaded { source: source.clone(), palette, images, controls });
    cache.push(loaded.clone());
    if cache.len() > 8 {
        cache.remove(0);
    }
    ctx.data_mut(|d| d.insert_temp(key, cache));
    loaded
}
fn color(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}
fn normalized(p: &GraphParameter, log: bool) -> f64 {
    if log && p.min > 0.0 {
        ((p.value.max(p.min).ln() - p.min.ln()) / (p.max.ln() - p.min.ln())).clamp(0.0, 1.0)
    } else if p.max > p.min {
        ((p.value - p.min) / (p.max - p.min)).clamp(0.0, 1.0)
    } else {
        0.0
    }
}
fn paint(painter: &egui::Painter, drawings: &[Draw], origin: Pos2, parameters: &[GraphParameter], loaded: &Loaded) {
    let lookup = |id: &str, norm: bool, log: bool| {
        parameters.iter().find(|p| p.id == id).map_or(0.0, |p| if norm { normalized(p, log) } else { p.value })
    };
    let value = |v: &UiValue| v.evaluate(&lookup);
    let point = |p: &[UiValue; 2]| origin + Vec2::new(value(&p[0]), value(&p[1]));
    let rectangle = |r: &[UiValue; 4]| {
        Rect::from_min_size(
            origin + Vec2::new(value(&r[0]), value(&r[1])),
            Vec2::new(value(&r[2]).clamp(0.0, 8192.0), value(&r[3]).clamp(0.0, 8192.0)),
        )
    };
    for draw in drawings {
        match draw {
            Draw::Rect { rect, color: c, radius } => {
                painter.rect_filled(rectangle(rect), *radius, color(*c));
            }
            Draw::Circle { center, radius, color: c } => {
                painter.circle_filled(point(center), value(radius).clamp(0.0, 8192.0), color(*c));
            }
            Draw::Line { from, to, color: c, width } => {
                painter.line_segment([point(from), point(to)], Stroke::new(*width, color(*c)));
            }
            Draw::Path { points, color: c, width, closed, fill } => {
                let points = points.iter().map(point).collect::<Vec<_>>();
                if let Some(fill) = fill {
                    painter.add(egui::Shape::convex_polygon(points, color(*fill), Stroke::new(*width, color(*c))));
                } else if *closed {
                    painter.add(egui::Shape::closed_line(points, Stroke::new(*width, color(*c))));
                } else {
                    painter.add(egui::Shape::line(points, Stroke::new(*width, color(*c))));
                }
            }
            Draw::Arc { center, radius, start, end, color: c, width } => {
                let center = point(center);
                let radius = value(radius).clamp(0.0, 8192.0);
                let start = value(start);
                let end = value(end);
                let points = (0..=64)
                    .map(|i| {
                        let a = start + (end - start) * i as f32 / 64.0;
                        center + Vec2::new(a.cos(), a.sin()) * radius
                    })
                    .collect();
                painter.add(egui::Shape::line(points, Stroke::new(*width, color(*c))));
            }
            Draw::Text { position, text, color: c, size, parameter, decimals, align, format } => {
                let label = parameter.as_ref().and_then(|id| parameters.iter().find(|p| p.id == *id)).map_or_else(
                    || text.clone(),
                    |p| match format {
                        TextFormat::Choice => {
                            p.ui.options.get(p.value.round().max(0.0) as usize).cloned().unwrap_or_default()
                        }
                        TextFormat::Frequency if p.value >= 1000.0 => format!("{text}{:.2} kHz", p.value / 1000.0),
                        _ => format!(
                            "{text}{:.*}{}{}",
                            *decimals,
                            p.value,
                            if p.unit.is_empty() || p.unit == "%" { "" } else { " " },
                            p.unit
                        ),
                    },
                );
                let position = point(position);
                let mut font = egui::FontId::proportional(*size);
                let mut galley = painter.layout_no_wrap(label.clone(), font.clone(), color(*c));
                if parameter.is_some() {
                    let clip = painter.clip_rect();
                    let available_width = if *align == TextAlign::Center {
                        2. * (position.x - clip.left()).min(clip.right() - position.x)
                    } else {
                        clip.right() - position.x
                    };
                    let available_height = clip.bottom() - position.y;
                    let scale = (available_width / galley.size().x.max(1.))
                        .min(available_height / galley.size().y.max(1.))
                        .clamp(0., 1.);
                    if scale < 1. {
                        font.size = (font.size * scale).max(8.);
                        galley = painter.layout_no_wrap(label, font, color(*c));
                    }
                }
                let offset = if *align == TextAlign::Center { Vec2::new(galley.size().x / 2., 0.) } else { Vec2::ZERO };
                painter.galley(position - offset, galley, color(*c));
            }
            Draw::Gradient { rect, colors } => {
                let r = rectangle(rect);
                let mut mesh = egui::Mesh::default();
                for (pos, c) in [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()].into_iter().zip(colors)
                {
                    mesh.colored_vertex(pos, color(*c));
                }
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                painter.add(egui::Shape::mesh(mesh));
            }
            Draw::Image { asset, rect, uv, tint, tile_size, fit } => {
                if let Some(i) = loaded.source.assets.iter().position(|a| a.id == *asset) {
                    match loaded.images.get(i) {
                        Some(Ok(texture)) => {
                            let mut r = rectangle(rect);
                            let mut uv = Rect::from_min_max(Pos2::new(uv[0], uv[1]), Pos2::new(uv[2], uv[3]));
                            let size = texture.size_vec2() * uv.size();
                            if let Some([w, h]) = tile_size {
                                uv.max = uv.min + Vec2::new(r.width() / w, r.height() / h);
                            } else if size.x > 0.0 && size.y > 0.0 && r.width() > 0.0 && r.height() > 0.0 {
                                match fit {
                                    ImageFit::Contain => {
                                        let scale = (r.width() / size.x).min(r.height() / size.y);
                                        r = Rect::from_center_size(r.center(), size * scale);
                                    }
                                    ImageFit::Cover => {
                                        let scale = (r.width() / size.x).max(r.height() / size.y);
                                        uv = Rect::from_center_size(uv.center(), uv.size() * r.size() / (size * scale));
                                    }
                                    ImageFit::Stretch => {}
                                }
                            }
                            painter.image(texture.id(), r, uv, color(*tint));
                        }
                        Some(Err(error)) => {
                            painter.text(
                                rectangle(rect).min,
                                egui::Align2::LEFT_TOP,
                                error,
                                egui::FontId::proportional(12.0),
                                Color32::LIGHT_RED,
                            );
                        }
                        None => {}
                    }
                }
            }
        }
    }
}
pub(crate) fn show(
    ui: &mut egui::Ui,
    rect: Rect,
    canvas: &Arc<DeviceUi>,
    parameters: &[GraphParameter],
    track: TrackId,
    device: &crate::detail::detail_view::DeviceAt,
    actions: &mut Vec<Action>,
) {
    let loaded = images(ui.ctx(), canvas);
    let painter = ui.painter_at(rect.intersect(ui.clip_rect()));
    let background = if canvas.background == DeviceUi::default().background {
        crate::kit::theme::palette().panel_background
    } else {
        color(canvas.background)
    };
    painter.rect_filled(rect, 0.0, background);
    paint(&painter, &canvas.layers, rect.min, parameters, &loaded);
    for (control, drawings) in canvas.controls.iter().zip(&loaded.controls) {
        if control.visible_when.as_ref().is_some_and(|v| {
            v.evaluate(&|id, norm, log| {
                parameters.iter().find(|p| p.id == id).map_or(0.0, |p| if norm { normalized(p, log) } else { p.value })
            }) <= 0.0
        }) {
            continue;
        }
        let [x, y, w, h] = control.bounds;
        let bounds = Rect::from_min_size(rect.min + Vec2::new(x, y), Vec2::new(w, h));
        paint(
            &painter.with_clip_rect(bounds.intersect(painter.clip_rect())),
            drawings,
            bounds.min,
            parameters,
            &loaded,
        );
        if matches!(control.widget, Some(Widget::Label { .. } | Widget::Separator)) {
            continue;
        }
        let response = ui
            .interact(
                bounds,
                egui::Id::new(("sd-custom", track.0, device, &control.id)),
                if control.gesture == UiGesture::Menu { egui::Sense::click() } else { egui::Sense::click_and_drag() },
            )
            .on_hover_text(&control.tooltip);
        let response = if control.gesture == UiGesture::Drag {
            response.on_hover_cursor(egui::CursorIcon::ResizeVertical)
        } else {
            response
        };
        if response.is_pointer_button_down_on() || response.clicked() || response.drag_started() {
            response.request_focus();
        }
        let focused = response.has_focus();
        let (dx, dy, button, fine) = ui.input(|i| {
            (
                i32::from(focused && i.key_pressed(egui::Key::ArrowRight))
                    - i32::from(focused && i.key_pressed(egui::Key::ArrowLeft)),
                i32::from(focused && i.key_pressed(egui::Key::ArrowUp))
                    - i32::from(focused && i.key_pressed(egui::Key::ArrowDown)),
                focused && (i.key_down(egui::Key::Space) || i.key_down(egui::Key::Enter)),
                if i.modifiers.shift { 0.1 } else { 1.0 },
            )
        });
        let held_id = response.id.with("held");
        let held = response.is_pointer_button_down_on() || button;
        let was_held = ui.ctx().data_mut(|d| {
            let previous = d.get_temp::<bool>(held_id).unwrap_or(false);
            d.insert_temp(held_id, held);
            previous
        });
        let mut control_actions = Vec::new();
        let mut descriptions = Vec::new();
        for (axis, binding) in [control.x.as_ref(), control.y.as_ref()].into_iter().enumerate() {
            let Some(binding) = binding else { continue };
            let Some(p) = parameters.iter().find(|p| p.id == binding.parameter) else { continue };
            descriptions.push(format!("{}: {} {}", p.name, p.value, p.unit));
            if control.gesture == UiGesture::Menu {
                let key_open =
                    focused && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space));
                let mut popup = egui::Popup::menu(&response);
                if key_open {
                    popup = popup.open_memory(Some(egui::SetOpenCommand::Toggle));
                }
                let frame = egui::Frame::popup(ui.style())
                    .corner_radius(0)
                    .fill(color(loaded.palette.panel))
                    .stroke(Stroke::new(1., color(loaded.palette.accent)));
                let mut picked = None;
                popup.frame(frame).show(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.;
                    ui.spacing_mut().scroll =
                        egui::style::ScrollStyle { bar_width: 6., ..egui::style::ScrollStyle::solid() };
                    ui.spacing_mut().interact_size.y = 14.;
                    ui.visuals_mut().selection.bg_fill = color(loaded.palette.accent);
                    let choices: Vec<(f64, &str)> = if let Some(Widget::ValueMenu { choices }) = &control.widget {
                        choices.iter().map(|(v, label)| (f64::from(*v), label.as_str())).collect()
                    } else {
                        p.ui.options.iter().enumerate().map(|(i, label)| (i as f64, label.as_str())).collect()
                    };
                    let text_width = choices
                        .iter()
                        .map(|(_, label)| {
                            ui.fonts_mut(|fonts| {
                                fonts
                                    .layout_no_wrap((*label).into(), egui::FontId::proportional(11.), Color32::WHITE)
                                    .size()
                                    .x
                            })
                        })
                        .fold(0., f32::max);
                    let scroll = &ui.spacing().scroll;
                    let width = text_width
                        + ui.spacing().button_padding.x * 2.
                        + scroll.bar_width
                        + scroll.bar_inner_margin
                        + scroll.bar_outer_margin;
                    ui.set_min_width(width.ceil());
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    egui::ScrollArea::vertical().max_height(240.).show(ui, |ui| {
                        for (value, label) in choices {
                            let selected = p.value.round() == value;
                            let text = egui::RichText::new(label).size(11.).color(color(if selected {
                                loaded.palette.accent_text
                            } else {
                                loaded.palette.text
                            }));
                            if ui.selectable_label(selected, text).clicked() {
                                picked = Some(value);
                                ui.close();
                            }
                        }
                    });
                });
                if let Some(value) = picked.filter(|v| *v != p.value) {
                    control_actions.push(Action::new("device.setParam",json!({"track":track.0,"device":device.value(),"param":p.id,"value":value,"continuous":false,"gesture":control.id})));
                }
                continue;
            }
            let old = normalized(p, binding.logarithmic);
            let mut next = old;
            match control.gesture {
                UiGesture::Position if response.dragged() || response.clicked() => {
                    if let Some(pos) = response.interact_pointer_pos() {
                        next = if axis == 0 {
                            (pos.x - bounds.left()) as f64 / w as f64
                        } else {
                            (bounds.bottom() - pos.y) as f64 / h as f64
                        };
                    }
                }
                UiGesture::Drag if response.dragged() => {
                    let delta = response.drag_delta();
                    next += if axis == 0 { delta.x as f64 / w as f64 } else { -delta.y as f64 / h as f64 } * fine;
                }
                UiGesture::Toggle if response.clicked() => next = if old >= 0.5 { 0.0 } else { 1.0 },
                UiGesture::Set(value) if response.clicked() || button => {
                    let value = f64::from(value);
                    next = if binding.logarithmic {
                        (value.ln() - p.min.ln()) / (p.max.ln() - p.min.ln())
                    } else {
                        (value - p.min) / (p.max - p.min)
                    };
                }
                UiGesture::Momentary => next = f64::from(response.is_pointer_button_down_on() || button),
                _ => {}
            }
            if !matches!(
                control.gesture,
                UiGesture::Toggle | UiGesture::Momentary | UiGesture::Set(_) | UiGesture::Menu
            ) {
                next += (if axis == 0 { dx } else { dy }) as f64 * 0.01 * fine;
            }
            let mut value = if binding.logarithmic {
                (p.min.ln() + next.clamp(0.0, 1.0) * (p.max.ln() - p.min.ln())).exp()
            } else {
                p.min + next.clamp(0.0, 1.0) * (p.max - p.min)
            };
            let discrete = matches!(
                p.control,
                suhina_device_sdk::ParameterControl::Integer
                    | suhina_device_sdk::ParameterControl::Toggle
                    | suhina_device_sdk::ParameterControl::Button
                    | suhina_device_sdk::ParameterControl::Choice
                    | suhina_device_sdk::ParameterControl::Radio
            );
            let step_click = if matches!(control.widget, Some(Widget::Stepper { .. })) && response.clicked() {
                response
                    .interact_pointer_pos()
                    .filter(|pos| pos.x >= bounds.right() - 12.)
                    .map_or(0, |pos| if pos.y < bounds.center().y { 1 } else { -1 })
            } else {
                0
            };
            let key_delta = (if axis == 0 { dx } else { dy }) + step_click;
            if key_delta != 0
                && !matches!(
                    control.gesture,
                    UiGesture::Toggle | UiGesture::Momentary | UiGesture::Set(_) | UiGesture::Menu
                )
                && (discrete || p.ui.step > 0.0 || step_click != 0)
            {
                let step = if discrete {
                    p.ui.step.max(1.0)
                } else if let Some(Widget::Stepper { decimals }) = control.widget {
                    10_f64.powi(-(decimals.min(8) as i32)) * fine
                } else {
                    p.ui.step * fine
                };
                value = (p.value + f64::from(key_delta) * step).clamp(p.min, p.max);
            }
            if response.double_clicked()
                && !matches!(
                    control.gesture,
                    UiGesture::Toggle | UiGesture::Momentary | UiGesture::Set(_) | UiGesture::Menu
                )
            {
                value = p.default;
            }
            // Reuse the SDK's discrete/range rules through the shared command.
            let changed = next != old || response.double_clicked() || step_click != 0;
            let interacting = response.clicked()
                || response.dragged()
                || dx != 0
                || dy != 0
                || (control.gesture == UiGesture::Momentary && (held || was_held));
            if (interacting && changed) || response.drag_stopped() || (was_held && !held) {
                control_actions.push(Action::new(
                    "device.setParam",
                    json!({
                        "track": track.0, "device": device.value(), "param": p.id, "value": value,
                        "continuous": response.dragged() || response.is_pointer_button_down_on() || button,
                        "gesture": control.id
                    }),
                ));
            }
        }
        // Both axes are one undo gesture, including a single click or a keyboard step.
        let count = control_actions.len();
        for (index, mut action) in control_actions.into_iter().enumerate() {
            if index + 1 < count {
                action.params["continuous"] = json!(true);
            }
            actions.push(action);
        }
        response.widget_info(|| {
            let kind = if matches!(
                control.gesture,
                UiGesture::Toggle | UiGesture::Momentary | UiGesture::Set(_) | UiGesture::Menu
            ) {
                egui::WidgetType::Button
            } else {
                egui::WidgetType::DragValue
            };
            let mut info = egui::WidgetInfo::labeled(kind, true, &control.name);
            info.current_text_value = Some(descriptions.join(", "));
            info
        });
        if focused || response.is_pointer_button_down_on() || response.dragged() {
            let r = bounds.shrink(0.5);
            let leg = 3.0_f32.min(r.width() * 0.25).min(r.height() * 0.25);
            let stroke = Stroke::new(1., Color32::WHITE);
            for (corner, dx, dy) in [
                (r.left_top(), 1., 1.),
                (r.right_top(), -1., 1.),
                (r.left_bottom(), 1., -1.),
                (r.right_bottom(), -1., -1.),
            ] {
                painter.line_segment([corner + Vec2::new(dx * leg, 0.), corner], stroke);
                painter.line_segment([corner, corner + Vec2::new(0., dy * leg)], stroke);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parameter(id: &str, logarithmic: bool) -> GraphParameter {
        GraphParameter {
            default: 0.5,
            ui: Default::default(),
            control: Default::default(),
            unit: String::new(),
            id: id.into(),
            name: id.into(),
            min: if logarithmic { 20.0 } else { 0.0 },
            max: if logarithmic { 20000.0 } else { 1.0 },
            value: if logarithmic { 1000.0 } else { 0.5 },
        }
    }
    #[test]
    fn delay_panel_renders_and_selector_dispatches_a_literal() {
        let graph = super::delay_example::build().unwrap();
        let parameters: Vec<_> = graph
            .parameters
            .iter()
            .map(|p| GraphParameter {
                id: p.id.clone(),
                name: p.name.clone(),
                min: p.min,
                max: p.max,
                default: p.default,
                value: p.default,
                unit: p.unit.clone(),
                control: p.control,
                ui: p.ui.clone(),
            })
            .collect();
        let canvas = Arc::new(graph.ui.unwrap());
        let preview_canvas = canvas.clone();
        let preview_parameters = parameters.clone();
        let mut harness = egui_kittest::Harness::builder().with_size(Vec2::new(956., 180.)).build_ui(move |ui| {
            crate::kit::theme::install(ui.ctx());
            show(
                ui,
                Rect::from_min_size(Pos2::ZERO, Vec2::new(944., 168.)),
                &preview_canvas,
                &preview_parameters,
                TrackId(1),
                &crate::detail::detail_view::DeviceAt(vec![0]),
                &mut Vec::new(),
            );
        });
        harness.run();
        for control in &canvas.controls {
            for drawing in control.drawings().iter() {
                if let Draw::Text { text, size, parameter: None, position, align, .. } = drawing {
                    let galley = harness.ctx.fonts_mut(|fonts| {
                        fonts.layout_no_wrap(text.clone(), egui::FontId::proportional(*size), Color32::WHITE)
                    });
                    let x = position[0].evaluate(&|_, _, _| 0.);
                    if x < 0. || x > control.bounds[2] {
                        continue;
                    }
                    let width = if *align == TextAlign::Center {
                        2. * x.min(control.bounds[2] - x)
                    } else {
                        control.bounds[2] - x
                    };
                    assert!(
                        galley.size().x <= width + 0.5,
                        "{} caption clipped: {} > {}",
                        control.id,
                        galley.size().x,
                        width
                    );
                }
            }
        }
        snapshot(&mut harness, "delay-panel.png");
        let nord = crate::kit::theme::Theme::from_json(include_str!("../../themes/nord.json")).0;
        crate::kit::theme::set_palette(&harness.ctx, nord.colors);
        harness.run();
        snapshot(&mut harness, "delay-panel-nord.png");
        crate::kit::theme::set_palette(&harness.ctx, crate::kit::theme::Palette::DARK);
        use egui_kittest::kittest::Queryable;
        let mut narrow_parameters = parameters.clone();
        for p in &mut narrow_parameters {
            p.value = match p.id.as_str() {
                "cutoff" => 4010.0,
                "width" => 2.4,
                "offset_left" => -33.3,
                "offset_right" => 33.3,
                _ => p.value,
            };
        }
        let narrow_canvas = canvas.clone();
        let mut narrow = egui_kittest::Harness::builder().with_size(Vec2::new(956., 180.)).build_ui(move |ui| {
            crate::kit::theme::install(ui.ctx());
            show(
                ui,
                Rect::from_min_size(Pos2::ZERO, Vec2::new(944., 168.)),
                &narrow_canvas,
                &narrow_parameters,
                TrackId(1),
                &crate::detail::detail_view::DeviceAt(vec![0]),
                &mut Vec::new(),
            );
        });
        narrow.run();
        narrow.get_by_label("Feedback").click();
        narrow.run();
        snapshot(&mut narrow, "delay-focus.png");
        let mut time_parameters = parameters.clone();
        for p in &mut time_parameters {
            if p.id.starts_with("sync_") {
                p.value = 0.0;
            }
            if p.id.starts_with("time_") {
                p.value = 427.0;
            }
        }
        let time_canvas = canvas.clone();
        let mut time_harness = egui_kittest::Harness::builder().with_size(Vec2::new(956., 180.)).build_ui(move |ui| {
            crate::kit::theme::install(ui.ctx());
            show(
                ui,
                Rect::from_min_size(Pos2::ZERO, Vec2::new(944., 168.)),
                &time_canvas,
                &time_parameters,
                TrackId(1),
                &crate::detail::detail_view::DeviceAt(vec![0]),
                &mut Vec::new(),
            );
        });
        time_harness.run();
        snapshot(&mut time_harness, "delay-time-panel.png");
        let start = Pos2::new(64., 110.);
        let clicked = click(&canvas, &parameters, start);
        assert!(
            clicked.iter().all(|a| a.params["param"] == "offset_left" && a.params["value"] == 0.0),
            "clicking a drag field must preserve its value"
        );
        let offset = pointer_gesture(&canvas, &parameters, start, Some(start - Vec2::new(0., 12.)));
        assert!(
            offset.iter().any(
                |a| a.params["param"] == "offset_left" && (a.params["value"].as_f64().unwrap() - 33.3).abs() < 1e-6
            )
        );
        assert!(offset.iter().any(|a| a.params["param"] == "offset_left" && a.params["continuous"] == false));
        let actions = click(&canvas, &parameters, Pos2::new(100., 151.));
        assert!(actions.iter().any(|a| a.params["param"] == "mode" && a.params["value"] == 1.0));
        let mut time_parameters = parameters.clone();
        for p in &mut time_parameters {
            if p.id.starts_with("sync_") {
                p.value = 0.0;
            }
        }
        assert!(
            click(&canvas, &time_parameters, Pos2::new(89., 59.))
                .iter()
                .all(|a| !a.params["param"].as_str().unwrap_or_default().starts_with("division_")),
            "hidden tempo field must not intercept clicks"
        );
        let actions = click(&canvas, &time_parameters, Pos2::new(35., 59.));
        assert!(actions.iter().any(|a| a.params["param"] == "sync_left" && a.params["value"] == 1.0));
    }
    #[test]
    fn delay_menus_open_choose_and_cancel_without_spurious_edits() {
        use egui_kittest::kittest::Queryable;
        let graph = super::delay_example::build().unwrap();
        let parameters: Vec<_> = graph
            .parameters
            .iter()
            .map(|p| GraphParameter {
                id: p.id.clone(),
                name: p.name.clone(),
                min: p.min,
                max: p.max,
                default: p.default,
                value: p.default,
                unit: p.unit.clone(),
                control: p.control,
                ui: p.ui.clone(),
            })
            .collect();
        let canvas = Arc::new(graph.ui.unwrap());
        let mut harness = egui_kittest::Harness::builder().with_size(Vec2::new(956., 210.)).build_ui_state(
            move |ui, state: &mut (Vec<GraphParameter>, Vec<Action>)| {
                crate::kit::theme::install(ui.ctx());
                let mut actions = Vec::new();
                show(
                    ui,
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(944., 168.)),
                    &canvas,
                    &state.0,
                    TrackId(1),
                    &crate::detail::detail_view::DeviceAt(vec![0]),
                    &mut actions,
                );
                for a in &actions {
                    if let Some(p) = state.0.iter_mut().find(|p| a.params["param"] == p.id) {
                        p.value = a.params["value"].as_f64().unwrap();
                    }
                }
                state.1.extend(actions);
            },
            (parameters, Vec::new()),
        );
        harness.run();
        harness.get_by_label("Motion waveform").click();
        harness.run();
        assert!(harness.state().1.is_empty(), "opening Wave must not edit the parameter");
        snapshot(&mut harness, "delay-wave-menu.png");
        harness.get_by_label("Wander").click();
        harness.run();
        assert_eq!(harness.state().1.len(), 1);
        assert_eq!(harness.state().1[0].params["param"], "mod_shape");
        assert_eq!(harness.state().1[0].params["value"], 6.0);
        assert_eq!(harness.state().1[0].params["continuous"], false);
        harness.get_by_label("Motion rate mode").click();
        harness.run();
        assert_eq!(harness.state().1.len(), 1);
        snapshot(&mut harness, "delay-rate-menu.png");
        harness.get_by_label("Triplet").click();
        harness.run();
        assert_eq!(harness.state().1.len(), 2);
        assert_eq!(harness.state().1[1].params["param"], "mod_rate_mode");
        assert_eq!(harness.state().1[1].params["value"], 3.0);
        harness.get_by_label("Motion rate mode").click();
        harness.run();
        harness.key_combination(&[egui::Key::Escape]);
        harness.run();
        assert_eq!(harness.state().1.len(), 2, "Escape must not edit a parameter");
        assert!(harness.query_by_label("Motion details").is_none());
        assert!(harness.query_by_label("Motion waveform").is_some());
        assert!(harness.query_by_label("Morph").is_some());
        assert!(harness.query_by_label("Delay depth").is_some());
        assert!(harness.query_by_label("Filter depth").is_some());
        harness.get_by_label("Motion rhythm").click();
        harness.run();
        assert_eq!(harness.state().1.len(), 2);
        snapshot(&mut harness, "delay-rhythm-menu.png");
        let before = harness.get_by_label("1/8 T").rect();
        harness.hover_at(egui::pos2(before.right() + 8., before.center().y));
        harness.run();
        assert_eq!(
            before,
            harness.get_by_label("1/8 T").rect(),
            "hovering the scroll bar must not resize menu content"
        );
        snapshot(&mut harness, "delay-rhythm-hover.png");
        harness.get_by_label("1/8 T").click();
        harness.run();
        assert_eq!(harness.state().1.last().unwrap().params["param"], "mod_division");
        assert_eq!(harness.state().1.last().unwrap().params["value"], 8.);
        harness.get_by_label("Motion rate mode").click();
        harness.run();
        harness.get_by_label("16th steps").click();
        harness.run();
        harness.get_by_label("Motion rhythm").click();
        harness.run();
        harness.get_by_label("3 x 1/16").click();
        harness.run();
        assert_eq!(harness.state().1.last().unwrap().params["value"], 3.);
        harness.get_by_label("Motion rate mode").click();
        harness.run();
        harness.get_by_label("Time").click();
        harness.run();
        assert!(harness.query_by_label("Motion rhythm").is_none());
        assert!(harness.query_by_label("mod time").is_some());
        assert!(harness.query_by_label("motion_time").is_none());
        harness.get_by_label("Left delay").click();
        harness.run();
        harness.get_by_label("1/4").click();
        harness.run();
        assert_eq!(harness.state().1.last().unwrap().params["param"], "division_left");
        assert_eq!(harness.state().1.last().unwrap().params["value"], 4.);
        harness.get_by_label("Left timing mode").click();
        harness.run();
        assert!(harness.query_by_label("Left delay").is_none());
        assert!(harness.query_by_label("time left").is_some());
        snapshot(&mut harness, "delay-motion.png");
    }
    /// Saves a picture of `harness` to `target/` for the design review when
    /// `SUHINA_UI_SNAPSHOTS` is set; drawing it needs a GPU adapter, which test machines may lack.
    fn snapshot<State>(harness: &mut egui_kittest::Harness<'_, State>, name: &str) {
        if std::env::var_os("SUHINA_UI_SNAPSHOTS").is_some() {
            harness.render().unwrap().save(format!("../../target/{name}")).unwrap();
        }
    }
    fn click(canvas: &Arc<DeviceUi>, parameters: &[GraphParameter], position: Pos2) -> Vec<Action> {
        pointer_gesture(canvas, parameters, position, None)
    }
    fn pointer_gesture(
        canvas: &Arc<DeviceUi>,
        parameters: &[GraphParameter],
        position: Pos2,
        drag_to: Option<Pos2>,
    ) -> Vec<Action> {
        let ctx = egui::Context::default();
        let mut actions = Vec::new();
        let mut frames = vec![(position, None), (position, Some(true))];
        if let Some(end) = drag_to {
            frames.push((end, None));
        }
        frames.push((drag_to.unwrap_or(position), Some(false)));
        for (position, pressed) in frames {
            let events = pressed.map_or_else(
                || vec![egui::Event::PointerMoved(position)],
                |pressed| {
                    vec![
                        egui::Event::PointerMoved(position),
                        egui::Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ]
                },
            );
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(956., 180.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    show(
                        ui,
                        Rect::from_min_size(Pos2::ZERO, Vec2::new(944., 168.)),
                        canvas,
                        parameters,
                        TrackId(1),
                        &crate::detail::detail_view::DeviceAt(vec![0]),
                        &mut actions,
                    )
                },
            );
            output.textures_delta.clear();
        }
        actions
    }
    #[test]
    fn idle_log_control_does_not_emit_edits_and_xy_click_is_one_gesture() {
        let ctx = egui::Context::default();
        let canvas = Arc::new(DeviceUi {
            size: [160.0, 160.0],
            controls: vec![CustomControl {
                widget: None,
                visible_when: None,
                id: "xy".into(),
                name: "XY".into(),
                tooltip: String::new(),
                bounds: [0.0, 0.0, 100.0, 100.0],
                gesture: UiGesture::Position,
                x: Some(UiBinding { parameter: "x".into(), logarithmic: true }),
                y: Some(UiBinding::new("y")),
                drawing: vec![],
            }],
            ..Default::default()
        });
        let params = [parameter("x", true), parameter("y", false)];
        let mut actions = Vec::new();
        let mut frame = |events| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(200.0))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                show(
                    ui,
                    Rect::from_min_size(Pos2::ZERO, Vec2::splat(160.0)),
                    &canvas,
                    &params,
                    TrackId(1),
                    &crate::detail::detail_view::DeviceAt(vec![0]),
                    &mut actions,
                )
            });
            output.textures_delta.clear();
        };
        frame(vec![]);
        frame(vec![
            egui::Event::PointerMoved(Pos2::new(75.0, 25.0)),
            egui::Event::PointerButton {
                pos: Pos2::new(75.0, 25.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        frame(vec![egui::Event::PointerButton {
            pos: Pos2::new(75.0, 25.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].params["continuous"], true);
        assert_eq!(actions[1].params["continuous"], false);
        assert_eq!(actions[0].params["gesture"], "xy");
        assert!((actions[0].params["value"].as_f64().unwrap() - 3556.5588).abs() < 0.1);
        assert_eq!(actions[1].params["value"], 0.75);
    }

    #[test]
    fn built_in_widget_colors_follow_the_active_theme_and_refresh_the_cache() {
        let context = egui::Context::default();
        let canvas = Arc::new(DeviceUi {
            controls: vec![CustomControl {
                widget: Some(Widget::Button { label: "On".into(), off_label: None, indicator: false }),
                id: "on".into(),
                name: "On".into(),
                tooltip: String::new(),
                bounds: [0.0, 0.0, 40.0, 12.0],
                visible_when: None,
                gesture: UiGesture::Toggle,
                x: Some(UiBinding::new("enabled")),
                y: None,
                drawing: Vec::new(),
            }],
            ..DeviceUi::default()
        });
        let dark = crate::kit::theme::Palette::DARK;
        crate::kit::theme::set_palette(&context, dark);
        let dark_loaded = images(&context, &canvas);
        let mut alternate = dark;
        alternate.accent = Color32::from_rgb(12, 34, 56);
        crate::kit::theme::set_palette(&context, alternate);
        let alternate_loaded = images(&context, &canvas);
        assert!(!Arc::ptr_eq(&dark_loaded, &alternate_loaded));
        assert!(
            alternate_loaded.controls[0]
                .iter()
                .any(|draw| matches!(draw, Draw::Rect { color, .. } if *color == [12, 34, 56, 255]))
        );
        let delay = Arc::new(super::delay_example::build().unwrap().ui.unwrap());
        let themed = images(&context, &delay);
        let cutoff = delay.controls.iter().position(|c| c.id == "cutoff").unwrap();
        assert!(themed.controls[cutoff].iter().any(|draw| matches!(
            draw, Draw::Text { color, .. } if *color == rgba(alternate.text_primary)
        )));
        let button = delay.controls.iter().position(|c| matches!(c.widget, Some(Widget::Button { .. }))).unwrap();
        assert!(themed.controls[button].iter().any(|draw| matches!(
            draw, Draw::Rect { color, .. } if *color == rgba(alternate.accent)
        )));
        crate::kit::theme::set_palette(&context, dark);
    }

    #[test]
    fn default_canvas_background_follows_the_theme_and_authored_background_stays_fixed() {
        let context = egui::Context::default();
        let mut palette = crate::kit::theme::Palette::DARK;
        palette.panel_background = Color32::from_rgb(11, 22, 33);
        crate::kit::theme::set_palette(&context, palette);
        let default = Arc::new(DeviceUi::default());
        let authored = Arc::new(DeviceUi { background: [44, 55, 66, 255], ..DeviceUi::default() });
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            let mut actions = Vec::new();
            for (x, canvas) in [(8.0, &default), (56.0, &authored)] {
                show(
                    ui,
                    Rect::from_min_size(Pos2::new(x, 8.0), Vec2::splat(40.0)),
                    canvas,
                    &[],
                    TrackId(1),
                    &crate::detail::detail_view::DeviceAt(vec![0]),
                    &mut actions,
                );
            }
        });
        assert!(output.shapes.iter().any(|shape| matches!(
            &shape.shape,
            egui::Shape::Rect(rect) if rect.fill == palette.panel_background
        )));
        assert!(output.shapes.iter().any(|shape| matches!(
            &shape.shape,
            egui::Shape::Rect(rect) if rect.fill == Color32::from_rgb(44, 55, 66)
        )));
        output.textures_delta.clear();
        crate::kit::theme::set_palette(&context, crate::kit::theme::Palette::DARK);
    }
}

#[cfg(test)]
#[allow(clippy::duplicate_mod)]
#[path = "../../../device-sdk/examples/delay.rs"]
mod delay_example;
