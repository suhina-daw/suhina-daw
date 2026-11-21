// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `Widget::Strands` on the GPU (`kit::strands::Provider`): every dot of `kit::strands::GPU_PLAN`
//! drawn as an instanced quad into a texture of the widget's own, behind dots first, then the
//! dots in front, and handed to egui as an image. The dots' places are worked out in
//! `strands.wgsl` from a few numbers per frame, so hundreds of thousands of them cost the CPU
//! nothing.

use std::collections::HashMap;
use std::sync::Mutex;

use egui_wgpu::wgpu;
use suhina_ui::kit::strands::{Frame, GPU_PLAN, Provider, points};
use wgpu::util::DeviceExt;

/// The most pixels a side of a widget's texture has.
const MAX_SIDE: u32 = 2048;
/// Widgets not drawn for this many frames give their texture back.
const IDLE_FRAMES: u64 = 600;

pub struct Strands {
    gpu: egui_wgpu::RenderState,
    state: Mutex<Option<State>>,
}

struct State {
    pipeline: wgpu::RenderPipeline,
    points: wgpu::Buffer,
    count: u32,
    /// The behind pass's and the front pass's uniforms.
    uniforms: [wgpu::Buffer; 2],
    bindings: [wgpu::BindGroup; 2],
    targets: HashMap<egui::Id, Target>,
    frame: u64,
}

struct Target {
    size: [u32; 2],
    texture: wgpu::Texture,
    image: egui::TextureId,
    drawn: u64,
}

impl Strands {
    pub fn new(gpu: egui_wgpu::RenderState) -> Self {
        Self { gpu, state: Mutex::new(None) }
    }
}

impl State {
    fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("strands"),
            source: wgpu::ShaderSource::Wgsl(include_str!("strands.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("strands dots"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &[wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 0,
                        shader_location: 0,
                    }],
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
        let data: Vec<u8> = points(&GPU_PLAN).into_iter().flatten().flat_map(f32::to_ne_bytes).collect();
        let count = (data.len() / 16) as u32;
        let points = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("strands points"),
            contents: &data,
            usage: wgpu::BufferUsages::VERTEX,
        });
        let uniform = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: UNIFORM_BYTES as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let uniforms = [uniform("strands behind"), uniform("strands in front")];
        let layout = pipeline.get_bind_group_layout(0);
        let binding = |buffer: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("strands inputs"),
                layout: &layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
            })
        };
        let bindings = [binding(&uniforms[0]), binding(&uniforms[1])];
        Self { pipeline, points, count, uniforms, bindings, targets: HashMap::new(), frame: 0 }
    }
}

/// Nine vec4s: see `Uniforms` in `strands.wgsl`.
const UNIFORM_BYTES: usize = 9 * 16;

fn uniform_bytes(frame: &Frame, size: [u32; 2], scale: f32, front: bool) -> Vec<u8> {
    let shared = frame.shared(size[0] as f32, size[1] as f32);
    let s = shared.strength;
    let [light, mid, shadow, thread] = frame.colors;
    let values: [f32; 36] = [
        shared.width,
        shared.height,
        shared.mid,
        shared.reach,
        shared.ribbon_width,
        shared.clock,
        shared.swell,
        shared.burst,
        shared.burst_age,
        scale,
        if front { 1.0 } else { 0.0 },
        0.0,
        s[0],
        s[1],
        s[2],
        s[3],
        s[4],
        s[5],
        0.0,
        0.0,
        light[0],
        light[1],
        light[2],
        light[3],
        mid[0],
        mid[1],
        mid[2],
        mid[3],
        shadow[0],
        shadow[1],
        shadow[2],
        shadow[3],
        thread[0],
        thread[1],
        thread[2],
        thread[3],
    ];
    values.into_iter().flat_map(f32::to_ne_bytes).collect()
}

impl Provider for Strands {
    fn draw(&self, ctx: &egui::Context, id: egui::Id, size: [u32; 2], frame: &Frame) -> Option<egui::TextureId> {
        let size = [size[0].clamp(1, MAX_SIDE), size[1].clamp(1, MAX_SIDE)];
        let device = &self.gpu.device;
        let queue = &self.gpu.queue;
        let mut guard = self.state.lock().ok()?;
        let state = guard.get_or_insert_with(|| State::new(device));
        state.frame = state.frame.wrapping_add(1);
        let now = state.frame;
        // Give back the textures of widgets that are no longer drawn.
        let mut renderer = self.gpu.renderer.write();
        state.targets.retain(|_, target| {
            let keep = now.wrapping_sub(target.drawn) < IDLE_FRAMES;
            if !keep {
                renderer.free_texture(&target.image);
            }
            keep
        });
        let fresh = |device: &wgpu::Device| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("strands image"),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        match state.targets.get_mut(&id) {
            Some(target) if target.size == size => target.drawn = now,
            Some(target) => {
                target.texture = fresh(device);
                let view = target.texture.create_view(&Default::default());
                renderer.update_egui_texture_from_wgpu_texture(device, &view, wgpu::FilterMode::Linear, target.image);
                target.size = size;
                target.drawn = now;
            }
            None => {
                let texture = fresh(device);
                let view = texture.create_view(&Default::default());
                let image = renderer.register_native_texture(device, &view, wgpu::FilterMode::Linear);
                state.targets.insert(id, Target { size, texture, image, drawn: now });
            }
        }
        drop(renderer);
        let target = state.targets.get(&id)?;
        let scale = ctx.pixels_per_point();
        for (front, buffer) in [false, true].into_iter().zip(&state.uniforms) {
            queue.write_buffer(buffer, 0, &uniform_bytes(frame, size, scale, front));
        }
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let view = target.texture.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("strands dots"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&state.pipeline);
            pass.set_vertex_buffer(0, state.points.slice(..));
            for binding in &state.bindings {
                pass.set_bind_group(0, binding, &[]);
                pass.draw(0..6, 0..state.count);
            }
        }
        queue.submit([encoder.finish()]);
        Some(target.image)
    }
}

#[cfg(test)]
#[path = "strands_tests.rs"]
mod tests;
