// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `suhina-theme-render`: a theme drawn in a fixed set of scenes without opening the app (#1231).
//!
//! Each scene is the demo project's window, driven through labels as the window tests drive it,
//! with a fixed step so that two renders of the same theme are the same. The window is drawn
//! offscreen on wgpu with the app's own providers (`suhina_ui_gpu`) and a transparent clear, so a
//! see-through theme's fills set the pixels' alpha as they do in the real window. Each scene is
//! written twice: `<scene>.raw.png`, the window's own pixels, and `<scene>.png`, those pixels over
//! the backdrop, blurred where the system blurs behind the window.
//!
//! [`budget`] counts the style engine's work in a scene, for the tests that hold it.

pub mod budget;
pub mod composite;
pub mod scenes;

use std::path::Path;
use std::sync::Arc;

use egui_kittest::Harness;
use suhina_ui::App;
use suhina_ui::kit::theme::{self, Theme};
use suhina_ui::settings::themes::ThemeEntry;

/// The window's size in points, as the window tests draw it.
pub const SIZE: (f32, f32) = (1280.0, 900.0);
/// Seconds a frame: a fixed step, so renders are deterministic.
pub const STEP: f32 = 1.0 / 30.0;
/// At most this many frames for a scene to settle; the scenes take five at most, unless something
/// repaints every frame.
pub const MAX_STEPS: u64 = 60;

/// The theme to draw: a folder's files, or a theme built into Suhina.
#[derive(Clone)]
pub struct Subject {
    pub id: String,
    /// `None` for a built-in theme, which the app already has.
    pub theme: Option<Theme>,
}

impl Subject {
    /// The theme in `folder` (its `theme.json`, `theme.css` and files), read and never written.
    pub fn folder(folder: &Path) -> Result<(Self, Vec<String>), String> {
        let files = suhina_theme_package::read_folder(folder)?;
        let (theme, warnings) = theme::from_files(|relative, _| Ok(files.get(relative).cloned()))
            .map_err(|e| format!("{}: {e}", folder.display()))?;
        let id = folder
            .file_name()
            .and_then(|n| n.to_str())
            .map(suhina_ui::settings::themes::slug)
            .filter(|id| !id.is_empty() && !theme::is_built_in(id))
            .unwrap_or_else(|| "rendered-theme".to_owned());
        Ok((Self { id, theme: Some(theme) }, warnings))
    }

    /// A theme built into Suhina, by its id.
    pub fn built_in(id: &str) -> Option<Self> {
        theme::is_built_in(id).then(|| Self { id: id.to_ascii_lowercase(), theme: None })
    }

    /// Every theme built into Suhina.
    pub fn built_ins() -> Vec<Self> {
        theme::built_in_themes().into_iter().map(|(id, _)| Self { id: id.to_owned(), theme: None }).collect()
    }
}

/// The demo project, as the window tests open it: the browser hidden so the views have the whole
/// width.
pub fn demo_app() -> App {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(suhina_session::Session::new(suhina_ui::demo::demo_project()), &[]);
    app
}

/// What the window does after the app has drawn a frame, as the app's window does.
fn finish_frame(ctx: &egui::Context) {
    suhina_ui::kit::liquid::finish_backdrops(ctx);
    suhina_ui::kit::surface::finish_frame(ctx);
}

/// Builds the window around `app` with `subject` chosen, on `renderer` when there is one.
/// Animations are off, so a scene ends in a still frame.
fn harness(
    mut app: App,
    subject: &Subject,
    width: f32,
    scale: f32,
    renderer: Option<egui_kittest::wgpu::WgpuTestRenderer>,
) -> Harness<'static, App> {
    app.set_arrangement_grid(suhina_ui::arrangement::arrangement_view::PianoGrid::from_value("1 bar"));
    // The RAM in the control bar is measured; a fixed figure keeps the renders the same.
    app.hold_ram(Some(96 << 20));
    let mut builder = Harness::builder()
        .allow_missing_glyphs()
        .with_size((width, SIZE.1))
        .with_pixels_per_point(scale)
        .with_step_dt(STEP)
        .with_max_steps(MAX_STEPS);
    if let Some(renderer) = renderer {
        builder = builder.renderer(renderer);
    }
    let mut harness = builder.build_ui_state(
        |ui, app: &mut App| {
            app.ui(ui);
            finish_frame(ui.ctx());
        },
        app,
    );
    let ctx = harness.ctx.clone();
    theme::install(&ctx);
    App::set_animations(&ctx, false);
    choose(&mut harness, subject);
    harness
}

/// Makes `subject` the window's theme, as picking it in Settings does.
pub fn choose(harness: &mut Harness<'static, App>, subject: &Subject) {
    let ctx = harness.ctx.clone();
    let app = harness.state_mut();
    if let Some(theme) = &subject.theme {
        app.set_themes(&ctx, vec![ThemeEntry { id: subject.id.clone(), theme: theme.clone() }], None);
    }
    let mut settings = app.settings().clone();
    settings.theme.clone_from(&subject.id);
    app.set_settings(&ctx, settings);
}

/// A window without a GPU, for driving scenes and counting.
pub fn headless(app: App, subject: &Subject, width: f32) -> Harness<'static, App> {
    harness(app, subject, width, 1.0, None)
}

/// The GPU the renders are drawn with.
pub struct Gpu {
    setup: egui_wgpu::WgpuSetup,
}

impl Gpu {
    /// The default adapter, as kittest's snapshot tests find one. Each window gets a render
    /// state of its own, since kittest takes only a fresh one.
    pub fn new() -> Self {
        Self { setup: egui_kittest::wgpu::default_wgpu_setup() }
    }

    fn render_state(&self) -> egui_wgpu::RenderState {
        egui_kittest::wgpu::create_render_state(self.setup.clone(), egui_wgpu::RendererOptions::PREDICTABLE)
    }
}

impl Default for Gpu {
    fn default() -> Self {
        Self::new()
    }
}

/// A window on the GPU, with the app's surface and liquid providers installed as the app installs
/// them in a see-through window.
pub fn rendered(app: App, subject: &Subject, width: f32, scale: f32, gpu: &Gpu) -> Harness<'static, App> {
    let state = gpu.render_state();
    let renderer = egui_kittest::wgpu::WgpuTestRenderer::from_render_state(state.clone());
    let mut harness = harness(app, subject, width, scale, Some(renderer));
    let ctx = harness.ctx.clone();
    suhina_ui::kit::surface::install(&ctx, suhina_ui_gpu::surface::provider(&state));
    // As the app: liquid needs encoded and linear views of its captures.
    if state.adapter.get_downlevel_capabilities().flags.contains(egui_wgpu::wgpu::DownlevelFlags::VIEW_FORMATS) {
        suhina_ui::kit::liquid::install(&ctx, Arc::new(suhina_ui_gpu::liquid::Liquid::new(state)));
    }
    harness.run_steps(2);
    harness
}

/// One scene drawn: the window's pixels as it leaves them (premultiplied alpha) and whether the
/// window would be see-through, so the system blurs behind it.
pub struct Shot {
    pub pixels: image::RgbaImage,
    pub see_through: bool,
    /// The colour an opaque window clears to before drawing, premultiplied.
    pub clear: [u8; 4],
}

/// Draws the scene the harness is in.
pub fn shoot(harness: &mut Harness<'static, App>) -> Result<Shot, String> {
    let pixels = harness.render()?;
    let see_through = theme::window_background().a() < u8::MAX;
    let [r, g, b, _] = harness.ctx.global_style().visuals.panel_fill.to_array();
    Ok(Shot { pixels, see_through: see_through && theme::see_through(), clear: [r, g, b, 255] })
}

/// What [`render_all`] did.
#[derive(Debug, Default)]
pub struct Rendered {
    /// The files written, by name.
    pub written: Vec<String>,
    /// The scenes that could not be drawn, with why.
    pub failed: Vec<String>,
    /// The scenes whose window never stopped asking to be drawn again: the window would keep
    /// repainting there, and its last frame is the one drawn.
    pub restless: Vec<String>,
}

/// Whether the window asked to be drawn again at once after its last frame.
pub fn restless(harness: &Harness<'static, App>) -> bool {
    harness.output().viewport_output.get(&egui::ViewportId::ROOT).is_some_and(|v| v.repaint_delay.is_zero())
}

/// Renders `subject` in every scene, or only the one called `only`, into `out` over `backdrop`:
/// two PNGs a scene.
pub fn render_all(
    subject: &Subject,
    backdrop: &image::RgbaImage,
    out: &Path,
    scale: f32,
    only: Option<&str>,
) -> Result<Rendered, String> {
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let gpu = Gpu::new();
    let mut done = Rendered::default();
    for scene in scenes::scenes().iter().filter(|s| only.is_none_or(|name| s.name == name)) {
        let mut harness = rendered((scene.app)(), subject, scene.width, scale, &gpu);
        if let Err(e) = (scene.drive)(&mut harness) {
            done.failed.push(format!("{}: {e}", scene.name));
            continue;
        }
        if restless(&harness) {
            done.restless.push(scene.name.clone());
        }
        let shot = shoot(&mut harness)?;
        let raw = composite::unpremultiply(&shot.pixels);
        let over = composite::over_backdrop(&shot, backdrop, scale);
        for (name, image) in [(format!("{}.raw.png", scene.name), raw), (format!("{}.png", scene.name), over)] {
            let path = out.join(&name);
            image.save(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            done.written.push(name);
        }
    }
    Ok(done)
}
