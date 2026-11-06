// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn svg_symbols_keep_aspect_ratio_while_surface_artwork_stretches() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ffffff"/></svg>"##;
    for (width, height) in [(32, 16), (16, 32)] {
        let symbol = raster_artwork(svg, Color32::WHITE, width, height, true).unwrap();
        let surface = raster_artwork(svg, Color32::WHITE, width, height, false).unwrap();
        let lit = symbol.pixels.iter().filter(|p| p.a() > 0).count();
        assert_eq!(lit, 16 * 16);
        assert!(surface.pixels.iter().all(|p| p.a() == 255));
        assert_eq!(symbol.pixels[0].a(), 0);
    }
}

#[test]
fn png_roles_reuse_pixels_until_a_live_edit_changes_the_source() {
    let png = super::super::artwork_png::fixture(2, 2, [20, 80, 160, 255]);
    let mut skin = Skin { assets: BTreeMap::from([("button".into(), png)]), ..Default::default() };
    let mut warnings = Vec::new();
    assert_eq!(Skin::from_json(&skin.to_json(), &mut warnings), skin);
    assert!(warnings.is_empty());
    let ctx = egui::Context::default();
    for frame in 0..3 {
        if frame == 2 {
            skin.assets.insert("button".into(), super::super::artwork_png::fixture(2, 2, [40, 160, 80, 255]));
        }
        configure(&skin);
        let mut output = ctx.run_ui(Default::default(), |ui| {
            assert!(button(
                ui.painter(),
                Rect::from_min_size(egui::pos2(10.0, 10.0), egui::Vec2::splat(20.0)),
                State::Normal,
                Color32::RED,
                0.0
            ));
        });
        let expected = if frame == 2 { Color32::from_rgb(40, 160, 80) } else { Color32::from_rgb(20, 80, 160) };
        let found = output.textures_delta.set.values().flat_map(|d| d.iter()).any(|delta| match &delta.image {
            egui::ImageData::Color(image) => image.pixels.iter().all(|pixel| *pixel == expected),
        });
        output.textures_delta.clear();
        assert_eq!(found, frame != 1, "warm draws reuse the same PNG; live edits publish new pixels");
    }
    configure(&Skin::default());
    let huge = json!({"assets":{"button": "x".repeat(MAX_ARTWORK_BYTES + 1)}});
    assert!(Skin::from_json(&huge, &mut warnings).assets.is_empty());
    assert!(!warnings.is_empty());
}

#[test]
fn svg_backgrounds_clear_native_alpha_once_and_keep_private_images_underneath() {
    struct Mark;
    impl super::super::surface::Provider for Mark {
        fn shape(&self, rect: Rect, _: egui::CornerRadius, color: Color32) -> egui::Shape {
            assert_eq!(color, Color32::TRANSPARENT);
            egui::Shape::circle_filled(rect.center(), 1.0, Color32::BLUE)
        }
    }
    configure(&Skin::default());
    let ctx = egui::Context::default();
    super::super::surface::install(&ctx, Arc::new(Mark));
    // Only a theme whose surfaces show the desktop clears under its artwork.
    let saved = super::super::theme::palette();
    let mut see_through = saved;
    see_through.window_background = Color32::from_rgba_unmultiplied(20, 20, 20, 160);
    super::super::theme::set_palette(&ctx, see_through);
    for private_image in [false, true] {
        super::super::liquid::set_background_image(
            &ctx,
            private_image.then(|| super::super::liquid::BackgroundImage {
                id: egui::TextureId::Managed(42),
                pixels: Arc::new(egui::ColorImage::filled([2, 2], Color32::RED)),
            }),
        );
        let mut output = ctx.run_ui(Default::default(), |ui| {
            super::super::surface::begin_frame(ui.ctx());
            let painter = ui.ctx().layer_painter(egui::LayerId::background());
            assert!(button(
                &painter,
                Rect::from_min_size(egui::pos2(10.0, 10.0), egui::Vec2::splat(30.0)),
                State::Normal,
                Color32::from_rgba_unmultiplied(50, 60, 70, 128),
                3.0
            ));
            super::super::surface::finish_frame(ui.ctx());
        });
        output.textures_delta.clear();
        assert_eq!(
            output.shapes.iter().filter(|shape| matches!(shape.shape, egui::Shape::Circle(_))).count(),
            usize::from(!private_image)
        );
        assert!(
            output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.brush.is_some()))
        );
    }
    // Over opaque surfaces nothing is cleared: the desktop never shows at the artwork's edges.
    super::super::theme::set_palette(&ctx, saved);
    super::super::liquid::set_background_image(&ctx, None);
    let mut output = ctx.run_ui(Default::default(), |ui| {
        super::super::surface::begin_frame(ui.ctx());
        let painter = ui.ctx().layer_painter(egui::LayerId::background());
        let rect = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::Vec2::splat(30.0));
        assert!(button(&painter, rect, State::Normal, Color32::from_rgb(50, 60, 70), 3.0));
        super::super::surface::finish_frame(ui.ctx());
    });
    output.textures_delta.clear();
    assert!(!output.shapes.iter().any(|shape| matches!(shape.shape, egui::Shape::Circle(_))));
    // A translucent hover colour is not a see-through window.
    let mut hover = saved;
    hover.control_hover = Color32::from_rgba_unmultiplied(255, 255, 255, 20);
    super::super::theme::set_palette(&ctx, hover);
    let mut output = ctx.run_ui(Default::default(), |ui| {
        super::super::surface::begin_frame(ui.ctx());
        let painter = ui.ctx().layer_painter(egui::LayerId::background());
        let rect = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::Vec2::splat(30.0));
        assert!(button(&painter, rect, State::Normal, Color32::from_rgb(50, 60, 70), 3.0));
        super::super::surface::finish_frame(ui.ctx());
    });
    output.textures_delta.clear();
    super::super::theme::set_palette(&ctx, saved);
    assert!(!output.shapes.iter().any(|shape| matches!(shape.shape, egui::Shape::Circle(_))));
}
#[test]
fn inherited_svg_renders_missing_roles_while_theme_artwork_overrides_one_role() {
    assert_eq!(DEFAULT_SVGS.len(), ROLES.len());
    for (role, svg) in DEFAULT_SVGS {
        assert!(parse_svg(svg).is_ok(), "{role}");
    }
    let skin = Skin {assets:BTreeMap::from([("play".into(), "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"28\" height=\"28\"><rect width=\"28\" height=\"28\" fill=\"#00ff00\"/></svg>".into())]), ..Default::default()};
    configure(&skin);
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(28.0));
        assert!(paint(ui.painter(), rect, "button", State::Normal, Color32::BLUE, 0.0));
        assert!(paint(ui.painter(), rect, "play", State::Normal, Color32::RED, 0.0));
        assert!(paint(ui.painter(), rect, "stop", State::Normal, Color32::RED, 0.0));
    });
    let pixels: Vec<_> = output
        .textures_delta
        .set
        .values()
        .flat_map(|deltas| deltas.iter())
        .map(|delta| match &delta.image {
            egui::ImageData::Color(image) => image.pixels[14 * 28 + 14],
        })
        .collect();
    for color in [Color32::BLUE, Color32::GREEN, Color32::RED] {
        assert!(pixels.contains(&color));
    }
    assert_eq!(skin.assets.len(), 1, "inheritance must not populate the theme's overrides");
    output.textures_delta.clear();
    configure(&Skin::default());
}
#[test]
fn svg_states_publish_distinct_pixels_and_reuse_cached_textures() {
    let artwork = |color: &str| {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\"><rect width=\"16\" height=\"16\" fill=\"{color}\"/></svg>"
        )
    };
    let skin = Skin {
        assets: [("button".into(), artwork("#FF0000")), ("button.active".into(), artwork("#00FF00"))].into(),
        ..Default::default()
    };
    configure(&skin);
    let ctx = egui::Context::default();
    let mut frame = ctx.run_ui(egui::RawInput::default(), |ui| {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(16.0));
        assert!(paint(ui.painter(), rect, "button", State::Normal, Color32::WHITE, 0.0));
        assert!(paint(ui.painter(), rect, "button", State::Normal, Color32::WHITE, 0.0));
        assert!(paint(ui.painter(), rect, "button", State::Active, Color32::WHITE, 0.0));
        assert!(!paint(ui.painter(), rect, "absent", State::Normal, Color32::WHITE, 0.0));
    });
    let images: Vec<_> = frame
        .textures_delta
        .set
        .values()
        .flat_map(|deltas| deltas.iter())
        .filter(|delta| delta.image.size() == [16, 16])
        .collect();
    assert_eq!(images.len(), 2, "a repeated state should reuse its texture");
    let pixels: Vec<_> = images
        .iter()
        .map(|delta| match &delta.image {
            egui::ImageData::Color(image) => image.pixels[8 * 16 + 8],
        })
        .collect();
    assert!(pixels.contains(&Color32::RED));
    assert!(pixels.contains(&Color32::GREEN));
    frame.textures_delta.clear();
    configure(&Skin::default());
}
#[test]
fn skin_round_trip_and_old_defaults() {
    let skin = Skin {
        css: Some(Arc::from("/* preserved */ @layer test { .browser { color:var(--ink); } }")),
        variant: None,
        background: None,
        background_opacity: 70,
        background_blur: 0,
        styles: Default::default(),
        assets: example_assets(),
        session: Default::default(),
        meter: MeterStyle::Segmented,
        segments: 16,
        meter_intensity: 65,
        artwork_fps: 12,
        toolbar: Some(Toolbar::default()),
        toolbar_offsets: BTreeMap::from([("transport".into(), 80)]),
        controls: BTreeMap::new(),
        variables: Arc::default(),
    };
    let mut warnings = Vec::new();
    assert_eq!(Skin::from_json(&skin.to_json(), &mut warnings), skin);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(Skin::from_json(&json!({}), &mut warnings), Skin::default());
}
#[test]
fn malformed_assets_and_layout_are_bounded() {
    let mut warnings = Vec::new();
    let skin = Skin::from_json(
        &json!({"assets":{"fader.cap":"../secret.svg", "play":"<svg>"},"segments":999,"meter":"oops","toolbar":{"order":["tempo","tempo","bogus"]}}),
        &mut warnings,
    );
    assert!(skin.assets.is_empty());
    assert_eq!(skin.segments, 24);
    assert_eq!(
        skin.toolbar.unwrap().order,
        Toolbar::default().order.into_iter().filter(|s| s != "tempo").fold(vec!["tempo".into()], |mut v, s| {
            v.push(s);
            v
        })
    );
    assert_eq!(warnings.len(), 6);
    for path in ["/tmp/a.svg", "../a.svg", "a/../b.svg", "C:/a.svg", "a\\b.svg", "a//b.svg"] {
        assert!(!asset_path(path));
    }
    for svg in ["<!DOCTYPE svg><svg/>", "<svg><image href=\"file:///tmp/x\"/></svg>"] {
        assert!(parse_svg(svg).is_err());
    }
}

#[test]
fn toolbar_offsets_reject_unknown_modules_and_non_integer_or_unbounded_values() {
    let mut warnings = Vec::new();
    let skin = Skin::from_json(
        &json!({"toolbar-offsets":{"transport":80,"tempo":0.5,"views":16385,"bogus":4}}),
        &mut warnings,
    );
    assert_eq!(skin.toolbar_offsets, BTreeMap::from([("transport".into(), 80)]));
    assert_eq!(warnings.len(), 3);
    assert!(skin.toolbar.is_none());
}
#[test]
fn moving_module_preserves_all_others() {
    let mut toolbar = Toolbar::default();
    toolbar.move_before("views", "tempo");
    assert_eq!(toolbar.order[1], "views");
    assert_eq!(toolbar.order.len(), MODULES.len());
    let before = toolbar.clone();
    toolbar.move_before("views", "views");
    toolbar.move_before("nope", "tempo");
    assert_eq!(toolbar, before);
    toolbar.move_after("browser", "agents");
    assert_eq!(toolbar.order.last().map(String::as_str), Some("browser"));
    assert_eq!(toolbar.order.len(), MODULES.len());
}

#[test]
fn artwork_takes_the_theme_s_variables() {
    let vars: BTreeMap<String, String> = [("--accent".to_owned(), "#ff0000".to_owned())].into();
    let svg = r#"<rect fill="var(--accent)" stroke="var(--line, #00ff00)" opacity="var(--none)"/>"#;
    assert_eq!(
        super::substitute_variables(svg, &vars),
        r##"<rect fill="#ff0000" stroke="#00ff00" opacity="var(--none)"/>"##
    );
    assert!(matches!(super::substitute_variables("<svg/>", &vars), std::borrow::Cow::Borrowed(_)));
}

#[test]
fn svg_text_draws_with_the_bundled_font() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><text x="2" y="16" font-size="16" fill="#ff0000">Hi</text></svg>"##;
    let image = super::raster_artwork(svg, Color32::WHITE, 40, 20, false).unwrap();
    assert!(image.pixels.iter().any(|p| p.r() > 200 && p.a() > 200), "the text should be drawn");
}

#[test]
fn nine_slice_keeps_the_corners_and_stretches_the_middle() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10" data-suhina-slice="2 4"><rect width="20" height="10"/></svg>"#;
    let slice = super::slice_of(svg).unwrap();
    assert_eq!(slice.insets, [2.0, 4.0, 2.0, 4.0]);
    assert_eq!(slice.size, [20.0, 10.0]);
    let mut mesh = egui::Mesh::default();
    super::nine_slice(&mut mesh, Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 30.0)), slice, [0.05, 0.1]);
    assert_eq!(mesh.vertices.len(), 36);
    // The top-left corner stays 4 by 2 and maps to the artwork's own corner.
    assert_eq!(mesh.vertices[3].pos, egui::pos2(4.0, 2.0));
    assert_eq!(mesh.vertices[3].uv, egui::pos2(0.2, 0.2));
    // The stretched middle samples half a texel inside its own pixels: no corner pixel in it.
    let middle = &mesh.vertices[16..20];
    let min_u = middle.iter().map(|v| v.uv.x).fold(f32::MAX, f32::min);
    let min_v = middle.iter().map(|v| v.uv.y).fold(f32::MAX, f32::min);
    assert!((min_u - 0.225).abs() < 1e-4 && (min_v - 0.25).abs() < 1e-4, "{middle:?}");
    assert!(
        super::slice_of(r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4" data-suhina-slice="3"/>"#)
            .is_none()
    );
}

#[test]
fn piano_roll_tools_have_artwork_roles() {
    assert!(valid_role("piano.draw") && valid_role("piano.draw.on") && valid_role("piano.midi-tools.hover"));
    assert!(!valid_role("piano.unknown"));
}

#[test]
fn a_nine_slice_too_big_for_its_rect_shrinks_its_corners_evenly() {
    let slice = super::Slice { insets: [4.0, 4.0, 4.0, 4.0], size: [12.0, 12.0] };
    let mut mesh = egui::Mesh::default();
    super::nine_slice(&mut mesh, Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(100.0, 6.0)), slice, [0.0, 0.0]);
    // The top-left corner is 3 by 3, not 4 wide and 3 high.
    assert_eq!(mesh.vertices[3].pos, egui::pos2(3.0, 3.0));
}
