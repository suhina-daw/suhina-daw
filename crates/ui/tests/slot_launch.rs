// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A slot's launch button pressed and released with the mouse, and the menu of a clip's launch
//! settings (`docs/spec/ui-session-view.md` › Clip slots).

use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use suhina_engine::model::LaunchMode;
use suhina_ui::demo::demo_project;
use suhina_ui::interaction::grid::TrackGrid;
use suhina_ui::kit::theme;
use suhina_ui::kit::widgets::Action;
use suhina_ui::session::session_view::{self, SessionViewState};

struct View {
    grid: TrackGrid,
    state: SessionViewState,
    actions: Vec<Action>,
}

const SLOT: &str = "Launch Walk on Bass (scene 2)";

/// The demo set with the clip in Bass, scene 2 in `mode`.
fn view(mode: LaunchMode) -> Harness<'static, View> {
    let mut project = demo_project();
    let bass = project.tracks.iter().find(|t| t.name == "Bass").map(|t| t.id);
    if let Some(clip) = project
        .launcher
        .columns
        .iter_mut()
        .find(|c| Some(c.track) == bass)
        .and_then(|c| c.slots.get_mut(1))
        .and_then(|s| s.clip.as_mut())
    {
        clip.launch.mode = mode;
    }
    let view = View { grid: TrackGrid::build(&project, None), state: SessionViewState::default(), actions: Vec::new() };
    let mut harness = Harness::builder()
        .with_size((1280.0, 800.0))
        .build_ui_state(|ui, v: &mut View| session_view::show(ui, &v.grid, &[], &mut v.state, &mut v.actions), view);
    theme::install(&harness.ctx);
    harness.run();
    harness
}

fn button(harness: &mut Harness<'static, View>, at: Pos2, pressed: bool) -> Vec<Action> {
    harness.state_mut().actions.clear();
    harness.event(Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
    harness.run();
    harness.state().actions.clone()
}

fn press_and_release(mode: LaunchMode) -> (Vec<Action>, Vec<Action>) {
    let mut harness = view(mode);
    let at = harness.get_by_label(SLOT).rect().center();
    harness.hover_at(at);
    harness.run();
    let pressed = button(&mut harness, at, true);
    let released = button(&mut harness, at, false);
    (pressed, released)
}

#[test]
fn a_slot_launches_as_the_button_goes_down() {
    let launch = Action::new("clip.launch", json!({ "track": 2, "scene": 2 }));
    let (pressed, released) = press_and_release(LaunchMode::Trigger);
    assert_eq!(pressed, std::slice::from_ref(&launch));
    // A trigger clip does not listen for the release.
    assert_eq!(released, []);
}

#[test]
fn gate_and_repeat_clips_hear_the_release() {
    let launch = Action::new("clip.launch", json!({ "track": 2, "scene": 2 }));
    let release = Action::new("clip.release", json!({ "track": 2, "scene": 2 }));
    for mode in [LaunchMode::Gate, LaunchMode::Repeat] {
        let (pressed, released) = press_and_release(mode);
        assert_eq!(pressed, std::slice::from_ref(&launch), "{mode:?}");
        assert_eq!(released, std::slice::from_ref(&release), "{mode:?}");
    }
    let (_, released) = press_and_release(LaunchMode::Toggle);
    assert_eq!(released, []);
}

#[test]
fn a_click_without_the_mouse_launches_once() {
    let mut harness = view(LaunchMode::Gate);
    harness.get_by_label(SLOT).click_accesskit();
    harness.run();
    assert_eq!(harness.state().actions, [Action::new("clip.launch", json!({ "track": 2, "scene": 2 }))]);
}

#[test]
fn the_right_click_menu_sets_the_launch_settings() {
    let mut harness = view(LaunchMode::Trigger);
    let pick = |harness: &mut Harness<'static, View>, menu: &str, item: &str| {
        harness.state_mut().actions.clear();
        harness.get_by_label(SLOT).click_secondary();
        harness.run();
        harness.get_by_label_contains(menu).hover();
        harness.run();
        harness.get_by_label(item).click();
        harness.run();
        harness.state().actions.clone()
    };
    let slot = |extra: serde_json::Value| {
        let mut params = json!({ "track": 2, "scene": 2 });
        params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        params
    };
    assert_eq!(
        pick(&mut harness, "Launch mode: Trigger", "Gate"),
        [Action::new("clip.setLaunchMode", slot(json!({ "mode": "gate" })))]
    );
    assert_eq!(
        pick(&mut harness, "Launch quantisation: Global", "1/8T"),
        [Action::new("clip.setQuantization", slot(json!({ "value": "1/8T" })))]
    );
    harness.state_mut().actions.clear();
    harness.get_by_label(SLOT).click_secondary();
    harness.run();
    harness.get_by_label("Legato").click();
    harness.run();
    assert_eq!(harness.state().actions, [Action::new("clip.setLegato", slot(json!({ "on": true })))]);
}
