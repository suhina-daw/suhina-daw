// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Backgrounds that set the window's pixels. A background fill writes its colour and its alpha
//! into the pixels it covers instead of blending with what was painted under it, so a theme's
//! alpha is how much of the desktop shows through the window there (with the system's blur
//! where it has one). Text, lines, marks and overlays still blend, and so does everything in a
//! floating layer: dialogs, menus and floating panels blend with the window behind them.
//!
//! Views paint a background with [`Backgrounds::fill_background`], an ordinary filled
//! rectangle while the frame is drawn, so the liquid materials capture it like any shape. Once
//! the frame is complete, [`finish_frame`] swaps each one for the host's [`Provider`] shape,
//! which writes the pixels directly. Without a provider (the glow renderer, an opaque window,
//! tests) the rectangles stay as they are and blend as before.

use std::sync::Arc;

use egui::layers::ShapeIdx;
use egui::{Color32, Context, CornerRadius, Id, LayerId, Order, Painter, Rect, Shape};

const HOST: &str = "surface-host";
const FILLS: &str = "surface-fills";
/// At most this many backgrounds are swapped in a frame; any further ones blend.
const MAX_FILLS: usize = 32768;

/// The native side: a shape that writes `color` (premultiplied, as egui keeps it) and its alpha
/// into the pixels of `rect` inside its rounded corners.
pub trait Provider: Send + Sync {
    fn shape(&self, rect: Rect, corner_radius: CornerRadius, color: Color32) -> Shape;
}

#[derive(Clone)]
struct Host(Arc<dyn Provider>);

/// Installs the provider for a window whose pixels show the desktop through their alpha.
pub fn install(ctx: &Context, provider: Arc<dyn Provider>) {
    ctx.data_mut(|data| data.insert_temp(Id::unique(HOST), Host(provider)));
}

/// Whether backgrounds set the window's pixels in this window: only while the theme asks for a
/// see-through window (its `--window-background` has alpha). An opaque theme's alpha colours
/// blend with Suhina's own surfaces, never with the desktop. A private image needs normal
/// blending, so its pixels remain under the theme surfaces instead of exposing the desktop.
pub fn available(ctx: &Context) -> bool {
    ctx.data(|data| data.get_temp::<Host>(Id::unique(HOST))).is_some()
        && super::theme::window_background().a() < u8::MAX
        && super::liquid::background_image(ctx).is_none()
}

/// A shape that clears `rect` to see-through, for something painted over the window's content
/// that must not blend with it; `None` when backgrounds blend.
pub fn clear(ctx: &Context, rect: Rect) -> Option<Shape> {
    if !available(ctx) {
        return None;
    }
    let host = ctx.data(|data| data.get_temp::<Host>(Id::unique(HOST)))?;
    Some(host.0.shape(rect, CornerRadius::ZERO, Color32::TRANSPARENT))
}

/// Forgets the backgrounds of a frame that never finished.
pub fn begin_frame(ctx: &Context) {
    ctx.data_mut(|data| data.remove_temp::<Vec<(LayerId, ShapeIdx)>>(Id::unique(FILLS)));
}

/// Swaps this frame's backgrounds for the provider's shapes. Called once everything is painted.
pub fn finish_frame(ctx: &Context) {
    let fills = ctx.data_mut(|data| data.remove_temp::<Vec<(LayerId, ShapeIdx)>>(Id::unique(FILLS)));
    let host = ctx.data(|data| data.get_temp::<Host>(Id::unique(HOST)));
    let (Some(fills), Some(host)) = (fills, host) else { return };
    ctx.graphics_mut(|layers| {
        for (layer, index) in fills {
            let Some(list) = layers.get_mut(layer) else { continue };
            list.mutate_shape(index, |clipped| {
                // A shape set again after it was painted is left alone.
                if let Shape::Rect(rect) = &clipped.shape
                    && plain(rect)
                {
                    clipped.shape = host.0.shape(rect.rect, rect.corner_radius, rect.fill);
                }
            });
        }
    });
}

/// A filled, upright rectangle with nothing else to draw: no outline, texture, blur or brush.
fn plain(rect: &egui::epaint::RectShape) -> bool {
    rect.stroke.is_empty()
        && rect.blur_width == 0.0
        && rect.angle == 0.0
        && rect.brush.is_none()
        && rect.rect.is_finite()
}

/// Background fills for painters.
pub trait Backgrounds {
    /// Fills `rect` with a background colour: a theme's background token, or a colour mixed
    /// from them. In the window's own layers it sets the pixels' colour and alpha; in a floating
    /// layer it blends, as every other shape does.
    fn fill_background(&self, rect: Rect, corner_radius: impl Into<CornerRadius>, color: impl Into<Color32>);
}

impl Backgrounds for Painter {
    fn fill_background(&self, rect: Rect, corner_radius: impl Into<CornerRadius>, color: impl Into<Color32>) {
        let index = self.add(Shape::rect_filled(rect, corner_radius, color));
        let layer = self.layer_id();
        // Only the main window is see-through: a second window (the sample map's) blends.
        let main = self.ctx().viewport_id() == egui::ViewportId::ROOT;
        if !matches!(layer.order, Order::Background | Order::Middle) || !main || !available(self.ctx()) {
            return;
        }
        self.ctx().data_mut(|data| {
            let fills = data.get_temp_mut_or_default::<Vec<(LayerId, ShapeIdx)>>(Id::unique(FILLS));
            if fills.len() < MAX_FILLS {
                fills.push((layer, index));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, Vec2};

    /// Marks the swapped shapes so the test can find them.
    struct Marking;

    impl Provider for Marking {
        fn shape(&self, rect: Rect, _: CornerRadius, color: Color32) -> Shape {
            Shape::circle_filled(rect.center(), 1.0, color)
        }
    }

    fn painted(ctx: &Context, paint: impl Fn(&Context)) -> Vec<Shape> {
        let mut output = ctx.run_ui(Default::default(), |ui| {
            begin_frame(ui.ctx());
            paint(ui.ctx());
            finish_frame(ui.ctx());
        });
        output.textures_delta.clear();
        output.shapes.into_iter().map(|clipped| clipped.shape).collect()
    }

    /// A context with the provider and a theme that asks for a see-through window.
    fn see_through() -> Context {
        let ctx = Context::default();
        let mut palette = super::super::theme::Palette::DARK;
        palette.window_background = Color32::from_rgba_unmultiplied(20, 20, 20, 200);
        super::super::theme::set_palette(&ctx, palette);
        install(&ctx, Arc::new(Marking));
        ctx
    }

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(40.0, 20.0))
    }

    #[test]
    fn backgrounds_in_the_window_are_swapped_and_floating_ones_blend() {
        let ctx = see_through();
        let shapes = painted(&ctx, |ctx| {
            let window = ctx.layer_painter(LayerId::background());
            window.fill_background(rect(), 0.0, Color32::from_rgba_unmultiplied(20, 90, 130, 100));
            // An overlay in the same layer blends.
            window.rect_filled(rect(), 0.0, Color32::from_white_alpha(30));
            let dialog = ctx.layer_painter(LayerId::new(Order::Foreground, Id::unique("dialog")));
            dialog.fill_background(rect(), 0.0, Color32::from_rgb(1, 2, 3));
        });
        let circles = shapes.iter().filter(|s| matches!(s, Shape::Circle(_))).count();
        let rects = shapes.iter().filter(|s| matches!(s, Shape::Rect(_))).count();
        assert_eq!((circles, rects), (1, 2), "{shapes:?}");
    }

    #[test]
    fn an_opaque_theme_blends_every_background() {
        let ctx = see_through();
        super::super::theme::set_palette(&ctx, super::super::theme::Palette::DARK);
        let shapes = painted(&ctx, |ctx| {
            let color = Color32::from_rgba_unmultiplied(20, 90, 130, 100);
            ctx.layer_painter(LayerId::background()).fill_background(rect(), 0.0, color);
        });
        assert!(!shapes.iter().any(|s| matches!(s, Shape::Circle(_))), "{shapes:?}");
        assert!(clear(&ctx, rect()).is_none());
    }

    #[test]
    fn without_a_provider_a_background_is_an_ordinary_rectangle() {
        let ctx = Context::default();
        let shapes = painted(&ctx, |ctx| {
            ctx.layer_painter(LayerId::background()).fill_background(rect(), 2.0, Color32::from_rgb(9, 9, 9));
        });
        assert!(shapes.iter().any(|s| matches!(s, Shape::Rect(r) if r.fill == Color32::from_rgb(9, 9, 9))));
        assert!(!shapes.iter().any(|s| matches!(s, Shape::Circle(_))));
    }

    #[test]
    fn private_image_keeps_surface_blending_and_removal_restores_transparency() {
        let ctx = see_through();
        for private_image in [true, false, true] {
            super::super::liquid::set_background_image(
                &ctx,
                private_image.then(|| super::super::liquid::BackgroundImage {
                    id: egui::TextureId::Managed(42),
                    pixels: Arc::new(egui::ColorImage::filled([2, 2], Color32::RED)),
                }),
            );
            for alpha in [0, 128, 255] {
                let color = Color32::from_rgba_unmultiplied(20, 90, 130, alpha);
                let shapes = painted(&ctx, |ctx| {
                    ctx.layer_painter(LayerId::background()).fill_background(rect(), 0.0, color);
                });
                assert_eq!(shapes.iter().any(|s| matches!(s, Shape::Circle(_))), !private_image);
                assert_eq!(clear(&ctx, rect()).is_none(), private_image);
            }
        }
    }

    #[test]
    fn a_shape_set_again_after_it_was_painted_is_left_alone() {
        let ctx = see_through();
        let shapes = painted(&ctx, |ctx| {
            let painter = ctx.layer_painter(LayerId::background());
            painter.fill_background(rect(), 0.0, Color32::from_rgb(9, 9, 9));
            // The fill's slot is taken over by something else, as a lens does.
            ctx.graphics_mut(|layers| {
                let list = layers.entry(LayerId::background());
                list.set(ShapeIdx(0), rect(), Shape::Noop);
            });
        });
        assert!(!shapes.iter().any(|s| matches!(s, Shape::Circle(_))));
    }
}
