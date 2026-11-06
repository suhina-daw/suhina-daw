// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Backgrounds that set the window's pixels (`suhina_ui::kit::surface`): each fill is a paint
//! callback drawn with no blending, so its colour and alpha replace what is under it and the
//! window's alpha shows the desktop through. All of a frame's fills share one instance buffer,
//! written once after every callback has been prepared.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use eframe::egui::{self, Color32, CornerRadius, Rect, Shape};
use eframe::{egui_wgpu, wgpu};
use suhina_ui::kit::surface::Provider;

/// Floats in one fill: its rectangle and corner radii in pixels, its colour, the screen's size.
const FLOATS: usize = 16;
const STRIDE: u64 = (FLOATS * std::mem::size_of::<f32>()) as u64;

const SHADER: &str = r"
struct Fill {
    @location(0) rect: vec4<f32>,
    @location(1) radii: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) screen: vec4<f32>,
};

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) rect: vec4<f32>,
    @location(1) radii: vec4<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32, fill: Fill) -> Out {
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(1.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0),
    );
    let at = mix(fill.rect.xy, fill.rect.zw, corners[index]);
    let ndc = at / fill.screen.xy * 2.0 - 1.0;
    var out: Out;
    out.position = vec4(ndc.x, -ndc.y, 0.0, 1.0);
    out.rect = fill.rect;
    out.radii = fill.radii;
    out.color = fill.color;
    return out;
}

@fragment
fn fs_main(in: Out) -> @location(0) vec4<f32> {
    let centre = (in.rect.xy + in.rect.zw) * 0.5;
    let half = (in.rect.zw - in.rect.xy) * 0.5;
    let p = in.position.xy - centre;
    // The radius of the corner this pixel is nearest: nw, ne, sw, se.
    var r = select(in.radii.x, in.radii.y, p.x > 0.0);
    if p.y > 0.0 {
        r = select(in.radii.z, in.radii.w, p.x > 0.0);
    }
    let q = abs(p) - half + r;
    let distance = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - r;
    if distance > 0.0 {
        discard;
    }
    return in.color;
}
";

pub struct Surfaces {
    /// Whether the framebuffer is sRGB, so colours are written linear.
    linear: bool,
}

impl Surfaces {
    /// Builds the pipeline for the window's framebuffer and keeps it with egui's renderer.
    pub fn new(gpu: &egui_wgpu::RenderState) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("background fills"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("background fills"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("background fills"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: STRIDE,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                // No blend state: the fill's colour and alpha replace the pixel's.
                targets: &[Some(wgpu::ColorTargetState {
                    format: gpu.target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        gpu.renderer.write().callback_resources.insert(Resources {
            pipeline,
            buffer: None,
            fills: Vec::new(),
            written: false,
        });
        Self { linear: gpu.target_format.is_srgb() }
    }
}

impl Provider for Surfaces {
    fn shape(&self, rect: Rect, corner_radius: CornerRadius, color: Color32) -> Shape {
        let color = if self.linear {
            egui::Rgba::from(color).to_array()
        } else {
            color.to_array().map(|c| f32::from(c) / 255.0)
        };
        let fill = Fill { rect, corner_radius, color, slot: AtomicU32::new(u32::MAX) };
        Shape::Callback(egui_wgpu::Callback::new_paint_callback(rect, fill))
    }
}

struct Resources {
    pipeline: wgpu::RenderPipeline,
    buffer: Option<(wgpu::Buffer, u64)>,
    /// This frame's fills, as they are prepared.
    fills: Vec<[f32; FLOATS]>,
    /// The fills are in the buffer; the next one prepared starts a new frame.
    written: bool,
}

struct Fill {
    rect: Rect,
    corner_radius: CornerRadius,
    color: [f32; 4],
    /// Where this fill is in the frame's buffer, once prepared.
    slot: AtomicU32,
}

impl egui_wgpu::CallbackTrait for Fill {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(resources) = resources.get_mut::<Resources>() else { return Vec::new() };
        if resources.written {
            resources.fills.clear();
            resources.written = false;
        }
        let Ok(slot) = u32::try_from(resources.fills.len()) else { return Vec::new() };
        let ppp = screen.pixels_per_point;
        // Whole pixels, so neighbouring fills meet without a seam or an overlap.
        let min = (self.rect.min.to_vec2() * ppp).round();
        let max = (self.rect.max.to_vec2() * ppp).round();
        let r = |radius: u8| f32::from(radius) * ppp;
        let c = self.corner_radius;
        let [w, h] = screen.size_in_pixels.map(|n| n as f32);
        let [cr, cg, cb, ca] = self.color;
        resources.fills.push([
            min.x,
            min.y,
            max.x,
            max.y,
            r(c.nw),
            r(c.ne),
            r(c.sw),
            r(c.se),
            cr,
            cg,
            cb,
            ca,
            w.max(1.0),
            h.max(1.0),
            0.0,
            0.0,
        ]);
        self.slot.store(slot, Ordering::Relaxed);
        Vec::new()
    }

    fn finish_prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(resources) = resources.get_mut::<Resources>() else { return Vec::new() };
        if resources.written || resources.fills.is_empty() {
            return Vec::new();
        }
        let bytes: Vec<u8> = resources.fills.iter().flatten().flat_map(|f| f.to_ne_bytes()).collect();
        let needed = bytes.len() as u64;
        if resources.buffer.as_ref().is_none_or(|(_, size)| *size < needed) {
            // Grown in powers of two, so a frame with a few more fills does not make a new one.
            let size = needed.next_power_of_two().max(STRIDE * 256);
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("background fills"),
                size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            resources.buffer = Some((buffer, size));
        }
        if let Some((buffer, _)) = &resources.buffer {
            queue.write_buffer(buffer, 0, &bytes);
        }
        resources.written = true;
        Vec::new()
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let slot = self.slot.load(Ordering::Relaxed);
        let Some(resources) = resources.get::<Resources>() else { return };
        let Some((buffer, _)) = &resources.buffer else { return };
        if slot == u32::MAX || slot as usize >= resources.fills.len() {
            return;
        }
        // egui narrows the viewport to the callback's rectangle; the fill is placed in the
        // whole screen's pixels, and the clip rectangle egui set still cuts it.
        let [w, h] = info.screen_size_px.map(|n| n as f32);
        pass.set_viewport(0.0, 0.0, w, h, 0.0, 1.0);
        pass.set_pipeline(&resources.pipeline);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..6, slot..slot + 1);
    }
}

/// Whether the window's swapchain hands its alpha to the system's compositor, so backgrounds
/// with alpha show the desktop. macOS's Metal layer does; so does DX12 with the swapchain made
/// from a DirectComposition visual (`main.rs` asks for it on Windows). Otherwise a throwaway
/// surface on the same window says whether the GPU can present premultiplied or postmultiplied
/// alpha, the modes egui-wgpu picks for a transparent window.
pub fn composites_alpha(gpu: &egui_wgpu::RenderState, window: impl Into<wgpu::SurfaceTarget<'static>>) -> bool {
    if cfg!(target_os = "macos") {
        return true;
    }
    if gpu.adapter.get_info().backend == wgpu::Backend::Dx12 {
        // A second visual for the same window cannot be made to ask; the visual swapchain is
        // documented to support transparency.
        return cfg!(target_os = "windows");
    }
    let Ok(probe) = gpu.instance.create_surface(window) else { return false };
    let modes = probe.get_capabilities(&gpu.adapter).alpha_modes;
    modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        || modes.contains(&wgpu::CompositeAlphaMode::PostMultiplied)
}

/// The provider for `kit::surface`, shared with egui's context.
pub fn provider(gpu: &egui_wgpu::RenderState) -> Arc<dyn Provider> {
    Arc::new(Surfaces::new(gpu))
}
