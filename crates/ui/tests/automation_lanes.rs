// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Automation lanes in the Arrangement view (#420), driven like a user would: points added,
//! moved and deleted with the pointer and the keyboard become `automation.*` commands that
//! change the project through the session, with undo.

#![allow(clippy::expect_used)] // test helpers outside #[test] functions

use egui::{Key, Modifiers, Pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_engine::model::automation::{self, Curve};
use suhina_engine::model::{AutomationLane, AutomationPoint, Device, DeviceKind, DeviceParam, Extensions, Project};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::{App, kit::theme};

/// The window the tests draw in, in points.
const SIZE: (f32, f32) = (1280.0, 800.0);

/// `project` open in the app, in the Arrangement view, with the browser closed, and a click
/// anywhere on a lane adding a point (Settings › Arrangement › Automation), which these tests
/// place points with.
fn arrangement(project: Project) -> Harness<'static, App> {
    let mut harness = arrangement_sized(project, SIZE);
    click_anywhere(&mut harness, true);
    harness
}

/// Turns Settings › Arrangement › Automation › Click anywhere adds on or off.
fn click_anywhere(harness: &mut Harness<'static, App>, on: bool) {
    let ctx = harness.ctx.clone();
    let mut settings = harness.state().settings().clone();
    settings.view.automation_click_anywhere = on;
    harness.state_mut().set_settings(&ctx, settings);
    harness.run();
}

/// [`arrangement`] in a window of `size`.
fn arrangement_sized(project: Project, size: (f32, f32)) -> Harness<'static, App> {
    let mut app = App::new();
    app.set_browser_open(false);
    // These tests place points on a bar grid.
    app.set_arrangement_grid(suhina_ui::arrangement::arrangement_view::PianoGrid::from_value("1 bar"));
    app.set_session(Session::new(project), &[]);
    let mut harness = Harness::builder()
        .with_size(size)
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness
}

/// The demo project in the Arrangement view with the Drums track's lanes shown.
fn with_lanes() -> Harness<'static, App> {
    let mut harness = arrangement(demo_project());
    harness.get_by_label("Show automation of Drums").click();
    harness.run();
    harness
}

/// The points of the Drums track's volume lane, and whether it plays.
fn volume(harness: &Harness<'static, App>) -> (Vec<AutomationPoint>, bool) {
    let Some(session) = harness.state().session() else { return (Vec::new(), true) };
    let project = session.project();
    let channel = project.tracks.iter().find(|t| t.name == "Drums").and_then(|t| t.channel);
    let lane = project.channels.iter().find(|c| Some(c.id) == channel).and_then(|c| c.lane("volume"));
    lane.map_or((Vec::new(), true), |l| (l.points.clone(), l.enabled))
}

/// The points of the Drums track's pan lane.
fn pan(harness: &Harness<'static, App>) -> (Vec<AutomationPoint>, bool) {
    let Some(session) = harness.state().session() else { return (Vec::new(), true) };
    let project = session.project();
    let channel = project.tracks.iter().find(|t| t.name == "Drums").and_then(|t| t.channel);
    let lane = project.channels.iter().find(|c| Some(c.id) == channel).and_then(|c| c.lane("pan"));
    lane.map_or((Vec::new(), true), |l| (l.points.clone(), l.enabled))
}

/// A press and release of the left button at `pos`.
fn click_at(harness: &mut Harness<'static, App>, pos: Pos2) {
    harness.hover_at(pos);
    harness.run();
    harness.drag_at(pos);
    harness.run();
    harness.drop_at(pos);
    harness.run();
}

/// A drag with the left button from `from` by `by`, through its middle.
fn drag(harness: &mut Harness<'static, App>, from: Pos2, by: egui::Vec2) {
    harness.hover_at(from);
    harness.run();
    harness.drag_at(from);
    harness.run();
    harness.hover_at(from + by * 0.5);
    harness.run();
    harness.hover_at(from + by);
    harness.run();
    harness.drop_at(from + by);
    harness.run();
}

/// Where the volume lane is on screen.
fn lane(harness: &Harness<'static, App>) -> egui::Rect {
    harness.get_by_label("Volume automation on Drums").rect()
}

/// A click on the volume lane `x` points from its left, half-way up.
fn add_point(harness: &mut Harness<'static, App>, x: f32) {
    let rect = lane(harness);
    click_at(harness, Pos2::new(rect.left() + x, rect.center().y));
}

/// The envelope button in a track header shows its lanes under it and hides them again.
#[test]
fn the_envelope_button_shows_and_hides_a_tracks_lanes() {
    let mut harness = arrangement(demo_project());
    assert!(harness.query_by_label("Volume automation on Drums").is_none());
    harness.get_by_label("Show automation of Drums").click();
    harness.run();
    assert!(harness.query_by_label("Volume automation on Drums").is_some());
    // The next track moved down to make room.
    harness.get_by_label("Hide automation of Drums").click();
    harness.run();
    assert!(harness.query_by_label("Volume automation on Drums").is_none());
}

/// A click on a lane adds a point on the grid at the pointer's height, and undo removes it.
#[test]
fn a_click_on_a_lane_adds_a_point_on_the_grid_and_undo_removes_it() {
    let mut harness = with_lanes();
    // 8 points a beat by default, where the grid is a bar: 100 points in is beat 12.5, bar 4.
    add_point(&mut harness, 100.0);
    let (points, enabled) = volume(&harness);
    assert_eq!(points.len(), 1, "{points:?}");
    assert_eq!(points[0].beat, 12.0);
    assert!(enabled);
    // Half-way up the lane is below 0 dB (0 dB sits at 85 %).
    assert!(points[0].value > 0.0 && points[0].value < 1.0, "{}", points[0].value);
    assert!(harness.query_by_label("Volume point 1 on Drums").is_some());

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(volume(&harness).0.is_empty());
}

/// A click with Alt (Option on macOS) adds a point where the pointer is, off the grid.
#[test]
fn alt_adds_a_point_off_the_grid() {
    let mut harness = with_lanes();
    let rect = lane(&harness);
    let pos = Pos2::new(rect.left() + 100.0, rect.center().y);
    harness.hover_at(pos);
    harness.run();
    harness.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    harness.drag_at(pos);
    harness.run();
    harness.drop_at(pos);
    harness.run();
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    let (points, _) = volume(&harness);
    assert_eq!(points.len(), 1, "{points:?}");
    assert!((points[0].beat - 12.5).abs() < 0.2, "{}", points[0].beat);
}

/// Dragging a point moves it on the grid and up, in one undo step.
#[test]
fn dragging_a_point_moves_it_on_the_grid_in_one_undo_step() {
    let mut harness = with_lanes();
    add_point(&mut harness, 100.0);
    let before = volume(&harness).0;
    let point = harness.get_by_label("Volume point 1 on Drums").rect().center();
    // Two bars later (64 points) and up.
    drag(&mut harness, point, vec2(64.0, -10.0));
    let (points, _) = volume(&harness);
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].beat, 20.0);
    assert!(points[0].value > before[0].value, "{points:?} {before:?}");

    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(volume(&harness).0, before);
}

/// A selected point goes with Delete, another with a double-click, and the lane stays shown.
#[test]
fn a_double_click_or_delete_removes_points() {
    let mut harness = with_lanes();
    add_point(&mut harness, 100.0);
    add_point(&mut harness, 200.0);
    assert_eq!(volume(&harness).0.len(), 2);

    // A click selects a point and Delete removes it.
    let point = harness.get_by_label("Volume point 2 on Drums").rect().center();
    click_at(&mut harness, point);
    harness.key_press(Key::Delete);
    harness.run();
    let (points, _) = volume(&harness);
    assert_eq!(points.iter().map(|p| p.beat).collect::<Vec<_>>(), [12.0]);

    // A double-click removes the last, and the lane goes with it.
    let at = harness.get_by_label("Volume point 1 on Drums").rect().center();
    harness.hover_at(at);
    harness.run();
    for _ in 0..2 {
        harness.hover_at(at);
        harness.drag_at(at);
        harness.drop_at(at);
        harness.step();
    }
    harness.run();
    assert!(volume(&harness).0.is_empty());
    // The lane stays shown, ready for a new point.
    assert!(harness.query_by_label("Volume automation on Drums").is_some());
}

/// Points selected with a box or Shift-click move together and are deleted in one step.
#[test]
fn selected_points_move_and_delete_together() {
    let mut harness = with_lanes();
    for x in [100.0, 200.0, 300.0] {
        add_point(&mut harness, x);
    }
    // A box around the first two selects them.
    let rect = lane(&harness);
    drag(&mut harness, Pos2::new(rect.left() + 60.0, rect.top() + 2.0), vec2(180.0, rect.height() - 4.0));
    let first = harness.get_by_label("Volume point 1 on Drums").rect().center();
    drag(&mut harness, first, vec2(32.0, 0.0));
    let beats: Vec<f64> = volume(&harness).0.iter().map(|p| p.beat).collect();
    assert_eq!(beats, [16.0, 28.0, 36.0]);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    let beats: Vec<f64> = volume(&harness).0.iter().map(|p| p.beat).collect();
    assert_eq!(beats, [12.0, 24.0, 36.0]);

    // Shift-click adds a point to the selection; Delete removes both in one step.
    let one = harness.get_by_label("Volume point 1 on Drums").rect().center();
    let three = harness.get_by_label("Volume point 3 on Drums").rect().center();
    click_at(&mut harness, one);
    harness.event(egui::Event::ModifiersChanged(Modifiers::SHIFT));
    click_at(&mut harness, three);
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    harness.key_press(Key::Delete);
    harness.run();
    let beats: Vec<f64> = volume(&harness).0.iter().map(|p| p.beat).collect();
    assert_eq!(beats, [24.0]);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(volume(&harness).0.len(), 3);
}

/// A lane a manual change turned off turns on again from its header's toggle.
#[test]
fn a_lane_turned_off_by_a_manual_change_turns_on_again_from_its_header() {
    let mut project = demo_project();
    let channel = project.tracks.iter().find(|t| t.name == "Drums").and_then(|t| t.channel);
    if let Some(channel) = project.channels.iter_mut().find(|c| Some(c.id) == channel) {
        let mut lane = suhina_engine::model::AutomationLane::new("volume");
        lane.insert(AutomationPoint::new(0.0, 0.5));
        lane.insert(AutomationPoint::new(8.0, 1.0));
        lane.enabled = false;
        channel.automation.push(lane);
    }
    let mut harness = arrangement(project);
    // The lane with points is the one shown first.
    harness.get_by_label("Show automation of Drums").click();
    harness.run();
    assert!(!volume(&harness).1);
    harness.get_by_label("Turn on again Volume automation on Drums").click();
    harness.run();
    assert!(volume(&harness).1);
    assert!(harness.query_by_label("Turn off Volume automation on Drums").is_some());
}

/// **+** adds a lane of the next parameter and **×** hides it again.
#[test]
fn a_lane_shows_another_parameter_and_more_lanes_are_added() {
    let mut harness = with_lanes();
    harness.get_by_label("Add an automation lane to Drums").click();
    harness.run();
    // The next parameter not shown: the pan.
    assert!(harness.query_by_label("Pan automation on Drums").is_some());
    harness.get_by_label("Hide Pan lane on Drums").click();
    harness.run();
    assert!(harness.query_by_label("Pan automation on Drums").is_none());
    assert!(harness.query_by_label("Volume automation on Drums").is_some());
}

/// Dragging the line between two points up bends the first point's curve, in one undo step.
#[test]
fn dragging_a_segment_up_bends_its_curve() {
    let mut harness = with_lanes();
    // The pan's lane: its values are spread evenly up the lane.
    harness.get_by_label("Add an automation lane to Drums").click();
    harness.run();
    let rect = harness.get_by_label("Pan automation on Drums").rect();
    click_at(&mut harness, Pos2::new(rect.left() + 100.0, rect.bottom() - 8.0));
    click_at(&mut harness, Pos2::new(rect.left() + 228.0, rect.top() + 8.0));
    let (points, _) = pan(&harness);
    assert_eq!(points.len(), 2);
    assert!(points[1].value > points[0].value);
    // The middle of the straight line between them, dragged up: a rising segment moves fast first.
    let a = harness.get_by_label("Pan point 1 on Drums").rect().center();
    let b = harness.get_by_label("Pan point 2 on Drums").rect().center();
    let middle = Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    drag(&mut harness, middle, vec2(0.0, -20.0));
    let (points, _) = pan(&harness);
    assert!(points[0].curve.shape(0.5) > 0.55, "{points:?}");
    assert!(points[1].curve.is_straight());
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(pan(&harness).0[0].curve.is_straight());
}

/// The demo project with a rising pan segment on Drums whose curve is a split's half outside the
/// unit square, and the Drums lanes shown; with where the segment's line is half-way along.
fn with_a_split_half() -> (Harness<'static, App>, Curve, Pos2) {
    let outside =
        Curve { x1: 1.195_800_352_596_018_6, y1: 0.0, x2: 0.246_692_986_530_421_18, y2: 0.828_981_542_001_562_4 };
    let mut project = demo_project();
    let drums = project.tracks.iter().find(|t| t.name == "Drums").and_then(|t| t.channel);
    let channel = project.channels.iter_mut().find(|c| Some(c.id) == drums).expect("the Drums channel");
    let mut pan = AutomationLane::new("pan");
    pan.insert(AutomationPoint { beat: 0.0, value: -1.0, curve: outside });
    pan.insert(AutomationPoint::new(16.0, 1.0));
    channel.automation.push(pan);
    let mut harness = arrangement(project);
    harness.get_by_label("Show automation of Drums").click();
    harness.run();
    let rect = harness.get_by_label("Pan automation on Drums").rect();
    let a = harness.get_by_label("Pan point 1 on Drums").rect().center();
    let b = harness.get_by_label("Pan point 2 on Drums").rect().center();
    let x = (a.x + b.x) / 2.0;
    let y = envelope_y_at(&harness, rect, x).expect("the envelope is drawn");
    (harness, outside, Pos2::new(x, y))
}

/// A drag of a split half's line shorter than one step still holds its control points in the
/// unit square, each coordinate on its own, and moves nothing else, as measured.
#[test]
fn a_drag_under_a_step_holds_a_split_half_in_the_square() {
    let (mut harness, outside, middle) = with_a_split_half();
    assert_eq!(pan(&harness).0[0].curve, outside);
    // Sideways only: the drag starts but moves no step.
    drag(&mut harness, middle, vec2(12.0, 0.0));
    let held = pan(&harness).0[0].curve;
    let want = outside.boxed();
    assert!(held.to_array().iter().zip(want.to_array()).all(|(a, b)| (a - b).abs() < 1e-9), "{held:?}");
    assert_eq!(held.x1, 1.0);
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(pan(&harness).0[0].curve, outside, "undo brings the split half back");
}

/// A longer drag holds the curve first, then moves it along the paths from the held curve.
#[test]
fn a_drag_of_a_split_half_moves_from_the_held_curve() {
    let (mut harness, outside, middle) = with_a_split_half();
    drag(&mut harness, middle, vec2(0.0, -20.0));
    let got = pan(&harness).0[0].curve;
    let paths = automation::CurveDrag::new(&outside);
    let steps = (-40..=-1)
        .find(|&n| paths.dragged(n).to_array().iter().zip(got.to_array()).all(|(a, b)| (a - b).abs() < 1e-12));
    assert!(steps.is_some(), "{got:?} is not the held curve dragged up");
    assert!(got.to_array().iter().all(|v| (0.0..=1.0).contains(v)));
}

/// The height of the envelope line painted last frame at `x`, from the longest line inside `lane`.
fn envelope_y_at(harness: &Harness<'static, App>, lane: egui::Rect, x: f32) -> Option<f32> {
    fn walk(shape: &egui::Shape, out: &mut Vec<Vec<Pos2>>) {
        match shape {
            egui::Shape::Vec(inner) => inner.iter().for_each(|s| walk(s, out)),
            egui::Shape::Path(path) => out.push(path.points.clone()),
            _ => {}
        }
    }
    let mut lines = Vec::new();
    for clipped in &harness.output().shapes {
        if clipped.clip_rect.intersects(lane) {
            walk(&clipped.shape, &mut lines);
        }
    }
    let inside = |line: &Vec<Pos2>| line.iter().all(|p| lane.expand(1.0).contains(*p));
    let line = lines.into_iter().filter(inside).max_by_key(Vec::len)?;
    line.iter().min_by(|a, b| (a.x - x).abs().total_cmp(&(b.x - x).abs())).map(|p| p.y)
}

/// The frame the mouse is let go in already draws the new bend: the lane no longer goes back to
/// the segment's old shape for a frame until the command has run.
#[test]
fn a_bent_segment_keeps_its_new_shape_when_the_mouse_is_let_go() {
    let mut harness = with_lanes();
    harness.get_by_label("Add an automation lane to Drums").click();
    harness.run();
    let rect = harness.get_by_label("Pan automation on Drums").rect();
    click_at(&mut harness, Pos2::new(rect.left() + 100.0, rect.bottom() - 8.0));
    click_at(&mut harness, Pos2::new(rect.left() + 228.0, rect.top() + 8.0));
    let a = harness.get_by_label("Pan point 1 on Drums").rect().center();
    let b = harness.get_by_label("Pan point 2 on Drums").rect().center();
    let middle = Pos2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let straight = envelope_y_at(&harness, rect, middle.x).expect("the envelope is drawn");
    // Drag the middle up, as `drag` does, but look at the frame the button is let go in.
    let by = vec2(0.0, -60.0);
    harness.hover_at(middle);
    harness.run();
    harness.drag_at(middle);
    harness.run();
    harness.hover_at(middle + by * 0.5);
    harness.run();
    harness.hover_at(middle + by);
    harness.run();
    let dragging = envelope_y_at(&harness, rect, middle.x).expect("the envelope is drawn");
    assert!(dragging < straight - 3.0, "bent while dragging: {dragging} against {straight}");
    // The release alone, so this step is the one frame it is handled in (`drop_at` queues more
    // events, and the frame of the last one would be the one looked at).
    harness.event(egui::Event::PointerButton {
        pos: middle + by,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    let released = envelope_y_at(&harness, rect, middle.x).expect("the envelope is drawn");
    assert!((released - dragging).abs() < 0.5, "on release: {released}, while dragging: {dragging}");
    harness.run();
    let after = envelope_y_at(&harness, rect, middle.x).expect("the envelope is drawn");
    assert!((after - dragging).abs() < 0.5, "after: {after}, while dragging: {dragging}");
    assert!(pan(&harness).0[0].curve.shape(0.5) > 0.55);
}

/// The demo project with the FM synth, which has over a hundred parameters, on the Drums
/// channel and its lanes shown; the volume lane's parameter menu is open.
fn with_the_menu_open() -> (Harness<'static, App>, String) {
    let info = suhina_device_registry::info("fm-synth").expect("the FM synth is built in");
    let last = info.params.last().expect("the FM synth has parameters").name.to_string();
    (menu_with(&[("fm-synth", "FM Synth")]), last)
}

/// The demo project with built-in `devices` (id and name) on the Drums channel and its lanes
/// shown; the volume lane's parameter menu is open.
fn menu_with(devices: &[(&str, &str)]) -> Harness<'static, App> {
    menu_in(devices, SIZE)
}

/// [`menu_with`] in a window of `size`.
fn menu_in(devices: &[(&str, &str)], size: (f32, f32)) -> Harness<'static, App> {
    menu_on(devices, size, 0, true)
}

/// [`menu_in`], on the demo's track `track` (counting from 0) rather than the Drums, and with
/// the detail area under the arrangement shown or not (`detail`).
fn menu_on(devices: &[(&str, &str)], size: (f32, f32), track: usize, detail: bool) -> Harness<'static, App> {
    let mut project = demo_project();
    let name = project.tracks[track].name.clone();
    let channel = project.tracks[track].channel;
    let channel = project.channels.iter_mut().find(|c| Some(c.id) == channel).expect("the track has a channel");
    for (id, device) in devices {
        let info = suhina_device_registry::info(id).expect("a built-in device");
        let params = info.params.iter().map(|p| DeviceParam::new(p.id, p.default)).collect();
        channel.devices.push(Device {
            name: (*device).into(),
            kind: DeviceKind::Builtin((*id).into()),
            enabled: true,
            state: None,
            params,
            automation: Vec::new(),
            clip_host: false,
            extensions: Extensions::default(),
        });
    }
    let mut harness = arrangement_sized(project, size);
    if !detail {
        harness.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
        harness.run();
    }
    harness.get_by_label(&format!("Show automation of {name}")).click();
    harness.run();
    harness.get_by_label(&format!("Parameter of Volume lane on {name}")).click();
    harness.run();
    harness
}

/// A long list scrolls inside the window and opens on the lane's own parameter.
#[test]
fn the_parameter_menu_of_a_device_with_many_parameters_scrolls_to_the_current_one() {
    let (harness, _) = with_the_menu_open();
    let search = harness.get_by_label("Search parameters").rect();
    assert!(search.bottom() < SIZE.1, "{search:?}");
    // The list is longer than the window and the rows below it are clipped, not pushed out of
    // the window; the lane's own parameter, the volume, is scrolled into view.
    let on_screen = harness.get_all_by_label("Volume").filter(|n| n.rect().bottom() < SIZE.1).count();
    assert!(on_screen >= 1);
}

/// A search finds the last parameter of a long device, and choosing it shows it in the lane.
#[test]
fn searching_the_parameter_menu_reaches_the_last_parameter_of_a_long_device() {
    let (mut harness, last) = with_the_menu_open();
    harness.get_by_label("Search parameters").type_text(&last);
    harness.run();
    let label = format!("FM Synth: {last}");
    harness.get_by_label(&label).click();
    harness.run();
    assert!(harness.query_by_label(&format!("{label} automation on Drums")).is_some());
    assert!(harness.query_by_label("Volume automation on Drums").is_none(), "the lane showed another parameter");
}

/// Enter takes the first match as listed: a device's parameters without a group come first.
#[test]
fn enter_takes_the_first_match() {
    let (mut harness, _) = with_the_menu_open();
    // The first listed: the device's parameters without a group (its volume) come before its
    // groups, though the operators come first in the device's own order.
    harness.get_by_label("Search parameters").type_text("fm synth");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.query_by_label("FM Synth: Volume automation on Drums").is_some());
}

/// Enter takes the first match inside a group that a search by its name finds.
#[test]
fn enter_takes_the_first_match_inside_a_group() {
    let (mut harness, _) = with_the_menu_open();
    // A group's name finds its parameters.
    harness.get_by_label("Search parameters").type_text("lfo");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.query_by_label("FM Synth: LFO Speed automation on Drums").is_some());
}

/// A search that matches nothing says so.
#[test]
fn a_search_with_no_match_says_so() {
    let (mut harness, _) = with_the_menu_open();
    harness.get_by_label("Search parameters").type_text("no such parameter");
    harness.run();
    assert!(harness.query_by_label("No parameter matches").is_some());
}

/// A device's grouped parameters sit under closed group headings.
#[test]
fn the_parameter_menu_gathers_a_devices_parameters_into_closed_groups() {
    let (harness, _) = with_the_menu_open();
    // Each operator is a group heading; its parameters stay hidden until it opens.
    assert!(harness.query_by_label("Op 3").is_some());
    assert!(harness.query_by_label("LFO").is_some());
    assert!(harness.query_by_label("FM Synth: Op 3 Rate 1").is_none());
    // A parameter without a group is listed under the device's name.
    assert!(harness.query_by_label("FM Synth: Volume").is_some());
}

/// A search opens only the groups with matches, and a row there chooses its parameter.
#[test]
fn a_search_opens_the_groups_it_finds_and_shows_names_under_their_group() {
    let (mut harness, _) = with_the_menu_open();
    harness.get_by_label("Search parameters").type_text("op 3 coarse");
    harness.run();
    assert!(harness.query_by_label("Op 3").is_some());
    assert!(harness.query_by_label("Op 1").is_none(), "a group without a match is left out");
    // The row reads "Coarse" under the "Op 3" heading; its accessible label is the whole name.
    assert!(harness.query_by_label("FM Synth: Op 3 Fine").is_none());
    harness.get_by_label("FM Synth: Op 3 Coarse").click();
    harness.run();
    assert!(harness.query_by_label("FM Synth: Op 3 Coarse automation on Drums").is_some());
}

/// The menu keeps its own width rather than filling the window.
#[test]
fn the_parameter_menu_stays_narrow() {
    let (harness, _) = with_the_menu_open();
    let search = harness.get_by_label("Search parameters").rect();
    assert!(search.width() <= 260.0, "{search:?}");
    assert!(harness.get_by_label("Op 3").rect().width() <= 260.0);
}

/// A click in the search field leaves the menu open.
#[test]
fn clicking_the_search_field_keeps_the_menu_open() {
    let (mut harness, _) = with_the_menu_open();
    harness.get_by_label("Search parameters").click();
    harness.run();
    assert!(harness.query_by_label("Search parameters").is_some());
    assert!(harness.query_by_label("Op 3").is_some());
}

/// A group heading opens and closes with a click while the menu stays open; choosing a
/// parameter closes it.
#[test]
fn a_click_on_a_group_heading_opens_and_closes_it_and_keeps_the_menu_open() {
    let (mut harness, _) = with_the_menu_open();
    harness.get_by_label("Op 3").click();
    harness.run();
    assert!(harness.query_by_label("Search parameters").is_some(), "the menu closed");
    assert!(harness.query_by_label("FM Synth: Op 3 Rate 1").is_some());
    harness.get_by_label("Op 3").click();
    harness.run();
    assert!(harness.query_by_label("Search parameters").is_some(), "the menu closed");
    assert!(harness.query_by_label("FM Synth: Op 3 Rate 1").is_none());
    // Choosing a parameter still closes it.
    harness.get_by_label("FM Synth: Volume").click();
    harness.run();
    assert!(harness.query_by_label("Search parameters").is_none());
    assert!(harness.query_by_label("FM Synth: Volume automation on Drums").is_some());
}

/// A click outside the menu closes it.
#[test]
fn a_click_outside_closes_the_parameter_menu() {
    let (mut harness, _) = with_the_menu_open();
    // The status bar, far from the menu.
    click_at(&mut harness, Pos2::new(5.0, SIZE.1 - 5.0));
    assert!(harness.query_by_label("Search parameters").is_none());
}

/// Several devices each fold under their name, and a device closed and opened again keeps its
/// groups as they were.
#[test]
fn with_several_devices_each_folds_under_its_name() {
    let mut harness = menu_with(&[("fm-synth", "FM Synth"), ("utility", "Utility")]);
    // The lane shows the volume, so both devices start closed.
    assert!(harness.query_by_label("FM Synth").is_some());
    assert!(harness.query_by_label("Op 3").is_none());
    assert!(harness.query_by_label("Utility: Gain").is_none());
    harness.get_by_label("Utility").click();
    harness.run();
    assert!(harness.query_by_label("Utility: Gain").is_some());
    // Closed again, so what follows fits in the list without scrolling.
    harness.get_by_label("Utility").click();
    harness.run();
    assert!(harness.query_by_label("Utility: Gain").is_none());
    // A device closed and opened again keeps its open groups.
    harness.get_by_label("FM Synth").click();
    harness.run();
    harness.get_by_label("Op 3").click();
    harness.run();
    harness.get_by_label("FM Synth").click();
    harness.run();
    assert!(harness.query_by_label("FM Synth: Op 3 Rate 1").is_none());
    harness.get_by_label("FM Synth").click();
    harness.run();
    assert!(harness.query_by_label("FM Synth: Op 3 Rate 1").is_some());
    assert!(harness.query_by_label("Search parameters").is_some(), "the menu closed");
}

/// The menu opens on the device and group of the lane's parameter, and a search opens the
/// devices closed around it.
#[test]
fn the_menu_opens_the_device_and_group_of_the_lanes_parameter_and_a_search_opens_the_rest() {
    let mut harness = menu_with(&[("fm-synth", "FM Synth"), ("utility", "Utility")]);
    harness.get_by_label("Search parameters").type_text("op 3 coarse");
    harness.run();
    harness.get_by_label("FM Synth: Op 3 Coarse").click();
    harness.run();
    harness.get_by_label("Parameter of FM Synth: Op 3 Coarse lane on Drums").click();
    harness.run();
    // Its device and group are open, the other device closed, and the search is empty again.
    assert!(harness.query_by_label("FM Synth: Op 3 Coarse").is_some());
    assert!(harness.query_by_label("FM Synth: Op 1 Rate 1").is_none());
    assert!(harness.query_by_label("Utility: Gain").is_none());
    harness.get_by_label("Search parameters").type_text("gain");
    harness.run();
    assert!(harness.query_by_label("Utility: Gain").is_some());
}

/// A lane switched to another parameter and back opens its menu afresh: no search left over,
/// and only the lane's own device and group open.
#[test]
fn a_lane_switched_back_opens_its_menu_afresh() {
    let mut harness = menu_with(&[("fm-synth", "FM Synth"), ("utility", "Utility")]);
    harness.get_by_label("Search parameters").type_text("op 3 coarse");
    harness.run();
    harness.get_by_label("FM Synth: Op 3 Coarse").click();
    harness.run();
    harness.get_by_label("Parameter of FM Synth: Op 3 Coarse lane on Drums").click();
    harness.run();
    // The list opens on the coarse tuning, the mixer's volume scrolled out of view above it.
    harness.get_by_label("Search parameters").type_text("volume");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    harness.get_by_label("Parameter of Volume lane on Drums").click();
    harness.run();
    // Left over, the search the volume lane chose with would show the coarse tuning again.
    assert!(harness.query_by_label("FM Synth: Op 3 Coarse").is_none());
    assert!(harness.query_by_label("Op 3").is_none(), "the FM synth was left open");
    assert!(harness.query_by_label("Utility").is_some());
}

/// As headings open, the menu grows in place under its dropdown and stays inside the window.
#[test]
fn the_parameter_menu_stays_where_it_opened_as_it_grows() {
    // A window short enough that the opened list is taller than the room under the dropdown.
    let size = (1280.0, 500.0);
    let mut harness = menu_in(&[("fm-synth", "FM Synth"), ("utility", "Utility"), ("reverb", "Reverb")], size);
    let search = harness.get_by_label("Search parameters").rect();
    let menu = |harness: &Harness<'static, App>| {
        let layer = harness.ctx.layer_id_at(search.center());
        layer.and_then(|l| harness.ctx.memory(|m| m.area_rect(l.id))).expect("the menu is open")
    };
    let dropdown = harness.get_by_label("Parameter of Volume lane on Drums").rect();
    assert!(menu(&harness).top() >= dropdown.bottom() - 1.0, "the menu opens under the dropdown");
    // Each opened from the last up, so the next heading stays in view.
    for heading in ["Reverb", "Utility", "FM Synth"] {
        harness.get_by_label(heading).click();
        harness.run();
        assert_eq!(harness.get_by_label("Search parameters").rect(), search, "the menu moved after {heading} opened");
        let menu = menu(&harness);
        assert!(menu.bottom() <= size.1, "the menu runs past the window: {menu:?}");
    }
}

/// Over its dropdown, the menu stays open and against the dropdown in every frame as the
/// device of the lane's parameter closes.
#[test]
fn over_the_dropdown_the_menu_stays_open_and_in_place_as_the_lanes_device_closes() {
    // A window so short, without the detail area, that the Keys lane's menu opens over its
    // dropdown.
    let mut harness = menu_on(&[("fm-synth", "FM Synth"), ("utility", "Utility")], (1280.0, 480.0), 2, false);
    harness.get_by_label("Search parameters").type_text("utility gain");
    harness.run();
    harness.get_by_label("Utility: Gain").click();
    harness.run();
    let dropdown = "Parameter of Utility: Gain lane on Keys";
    harness.get_by_label(dropdown).click();
    harness.run();
    let top = harness.get_by_label(dropdown).rect().top();
    let menu = |harness: &Harness<'static, App>| {
        let search = harness.get_by_label("Search parameters").rect().center();
        let layer = harness.ctx.layer_id_at(search);
        layer.and_then(|l| harness.ctx.memory(|m| m.area_rect(l.id))).expect("the menu is open")
    };
    assert!((menu(&harness).bottom() - top).abs() <= 1.0, "the menu opens over the dropdown");
    assert!(harness.query_by_label("Utility: Gain").is_some());
    let heading = harness.get_by_label("Utility").rect();
    assert!(menu(&harness).contains_rect(heading), "the device's heading is in view");

    // Closing the device of the lane's parameter shrinks the menu from under the click: it
    // stays open, and each frame shows it against the dropdown, not first in the old place.
    harness.get_by_label("Utility").click();
    for _ in 0..4 {
        harness.step();
        assert!(harness.query_by_label("Search parameters").is_some(), "the menu closed");
        assert!((menu(&harness).bottom() - top).abs() <= 1.0, "the menu was drawn away from the dropdown");
    }
    assert!(harness.query_by_label("Utility: Gain").is_none());
    // The lane still shows its parameter.
    assert!(harness.query_by_label(dropdown).is_some());
}

/// The demo project with the FM synth and the utility on the Drums channel, its volume and the
/// utility's gain automated, and its lanes shown (those two, as they have points).
fn with_automation() -> Harness<'static, App> {
    with_automation_of(&[])
}

/// [`with_automation`], with the FM synth's parameters `fm` (by id) automated as well.
fn with_automation_of(fm: &[&str]) -> Harness<'static, App> {
    let mut project = demo_project();
    let channel = project.tracks.iter().find(|t| t.name == "Drums").and_then(|t| t.channel);
    let channel = project.channel_mut(channel.expect("Drums has a channel"));
    let channel = channel.expect("the Drums channel");
    let points = |id: &str, values: [f64; 2]| {
        let mut lane = suhina_engine::model::AutomationLane::new(id);
        lane.insert(AutomationPoint::new(0.0, values[0]));
        lane.insert(AutomationPoint::new(8.0, values[1]));
        lane
    };
    channel.automation.push(points("volume", [0.5, 1.0]));
    for (id, name) in [("fm-synth", "FM Synth"), ("utility", "Utility")] {
        let info = suhina_device_registry::info(id).expect("a built-in device");
        let params = info.params.iter().map(|p| DeviceParam::new(p.id, p.default)).collect();
        let automation = match id {
            "utility" => vec![points("gain", [-6.0, 0.0])],
            _ => fm.iter().map(|p| points(p, [0.0, 1.0])).collect(),
        };
        channel.devices.push(Device {
            name: name.into(),
            kind: DeviceKind::Builtin(id.into()),
            enabled: true,
            state: None,
            params,
            automation,
            clip_host: false,
            extensions: Extensions::default(),
        });
    }
    let mut harness = arrangement(project);
    harness.get_by_label("Show automation of Drums").click();
    harness.run();
    harness
}

/// Every automated parameter of the track is listed again at the top under "Automated", a
/// device's with its device's name, while its device stays closed.
#[test]
fn the_parameter_menu_lists_the_automated_parameters_first() {
    let mut harness = with_automation();
    harness.get_by_label("Parameter of Volume lane on Drums").click();
    harness.run();
    let heading = harness.get_by_label("Automated").rect();
    let volume = harness.get_by_label("Volume (automated)").rect();
    let gain = harness.get_by_label("Utility: Gain (automated)").rect();
    let pan = harness.get_by_label("Pan").rect();
    assert!(heading.bottom() <= volume.top() && volume.bottom() <= gain.top(), "{heading:?} {volume:?} {gain:?}");
    assert!(gain.bottom() <= pan.top(), "the section comes before the mixer's parameters");
    // The FM synth has no points, and the utility stays closed: its gain is not listed twice.
    assert!(harness.query_by_label("FM Synth: Volume (automated)").is_none());
    assert!(harness.query_by_label("Utility: Gain").is_none());
    assert!(harness.query_by_label("Utility").is_some());
}

/// A search narrows the "Automated" section like the rest, and leaves it out when none of its
/// parameters match.
#[test]
fn a_search_narrows_the_automated_parameters() {
    let mut harness = with_automation();
    harness.get_by_label("Parameter of Volume lane on Drums").click();
    harness.run();
    harness.get_by_label("Search parameters").type_text("gain");
    harness.run();
    assert!(harness.query_by_label("Utility: Gain (automated)").is_some());
    assert!(harness.query_by_label("Volume (automated)").is_none());
    // Its place in its device is listed as well.
    assert!(harness.query_by_label("Utility: Gain").is_some());
    harness.get_by_label("Search parameters").type_text(" coarse");
    harness.run();
    assert!(harness.query_by_label("Automated").is_none());
}

/// Choosing a parameter in the "Automated" section shows it in the lane.
#[test]
fn choosing_an_automated_parameter_at_the_top_switches_the_lane() {
    let mut harness = with_automation();
    // The gain's own lane is hidden, so the volume lane may show it.
    harness.get_by_label("Hide Utility: Gain lane on Drums").click();
    harness.run();
    harness.get_by_label("Parameter of Volume lane on Drums").click();
    harness.run();
    harness.get_by_label("Utility: Gain (automated)").click();
    harness.run();
    assert!(harness.query_by_label("Search parameters").is_none(), "the menu stayed open");
    assert!(harness.query_by_label("Utility: Gain automation on Drums").is_some());
    assert!(harness.query_by_label("Volume automation on Drums").is_none(), "the lane kept the volume");
}

/// The open parameter menu's own area.
fn menu_area(harness: &Harness<'static, App>) -> egui::Rect {
    let search = harness.get_by_label("Search parameters").rect().center();
    let layer = harness.ctx.layer_id_at(search);
    layer.and_then(|l| harness.ctx.memory(|m| m.area_rect(l.id))).expect("the menu is open")
}

/// Whether the row labelled `label` is in the open menu's list, not scrolled out of it.
fn in_view(harness: &Harness<'static, App>, label: &str) -> bool {
    let list_top = harness.get_by_label("Search parameters").rect().bottom();
    let row = harness.get_by_label(label).rect();
    row.top() >= list_top && row.bottom() <= menu_area(harness).bottom()
}

/// The FM synth's first operator's last parameter automated, and its lane hidden, so the
/// volume lane may show it; the volume lane's menu open.
fn op_1_on_automated() -> Harness<'static, App> {
    let mut harness = with_automation_of(&["op1.on"]);
    harness.get_by_label("Hide FM Synth: Op 1 On lane on Drums").click();
    harness.run();
    harness.get_by_label("Parameter of Volume lane on Drums").click();
    harness.run();
    harness
}

/// A parameter chosen under "Automated" opens the menu there again, with its device and group
/// open below.
#[test]
fn the_menu_opens_on_the_automated_copy_it_was_chosen_from() {
    let mut harness = op_1_on_automated();
    harness.get_by_label("FM Synth: Op 1 On (automated)").click();
    harness.run();
    let dropdown = "Parameter of FM Synth: Op 1 On lane on Drums";
    harness.get_by_label(dropdown).click();
    harness.run();
    assert!(in_view(&harness, "FM Synth: Op 1 On (automated)"));
    // Its own row is listed too, in its open group, below the list's end.
    assert!(harness.query_by_label("FM Synth: Op 1 On").is_some());
    assert!(!in_view(&harness, "FM Synth: Op 1 On"), "the list scrolled to the parameter's own row");
}

/// A parameter chosen in its device opens the menu on its own row again, though it is
/// automated.
#[test]
fn the_menu_opens_on_the_row_in_its_device_it_was_chosen_from() {
    let mut harness = op_1_on_automated();
    harness.get_by_label("Search parameters").type_text("op 1 on");
    harness.run();
    harness.get_by_label("FM Synth: Op 1 On").click();
    harness.run();
    let dropdown = "Parameter of FM Synth: Op 1 On lane on Drums";
    harness.get_by_label(dropdown).click();
    harness.run();
    assert!(in_view(&harness, "FM Synth: Op 1 On"));
    assert!(!in_view(&harness, "FM Synth: Op 1 On (automated)"), "the list scrolled to the top");
}

/// With nothing automated, the menu has no "Automated" section.
#[test]
fn without_automation_the_menu_has_no_automated_section() {
    let harness = menu_with(&[("utility", "Utility")]);
    assert!(harness.query_by_label("Search parameters").is_some());
    assert!(harness.query_by_label("Automated").is_none());
    assert!(harness.query_by_label("Volume (automated)").is_none());
}

/// Renders the Arrangement view with lanes shown to `target/ui-snapshots/automation-lanes.png`,
/// for looking at: `cargo test -p suhina-ui --test automation_lanes -- --ignored`.
#[test]
#[ignore = "needs a GPU adapter; run with --ignored"]
fn render_the_lanes() {
    let mut project = demo_project();
    let channel = project.tracks.iter().find(|t| t.name == "Drums").and_then(|t| t.channel);
    if let Some(channel) = project.channels.iter_mut().find(|c| Some(c.id) == channel) {
        let mut volume = suhina_engine::model::AutomationLane::new("volume");
        for (beat, value, curve) in [(0.0, 0.3, 0.0), (16.0, 1.0, 0.6), (48.0, 0.5, -0.5), (64.0, 0.9, 0.0)] {
            volume.insert(AutomationPoint { beat, value, curve: suhina_engine::model::automation::Curve::bend(curve) });
        }
        let mut pan = suhina_engine::model::AutomationLane::new("pan");
        pan.insert(AutomationPoint::new(8.0, -0.6));
        pan.insert(AutomationPoint::new(40.0, 0.6));
        pan.enabled = false;
        channel.automation.extend([volume, pan]);
    }
    let mut app = App::new();
    app.set_browser_open(false);
    // These tests place points on a bar grid.
    app.set_arrangement_grid(suhina_ui::arrangement::arrangement_view::PianoGrid::from_value("1 bar"));
    app.set_session(Session::new(project), &[]);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_pixels_per_point(2.0)
        .with_step_dt(0.05)
        .with_max_steps(40)
        .wgpu()
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness.get_by_label("Arrangement view").click();
    harness.run();
    harness.get_by_label("Show automation of Drums").click();
    harness.run();
    // A lane without points: the mute's faint line.
    if let Some(add) = harness.get_all_by_label("Add an automation lane to Drums").last() {
        add.click();
    }
    harness.run();
    let one = harness.get_by_label("Volume point 2 on Drums").rect().center();
    click_at(&mut harness, one);
    harness.remove_cursor();
    harness.run();
    let image = harness.render().unwrap_or_else(|e| panic!("render failed: {e}"));
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&out).unwrap_or_else(|e| panic!("{e}"));
    image.save(out.join("automation-lanes.png")).unwrap_or_else(|e| panic!("{e}"));
}

/// A click near the envelope adds a point on it, keeping a level segment level; a click away
/// from it adds the point at the pointer's height.
#[test]
fn a_click_near_the_envelope_adds_a_point_on_it() {
    let mut harness = with_lanes();
    // A level segment: the second point, added on the line held after the first, keeps its value.
    add_point(&mut harness, 100.0);
    add_point(&mut harness, 300.0);
    let (points, _) = volume(&harness);
    assert_eq!(points.len(), 2, "{points:?}");
    assert_eq!(points[0].value, points[1].value);
    // A click a little under the line, between them, adds a point on it: still level.
    let rect = lane(&harness);
    click_at(&mut harness, Pos2::new(rect.left() + 200.0, rect.center().y + 3.0));
    let (points, _) = volume(&harness);
    assert_eq!(points.len(), 3, "{points:?}");
    assert!(points.iter().all(|p| p.value == points[0].value), "{points:?}");
    // Further from the line, a point goes where the pointer is.
    click_at(&mut harness, Pos2::new(rect.left() + 400.0, rect.top() + 6.0));
    let (points, _) = volume(&harness);
    assert!(points[3].value > points[0].value, "{points:?}");
}

/// A click near the faint line of a lane without points adds a point at the parameter's value.
#[test]
fn a_lane_without_points_adds_one_on_the_parameters_value() {
    let mut harness = with_lanes();
    harness.get_by_label("Add an automation lane to Drums").click();
    harness.run();
    let set = {
        let project = harness.state().session().map(|s| s.project().clone());
        let channel = project.as_ref().and_then(|p| {
            let id = p.tracks.iter().find(|t| t.name == "Drums").and_then(|t| t.channel);
            p.channels.iter().find(|c| Some(c.id) == id).cloned()
        });
        channel.map_or(0.0, |c| c.pan)
    };
    // The faint line sits at the pan's value; a click just off it adds a point on it.
    let rect = harness.get_by_label("Pan automation on Drums").rect();
    let y = rect.bottom() - 4.0 - ((set as f32 + 1.0) / 2.0) * (rect.height() - 8.0);
    click_at(&mut harness, Pos2::new(rect.left() + 100.0, y + 3.0));
    let (points, _) = pan(&harness);
    assert_eq!(points.len(), 1, "{points:?}");
    assert_eq!(points[0].value, set);
}

/// A drag with Alt (Option on macOS) moves a point off the grid.
#[test]
fn alt_drags_a_point_off_the_grid() {
    let mut harness = with_lanes();
    add_point(&mut harness, 100.0);
    let point = harness.get_by_label("Volume point 1 on Drums").rect().center();
    // 36 points is four and a half beats: the grid (a bar) would take it to beat 16.
    harness.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    drag(&mut harness, point, vec2(36.0, 0.0));
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    let (points, _) = volume(&harness);
    assert_eq!(points.len(), 1);
    assert!((points[0].beat - 16.5).abs() < 0.2, "{points:?}");
}

/// The line under a lane drags it taller, the height stays while the lanes are hidden, and a
/// double-click on the line puts it back (#818).
#[test]
fn a_lanes_bottom_edge_drags_its_height_and_a_double_click_resets_it() {
    let mut harness = with_lanes();
    let before = lane(&harness).height();
    let edge = Pos2::new(lane(&harness).center().x, lane(&harness).bottom());
    drag(&mut harness, edge, vec2(0.0, 60.0));
    let taller = lane(&harness).height();
    assert!((taller - (before + 60.0)).abs() < 2.0, "{before} -> {taller}");
    harness.get_by_label("Hide automation of Drums").click();
    harness.run();
    harness.get_by_label("Show automation of Drums").click();
    harness.run();
    assert!((lane(&harness).height() - taller).abs() < 0.5);
    let edge = Pos2::new(lane(&harness).center().x, lane(&harness).bottom());
    harness.hover_at(edge);
    harness.run();
    for _ in 0..2 {
        harness.hover_at(edge);
        harness.drag_at(edge);
        harness.drop_at(edge);
        harness.step();
    }
    harness.run();
    assert!((lane(&harness).height() - before).abs() < 0.5, "{}", lane(&harness).height());
}

/// A drag on a lane paints a time range: Ctrl+C copies it, Ctrl+D copies it right after itself,
/// and Ctrl+V pastes it at the start of a range painted elsewhere (#916).
#[test]
fn a_painted_range_copies_duplicates_and_pastes() {
    let mut harness = with_lanes();
    for x in [100.0, 200.0] {
        add_point(&mut harness, x);
    }
    let beats = |h: &Harness<'static, App>| -> Vec<f64> { volume(h).0.iter().map(|p| p.beat).collect() };
    assert_eq!(beats(&harness), [12.0, 24.0]);
    // Bars 3 to 8 (beats 8 to 32), on the grid, painted from the lane's upper half.
    let rect = lane(&harness);
    drag(&mut harness, Pos2::new(rect.left() + 60.0, rect.top() + rect.height() * 0.25), vec2(180.0, 0.0));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::C);
    harness.run();
    harness.key_press_modifiers(Modifiers::COMMAND, Key::D);
    harness.run();
    let after = beats(&harness);
    assert!(after.contains(&36.0) && after.contains(&48.0), "the copy is right after the range: {after:?}");
    // A range painted at beat 48: Ctrl+V pastes the copied range from there.
    drag(&mut harness, Pos2::new(rect.left() + 392.0, rect.top() + rect.height() * 0.25), vec2(32.0, 0.0));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    let after = beats(&harness);
    assert!(after.contains(&52.0) && after.contains(&64.0), "pasted from beat 48: {after:?}");
    // Delete clears a range (beats 48 to 72), keeping the values at its edges.
    let pasted = beats(&harness);
    drag(&mut harness, Pos2::new(rect.left() + 392.0, rect.top() + rect.height() * 0.25), vec2(192.0, 0.0));
    harness.key_press(Key::Delete);
    harness.run();
    let deleted = beats(&harness);
    assert!(!deleted.iter().any(|b| *b > 48.0 && *b < 72.0), "{deleted:?}");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert_eq!(beats(&harness), pasted);
}

/// A right-click at `at`.
fn right_click_at(harness: &mut Harness<'static, App>, at: Pos2) {
    harness.hover_at(at);
    harness.run();
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();
}

/// The lane's menu inside a painted range fills it with a shape; away from a range it chooses the
/// shape the next points' segments take, and draws nothing (#916).
#[test]
fn the_lane_menu_fills_a_range_or_chooses_the_shape_for_new_points() {
    let mut harness = with_lanes();
    let rect = lane(&harness);
    // Beats 32 to 64, painted.
    drag(&mut harness, Pos2::new(rect.left() + 256.0, rect.top() + rect.height() * 0.4), vec2(256.0, 0.0));
    right_click_at(&mut harness, Pos2::new(rect.left() + 384.0, rect.center().y));
    harness.get_by_label("Insert sine").click();
    harness.run();
    let points = volume(&harness).0;
    assert!(points.len() > 10, "{}", points.len());
    assert!(points.iter().all(|p| (32.0..=64.0).contains(&p.beat)), "{points:?}");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    harness.run();
    assert!(volume(&harness).0.is_empty());

    // Away from the range: nothing is drawn, the triangle is the shape for new points.
    right_click_at(&mut harness, Pos2::new(rect.left() + 700.0, rect.center().y));
    harness.get_by_label("Insert triangle").click();
    harness.run();
    assert!(volume(&harness).0.is_empty(), "choosing it drew nothing");
    add_point(&mut harness, 100.0);
    add_point(&mut harness, 200.0);
    let beats: Vec<f64> = volume(&harness).0.iter().map(|p| p.beat).collect();
    assert!(beats.len() > 2 && beats.first() == Some(&12.0) && beats.last() == Some(&24.0), "{beats:?}");
}

/// A point's menu gives the segment from the point before it a shape, between their heights; the
/// round handle half-way along a segment bends it and a double-click straightens it (#916).
#[test]
fn a_points_menu_shapes_its_segment_and_the_handle_bends_it() {
    let mut harness = with_lanes();
    let rect = lane(&harness);
    click_at(&mut harness, Pos2::new(rect.left() + 100.0, rect.bottom() - 6.0));
    click_at(&mut harness, Pos2::new(rect.left() + 300.0, rect.top() + 6.0));
    assert_eq!(volume(&harness).0.len(), 2);
    // The handle half-way: up bends the rising line up.
    let handle = harness.get_by_label("Volume curve 1 on Drums").rect().center();
    drag(&mut harness, handle, vec2(0.0, -20.0));
    let curve = volume(&harness).0.first().map(|p| p.curve);
    assert!(curve.is_some_and(|c| !c.is_straight()), "{curve:?}");
    let handle = harness.get_by_label("Volume curve 1 on Drums").rect().center();
    harness.hover_at(handle);
    harness.run();
    for pressed in [true, false, true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos: handle,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    harness.run();
    let straight = volume(&harness).0.first().map_or(1.0, |p| p.curve.bend_of().unwrap_or(1.0));
    assert!(straight.abs() < 1e-9, "{straight}");
    // The second point's menu: the segment from the first takes the square.
    harness.get_by_label("Volume point 2 on Drums").click_secondary();
    harness.run();
    harness.get_by_label("Insert square").click();
    harness.run();
    let points = volume(&harness).0;
    assert!(points.len() > 4, "{points:?}");
    assert!(points.iter().all(|p| (12.0..=36.0).contains(&p.beat)), "{points:?}");
}

/// By default a click away from the line takes nothing, a click on the line adds a point on it
/// and a double-click adds one anywhere (#916).
#[test]
fn by_default_only_a_click_on_the_line_adds_a_point() {
    let mut harness = with_lanes();
    click_anywhere(&mut harness, false);
    let rect = lane(&harness);
    // Half-way up is away from the faint line at 0 dB (85 % up).
    click_at(&mut harness, Pos2::new(rect.left() + 100.0, rect.center().y));
    assert!(volume(&harness).0.is_empty(), "a click away from the line adds nothing");
    let line = rect.bottom() - 0.85 * (rect.height() - 2.0 * 4.0) - 4.0;
    click_at(&mut harness, Pos2::new(rect.left() + 100.0, line));
    assert_eq!(volume(&harness).0.len(), 1, "a click on the line adds a point");
}

/// A grid value in a lane's menu is the Arrangement's: the launch quantisation stays.
#[test]
fn a_lanes_grid_menu_leaves_the_launch_quantisation_alone() {
    let mut harness = with_lanes();
    let rect = lane(&harness);
    let before = harness.state().session().map(|s| s.quantization());
    right_click_at(&mut harness, Pos2::new(rect.left() + 700.0, rect.center().y));
    harness.get_by_label("1/8").click_accesskit();
    harness.run();
    assert_eq!(harness.state().arrangement().grid.label(), "1/8");
    assert_eq!(harness.state().session().map(|s| s.quantization()), before);
}

/// A right-click on the line offers Add value; a point's menu has Edit value, whose field sets the
/// value typed; Snap to grid off in the menu lets a point land off the grid (#916).
#[test]
fn add_value_edit_value_and_snap_to_grid_from_the_menus() {
    let mut harness = with_lanes();
    click_anywhere(&mut harness, false);
    let rect = lane(&harness);
    // The faint line at 0 dB is 85 % up the lane's plot.
    let line = rect.bottom() - 4.0 - 0.85 * (rect.height() - 8.0);
    right_click_at(&mut harness, Pos2::new(rect.left() + 100.0, line));
    harness.get_by_label("Add value").click();
    harness.run();
    let points = volume(&harness).0;
    assert_eq!(points.iter().map(|p| p.beat).collect::<Vec<_>>(), [12.0], "on the grid");
    // Edit value: type -6 and Enter.
    harness.get_by_label("Volume point 1 on Drums").click_secondary();
    harness.run();
    harness.get_by_label("Edit value").click();
    harness.run();
    harness.run();
    harness.input_mut().events.push(egui::Event::Text("-6".into()));
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    let gain = volume(&harness).0.first().map_or(0.0, |p| p.value);
    assert!((20.0 * gain.log10() + 6.0).abs() < 0.01, "{gain}");
    // Snap to grid off: a point lands where the pointer is.
    harness.get_by_label("Volume point 1 on Drums").click_secondary();
    harness.run();
    harness.get_by_label_contains("Snap to grid").click();
    harness.run();
    click_anywhere(&mut harness, true);
    click_at(&mut harness, Pos2::new(rect.left() + 303.0, rect.center().y));
    let beats: Vec<f64> = volume(&harness).0.iter().map(|p| p.beat).collect();
    assert!(beats.iter().any(|b| (b - 37.875).abs() < 0.01), "off the grid: {beats:?}");
}

/// A drag from the line makes a point there and moves it; with Shift it moves the segment's two
/// points up or down together; with Alt it bends the segment (#916).
#[test]
fn a_drag_from_the_line_makes_a_point_shift_moves_the_segment_alt_bends_it() {
    let mut harness = with_lanes();
    let rect = lane(&harness);
    click_at(&mut harness, Pos2::new(rect.left() + 100.0, rect.bottom() - 10.0));
    click_at(&mut harness, Pos2::new(rect.left() + 300.0, rect.top() + 20.0));
    let before = volume(&harness).0;
    let a = harness.get_by_label("Volume point 1 on Drums").rect().center();
    let b = harness.get_by_label("Volume point 2 on Drums").rect().center();
    // A quarter of the way along the line, away from the curve handle in the middle.
    let on = a.lerp(b, 0.25);
    // A plain drag: a point made at once where the press was, on the grid, then moved to where the
    // drag ends.
    drag(&mut harness, on, vec2(0.0, 12.0));
    let made = volume(&harness).0;
    assert_eq!(made.len(), 3, "{made:?}");
    assert_eq!((made[0].beat, made[2].beat), (before[0].beat, before[1].beat), "the others stay");
    // The press added it at once (one step), the drag moved it (another).
    for _ in 0..2 {
        harness.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        harness.run();
    }
    assert_eq!(volume(&harness).0.len(), 2);
    // With Shift: the segment's two points move down together.
    harness.event(egui::Event::ModifiersChanged(Modifiers::SHIFT));
    drag(&mut harness, on, vec2(0.0, 8.0));
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    let after = volume(&harness).0;
    assert_eq!(after.len(), 2);
    assert!(after[0].value < before[0].value && after[1].value < before[1].value, "{before:?} -> {after:?}");
    assert_eq!((after[0].beat, after[1].beat), (before[0].beat, before[1].beat), "up and down only");
    // With Alt the drag bends the segment.
    let a = harness.get_by_label("Volume point 1 on Drums").rect().center();
    let b = harness.get_by_label("Volume point 2 on Drums").rect().center();
    harness.event(egui::Event::ModifiersChanged(Modifiers::ALT));
    drag(&mut harness, a.lerp(b, 0.25), vec2(0.0, -20.0));
    harness.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    assert!(volume(&harness).0.first().is_some_and(|p| !p.curve.is_straight()));
}

/// From the lane's lower half, away from the line, a drag draws a box that selects the points in
/// it, painting no range (#916).
#[test]
fn a_drag_from_the_lower_half_selects_points_with_a_box() {
    let mut harness = with_lanes();
    let rect = lane(&harness);
    let low = rect.center().y + 4.0;
    for (x, y) in [(100.0, low), (200.0, low), (300.0, rect.top() + 8.0)] {
        click_at(&mut harness, Pos2::new(rect.left() + x, y));
    }
    assert_eq!(volume(&harness).0.len(), 3);
    // A box from near the bottom (above the lane's resize edge), well below the line, up over the
    // two low points only.
    let from = Pos2::new(rect.left() + 60.0, rect.bottom() - 6.0);
    drag(&mut harness, from, vec2(300.0, rect.center().y + 1.0 - from.y));
    harness.key_press(Key::Delete);
    harness.run();
    let left: Vec<f64> = volume(&harness).0.iter().map(|p| p.beat).collect();
    assert_eq!(left, [36.0], "the high point stays");
}

/// Selected points cut with Ctrl+X come back with Ctrl+V from the play position, on the lane
/// clicked last (#916).
#[test]
fn selected_points_cut_and_paste_from_the_play_position() {
    let mut harness = with_lanes();
    let rect = lane(&harness);
    let low = rect.center().y + 4.0;
    for x in [200.0, 300.0] {
        click_at(&mut harness, Pos2::new(rect.left() + x, low));
    }
    let beats = |h: &Harness<'static, App>| -> Vec<f64> { volume(h).0.iter().map(|p| p.beat).collect() };
    assert_eq!(beats(&harness), [24.0, 36.0]);
    let from = Pos2::new(rect.left() + 150.0, rect.bottom() - 6.0);
    drag(&mut harness, from, vec2(200.0, rect.center().y + 1.0 - from.y));
    harness.key_press_modifiers(Modifiers::COMMAND, Key::X);
    harness.run();
    assert!(beats(&harness).is_empty(), "cut");
    harness.key_press_modifiers(Modifiers::COMMAND, Key::V);
    harness.run();
    let play = harness.state().session().and_then(|s| s.player().map(|p| p.monitor().start_beat())).unwrap_or(0.0);
    let pasted = beats(&harness);
    assert_eq!(pasted.len(), 2, "{pasted:?}");
    assert!((pasted[1] - pasted[0] - 12.0).abs() < 1e-9 && pasted[0] >= play - 1e-9, "{pasted:?} from {play}");
}

/// The arrows move selected points: Left and Right a grid step, Up and Down a decibel (#916).
#[test]
fn the_arrow_keys_move_selected_points() {
    let mut harness = with_lanes();
    add_point(&mut harness, 100.0);
    let point = harness.get_by_label("Volume point 1 on Drums").rect().center();
    click_at(&mut harness, point);
    let before = volume(&harness).0[0];
    harness.key_press(Key::ArrowRight);
    harness.run();
    let moved = volume(&harness).0[0];
    assert!(moved.beat > before.beat, "{before:?} -> {moved:?}");
    harness.key_press(Key::ArrowUp);
    harness.run();
    let up = volume(&harness).0[0];
    let db = |g: f64| 20.0 * g.log10();
    assert!((db(up.value) - db(moved.value) - 1.0).abs() < 0.01, "{moved:?} -> {up:?}");
}

/// In a range's menu, Depth puts the shape put in last in again, swinging less
/// (#916).
#[test]
fn depth_puts_the_last_shape_in_again_with_less_swing() {
    let mut harness = with_lanes();
    let rect = lane(&harness);
    let y = rect.top() + rect.height() * 0.4;
    drag(&mut harness, Pos2::new(rect.left() + 256.0, y), vec2(256.0, 0.0));
    right_click_at(&mut harness, Pos2::new(rect.left() + 384.0, y));
    harness.get_by_label("Insert square").click();
    harness.run();
    // The swing in decibels, the quietest held at -70 dB.
    let swing = |h: &Harness<'static, App>| {
        let db: Vec<f64> = volume(h)
            .0
            .iter()
            .filter(|p| (32.0..=64.0).contains(&p.beat))
            .map(|p| if p.value > 0.0 { (20.0 * p.value.log10()).max(-70.0) } else { -70.0 })
            .collect();
        db.iter().copied().fold(f64::NEG_INFINITY, f64::max) - db.iter().copied().fold(f64::INFINITY, f64::min)
    };
    let full = swing(&harness);
    right_click_at(&mut harness, Pos2::new(rect.left() + 384.0, y));
    let slider =
        harness.query_all_by_label("Depth").map(|n| n.rect()).max_by(|a, b| a.width().total_cmp(&b.width())).unwrap();
    drag(&mut harness, slider.center(), vec2(-slider.width() * 0.3, 0.0));
    let less = swing(&harness);
    assert!(less < full * 0.9, "{full} -> {less}");
}

/// A selection box begun on a track, over no clip, picks the automation points it covers on the
/// lanes under the track (#916).
#[test]
fn a_box_from_the_track_picks_its_lanes_points() {
    // Drums with no clips, so the box goes over none.
    let mut project = demo_project();
    let drums = project.tracks.iter().find(|t| t.name == "Drums").map(|t| t.id);
    for lane in project.arrangement.lanes.iter_mut().filter(|l| l.track == drums) {
        lane.clips.clear();
    }
    let mut harness = arrangement(project);
    harness.get_by_label("Show automation of Drums").click();
    harness.run();
    for x in [600.0, 700.0] {
        add_point(&mut harness, x);
    }
    let rect = lane(&harness);
    // From the lower part of the track's row above the lane, down over both points.
    drag(&mut harness, Pos2::new(rect.left() + 560.0, rect.top() - 6.0), vec2(200.0, rect.height()));
    harness.key_press(Key::Delete);
    harness.run();
    assert!(volume(&harness).0.is_empty(), "both points were picked and deleted: {:?}", volume(&harness).0);
}

/// A click on a whole-number lane's slope adds a point on it, holding the value between two
/// numbers, so the envelope keeps its shape (the engine plays it with the fraction cut off).
#[test]
fn a_click_on_a_whole_number_slope_adds_a_point_between_numbers() {
    let mut harness = with_automation_of(&["op1.rate1"]);
    let rate = |harness: &Harness<'static, App>| {
        let session = harness.state().session().expect("a session");
        let project = session.project();
        let channel = project.tracks.iter().find(|t| t.name == "Drums").and_then(|t| t.channel);
        let channel = project.channels.iter().find(|c| Some(c.id) == channel).expect("the Drums channel");
        let device = channel.devices.iter().find(|d| d.name == "FM Synth").expect("the FM synth");
        device.automation.iter().find(|l| l.param == "op1.rate1").map(|l| l.points.clone()).unwrap_or_default()
    };
    let lane = harness.get_by_label("FM Synth: Op 1 Rate 1 automation on Drums").rect();
    let a = harness.get_by_label("FM Synth: Op 1 Rate 1 point 1 on Drums").rect().center();
    let b = harness.get_by_label("FM Synth: Op 1 Rate 1 point 2 on Drums").rect().center();
    // A quarter of the way along, clear of the curve handle in the middle; the point snaps to
    // the grid at beat 4.
    let x = a.x + (b.x - a.x) / 4.0;
    let y = envelope_y_at(&harness, lane, x).expect("the envelope is drawn");
    click_at(&mut harness, Pos2::new(x, y));
    let points = rate(&harness);
    assert_eq!(points.len(), 3, "{points:?}");
    assert_eq!((points[1].beat, points[1].value), (4.0, 0.5), "{points:?}");
}
