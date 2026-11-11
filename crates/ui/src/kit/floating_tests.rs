// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn declarations_last_one_frame() {
    let ctx = Context::default();
    declare(&ctx, Id::unique("drag"), Kind::Plain);
    assert_eq!(declared(&ctx).get(&Id::unique("drag")), Some(&Kind::Plain));
    begin_frame(&ctx);
    assert!(declared(&ctx).is_empty());
}

#[test]
fn an_area_fades_in_as_egui_fades_it_and_a_bare_layer_is_shown_whole() {
    let ctx = Context::default();
    assert_eq!(shown(&ctx, Id::unique("painted")), 1.0);
    let fade = f64::from(ctx.global_style().animation_time);
    // The first frame is egui's sizing pass, which paints nothing.
    for (frame, time) in [0.0, fade / 4.0, fade / 2.0, fade, 10.0].into_iter().enumerate() {
        let input = egui::RawInput { time: Some(time), ..Default::default() };
        let mut egui_s = 0.0;
        let mut ours = 0.0;
        let mut output = ctx.run_ui(input, |ui| {
            area(ui.ctx(), Id::unique("menu"), Kind::Popup).show(ui.ctx(), |ui| {
                egui_s = ui.opacity();
                ui.label("Open");
            });
            ours = shown(ui.ctx(), Id::unique("menu"));
        });
        output.textures_delta.clear();
        if frame > 0 {
            assert!((egui_s - ours).abs() < 1e-4, "at {time}: egui {egui_s}, ours {ours}");
        }
    }
}

/// Every floating layer the app paints at the orders egui's popups and tooltips use goes through
/// this module, so none of the app's is taken for a popup.
#[test]
fn the_app_declares_every_layer_it_paints_over_the_window() {
    fn visit(dir: &std::path::Path, found: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs")
                && !path.to_string_lossy().ends_with("_tests.rs")
                && !path.ends_with("kit/floating.rs")
            {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                for (n, line) in text.lines().enumerate() {
                    if line.contains("LayerId::unique(egui::Order::Foreground")
                        || line.contains("LayerId::unique(egui::Order::Tooltip")
                    {
                        found.push(format!("{}:{}", path.display(), n + 1));
                    }
                }
            }
        }
    }
    let mut found = Vec::new();
    visit(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut found);
    assert!(found.is_empty(), "use kit::floating::layer: {found:?}");
}
