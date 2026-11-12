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
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::unique("dialog")));
        let rect = Rect::from_min_size(pos2(40.0, 40.0), egui::vec2(120.0, 80.0));
        backdrop(&painter, rect);
        painter.rect_filled(rect, 0.0, panel);
        finish_backdrops(&ctx);
    });
    output.textures_delta.clear();
    let shapes: Vec<_> = output.shapes.into_iter().map(|clipped| clipped.shape).collect();
    let image =
        shapes.iter().position(|s| matches!(s, egui::Shape::Rect(r) if r.fill_texture_id() == TextureId::User(8)));
    let fill = shapes.iter().position(|s| matches!(s, egui::Shape::Rect(r) if r.fill == panel));
    assert!(matches!((image, fill), (Some(image), Some(fill)) if image < fill), "{shapes:?}");
}

/// A host that counts its captures behind the floating surfaces and keeps the last one's shapes.
struct Behind {
    calls: std::sync::atomic::AtomicUsize,
    seen: std::sync::Mutex<Vec<egui::epaint::ClippedShape>>,
    /// Captures of the window with a dialog in it, and the last one's shapes.
    above: std::sync::atomic::AtomicUsize,
    seen_above: std::sync::Mutex<Vec<egui::epaint::ClippedShape>>,
}

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
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut seen) = self.seen.lock() {
            *seen = shapes;
        }
        Some(Backdrop { texture: TextureId::User(7), sharp_texture: None, screen: ctx.content_rect() })
    }
    fn capture_above(
        &self,
        ctx: &Context,
        shapes: Vec<egui::epaint::ClippedShape>,
        _see_through: bool,
    ) -> Option<Backdrop> {
        self.above.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut seen) = self.seen_above.lock() {
            *seen = shapes;
        }
        Some(Backdrop { texture: TextureId::User(9), sharp_texture: None, screen: ctx.content_rect() })
    }
}

const WINDOW: Color32 = Color32::from_rgb(1, 2, 3);

/// `float` drawn over a window for long enough that what it shows has faded in, with `css` as
/// the theme; the last frame's shapes and how many captures the host made in it.
fn floating_frames(css: Option<&str>, float: impl Fn(&Context)) -> (Vec<egui::Shape>, usize, Arc<Behind>) {
    super::super::skin::configure(&super::super::skin::Skin { css: css.map(Into::into), ..Default::default() });
    let host = Arc::new(Behind {
        calls: Default::default(),
        seen: Default::default(),
        above: Default::default(),
        seen_above: Default::default(),
    });
    let ctx = Context::default();
    install(&ctx, host.clone());
    let frame = |ui: &mut egui::Ui| {
        let ctx = ui.ctx().clone();
        begin_frame(&ctx, Default::default());
        let area = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
        ctx.layer_painter(egui::LayerId::background()).rect_filled(area, 0.0, WINDOW);
        float(&ctx);
        finish_backdrops(&ctx);
    };
    // A new area is laid out unseen in its first frame, then fades in.
    let mut last = Vec::new();
    for step in 0..4 {
        host.calls.store(0, std::sync::atomic::Ordering::SeqCst);
        host.above.store(0, std::sync::atomic::Ordering::SeqCst);
        let input = egui::RawInput {
            time: Some(f64::from(step)),
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, frame);
        output.textures_delta.clear();
        // egui paints a frame with a shadow as `[shadow, frame]`: flattened, each shape is one.
        last = output
            .shapes
            .into_iter()
            .flat_map(|clipped| match clipped.shape {
                egui::Shape::Vec(parts) => parts,
                shape => vec![shape],
            })
            .collect();
    }
    super::super::skin::configure(&super::super::skin::Skin::default());
    let calls = host.calls.load(std::sync::atomic::Ordering::SeqCst);
    (last, calls, host)
}

fn is_image(shape: &egui::Shape) -> bool {
    matches!(shape, egui::Shape::Rect(r) if r.fill_texture_id() == TextureId::User(7))
}

/// A popup's frame in another colour than egui's window fill, as a theme gives one.
fn odd_menu(ctx: &Context, id: &str, order: egui::Order, at: egui::Pos2) {
    super::super::floating::area(ctx, Id::unique(id), super::super::floating::Kind::Popup)
        .order(order)
        .fixed_pos(at)
        .show(ctx, |ui| {
            egui::Frame::menu(ui.style()).fill(Color32::from_rgb(200, 10, 10)).show(ui, |ui| ui.label("Open"));
        });
}

#[test]
fn popups_and_tooltips_get_the_window_blurred_under_their_frames_whatever_their_colour() {
    let (shapes, calls, host) = floating_frames(None, |ctx| {
        odd_menu(ctx, "menu", egui::Order::Foreground, pos2(20.0, 20.0));
        odd_menu(ctx, "tooltip", egui::Order::Tooltip, pos2(200.0, 20.0));
        // A drag's preview, painted on without an area: never in the capture.
        ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::unique("drag"))).rect_filled(
            Rect::from_min_size(pos2(300.0, 200.0), egui::vec2(40.0, 20.0)),
            0.0,
            Color32::from_rgb(0, 200, 0),
        );
    });
    assert_eq!(calls, 1, "one capture however many surfaces float");
    let images: Vec<usize> = shapes.iter().enumerate().filter(|(_, s)| is_image(s)).map(|(i, _)| i).collect();
    let frames: Vec<usize> = shapes
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(s, egui::Shape::Rect(r) if r.fill.to_srgba_unmultiplied()[0] == 200))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(images.len(), 2, "{shapes:?}");
    assert_eq!(frames.len(), 2, "{shapes:?}");
    assert!(images.iter().zip(&frames).all(|(image, frame)| image < frame), "each image is under its frame");
    // The capture holds the window's own layers, never what floats.
    let seen = host.seen.lock().map(|seen| seen.clone()).unwrap_or_default();
    assert!(seen.iter().any(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.fill == WINDOW)));
    assert!(!seen.iter().any(|c| matches!(&c.shape, egui::Shape::Text(_))));
    assert!(!seen.iter().any(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.fill == Color32::from_rgb(0, 200, 0))));
}

#[test]
fn the_app_s_own_layers_are_not_blurred_and_nothing_floating_means_no_capture() {
    use super::super::floating::{Kind, area};
    let (shapes, calls, _) = floating_frames(None, |ctx| {
        // A dialog's dimming, an overlay and a drag's preview, each a rectangle first.
        for (id, kind, order) in [
            ("dialog", Kind::Own, egui::Order::Foreground),
            ("overlay", Kind::Own, egui::Order::Foreground),
            ("splash", Kind::Own, egui::Order::Tooltip),
        ] {
            area(ctx, Id::unique(id), kind).order(order).fixed_pos(pos2(20.0, 20.0)).show(ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| ui.label(id));
            });
        }
        // Painted on without an area: never taken for a tooltip.
        super::super::floating::modal(ctx, Id::unique("help")).show(ctx, |ui| ui.label("Help"));
        let drag = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, Id::unique("drag")));
        drag.rect_filled(Rect::from_min_size(pos2(5.0, 5.0), egui::vec2(40.0, 20.0)), 0.0, Color32::RED);
    });
    assert_eq!(calls, 0, "no capture when nothing floats");
    assert!(!shapes.iter().any(is_image), "{shapes:?}");
}

/// A menu opened over a dialog (a dropdown in Export audio, Settings' More) shows the dialog
/// blurred behind it, with the dialog's own blur in it; the dialog's blur is the window alone.
#[test]
fn a_menu_over_a_dialog_gets_the_dialog_blurred_behind_it() {
    use super::super::floating::{Kind, area};
    let panel = Color32::from_rgb(4, 5, 6);
    let (shapes, calls, host) = floating_frames(None, |ctx| {
        area(ctx, Id::unique("dialog"), Kind::Own).order(egui::Order::Foreground).fixed_pos(pos2(0.0, 0.0)).show(
            ctx,
            |ui| {
                let rect = Rect::from_min_size(pos2(40.0, 40.0), egui::vec2(200.0, 150.0));
                backdrop(ui.painter(), rect);
                ui.painter().rect_filled(rect, 0.0, panel);
                ui.allocate_rect(rect, egui::Sense::hover());
            },
        );
        odd_menu(ctx, "dropdown", egui::Order::Foreground, pos2(60.0, 60.0));
    });
    assert_eq!(calls, 1, "the window, for the dialog");
    assert_eq!(host.above.load(std::sync::atomic::Ordering::SeqCst), 1, "the window and the dialog, for the menu");
    let dialog_image = shapes.iter().position(is_image).expect("the dialog's blur");
    let menu_image = shapes
        .iter()
        .position(|s| matches!(s, egui::Shape::Rect(r) if r.fill_texture_id() == TextureId::User(9)))
        .expect("the menu's blur");
    assert!(dialog_image < menu_image);
    let seen = host.seen_above.lock().map(|seen| seen.clone()).unwrap_or_default();
    assert!(seen.iter().any(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.fill == panel)), "the dialog is in it");
    assert!(
        seen.iter().any(|c| matches!(&c.shape, egui::Shape::Rect(r) if r.fill_texture_id() == TextureId::User(7))),
        "with its own blur"
    );
    assert!(!seen.iter().any(|c| matches!(&c.shape, egui::Shape::Text(_))), "and never the menu");
}

/// A popup that is not blurred still floats: a tooltip over it, over a dialog, blurs the dialog
/// and never the popup's contents.
#[test]
fn a_capture_never_holds_a_floating_surface_even_one_left_sharp() {
    use super::super::floating::{Kind, area};
    let (_, _, host) = floating_frames(Some("popup { backdrop-filter: none; }"), |ctx| {
        area(ctx, Id::unique("dialog"), Kind::Own).order(egui::Order::Foreground).fixed_pos(pos2(0.0, 0.0)).show(
            ctx,
            |ui| {
                ui.painter().rect_filled(Rect::from_min_size(pos2(40.0, 40.0), egui::vec2(200.0, 150.0)), 0.0, WINDOW);
            },
        );
        odd_menu(ctx, "dropdown", egui::Order::Foreground, pos2(60.0, 60.0));
        odd_menu(ctx, "tooltip", egui::Order::Tooltip, pos2(70.0, 70.0));
    });
    assert_eq!(host.above.load(std::sync::atomic::Ordering::SeqCst), 1);
    let seen = host.seen_above.lock().map(|seen| seen.clone()).unwrap_or_default();
    assert!(!seen.is_empty());
    assert!(!seen.iter().any(|c| matches!(&c.shape, egui::Shape::Text(_))), "{seen:?}");
}

#[test]
fn backdrop_filter_none_keeps_a_surface_sharp() {
    let (shapes, calls, _) = floating_frames(Some("popup { backdrop-filter: none; }"), |ctx| {
        odd_menu(ctx, "menu", egui::Order::Foreground, pos2(20.0, 20.0));
        odd_menu(ctx, "tooltip", egui::Order::Tooltip, pos2(200.0, 20.0));
    });
    assert_eq!(calls, 1);
    assert_eq!(shapes.iter().filter(|s| is_image(s)).count(), 1, "only the tooltip is blurred: {shapes:?}");
}

#[test]
fn a_theme_s_popup_and_tooltip_frames_replace_egui_s() {
    let css = "popup { background-color: #00ff00; border-radius: 6px; } \
               tooltip { background-color: #0000ff; border-width: 2px; border-color: #ff00ff; }";
    let (shapes, _, _) = floating_frames(Some(css), |ctx| {
        odd_menu(ctx, "menu", egui::Order::Foreground, pos2(20.0, 20.0));
        odd_menu(ctx, "tooltip", egui::Order::Tooltip, pos2(200.0, 20.0));
    });
    let rect = |s: &egui::Shape| match s {
        egui::Shape::Rect(r) => Some(r.clone()),
        _ => None,
    };
    let rects: Vec<_> = shapes.iter().filter_map(rect).collect();
    assert!(!rects.iter().any(|r| r.fill.to_srgba_unmultiplied()[0] == 200), "egui's frames are gone: {rects:?}");
    let popup = rects.iter().find(|r| r.fill == Color32::from_rgb(0, 255, 0)).expect("the popup's frame");
    assert_eq!(popup.corner_radius, egui::CornerRadius::same(6));
    let image = rects.iter().find(|r| r.fill_texture_id() == TextureId::User(7) && r.rect == popup.rect);
    assert_eq!(image.map(|r| r.corner_radius), Some(egui::CornerRadius::same(6)), "the blur takes its corners");
    assert!(rects.iter().any(|r| r.fill == Color32::from_rgb(0, 0, 255)), "the tooltip's frame");
}

/// A sheet that sets a popup's background but no shadow keeps egui's shadow, and an outline is
/// drawn over the popup's contents, as CSS draws it.
#[test]
fn a_restyled_frame_keeps_egui_s_shadow_and_draws_its_outline_over_the_contents() {
    let css = "popup { background-color: #00ff00; outline: 2px solid #ff0000; }";
    let (shapes, _, _) =
        floating_frames(Some(css), |ctx| odd_menu(ctx, "menu", egui::Order::Foreground, pos2(20.0, 20.0)));
    let frame = shapes
        .iter()
        .position(|s| matches!(s, egui::Shape::Rect(r) if r.fill == Color32::from_rgb(0, 255, 0)))
        .expect("the popup's frame");
    let shadow = shapes.iter().position(|s| matches!(s, egui::Shape::Rect(r) if r.blur_width > 0.0));
    assert!(shadow.is_some_and(|shadow| shadow < frame), "egui's shadow stays under it: {shapes:?}");
    let text = shapes.iter().position(|s| matches!(s, egui::Shape::Text(_))).expect("the contents");
    let outline = shapes
        .iter()
        .position(|s| matches!(s, egui::Shape::Rect(r) if r.stroke.color == Color32::from_rgb(255, 0, 0)))
        .expect("the outline");
    assert!(frame < text && text < outline, "{frame} < {text} < {outline}");
}

/// `box-shadow: none` takes egui's shadow away, as any other value replaces it.
#[test]
fn box_shadow_none_removes_egui_s_shadow() {
    let css = "popup { background-color: #00ff00; box-shadow: none; }";
    let (shapes, _, _) =
        floating_frames(Some(css), |ctx| odd_menu(ctx, "menu", egui::Order::Foreground, pos2(20.0, 20.0)));
    assert!(shapes.iter().any(|s| matches!(s, egui::Shape::Rect(r) if r.fill == Color32::from_rgb(0, 255, 0))));
    assert!(!shapes.iter().any(|s| matches!(s, egui::Shape::Rect(r) if r.blur_width > 0.0)), "{shapes:?}");
}

/// A `border-color` alone colours egui's own stroke: the width stays egui's.
#[test]
fn border_color_alone_colours_egui_s_stroke() {
    let (shapes, _, _) = floating_frames(Some("popup { border-color: #ff0000; }"), |ctx| {
        odd_menu(ctx, "menu", egui::Order::Foreground, pos2(20.0, 20.0))
    });
    let stroke = shapes.iter().find_map(|s| match s {
        egui::Shape::Rect(r) if r.stroke.width > 0.0 && r.fill_texture_id() != TextureId::User(7) => Some(r.stroke),
        _ => None,
    });
    assert!(stroke.is_some_and(|stroke| matches!(stroke.color.to_array(), [r, 0, 0, _] if r > 0)), "{shapes:?}");
}

/// `user.css` styles popups and tooltips when the theme has no sheet at all.
#[test]
fn user_css_alone_styles_popups() {
    super::super::native_css::set_user_sheet(Some(
        "popup { backdrop-filter: none; background-color: #00ff00; }".into(),
    ));
    let (shapes, calls, _) =
        floating_frames(None, |ctx| odd_menu(ctx, "menu", egui::Order::Foreground, pos2(20.0, 20.0)));
    super::super::native_css::set_user_sheet(None);
    assert_eq!(calls, 0, "left sharp");
    assert!(shapes.iter().any(|s| matches!(s, egui::Shape::Rect(r) if r.fill == Color32::from_rgb(0, 255, 0))));
}

#[test]
fn a_bar_menu_blurs_as_menu_and_keeps_egui_s_frame() {
    let (shapes, _, _) =
        floating_frames(Some("popup { background-color: #00ff00; } popup { backdrop-filter: none; }"), |ctx| {
            super::super::floating::area(ctx, Id::unique("bar"), super::super::floating::Kind::Menu)
                .order(egui::Order::Foreground)
                .fixed_pos(pos2(20.0, 20.0))
                .show(ctx, |ui| {
                    egui::Frame::menu(ui.style()).show(ui, |ui| ui.label("File"));
                });
        });
    assert_eq!(shapes.iter().filter(|s| is_image(s)).count(), 1, "{shapes:?}");
    assert!(!shapes.iter().any(|s| matches!(s, egui::Shape::Rect(r) if r.fill == Color32::from_rgb(0, 255, 0))));
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
                paint(painter, rect, &super::super::element::WORKSPACE_PANEL.enter(), base, surface);
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
        assert!(mesh.vertices.iter().any(|vertex| vertex.color.to_array()[0] > base.to_array()[0]));
        assert!(mesh.vertices.iter().any(|vertex| vertex.color.to_array()[0] < base.to_array()[0]));
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
            assert!(vertex.color.to_array()[0] <= vertex.color.a());
            assert!(vertex.color.to_array()[1] <= vertex.color.a());
            assert!(vertex.color.to_array()[2] <= vertex.color.a());
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
        paint(&painter, area, &super::super::element::WORKSPACE_PANEL.enter(), Color32::GRAY, Surface::Track);
        assert_eq!(count(), before);
        ctx.data_mut(|data| data.insert_temp(Id::unique(OPTIONS), Options { tracks: 50, ..Default::default() }));
        paint(&painter, area, &super::super::element::WORKSPACE_PANEL.enter(), Color32::GRAY, Surface::Track);
        assert!(count() > before);
        let coated = count();
        // The track's slider does not switch on the clip material.
        paint(&painter, area, &super::super::element::WORKSPACE_PANEL.enter(), Color32::GRAY, Surface::Clip);
        assert_eq!(count(), coated);
        set_effects_allowed(ctx, false);
        paint(&painter, area, &super::super::element::WORKSPACE_PANEL.enter(), Color32::GRAY, Surface::Track);
        assert_eq!(count(), coated);
        set_effects_allowed(ctx, true);
        paint(
            &painter,
            Rect::from_min_size(area.min, egui::vec2(10.0, 10.0)),
            &super::super::element::WORKSPACE_PANEL.enter(),
            Color32::GRAY,
            Surface::Track,
        );
        assert_eq!(count(), coated);
    });
    output.textures_delta.clear();
}

#[test]
fn failed_capture_does_not_reuse_a_previous_backdrop() {
    let ctx = Context::default();
    ctx.data_mut(|data| {
        data.insert_temp(
            Id::unique(BACKDROP),
            Backdrop { texture: TextureId::User(42), sharp_texture: None, screen: Rect::EVERYTHING },
        )
    });
    prepare(&ctx);
    assert!(ctx.data(|data| data.get_temp::<Backdrop>(Id::unique(BACKDROP))).is_none());
}
