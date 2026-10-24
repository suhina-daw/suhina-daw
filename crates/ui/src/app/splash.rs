// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The splash shown while Suhina starts: a small panel with the logo and the version, drawn
//! centred in the main window over its first frames. The work that makes starting slow (the
//! browser's library, the sample map, the project to open) waits until the splash has been on
//! screen, runs once, and the splash fades out over the window that is ready.
//!
//! It is drawn inside the main window rather than in a window of its own: a second window would
//! need its own GL or wgpu context, the slowest part of starting, and one window that shrinks
//! and grows would fight the main window's remembered size and position.

use egui::{Align2, Color32, Context, Id, Rect, Sense, Ui, Vec2};

use crate::kit::motion;
use crate::kit::theme;
use crate::kit::widgets;

/// The panel's size, made smaller in a window that is smaller still.
pub const SIZE: Vec2 = Vec2::new(300.0, 200.0);
/// Frames the splash is drawn before the start-up work runs, so it is on screen while it does:
/// the window is made visible after its first frame.
const DRAWN_BEFORE_START: u64 = 2;
/// Seconds the splash is drawn before the start-up work runs. Frames alone are not enough: on
/// macOS the first frames are drawn before the window has come on screen, and work that blocks
/// the main thread then keeps it from coming on screen at all.
const SHOWN_BEFORE_START: f64 = 0.2;
/// Seconds the splash stays, from its first frame, before it fades out, so it is seen even where
/// starting is quick.
const SHOWN_AT_LEAST: f64 = 1.0;
/// Seconds it stays after the start-up work, counted from the first frame after it: where the
/// window comes on screen only once that work is done, the splash is still seen there.
const SHOWN_AFTER_START: f64 = 0.3;
/// The name, 13 px like a page title, when the logo cannot be drawn.
const NAME: f32 = theme::TEXT + 2.0;
/// The logo's width on the panel, and the least room kept on each side of it.
const LOGO_WIDTH: f32 = 200.0;
const LOGO_MARGIN: f32 = 24.0;

#[derive(Debug, Clone)]
pub struct Splash {
    version: String,
    /// The frame it was first drawn in (`Context::cumulative_frame_nr`), and when (`InputState::time`).
    first_frame: Option<(u64, f64)>,
    /// The frame the start-up work was handed out in (`take_start`).
    start_frame: Option<u64>,
    /// When the first frame after the start-up work began.
    after_start: Option<f64>,
}

impl Splash {
    /// A splash that shows `version` (the app's `CARGO_PKG_VERSION`).
    pub fn new(version: &str) -> Self {
        Self { version: version.to_owned(), first_frame: None, start_frame: None, after_start: None }
    }

    /// Whether the start-up work runs now: true once, in the first frame after the splash has
    /// been drawn in `DRAWN_BEFORE_START` whole frames and for `SHOWN_BEFORE_START` seconds.
    /// Frames, not passes: egui may run a frame twice, and only a finished frame is on screen.
    pub fn take_start(&mut self, ctx: &Context) -> bool {
        let frame = ctx.cumulative_frame_nr();
        let now = ctx.input(|i| i.time);
        let shown = self.first_frame.is_some_and(|(first, since)| {
            frame >= first.saturating_add(DRAWN_BEFORE_START) && now - since >= SHOWN_BEFORE_START
        });
        let start = self.start_frame.is_none() && shown;
        if start {
            self.start_frame = Some(frame);
        }
        start
    }

    /// The start-up work has run, so the window is drawn under the splash.
    pub fn started(&self) -> bool {
        self.start_frame.is_some()
    }

    /// Whether it fades out now: the start-up work has run, and it has been on screen for
    /// `SHOWN_AT_LEAST` seconds and for `SHOWN_AFTER_START` seconds after that work.
    fn leaving(&self, now: f64) -> bool {
        let long_enough = self.first_frame.is_some_and(|(_, since)| now - since >= SHOWN_AT_LEAST);
        let after_start = self.after_start.is_some_and(|since| now - since >= SHOWN_AFTER_START);
        long_enough && after_start
    }

    /// Draws the splash over the whole window, above everything else. Until the start-up work
    /// has run and it has been seen long enough (`leaving`) it covers the window and keeps frames
    /// coming; then it fades out (at once with animations off). Returns false when it is gone and
    /// can be dropped.
    pub fn show(&mut self, ui: &Ui) -> bool {
        let ctx = ui.ctx();
        let now = ctx.input(|i| i.time);
        if self.after_start.is_none() && self.start_frame.is_some_and(|start| ctx.cumulative_frame_nr() > start) {
            self.after_start = Some(now);
        }
        let leaving = self.leaving(now);
        let opacity = motion::toward(ctx, Id::new("splash-fade"), !leaving, motion::VALUE);
        if opacity <= 0.0 {
            return false;
        }
        if !leaving {
            ctx.request_repaint();
        }
        let area = ctx.content_rect();
        egui::Area::new(Id::new("splash")).order(egui::Order::Tooltip).fixed_pos(area.min).show(ctx, |ui| {
            ui.set_min_size(area.size());
            self.draw(ui, area, opacity);
        });
        self.first_frame.get_or_insert((ctx.cumulative_frame_nr(), now));
        true
    }

    fn draw(&self, ui: &Ui, area: Rect, opacity: f32) {
        // Clicks on the window do nothing until it is there.
        let behind = ui.interact(area, Id::new("splash-behind"), Sense::click_and_drag());
        let _swallowed = behind.clicked();
        let fade = |color: Color32| color.gamma_multiply(opacity);
        let painter = ui.painter_at(area);
        painter.rect_filled(area, 0.0, fade(theme::window_background()));

        let rect = Rect::from_center_size(area.center(), SIZE.min(area.size()));
        painter.rect_filled(rect, 0.0, fade(theme::panel_background()));
        painter.rect_stroke(rect, 0.0, theme::hairline(fade(theme::border_strong())), egui::StrokeKind::Inside);

        let line = |center_y: f32, height: f32| {
            Rect::from_center_size(egui::pos2(rect.center().x, center_y), Vec2::new(rect.width(), height))
        };
        // The logo, the name in the theme's text colour; the name as text if it cannot be drawn.
        let width = (rect.width() - 2.0 * LOGO_MARGIN).clamp(0.0, LOGO_WIDTH);
        let size = Vec2::new(width, width / crate::kit::logo::aspect());
        let name = Rect::from_center_size(egui::pos2(rect.center().x, rect.center().y - 12.0), size);
        let pixels = (size * ui.ctx().pixels_per_point()).round();
        match crate::kit::logo::texture(ui.ctx(), pixels.x as u32, pixels.y as u32, theme::text_primary()) {
            Some(logo) => {
                let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                painter.image(logo.id(), name, uv, fade(Color32::WHITE));
            }
            None => widgets::text_in(
                &painter,
                name,
                Align2::CENTER_CENTER,
                "Suhina",
                theme::ui_font(NAME),
                fade(theme::text_primary()),
            ),
        }
        name_for_readers(ui, name, "Suhina");
        let version = line(name.bottom() + 8.0, 16.0);
        let shown = format!("Version {}", self.version);
        widgets::text_in(
            &painter,
            version,
            Align2::CENTER_CENTER,
            &shown,
            theme::mono_font(theme::TEXT_SMALL),
            fade(theme::text_secondary()),
        );
        name_for_readers(ui, version, &shown);
        let status = line(rect.bottom() - theme::PAD - 8.0, 16.0);
        widgets::text_in(
            &painter,
            status,
            Align2::CENTER_CENTER,
            "Starting",
            theme::ui_font(theme::TEXT_SMALL),
            fade(theme::text_hint()),
        );
    }
}

/// Names a line of the splash for screen readers and tests.
fn name_for_readers(ui: &Ui, rect: Rect, label: &str) {
    let response = ui.interact(rect, Id::new(("splash-line", label)), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
}

#[cfg(test)]
#[path = "splash_tests.rs"]
mod tests;
