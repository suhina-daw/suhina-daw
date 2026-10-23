// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Suhina's logo (`assets/brand/suhina-logo.svg`), drawn by the app itself: the file's paths are
//! filled with tiny-skia into a texture of the size asked for, so it stays sharp at any zoom.
//!
//! Only what the file uses is read: its `viewBox`, one `matrix(...)` transform for every path,
//! and paths of absolute `M`, `L`, `C` and `Z` commands, filled even-odd. A white path is the
//! name, drawn in the colour asked for (the theme's text colour); any other fill (the wave) keeps
//! its own colour.

use egui::{
    Align2, Color32, ColorImage, Context, FontId, Id, Painter, Pos2, Rect, Sense, TextureHandle, TextureOptions, Ui,
};

use crate::kit::{theme, widgets};
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Transform};

const SVG: &str = include_str!("../../../../assets/brand/suhina-logo.svg");

/// The largest texture side drawn, in pixels.
const MAX_SIDE: u32 = 4096;

/// The logo's paths and their place, read from the file once per texture.
struct Logo {
    /// The viewBox's width and height.
    size: [f32; 2],
    transform: Transform,
    /// Each path with its own colour, or `None` for the name's colour.
    shapes: Vec<(Path, Option<[u8; 3]>)>,
}

/// Width over height of the logo, from its viewBox.
pub fn aspect() -> f32 {
    parse(SVG).map_or(1.0, |logo| logo.size[0] / logo.size[1].max(1.0))
}

/// The logo as a texture `width` × `height` pixels, the name in `name`; made once per size and
/// colour and kept in the context. `None` when it cannot be drawn.
pub fn texture(ctx: &Context, width: u32, height: u32, name: Color32) -> Option<TextureHandle> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return None;
    }
    let id = Id::new(("suhina-logo", width, height, name.to_array()));
    if let Some(texture) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(texture);
    }
    let image = render(SVG, width, height, name)?;
    let texture = ctx.load_texture("suhina-logo", image, TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    Some(texture)
}

/// Paints the logo into `rect`, the name in the theme's text colour, all of it `opacity` opaque; the
/// name as text in `font` when it cannot be drawn. Screen readers read it as "Suhina".
pub fn paint(ui: &Ui, painter: &Painter, rect: Rect, font: FontId, opacity: f32) {
    let pixels = (rect.size() * ui.ctx().pixels_per_point()).round();
    match texture(ui.ctx(), pixels.x as u32, pixels.y as u32, theme::text_primary()) {
        Some(logo) => {
            let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
            painter.image(logo.id(), rect, uv, Color32::WHITE.gamma_multiply(opacity));
        }
        None => widgets::text_in(
            painter,
            rect,
            Align2::CENTER_CENTER,
            "Suhina",
            font,
            theme::text_primary().gamma_multiply(opacity),
        ),
    }
    let response = ui.interact(rect, Id::new(("suhina-logo", rect.min.x as i32, rect.min.y as i32)), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "Suhina"));
}

fn render(svg: &str, width: u32, height: u32, name: Color32) -> Option<ColorImage> {
    let logo = parse(svg)?;
    let mut pixmap = Pixmap::new(width, height)?;
    let scale = (width as f32 / logo.size[0]).min(height as f32 / logo.size[1]);
    let transform = Transform::from_scale(scale, scale).pre_concat(logo.transform);
    for (path, colour) in &logo.shapes {
        let mut paint = Paint { anti_alias: true, ..Paint::default() };
        match colour {
            Some([r, g, b]) => paint.set_color_rgba8(*r, *g, *b, 255),
            None => paint.set_color_rgba8(name.r(), name.g(), name.b(), name.a()),
        }
        pixmap.fill_path(path, &paint, FillRule::EvenOdd, transform, None);
    }
    let size = [usize::try_from(width).ok()?, usize::try_from(height).ok()?];
    Some(ColorImage::from_rgba_premultiplied(size, pixmap.data()))
}

fn parse(svg: &str) -> Option<Logo> {
    let view: Vec<f32> = numbers(attribute(svg, "viewBox")?);
    let [_, _, width, height] = view.as_slice() else { return None };
    if !(*width > 0.0 && *height > 0.0) {
        return None;
    }
    let transform = match svg.find("matrix(") {
        Some(at) => {
            let rest = svg.get(at + "matrix(".len()..)?;
            let inside = rest.get(..rest.find(')')?)?;
            let values = numbers(inside);
            let [a, b, c, d, e, f] = values.as_slice() else { return None };
            Transform::from_row(*a, *b, *c, *d, *e, *f)
        }
        None => Transform::identity(),
    };
    let mut shapes = Vec::new();
    for element in svg.split("<path").skip(1) {
        let element = element.get(..element.find('>')?)?;
        let path = path(attribute(element, "d")?)?;
        let fill = attribute(element, "style")
            .and_then(|style| style.split(';').find_map(|part| part.trim().strip_prefix("fill:")))
            .or_else(|| attribute(element, "fill"))
            .unwrap_or("white");
        shapes.push((path, colour(fill)?));
    }
    (!shapes.is_empty()).then_some(Logo { size: [*width, *height], transform, shapes })
}

/// The value of `name="..."` in `text`.
fn attribute<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let start = text.find(&key)? + key.len();
    let rest = text.get(start..)?;
    rest.get(..rest.find('"')?)
}

/// `None` inside for white (the name's colour), else the colour; `None` outside when unknown.
fn colour(fill: &str) -> Option<Option<[u8; 3]>> {
    let fill = fill.trim();
    if fill.eq_ignore_ascii_case("white") || fill.eq_ignore_ascii_case("#ffffff") {
        return Some(None);
    }
    if let Some(inside) = fill.strip_prefix("rgb(").and_then(|f| f.strip_suffix(')')) {
        let parts: Vec<u8> = inside.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        let [r, g, b] = parts.as_slice() else { return None };
        return Some(Some([*r, *g, *b]));
    }
    let hex = fill.strip_prefix('#').filter(|h| h.len() == 6)?;
    let byte = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    Some(Some([byte(0)?, byte(2)?, byte(4)?]))
}

/// The numbers in `text`, separated by spaces, commas or a sign.
fn numbers(text: &str) -> Vec<f32> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        let sign_starts = (c == '-' || c == '+') && !current.is_empty() && !current.ends_with(['e', 'E']);
        if c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '-' || c == '+' {
            if sign_starts {
                out.extend(current.parse::<f32>().ok());
                current.clear();
            }
            current.push(c);
        } else {
            out.extend(current.parse::<f32>().ok());
            current.clear();
        }
    }
    out.extend(current.parse::<f32>().ok());
    out
}

/// A path from SVG path data of absolute `M`, `L`, `C` and `Z` commands.
fn path(d: &str) -> Option<Path> {
    let mut builder = PathBuilder::new();
    // Split before each command letter: "M1,2C3,4 5,6 7,8Z" gives "M1,2", "C3,4 5,6 7,8", "Z".
    let mut start = 0;
    let mut pieces = Vec::new();
    for (i, c) in d.char_indices() {
        if c.is_ascii_alphabetic() && c != 'e' && c != 'E' && i > start {
            pieces.push(d.get(start..i)?);
            start = i;
        }
    }
    pieces.push(d.get(start..)?);
    for piece in pieces {
        let piece = piece.trim();
        let command = piece.chars().next()?;
        let values = numbers(piece.get(1..)?);
        match command {
            'M' | 'L' => {
                for (n, [x, y]) in values.as_chunks::<2>().0.iter().enumerate() {
                    if command == 'M' && n == 0 {
                        builder.move_to(*x, *y);
                    } else {
                        builder.line_to(*x, *y);
                    }
                }
            }
            'C' => {
                for [x1, y1, x2, y2, x, y] in values.as_chunks::<6>().0 {
                    builder.cubic_to(*x1, *y1, *x2, *y2, *x, *y);
                }
            }
            'Z' | 'z' => builder.close(),
            _ => return None,
        }
    }
    builder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logo_file_is_read_whole() {
        let logo = parse(SVG).unwrap();
        assert_eq!(logo.size, [12525.0, 3663.0]);
        // Six letters with the dot of the i, and the wave in its own colour.
        assert_eq!(logo.shapes.len(), 8);
        assert_eq!(logo.shapes.iter().filter(|(_, c)| *c == Some([255, 217, 125])).count(), 1);
        assert!((aspect() - 12525.0 / 3663.0).abs() < 1e-3);
    }

    #[test]
    fn it_draws_the_name_in_the_colour_asked_for_and_the_wave_in_its_own() {
        let image = render(SVG, 400, 117, Color32::from_rgb(10, 20, 30)).unwrap();
        assert_eq!(image.size, [400, 117]);
        let drawn: Vec<Color32> = image.pixels.iter().copied().filter(|p| p.a() == 255).collect();
        assert!(drawn.contains(&Color32::from_rgb(10, 20, 30)), "the name");
        assert!(drawn.contains(&Color32::from_rgb(255, 217, 125)), "the wave");
        // Most of the box is empty: letters, not a filled rectangle.
        assert!(drawn.len() < image.pixels.len() / 2);
    }

    #[test]
    fn numbers_and_paths_are_read_and_bad_input_is_refused() {
        assert_eq!(numbers("1,2 -3.5-4e1 5"), vec![1.0, 2.0, -3.5, -40.0, 5.0]);
        assert!(path("M0,0L10,0 10,10Z").is_some());
        assert!(path("M0,0C1,1 2,2 3,3Z").is_some());
        assert!(path("M0,0Q1,1 2,2Z").is_none(), "commands it does not read");
        assert!(path("").is_none());
        for bad in ["", "<svg>", "<svg viewBox=\"0 0 0 10\"><path d=\"M0,0L1,1Z\"/></svg>"] {
            assert!(parse(bad).is_none(), "{bad}");
        }
        assert!(parse("<svg viewBox=\"0 0 10 10\"><path d=\"M0,0L10,0 10,10Z\" fill=\"#ff0000\"/></svg>").is_some());
        assert_eq!(colour("tomato"), None);
        assert_eq!(colour("#FFD97D"), Some(Some([255, 217, 125])));
        assert!(render(SVG, 0, 10, Color32::WHITE).is_none());
        assert!(texture(&Context::default(), MAX_SIDE + 1, 10, Color32::WHITE).is_none());
    }
}
