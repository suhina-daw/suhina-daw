// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Steps view: 36 hues, or any hue, each with a 7 x 7 grid of its tints, tones and shades.
//!
//! Colors are laid out in Oklab with its lightness replaced by Lr, Björn Ottosson's
//! toe-corrected lightness (an estimate of CIELAB L*), so the gray row steps evenly to the eye
//! where Oklab's own lightness crowds the darks.

use super::Hsv;
use egui::{Sense, Ui, Vec2};
use std::sync::OnceLock;
use suhina_engine::model::Rgb;

/// How many hues Steps offers: six 60° sectors, each divided in six.
pub(super) const HUES: usize = 36;
const PER_SECTOR: usize = HUES / 6;
/// How far into its sector, in steps, a sector's first hue sits. Starting a little past the
/// primaries and secondaries puts steps near the palette's colors that sit between them, such as
/// its orange.
const OFFSET: f32 = 0.375;
/// The grid's rows and columns.
pub(super) const ROWS: usize = 7;
pub(super) const COLUMNS: usize = 7;
/// The middle column, where the hue's full color tops it, and the middle row.
const MID: usize = (COLUMNS - 1) / 2;
const MID_ROW: usize = (ROWS - 1) / 2;
/// The grid's swatches, row by row from the top.
pub(super) const CELLS: usize = ROWS * COLUMNS;
/// The size of the grid.
const SIZE: Vec2 = Vec2::new(154.0, 110.0);
/// The room between swatches.
const GAP: f32 = 2.0;

/// Encoded sRGB (0..1) to linear light.
fn linear(v: f32) -> f32 {
    if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// Linear light to encoded sRGB (0..1).
fn encoded(v: f32) -> f32 {
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

/// Encoded sRGB (0..1) to Oklab.
pub(super) fn oklab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = [linear(r), linear(g), linear(b)];
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// Oklab to linear sRGB, unclipped.
fn linear_from_oklab([l, a, b]: [f32; 3]) -> [f32; 3] {
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_,
        -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_,
        -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_,
    ]
}

const K1: f32 = 0.206;
const K2: f32 = 0.03;
const K3: f32 = (1.0 + K1) / (1.0 + K2);

/// Oklab lightness to Lr.
fn toe(x: f32) -> f32 {
    0.5 * (K3 * x - K1 + ((K3 * x - K1).powi(2) + 4.0 * K2 * K3 * x).sqrt())
}

/// Lr to Oklab lightness.
fn toe_inverse(x: f32) -> f32 {
    (x * x + K1 * x) / (K3 * (x + K2))
}

/// HSV hue `hue` (0..1) at full saturation and value, as (Lr, a, b).
fn full(hue: f32) -> [f32; 3] {
    let [l, a, b] = oklab(egui::ecolor::rgb_from_hsv((hue.rem_euclid(1.0), 1.0, 1.0)));
    [toe(l), a, b]
}

/// (Lr, a, b) to sRGB bytes, each channel clipped to the gamut.
fn srgb([lr, a, b]: [f32; 3]) -> Rgb {
    let [r, g, b] = linear_from_oklab([toe_inverse(lr.clamp(0.0, 1.0)), a, b])
        .map(|v| (encoded(v.clamp(0.0, 1.0)) * 255.0).round().clamp(0.0, 255.0) as u8);
    Rgb(r, g, b)
}

/// The HSV hues (0..1) Steps offers. Each 60° sector between a primary and a secondary is cut into
/// six steps of equal distance along the path its full colors take in (Lr, a, b), so
/// neighboring steps look about as far apart, each [`OFFSET`] of a step into it.
pub(crate) fn hues() -> &'static [f32; HUES] {
    static HUES_TABLE: OnceLock<[f32; HUES]> = OnceLock::new();
    HUES_TABLE.get_or_init(|| {
        const SAMPLES: usize = 256;
        let mut table = [0.0; HUES];
        for sector in 0..6 {
            let low = sector as f32 / 6.0;
            let at = |i: usize| low + i as f32 / (SAMPLES as f32 * 6.0);
            // The path's length up to each sample.
            let mut lengths = [0.0_f32; SAMPLES + 1];
            let mut previous = full(low);
            for i in 1..=SAMPLES {
                let here = full(at(i));
                let step = here.iter().zip(previous).map(|(x, y)| (x - y).powi(2)).sum::<f32>().sqrt();
                lengths[i] = lengths[i - 1] + step;
                previous = here;
            }
            let total = lengths[SAMPLES];
            for step in 0..PER_SECTOR {
                let target = total * (step as f32 + OFFSET) / PER_SECTOR as f32;
                let i = lengths.iter().position(|&l| l >= target).unwrap_or(SAMPLES).max(1);
                let (before, after) = (lengths[i - 1], lengths[i]);
                let share = if after > before { (target - before) / (after - before) } else { 0.0 };
                let hue = at(i - 1) + share / (SAMPLES as f32 * 6.0);
                if let Some(slot) = table.get_mut(sector * PER_SECTOR + step) {
                    *slot = hue;
                }
            }
        }
        table
    })
}

/// The step nearest `hue` (0..1), going round the circle.
pub(super) fn nearest(hue: f32) -> usize {
    let distance = |h: f32| {
        let d = (h - hue).rem_euclid(1.0);
        d.min(1.0 - d)
    };
    hues().iter().enumerate().min_by(|a, b| distance(*a.1).total_cmp(&distance(*b.1))).map_or(0, |(index, _)| index)
}

/// The most chroma sRGB holds at Oklab lightness `l` along hue angle (`cos`, `sin`) of the
/// a-b plane.
fn most_chroma(l: f32, cos: f32, sin: f32) -> f32 {
    let inside = |c: f32| linear_from_oklab([l, c * cos, c * sin]).iter().all(|v| (-1e-4..=1.0 + 1e-4).contains(v));
    let (mut low, mut high) = (0.0_f32, 0.5_f32);
    for _ in 0..24 {
        let middle = (low + high) / 2.0;
        if inside(middle) { low = middle } else { high = middle }
    }
    low
}

/// A point of the hue's slice, `[chroma, Lr]`.
type Point = [f32; 2];

fn lerp(a: Point, b: Point, t: f32) -> Point {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

/// The grid for HSV hue `hue`, row by row from the top, laid out in the hue's slice of
/// (Lr, a, b), where the hue at full saturation and value (F), white (W) and black (K) make a
/// triangle. The grid is that triangle warped onto a square, so the vivid part near F gets half
/// of it:
///
/// - F is the top row's middle; the points a quarter of the way from F to white and to black are
///   its corners, so the top row runs evenly along the two edges.
/// - The line half way from F to the grays (parallel to them) is the middle row, and the grays,
///   white to black in even Lr, the bottom row. The side columns follow the edges, and the rows
///   between run evenly from one of these rows to the next.
/// - The cells inside the top corners, off the middle column, are each the average of their four
///   neighbors (relaxed until they settle). Going straight down from a corner would walk back
///   along the edge the top row came out along, and the cells inside it would land on the edge.
///
/// A point's chroma is taken as a share of the triangle's straight edge at its lightness, and that
/// share of the chroma sRGB holds there, so the edges, which run outside sRGB near F, follow the
/// gamut's edge instead of clipping cells together. F is the HSV color exactly.
pub(crate) fn grid(hue: f32) -> [Rgb; CELLS] {
    const CORNER: f32 = 0.25;
    const MIDDLE: f32 = 0.5;
    let [lr, a, b] = full(hue);
    let chroma = a.hypot(b);
    let (cos, sin) = if chroma > 0.0 { (a / chroma, b / chroma) } else { (1.0, 0.0) };
    let (f, white, black): (Point, Point, Point) = ([chroma, lr], [0.0, 1.0], [0.0, 0.0]);
    let (left, right) = (lerp(f, white, CORNER), lerp(f, black, CORNER));
    let (middle_left, middle_right) = (lerp(f, white, MIDDLE), lerp(f, black, MIDDLE));
    let across = |from: Point, to: Point| -> [Point; COLUMNS] {
        std::array::from_fn(|c| lerp(from, to, c as f32 / (COLUMNS - 1) as f32))
    };
    let top: [Point; COLUMNS] = std::array::from_fn(|c| {
        if c <= MID { lerp(left, f, c as f32 / MID as f32) } else { lerp(f, right, (c - MID) as f32 / MID as f32) }
    });
    let middle = across(middle_left, middle_right);
    let bottom = across(white, black);
    let mut rows: [[Point; COLUMNS]; ROWS] = std::array::from_fn(|r| {
        let (from, to, t) = if r <= MID_ROW {
            (&top, &middle, r as f32 / MID_ROW as f32)
        } else {
            (&middle, &bottom, (r - MID_ROW) as f32 / (ROWS - 1 - MID_ROW) as f32)
        };
        std::array::from_fn(|c| lerp(from[c], to[c], t))
    });
    // Gauss–Seidel on the cells inside the top corners; a few dozen passes settle a 7 x 7 grid.
    for _ in 0..64 {
        for r in 1..MID_ROW {
            for c in (1..COLUMNS - 1).filter(|&c| c != MID) {
                let around = [rows[r - 1][c], rows[r + 1][c], rows[r][c - 1], rows[r][c + 1]];
                rows[r][c] = around.iter().fold([0.0, 0.0], |sum, p| [sum[0] + p[0] / 4.0, sum[1] + p[1] / 4.0]);
            }
        }
    }
    let straight = |l: f32| {
        if l >= lr { chroma * (1.0 - l) / (1.0 - lr).max(1e-6) } else { chroma * l / lr.max(1e-6) }
    };
    let color = |[c, l]: Point| {
        let l = l.clamp(0.0, 1.0);
        let edge = straight(l);
        let c = if edge > 1e-6 { c.max(0.0) / edge * edge.min(most_chroma(toe_inverse(l), cos, sin)) } else { 0.0 };
        srgb([l, c * cos, c * sin])
    };
    let [r, g, b] = Hsv { h: hue, s: 1.0, v: 1.0 }.to_srgb();
    std::array::from_fn(|index| {
        let (row, column) = (index / COLUMNS, index % COLUMNS);
        if (row, column) == (0, MID) {
            return Rgb(r, g, b);
        }
        rows.get(row).and_then(|r| r.get(column)).map_or(Rgb(0, 0, 0), |&p| color(p))
    })
}

/// Draw `cells` as a grid of swatches named "Shade #rrggbb", ringing `current`'s, the size of the
/// Field view's plane. The cell clicked, if any.
pub(super) fn show(ui: &mut Ui, cells: &[Rgb; CELLS], current: Rgb) -> Option<usize> {
    let mut picked = None;
    let swatch = Vec2::new(
        (SIZE.x - GAP * (COLUMNS - 1) as f32) / COLUMNS as f32,
        (SIZE.y - GAP * (ROWS - 1) as f32) / ROWS as f32,
    );
    let pitch = swatch + Vec2::splat(GAP);
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(pitch.x * COLUMNS as f32 - GAP, pitch.y * ROWS as f32 - GAP), Sense::hover());
    let ids: Vec<egui::Id> = (0..CELLS).map(|index| ui.scope_id().with(("color-step", index))).collect();
    let roving = super::super::Roving::new(ui, &ids, COLUMNS, cells.iter().position(|c| *c == current));
    let mut focused = None;
    for (index, (&color, &id)) in cells.iter().zip(&ids).enumerate() {
        let (row, column) = (index / COLUMNS, index % COLUMNS);
        let cell =
            egui::Rect::from_min_size(rect.min + Vec2::new(column as f32 * pitch.x, row as f32 * pitch.y), swatch);
        let response = ui.interact(cell, id, roving.sense(index, Sense::click()));
        let name = super::super::swatch_name("Shade", color);
        let chosen = color == current;
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), chosen, &name));
        ui.painter().rect_filled(cell, 0.0, super::super::color32(color));
        if chosen || response.hovered() {
            super::super::swatch_ring(ui.painter(), &response, color, chosen);
        }
        if response.has_focus() {
            focused = Some(cell);
        }
        if response.clicked() {
            // The keys follow the click, so the arrows go on from here.
            super::super::focus_by_pointer(&response);
            picked = Some(index);
        }
        super::super::swatch_tooltip(response, color);
    }
    if let Some(cell) = focused {
        super::super::focus_ring(ui, cell);
    }
    if let Some(index) = roving.keys(ui) {
        picked = Some(index);
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hues_step_evenly_between_full_colors_from_part_of_a_step_into_each_sector() {
        let table = hues();
        // The path's length between two hues, measured finely.
        let length = |a: f32, b: f32| {
            let n = 64;
            (0..n)
                .map(|i| {
                    let (x, y) = (a + (b - a) * i as f32 / n as f32, a + (b - a) * (i + 1) as f32 / n as f32);
                    full(x).iter().zip(full(y)).map(|(p, q)| (p - q).powi(2)).sum::<f32>().sqrt()
                })
                .sum::<f32>()
        };
        for sector in 0..6 {
            let low = sector as f32 / 6.0;
            let steps: Vec<f32> = (0..PER_SECTOR - 1)
                .map(|step| {
                    let (here, next) = (table[sector * PER_SECTOR + step], table[sector * PER_SECTOR + step + 1]);
                    assert!(here < next, "sector {sector} step {step} rises");
                    length(here, next)
                })
                .collect();
            let mean = steps.iter().sum::<f32>() / steps.len() as f32;
            for (step, length) in steps.iter().enumerate() {
                assert!((length - mean).abs() < 0.02 * mean, "sector {sector} step {step}: {length} against {mean}");
            }
            let first = length(low, table[sector * PER_SECTOR]);
            assert!(
                (first - OFFSET * mean).abs() < 0.03 * mean,
                "sector {sector} starts {first} in, not {OFFSET} of {mean}"
            );
        }
        for (index, &hue) in table.iter().enumerate() {
            assert_eq!(nearest(hue), index);
        }
        assert_eq!(nearest(0.999), 0, "the circle wraps");
    }

    #[test]
    fn every_palette_color_is_a_steps_swatch_and_a_column_shares_one_hue_step() {
        let grids: Vec<[Rgb; CELLS]> = hues().iter().map(|&hue| grid(hue)).collect();
        let palette = crate::interaction::color_picker::swatches();
        let columns = suhina_session::colors::PALETTE_COLUMNS;
        for column in 0..columns {
            let colors: Vec<Rgb> = palette.iter().skip(column).step_by(columns).copied().collect();
            let shared = grids.iter().position(|cells| colors.iter().all(|color| cells.contains(color)));
            assert!(shared.is_some(), "palette column {column} ({colors:?}) is not one hue step's swatches");
        }
    }

    /// (Lr, a, b) of an sRGB color.
    fn lr_ab(c: Rgb) -> [f32; 3] {
        let [l, a, b] = oklab([c.0, c.1, c.2].map(|v| f32::from(v) / 255.0));
        [toe(l), a, b]
    }

    #[test]
    fn the_grid_has_the_hue_top_middle_and_an_even_gray_row_from_white_to_black() {
        for &hue in hues() {
            let cells = grid(hue);
            let [r, g, b] = Hsv { h: hue, s: 1.0, v: 1.0 }.to_srgb();
            assert_eq!(cells[MID], Rgb(r, g, b));
            let bottom = &cells[CELLS - COLUMNS..];
            assert_eq!(bottom[0], Rgb(255, 255, 255));
            assert_eq!(bottom[COLUMNS - 1], Rgb(0, 0, 0));
            for (i, gray) in bottom.iter().enumerate() {
                assert!(gray.0 == gray.1 && gray.1 == gray.2, "{hue}: {gray:?} is a gray");
                let lr = lr_ab(*gray)[0];
                let want = 1.0 - i as f32 / (COLUMNS - 1) as f32;
                assert!((lr - want).abs() < 0.01, "{hue}: gray {i} at Lr {lr}, not {want}");
            }
        }
    }
}
