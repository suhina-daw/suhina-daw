// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Coordinates a static source, display-space effects and publication to the UI.
mod filter;
mod placement;
mod source;

use crate::kit::surface::Backgrounds as _;
use egui::{ColorImage, TextureHandle, TextureOptions, Ui, Vec2};
use std::sync::{
    Arc,
    mpsc::{self, Receiver},
};

#[derive(Clone, PartialEq, Eq)]
struct Request {
    path: Arc<str>,
    blur: u8,
    size: [u32; 2],
}
struct Completed {
    request: Request,
    result: Result<(Arc<image::RgbaImage>, ColorImage), String>,
}

#[derive(Default)]
pub(super) struct Background {
    requested: Option<Request>,
    worker: Option<Receiver<Completed>>,
    source: Option<(Arc<str>, Arc<image::RgbaImage>)>,
    texture: Option<TextureHandle>,
    texture_path: Arc<str>,
    visible: bool,
}

impl Background {
    /// An explicit picker action may retry a repaired file; frames never retry failed loads.
    pub(super) fn retry_failed(&mut self) {
        if self.worker.is_none() && self.texture.is_none() {
            self.requested = None;
        }
    }

    pub(super) fn update(
        &mut self,
        ui: &Ui,
        settings: &crate::settings::Settings,
        mut palette: crate::kit::theme::Palette,
    ) -> Result<(), String> {
        crate::kit::liquid::set_panel_base(ui.ctx(), palette.window_background);
        let skin = crate::kit::skin::current();
        let themed = skin.background.as_ref();
        let path: Arc<str> = if settings.theme == "high-contrast" {
            Arc::from("")
        } else {
            themed.cloned().unwrap_or_else(|| Arc::from(settings.background_image.as_str()))
        };
        let blur = if themed.is_some() { skin.background_blur } else { settings.view.background_blur };
        let result = self.paint(ui, path, blur);
        let visible = result.as_ref().copied().unwrap_or(false);
        if visible {
            let opacity = f32::from(
                if themed.is_some() { skin.background_opacity } else { settings.view.background_tint }.min(100),
            ) / 100.0;
            palette.window_background = palette.window_background.gamma_multiply(opacity);
            palette.panel_background = palette.panel_background.gamma_multiply(opacity);
            palette.border = palette.border.gamma_multiply(opacity);
        }
        if (visible || self.visible) && crate::kit::theme::palette() != palette {
            crate::kit::theme::set_palette(ui.ctx(), palette);
        }
        self.visible = visible;
        result.map(|_| ())
    }

    fn paint(&mut self, ui: &Ui, path: Arc<str>, blur: u8) -> Result<bool, String> {
        let rect = ui.max_rect();
        let request =
            Request { path: path.clone(), blur, size: [rect.width().max(1.0) as u32, rect.height().max(1.0) as u32] };
        if let Some(worker) = &self.worker {
            match worker.try_recv() {
                Ok(completed) => {
                    self.worker = None;
                    match completed.result {
                        Ok((source, pixels)) => {
                            if completed.request.path == path && !path.is_empty() {
                                self.source = Some((path.clone(), source));
                            }
                            if completed.request == request && !path.is_empty() {
                                let pixels = Arc::new(pixels);
                                let texture =
                                    ui.ctx().load_texture("private-background", pixels.clone(), TextureOptions::LINEAR);
                                crate::kit::liquid::set_background_image(
                                    ui.ctx(),
                                    Some(crate::kit::liquid::BackgroundImage { id: texture.id(), pixels }),
                                );
                                self.texture = Some(texture);
                                self.texture_path = path.clone();
                            }
                        }
                        Err(error) if completed.request == request => return Err(error),
                        Err(_) => {}
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.worker = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if path.is_empty() || self.texture_path != path {
            self.texture = None;
            crate::kit::liquid::set_background_image(ui.ctx(), None);
        }
        if path.is_empty() {
            self.source = None;
            self.requested = None;
            return Ok(false);
        }
        if self.worker.is_none() && self.requested.as_ref() != Some(&request) {
            let cached = self
                .source
                .as_ref()
                .filter(|(cached_path, _)| cached_path == &path)
                .map(|(_, image)| Arc::clone(image));
            let job = request.clone();
            let ctx = ui.ctx().clone();
            let (send, receive) = mpsc::sync_channel(1);
            std::thread::Builder::new()
                .name("background-image".into())
                .spawn(move || {
                    let result =
                        cached.map(Ok).unwrap_or_else(|| source::load(&job.path).map(Arc::new)).map(|source| {
                            let sigma = placement::source_sigma(
                                job.blur,
                                Vec2::new(source.width() as f32, source.height() as f32),
                                Vec2::new(job.size[0] as f32, job.size[1] as f32),
                            );
                            let pixels = filter::apply(&source, sigma);
                            (source, pixels)
                        });
                    let _ = send.send(Completed { request: job, result });
                    ctx.request_repaint();
                })
                .map_err(|error| error.to_string())?;
            self.requested = Some(request);
            self.worker = Some(receive);
        }
        if self.worker.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(texture) = &self.texture {
            ui.painter().fill_background(rect, 0.0, crate::kit::theme::window_background().to_opaque());
            ui.painter().image(
                texture.id(),
                rect,
                placement::cover_uv(texture.size_vec2(), rect.size()),
                egui::Color32::WHITE,
            );
            return Ok(true);
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_load_retries_only_after_explicit_reselection() {
        let ctx = egui::Context::default();
        let mut background = Background::default();
        let mut request = None;
        ctx.run_ui(egui::RawInput::default(), |ui| {
            let rect = ui.max_rect();
            request = Some(Request {
                path: "repaired.png".into(),
                blur: 5,
                size: [rect.width().max(1.0) as u32, rect.height().max(1.0) as u32],
            });
        })
        .textures_delta
        .clear();
        let request = request.unwrap();
        let (send, receive) = mpsc::sync_channel(1);
        send.send(Completed { request: request.clone(), result: Err("invalid PNG".into()) }).unwrap();
        background.worker = Some(receive);
        background.requested = Some(request.clone());
        ctx.run_ui(egui::RawInput::default(), |ui| {
            assert!(background.paint(ui, "repaired.png".into(), 5).is_err());
            assert!(!background.paint(ui, "repaired.png".into(), 5).unwrap());
        })
        .textures_delta
        .clear();
        assert!(background.worker.is_none());
        background.retry_failed();
        assert!(background.requested.is_none());
        // Supply cached decoded pixels to exercise retry publication without filesystem timing.
        background.source = Some(("repaired.png".into(), Arc::new(image::RgbaImage::new(1, 1))));
        ctx.run_ui(egui::RawInput::default(), |ui| {
            assert!(!background.paint(ui, "repaired.png".into(), 5).unwrap());
        })
        .textures_delta
        .clear();
        assert!(background.worker.is_some());
        background.retry_failed();
        assert!(background.requested.as_ref() == Some(&request), "never reset an in-flight request");
    }

    #[test]
    fn obsolete_worker_result_cannot_restore_a_cleared_background() {
        let (send, receive) = mpsc::sync_channel(1);
        send.send(Completed {
            request: Request { path: "old.png".into(), blur: 5, size: [100, 100] },
            result: Ok((Arc::new(image::RgbaImage::new(1, 1)), ColorImage::new([1, 1], vec![egui::Color32::RED]))),
        })
        .unwrap();
        let mut background = Background { worker: Some(receive), ..Default::default() };
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert!(!background.paint(ui, "".into(), 5).unwrap());
        });
        output.textures_delta.clear();
        assert!(background.texture.is_none());
        assert!(background.source.is_none());
        assert!(crate::kit::liquid::background_image(&ctx).is_none());
    }
}
