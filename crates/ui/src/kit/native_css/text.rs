// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded text shadows reuse native glyph layout; styling never changes hit geometry.
use std::sync::Arc;

use egui::{Color32, Galley, Painter, Pos2, Rect, Vec2};
use suhina_style::paint::ResolvedShadow;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Shadow {
    offset: Vec2,
    blur: f32,
    color: Color32,
}

impl Shadow {
    pub fn from_native(shadow: &ResolvedShadow) -> Self {
        Self {
            offset: Vec2::new(bounded(shadow.offset[0], -256.0, 256.0), bounded(shadow.offset[1], -256.0, 256.0)),
            blur: bounded(shadow.blur, 0.0, 32.0),
            color: Color32::from_rgba_unmultiplied(shadow.color[0], shadow.color[1], shadow.color[2], shadow.color[3]),
        }
    }

    fn paint(&self, painter: &Painter, pos: Pos2, galley: &Arc<Galley>) {
        if self.color.a() == 0
            || !painter.clip_rect().intersects(Rect::from_min_size(pos + self.offset, galley.size()).expand(self.blur))
        {
            return;
        }
        let pos = pos + self.offset;
        if self.blur <= f32::EPSILON {
            painter.galley_with_override_text_color(pos, Arc::clone(galley), self.color);
            return;
        }
        // A fixed separable 1/4, 1/2, 1/4 kernel approximates a soft shadow. Keep
        // draw work bounded independently of authored blur radius and label size.
        // The taps stay within a quarter of the text's height: spread wider, small text showed
        // as separate copies of itself rather than a shadow.
        let reach = (self.blur * 0.5).min(galley.size().y.max(1.0) * 0.12).max(0.5);
        for y in -1..=1 {
            for x in -1..=1 {
                let weight = (if x == 0 { 2.0 } else { 1.0 }) * (if y == 0 { 2.0 } else { 1.0 }) / 16.0;
                painter.galley_with_override_text_color(
                    pos + Vec2::new(x as f32, y as f32) * reach,
                    Arc::clone(galley),
                    self.color.gamma_multiply(weight),
                );
            }
        }
    }
}

fn bounded(value: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() { value.clamp(min, max) } else { 0.0 }
}

pub fn paint_text(painter: &Painter, pos: Pos2, galley: Arc<Galley>, fallback_color: Color32) {
    if !pos.is_finite() || galley.is_empty() {
        return;
    }
    if let Some(style) = super::super::style_class::computed() {
        // CSS lists the frontmost text shadow first, like background/shadow layers.
        for shadow in style.text_shadows.iter().rev() {
            shadow.paint(painter, pos, &galley);
        }
    }
    painter.galley(pos, galley, fallback_color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::{skin, style_class};

    fn draw(css: &str) -> Vec<egui::epaint::TextShape> {
        skin::configure(&skin::Skin { css: Some(Arc::from(css)), ..Default::default() });
        let ctx = egui::Context::default();
        let mut output: Option<egui::FullOutput> = None;
        for frame in 0..3 {
            if let Some(previous) = output.as_mut() {
                previous.textures_delta.clear();
            }
            output = Some(ctx.run_ui(
                egui::RawInput {
                    time: Some(frame as f64 * 0.01),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1024.0, 768.0))),
                    ..Default::default()
                },
                |ui| {
                    let _frame = super::super::frame(ui);
                    let _parent = style_class::enter("browser");
                    let _control = style_class::enter("button");
                    let galley =
                        ui.painter().layout_no_wrap("Native".into(), egui::FontId::proportional(12.0), Color32::WHITE);
                    paint_text(ui.painter(), Pos2::new(40.0, 40.0), galley, Color32::WHITE);
                },
            ));
        }
        let mut output = output.unwrap();
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.clone()),
                _ => None,
            })
            .collect();
        output.textures_delta.clear();
        skin::configure(&skin::Skin::default());
        text
    }

    #[test]
    fn inherited_shadow_lists_resolve_current_color_and_keep_native_glyph_layout() {
        let text = draw(".browser {text-shadow:3px 4px #aabbcc, -2px -1px currentColor;} .button {color:#112233;}");
        assert_eq!(text.len(), 3);
        assert_eq!(text[0].pos, Pos2::new(38.0, 39.0));
        assert_eq!(text[0].override_text_color, Some(Color32::from_rgb(0x11, 0x22, 0x33)));
        assert_eq!(text[1].pos, Pos2::new(43.0, 44.0));
        assert_eq!(text[1].override_text_color, Some(Color32::from_rgb(0xaa, 0xbb, 0xcc)));
        assert_eq!(text[2].pos, Pos2::new(40.0, 40.0));
        assert!(text[2].override_text_color.is_none());
        assert!(text.iter().all(|shape| Arc::ptr_eq(&shape.galley, &text[2].galley)));
    }

    #[test]
    fn a_blurred_shadow_stays_close_to_small_text() {
        let text = draw(".button {text-shadow:0 0 8px #000000;}");
        assert_eq!(text.len(), 10);
        for shadow in &text[..9] {
            assert!((shadow.pos - Pos2::new(40.0, 40.0)).length() < 3.0, "{:?}", shadow.pos);
        }
    }

    #[test]
    fn hostile_blur_offsets_and_layer_counts_have_fixed_paint_work() {
        let list = std::iter::repeat_n("10000px 10000px 10000px red", 8).collect::<Vec<_>>().join(",");
        let text = draw(&format!(".button {{text-shadow:{list};}}"));
        assert_eq!(text.len(), 4 * 9 + 1);
        for shadow in &text[..36] {
            assert!((280.0..=312.0).contains(&shadow.pos.x));
            assert!((280.0..=312.0).contains(&shadow.pos.y));
        }
        assert_eq!(text[36].pos, Pos2::new(40.0, 40.0));
    }

    #[test]
    fn none_and_invalid_shadows_do_not_add_drawing() {
        assert_eq!(draw(".browser {text-shadow:1px 1px black;} .button {text-shadow:none;}").len(), 1);
        assert_eq!(draw(".button {text-shadow:1px 2px -5px red;}").len(), 1);
        assert!(
            draw(".button {text-shadow:calc(infinity * 1px) 0 2px black;}").iter().all(|text| text.pos.is_finite())
        );
    }

    #[test]
    fn text_shadow_keyframes_follow_native_time_and_motion_suppression() {
        let css = ".browser {text-shadow:0 0 red;animation:glow 1s linear forwards;} @keyframes glow {from{text-shadow:0 0 red;}to{text-shadow:10px 0 blue;}}";
        for motion in [true, false] {
            skin::configure(&skin::Skin { css: Some(Arc::from(css)), ..Default::default() });
            let ctx = egui::Context::default();
            let mut samples = Vec::new();
            for time in [0.0, 0.5, 1.1] {
                ctx.run_ui(
                    egui::RawInput {
                        time: Some(time),
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
                        ..Default::default()
                    },
                    |ui| {
                        let _frame = super::super::frame_with_motion(ui, motion);
                        let _scope = style_class::enter("browser");
                    },
                )
                .textures_delta
                .clear();
                samples.push(super::super::ENGINE.with(|engine| {
                    let engine = engine.borrow();
                    let key = engine.nodes.iter().find(|node| node.class == "browser").unwrap().key;
                    engine.values[&key].text_shadows[0].clone()
                }));
            }
            if motion {
                assert!(samples[1].offset.x > 1.0 && samples[1].offset.x < 9.0);
                assert!(samples[1].color.r() > 20 && samples[1].color.b() > 20);
                assert_eq!(samples[2].offset.x, 10.0);
                assert_eq!(samples[2].color, Color32::BLUE);
            } else {
                assert!(samples.iter().all(|shadow| shadow.offset == Vec2::ZERO && shadow.color == Color32::RED));
            }
        }
        skin::configure(&skin::Skin::default());
    }
}
