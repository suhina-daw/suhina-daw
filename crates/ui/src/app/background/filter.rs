// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Alpha-correct blur, isolated from decoding and GPU publication.
use egui::ColorImage;

/// The widest a blur is taken at the picture's own size, in pixels of sigma; wider ones are taken
/// on a smaller copy, so a big radius stays smooth, with no banding, and is quick.
const FULL_SIZE_SIGMA: f32 = 4.0;

/// `image` Gaussian-blurred by `sigma` pixels. A wide blur runs on a copy scaled down by about a
/// third of the sigma, which is scaled back up with a smooth filter.
fn soft_blur(
    image: &image::ImageBuffer<image::Rgba<f32>, Vec<f32>>,
    sigma: f32,
) -> image::ImageBuffer<image::Rgba<f32>, Vec<f32>> {
    use image::imageops::{FilterType, blur, resize};
    let (width, height) = image.dimensions();
    if sigma <= FULL_SIZE_SIGMA || width < 2 || height < 2 {
        return blur(image, sigma);
    }
    let factor = sigma / 3.0;
    let small = |side: u32| ((side as f32 / factor).round() as u32).clamp(1, side);
    let (small_width, small_height) = (small(width), small(height));
    // The resizing filters hold float pixels to 0 to 1.
    let unit = |scale: f32, from: &image::ImageBuffer<image::Rgba<f32>, Vec<f32>>| {
        image::ImageBuffer::from_fn(from.width(), from.height(), |x, y| {
            let p = from.get_pixel(x, y);
            image::Rgba([p[0] * scale, p[1] * scale, p[2] * scale, p[3] * scale])
        })
    };
    let reduced = resize(&unit(1.0 / 255.0, image), small_width, small_height, FilterType::Triangle);
    let blurred = blur(&reduced, sigma * small_width as f32 / width as f32);
    unit(255.0, &resize(&blurred, width, height, FilterType::CatmullRom))
}

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
    let blurred = soft_blur(&premultiplied, sigma);
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
        assert!(result.pixels.iter().all(|pixel| pixel.to_array()[0] == 0));
        assert!(result.pixels.iter().any(|pixel| pixel.to_array()[2] > 0));
    }
    #[test]
    fn a_wide_blur_of_a_spot_is_smooth_and_symmetric() {
        let mut input = image::RgbaImage::from_pixel(129, 129, image::Rgba([0, 0, 0, 255]));
        for (x, y) in (61..68).flat_map(|x| (61..68).map(move |y| (x, y))) {
            input.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
        }
        let row = 64 * 129;
        let level = |result: &ColorImage, x: usize| result.pixels[row + x].to_array()[0];
        let result = apply(&input, 12.0);
        assert!(level(&result, 64) > 0 && level(&result, 64) < 255, "{}", level(&result, 64));
        // Falling away from the middle, the same each side, with no bands.
        for x in 64..110 {
            assert!(level(&result, x + 1) <= level(&result, x) + 1, "rises at {x}");
        }
        assert!(level(&result, 40).abs_diff(level(&result, 88)) <= 2);
    }
}
