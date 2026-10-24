// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn a_dialog_that_marks_its_panel_gets_the_window_blurred_under_it() {
    struct Behind;
    impl Provider for Behind {
        fn capture(&self, _: &Context) -> Option<Backdrop> {
            None
        }
        fn capture_behind(
            &self,
            ctx: &Context,
            _shapes: Vec<egui::epaint::ClippedShape>,
            _see_through: bool,
        ) -> Option<Backdrop> {
            Some(Backdrop { texture: TextureId::User(8), sharp_texture: None, screen: ctx.content_rect() })
        }
    }
    let ctx = Context::default();
    install(&ctx, Arc::new(Behind));
    let panel = Color32::from_rgb(4, 5, 6);
    let mut output = ctx.run_ui(Default::default(), |ui| {
        let ctx = ui.ctx().clone();
        begin_frame(&ctx, Default::default());
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("dialog")));
        let rect = Rect::from_min_size(pos2(40.0, 40.0), egui::vec2(120.0, 80.0));
        backdrop(&painter, rect);
        painter.rect_filled(rect, 0.0, panel);
        finish_backdrops(&ctx);
    });
    output.textures_delta.clear();
    let shapes: Vec<_> = output.shapes.into_iter().map(|clipped| clipped.shape).collect();
    let image = shapes.iter().position(|s| matches!(s, egui::Shape::Mesh(m) if m.texture_id == TextureId::User(8)));
    let fill = shapes.iter().position(|s| matches!(s, egui::Shape::Rect(r) if r.fill == panel));
    assert!(matches!((image, fill), (Some(image), Some(fill)) if image < fill), "{shapes:?}");
}

#[test]
fn a_menu_gets_the_window_blurred_under_its_frame_and_other_floating_layers_do_not() {
    use std::sync::Mutex;
    struct Behind(Mutex<Vec<egui::epaint::ClippedShape>>);
    impl Provider for Behind {
        fn capture(&self, _: &Context) -> Option<Backdrop> {
            None
        }
        fn capture_behind(
            &self,
            ctx: &Context,
            shapes: Vec<egui::epaint::ClippedShape>,
            _see_through: bool,
        ) -> Option<Backdrop> {
            if let Ok(mut seen) = self.0.lock() {
                *seen = shapes;
            }
            Some(Backdrop { texture: TextureId::User(7), sharp_texture: None, screen: ctx.content_rect() })
        }
    }
    let host = Arc::new(Behind(Mutex::new(Vec::new())));
    let ctx = Context::default();
    install(&ctx, host.clone());
    let window = Color32::from_rgb(1, 2, 3);
    // A new area is laid out unseen in its first frame; the second shows it.
    let frame = |ui: &mut egui::Ui| {
        let ctx = ui.ctx().clone();
        let area = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 200.0));
        ctx.layer_painter(egui::LayerId::background()).rect_filled(area, 0.0, window);
        egui::Area::new(Id::new("menu")).order(egui::Order::Foreground).fixed_pos(pos2(20.0, 20.0)).show(&ctx, |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| ui.label("Open"));
        });
        // A floating layer that is not a popup's frame is left alone.
        egui::Area::new(Id::new("dialog"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(150.0, 20.0))
            .show(&ctx, |ui| ui.label("Dialog"));
        finish_backdrops(&ctx);
    };
    let mut first = ctx.run_ui(Default::default(), frame);
    first.textures_delta.clear();
    let mut output = ctx.run_ui(Default::default(), frame);
    output.textures_delta.clear();
    let shapes: Vec<_> = output.shapes.into_iter().map(|clipped| clipped.shape).collect();
    let image = shapes
        .iter()
        .position(|s| matches!(s, egui::Shape::Mesh(m) if m.texture_id == TextureId::User(7)))
        .expect("the backdrop under the menu");
    let frame = shapes
        .iter()
        .position(|s| match s {
            egui::Shape::Rect(r) => r.fill != window,
            egui::Shape::Vec(parts) => matches!(parts.last(), Some(egui::Shape::Rect(_))),
            _ => false,
        })
        .expect("the menu's frame");
    assert!(image < frame, "the backdrop is drawn before the menu's frame: {shapes:?}");
    let images = shapes.iter().filter(|s| matches!(s, egui::Shape::Mesh(m) if m.texture_id == TextureId::User(7)));
    assert_eq!(images.count(), 1, "only the menu is blurred behind");
    // The capture holds the window's own layers, never the menu.
    let seen = host.0.lock().map(|seen| seen.clone()).unwrap_or_default();
    assert!(seen.iter().any(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.fill == window)));
    assert!(!seen.iter().any(|c| matches!(&c.shape, egui::Shape::Text(_))));
}

#[test]
fn content_lenses_share_one_current_substrate_and_do_not_poison_panel_capture() {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    struct Observed {
        calls: AtomicUsize,
        sources: Mutex<Vec<Vec<egui::epaint::ClippedShape>>>,
    }
    impl Provider for Observed {
        fn capture(&self, ctx: &Context) -> Option<Backdrop> {
            // A modal replay must never contain the content capture's native texture.
            let order: Vec<_> = ctx.memory(|memory| memory.layer_ids().collect());
            let shapes = ctx.graphics(|layers| layers.clone().drain(&order, &Default::default()));
            assert!(!shapes.iter().any(
                |shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == TextureId::User(99))
            ));
            Some(Backdrop { texture: TextureId::User(100), sharp_texture: None, screen: ctx.content_rect() })
        }
        fn capture_surfaces(&self, ctx: &Context, shapes: Vec<egui::epaint::ClippedShape>) -> Option<Backdrop> {
            self.sources.lock().unwrap().push(shapes);
            self.calls.fetch_add(1, Ordering::SeqCst);
            Some(Backdrop {
                texture: TextureId::User(99),
                sharp_texture: Some(TextureId::User(99)),
                screen: ctx.content_rect(),
            })
        }
    }
    let host = Arc::new(Observed { calls: AtomicUsize::new(0), sources: Mutex::new(Vec::new()) });
    let ctx = Context::default();
    install(&ctx, host.clone());
    configure(&ctx, Options { panels: 50, tracks: 65, clips: 75 });
    for frame in 0..2 {
        let mut output = ctx.run_ui(Default::default(), |ui| {
            begin_frame(ui.ctx(), Default::default());
            let painter = ui.painter();
            for (x, surface) in [(20.0, Surface::Track), (240.0, Surface::Clip)] {
                let rect = Rect::from_min_size(pos2(x, 20.0), egui::vec2(200.0, 80.0));
                let base = if frame == 0 { Color32::RED } else { Color32::BLUE };
                painter.rect_filled(rect, 0.0, base);
                paint(painter, rect, base, surface);
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Sharp notes",
                    egui::FontId::proportional(15.0),
                    Color32::WHITE,
                );
            }
            finish_surfaces(ui.ctx());
            prepare(ui.ctx());
        });
        assert_eq!(host.calls.load(Ordering::SeqCst), frame + 1);
        assert!(
            output
                .shapes
                .iter()
                .any(|shape| matches!(&shape.shape, egui::Shape::Mesh(mesh) if mesh.texture_id == TextureId::User(99)))
        );
        output.textures_delta.clear();
    }
    let sources = host.sources.lock().unwrap();
    assert!(sources.iter().flatten().all(|shape| !matches!(shape.shape, egui::Shape::Text(_))));
    assert!(matches!(&sources[0][0].shape, egui::Shape::Rect(rect) if rect.fill == Color32::RED));
    assert!(matches!(&sources[1][0].shape, egui::Shape::Rect(rect) if rect.fill == Color32::BLUE));
}

#[test]
fn coloured_gradient_keeps_the_fill_opacity() {
    let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(220.0, 64.0));
    for alpha in [0, 64, 128, 255] {
        let base = Color32::from_rgba_unmultiplied(120, 160, 200, alpha);
        let mesh = body_mesh(rect, base, 1.0);
        assert!(mesh.vertices.iter().all(|v| v.color.a() == alpha));
    }
}

#[test]
fn edge_refraction_bends_current_backdrop_without_moving_the_centre() {
    let screen = Rect::from_min_size(pos2(10.0, 20.0), egui::vec2(800.0, 600.0));
    let rect = Rect::from_min_size(pos2(50.0, 60.0), egui::vec2(400.0, 300.0));
    let backdrop = Backdrop { screen, texture: TextureId::User(1), sharp_texture: Some(TextureId::User(2)) };
    let mesh = refraction_mesh(rect, backdrop, 1.0);
    assert!(mesh.is_valid());
    assert_eq!(mesh.vertices.len(), 121);
    assert_eq!(mesh.indices.len(), 600);
    assert_eq!(mesh.texture_id, TextureId::User(2));
    assert_eq!(mesh.calc_bounds(), rect);
    let center = &mesh.vertices[60];
    assert_eq!(center.pos, rect.center());
    assert_eq!(center.uv, ((rect.center() - screen.min) / screen.size()).to_pos2());
    assert_eq!(center.color.a(), 0);
    assert!(mesh.vertices.iter().any(|v| v.color.a() > 0 && v.uv != ((v.pos - screen.min) / screen.size()).to_pos2()));
    for row in mesh.vertices.chunks(11) {
        assert!(row.windows(2).all(|v| v[0].uv.x <= v[1].uv.x), "lens folded its texture");
    }
    for vertex in &mesh.vertices {
        assert!(vertex.uv.x.is_finite() && vertex.uv.y.is_finite());
        assert!((0.0..=1.0).contains(&vertex.uv.x) && (0.0..=1.0).contains(&vertex.uv.y));
    }
    let low = refraction_mesh(rect, backdrop, 0.25);
    assert!(low.vertices.iter().map(|v| v.color.a()).max() < mesh.vertices.iter().map(|v| v.color.a()).max());
    assert!(refraction_mesh(rect, backdrop, f32::NAN).is_empty());
    assert!(refraction_mesh(rect, backdrop, 0.0).is_empty());
    assert!(refraction_mesh(Rect::NOTHING, backdrop, 1.0).is_empty());
    assert!(refraction_mesh(rect, Backdrop { screen: Rect::NOTHING, ..backdrop }, 1.0).is_empty());
    let clipped = refraction_mesh(Rect::from_min_size(pos2(-20.0, -30.0), rect.size()), backdrop, 1.0);
    assert!(clipped.vertices.iter().all(|v| (0.0..=1.0).contains(&v.uv.x) && (0.0..=1.0).contains(&v.uv.y)));
}

#[test]
fn coloured_body_retains_a_gradient_between_the_rims() {
    let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(220.0, 64.0));
    for base in [Color32::from_rgb(204, 210, 175), Color32::from_rgb(45, 60, 80)] {
        let mesh = body_mesh(rect, base, 1.0);
        assert!(mesh.is_valid());
        assert_eq!(mesh.calc_bounds(), rect);
        assert!(mesh.vertices.iter().any(|vertex| vertex.pos.y > rect.top() && vertex.pos.y < rect.bottom()));
        assert!(mesh.vertices.iter().any(|vertex| vertex.color.r() > base.r()));
        assert!(mesh.vertices.iter().any(|vertex| vertex.color.r() < base.r()));
        assert!(mesh.vertices.iter().all(|vertex| vertex.color.is_opaque()));
    }
    assert!(body_mesh(rect, Color32::GRAY, 0.0).is_empty());
    assert!(body_mesh(rect, Color32::GRAY, f32::NAN).is_empty());
    assert!(body_mesh(Rect::NOTHING, Color32::GRAY, 1.0).is_empty());
}

#[test]
fn optical_rim_is_bounded_and_leaves_the_centre_clear() {
    for size in [egui::vec2(13.0, 14.0), egui::vec2(220.0, 64.0), egui::vec2(1600.0, 200.0)] {
        let rect = Rect::from_min_size(pos2(30.0, 50.0), size);
        let mesh = rim_mesh(rect, 1.0);
        assert!(mesh.is_valid());
        assert_eq!(mesh.vertices.len(), 216);
        for vertex in &mesh.vertices {
            assert!(rect.expand(0.001).contains(vertex.pos));
            assert!(vertex.pos.distance(rect.center()) > size.min_elem() * 0.2);
            assert!(vertex.color.r() <= vertex.color.a());
            assert!(vertex.color.g() <= vertex.color.a());
            assert!(vertex.color.b() <= vertex.color.a());
        }
    }
}

#[test]
fn rim_fades_at_small_sizes_and_rejects_invalid_geometry() {
    let area = |height| Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, height));
    let peak = |height, strength| {
        rim_mesh(area(height), strength).vertices.iter().map(|vertex| vertex.color.a()).max().unwrap_or(0)
    };
    assert!(peak(14.0, 1.0) < peak(40.0, 1.0));
    assert!(peak(40.0, 0.5) < peak(40.0, 1.0));
    assert_eq!(peak(12.0, 1.0), 0);
    assert!(rim_mesh(area(40.0), 0.0).is_empty());
    assert!(rim_mesh(area(f32::NAN), 1.0).is_empty());
    assert!(rim_mesh(area(40.0), f32::INFINITY).is_empty());
}

#[test]
fn default_reduced_effects_and_tiny_surfaces_remain_solid() {
    let ctx = Context::default();
    let mut output = ctx.run_ui(Default::default(), |ui| {
        let ctx = ui.ctx();
        let painter = ctx.layer_painter(egui::LayerId::background());
        let area = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 40.0));
        let count =
            || ctx.graphics(|graphics| graphics.get(painter.layer_id()).map_or(0, |list| list.all_entries().len()));
        let before = count();
        paint(&painter, area, Color32::GRAY, Surface::Track);
        assert_eq!(count(), before);
        ctx.data_mut(|data| data.insert_temp(Id::new(OPTIONS), Options { tracks: 50, ..Default::default() }));
        paint(&painter, area, Color32::GRAY, Surface::Track);
        assert!(count() > before);
        let coated = count();
        // The track's slider does not switch on the clip material.
        paint(&painter, area, Color32::GRAY, Surface::Clip);
        assert_eq!(count(), coated);
        set_effects_allowed(ctx, false);
        paint(&painter, area, Color32::GRAY, Surface::Track);
        assert_eq!(count(), coated);
        set_effects_allowed(ctx, true);
        paint(&painter, Rect::from_min_size(area.min, egui::vec2(10.0, 10.0)), Color32::GRAY, Surface::Track);
        assert_eq!(count(), coated);
    });
    output.textures_delta.clear();
}

#[test]
fn failed_capture_does_not_reuse_a_previous_backdrop() {
    let ctx = Context::default();
    ctx.data_mut(|data| {
        data.insert_temp(
            Id::new(BACKDROP),
            Backdrop { texture: TextureId::User(42), sharp_texture: None, screen: Rect::EVERYTHING },
        )
    });
    prepare(&ctx);
    assert!(ctx.data(|data| data.get_temp::<Backdrop>(Id::new(BACKDROP))).is_none());
}
