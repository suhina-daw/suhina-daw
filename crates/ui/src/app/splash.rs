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
        crate::kit::logo::paint(ui, &painter, name, theme::ui_font(NAME), opacity);
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
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    /// What a frame of the app does with a splash: the start-up work, then the window.
    #[derive(Default)]
    struct State {
        splash: Option<Splash>,
        /// The frame the splash was first drawn in, and when.
        first: Option<(u64, f64)>,
        /// The frames the start-up work ran in, and when.
        starts: Vec<(u64, f64)>,
        /// Frames the window was drawn in, under the splash or without it.
        window_frames: u32,
        /// When the splash was gone.
        gone: Option<f64>,
    }

    fn frame(ui: &mut Ui, state: &mut State) {
        let ctx = ui.ctx().clone();
        let now = (ctx.cumulative_frame_nr(), ctx.input(|i| i.time));
        if let Some(splash) = state.splash.as_mut()
            && splash.take_start(&ctx)
        {
            state.starts.push(now);
        }
        if state.splash.as_ref().is_none_or(Splash::started) {
            state.window_frames += 1;
        }
        if let Some(splash) = state.splash.as_mut() {
            state.first.get_or_insert(now);
            if !splash.show(ui) {
                state.splash = None;
                state.gone = Some(now.1);
            }
        }
    }

    const STEP: f64 = 1.0 / 60.0;

    /// A harness whose next frame is the splash's first. Building it runs frames until nothing
    /// moves, so the splash comes in after.
    fn harness(size: Vec2, animations: bool) -> Harness<'static, State> {
        let mut harness = Harness::builder()
            .with_size(size)
            .with_step_dt(STEP as f32)
            .with_max_steps(240)
            .build_ui_state(frame, State::default());
        motion::set_enabled(&harness.ctx, animations);
        *harness.state_mut() = State { splash: Some(Splash::new("1.2.3")), ..State::default() };
        harness
    }

    /// Runs frames until the start-up work has run.
    fn until_started(harness: &mut Harness<'static, State>) {
        for _ in 0..240 {
            harness.step();
            if !harness.state().starts.is_empty() {
                return;
            }
        }
        panic!("the start-up work never ran");
    }

    /// Seconds from the splash's first frame to `time`.
    fn since_first(harness: &Harness<'static, State>, time: f64) -> f64 {
        time - harness.state().first.map_or(f64::NAN, |(_, first)| first)
    }

    const WINDOW: Vec2 = Vec2::new(800.0, 500.0);

    #[test]
    fn it_shows_the_logo_and_version_from_its_first_frame() {
        let mut harness = harness(WINDOW, true);
        harness.step();
        // The name is the logo, at the logo's own proportions, with the version under it.
        let logo = harness.get_by_label("Suhina").rect();
        assert!((logo.width() / logo.height() - crate::kit::logo::aspect()).abs() < 0.05, "{logo:?}");
        assert_eq!(logo.width(), LOGO_WIDTH);
        assert!(harness.get_by_label("Version 1.2.3").rect().top() >= logo.bottom());
        assert!(harness.state().starts.is_empty(), "nothing starts before the splash is on screen");
        assert_eq!(harness.state().window_frames, 0, "the window waits under the splash");
    }

    #[test]
    fn the_start_up_work_runs_once_after_the_splash_was_drawn_for_a_while() {
        let mut harness = harness(WINDOW, true);
        for _ in 0..DRAWN_BEFORE_START {
            harness.step();
        }
        assert!(harness.state().starts.is_empty(), "two frames are not enough on their own");
        harness.get_by_label("Suhina");
        until_started(&mut harness);
        let (frame, time) = harness.state().starts[0];
        let first = harness.state().first.map_or(0, |(first, _)| first);
        assert!(frame >= first + DRAWN_BEFORE_START);
        let shown = since_first(&harness, time);
        assert!((SHOWN_BEFORE_START - 1e-9..SHOWN_BEFORE_START + 2.0 * STEP).contains(&shown), "{shown}");
        assert_eq!(harness.state().window_frames, 1, "the window is drawn under the splash");
        harness.run();
        assert_eq!(harness.state().starts.len(), 1, "once");
    }

    #[test]
    fn it_stays_long_enough_to_be_seen_then_fades_out() {
        let mut harness = harness(WINDOW, true);
        until_started(&mut harness);
        // Quick to start, it still covers the window until it has been seen.
        for _ in 0..10 {
            harness.step();
        }
        assert!(harness.state().splash.is_some());
        harness.get_by_label("Suhina");
        harness.run();
        assert!(harness.state().splash.is_none(), "gone after the fade");
        assert!(harness.query_by_label("Suhina").is_none());
        let gone = since_first(&harness, harness.state().gone.unwrap_or(f64::NAN));
        // It fades only once it has been on screen for `SHOWN_AT_LEAST`.
        assert!(gone >= SHOWN_AT_LEAST + 2.0 * STEP, "{gone}");
    }

    #[test]
    fn without_animations_it_goes_at_once_when_seen_long_enough() {
        let mut harness = harness(WINDOW, false);
        until_started(&mut harness);
        harness.run();
        let gone = since_first(&harness, harness.state().gone.unwrap_or(f64::NAN));
        assert!((SHOWN_AT_LEAST - 1e-9..SHOWN_AT_LEAST + 2.0 * STEP).contains(&gone), "no fade: {gone}");
    }

    #[test]
    fn a_slow_start_keeps_it_on_screen_after_the_work() {
        // The first frame at 0 s, the work in frame 12, the frame after it at 3 s: the window may
        // only now be on screen, so it stays a while longer.
        let splash = Splash {
            first_frame: Some((0, 0.0)),
            start_frame: Some(12),
            after_start: Some(3.0),
            ..Splash::new("1.2.3")
        };
        assert!(!splash.leaving(3.0 + SHOWN_AFTER_START - 0.01));
        assert!(splash.leaving(3.0 + SHOWN_AFTER_START + 0.01));
        // Not before the work has run, however long it has been on screen.
        let waiting = Splash { first_frame: Some((0, 0.0)), ..Splash::new("1.2.3") };
        assert!(!waiting.leaving(10.0));
    }

    #[test]
    fn it_fits_a_window_smaller_than_itself() {
        let size = Vec2::new(200.0, 120.0);
        let mut harness = harness(size, true);
        harness.step();
        let window = Rect::from_min_size(egui::Pos2::ZERO, size);
        for label in ["Suhina", "Version 1.2.3"] {
            let rect = harness.get_by_label(label).rect();
            assert!(window.contains_rect(rect), "{label}: {rect:?}");
        }
    }
}
