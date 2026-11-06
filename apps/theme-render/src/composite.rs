// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The window's pixels over what is behind it, as the system's compositor puts them: a window
//! keeps premultiplied alpha, and a see-through one shows the desktop blurred.

use image::{Rgba, RgbaImage};

use crate::Shot;

/// How far the system blurs what is behind a see-through window, in points. An approximation of
/// the window server's own blur, which is not documented.
pub const BLUR: f32 = 24.0;

/// The window's pixels as a PNG keeps them: straight alpha.
pub fn unpremultiply(pixels: &RgbaImage) -> RgbaImage {
    let mut out = pixels.clone();
    for Rgba([r, g, b, a]) in out.pixels_mut() {
        if *a == 0 {
            [*r, *g, *b] = [0, 0, 0];
        } else if *a < u8::MAX {
            let undo = |c: u8| ((u32::from(c) * 255 + u32::from(*a) / 2) / u32::from(*a)).min(255) as u8;
            [*r, *g, *b] = [undo(*r), undo(*g), undo(*b)];
        }
    }
    out
}

/// The shot over `backdrop`, scaled to cover the window and blurred when the window is
/// see-through. An opaque window clears to its colour first, so nothing behind it shows.
pub fn over_backdrop(shot: &Shot, backdrop: &RgbaImage, scale: f32) -> RgbaImage {
    let (w, h) = shot.pixels.dimensions();
    let under = if shot.see_through {
        let covered = cover(backdrop, w, h).unwrap_or_else(|| RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 255])));
        if scale > 0.0 { image::imageops::fast_blur(&covered, BLUR * scale) } else { covered }
    } else {
        RgbaImage::from_pixel(w, h, Rgba(shot.clear))
    };
    over(&shot.pixels, &under)
}

/// `top` (premultiplied) over `under` (opaque).
pub fn over(top: &RgbaImage, under: &RgbaImage) -> RgbaImage {
    let mut out = under.clone();
    for (dst, src) in out.pixels_mut().zip(top.pixels()) {
        let keep = 255 - u32::from(src[3]);
        for c in 0..3 {
            dst[c] = (u32::from(src[c]) + (u32::from(dst[c]) * keep + 127) / 255).min(255) as u8;
        }
        dst[3] = 255;
    }
    out
}

/// `image` scaled to cover `w` × `h` and cropped to its centre, as a desktop picture fills it. The
/// part that shows is cut out first, so the work is the window's size however thin the picture
/// is, and it is laid over black so that its transparent pixels show nothing of their colour.
fn cover(image: &RgbaImage, w: u32, h: u32) -> Option<RgbaImage> {
    let (iw, ih) = image.dimensions();
    if iw == 0 || ih == 0 || w == 0 || h == 0 {
        return None;
    }
    // The largest centred part of the image with the window's proportions.
    let (cw, ch) = if u64::from(iw) * u64::from(h) >= u64::from(ih) * u64::from(w) {
        (((u64::from(ih) * u64::from(w)) / u64::from(h)).clamp(1, u64::from(iw)) as u32, ih)
    } else {
        (iw, ((u64::from(iw) * u64::from(h)) / u64::from(w)).clamp(1, u64::from(ih)) as u32)
    };
    let mut part = image::imageops::crop_imm(image, (iw - cw) / 2, (ih - ch) / 2, cw, ch).to_image();
    for pixel in part.pixels_mut() {
        let a = u32::from(pixel[3]);
        for c in 0..3 {
            pixel[c] = ((u32::from(pixel[c]) * a + 127) / 255) as u8;
        }
        pixel[3] = 255;
    }
    Some(image::imageops::resize(&part, w, h, image::imageops::FilterType::Triangle))
}

/// A backdrop for when none is given: bright diagonal bands and a fine checker, so a hole in the
/// window and the blur behind it both show.
pub fn default_backdrop() -> RgbaImage {
    const COLOURS: [[u8; 3]; 4] = [[236, 92, 72], [250, 204, 72], [64, 180, 120], [72, 132, 236]];
    RgbaImage::from_fn(1280, 900, |x, y| {
        let band = COLOURS[((x + y) / 160 % 4) as usize];
        let checker = (x / 8 + y / 8) % 2 == 0;
        let [r, g, b] = band.map(|c| if checker { c } else { c.saturating_sub(40) });
        Rgba([r, g, b, 255])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_half_transparent_white_over_black_is_mid_grey_and_unpremultiplies_to_white() {
        let top = RgbaImage::from_pixel(1, 1, Rgba([128, 128, 128, 128]));
        let under = RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255]));
        assert_eq!(over(&top, &under).get_pixel(0, 0).0, [128, 128, 128, 255]);
        assert_eq!(unpremultiply(&top).get_pixel(0, 0).0, [255, 255, 255, 128]);
    }

    #[test]
    fn a_thin_backdrop_covers_the_window_from_its_centre() {
        let thin =
            RgbaImage::from_fn(
                1,
                10_000,
                |_, y| if y < 5_000 { Rgba([255, 0, 0, 255]) } else { Rgba([0, 0, 255, 255]) },
            );
        let covered = cover(&thin, 8, 8).unwrap();
        assert_eq!(covered.dimensions(), (8, 8));
        // Only the picture's middle shows, at the seam between red and blue.
        let [r, _, b, a] = covered.get_pixel(4, 4).0;
        assert!(a == 255 && (r > 0 || b > 0));
    }

    #[test]
    fn transparent_backdrop_pixels_show_black_not_their_colour() {
        let clear = RgbaImage::from_pixel(4, 4, Rgba([0, 255, 0, 0]));
        assert_eq!(cover(&clear, 4, 4).unwrap().get_pixel(1, 1).0, [0, 0, 0, 255]);
        let half = RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 128]));
        let [r, g, b, a] = cover(&half, 4, 4).unwrap().get_pixel(1, 1).0;
        assert!(a == 255 && r < 110 && g < 55 && b < 30, "{r} {g} {b}");
    }

    #[test]
    fn a_hole_shows_what_is_under_it_and_an_opaque_window_shows_only_its_clear() {
        let mut pixels = RgbaImage::from_pixel(4, 4, Rgba([10, 20, 30, 255]));
        pixels.put_pixel(1, 1, Rgba([0, 0, 0, 0]));
        let backdrop = RgbaImage::from_pixel(4, 4, Rgba([200, 100, 50, 255]));
        let see_through = Shot { pixels: pixels.clone(), see_through: true, clear: [0, 0, 0, 255] };
        assert_eq!(over_backdrop(&see_through, &backdrop, 0.0).get_pixel(1, 1).0, [200, 100, 50, 255]);
        assert_eq!(over_backdrop(&see_through, &backdrop, 0.0).get_pixel(0, 0).0, [10, 20, 30, 255]);
        let opaque = Shot { pixels, see_through: false, clear: [5, 5, 5, 255] };
        assert_eq!(over_backdrop(&opaque, &backdrop, 0.0).get_pixel(1, 1).0, [5, 5, 5, 255]);
    }
}
