// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
#[ignore = "manual performance comparison; run without a simultaneous interactive test"]
fn measure_short_moving_meter_glow() {
    for glow in [false, true] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        super::super::skin::configure(&super::super::skin::Skin {
            css: Some(
                if glow {
                    ".meter-level { box-shadow: 0 0 2px #83d82b55; }"
                } else {
                    ".meter-level { color: #83d82b; }"
                }
                .into(),
            ),
            ..Default::default()
        });
        let mut times = Vec::new();
        let mut uploads = 0;
        for frame in 0..24 {
            let started = std::time::Instant::now();
            let mut output =
                ctx.run_ui(egui::RawInput { time: Some(frame as f64 * 0.02), ..Default::default() }, |ui| {
                    let _frame = super::super::native_css::frame(ui);
                    for track in 0..20 {
                        let amplitude = 0.0004 + (frame * 20 + track) as f32 * 0.000001;
                        show(
                            ui,
                            ui.painter(),
                            Rect::from_min_size(Pos2::new(20.0 + track as f32 * 20.0, 20.0), Vec2::new(10.0, 38.0)),
                            "test",
                            Source::Track(TrackId(track)),
                            &Level { peak: [amplitude; 2], rms: [amplitude * 0.5; 2], clipped: false },
                            &mut Vec::new(),
                        );
                    }
                });
            if frame > 3 {
                times.push(started.elapsed().as_secs_f64() * 1000.0);
                uploads += output.textures_delta.set.len();
            }
            output.textures_delta.clear();
        }
        times.sort_by(f64::total_cmp);
        eprintln!("short moving meters glow={glow}: median {:.3} ms, uploads {uploads}", times[10]);
    }
}

#[test]
fn intensity_dims_native_ink_without_changing_geometry_or_clip_light() {
    use crate::kit::{native_css, skin};
    for meter in [skin::MeterStyle::Continuous, skin::MeterStyle::Segmented, skin::MeterStyle::Needle] {
        for upright in [true, false] {
            let render = |intensity, css: Option<&str>| {
                let ctx = egui::Context::default();
                theme::install(&ctx);
                skin::configure(&skin::Skin {
                    meter,
                    meter_intensity: intensity,
                    css: css.map(Into::into),
                    ..Default::default()
                });
                let mut ink = Vec::new();
                for frame in 0..3 {
                    let mut output =
                        ctx.run_ui(egui::RawInput { time: Some(frame as f64 * 0.01), ..Default::default() }, |ui| {
                            let _frame = native_css::frame(ui);
                            let size = if upright { Vec2::new(12.0, 100.0) } else { Vec2::new(100.0, 12.0) };
                            show(
                                ui,
                                ui.painter(),
                                Rect::from_min_size(Pos2::new(20.0, 20.0), size),
                                "Main",
                                Source::Main,
                                &Level { peak: [0.5; 2], rms: [0.2; 2], clipped: true },
                                &mut Vec::new(),
                            );
                        });
                    output.textures_delta.clear();
                    if frame == 2 {
                        ink = output
                            .shapes
                            .into_iter()
                            .filter_map(|shape| match shape.shape {
                                egui::Shape::Rect(rect) => Some((rect.rect, rect.fill)),
                                egui::Shape::LineSegment { points, stroke } => {
                                    Some((Rect::from_two_pos(points[0], points[1]), stroke.color))
                                }
                                _ => None,
                            })
                            .collect();
                    }
                }
                ink
            };
            let full = render(100, None);
            let dim = render(50, None);
            let opacity = render(100, Some(".meter-level { opacity: 0.5; }"));
            assert_eq!(dim, opacity, "native CSS and theme intensity agree");
            let zero = render(0, None);
            assert_eq!(zero, render(100, Some(".meter-level { opacity: 0; }")));
            assert!(zero.iter().any(|(_, color)| *color == theme::recording()));
            assert_eq!(full.len(), dim.len());
            let mut changed = 0;
            for ((bounds, color), (dim_bounds, dim_color)) in full.iter().zip(&dim) {
                assert_eq!(bounds, dim_bounds);
                if color != dim_color {
                    changed += 1;
                    assert_eq!(*dim_color, color.gamma_multiply(0.5));
                }
            }
            assert!(changed >= 2, "both channels dim");
            assert!(dim.iter().any(|(_, color)| *color == theme::recording()), "clip warning stays visible");
            if meter != skin::MeterStyle::Needle {
                let transparent = render(100, Some(".meter-level { background-color: transparent; }"));
                assert!(
                    !transparent.iter().any(|(_, color)| *color == theme::meter_normal().gamma_multiply(0.45)),
                    "transparent fill must not revive native peak ink"
                );
            }
        }
    }
    skin::configure(&skin::Skin::default());
}

#[test]
fn css_meter_ramps_keep_the_full_scale_anchor_when_signal_and_orientation_change() {
    use crate::kit::{native_css, skin};
    for upright in [true, false] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        skin::configure(&skin::Skin { meter_intensity: 50, css: Some(".meter-vertical .meter-level { background: linear-gradient(to top, #00ff00, #ffff00 80%, #ff0000); } .meter-horizontal .meter-level { background: linear-gradient(to right, #00ff00, #ffff00 80%, #ff0000); }".into()), ..Default::default() });
        let mut observed = Vec::new();
        for frame in 0..4 {
            let peak = if frame == 3 { 1.0 } else { 0.1 };
            let mut output =
                ctx.run_ui(egui::RawInput { time: Some(frame as f64 * 0.01), ..Default::default() }, |ui| {
                    let _frame = native_css::frame(ui);
                    let size = if upright { egui::vec2(12.0, 100.0) } else { egui::vec2(100.0, 12.0) };
                    show(
                        ui,
                        ui.painter(),
                        Rect::from_min_size(egui::pos2(20.0, 20.0), size),
                        "Main",
                        Source::Main,
                        &Level { peak: [peak; 2], rms: [peak * 0.5; 2], ..Default::default() },
                        &mut Vec::new(),
                    );
                });
            if frame >= 2 {
                observed.push(output.shapes.iter().find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) => {
                        Some((mesh.texture_id, mesh.calc_bounds(), shape.clip_rect, mesh.vertices[0].color.a()))
                    }
                    _ => None,
                }));
            }
            output.textures_delta.clear();
        }
        let low = observed[0].as_ref().expect("quiet gradient");
        let high = observed[1].as_ref().expect("loud gradient");
        assert_eq!((low.0, low.1), (high.0, high.1), "material must not move or stretch with signal");
        assert!((55..=58).contains(&low.3), "native dim peak alpha combines with theme intensity");
        if upright {
            assert!(high.2.top() < low.2.top());
        } else {
            assert!(high.2.right() > low.2.right());
        }
    }
    skin::configure(&skin::Skin::default());
}

#[test]
fn custom_frames_follow_signal_and_keep_native_clip_reset() {
    let ctx = egui::Context::default();
    theme::install(&ctx);
    let mut skin = super::super::skin::Skin {
        meter: super::super::skin::MeterStyle::Frames,
        meter_intensity: 50,
        ..Default::default()
    };
    for (index, color) in [(0, "#ff0000"), (1, "#00ff00")] {
        skin.assets.insert(format!("meter.frame-{index}"), format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="{color}"/></svg>"##));
    }
    super::super::skin::configure(&skin);
    let pos = egui::pos2(30.0, 30.0);
    let mut actions = Vec::new();
    for frame in 0..4 {
        let events = match frame {
            2 | 3 => vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: frame == 2,
                modifiers: egui::Modifiers::default(),
            }],
            _ => vec![egui::Event::PointerMoved(pos)],
        };
        let mut output =
            ctx.run_ui(egui::RawInput { events, time: Some(frame as f64 * 0.1), ..Default::default() }, |ui| {
                show(
                    ui,
                    ui.painter(),
                    Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(100.0, 40.0)),
                    "Main",
                    Source::Main,
                    &Level {
                        peak: if frame == 0 { [0.0, 0.0] } else { [0.0, 4.0] },
                        clipped: true,
                        ..Default::default()
                    },
                    &mut actions,
                );
            });
        if frame < 2 {
            let expected = if frame == 0 { egui::Color32::RED } else { egui::Color32::GREEN };
            let found = output.textures_delta.set.values().flatten().any(|delta| match &delta.image {
                egui::ImageData::Color(image) => image.pixels.iter().all(|pixel| *pixel == expected),
            });
            output.textures_delta.clear();
            assert!(found, "missing signal frame {frame}");
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                    egui::Shape::Mesh(mesh) if mesh.vertices.iter().all(|vertex| (127..=128).contains(&vertex.color.a()))
                )), "frame artwork respects intensity");
        }
    }
    assert_eq!(actions, vec![Action::new("mixer.resetClip", json!({"main": true}))]);
}

#[test]
fn silent_rails_survive_parent_gradients_and_accept_their_own_background() {
    use crate::kit::{native_css, skin, style_class};
    for override_color in [None, Some("#081014")] {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let native = theme::palette().window_background;
        let css = format!(
            ".session {{ background: linear-gradient(red, blue); }} {}",
            override_color.map_or(String::new(), |color| format!(".meter {{ background-color: {color}; }}"))
        );
        skin::configure(&skin::Skin { css: Some(css.into()), ..Default::default() });
        for frame in 0..3 {
            let mut output =
                ctx.run_ui(egui::RawInput { time: Some(frame as f64 * 0.01), ..Default::default() }, |ui| {
                    let _frame = native_css::frame(ui);
                    let _parent = style_class::enter("session");
                    show(
                        ui,
                        ui.painter(),
                        Rect::from_min_size(Pos2::new(20.0, 20.0), Vec2::new(WIDTH, 100.0)),
                        "Main",
                        Source::Main,
                        &Level::default(),
                        &mut Vec::new(),
                    );
                });
            if frame == 2 {
                let expected = override_color.and_then(theme::parse_hex).unwrap_or(native);
                let rails = output
                    .shapes
                    .iter()
                    .filter(|shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == expected));
                assert_eq!(rails.count(), 3, "two silent rails and the unlit clip light");
            }
            output.textures_delta.clear();
        }
        skin::configure(&skin::Skin::default());
    }
}

#[test]
fn the_scale_is_the_volume_controls() {
    assert_eq!(fraction(0.0), 0.0);
    assert_eq!(fraction(f32::NAN), 0.0);
    assert!((fraction(1.0) - 70.0 / 76.0).abs() < 1e-6);
    assert_eq!(fraction(4.0), 1.0);
    // -3 dB is where the top 12 % begin.
    let hot = fraction(10f32.powf(-3.0 / 20.0));
    assert!((hot - 0.88).abs() < 0.005, "{hot}");
}

#[test]
fn the_description_names_the_louder_side_and_the_clip() {
    let level = Level { peak: [0.25, 0.5], rms: [0.1, 0.1], clipped: false };
    assert_eq!(describe("Drums", &level), "Drums level: -6.0 dB");
    assert_eq!(describe("Main", &Level { clipped: true, ..Level::default() }), "Main level: silent, clipped");
}
