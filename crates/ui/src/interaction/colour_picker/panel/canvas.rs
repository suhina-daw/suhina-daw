// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fixed-size meshes, cached while their colour plane stays the same. Moving the marker
//! never rebuilds the field; the wheel only rebuilds when its value/mode/rectangle changes.

use crate::kit::{element, theme};
use egui::{
    Color32, Key, Mesh, Modifiers, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, ecolor::Hsva,
};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct Cache {
    key: Option<(Rect, f32, bool, bool, bool)>,
    mesh: Arc<Mesh>,
}

impl Cache {
    pub fn show(&mut self, ui: &mut Ui, hsv: &mut Hsva, wheel: bool, steps: bool, soft: bool) -> Response {
        let size = if wheel { Vec2::splat(150.0) } else { Vec2::new(268.0, 110.0) };
        let (rect, mut response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let label = if wheel { "Colour wheel" } else { "Colour field" };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, label));
        let key = (rect, if wheel { hsv.v } else { hsv.h }, wheel, steps, soft);
        if self.key != Some(key) {
            self.mesh = Arc::new(build(rect, *hsv, wheel, steps, soft));
            self.key = Some(key);
        }
        ui.painter().add(Shape::Mesh(self.mesh.clone()));
        if (response.dragged() || response.clicked())
            && let Some(at) = response.interact_pointer_pos()
        {
            if wheel {
                let delta = at - rect.center();
                hsv.h = delta.y.atan2(delta.x).rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
                hsv.s = (delta.length() / (rect.width() * 0.5)).clamp(0.0, 1.0);
            } else {
                hsv.s = ((at.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                hsv.v = (1.0 - (at.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
            }
            response.request_focus();
            response.mark_changed();
        }
        if response.has_focus() {
            let (horizontal, vertical) = ui.input_mut(|i| {
                let left = i.consume_key(Modifiers::NONE, Key::ArrowLeft);
                let right = i.consume_key(Modifiers::NONE, Key::ArrowRight);
                let up = i.consume_key(Modifiers::NONE, Key::ArrowUp);
                let down = i.consume_key(Modifiers::NONE, Key::ArrowDown);
                (i32::from(right) - i32::from(left), i32::from(up) - i32::from(down))
            });
            if horizontal != 0 || vertical != 0 {
                let step = if steps { 0.1 } else { 0.01 };
                if wheel {
                    hsv.h = (hsv.h + horizontal as f32 * if steps { 1.0 / 24.0 } else { 1.0 / 360.0 }).rem_euclid(1.0);
                    hsv.s = (hsv.s + vertical as f32 * step).clamp(0.0, 1.0);
                } else {
                    hsv.s = (hsv.s + horizontal as f32 * step).clamp(0.0, 1.0);
                    hsv.v = (hsv.v + vertical as f32 * step).clamp(0.0, 1.0);
                }
                response.mark_changed();
            }
        }
        let marker = if wheel {
            let angle = hsv.h * std::f32::consts::TAU;
            rect.center() + Vec2::new(angle.cos(), angle.sin()) * hsv.s * rect.width() * 0.5
        } else {
            Pos2::new(rect.left() + hsv.s * rect.width(), rect.bottom() - hsv.v * rect.height())
        };
        let [r, g, b] = hsv.to_srgb();
        ui.painter().circle_stroke(marker, 4.0, Stroke::new(2.0, theme::text_on(Color32::from_rgb(r, g, b))));
        let field =
            element::COLOUR_FIELD.attribute("kind", if wheel { "wheel" } else { "field" }).response(&response).enter();
        ui.painter().rect_stroke(
            rect,
            0.0,
            Stroke::new(if response.has_focus() { 2.0 } else { 1.0 }, field.border()),
            StrokeKind::Inside,
        );
        response
    }
}

fn build(rect: Rect, hsv: Hsva, wheel: bool, steps: bool, soft: bool) -> Mesh {
    let mut mesh = Mesh::default();
    let columns = if wheel {
        if steps { 24 } else { 64 }
    } else if steps {
        10
    } else {
        24
    };
    let rows = if steps { 10 } else { 16 };
    for row in 0..rows {
        for column in 0..columns {
            let index = mesh.vertices.len() as u32;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let x = (column + dx) as f32 / columns as f32;
                let y = (row + dy) as f32 / rows as f32;
                let (position, mut colour) = if wheel {
                    let angle = x * std::f32::consts::TAU;
                    (
                        rect.center() + Vec2::new(angle.cos(), angle.sin()) * y * rect.width() * 0.5,
                        Hsva { h: x, s: y, v: hsv.v, a: 1.0 },
                    )
                } else {
                    (
                        Pos2::new(rect.left() + x * rect.width(), rect.top() + y * rect.height()),
                        Hsva { h: hsv.h, s: x, v: 1.0 - y, a: 1.0 },
                    )
                };
                // A stepped cell has one colour, with a visibly discrete boundary.
                if steps {
                    colour.h = if wheel { (column as f32 + 0.5) / columns as f32 } else { hsv.h };
                    colour.s =
                        if wheel { (row as f32 + 0.5) / rows as f32 } else { (column as f32 + 0.5) / columns as f32 };
                    if !wheel {
                        colour.v = 1.0 - (row as f32 + 0.5) / rows as f32;
                    }
                    colour = super::constrain(colour, true, false);
                }
                if soft && (!(0.25..=0.7).contains(&colour.s) || !(0.45..=0.9).contains(&colour.v)) {
                    colour.s = 0.0;
                    colour.v *= 0.45;
                }
                let [r, g, b] = colour.to_srgb();
                mesh.colored_vertex(position, Color32::from_rgb(r, g, b));
            }
            mesh.add_triangle(index, index + 1, index + 2);
            mesh.add_triangle(index + 1, index + 3, index + 2);
        }
    }
    mesh
}
