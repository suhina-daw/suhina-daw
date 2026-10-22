// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded native inset/outset box shadows. Blur uses a Gaussian distance-field approximation.
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use egui::{Color32, Painter, Pos2, Rect, TextureHandle, Vec2};
use suhina_style::paint::ResolvedShadow;

const RESOLUTION: usize = 256;
const MAX_TEXTURES: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Shadow {
    offset: Vec2,
    blur: f32,
    spread: f32,
    color: Color32,
    pub inset: bool,
}

fn distance(point: Pos2, rect: Rect, radius: f32) -> f32 {
    if !rect.is_positive() {
        return f32::INFINITY;
    }
    let radius = radius.clamp(0.0, rect.width().min(rect.height()) * 0.5);
    let q = (point - rect.center()).abs() - rect.size() * 0.5 + Vec2::splat(radius);
    q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0) - radius
}

fn coverage(distance: f32, blur: f32) -> f32 {
    if blur <= f32::EPSILON {
        return f32::from(distance <= 0.0);
    }
    // erf approximation, with sigma = half the CSS blur radius. The same CPU
    // image is consumed by both egui rendering backends.
    let x = distance / (blur * std::f32::consts::FRAC_1_SQRT_2);
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let erf =
        1.0 - (((((1.0614054 * t - 1.4531521) * t + 1.4214138) * t - 0.28449672) * t + 0.2548296) * t) * (-x * x).exp();
    (0.5 * (1.0 - x.signum() * erf)).clamp(0.0, 1.0)
}

// Four samples per physical pixel retain subpixel edges on small moving meters.
// Large templates retain the existing cap; DPI changes select a different texture.
fn raster_size(size: Vec2, pixels_per_point: f32) -> [usize; 2] {
    let density = if pixels_per_point.is_finite() { pixels_per_point.clamp(0.25, 8.0) * 4.0 } else { 4.0 };
    [size.x, size.y].map(|extent| ((extent * density).ceil() as usize).saturating_add(1).clamp(2, RESOLUTION))
}

#[derive(Default)]
struct Textures(HashMap<u64, TextureHandle>);

impl Shadow {
    pub fn from_native(shadow: &ResolvedShadow) -> Self {
        Self {
            offset: Vec2::new(shadow.offset[0].clamp(-2048.0, 2048.0), shadow.offset[1].clamp(-2048.0, 2048.0)),
            blur: shadow.blur.clamp(0.0, 256.0),
            spread: shadow.spread.clamp(-512.0, 512.0),
            color: Color32::from_rgba_unmultiplied(shadow.color[0], shadow.color[1], shadow.color[2], shadow.color[3]),
            inset: shadow.inset,
        }
    }

    fn bounds(&self, caster: Rect) -> Rect {
        if self.inset {
            caster
        } else {
            caster.expand(self.spread).translate(self.offset).expand(self.blur * 2.0).union(caster)
        }
    }

    // Outside these edge regions the distance-field shadow is constant along
    // a straight edge. Keep the end caps and stretch only that constant centre.
    fn template_size(&self, size: Vec2, radius: f32) -> Vec2 {
        let support = Vec2::splat(radius + self.spread.abs() + self.blur * 2.0 + 1.0) + self.offset.abs();
        size.min(support * 2.0)
    }

    fn alpha(&self, point: Pos2, caster: Rect, radius: f32) -> f32 {
        let original = coverage(distance(point, caster, radius), 0.0);
        let spread = if self.inset { -self.spread } else { self.spread };
        let shape = caster.expand(spread).translate(self.offset);
        let shadow = coverage(distance(point, shape, (radius + spread).max(0.0)), self.blur);
        if self.inset { original * (1.0 - shadow) } else { (1.0 - original) * shadow }
    }

    pub fn paint(&self, painter: &Painter, caster: Rect, radius: f32, opacity: f32) -> bool {
        if !caster.is_finite()
            || !caster.size().is_finite()
            || !caster.is_positive()
            || !radius.is_finite()
            || self.color.a() == 0
        {
            return false;
        }
        let radius = radius.clamp(0.0, caster.width().min(caster.height()) * 0.5);
        let bounds = self.bounds(caster);
        if !bounds.is_finite() || !bounds.is_positive() || !painter.clip_rect().intersects(bounds) {
            return false;
        }
        let template_size = self.template_size(caster.size(), radius);
        let local = Rect::from_min_size(Pos2::ZERO, template_size);
        let local_bounds = self.bounds(local);
        let [width, height] = raster_size(local_bounds.size(), painter.ctx().pixels_per_point());
        let mut hasher = std::hash::DefaultHasher::new();
        [width, height].hash(&mut hasher);
        [template_size.x, template_size.y, radius, self.offset.x, self.offset.y, self.blur, self.spread]
            .map(f32::to_bits)
            .hash(&mut hasher);
        (self.color, self.inset).hash(&mut hasher);
        let key = hasher.finish();
        let cache = painter.ctx().data_mut(|data| {
            data.get_temp_mut_or_default::<Arc<Mutex<Textures>>>(egui::Id::new("css-shadow-textures")).clone()
        });
        let Ok(mut cache) = cache.lock() else {
            return false;
        };
        let texture = if let Some(texture) = cache.0.get(&key) {
            texture.clone()
        } else {
            let rgba = self.color.to_srgba_unmultiplied();
            let mut pixels = Vec::with_capacity(width * height);
            for y in 0..height {
                for x in 0..width {
                    let at = local_bounds.min
                        + Vec2::new(x as f32 / (width - 1) as f32, y as f32 / (height - 1) as f32)
                            * local_bounds.size();
                    let alpha = (self.alpha(at, local, radius) * f32::from(rgba[3])).round() as u8;
                    pixels.push(Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], alpha));
                }
            }
            let texture = painter.ctx().load_texture(
                format!("css-shadow-{key}"),
                egui::ColorImage::new([width, height], pixels),
                egui::TextureOptions::LINEAR,
            );
            if cache.0.len() >= MAX_TEXTURES {
                cache.0.clear();
            }
            cache.0.insert(key, texture.clone());
            texture
        };
        drop(cache);
        let target_bounds = bounds.translate(-caster.min.to_vec2());
        let (source_x, target_x, nx) =
            slices(caster.width(), template_size.x, local_bounds.x_range(), target_bounds.x_range());
        let (source_y, target_y, ny) =
            slices(caster.height(), template_size.y, local_bounds.y_range(), target_bounds.y_range());
        let mut mesh = egui::Mesh::with_texture(texture.id());
        mesh.vertices.reserve(4 * nx * ny);
        mesh.indices.reserve(6 * nx * ny);
        for (source_y, target_y) in source_y.windows(2).zip(target_y.windows(2)).take(ny) {
            let ([sy0, sy1], [ty0, ty1]) = (source_y, target_y) else {
                continue;
            };
            for (source_x, target_x) in source_x.windows(2).zip(target_x.windows(2)).take(nx) {
                let ([sx0, sx1], [tx0, tx1]) = (source_x, target_x) else {
                    continue;
                };
                let uv = Rect::from_min_max(
                    Pos2::new(
                        (*sx0 - local_bounds.left()) / local_bounds.width(),
                        (*sy0 - local_bounds.top()) / local_bounds.height(),
                    ),
                    Pos2::new(
                        (*sx1 - local_bounds.left()) / local_bounds.width(),
                        (*sy1 - local_bounds.top()) / local_bounds.height(),
                    ),
                );
                mesh.add_rect_with_uv(
                    Rect::from_min_max(caster.min + Vec2::new(*tx0, *ty0), caster.min + Vec2::new(*tx1, *ty1)),
                    uv,
                    Color32::WHITE.gamma_multiply(opacity),
                );
            }
        }
        painter.add(mesh);
        true
    }
}

// Source/target cuts for a bounded nine-slice mesh. The one-point centre strip
// is sampled rather than stretching the rounded corners or the blur radius.
fn slices(actual: f32, template: f32, source: egui::Rangef, target: egui::Rangef) -> ([f32; 4], [f32; 4], usize) {
    if actual > template {
        let left = template * 0.5 - 0.5;
        let right = template * 0.5 + 0.5;
        ([source.min, left, right, source.max], [target.min, left, actual - template + right, target.max], 3)
    } else {
        ([source.min, source.max, 0.0, 0.0], [target.min, target.max, 0.0, 0.0], 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn short_meter_raster_tracks_physical_extent_with_bounded_supersampling() {
        assert_eq!(raster_size(Vec2::new(12.0, 10.0), 1.0), [49, 41]);
        assert_eq!(raster_size(Vec2::new(12.0, 10.0), 2.0), [97, 81]);
        assert_eq!(raster_size(Vec2::splat(f32::MAX), 8.0), [256, 256]);
        assert_eq!(raster_size(Vec2::ZERO, f32::NAN), [2, 2]);
        // A moving short meter must not upload a full 256-square texture per level.
        let ctx = egui::Context::default();
        let shadow = Shadow { offset: Vec2::ZERO, blur: 2.0, spread: 0.0, color: Color32::GREEN, inset: false };
        for frame in 0..4 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                assert!(shadow.paint(
                    ui.painter(),
                    Rect::from_min_size(Pos2::new(20.0, 20.0), Vec2::new(4.0, 1.0 + frame as f32 * 0.1)),
                    0.0,
                    1.0
                ));
            });
            let sizes: Vec<_> = output
                .textures_delta
                .set
                .iter()
                .filter(|(id, _)| **id != egui::TextureId::default())
                .flat_map(|(_, deltas)| deltas.iter().map(|delta| delta.image.size()))
                .collect();
            assert_eq!(sizes.len(), 1);
            assert!(sizes.iter().all(|size| size[0] * size[1] < 4096));
            output.textures_delta.clear();
        }
    }

    #[test]
    fn stretched_centres_preserve_shadow_edges_and_caster_exclusion() {
        let coordinate = |value: f32, source: [f32; 4], target: [f32; 4], count: usize| {
            source
                .windows(2)
                .zip(target.windows(2))
                .take(count)
                .find_map(|(source, target)| {
                    let ([s0, s1], [t0, t1]) = (source, target) else {
                        return None;
                    };
                    (*t0 <= value && value <= *t1 && t1 > t0).then(|| s0 + (value - t0) / (t1 - t0) * (s1 - s0))
                })
                .unwrap()
        };
        for inset in [false, true] {
            for spread in [-5.0, 0.0, 5.0] {
                for size in [Vec2::new(400.0, 180.0), Vec2::new(8.0, 180.0), Vec2::new(180.0, 8.0)] {
                    let radius = 4.0;
                    let shadow =
                        Shadow { offset: Vec2::new(4.0, -3.0), blur: 6.0, spread, color: Color32::WHITE, inset };
                    let actual = Rect::from_min_size(Pos2::ZERO, size);
                    let template = Rect::from_min_size(Pos2::ZERO, shadow.template_size(size, radius));
                    let bounds = shadow.bounds(actual);
                    let source_bounds = shadow.bounds(template);
                    let (sx, tx, nx) = slices(size.x, template.width(), source_bounds.x_range(), bounds.x_range());
                    let (sy, ty, ny) = slices(size.y, template.height(), source_bounds.y_range(), bounds.y_range());
                    for y in 0..41 {
                        for x in 0..41 {
                            let point = bounds.min + bounds.size() * Vec2::new(x as f32 / 40.0, y as f32 / 40.0);
                            let sample = Pos2::new(coordinate(point.x, sx, tx, nx), coordinate(point.y, sy, ty, ny));
                            let error =
                                (shadow.alpha(point, actual, radius) - shadow.alpha(sample, template, radius)).abs();
                            assert!(
                                error < 1.0 / 255.0,
                                "edge changed: inset={inset} spread={spread} size={size:?} at {point:?}, error={error}"
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn changing_meter_extent_reuses_shadow_texture() {
        let ctx = egui::Context::default();
        let shadow = Shadow {
            offset: Vec2::ZERO,
            blur: 3.0,
            spread: 0.0,
            color: Color32::from_rgba_unmultiplied(95, 221, 176, 85),
            inset: false,
        };
        let mut uploads = 0;
        for height in 60..100 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                assert!(shadow.paint(
                    ui.painter(),
                    Rect::from_min_size(egui::pos2(30.0, 30.0), Vec2::new(8.0, height as f32)),
                    0.0,
                    1.0
                ));
            });
            uploads += output
                .textures_delta
                .set
                .iter()
                .filter(|(id, _)| **id != egui::TextureId::default())
                .flat_map(|(_, deltas)| deltas.iter())
                .count();
            output.textures_delta.clear();
        }
        assert_eq!(uploads, 1, "changing only the lit meter extent rasterized its glow repeatedly");
    }
    #[test]
    #[ignore = "manual live-meter shadow timing comparison"]
    fn measure_live_meter_shadows() {
        for moving in [false, true] {
            let ctx = egui::Context::default();
            let shadow = Shadow {
                offset: Vec2::ZERO,
                blur: 3.0,
                spread: 0.0,
                color: Color32::from_rgba_unmultiplied(95, 221, 176, 85),
                inset: false,
            };
            let mut times = Vec::new();
            for sample in 0..22 {
                let started = std::time::Instant::now();
                ctx.run_ui(egui::RawInput::default(), |ui| {
                    for meter in 0..20 {
                        let height = if moving { 60.0 + ((sample * 17 + meter * 7) % 140) as f32 } else { 100.0 };
                        shadow.paint(
                            ui.painter(),
                            Rect::from_min_size(egui::pos2(20.0 + meter as f32 * 25.0, 30.0), Vec2::new(8.0, height)),
                            0.0,
                            1.0,
                        );
                    }
                })
                .textures_delta
                .clear();
                if sample > 1 {
                    times.push(started.elapsed().as_secs_f64() * 1000.0);
                }
            }
            times.sort_by(f64::total_cmp);
            eprintln!("20 meter shadows, moving={moving}: median {:.3} ms, max {:.3} ms", times[10], times[19]);
        }
    }
    #[test]
    fn outset_excludes_the_caster_and_inset_excludes_its_exterior() {
        let caster = Rect::from_min_size(Pos2::ZERO, Vec2::splat(40.0));
        let mut shadow =
            Shadow { offset: Vec2::new(4.0, 0.0), blur: 0.0, spread: 4.0, color: Color32::RED, inset: false };
        assert_eq!(shadow.alpha(egui::pos2(44.0, 20.0), caster, 0.0), 1.0);
        assert_eq!(shadow.alpha(caster.center(), caster, 0.0), 0.0);
        shadow.inset = true;
        assert_eq!(shadow.alpha(egui::pos2(2.0, 20.0), caster, 0.0), 1.0);
        assert_eq!(shadow.alpha(egui::pos2(-2.0, 20.0), caster, 0.0), 0.0);
        assert_eq!(shadow.alpha(caster.center(), caster, 0.0), 0.0);
        shadow.blur = 8.0;
        let soft = shadow.alpha(egui::pos2(8.0, 20.0), caster, 0.0);
        assert!((0.4..0.6).contains(&soft));
    }
}
