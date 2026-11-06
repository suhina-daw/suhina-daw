// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Isolated GPU material experiment, not the DAW's rendering path.
//! Run with `cargo run -p suhina --example glass_preview`.

use eframe::{egui, wgpu};
use wgpu::util::DeviceExt;

const WIDTH: u32 = 768;
const HEIGHT: u32 = 432;

struct GlassPreview {
    gpu: eframe::egui_wgpu::RenderState,
    original: egui::TextureId,
    blurred: egui::TextureId,
    source: wgpu::Texture,
    scratch: wgpu::Texture,
    output: wgpu::Texture,
    pipeline: wgpu::RenderPipeline,
    blur: f32,
    tint: f32,
    enabled: bool,
    stepped: bool,
}

impl GlassPreview {
    fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let gpu = cc.wgpu_render_state.clone().ok_or("This experiment needs the wgpu renderer")?;
        let texture = |label| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: WIDTH, height: HEIGHT, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
        };
        let source = texture("glass backdrop");
        let scratch = texture("glass horizontal");
        let output = texture("glass vertical");
        // An explicit fixed backdrop avoids screenshots, stale frames and self-feedback.
        let pixels = backdrop();
        gpu.queue.write_texture(
            source.as_image_copy(),
            &pixels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(WIDTH * 4), rows_per_image: Some(HEIGHT) },
            source.size(),
        );
        let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glass Gaussian blur"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../../crates/ui/src/kit/glass_blur.wgsl").into()),
        });
        let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glass blur"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let original = gpu.renderer.write().register_native_texture(
            &gpu.device,
            &source.create_view(&Default::default()),
            wgpu::FilterMode::Linear,
        );
        let blurred = gpu.renderer.write().register_native_texture(
            &gpu.device,
            &output.create_view(&Default::default()),
            wgpu::FilterMode::Linear,
        );
        let preview = Self {
            gpu,
            original,
            blurred,
            source,
            scratch,
            output,
            pipeline,
            blur: 3.0,
            tint: 0.5,
            enabled: true,
            stepped: false,
        };
        preview.update_blur();
        Ok(preview)
    }

    fn update_blur(&self) {
        let device = &self.gpu.device;
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glass clamp sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        for (source, target, step) in [
            (&self.source, &self.scratch, [1.0 / WIDTH as f32, 0.0, self.blur, f32::from(self.stepped)]),
            (&self.scratch, &self.output, [0.0, 1.0 / HEIGHT as f32, self.blur, f32::from(self.stepped)]),
        ] {
            let bytes: Vec<u8> = step.into_iter().flat_map(f32::to_ne_bytes).collect();
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("glass step"),
                contents: &bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let view = source.create_view(&Default::default());
            let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("glass pass inputs"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
                ],
            });
            let target = target.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glass separable blur"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bindings, &[]);
            pass.draw(0..3, 0..1);
        }
        self.gpu.queue.submit([encoder.finish()]);
    }
}

impl eframe::App for GlassPreview {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.heading("Suhina / glass material experiment");
        ui.label("Isolated static backdrop. This is not yet the DAW's live UI.");
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.enabled, "Glass");
            if ui.add(egui::Slider::new(&mut self.blur, 0.0..=8.0).text("Blur")).changed() {
                self.update_blur();
            }
            ui.add(egui::Slider::new(&mut self.tint, 0.0..=1.0).text("Tint"));
            if ui.checkbox(&mut self.stepped, "Stepped texture").changed() {
                self.update_blur();
            }
        });
        let width = ui.available_width().min(1100.0);
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(width, width * HEIGHT as f32 / WIDTH as f32), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
        painter.image(self.original, rect, uv, egui::Color32::WHITE);
        let panel = egui::Rect::from_min_size(
            rect.min + rect.size() * egui::vec2(0.42, 0.12),
            rect.size() * egui::vec2(0.52, 0.76),
        );
        if self.enabled {
            let a = (panel.min - rect.min) / rect.size();
            let b = (panel.max - rect.min) / rect.size();
            painter.image(
                self.blurred,
                panel,
                egui::Rect::from_min_max(a.to_pos2(), b.to_pos2()),
                egui::Color32::WHITE,
            );
            painter.rect_filled(panel, 0.0, egui::Color32::from_black_alpha((self.tint * 220.0) as u8));
        } else {
            painter.rect_filled(panel, 0.0, egui::Color32::from_rgb(38, 40, 35));
        }
        painter.rect_stroke(
            panel,
            0.0,
            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(65)),
            egui::StrokeKind::Inside,
        );
        let content = panel.shrink(24.0);
        ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
            ui.heading("Your sound. In focus.");
            ui.label("Background softened. Controls stay sharp.");
            ui.add_space(16.0);
            ui.label("This prototype reuses its textures and only re-runs the GPU blur when Blur changes.");
            ui.add_space(16.0);
            if ui.button("Reset material").clicked() {
                self.blur = 3.0;
                self.tint = 0.5;
                self.stepped = false;
                self.update_blur();
            }
        });
    }
}

fn backdrop() -> Vec<u8> {
    let mut pixels = Vec::with_capacity(WIDTH as usize * HEIGHT as usize * 4);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let mut c = if x % 48 == 0 || y % 54 == 0 { [66, 70, 60, 255] } else { [32, 35, 29, 255] };
            let colors = [[191, 134, 95, 255], [211, 183, 97, 255], [105, 157, 170, 255], [156, 135, 181, 255]];
            for (lane, color) in colors.into_iter().enumerate() {
                let top = 55 + lane as u32 * 85;
                let left = 30 + lane as u32 * 65;
                if y > top && y < top + 55 && x > left && x < left + 470 {
                    c = color;
                    if x % 19 < 2 && y > top + 10 && y < top + 43 {
                        c = [225, 230, 212, 255];
                    }
                }
            }
            pixels.extend_from_slice(&c);
        }
    }
    pixels
}

fn main() -> eframe::Result {
    eframe::run_native(
        "Suhina glass preview",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 760.0]),
            ..Default::default()
        },
        Box::new(|cc| GlassPreview::new(cc).map(|app| Box::new(app) as Box<dyn eframe::App>)),
    )
}
