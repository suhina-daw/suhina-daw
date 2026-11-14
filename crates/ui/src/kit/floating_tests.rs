// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn declarations_last_one_frame() {
    let ctx = Context::default();
    declare(&ctx, Id::unique("drag"), Kind::Own);
    assert_eq!(declared(&ctx).get(&Id::unique("drag")), Some(&Kind::Own));
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
