// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window's see-through pixels. Every background blends, as every other shape does: the window
//! clears once (to nothing while the theme asks for a see-through window), the root paints its
//! background once, and each element paints only its own background over what is under it. A
//! theme's alpha is therefore how much of what is under an element shows through it, and where
//! nothing under it is opaque, how much of the desktop shows through the window.
//!
//! One thing still replaces pixels: a menu's or dialog's blurred backdrop
//! (`kit::liquid::finish_backdrops`) first clears the window's pixels under it with
//! [`clear`], so the sharp content it is a picture of does not show through the picture's alpha.
//! The host draws that clear with its [`Provider`]; without one (the glow renderer, an opaque
//! window, tests) nothing is cleared.

use std::sync::Arc;

use egui::{Color32, Context, CornerRadius, Id, Rect, Shape};

const HOST: &str = "surface-host";

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

/// Whether the window shows the desktop through its pixels: only while the theme asks for a
/// see-through window (its `--window-background` has alpha). Over an opaque theme, or over a
/// private background image, nothing shows the desktop, so nothing is cleared.
pub fn available(ctx: &Context) -> bool {
    ctx.data(|data| data.get_temp::<Host>(Id::unique(HOST))).is_some()
        && super::theme::window_background().a() < u8::MAX
        && super::liquid::background_image(ctx).is_none()
}

/// A shape that clears `rect` to see-through, for something painted over the window's content
/// that must not blend with it; `None` when the window shows no desktop.
pub fn clear(ctx: &Context, rect: Rect) -> Option<Shape> {
    if !available(ctx) {
        return None;
    }
    let host = ctx.data(|data| data.get_temp::<Host>(Id::unique(HOST)))?;
    Some(host.0.shape(rect, CornerRadius::ZERO, Color32::TRANSPARENT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, Vec2};

    /// Marks the clearing shapes so the test can find them.
    struct Marking;

    impl Provider for Marking {
        fn shape(&self, rect: Rect, _: CornerRadius, color: Color32) -> Shape {
            Shape::circle_filled(rect.center(), 1.0, color)
        }
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
    fn a_see_through_window_clears_with_the_provider() {
        let ctx = see_through();
        assert!(matches!(clear(&ctx, rect()), Some(Shape::Circle(_))));
    }

    #[test]
    fn an_opaque_theme_or_no_provider_clears_nothing() {
        let ctx = see_through();
        super::super::theme::set_palette(&ctx, super::super::theme::Palette::DARK);
        assert!(clear(&ctx, rect()).is_none());
        assert!(clear(&Context::default(), rect()).is_none());
    }

    #[test]
    fn a_private_image_clears_nothing_and_removing_it_clears_again() {
        let ctx = see_through();
        for private_image in [true, false, true] {
            super::super::liquid::set_background_image(
                &ctx,
                private_image.then(|| super::super::liquid::BackgroundImage {
                    id: egui::TextureId::Managed(42),
                    pixels: Arc::new(egui::ColorImage::filled([2, 2], Color32::RED)),
                }),
            );
            assert_eq!(clear(&ctx, rect()).is_none(), private_image);
        }
    }
}
