// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

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
                let shadow = Shadow { offset: Vec2::new(4.0, -3.0), blur: 6.0, spread, color: Color32::WHITE, inset };
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
    let mut shadow = Shadow { offset: Vec2::new(4.0, 0.0), blur: 0.0, spread: 4.0, color: Color32::RED, inset: false };
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

#[test]
fn artwork_casts_its_glow_in_its_own_shape() {
    let mut artwork = egui::Mesh::default();
    artwork.add_rect_with_uv(
        Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::splat(20.0)),
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
    let glow =
        Shadow { offset: Vec2::ZERO, blur: 8.0, spread: 0.0, color: Color32::from_rgb(0, 200, 255), inset: false };
    let copies = glow.cast_by_artwork(&artwork, 1.0);
    assert_eq!(copies.len(), 9, "the centre and eight taps around the blur");
    // Every copy is the artwork's own shape, tinted, and spread around it.
    assert!(copies.iter().all(|copy| copy.vertices.len() == artwork.vertices.len()));
    assert!(copies.iter().all(|copy| copy.vertices.iter().all(|v| v.color.b() > 0 && v.color.r() == 0)));
    let reach = copies.iter().map(|c| c.calc_bounds()).fold(Rect::NOTHING, |a, b| a.union(b));
    assert!(reach.width() > 20.0 + 6.0, "{reach:?}");
    let inset = Shadow { inset: true, ..glow };
    assert!(inset.cast_by_artwork(&artwork, 1.0).is_empty());
}
