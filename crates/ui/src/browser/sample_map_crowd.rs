// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Drawing a map with many points (docs/spec/sample-map.md › Many items): the clusters each kind
//! forms and where their labels go without overlapping, and the density raster drawn instead of
//! the dots when too many are in view. Everything here is plain data in and out, so it is tested
//! without a window.

use egui::{Color32, ColorImage, Pos2, Rect, Vec2};
use suhina_soundmap::kinds::Kind;

/// More points than this in view: the density raster instead of one dot each.
pub(crate) const DENSITY_ABOVE: usize = 20_000;
/// A density cell's side, in screen pixels.
pub(crate) const BIN: f32 = 3.0;
/// The map is divided into at most this many cells a side to find clusters; fewer for fewer
/// items (half the square root of the count, at least 4), so a cluster's cells touch.
const CLUSTER_CELLS: usize = 48;
/// A cluster needs this many items for a label (four times as many for Other)…
const LABEL_MIN_ITEMS: usize = 8;
/// …and one in this many of the map's items, so a large map has a bounded number of labels.
const LABEL_SHARE: usize = 2_000;
/// The largest label, as a multiple of the map's label size.
const LABEL_MAX_SCALE: f32 = 1.7;
/// Room kept between labels, in screen pixels.
const LABEL_GAP: f32 = 2.0;

const KINDS: usize = Kind::ALL.len();

/// Where `kind` is in [`Kind::ALL`].
fn kind_index(kind: Kind) -> usize {
    Kind::ALL.iter().position(|k| *k == kind).unwrap_or(KINDS - 1)
}

/// A group of map cells in which one kind holds a third or more of the points, joined across
/// sides and corners; its label goes at `spot`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Cluster {
    pub kind: Kind,
    /// The cluster's item nearest to its centre, in map coordinates.
    pub spot: [f32; 2],
    pub count: usize,
}

/// The cell of a map divided into `side` by `side` cells a position (0–1) falls in.
fn cell_of(p: [f32; 2], side: usize) -> Option<usize> {
    if !(p[0].is_finite() && p[1].is_finite()) || side == 0 {
        return None;
    }
    let column = ((p[0] * side as f32).floor().max(0.0) as usize).min(side - 1);
    let row = ((p[1] * side as f32).floor().max(0.0) as usize).min(side - 1);
    Some(row * side + column)
}

/// The clusters of the items (`position`, `kind`) that get a label, largest first. Three passes
/// over the items; call it when the points change, not every frame.
pub(crate) fn clusters(items: &[([f32; 2], Kind)]) -> Vec<Cluster> {
    let side = (((items.len() as f32).sqrt() / 2.0) as usize).clamp(4, CLUSTER_CELLS);
    let cells = side * side;
    let mut counts = vec![[0u32; KINDS]; cells];
    for (p, kind) in items {
        if let Some(cell) = cell_of(*p, side)
            && let Some(slot) = counts.get_mut(cell).and_then(|c| c.get_mut(kind_index(*kind)))
        {
            *slot += 1;
        }
    }
    // Per kind, the cells it holds a third of, joined across sides and corners into components.
    let mut component = vec![[u32::MAX; KINDS]; cells];
    let mut found: Vec<(Kind, usize)> = Vec::new();
    for (k, kind) in Kind::ALL.iter().enumerate() {
        let held = |cell: usize| {
            counts.get(cell).is_some_and(|c| {
                let mine = c.get(k).copied().unwrap_or(0);
                let all: u32 = c.iter().sum();
                mine > 0 && mine * 3 >= all
            })
        };
        for start in 0..cells {
            if !held(start) || component.get(start).and_then(|c| c.get(k)).is_some_and(|c| *c != u32::MAX) {
                continue;
            }
            let id = found.len() as u32;
            let mut size = 0usize;
            let mut stack = vec![start];
            while let Some(cell) = stack.pop() {
                let Some(slot) = component.get_mut(cell).and_then(|c| c.get_mut(k)) else { continue };
                if *slot != u32::MAX || !held(cell) {
                    continue;
                }
                *slot = id;
                size += counts.get(cell).and_then(|c| c.get(k)).copied().unwrap_or(0) as usize;
                let (row, column) = (cell / side, cell % side);
                for (dr, dc) in [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)] {
                    let (Some(r), Some(c)) = (row.checked_add_signed(dr), column.checked_add_signed(dc)) else {
                        continue;
                    };
                    if r < side && c < side {
                        stack.push(r * side + c);
                    }
                }
            }
            found.push((*kind, size));
        }
    }
    let at_least = (items.len() / LABEL_SHARE).max(1);
    let keep: Vec<bool> = found
        .iter()
        .map(|(kind, size)| {
            let floor = if *kind == Kind::Other { LABEL_MIN_ITEMS * 4 } else { LABEL_MIN_ITEMS };
            *size >= floor.max(at_least)
        })
        .collect();
    // The centre of each kept cluster, then the item nearest to it.
    let component_of = |p: [f32; 2], kind: Kind| -> Option<usize> {
        let id = *component.get(cell_of(p, side)?)?.get(kind_index(kind))?;
        let id = usize::try_from(id).ok()?;
        keep.get(id).copied().unwrap_or(false).then_some(id)
    };
    let mut sums = vec![[0.0f64; 2]; found.len()];
    for (p, kind) in items {
        if let Some(sum) = component_of(*p, *kind).and_then(|id| sums.get_mut(id)) {
            sum[0] += f64::from(p[0]);
            sum[1] += f64::from(p[1]);
        }
    }
    let centres: Vec<[f32; 2]> = sums
        .iter()
        .zip(&found)
        .map(|(s, (_, n))| {
            let n = (*n).max(1) as f64;
            [(s[0] / n) as f32, (s[1] / n) as f32]
        })
        .collect();
    let mut nearest: Vec<Option<([f32; 2], f32)>> = vec![None; found.len()];
    for (p, kind) in items {
        let Some(id) = component_of(*p, *kind) else { continue };
        let (Some(centre), Some(best)) = (centres.get(id), nearest.get_mut(id)) else { continue };
        let d = (p[0] - centre[0]).powi(2) + (p[1] - centre[1]).powi(2);
        if best.is_none_or(|(_, b)| d < b) {
            *best = Some((*p, d));
        }
    }
    let mut out: Vec<Cluster> = found
        .iter()
        .zip(&nearest)
        .zip(&keep)
        .filter(|(_, keep)| **keep)
        .filter_map(|(((kind, count), nearest), _)| Some(Cluster { kind: *kind, spot: (*nearest)?.0, count: *count }))
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(kind_index(a.kind).cmp(&kind_index(b.kind))));
    out
}

/// A label's size for a cluster of `count` items, as a multiple of the map's label size: it
/// grows with the logarithm of the count, from 1 at the smallest cluster with a label.
pub(crate) fn label_scale(count: usize, smallest: usize) -> f32 {
    let ratio = count.max(1) as f32 / smallest.max(1) as f32;
    (1.0 + 0.15 * ratio.max(1.0).log2()).min(LABEL_MAX_SCALE)
}

/// Where each label goes, in the order given (largest cluster first): centred on its spot, or
/// moved up or down by its own height when that overlaps a label already placed; `None` when
/// none of the three is free or the label would leave `area`. No two placed labels overlap.
pub(crate) fn place_labels(labels: &[(Pos2, Vec2)], area: Rect) -> Vec<Option<Rect>> {
    let mut placed: Vec<Rect> = Vec::new();
    labels
        .iter()
        .map(|(at, size)| {
            let shift = size.y + LABEL_GAP + 1.0;
            let tries = [Vec2::ZERO, Vec2::new(0.0, -shift), Vec2::new(0.0, shift)];
            let spot = tries
                .iter()
                .map(|offset| Rect::from_center_size(*at + *offset, *size))
                .find(|r| area.contains_rect(*r) && placed.iter().all(|p| !p.expand(LABEL_GAP).intersects(*r)));
            if let Some(r) = spot {
                placed.push(r);
            }
            spot
        })
        .collect()
}

/// The density raster of the points at `screen` that fall in `rect`: one texel per [`BIN`] pixels.
/// The points are spread over their neighbourhood with a small Gaussian kernel, so clusters read as
/// soft regions rather than blocks. A texel is coloured by the mix of the kinds' colours there
/// (each weighted by its alpha), and more opaque the more points are near it (the logarithm,
/// against the densest texel) and the more opaque their colours are. `colour(i)` is point `i`'s
/// colour, or `None` when it is dimmed: dimmed points only show as faint grey under the others.
/// `spread` is how many times the kernel is applied: 0 keeps each point to its own texel, so
/// neighbours stand apart; more makes softer regions. Drawn with linear filtering.
pub(crate) fn density(
    screen: &[Pos2],
    colour: impl Fn(usize) -> Option<Color32>,
    rect: Rect,
    spread: usize,
) -> ColorImage {
    let width = ((rect.width() / BIN).ceil().max(1.0) as usize).min(4096);
    let height = ((rect.height() / BIN).ceil().max(1.0) as usize).min(4096);
    // Per texel: count, alpha-weighted red, green and blue sums, dimmed count, alpha sum (0 to 1).
    let mut texels = vec![[0.0f32; 6]; width * height];
    for (i, at) in screen.iter().enumerate() {
        if !rect.contains(*at) {
            continue;
        }
        let x = (((at.x - rect.min.x) / BIN) as usize).min(width - 1);
        let y = (((at.y - rect.min.y) / BIN) as usize).min(height - 1);
        let Some(texel) = texels.get_mut(y * width + x) else { continue };
        match colour(i) {
            Some(c) => {
                let [r, g, b, a] = c.to_srgba_unmultiplied();
                let a = f32::from(a) / 255.0;
                texel[0] += 1.0;
                texel[1] += a * f32::from(r);
                texel[2] += a * f32::from(g);
                texel[3] += a * f32::from(b);
                texel[5] += a;
            }
            None => texel[4] += 1.0,
        }
    }
    for _ in 0..spread {
        texels = blur(&blur(&texels, width, height, 1, width), width, height, width, height);
    }
    let densest = texels.iter().map(|t| t[0]).fold(0.0f32, f32::max).max(1e-6).ln_1p();
    let dimmest = texels.iter().map(|t| t[4]).fold(0.0f32, f32::max).max(1e-6).ln_1p();
    let [grey_r, grey_g, grey_b, grey_a] = crate::kit::theme::text_hint().to_srgba_unmultiplied();
    let pixels = texels
        .iter()
        .map(|t| {
            let shown = (t[0].ln_1p() / densest).min(1.0);
            if t[0] > 0.02 {
                // Smooth towards the edges; full in the densest regions.
                // ... and as opaque as the points' colours are on average.
                let alpha = shown * (2.0 - shown) * (t[5] / t[0]).min(1.0);
                let weight = t[5].max(1e-6);
                Color32::from_rgba_unmultiplied(
                    (t[1] / weight) as u8,
                    (t[2] / weight) as u8,
                    (t[3] / weight) as u8,
                    (alpha * 255.0) as u8,
                )
            } else if t[4] > 0.02 {
                let alpha = 48.0 * f32::from(grey_a) / 255.0 * (t[4].ln_1p() / dimmest).min(1.0);
                Color32::from_rgba_unmultiplied(grey_r, grey_g, grey_b, alpha as u8)
            } else {
                Color32::TRANSPARENT
            }
        })
        .collect();
    ColorImage::new([width, height], pixels)
}

/// The kernel the density is spread with: a binomial approximation of a Gaussian, two texels
/// (six pixels) each way.
const KERNEL: [f32; 5] = [1.0 / 16.0, 4.0 / 16.0, 6.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];

/// `texels` blurred with [`KERNEL`] along one axis: `step` is 1 for rows and `width` for columns,
/// `length` the number of texels along that axis.
fn blur(texels: &[[f32; 6]], width: usize, height: usize, step: usize, length: usize) -> Vec<[f32; 6]> {
    let mut out = vec![[0.0f32; 6]; texels.len()];
    for (i, target) in out.iter_mut().enumerate() {
        let along = if step == 1 { i % width } else { i / width };
        for (k, weight) in KERNEL.iter().enumerate() {
            let Some(from) = (along + k).checked_sub(2).filter(|f| *f < length) else { continue };
            let index = if step == 1 { i - along + from } else { from * width + i % width };
            if let Some(source) = texels.get(index).filter(|_| index < width * height) {
                for (t, s) in target.iter_mut().zip(source) {
                    *t += weight * s;
                }
            }
        }
    }
    out
}

/// A dot's radius and opacity when `visible` points share `area` square pixels: the radius
/// follows the room each point has (1.5 to 6 pixels), and dots that would cover their
/// neighbours are drawn translucent.
pub(crate) fn dot_style(visible: usize, area: f32) -> (f32, f32) {
    let room = (area.max(1.0) / visible.max(1) as f32).sqrt();
    let radius = (room * 0.15).clamp(1.5, 6.0);
    let coverage = visible as f32 * std::f32::consts::PI * radius * radius / area.max(1.0);
    let opacity = (0.5 / coverage.max(1e-6)).clamp(0.35, 1.0);
    (radius, opacity)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` items of `kind` spread evenly in a small square around `centre`.
    fn blob(kind: Kind, centre: [f32; 2], n: usize) -> Vec<([f32; 2], Kind)> {
        let side = (n as f32).sqrt().ceil() as usize;
        (0..n)
            .map(|i| {
                let (x, y) = ((i % side) as f32 / side as f32, (i / side) as f32 / side as f32);
                ([centre[0] - 0.04 + x * 0.08, centre[1] - 0.04 + y * 0.08], kind)
            })
            .collect()
    }

    #[test]
    fn each_cluster_of_a_kind_gets_its_own_label_spot() {
        let mut items = blob(Kind::Kick, [0.2, 0.2], 200);
        items.extend(blob(Kind::Kick, [0.8, 0.8], 100));
        items.extend(blob(Kind::Snare, [0.8, 0.2], 50));
        items.extend(blob(Kind::Hat, [0.2, 0.8], 3));
        let found = clusters(&items);
        let kinds: Vec<(Kind, usize)> = found.iter().map(|c| (c.kind, c.count)).collect();
        // Two kick clusters, largest first; the hat cluster is too small for a label.
        assert_eq!(kinds, vec![(Kind::Kick, 200), (Kind::Kick, 100), (Kind::Snare, 50)]);
        assert!((found[0].spot[0] - 0.2).abs() < 0.03 && (found[0].spot[1] - 0.2).abs() < 0.03);
        assert!((found[1].spot[0] - 0.8).abs() < 0.03, "{found:?}");
        // The spot is an item of the cluster.
        assert!(items.iter().any(|(p, k)| *k == Kind::Kick && *p == found[0].spot));
    }

    #[test]
    fn a_large_map_labels_only_clusters_of_its_share() {
        let mut items = blob(Kind::Kick, [0.3, 0.3], 190_000);
        items.extend(blob(Kind::Snare, [0.8, 0.8], 50));
        // One in 2 000 of 190 050 is 95: the snare cluster of 50 gets no label.
        assert_eq!(clusters(&items).iter().map(|c| c.kind).collect::<Vec<_>>(), vec![Kind::Kick]);
    }

    #[test]
    fn hostile_positions_are_left_out() {
        let items = vec![([f32::NAN, 0.5], Kind::Kick), ([f32::INFINITY, 0.5], Kind::Kick), ([-4.0, 9.0], Kind::Kick)];
        assert!(clusters(&items).is_empty());
        assert!(clusters(&[]).is_empty());
    }

    #[test]
    fn labels_never_overlap_and_stay_in_the_area() {
        let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(300.0, 200.0));
        let size = Vec2::new(60.0, 16.0);
        // Three labels on the same spot: one there, one above, one below; a fourth has no room.
        let at = Pos2::new(150.0, 100.0);
        let placed = place_labels(&[(at, size), (at, size), (at, size), (at, size)], area);
        assert_eq!(placed.iter().filter(|p| p.is_some()).count(), 3);
        assert!(placed[3].is_none());
        // A label that would leave the area is left out.
        assert_eq!(place_labels(&[(Pos2::new(5.0, 5.0), size)], area), vec![None]);
        let rects: Vec<Rect> = placed.into_iter().flatten().collect();
        for (i, a) in rects.iter().enumerate() {
            assert!(area.contains_rect(*a));
            for b in &rects[i + 1..] {
                assert!(!a.intersects(*b), "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn label_sizes_grow_with_the_cluster_and_stop() {
        assert_eq!(label_scale(8, 8), 1.0);
        assert!(label_scale(64, 8) > label_scale(16, 8));
        assert_eq!(label_scale(1_000_000, 8), LABEL_MAX_SCALE);
        assert_eq!(label_scale(0, 0), 1.0);
    }

    #[test]
    fn density_mixes_kind_colours_softly_and_shows_dimmed_points_faintly() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(60.0, 3.0));
        let red = Color32::from_rgb(200, 0, 0);
        let blue = Color32::from_rgb(0, 0, 200);
        // Texel 0: one red and one blue point; texel 10: a dimmed point; one outside.
        let screen = [Pos2::new(1.0, 1.0), Pos2::new(2.0, 2.0), Pos2::new(31.0, 1.0), Pos2::new(500.0, 1.0)];
        let colour = |i: usize| match i {
            0 => Some(red),
            1 => Some(blue),
            _ => None,
        };
        let image = density(&screen, colour, rect, 1);
        assert_eq!(image.size, [20, 1]);
        let mixed = image.pixels[0].to_srgba_unmultiplied();
        assert!(mixed[0].abs_diff(100) <= 1 && mixed[2].abs_diff(100) <= 1, "{mixed:?}");
        assert_eq!(mixed[3], 255, "the densest texel is opaque");
        // The points spread to their neighbours, fading.
        let next = image.pixels[1].a();
        assert!(next > 0 && next < 255, "{next}");
        assert_eq!(image.pixels[3], Color32::TRANSPARENT);
        assert!(image.pixels[10].a() > 0 && image.pixels[10].a() < 64, "a dimmed point is faint");
        assert_eq!(image.pixels[19], Color32::TRANSPARENT);
    }

    /// A translucent kind colour stays translucent in the raster, keeps its own RGB (not the
    /// premultiplied one) and counts for less in a mix with an opaque colour.
    #[test]
    fn density_keeps_the_alpha_of_translucent_colours() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(60.0, 3.0));
        let faint_red = Color32::from_rgba_unmultiplied(200, 0, 0, 128);
        let blue = Color32::from_rgb(0, 0, 200);
        // Texel 0: one faint red point alone; texel 10: a faint red and a blue point.
        let screen = [Pos2::new(1.0, 1.0), Pos2::new(31.0, 1.0), Pos2::new(32.0, 1.0)];
        let colour = |i: usize| Some(if i == 2 { blue } else { faint_red });
        let image = density(&screen, colour, rect, 0);
        let alone = image.pixels[0].to_srgba_unmultiplied();
        assert!(alone[0].abs_diff(200) <= 2 && alone[2] == 0, "{alone:?}");
        assert!(alone[3] < 140, "a translucent colour is not drawn opaque: {alone:?}");
        let mixed = image.pixels[10].to_srgba_unmultiplied();
        assert!(mixed[2] > mixed[0], "the opaque blue outweighs the faint red: {mixed:?}");
        assert!(mixed[3] > alone[3] && mixed[3] < 255, "{mixed:?}");
    }

    /// Smaller points spread less: with no spread, two points two texels apart keep a gap
    /// between them; with more, the gap fills in.
    #[test]
    fn a_smaller_spread_keeps_neighbours_apart() {
        // Ten texels a side; two points on row 5, in columns 0 and 2.
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(30.0, 30.0));
        let screen = [Pos2::new(1.0, 16.0), Pos2::new(7.0, 16.0)];
        let colour = |_| Some(Color32::from_rgb(200, 0, 0));
        let at = |image: &ColorImage, column: usize| image.pixels[5 * 10 + column];
        let sharp = density(&screen, colour, rect, 0);
        assert!(at(&sharp, 0).a() > 0 && at(&sharp, 2).a() > 0);
        assert_eq!(at(&sharp, 1), Color32::TRANSPARENT);
        let soft = density(&screen, colour, rect, 2);
        assert!(at(&soft, 1).a() > 0);
        // A wider spread reaches further: two texels past a point it is stronger.
        assert!(at(&soft, 4).a() > at(&density(&screen, colour, rect, 1), 4).a());
    }

    #[test]
    fn dots_shrink_and_fade_as_they_crowd() {
        let (few, few_opacity) = dot_style(100, 400.0 * 300.0);
        let (many, many_opacity) = dot_style(20_000, 400.0 * 300.0);
        assert!(few > 4.0 && few <= 6.0 && few_opacity == 1.0, "{few} {few_opacity}");
        assert!(many < few && many >= 1.5);
        assert!((0.35..1.0).contains(&many_opacity));
        let (radius, opacity) = dot_style(0, 0.0);
        assert!((1.5..=6.0).contains(&radius) && (0.35..=1.0).contains(&opacity));
    }
}
