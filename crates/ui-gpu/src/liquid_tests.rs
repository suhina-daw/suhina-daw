// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
#[ignore = "requires a native GPU; run explicitly for rendering changes"]
fn replacing_backgrounds_releases_old_capture_registrations() {
    let instance = wgpu::Instance::default();
    let gpu = wait(egui_wgpu::RenderState::create(&Default::default(), &instance, None, Default::default())).unwrap();
    let liquid = Liquid::new(gpu.clone());
    let ctx = egui::Context::default();
    let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(64.0, 64.0));
    let mut previous = None;
    for index in 1..=20 {
        let id = egui::TextureId::Managed(index);
        gpu.renderer.write().update_texture(
            &gpu.device,
            &gpu.queue,
            id,
            &egui::epaint::ImageDelta::full(egui::ColorImage::filled([8, 8], egui::Color32::RED), Default::default()),
        );
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(area), ..Default::default() }, |ui| {
            ui.painter().image(
                id,
                area,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            assert!(liquid.capture(ui.ctx()).is_some());
        });
        output.textures_delta.clear();
        let state = liquid.state.lock().unwrap();
        assert_eq!(state.shared.len(), 1, "superseded background retained");
        if let Some(old) = previous {
            assert!(state.renderer.texture(&old).is_none(), "old renderer registration retained");
        }
        previous = Some(state.shared[&id].0);
        gpu.renderer.write().free_texture(&id);
    }
    let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(area), ..Default::default() }, |ui| {
        assert!(liquid.capture(ui.ctx()).is_some());
    });
    output.textures_delta.clear();
    let state = liquid.state.lock().unwrap();
    assert!(state.shared.is_empty(), "removing the background must release its capture");
    assert!(state.renderer.texture(&previous.unwrap()).is_none());
}

#[test]
#[ignore = "requires a native GPU; run explicitly for rendering changes"]
fn opening_settings_keeps_a_live_backdrop_across_frames() {
    struct Observed {
        liquid: Liquid,
        captures: Mutex<Vec<bool>>,
        content_captures: Mutex<Vec<bool>>,
    }
    impl Provider for Observed {
        fn capture(&self, ctx: &egui::Context) -> Option<Backdrop> {
            let result = self.liquid.capture(ctx);
            self.captures.lock().unwrap().push(result.is_some());
            result
        }
        fn capture_surfaces(&self, ctx: &egui::Context, shapes: Vec<egui::epaint::ClippedShape>) -> Option<Backdrop> {
            let result = self.liquid.capture_surfaces(ctx, shapes);
            self.content_captures.lock().unwrap().push(result.is_some());
            result
        }
    }
    let instance = wgpu::Instance::default();
    let gpu = wait(egui_wgpu::RenderState::create(&Default::default(), &instance, None, Default::default())).unwrap();
    let host = std::sync::Arc::new(Observed {
        liquid: Liquid::new(gpu),
        captures: Mutex::new(Vec::new()),
        content_captures: Mutex::new(Vec::new()),
    });
    let ctx = egui::Context::default();
    suhina_ui::kit::theme::install(&ctx);
    suhina_ui::kit::liquid::install(&ctx, host.clone());
    let mut app = suhina_ui::App::new();
    app.set_session(suhina_session::Session::new(suhina_ui::demo::demo_project()), &[]);
    let mut settings = suhina_ui::settings::Settings::default();
    settings.view.liquid.panels = 55;
    settings.view.liquid.tracks = 65;
    settings.view.liquid.clips = 75;
    app.set_settings(&ctx, settings);
    for frame in 0..40 {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 850.0))),
            time: Some(f64::from(frame) / 60.0),
            ..Default::default()
        };
        if frame == 2 {
            input.events.push(egui::Event::ModifiersChanged(egui::Modifiers::COMMAND));
            input.events.push(egui::Event::Key {
                key: egui::Key::Comma,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            });
        }
        let mut output = ctx.run_ui(input, |ui| app.ui(ui));
        output.textures_delta.clear();
    }
    let captures = host.captures.lock().unwrap();
    assert!(captures.len() >= 30, "Settings did not open: {captures:?}");
    assert!(captures.iter().all(|captured| *captured), "backdrop alternated with solid fallback: {captures:?}");
    let content = host.content_captures.lock().unwrap();
    assert!(content.len() >= 30 && content.iter().all(|captured| *captured), "content refraction failed: {content:?}");
}

fn wait<F: std::future::Future>(future: F) -> F::Output {
    struct Wake(std::thread::Thread);
    impl std::task::Wake for Wake {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &std::sync::Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(std::sync::Arc::new(Wake(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
        std::thread::park_timeout(std::time::Duration::from_millis(100));
    }
}

fn pixels(liquid: &Liquid) -> Vec<u8> {
    let state = liquid.state.lock().unwrap();
    texture_pixels(&liquid.gpu, &state.targets.as_ref().unwrap().output)
}

fn texture_pixels(gpu: &egui_wgpu::RenderState, texture: &wgpu::Texture) -> Vec<u8> {
    let row = (texture.width() * 4).div_ceil(256) * 256;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-only liquid readback"),
        size: u64::from(row) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
        },
        texture.size(),
    );
    gpu.queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
        let _ = send.send(result);
    });
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receive.recv().unwrap().unwrap();
    let mapped = buffer.slice(..).get_mapped_range().unwrap();
    let result =
        mapped.chunks(row as usize).flat_map(|row| row[..texture.width() as usize * 4].iter().copied()).collect();
    drop(mapped);
    buffer.unmap();
    result
}

#[test]
#[ignore = "requires a native GPU; run explicitly for rendering changes"]
fn gpu_preserves_constants_blurs_in_linear_light_and_updates_current_frame() {
    let instance = wgpu::Instance::default();
    let gpu = wait(egui_wgpu::RenderState::create(&Default::default(), &instance, None, Default::default())).unwrap();
    let liquid = Liquid::new(gpu);
    let ctx = egui::Context::default();
    let draw = |size: f32, pattern: u8| {
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size, size));
        let mut captured = None;
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(area), ..Default::default() }, |ui| {
            let ctx = ui.ctx();
            let painter = ctx.layer_painter(egui::LayerId::background());
            let color = match pattern {
                0 => egui::Color32::from_rgb(100, 150, 200),
                3 => egui::Color32::BLUE,
                _ => egui::Color32::BLACK,
            };
            painter.rect_filled(area, 0.0, color);
            let mut pixels = egui::Mesh::default();
            if pattern == 1 {
                for y in 0..size as u32 {
                    for x in 0..size as u32 {
                        if (x + y) % 2 == 0 {
                            pixels.add_colored_rect(
                                egui::Rect::from_min_size(egui::pos2(x as f32, y as f32), egui::Vec2::splat(1.0)),
                                egui::Color32::WHITE,
                            );
                        }
                    }
                }
            } else if pattern == 2 {
                pixels.add_colored_rect(
                    egui::Rect::from_min_size(egui::pos2(32.0, 32.0), egui::Vec2::splat(1.0)),
                    egui::Color32::WHITE,
                );
            }
            painter.add(pixels);
            captured = liquid.capture(ctx);
        });
        // This test owns the font upload; there is no eframe integration consuming deltas.
        output.textures_delta.clear();
        captured.unwrap()
    };
    let first = draw(64.0, 0);
    assert!(first.sharp_texture.is_some());
    assert_ne!(first.sharp_texture, Some(first.texture));
    let constant = pixels(&liquid);
    let center = (32 * 64 + 32) * 4;
    for (actual, expected) in constant[center..center + 4].iter().zip([100_u8, 150, 200, 255]) {
        assert!(actual.abs_diff(expected) <= 3, "constant colour changed: {actual} vs {expected}");
    }
    draw(64.0, 1);
    let checker = pixels(&liquid);
    assert!((178..=198).contains(&checker[center]), "linear-light mean should be near 188, got {}", checker[center]);
    draw(64.0, 2);
    let impulse = pixels(&liquid);
    assert!(impulse[center] > 0 && impulse[center] < 100);
    assert!(impulse[center - 8].abs_diff(impulse[center + 8]) <= 3);
    let resized = draw(128.0, 3);
    assert_eq!(first.texture, resized.texture);
    assert_eq!(first.sharp_texture, resized.sharp_texture);
    let blue = pixels(&liquid);
    let center = (64 * 128 + 64) * 4;
    assert!(blue[center] < 3 && blue[center + 2] > 250, "old frame leaked into resized output");
}

#[test]
#[ignore = "requires a native GPU; renders review images explicitly"]
fn gpu_refracting_surfaces_preserve_foreground_and_current_background() {
    use suhina_ui::kit::liquid::{self as material, Options, Surface};
    let instance = wgpu::Instance::default();
    let gpu = wait(egui_wgpu::RenderState::create(&Default::default(), &instance, None, Default::default())).unwrap();
    let liquid = std::sync::Arc::new(Liquid::new(gpu.clone()));
    let ctx = egui::Context::default();
    struct Coating;
    impl Provider for Coating {
        fn capture(&self, _: &egui::Context) -> Option<Backdrop> {
            None
        }
    }
    material::install(&ctx, liquid.clone());
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640.0, 400.0));
    let panel = egui::Rect::from_min_size(egui::pos2(80.0, 60.0), egui::vec2(480.0, 280.0));
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/liquid-refraction-review");
    for surface in [Surface::Panel, Surface::Track, Surface::Clip] {
        let folder = folder.join(format!("{surface:?}").to_lowercase());
        std::fs::create_dir_all(&folder).unwrap();
        let mut images = Vec::new();
        for strength in [0, 100] {
            if surface != Surface::Panel && strength == 0 {
                material::install(&ctx, std::sync::Arc::new(Coating));
            } else {
                material::install(&ctx, liquid.clone());
            }
            material::configure(
                &ctx,
                match surface {
                    Surface::Panel => Options { panels: strength, ..Default::default() },
                    Surface::Track => Options { tracks: 100, ..Default::default() },
                    Surface::Clip => Options { clips: 100, ..Default::default() },
                },
            );
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
                use suhina_ui::kit::gradient::{Gradient, Gradients, Stop};
                let gradient = Gradient {
                    angle: 37,
                    stops: vec![
                        Stop { transparent_rgb: None, position: 0, color: egui::Color32::RED },
                        Stop { transparent_rgb: None, position: 40, color: egui::Color32::from_rgb(30, 200, 180) },
                        Stop { transparent_rgb: None, position: 100, color: egui::Color32::BLUE },
                    ],
                };
                material::begin_frame(
                    ui.ctx(),
                    match surface {
                        Surface::Panel => Gradients::default(),
                        Surface::Track => Gradients { tracks: Some(gradient), ..Default::default() },
                        Surface::Clip => Gradients { clips: Some(gradient), ..Default::default() },
                    },
                );
                let painter = ui.painter();
                painter.rect_filled(screen, 0.0, egui::Color32::from_rgb(40, 50, 65));
                for x in (0..640).step_by(20) {
                    painter.vline(
                        x as f32,
                        screen.y_range(),
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(175, 180, 190)),
                    );
                }
                for y in (0..400).step_by(20) {
                    painter.hline(
                        screen.x_range(),
                        y as f32,
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(175, 180, 190)),
                    );
                }
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(50.0, 130.0), egui::vec2(540.0, 50.0)),
                    0.0,
                    egui::Color32::from_rgb(70, 160, 200),
                );
                if surface == Surface::Panel {
                    material::prepare(&ctx);
                }
                let base = egui::Color32::from_rgb(65, 75, 90);
                painter.rect_filled(panel, 0.0, base);
                material::paint(painter, panel, base, surface);
                painter.rect_filled(
                    egui::Rect::from_center_size(panel.center(), egui::vec2(16.0, 16.0)),
                    0.0,
                    egui::Color32::WHITE,
                );
                painter.text(
                    egui::pos2(110.0, 95.0),
                    egui::Align2::LEFT_TOP,
                    "Sharp foreground",
                    egui::FontId::proportional(22.0),
                    egui::Color32::WHITE,
                );
                material::finish_surfaces(ui.ctx());
            });
            let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
            let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("refraction review output"),
                size: wgpu::Extent3d { width: 640, height: 400, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: gpu.target_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let descriptor =
                egui_wgpu::ScreenDescriptor { size_in_pixels: [640, 400], pixels_per_point: output.pixels_per_point };
            let mut renderer = gpu.renderer.write();
            for (id, deltas) in &output.textures_delta.set {
                for delta in deltas {
                    renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
                }
            }
            output.textures_delta.clear();
            let commands = renderer.update_buffers(&gpu.device, &gpu.queue, &mut encoder, &jobs, &descriptor);
            let view = target.create_view(&Default::default());
            {
                let mut pass = encoder
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        color_attachments: &[Some(attachment(&view))],
                        ..Default::default()
                    })
                    .forget_lifetime();
                renderer.render(&mut pass, &jobs, &descriptor);
            }
            drop(renderer);
            gpu.queue.submit(commands.into_iter().chain([encoder.finish()]));
            let rgba = texture_pixels(&gpu, &target);
            let mut ppm = b"P6\n640 400\n255\n".to_vec();
            for pixel in rgba.as_chunks::<4>().0 {
                if matches!(gpu.target_format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb) {
                    ppm.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
                } else {
                    ppm.extend_from_slice(&pixel[..3]);
                }
            }
            std::fs::write(folder.join(if strength == 0 { "flat.ppm" } else { "glass.ppm" }), ppm).unwrap();
            images.push(rgba);
        }
        let pixel = |x: usize, y: usize| (y * 640 + x) * 4;
        let center = pixel(320, 200);
        assert_eq!(&images[0][center..center + 4], &images[1][center..center + 4], "foreground changed");
        let outside = pixel(30, 30);
        assert_eq!(&images[0][outside..outside + 4], &images[1][outside..outside + 4], "lens escaped its panel");
        assert!(images[0].iter().zip(&images[1]).filter(|(a, b)| a != b).count() > 1000, "no visible material");
    }
}

#[test]
fn backdrop_size_is_bounded_and_handles_dpi() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0));
    assert_eq!(target_size(screen, 2.0), Some([1024, 576]));
    assert_eq!(target_size(screen, f32::NAN), None);
    assert_eq!(target_size(egui::Rect::NOTHING, 1.0), None);
}
