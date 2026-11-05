// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The waveform drawn at several zooms, rendered to `target/ui-snapshots/waveforms.png` for
//! looking at: `cargo test -p suhina-ui --test waveforms -- --ignored`. The shapes themselves are
//! checked without a GPU in `src/waveform/tests.rs`.

use std::path::Path;
use std::sync::Arc;

use egui::{Color32, Pos2, Rect, vec2};
use egui_kittest::Harness;
use suhina_engine::AudioData;
use suhina_ui::kit::theme;
use suhina_ui::kit::waveform::{Colors, Scale, geometry, paint, peaks, steady_columns};

/// Two seconds of stereo at 8 kHz: a decaying chord on the left, a quieter noise burst on the
/// right.
fn sample() -> Arc<AudioData> {
    let rate = 8000u32;
    let mut x = 1u32;
    let mut samples = Vec::new();
    for frame in 0..rate * 2 {
        let t = f64::from(frame) / f64::from(rate);
        let decay = (-t * 2.0).exp();
        let chord = [110.0, 138.6, 164.8].iter().map(|f| (t * f * std::f64::consts::TAU).sin()).sum::<f64>() / 3.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let noise = if (0.5..1.2).contains(&t) { (x as f32 / u32::MAX as f32 - 0.5) * 0.4 } else { 0.0 };
        samples.push((chord * decay) as f32);
        samples.push(noise);
    }
    Arc::new(AudioData { sample_rate: rate, channels: 2, samples })
}

#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_waveforms_at_several_zooms() {
    let peaks = Arc::new(peaks(&sample()));
    // (seconds from, seconds per point, height, scale) per row.
    let rows = [
        (0.0, 2.0 / 600.0, 70.0, Scale::Linear),
        (0.0, 2.0 / 600.0, 30.0, Scale::Linear),
        (0.0, 2.0 / 600.0, 70.0, Scale::Db),
        (0.4, 0.2 / 600.0, 70.0, Scale::Linear),
        (0.5, 0.02 / 600.0, 70.0, Scale::Linear),
        (0.5, 0.003 / 600.0, 70.0, Scale::Linear),
    ];
    let mut harness =
        Harness::builder().allow_missing_glyphs().with_size((640.0, 500.0)).with_pixels_per_point(2.0).wgpu().build_ui(
            |ui| {
                let mut y = 10.0;
                for (from, per_point, height, scale) in rows {
                    let rect = Rect::from_min_size(Pos2::new(20.0, y), vec2(600.0, height));
                    ui.painter().rect_filled(rect, 0.0, theme::control_background());
                    let columns = steady_columns(rect.left(), 600, from, per_point, 1.0);
                    let shape = geometry(&peaks, rect, &columns, scale);
                    paint(ui.painter(), &shape, Colors::from(Color32::from_rgb(0x8B, 0xC4, 0x8A)));
                    y += height + 12.0;
                }
            },
        );
    theme::install(&harness.ctx);
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&out).unwrap_or_else(|e| panic!("{e}"));
    image.save(out.join("waveforms.png")).unwrap_or_else(|e| panic!("{e}"));
}
