// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use suhina_device_sdk::builtin;

/// The panel's parameters at the device's defaults, with `changes`, as `show` reads them.
fn panel_parameters(changes: &[(&str, f64)]) -> Vec<GraphParameter> {
    let mut parameters: Vec<GraphParameter> = builtin::parameters(&suhina_device_compressor::INFO)
        .iter()
        .map(|p| GraphParameter::new(p, p.default))
        .collect();
    for (id, value) in changes {
        parameters.iter_mut().find(|p| p.id == *id).unwrap().value = *value;
    }
    parameters
}

fn rect() -> Rect {
    Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(200.0, 120.0))
}

#[test]
fn the_axes_map_back_and_forth() {
    let rect = rect();
    for db in [-60.0, -24.5, -12.0, 0.0, 6.0] {
        assert!((y_level(level_y(db, rect), rect) - db).abs() < 1e-4);
        assert!((x_level(level_x(db, rect), rect) - db).abs() < 1e-4);
    }
    assert_eq!(level_y(RANGE.0, rect), rect.bottom());
    assert_eq!(level_y(RANGE.1, rect), rect.top());
    assert_eq!(level_y(-200.0, rect), rect.bottom(), "silence sits on the floor");
    assert_eq!(reduction_y(0.0, rect), rect.top());
    assert_eq!(reduction_y(REDUCTION_RANGE * 2.0, rect), rect.bottom());
}

#[test]
fn the_curve_is_the_devices_gain_computer() {
    let values = values(&panel_parameters(&[("threshold", -20.0), ("ratio", 4.0), ("knee", 0.0)]));
    assert!((values.curve(-40.0) + 40.0).abs() < 1e-9, "under the threshold nothing changes");
    // 20 dB over a 4 : 1 threshold comes out 5 dB over it.
    assert!((values.curve(0.0) + 15.0).abs() < 1e-9, "{}", values.curve(0.0));
    let with_output = values_with(&values, |v| v.output = 6.0);
    assert!((with_output.curve(-40.0) + 34.0).abs() < 1e-9, "the output gain lifts the whole curve");
    let dry = values_with(&values, |v| v.mix = 0.0);
    assert!((dry.curve(0.0)).abs() < 1e-9, "fully dry the curve is unity");
    // A soft knee bends the curve under the threshold already, and meets the hard one past it.
    let soft = values_with(&values, |v| v.knee = 12.0);
    assert!(soft.curve(-20.0) < -20.0);
    assert!((soft.curve(-10.0) - values.curve(-10.0)).abs() < 1e-9);
}

fn values_with(values: &Values, change: impl FnOnce(&mut Values)) -> Values {
    let mut values = *values;
    change(&mut values);
    values
}

#[test]
fn the_readouts_become_the_live_values() {
    let mut parameters = panel_parameters(&[]);
    assert_eq!(values(&parameters).live, None, "a device that hands nothing shows no live values");
    for (index, value) in [
        (readout::DETECTOR, -9.0),
        (readout::INPUT, -10.0),
        (readout::OUTPUT, -14.0),
        (readout::REDUCTION, 4.5),
        (readout::KEYED, 1.0),
    ] {
        parameters.push(GraphParameter::readout(index, value).unwrap());
    }
    let live = values(&parameters).live.unwrap();
    assert_eq!(live, Live { detector: -9.0, output: -14.0, reduction: 4.5, keyed: true });
}

#[test]
fn the_history_keeps_the_last_seconds_at_its_rate() {
    let mut history = History::default();
    let live = Live { detector: -6.0, output: -9.0, reduction: 3.0, keyed: false };
    // Frames at 120 per second for six seconds: a reading is kept every 1/40 s.
    for frame in 0..720 {
        history.record(f64::from(frame) / 120.0, &live);
    }
    let kept = history.0.len() as f64;
    assert!((kept - HISTORY_SECONDS * HISTORY_RATE).abs() <= 2.0, "{kept} readings");
    let span = history.0.back().unwrap().time - history.0.front().unwrap().time;
    assert!(span <= HISTORY_SECONDS);
    // A clock that went back (another window's time) starts the history again.
    history.record(1.0, &live);
    assert_eq!(history.0.len(), 1);
    // Also when it goes back to a time still inside what is kept: never out of order.
    let mut history = History::default();
    for time in [0.0, 0.025, 0.05] {
        history.record(time, &live);
    }
    history.record(0.03, &live);
    let times: Vec<f64> = history.0.iter().map(|r| r.time).collect();
    assert_eq!(times, vec![0.03]);
}

#[test]
fn a_range_given_backwards_or_apart_from_the_views_never_panics() {
    assert_eq!(bounded(5.0, 0.0, 1.0), 1.0);
    assert_eq!(bounded(5.0, 1.0, 0.0), 1.0, "a backwards range holds the value as well");
    assert_eq!(bounded(-5.0, 1.0, 0.0), 0.0);
    assert_eq!(ratio_bounds(1.0, 20.0), (1.0, 20.0), "the built-in ratio drags over the view's range");
    assert_eq!(ratio_bounds(1.0, 50.0), (1.0, 20.0));
    assert_eq!(ratio_bounds(30.0, 40.0), (30.0, 40.0), "a range apart from the view's keeps its own");
}

/// Drags a handle from (50, 50) down by `steps`, each `(dy, shift)`, and returns how far the
/// drag has moved the value after each one.
fn drag_steps(steps: &[(f32, bool)]) -> Vec<f32> {
    let ctx = egui::Context::default();
    let state = egui::Id::unique("drag-test");
    let mut moved = Vec::new();
    let frame = |mut events: Vec<egui::Event>, shift: bool, moved: &mut Vec<f32>| {
        let modifiers = if shift { egui::Modifiers::SHIFT } else { egui::Modifiers::NONE };
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(200.0))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let rect = Rect::from_center_size(Pos2::new(50.0, 50.0), Vec2::splat(20.0));
            let response = ui.interact(rect, state.with("handle"), Sense::click_and_drag());
            if let Some((_, distance)) = drag_state(ui, &response, state, 0.0_f32) {
                moved.push(distance.y);
            }
        });
        output.textures_delta.clear();
    };
    let press = |pressed| egui::Event::PointerButton {
        pos: Pos2::new(50.0, 50.0),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(vec![egui::Event::PointerMoved(Pos2::new(50.0, 50.0))], false, &mut moved);
    frame(vec![press(true)], false, &mut moved);
    moved.clear();
    let mut y = 50.0;
    for &(dy, shift) in steps {
        y += dy;
        frame(vec![egui::Event::PointerMoved(Pos2::new(50.0, y))], shift, &mut moved);
    }
    moved
}

#[test]
fn shift_pressed_halfway_through_a_drag_slows_it_from_there_without_a_jump() {
    // 20 points, then Shift with no movement, then 8 more points at a quarter of the pace.
    let moved = drag_steps(&[(20.0, false), (0.0, true), (8.0, true), (0.0, false), (4.0, false)]);
    assert_eq!(moved, vec![20.0, 20.0, 22.0, 22.0, 26.0]);
}

#[test]
fn the_meters_view_puts_three_meters_side_by_side() {
    let display = Rect::from_min_size(Pos2::ZERO, Vec2::new(316.0, 168.0));
    let meters = areas(display, View::Meters);
    assert!(meters.input.right() < meters.reduction.left() && meters.reduction.right() < meters.output.left());
    let curve = areas(display, View::Curve);
    assert!(curve.arrow.right() < curve.view.left() && curve.view.right() < curve.reduction.left());
    assert!(curve.view.width() > 200.0, "the view takes most of the display");
    for areas in [meters, curve] {
        for rect in [areas.input, areas.reduction, areas.output] {
            assert!(display.contains_rect(rect));
            assert!(rect.top() >= display.top() + TABS_HEIGHT && rect.bottom() <= areas.values.top());
        }
    }
}

/// Renders the display's three views with live values and a pumping history to
/// `target/ui-snapshots/compressor-display-<view>.png`, for looking at:
/// `cargo test -p suhina-ui --lib -- --ignored the_display_renders_live_values_for_the_hand_check`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn the_display_renders_live_values_for_the_hand_check() {
    let mut parameters = panel_parameters(&[("threshold", -24.0), ("ratio", 4.0)]);
    for (index, value) in [
        (readout::DETECTOR, -9.0),
        (readout::INPUT, -40.0),
        (readout::OUTPUT, -18.0),
        (readout::REDUCTION, 9.0),
        (readout::KEYED, 1.0),
    ] {
        parameters.push(GraphParameter::readout(index, value).unwrap());
    }
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&out).unwrap();
    let at = DeviceAt(vec![0]);
    let id = egui::Id::unique(("compressor-display", 0, &at.0));
    for view in View::ALL {
        let parameters = parameters.clone();
        let at = at.clone();
        let mut harness =
            egui_kittest::Harness::builder().with_size(Vec2::new(340.0, 190.0)).wgpu().build_ui(move |ui| {
                let time = ui.input(|i| i.time);
                ui.ctx().data_mut(|d| {
                    d.insert_temp(id.with("view"), view);
                    // A kick every half second: the key jumps up and the reduction follows it.
                    let mut history = History::default();
                    for step in 0..160 {
                        let t = time - HISTORY_SECONDS + f64::from(step) / HISTORY_RATE;
                        let phase = (t.rem_euclid(0.5)) / 0.5;
                        let detector = -6.0 - 40.0 * phase;
                        let reduction = (12.0 * (-phase * 6.0).exp()) as f32;
                        history.0.push_back(Reading {
                            time: t,
                            detector: detector as f32,
                            output: detector as f32 - reduction,
                            reduction,
                        });
                    }
                    d.insert_temp(id.with("history"), history);
                });
                let display = Rect::from_min_size(Pos2::new(12.0, 11.0), Vec2::new(316.0, 168.0));
                let mut actions = Vec::new();
                show(
                    ui,
                    display,
                    &parameters,
                    Palette {
                        display: Color32::from_rgb(10, 17, 23),
                        reduction: Color32::from_rgb(242, 166, 90),
                        level: Color32::from_rgb(95, 211, 192),
                    },
                    TrackId(0),
                    &at,
                    &mut actions,
                );
            });
        harness.run();
        let image = harness.render().unwrap();
        image.save(out.join(format!("compressor-display-{}.png", view.label().to_lowercase()))).unwrap();
    }
}
