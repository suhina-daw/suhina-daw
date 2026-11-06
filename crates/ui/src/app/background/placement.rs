// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Image placement and display-space blur units; no I/O or renderer state.
use egui::{Rect, Vec2, pos2};

pub(super) fn cover_scale(image: Vec2, display: Vec2) -> f32 {
    (display.x / image.x.max(1.0)).max(display.y / image.y.max(1.0)).max(0.001)
}

pub(super) fn cover_uv(image: Vec2, display: Vec2) -> Rect {
    Rect::from_center_size(pos2(0.5, 0.5), display / (image * cover_scale(image, display)))
}

pub(super) fn source_sigma(blur: u8, image: Vec2, display: Vec2) -> f32 {
    (f32::from(blur.min(40)) / cover_scale(image, display)).min(80.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blur_is_measured_on_display_not_thumbnail() {
        assert_eq!(source_sigma(5, Vec2::new(960.0, 540.0), Vec2::new(1920.0, 1080.0)), 2.5);
        assert_eq!(source_sigma(0, Vec2::splat(100.0), Vec2::splat(200.0)), 0.0);
    }
    #[test]
    fn cover_crops_symmetrically() {
        let uv = cover_uv(Vec2::new(200.0, 100.0), Vec2::splat(100.0));
        assert_eq!(uv.min, pos2(0.25, 0.0));
        assert_eq!(uv.max, pos2(0.75, 1.0));
    }
}
