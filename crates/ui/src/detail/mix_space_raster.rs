// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Mix Space view's landscape drawn pixel by pixel (`docs/spec/mix-space.md`): triangles
//! with a depth buffer, light worked out at every pixel from the interpolated surface normal,
//! colours blended smoothly across each triangle, and a fine net of grid lines on the surface
//! that stays sharp at any distance because its width is measured in pixels. The result is an
//! image the panel shows as a texture, so it looks the same whichever way the window is drawn
//! (wgpu or OpenGL) and in tests.

use egui::{Color32, ColorImage};

/// One corner of the surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corner {
    /// Where it lands in the image, in pixels.
    pub at: [f32; 2],
    /// How far it is from the camera: smaller is nearer.
    pub depth: f32,
    /// The surface's normal there, in the box's space.
    pub normal: [f32; 3],
    /// Its colour (0 to 1) and opacity.
    pub color: [f32; 4],
    /// Its place on the grid of lines: a line falls at every whole number.
    pub grid: [f32; 2],
}

/// How the surface is lit and lined.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// The direction the light comes from, in the box's space, normalised.
    pub light: [f32; 3],
    /// How strongly the grid lines show: 0 none, 1 full.
    pub lines: f32,
    /// How wide a grid line is, in pixels.
    pub line_width: f32,
}

/// Draws `triangles` (indices into `corners`) into an image `width` by `height` pixels, clear
/// where there is no surface. Everything outside the image is clipped; triangles may come in
/// any order.
pub fn render(width: usize, height: usize, corners: &[Corner], triangles: &[[u32; 3]], look: &Look) -> ColorImage {
    let (width, height) = (width.clamp(1, 4096), height.clamp(1, 4096));
    let mut depth = vec![f32::INFINITY; width * height];
    let mut color = vec![[0.0f32; 4]; width * height];
    for triangle in triangles {
        let [Some(a), Some(b), Some(c)] = triangle.map(|i| corners.get(i as usize)) else { continue };
        if a.color[3] < 0.01 && b.color[3] < 0.01 && c.color[3] < 0.01 {
            continue;
        }
        fill(width, height, &mut depth, &mut color, [a, b, c], look);
    }
    let pixels = color
        .iter()
        .map(|[r, g, b, a]| {
            let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            Color32::from_rgba_premultiplied(channel(*r), channel(*g), channel(*b), channel(*a))
        })
        .collect();
    ColorImage::new([width, height], pixels)
}

/// Fills one triangle: every pixel whose centre is inside it and nearer than what is there.
fn fill(width: usize, height: usize, depth: &mut [f32], color: &mut [[f32; 4]], t: [&Corner; 3], look: &Look) {
    let [a, b, c] = t;
    let area = edge(a.at, b.at, c.at);
    if area.abs() < 1e-6 || !area.is_finite() {
        return;
    }
    let min_x = a.at[0].min(b.at[0]).min(c.at[0]).floor().max(0.0) as usize;
    let max_x = (a.at[0].max(b.at[0]).max(c.at[0]).ceil().max(0.0) as usize).min(width.saturating_sub(1));
    let min_y = a.at[1].min(b.at[1]).min(c.at[1]).floor().max(0.0) as usize;
    let max_y = (a.at[1].max(b.at[1]).max(c.at[1]).ceil().max(0.0) as usize).min(height.saturating_sub(1));
    if min_x > max_x || min_y > max_y {
        return;
    }
    // The grid's change per pixel is the same all over a flat triangle: its width in pixels.
    let gradient = |k: usize| {
        let (ga, gb, gc) = (a.grid[k], b.grid[k], c.grid[k]);
        let dx = ((gb - ga) * (c.at[1] - a.at[1]) - (gc - ga) * (b.at[1] - a.at[1])) / area;
        let dy = ((gc - ga) * (b.at[0] - a.at[0]) - (gb - ga) * (c.at[0] - a.at[0])) / area;
        (dx.abs() + dy.abs()).max(1e-4)
    };
    let fwidth = [gradient(0), gradient(1)];
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            let (w0, w1, w2) = (edge(b.at, c.at, p) / area, edge(c.at, a.at, p) / area, edge(a.at, b.at, p) / area);
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let index = y * width + x;
            let z = w0 * a.depth + w1 * b.depth + w2 * c.depth;
            let Some(nearest) = depth.get_mut(index) else { continue };
            if z >= *nearest {
                continue;
            }
            let mix = |f: fn(&Corner) -> f32| w0 * f(a) + w1 * f(b) + w2 * f(c);
            let alpha = mix(|v| v.color[3]).clamp(0.0, 1.0);
            if alpha < 0.01 {
                continue;
            }
            *nearest = z;
            let normal = normalise([mix(|v| v.normal[0]), mix(|v| v.normal[1]), mix(|v| v.normal[2])]);
            let lit = normal[0] * look.light[0] + normal[1] * look.light[1] + normal[2] * look.light[2];
            // A soft light from above and a faint fill from below, so no slope goes flat black.
            let shade = 0.28 + 0.72 * lit.max(0.0) + 0.12 * (-lit).max(0.0);
            let base = [mix(|v| v.color[0]), mix(|v| v.color[1]), mix(|v| v.color[2])];
            let line = [0, 1]
                .map(|k| {
                    let g = w0 * a.grid[k] + w1 * b.grid[k] + w2 * c.grid[k];
                    let distance = ((g - g.round()).abs() / fwidth[k]) / look.line_width.max(0.1);
                    (1.0 - distance).clamp(0.0, 1.0)
                })
                .into_iter()
                .fold(0.0f32, f32::max)
                * look.lines;
            let mut out = [0.0f32; 4];
            for (i, value) in base.iter().enumerate() {
                let surface = value * shade;
                let edge = (value * 1.35 + 0.12).min(1.0);
                if let Some(slot) = out.get_mut(i) {
                    *slot = (surface + (edge - surface) * line) * alpha;
                }
            }
            out[3] = alpha;
            if let Some(slot) = color.get_mut(index) {
                *slot = out;
            }
        }
    }
}

fn edge(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

fn normalise(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length > 1e-6 { [v[0] / length, v[1] / length, v[2] / length] } else { [0.0, 1.0, 0.0] }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corner(at: [f32; 2], depth: f32, color: [f32; 4]) -> Corner {
        Corner { at, depth, normal: [0.0, 1.0, 0.0], color, grid: [0.5, 0.5] }
    }

    const LOOK: Look = Look { light: [0.0, 1.0, 0.0], lines: 0.0, line_width: 1.0 };

    #[test]
    fn a_triangle_fills_its_pixels_and_the_nearer_one_wins() {
        let red = [1.0, 0.0, 0.0, 1.0];
        let blue = [0.0, 0.0, 1.0, 1.0];
        let corners = [
            corner([0.0, 0.0], 1.0, red),
            corner([10.0, 0.0], 1.0, red),
            corner([0.0, 10.0], 1.0, red),
            corner([0.0, 0.0], 0.5, blue),
            corner([10.0, 0.0], 0.5, blue),
            corner([0.0, 10.0], 0.5, blue),
        ];
        let image = render(10, 10, &corners, &[[0, 1, 2], [3, 4, 5]], &LOOK);
        let at = |x: usize, y: usize| image.pixels[y * 10 + x];
        assert_eq!(at(2, 2), Color32::from_rgb(0, 0, 255), "the nearer, blue one");
        assert_eq!(at(9, 9), Color32::TRANSPARENT, "outside the triangle");
        let reversed = render(10, 10, &corners, &[[3, 4, 5], [0, 1, 2]], &LOOK);
        assert_eq!(reversed.pixels[2 * 10 + 2], Color32::from_rgb(0, 0, 255), "in any order");
    }

    #[test]
    fn grid_lines_brighten_the_surface_and_hostile_input_is_safe() {
        let grey = [0.4, 0.4, 0.4, 1.0];
        let mut corners =
            [corner([0.0, 0.0], 1.0, grey), corner([20.0, 0.0], 1.0, grey), corner([0.0, 20.0], 1.0, grey)];
        corners[0].grid = [0.0, 0.0];
        corners[1].grid = [2.0, 0.0];
        corners[2].grid = [0.0, 2.0];
        let lined = Look { lines: 1.0, ..LOOK };
        let image = render(20, 20, &corners, &[[0, 1, 2]], &lined);
        let on_line = image.pixels[3 * 20 + 10];
        let between = image.pixels[3 * 20 + 5];
        assert!(on_line.r() > between.r(), "{on_line:?} {between:?}");
        let nan = Corner { at: [f32::NAN, 0.0], ..corners[0] };
        let _ = render(20, 20, &[nan, corners[1], corners[2]], &[[0, 1, 2], [0, 9, 1]], &lined);
        let _ = render(0, usize::MAX, &corners, &[[0, 1, 2]], &lined);
    }
}
