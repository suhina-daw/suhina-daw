// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `suhina-theme-render`: a theme drawn in a fixed set of scenes without opening the app (#1231).
//!
//! Each scene is the demo project's window, driven through labels as the window tests drive it,
//! with a fixed step so that two renders of the same theme are the same. The window is drawn
//! offscreen on wgpu with the app's own providers (`suhina_ui_gpu`) and a transparent clear, so a
//! see-through theme's backgrounds leave the pixels' alpha as they do in the real window. Each scene is
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

    /// A theme built into Suhina, or one of its variants (`suhina-studio@light`), by its id.
    pub fn built_in(id: &str) -> Option<Self> {
        (theme::is_built_in(id)
            || theme::built_in_variants().iter().any(|(variant, _)| variant.eq_ignore_ascii_case(id)))
        .then(|| Self { id: id.to_ascii_lowercase(), theme: None })
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
    // Setting the settings turns motion on as they say, so they say it off: no frame between.
    settings.animations = false;
    app.set_settings(&ctx, settings);
}

/// A window without a GPU, for driving scenes and counting.
pub fn headless(app: App, subject: &Subject, width: f32) -> Harness<'static, App> {
    harness(app, subject, width, 1.0, None)
}

/// The GPU the renders are drawn with: one device for every scene.
pub struct Gpu {
    state: egui_wgpu::RenderState,
}

impl Gpu {
    /// The default adapter, as kittest's snapshot tests find one; `Err` says there is none.
    pub fn new() -> Result<Self, String> {
        let setup = egui_kittest::wgpu::default_wgpu_setup();
        let instance = block_on(setup.new_instance());
        let config = egui_wgpu::WgpuConfiguration { wgpu_setup: setup, ..Default::default() };
        block_on(egui_wgpu::RenderState::create(&config, &instance, None, egui_wgpu::RendererOptions::PREDICTABLE))
            .map(|state| Self { state })
            .map_err(|e| format!("no GPU adapter to draw with ({e}); --budget runs without one"))
    }

    /// The most pixels a window may be wide or high.
    pub fn max_side(&self) -> u32 {
        self.state.device.limits().max_texture_dimension_2d
    }

    /// A render state of its own on the shared device: kittest takes only a fresh one, since the
    /// renderer keeps the textures of the window it draws.
    fn render_state(&self) -> egui_wgpu::RenderState {
        let existing = egui_wgpu::WgpuSetupExisting {
            instance: self.state.instance.clone(),
            adapter: self.state.adapter.clone(),
            device: self.state.device.clone(),
            queue: self.state.queue.clone(),
        };
        egui_kittest::wgpu::create_render_state(existing.into(), egui_wgpu::RendererOptions::PREDICTABLE)
    }
}

/// Runs a future that only waits for the GPU's driver.
fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    struct Wake(std::thread::Thread);
    impl std::task::Wake for Wake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(Wake(std::thread::current())));
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::park();
    }
}

/// A window on the GPU, with the app's surface and liquid providers installed as the app installs
/// them in a see-through window. The render shows a window that composites alpha; on a system whose
/// window does not, the app clears opaque under the same blended backgrounds.
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
    // A background picture is read on another thread: wait for it, and draw a frame with it.
    harness.state_mut().wait_for_background();
    harness.run_steps(2);
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
    /// The scenes drawn smaller than asked, because the GPU's textures are not large enough, with
    /// the scale used.
    pub clamped: Vec<String>,
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
    let gpu = Gpu::new()?;
    let mut done = Rendered::default();
    for scene in scenes::select(only) {
        let app = match (scene.app)() {
            Ok(app) => app,
            Err(e) => {
                done.failed.push(format!("{}: {e}", scene.name));
                continue;
            }
        };
        // The window's pixels must fit the GPU's textures.
        let fits = (gpu.max_side() as f32 - 1.0) / scene.width.max(SIZE.1);
        let scale = if scale > fits {
            done.clamped.push(format!("{}: {fits:.2}", scene.name));
            fits
        } else {
            scale
        };
        let mut harness = rendered(app, subject, scene.width, scale, &gpu);
        if let Err(e) = (scene.drive)(&mut harness) {
            done.failed.push(format!("{}: {e}", scene.name));
            continue;
        }
        if restless(&harness) {
            done.restless.push(scene.name.clone());
        }
        let shot = match shoot(&mut harness) {
            Ok(shot) => shot,
            Err(e) => {
                done.failed.push(format!("{}: {e}", scene.name));
                continue;
            }
        };
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
