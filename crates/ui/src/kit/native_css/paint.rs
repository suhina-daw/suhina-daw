// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared texture/mesh painting for native linear, radial and conic gradients.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::geometry::{Geometry, Mapping};
use egui::{Color32, Painter, Pos2, Rect, Vec2};

const SAMPLES: usize = 1024;
const MAX_TEXTURES: usize = 64;
const SPATIAL_SAMPLES: usize = 256;

#[derive(Clone, Debug)]
pub(super) struct Gradient {
    key: u64,
    geometry: Geometry,
    repeating: bool,
    native: suhina_style::gradient::Linear,
}

impl Gradient {
    #[cfg(test)]
    pub(super) fn pixel_samples(&self, basis: f32) -> Vec<Color32> {
        let points = self.positions(basis);
        (0..1024).map(|index| self.sample(&points, index as f32 / 1023.0)).collect()
    }
    #[cfg(test)]
    pub(super) fn geometry_samples(&self, size: Vec2) -> [f32; 5] {
        let mapping = self.geometry.resolve(size);
        [Vec2::ZERO, Vec2::new(size.x, 0.0), size, Vec2::new(0.0, size.y), size * 0.5]
            .map(|point| mapping.position(point))
    }
    pub fn from_native(gradient: suhina_style::gradient::Linear) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::hash::DefaultHasher::new();
        "native-gradient".hash(&mut hasher);
        format!("{gradient:?}").hash(&mut hasher);
        Self {
            key: hasher.finish(),
            geometry: Geometry::Native(gradient.direction, gradient.shape),
            repeating: gradient.repeating,
            native: gradient,
        }
    }

    fn positions(&self, basis: f32) -> Vec<f32> {
        self.native.positions(basis)
    }

    fn sample(&self, points: &[f32], position: f32) -> Color32 {
        let [r, g, b, a] = self.native.sample(points, position);
        Color32::from_rgba_unmultiplied(r, g, b, a)
    }
}

#[derive(Clone)]
struct Ramp {
    texture: egui::TextureHandle,
    start: f32,
    length: f32,
}
#[derive(Default)]
struct Textures(HashMap<(u64, u32, u32), Ramp>);

fn draw(painter: &Painter, rect: Rect, gradient: &Gradient, radius: f32, opacity: f32) -> bool {
    let _work_trace = crate::kit::frame_trace::WorkSpan::new("CSS gradient paint");
    if !rect.is_finite() || !rect.is_positive() {
        return false;
    }
    let mapping = gradient.geometry.resolve(rect.size());
    let linear = matches!(mapping, Mapping::Linear { .. });
    let basis = mapping.basis();
    if !basis.is_finite() || basis <= 0.0 {
        return false;
    }
    let key = (gradient.key, rect.width().to_bits(), rect.height().to_bits());
    let cache = painter.ctx().data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<Textures>>>(egui::Id::unique("css-gradient-textures")).clone()
    });
    let Ok(mut cache) = cache.lock() else {
        return false;
    };
    let ramp = if let Some(ramp) = cache.0.get(&key) {
        ramp.clone()
    } else {
        let points = gradient.positions(basis);
        let start = if gradient.repeating { points.first().copied().unwrap_or(0.0) } else { 0.0 };
        let length = if gradient.repeating { points.last().copied().unwrap_or(1.0) - start } else { 1.0 };
        if !length.is_finite() || length <= f32::EPSILON {
            return false;
        }
        let samples: Vec<_> = (0..SAMPLES)
            .map(|index| gradient.sample(&points, start + length * index as f32 / (SAMPLES - 1) as f32))
            .collect();
        let image = if linear {
            egui::ColorImage::new([SAMPLES, 1], samples)
        } else {
            // Sample a cached 1D colour ramp rather than evaluating expensive CSS
            // colour-space conversions separately for every spatial texel.
            let mut pixels = Vec::with_capacity(SPATIAL_SAMPLES * SPATIAL_SAMPLES);
            for y in 0..SPATIAL_SAMPLES {
                for x in 0..SPATIAL_SAMPLES {
                    let point = Vec2::new(x as f32, y as f32) / (SPATIAL_SAMPLES - 1) as f32 * rect.size();
                    let value = (mapping.position(point) - start) / length;
                    let value = if gradient.repeating { value.rem_euclid(1.0) } else { value.clamp(0.0, 1.0) };
                    let index = (value * (SAMPLES - 1) as f32).round() as usize;
                    pixels.push(samples.get(index).copied().unwrap_or(Color32::TRANSPARENT));
                }
            }
            egui::ColorImage::new([SPATIAL_SAMPLES, SPATIAL_SAMPLES], pixels)
        };
        let options = egui::TextureOptions {
            wrap_mode: if linear && gradient.repeating {
                egui::TextureWrapMode::Repeat
            } else {
                egui::TextureWrapMode::ClampToEdge
            },
            ..egui::TextureOptions::LINEAR
        };
        let ramp = Ramp {
            texture: painter.ctx().load_texture(format!("css-gradient-{}", gradient.key), image, options),
            start,
            length,
        };
        if cache.0.len() >= MAX_TEXTURES {
            cache.0.clear();
        }
        cache.0.insert(key, ramp.clone());
        ramp
    };
    drop(cache);
    let mut mesh = egui::Mesh::with_texture(ramp.texture.id());
    let uv = |point: Pos2| {
        if linear {
            egui::pos2((mapping.position(point - rect.min) - ramp.start) / ramp.length, 0.5)
        } else {
            let uv = (point - rect.min) / rect.size();
            egui::pos2(uv.x, uv.y)
        }
    };
    let color = Color32::WHITE.gamma_multiply(opacity);
    mesh.vertices.push(egui::epaint::Vertex { pos: rect.center(), uv: uv(rect.center()), color });
    let radius = radius.clamp(0.0, (rect.width().min(rect.height()) * 0.5).max(0.0));
    let corners = [
        (rect.left_top() + Vec2::splat(radius), std::f32::consts::PI),
        (rect.right_top() + Vec2::new(-radius, radius), -std::f32::consts::FRAC_PI_2),
        (rect.right_bottom() - Vec2::splat(radius), 0.0),
        (rect.left_bottom() + Vec2::new(radius, -radius), std::f32::consts::FRAC_PI_2),
    ];
    for (center, angle) in corners {
        for index in 0..=8 {
            let angle = angle + index as f32 / 8.0 * std::f32::consts::FRAC_PI_2;
            let pos = center + Vec2::angled(angle) * radius;
            mesh.vertices.push(egui::epaint::Vertex { pos, uv: uv(pos), color });
        }
    }
    let count = mesh.vertices.len().saturating_sub(1) as u32;
    for index in 1..=count {
        mesh.indices.extend_from_slice(&[0, index, if index == count { 1 } else { index + 1 }]);
    }
    painter.add(mesh);
    true
}

/// An element's box in `rect`, as far as it is painted before its contents (CSS's order): its
/// outer shadows, clipped by `outer` (the box of the nearest element around it that hides its
/// overflow) where one is known, its background colour and its gradients, then for a `control`
/// its inset shadows and border. `style` is its element's (`kit::element`); the caller's `radius`
/// and `fill` stand where the sheet sets none. A transparent background paints nothing, rather
/// than a see-through hole in the window. The surface blends with what is under it: a control is
/// never a hole to the desktop, whatever its alpha (only views, bars and panels set the window's
/// pixels). Returns the radius used, for what the element draws on top and for [`box_end`].
pub(in crate::kit) fn box_begin(
    painter: &Painter,
    rect: Rect,
    radius: f32,
    fill: egui::Color32,
    style: Option<&super::Computed>,
    outer: Option<Rect>,
    control: bool,
) -> f32 {
    if !rect.is_positive() || !rect.is_finite() {
        return 0.0;
    }
    let radius = style.filter(|s| s.authored.radius).map_or(radius, |s| s.radius);
    let radius = radius.min(rect.height().min(rect.width()) / 2.0).max(0.0);
    let fill = style.filter(|s| s.authored.background).map_or(fill, |s| s.background);
    let Some(style) = style else {
        if fill.a() > 0 {
            painter.rect_filled(rect, radius, fill);
        }
        return radius;
    };
    let reach = style.shadows.iter().filter(|s| !s.inset).fold(rect, |reach, shadow| reach.union(shadow.bounds(rect)));
    let outside = outer_painter(painter, outer, rect, reach);
    // A see-through surface shows only its glyph, so its shadow (a glow) is round, around the
    // glyph, rather than the square of a box no one sees, unless the sheet gives a radius.
    let glow = if fill.a() == 0 && !style.authored.radius { rect.width().min(rect.height()) / 2.0 } else { radius };
    for shadow in style.shadows.iter().rev().filter(|s| !s.inset) {
        shadow.paint(&outside, rect, glow, style.opacity);
    }
    if fill.a() > 0 {
        painter.rect_filled(rect, radius, fill);
    }
    for gradient in style.gradients.iter().rev() {
        draw(painter, rect, gradient, radius, style.opacity);
    }
    if control {
        edge(painter, rect, radius, style);
    }
    radius
}

/// The rest of an element's box, painted once its contents are: a region's or container's inset
/// shadows and border (a control's came before its contents), then any element's outline.
pub(in crate::kit) fn box_end(
    painter: &Painter,
    rect: Rect,
    radius: f32,
    style: &super::Computed,
    outer: Option<Rect>,
    control: bool,
) {
    if !rect.is_positive() || !rect.is_finite() {
        return;
    }
    if !control {
        edge(painter, rect, radius, style);
    }
    if let Some((width, color, offset)) = style.outline() {
        let around = rect.expand(offset);
        if around.is_positive() {
            let stroke = egui::Stroke::new(width, color.gamma_multiply(style.opacity));
            let radius = if offset > 0.0 { radius + offset } else { radius };
            outer_painter(painter, outer, rect, around.expand(width)).rect_stroke(
                around,
                radius,
                stroke,
                egui::StrokeKind::Outside,
            );
        }
    }
}

/// Whether `style` paints anything in [`box_end`] for an element of that kind.
pub(in crate::kit) fn has_end(style: &super::Computed, control: bool) -> bool {
    style.outline().is_some()
        || (!control && (style.border_width > 0.0 || style.shadows.iter().any(|shadow| shadow.inset)))
}

/// An element's inset shadows and border.
fn edge(painter: &Painter, rect: Rect, radius: f32, style: &super::Computed) {
    for shadow in style.shadows.iter().rev().filter(|s| s.inset) {
        shadow.paint(painter, rect, radius, style.opacity);
    }
    if style.border_width > 0.0 && style.border.a() > 0 {
        let stroke = egui::Stroke::new(style.border_width, style.border.gamma_multiply(style.opacity));
        painter.rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);
    }
}

/// `painter` clipped by `outer`, the box that clips what is outside an element, for what the
/// element paints around its box `caster` as far as `reach`; `painter` itself where no such box
/// is known.
///
/// `outer` lets the painting pass the painter's own clip (a toolbar section's edge), but only as
/// far as `reach`, and only on a side where `caster` is whole inside that clip: a side that cuts
/// the caster is a viewport's edge (a scroll area scrolled part of the element out of view), and
/// what is beyond it is not the element's to paint over.
fn outer_painter(painter: &Painter, outer: Option<Rect>, caster: Rect, reach: Rect) -> Painter {
    let mut outside = painter.clone();
    if let Some(outer) = outer {
        outside.set_clip_rect(outer.intersect(passable(painter.clip_rect(), caster, reach)));
    }
    outside
}

/// `clip` widened to `reach` on each side where `caster` is inside it.
fn passable(clip: Rect, caster: Rect, reach: Rect) -> Rect {
    let mut wide = clip;
    if caster.min.x >= clip.min.x {
        wide.min.x = clip.min.x.min(reach.min.x);
    }
    if caster.min.y >= clip.min.y {
        wide.min.y = clip.min.y.min(reach.min.y);
    }
    if caster.max.x <= clip.max.x {
        wide.max.x = clip.max.x.max(reach.max.x);
    }
    if caster.max.y <= clip.max.y {
        wide.max.y = clip.max.y.max(reach.max.y);
    }
    wide
}

/// [`box_begin`] and [`box_end`] at once, for a control drawn and done in one go (tests).
#[cfg(test)]
pub(in crate::kit) fn surface_with(
    painter: &Painter,
    rect: Rect,
    radius: f32,
    fill: egui::Color32,
    style: Option<super::Computed>,
) -> f32 {
    let radius = box_begin(painter, rect, radius, fill, style.as_ref(), None, true);
    if let Some(style) = &style {
        box_end(painter, rect, radius, style, None, true);
    }
    radius
}

/// The shadows (a glow is a shadow without an offset) of `style`, cast by artwork: drawn into
/// `slot`, a place reserved under the artwork, as tinted copies of its `mesh`. Returns whether
/// there were any.
pub fn artwork_shadows(
    painter: &Painter,
    slot: egui::layers::ShapeIdx,
    mesh: &egui::Mesh,
    style: &super::Computed,
) -> bool {
    let copies: Vec<egui::Shape> = style
        .shadows
        .iter()
        .rev()
        .flat_map(|shadow| shadow.cast_by_artwork(mesh, style.opacity))
        .map(egui::Shape::mesh)
        .collect();
    if copies.is_empty() {
        return false;
    }
    painter.set(slot, egui::Shape::Vec(copies));
    true
}

/// `style`'s gradients without its outer shadows: for a surface whose artwork casts them itself
/// ([`artwork_shadows`]).
pub fn gradients_rounded(painter: &Painter, rect: Rect, radius: f32, mut style: super::Computed) -> bool {
    if !style.authored.radius {
        style.radius = radius;
    }
    style.radius = style.radius.min(rect.width().min(rect.height()) / 2.0).max(0.0);
    let shadows: Vec<_> = style.shadows.iter().filter(|s| s.inset).cloned().collect();
    style.shadows = shadows.into();
    background_with(painter, rect, &style)
}

/// `style`'s shadows and gradients in `rect`, with `radius` where the sheet gives none.
pub fn background_rounded(painter: &Painter, rect: Rect, radius: f32, mut style: super::Computed) -> bool {
    if !style.authored.radius {
        style.radius = radius;
    }
    // A shadow follows the surface's corners, never rounder than the surface can be.
    style.radius = style.radius.min(rect.width().min(rect.height()) / 2.0).max(0.0);
    background_with(painter, rect, &style)
}

pub fn background_with(painter: &Painter, rect: Rect, style: &super::Computed) -> bool {
    let mut painted = false;
    for shadow in style.shadows.iter().rev().filter(|s| !s.inset) {
        painted |= shadow.paint(painter, rect, style.radius, style.opacity);
    }
    for gradient in style.gradients.iter().rev() {
        painted |= draw(painter, rect, gradient, style.radius, style.opacity);
    }
    for shadow in style.shadows.iter().rev().filter(|s| s.inset) {
        painted |= shadow.paint(painter, rect, style.radius, style.opacity);
    }
    painted
}

/// Meter fills keep their material anchored to the entire scale. The caller clips
/// the painter to the measured portion; shadows are drawn separately around it.
pub fn level_fill(painter: &Painter, scale: Rect, style: &super::Computed) -> bool {
    let mut painted = false;
    if style.authored.background {
        painter.rect_filled(scale, 0.0, style.background.gamma_multiply(style.opacity));
        painted = true;
    }
    for gradient in style.gradients.iter().rev() {
        painted |= draw(painter, scale, gradient, 0.0, style.opacity);
    }
    painted
}

pub fn level_glow(painter: &Painter, lit: Rect, style: &super::Computed) {
    for shadow in style.shadows.iter().rev().filter(|shadow| !shadow.inset) {
        shadow.paint(painter, lit, style.radius, style.opacity);
    }
}
