// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Alpha-correct blur, isolated from decoding and GPU publication.
use egui::ColorImage;

pub(super) fn apply(source: &image::RgbaImage, sigma: f32) -> ColorImage {
    let size = [source.width() as usize, source.height() as usize];
    if sigma <= 0.0 {
        return ColorImage::from_rgba_unmultiplied(size, source.as_raw());
    }
    // Premultiplication prevents invisible RGB from bleeding into visible edges.
    let premultiplied = image::ImageBuffer::from_fn(source.width(), source.height(), |x, y| {
        let pixel = source.get_pixel(x, y);
        let alpha = f32::from(pixel[3]) / 255.0;
        image::Rgba([
            f32::from(pixel[0]) * alpha,
            f32::from(pixel[1]) * alpha,
            f32::from(pixel[2]) * alpha,
            f32::from(pixel[3]),
        ])
    });
    let blurred = image::imageops::blur(&premultiplied, sigma);
    let rgba = image::ImageBuffer::from_fn(source.width(), source.height(), |x, y| {
        let p = blurred.get_pixel(x, y);
        let alpha = p[3].clamp(0.0, 255.0);
        let factor = if alpha > 0.0 { 255.0 / alpha } else { 0.0 };
        image::Rgba([
            (p[0] * factor).clamp(0.0, 255.0) as u8,
            (p[1] * factor).clamp(0.0, 255.0) as u8,
            (p[2] * factor).clamp(0.0, 255.0) as u8,
            alpha as u8,
        ])
    });
    ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transparent_red_does_not_contaminate_visible_blue() {
        let mut input = image::RgbaImage::from_pixel(7, 1, image::Rgba([255, 0, 0, 0]));
        input.put_pixel(3, 0, image::Rgba([0, 0, 255, 255]));
        let result = apply(&input, 1.0);
        assert!(result.pixels.iter().all(|pixel| pixel.r() == 0));
        assert!(result.pixels.iter().any(|pixel| pixel.b() > 0));
    }
}
