// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Field view's saturation/value plane, centered in the panel: a mesh, cached while its hue, aids and
//! rectangle stay the same, so moving the marker never rebuilds it.

use super::Hsv;
use crate::kit::{element, theme};
use egui::{Color32, Key, Mesh, Modifiers, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};
use std::sync::Arc;

/// The plane's size, its frame included; the Steps grid is the same size.
pub(super) const SIZE: Vec2 = Vec2::new(198.0, 110.0);

#[derive(Default)]
pub(super) struct Cache {
    key: Option<(Rect, f32, bool)>,
    mesh: Arc<Mesh>,
}

impl Cache {
    pub fn show(&mut self, ui: &mut Ui, hsv: &mut Hsv, soft: bool) -> Response {
        let (rect, mut response) = ui.allocate_exact_size(SIZE, Sense::click_and_drag());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, ui.is_enabled(), "Color field"));
        let field = element::COLOR_FIELD.response(&response).enter();
        let rect = field.paint(ui.painter(), rect).content;
        if !rect.is_positive() {
            return response;
        }
        let key = (rect, hsv.h, soft);
        if self.key != Some(key) {
            self.mesh = Arc::new(build(rect, hsv.h, soft));
            self.key = Some(key);
        }
        ui.painter().add(Shape::Mesh(self.mesh.clone()));
        if (response.dragged() || response.clicked())
            && let Some(at) = response.interact_pointer_pos()
        {
            hsv.s = ((at.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            hsv.v = (1.0 - (at.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
            response.request_focus();
            response.mark_changed();
        }
        if response.has_focus() {
            ui.memory_mut(|m| {
                let arrows = egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() };
                m.set_focus_lock_filter(response.id, arrows);
            });
            let (horizontal, vertical) = ui.input_mut(|i| {
                let left = i.consume_key(Modifiers::NONE, Key::ArrowLeft);
                let right = i.consume_key(Modifiers::NONE, Key::ArrowRight);
                let up = i.consume_key(Modifiers::NONE, Key::ArrowUp);
                let down = i.consume_key(Modifiers::NONE, Key::ArrowDown);
                (i32::from(right) - i32::from(left), i32::from(up) - i32::from(down))
            });
            if horizontal != 0 || vertical != 0 {
                hsv.s = (hsv.s + horizontal as f32 * 0.01).clamp(0.0, 1.0);
                hsv.v = (hsv.v + vertical as f32 * 0.01).clamp(0.0, 1.0);
                response.mark_changed();
            }
        }
        let marker = Pos2::new(rect.left() + hsv.s * rect.width(), rect.bottom() - hsv.v * rect.height());
        let [r, g, b] = hsv.to_srgb();
        ui.painter().circle_stroke(marker, 4.0, Stroke::new(2.0, theme::text_on(Color32::from_rgb(r, g, b))));
        response
    }
}

fn build(rect: Rect, hue: f32, soft: bool) -> Mesh {
    const COLUMNS: usize = 24;
    const ROWS: usize = 16;
    let mut mesh = Mesh::default();
    for row in 0..ROWS {
        for column in 0..COLUMNS {
            let index = mesh.vertices.len() as u32;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let x = (column + dx) as f32 / COLUMNS as f32;
                let y = (row + dy) as f32 / ROWS as f32;
                let position = Pos2::new(rect.left() + x * rect.width(), rect.top() + y * rect.height());
                let mut color = Hsv { h: hue, s: x, v: 1.0 - y };
                if soft && (!(0.25..=0.7).contains(&color.s) || !(0.45..=0.9).contains(&color.v)) {
                    color.s = 0.0;
                    color.v *= 0.45;
                }
                let [r, g, b] = color.to_srgb();
                mesh.colored_vertex(position, Color32::from_rgb(r, g, b));
            }
            mesh.add_triangle(index, index + 1, index + 2);
            mesh.add_triangle(index + 1, index + 3, index + 2);
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_field_spans_saturation_and_value_and_its_geometry_is_bounded() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(198.0, 110.0));
        for soft in [false, true] {
            let mesh = build(rect, 0.137, soft);
            assert!(mesh.vertices.len() <= 4096);
            assert!(mesh.indices.len() <= 6144);
            assert!(mesh.vertices.iter().all(|v| rect.expand(0.01).contains(v.pos)));
        }
        let mesh = build(rect, 0.0, false);
        let corner = |at: Pos2| mesh.vertices.iter().find(|v| v.pos == at).map(|v| v.color);
        assert_eq!(corner(rect.left_top()), Some(Color32::WHITE));
        assert_eq!(corner(rect.right_top()), Some(Color32::from_rgb(255, 0, 0)));
        assert_eq!(corner(rect.left_bottom()), Some(Color32::BLACK));
    }
}
