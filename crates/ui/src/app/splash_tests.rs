// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

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
    let splash =
        Splash { first_frame: Some((0, 0.0)), start_frame: Some(12), after_start: Some(3.0), ..Splash::new("1.2.3") };
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
