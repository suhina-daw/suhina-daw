// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Agents in the window (ADR 0028): the person allows or denies each one that connects, and
//! disconnects one from the control bar.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::{App, kit::theme};

fn window(app: App) -> Harness<'static, App> {
    let mut harness = Harness::builder()
        .with_size((1280.0, 800.0))
        .with_step_dt(0.25)
        .with_max_steps(4)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness
}

fn demo_app() -> App {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(demo_project()), &[]);
    app
}

#[test]
fn the_person_allows_or_denies_each_agent_that_connects() {
    let mut app = demo_app();
    app.ask_agent(7, "Claude Code".into());
    app.ask_agent(8, "Other agent".into());
    let mut harness = window(app);
    // One question at a time, the first first.
    assert!(harness.query_by_label("Allow Other agent").is_none());
    harness.get_by_label("Allow Claude Code").click();
    harness.run();
    assert_eq!(harness.state_mut().take_agent_answers(), vec![(7, true)]);
    harness.get_by_label("Deny Other agent").click();
    harness.run();
    assert_eq!(harness.state_mut().take_agent_answers(), vec![(8, false)]);
    assert!(harness.query_by_label("Deny Other agent").is_none(), "no question is left");
}

#[test]
fn a_question_goes_away_when_its_agent_leaves() {
    let mut app = demo_app();
    app.ask_agent(3, "Claude Code".into());
    app.agent_gone(3);
    let harness = window(app);
    assert!(harness.query_by_label("Allow Claude Code").is_none());
}

#[test]
fn an_agent_let_in_shows_in_the_control_bar_and_a_click_disconnects_it() {
    let mut app = demo_app();
    app.set_agents(vec![(5, "Claude Code".into())]);
    let mut harness = window(app);
    harness.get_by_label("Disconnect Claude Code").click();
    harness.run();
    assert_eq!(harness.state_mut().take_agent_disconnects(), vec![5]);
}
