// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `--outline` (#1582): every element of a scene's last frame drawn over the render as a
//! rectangle round its padding box, with its name (`button#play`) in its top left corner, so a
//! theme author sees which element is which. Drawn into the picture after the frame, never by the
//! window, so the frame inspected is the one rendered. Elements are coloured by depth, and a name
//! is cut to its box's width.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use image::{Rgba, RgbaImage};
use suhina_ui::kit::native_css::inspector::FrameElement;

/// Colours by depth, distinct on light and dark windows.
const COLOURS: [[u8; 3]; 6] =
    [[255, 64, 129], [0, 200, 255], [255, 196, 0], [120, 220, 80], [190, 120, 255], [255, 120, 40]];

/// The names' size in points.
const TEXT: f32 = 9.0;

/// `image` (the window at `scale` pixels a point) with `elements`' boxes and names drawn on it.
pub fn draw(image: &RgbaImage, elements: &[FrameElement], scale: f32) -> Result<RgbaImage, String> {
    let font = FontRef::try_from_slice(epaint_default_fonts::HACK_REGULAR).map_err(|e| e.to_string())?;
    let mut out = image.clone();
    let depths = depths(elements);
    for (element, depth) in elements.iter().zip(depths) {
        let Some((rect, _)) = element.painted else { continue };
        if !rect.is_positive() {
            continue;
        }
        let colour = COLOURS.get(depth % COLOURS.len()).copied().unwrap_or([255, 255, 255]);
        let [x0, y0, x1, y1] = [rect.min.x, rect.min.y, rect.max.x, rect.max.y].map(|v| (v * scale).round() as i64);
        frame(&mut out, [x0, y0, x1 - 1, y1 - 1], colour);
        let label = match &element.id {
            Some(id) => format!("{}#{id}", element.name),
            None => element.name.to_owned(),
        };
        text(&mut out, &font, &label, [x0 + 1, y0 + 1], x1 - x0 - 2, TEXT * scale, colour);
    }
    Ok(out)
}

/// Each element's depth in the tree.
fn depths(elements: &[FrameElement]) -> Vec<usize> {
    let mut depths: Vec<usize> = Vec::with_capacity(elements.len());
    for element in elements {
        // A parent comes before its children.
        let depth = element.parent.and_then(|parent| depths.get(parent)).map_or(0, |depth| depth + 1);
        depths.push(depth);
    }
    depths
}

fn put(image: &mut RgbaImage, x: i64, y: i64, colour: [u8; 3], alpha: f32) {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else { return };
    if x >= image.width() || y >= image.height() {
        return;
    }
    let pixel = image.get_pixel_mut(x, y);
    let a = alpha.clamp(0.0, 1.0);
    for (channel, value) in pixel.0.iter_mut().zip(colour) {
        *channel = (f32::from(*channel) * (1.0 - a) + f32::from(value) * a).round() as u8;
    }
    pixel.0[3] = pixel.0[3].max((a * 255.0) as u8);
}

/// A one-pixel rectangle through the corners `[x0, y0, x1, y1]`, inclusive.
fn frame(image: &mut RgbaImage, [x0, y0, x1, y1]: [i64; 4], colour: [u8; 3]) {
    for x in x0..=x1 {
        put(image, x, y0, colour, 1.0);
        put(image, x, y1, colour, 1.0);
    }
    for y in y0..=y1 {
        put(image, x0, y, colour, 1.0);
        put(image, x1, y, colour, 1.0);
    }
}

/// `label` at `at` (its top left), on a dark plate, cut to `width` pixels; nothing when not even
/// one letter fits.
fn text(image: &mut RgbaImage, font: &FontRef<'_>, label: &str, at: [i64; 2], width: i64, size: f32, colour: [u8; 3]) {
    let scaled = font.as_scaled(PxScale::from(size));
    let ascent = scaled.ascent();
    let height = (scaled.height().ceil() as i64).max(1);
    let mut glyphs = Vec::new();
    let mut x = 0.0f32;
    for c in label.chars() {
        let id = scaled.glyph_id(c);
        let advance = scaled.h_advance(id);
        if (x + advance).ceil() as i64 > width {
            break;
        }
        glyphs.push(id.with_scale_and_position(PxScale::from(size), point(x, ascent)));
        x += advance;
    }
    if glyphs.is_empty() {
        return;
    }
    for py in 0..height {
        for px in 0..(x.ceil() as i64) {
            put(image, at[0] + px, at[1] + py, [0, 0, 0], 0.7);
        }
    }
    for glyph in glyphs {
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, coverage| {
                let x = at[0] + bounds.min.x as i64 + i64::from(gx);
                let y = at[1] + bounds.min.y as i64 + i64::from(gy);
                put(image, x, y, colour, coverage);
            });
        }
    }
}

/// Whether two pictures differ anywhere: what the outline test checks.
pub fn differs(a: &RgbaImage, b: &RgbaImage) -> bool {
    a.dimensions() != b.dimensions() || a.pixels().zip(b.pixels()).any(|(p, q): (&Rgba<u8>, &Rgba<u8>)| p != q)
}
