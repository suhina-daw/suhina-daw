// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exercise the nested native viewport callback, rather than the embedded Window fallback.
use super::*;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn native_colour_window_paints_its_dark_background_and_settles_without_input() {
    let trace = tempfile::NamedTempFile::new().unwrap();
    suhina_plugins::trace::start(trace.path());
    let ctx = egui::Context::default();
    theme::install(&ctx);
    ctx.set_embed_viewports(false);
    let frames = Rc::new(RefCell::new(Vec::new()));
    let captured = frames.clone();
    egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
        let size = viewport.builder.inner_size.unwrap();
        let position = viewport.builder.position.unwrap();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let mut input = egui::RawInput {
            viewport_id: viewport.ids.this,
            screen_rect: Some(rect),
            time: Some(ctx.input(|i| i.time)),
            ..Default::default()
        };
        input.viewports.insert(
            viewport.ids.this,
            egui::ViewportInfo {
                parent: Some(viewport.ids.parent),
                inner_rect: Some(egui::Rect::from_min_size(position + egui::vec2(0.0, 32.0), size)),
                outer_rect: Some(egui::Rect::from_min_size(position, size + egui::vec2(0.0, 32.0))),
                focused: Some(true),
                native_pixels_per_point: Some(1.0),
                ..Default::default()
            },
        );
        // The same nested run as eframe::render_immediate_viewport, including multipass.
        let mut output = ctx.run_ui(input, |ui| (viewport.viewport_ui_cb)(ui));
        // This harness inspects shapes without a GPU; acknowledge texture uploads explicitly.
        output.textures_delta.clear();
        captured.borrow_mut().push((rect, output));
    });
    let mut app = demo_app();
    app.use_theme(&ctx, "suhina-studio");
    app.run(&Action::new("ui.colourPicker", json!({"tracks":[1]})));
    let root = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(SIZE.0, SIZE.1));
    for step in 0..24 {
        let mut input = egui::RawInput {
            screen_rect: Some(root),
            time: Some(f64::from(step) / 10.0),
            focused: false,
            ..Default::default()
        };
        input.viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                inner_rect: Some(root),
                outer_rect: Some(root),
                focused: Some(false),
                native_pixels_per_point: Some(1.0),
                ..Default::default()
            },
        );
        ctx.run_ui(input, |ui| app.ui(ui)).textures_delta.clear();
    }
    let expected = live("panel-background");
    let trace = std::fs::read_to_string(trace.path()).unwrap();
    let frames = frames.borrow();
    assert!(frames.len() >= 24);
    for (rect, frame) in frames.iter().rev().take(8) {
        let mut fills: Vec<_> = frame
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(fill) => Some((fill.rect, fill.fill, shape.clip_rect)),
                _ => None,
            })
            .collect();
        fills.sort_by(|a, b| b.0.area().total_cmp(&a.0.area()));
        fills.truncate(8);
        assert!(
            frame.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Rect(fill) if fill.fill == expected && fill.rect.contains_rect(*rect)
                    && shape.clip_rect.contains_rect(*rect)
            )),
            "native client area {rect:?} must paint the theme background {expected:?}; largest fills: {fills:?}; trace: {trace}"
        );
    }
    let last = &frames.last().unwrap().1.shapes;
    for (_, frame) in frames.iter().rev().skip(1).take(7) {
        assert_eq!(&frame.shapes, last, "idle native content must not oscillate");
    }
}
