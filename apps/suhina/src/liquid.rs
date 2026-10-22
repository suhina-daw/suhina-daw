// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Experimental, current-frame modal backdrop. Appearance strengths default to zero.
//! Own offscreen renderer and targets: never sample the swapchain or a previous UI frame.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use eframe::{egui, egui_wgpu, wgpu};
use suhina_ui::kit::liquid::{Backdrop, Provider};
use wgpu::util::DeviceExt;

const MAX_SIDE: f32 = 1024.0;

pub struct Liquid {
    gpu: egui_wgpu::RenderState,
    state: Mutex<State>,
    surface_state: Mutex<Option<State>>,
    /// The menus' backdrop: targets of its own, so a menu and a dialog never share an image.
    menu_state: Mutex<Option<State>>,
    resource_report: Mutex<Option<std::time::Instant>>,
}

struct State {
    renderer: egui_wgpu::Renderer,
    targets: Option<Targets>,
    horizontal: wgpu::RenderPipeline,
    vertical: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    font: Option<egui::ColorImage>,
    /// Textures drawn elsewhere, bound in this renderer: the main renderer's id, this renderer's
    /// id, and what it was bound from (rebound when that changes).
    shared: HashMap<egui::TextureId, (egui::TextureId, Source)>,
}

/// Where a shared texture comes from: one of egui's in the main renderer, or another capture's
/// image as the main renderer shows it.
#[derive(Clone, PartialEq)]
enum Source {
    Texture(wgpu::Texture),
    View(wgpu::TextureView),
}

struct Targets {
    size: [u32; 2],
    source: wgpu::Texture,
    scratch: wgpu::Texture,
    output: wgpu::Texture,
    image: egui::TextureId,
    sharp_image: egui::TextureId,
    horizontal: wgpu::BindGroup,
    vertical: wgpu::BindGroup,
}

impl State {
    fn new(gpu: &egui_wgpu::RenderState) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("liquid linear Gaussian"),
            source: wgpu::ShaderSource::Wgsl(include_str!("liquid.wgsl").into()),
        });
        let pipeline = |format, entry| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("liquid blur"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
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
            })
        };
        State {
            renderer: egui_wgpu::Renderer::new(device, wgpu::TextureFormat::Rgba8UnormSrgb, Default::default()),
            targets: None,
            horizontal: pipeline(wgpu::TextureFormat::Rgba8UnormSrgb, "linear_blur"),
            vertical: pipeline(wgpu::TextureFormat::Rgba8Unorm, "encoded_blur"),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("liquid clamp"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            font: None,
            shared: HashMap::new(),
        }
    }

    /// This renderer's id for the texture `id` of the main renderer, binding it the first time
    /// (or again when it changed). `None` when it cannot be drawn here: a native texture nobody
    /// here made, or this capture's own target.
    fn share(
        &mut self,
        device: &wgpu::Device,
        main: &egui_wgpu::Renderer,
        own: &[(egui::TextureId, wgpu::TextureView)],
        id: egui::TextureId,
    ) -> Option<egui::TextureId> {
        let source = match own.iter().find(|(own, _)| *own == id) {
            Some((_, view)) => Source::View(view.clone()),
            None => Source::Texture(main.texture(&id)?.texture.clone()?),
        };
        if let Some((shared, bound)) = self.shared.get(&id)
            && *bound == source
        {
            return Some(*shared);
        }
        let view = match &source {
            Source::View(view) => view.clone(),
            Source::Texture(texture) => texture.create_view(&Default::default()),
        };
        let shared = match self.shared.get(&id) {
            Some((shared, _)) => {
                self.renderer.update_egui_texture_from_wgpu_texture(device, &view, wgpu::FilterMode::Linear, *shared);
                *shared
            }
            None => self.renderer.register_native_texture(device, &view, wgpu::FilterMode::Linear),
        };
        self.shared.insert(id, (shared, source));
        Some(shared)
    }
}

/// A capture's images as the main renderer shows them, for another capture to draw.
fn images(targets: Option<&Targets>) -> Vec<(egui::TextureId, wgpu::TextureView)> {
    let Some(targets) = targets else { return Vec::new() };
    vec![
        (targets.image, targets.output.create_view(&Default::default())),
        (
            targets.sharp_image,
            targets.source.create_view(&wgpu::TextureViewDescriptor {
                format: Some(wgpu::TextureFormat::Rgba8Unorm),
                ..Default::default()
            }),
        ),
    ]
}

impl Liquid {
    pub fn new(gpu: egui_wgpu::RenderState) -> Self {
        let state = Mutex::new(State::new(&gpu));
        Self {
            gpu,
            state,
            surface_state: Mutex::new(None),
            menu_state: Mutex::new(None),
            resource_report: Mutex::new(None),
        }
    }

    fn trace_resources(&self) {
        if !suhina_plugins::trace::on() {
            return;
        }
        let Ok(mut report) = self.resource_report.try_lock() else { return };
        if report.is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(1)) {
            return;
        }
        *report = Some(std::time::Instant::now());
        let mut captures = 0usize;
        let mut shared = 0usize;
        let mut target_bytes = 0u64;
        let mut count = |state: &State| {
            shared += state.shared.len();
            if let Some(targets) = &state.targets {
                captures += 1;
                target_bytes += u64::from(targets.size[0]) * u64::from(targets.size[1]) * 4 * 3;
            }
        };
        if let Ok(state) = self.state.try_lock() {
            count(&state);
        }
        for state in [&self.surface_state, &self.menu_state] {
            if let Ok(state) = state.try_lock()
                && let Some(state) = state.as_ref()
            {
                count(state);
            }
        }
        suhina_plugins::trace::state(
            "liquid resources",
            format!("{captures} target sets, {target_bytes} target bytes, {shared} shared registrations"),
        );
    }
}

fn target_size(screen: egui::Rect, scale: f32) -> Option<[u32; 2]> {
    if !screen.is_finite() || screen.width() <= 0.0 || screen.height() <= 0.0 || !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let physical = screen.size() * scale;
    if !physical.is_finite() {
        return None;
    }
    let factor = (MAX_SIDE / physical.max_elem()).min(1.0);
    Some([(physical.x * factor).ceil().max(1.0) as u32, (physical.y * factor).ceil().max(1.0) as u32])
}

impl Targets {
    fn new(
        gpu: &egui_wgpu::RenderState,
        state: &State,
        size: [u32; 2],
        previous: Option<(egui::TextureId, egui::TextureId)>,
    ) -> Self {
        let device = &gpu.device;
        let texture = |label, format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: if format == wgpu::TextureFormat::Rgba8UnormSrgb {
                    &[wgpu::TextureFormat::Rgba8Unorm]
                } else {
                    &[]
                },
            })
        };
        let source = texture("liquid current UI", wgpu::TextureFormat::Rgba8UnormSrgb);
        let scratch = texture("liquid horizontal", wgpu::TextureFormat::Rgba8UnormSrgb);
        let output = texture("liquid encoded output", wgpu::TextureFormat::Rgba8Unorm);
        let bindings = |pipeline: &wgpu::RenderPipeline, source: &wgpu::Texture, step: [f32; 4]| {
            let bytes: Vec<_> = step.into_iter().flat_map(f32::to_ne_bytes).collect();
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("liquid blur step"),
                contents: &bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("liquid blur inputs"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&source.create_view(&Default::default())),
                    },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&state.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
                ],
            })
        };
        let horizontal = bindings(&state.horizontal, &source, [1.0 / size[0] as f32, 0.0, 3.0, 0.0]);
        let vertical = bindings(&state.vertical, &scratch, [0.0, 1.0 / size[1] as f32, 3.0, 0.0]);
        let view = output.create_view(&Default::default());
        let mut renderer = gpu.renderer.write();
        let image = if let Some((id, _)) = previous {
            renderer.update_egui_texture_from_wgpu_texture(device, &view, wgpu::FilterMode::Linear, id);
            id
        } else {
            renderer.register_native_texture(device, &view, wgpu::FilterMode::Linear)
        };
        // Reinterpret the encoded source without sRGB decoding, matching egui's image sampler.
        let sharp_view = source.create_view(&wgpu::TextureViewDescriptor {
            format: Some(wgpu::TextureFormat::Rgba8Unorm),
            ..Default::default()
        });
        let sharp_image = if let Some((_, id)) = previous {
            renderer.update_egui_texture_from_wgpu_texture(device, &sharp_view, wgpu::FilterMode::Linear, id);
            id
        } else {
            renderer.register_native_texture(device, &sharp_view, wgpu::FilterMode::Linear)
        };
        Self { size, source, scratch, output, image, sharp_image, horizontal, vertical }
    }
}

impl Provider for Liquid {
    fn capture(&self, ctx: &egui::Context) -> Option<Backdrop> {
        self.trace_resources();
        suhina_plugins::trace::timed("liquid backdrop", || self.capture_frame(ctx))
    }

    fn capture_surfaces(&self, ctx: &egui::Context, shapes: Vec<egui::epaint::ClippedShape>) -> Option<Backdrop> {
        self.trace_resources();
        suhina_plugins::trace::timed("liquid content substrate", || {
            let own = self.images_except(Which::Surfaces);
            let mut state = self.surface_state.lock().ok()?;
            let state = state.get_or_insert_with(|| State::new(&self.gpu));
            self.render_frame(ctx, state, shapes, false, false, &own)
        })
    }

    fn capture_behind(
        &self,
        ctx: &egui::Context,
        shapes: Vec<egui::epaint::ClippedShape>,
        see_through: bool,
    ) -> Option<Backdrop> {
        suhina_plugins::trace::timed("liquid menu backdrop", || {
            let own = self.images_except(Which::Menus);
            let mut state = self.menu_state.lock().ok()?;
            let state = state.get_or_insert_with(|| State::new(&self.gpu));
            self.render_frame(ctx, state, shapes, true, see_through, &own)
        })
    }
}

/// The three captures, each drawing into targets of its own.
#[derive(Clone, Copy, PartialEq)]
enum Which {
    Dialog,
    Surfaces,
    Menus,
}

impl Liquid {
    fn capture_frame(&self, ctx: &egui::Context) -> Option<Backdrop> {
        // Read painted shapes at this point in this frame, before the modal layer exists.
        // Callback-based foreign rendering cannot safely be replayed into our target.
        let order: Vec<_> = ctx.memory(|memory| memory.layer_ids().collect());
        let transforms =
            order.iter().filter_map(|layer| ctx.layer_transform_to_global(*layer).map(|t| (*layer, t))).collect();
        let shapes = ctx.graphics(|layers| layers.clone().drain(&order, &transforms));
        let own = self.images_except(Which::Dialog);
        let mut state = self.state.lock().ok()?;
        self.render_frame(ctx, &mut state, shapes, true, false, &own)
    }

    /// The images of the captures other than `except`, which is being drawn into.
    fn images_except(&self, except: Which) -> Vec<(egui::TextureId, wgpu::TextureView)> {
        let mut all = Vec::new();
        if except != Which::Dialog
            && let Ok(state) = self.state.try_lock()
        {
            all.extend(images(state.targets.as_ref()));
        }
        for (which, state) in [(Which::Surfaces, &self.surface_state), (Which::Menus, &self.menu_state)] {
            if which != except
                && let Ok(state) = state.try_lock()
            {
                all.extend(images(state.as_ref().and_then(|state| state.targets.as_ref())));
            }
        }
        all
    }

    fn render_frame(
        &self,
        ctx: &egui::Context,
        state: &mut State,
        shapes: Vec<egui::epaint::ClippedShape>,
        blur: bool,
        // Nothing under the shapes, so the image keeps the window's alpha.
        see_through: bool,
        own: &[(egui::TextureId, wgpu::TextureView)],
    ) -> Option<Backdrop> {
        let screen = ctx.content_rect();
        let size = target_size(screen, ctx.pixels_per_point())?;
        if state.targets.as_ref().is_none_or(|targets| targets.size != size) {
            let previous = state.targets.as_ref().map(|targets| (targets.image, targets.sharp_image));
            state.targets = Some(Targets::new(&self.gpu, state, size, previous));
        }
        let mut jobs = ctx.tessellate(shapes, ctx.pixels_per_point());
        let device = &self.gpu.device;
        let queue = &self.gpu.queue;
        let font = ctx.fonts(|fonts| fonts.image());
        if state.font.as_ref() != Some(&font) {
            state.renderer.update_texture(
                device,
                queue,
                egui::TextureId::Managed(0),
                &egui::epaint::ImageDelta::full(font.clone(), Default::default()),
            );
            state.font = Some(font);
        }
        // Text uses the atlas uploaded above; every other texture is bound from where it is drawn
        // (egui's in the main renderer, another capture's image). What cannot be drawn here (a
        // native callback, a texture nobody here can bind) is left out; the rest still draws.
        let before = jobs.len();
        let mut used = HashSet::new();
        {
            let main = self.gpu.renderer.read();
            jobs.retain_mut(|job| match &mut job.primitive {
                egui::epaint::Primitive::Callback(_) => false,
                egui::epaint::Primitive::Mesh(mesh) if mesh.texture_id == egui::TextureId::Managed(0) => true,
                egui::epaint::Primitive::Mesh(mesh) => match state.share(device, &main, own, mesh.texture_id) {
                    Some(id) => {
                        used.insert(mesh.texture_id);
                        mesh.texture_id = id;
                        true
                    }
                    None => false,
                },
            });
        }
        // Registrations own GPU views as well as the source handles in `shared`. A new
        // background has a new ID, so retaining past captures would keep every old image alive.
        state.shared.retain(|id, (registered, _)| {
            if used.contains(id) {
                true
            } else {
                state.renderer.free_texture(registered);
                false
            }
        });
        if suhina_plugins::trace::on() {
            let left_out = before - jobs.len();
            suhina_plugins::trace::state(
                "liquid capture",
                format!("live {}x{}, {left_out} left out", size[0], size[1]),
            );
        }
        let scale = size[0] as f32 / screen.width();
        // Content rect coordinates are in points; translate to the offscreen origin.
        for job in &mut jobs {
            job.clip_rect = job.clip_rect.translate(-screen.min.to_vec2());
            if let egui::epaint::Primitive::Mesh(mesh) = &mut job.primitive {
                for vertex in &mut mesh.vertices {
                    vertex.pos -= screen.min.to_vec2();
                }
            }
        }
        let descriptor = egui_wgpu::ScreenDescriptor { size_in_pixels: size, pixels_per_point: scale };
        let mut encoder = device.create_command_encoder(&Default::default());
        let commands = state.renderer.update_buffers(device, queue, &mut encoder, &jobs, &descriptor);
        let targets = state.targets.as_ref()?;
        {
            let view = targets.source.create_view(&Default::default());
            let background = egui::Rgba::from(suhina_ui::kit::theme::window_background()).to_array();
            let mut color = attachment(&view);
            if !see_through {
                color.ops.load = wgpu::LoadOp::Clear(wgpu::Color {
                    r: f64::from(background[0]),
                    g: f64::from(background[1]),
                    b: f64::from(background[2]),
                    a: 1.0,
                });
            }
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("liquid live UI"),
                    color_attachments: &[Some(color)],
                    ..Default::default()
                })
                .forget_lifetime();
            state.renderer.render(&mut pass, &jobs, &descriptor);
        }
        if blur {
            for (target, pipeline, bindings) in [
                (&targets.scratch, &state.horizontal, &targets.horizontal),
                (&targets.output, &state.vertical, &targets.vertical),
            ] {
                let view = target.create_view(&Default::default());
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("liquid linear blur"),
                    color_attachments: &[Some(attachment(&view))],
                    ..Default::default()
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, bindings, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        queue.submit(commands.into_iter().chain([encoder.finish()]));
        Some(Backdrop { texture: targets.image, sharp_texture: Some(targets.sharp_image), screen })
    }
}

fn attachment(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a native GPU; run explicitly for rendering changes"]
    fn replacing_backgrounds_releases_old_capture_registrations() {
        let instance = wgpu::Instance::default();
        let gpu =
            wait(egui_wgpu::RenderState::create(&Default::default(), &instance, None, Default::default())).unwrap();
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
                &egui::epaint::ImageDelta::full(
                    egui::ColorImage::filled([8, 8], egui::Color32::RED),
                    Default::default(),
                ),
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
            fn capture_surfaces(
                &self,
                ctx: &egui::Context,
                shapes: Vec<egui::epaint::ClippedShape>,
            ) -> Option<Backdrop> {
                let result = self.liquid.capture_surfaces(ctx, shapes);
                self.content_captures.lock().unwrap().push(result.is_some());
                result
            }
        }
        let instance = wgpu::Instance::default();
        let gpu =
            wait(egui_wgpu::RenderState::create(&Default::default(), &instance, None, Default::default())).unwrap();
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
        assert!(
            content.len() >= 30 && content.iter().all(|captured| *captured),
            "content refraction failed: {content:?}"
        );
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
        let gpu =
            wait(egui_wgpu::RenderState::create(&Default::default(), &instance, None, Default::default())).unwrap();
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
        assert!(
            (178..=198).contains(&checker[center]),
            "linear-light mean should be near 188, got {}",
            checker[center]
        );
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
        let gpu =
            wait(egui_wgpu::RenderState::create(&Default::default(), &instance, None, Default::default())).unwrap();
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
                let descriptor = egui_wgpu::ScreenDescriptor {
                    size_in_pixels: [640, 400],
                    pixels_per_point: output.pixels_per_point,
                };
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
                    if matches!(
                        gpu.target_format,
                        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
                    ) {
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
}
