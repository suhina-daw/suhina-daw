// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! File › Open recent: the recent projects, newest first, each opened as File › Open… opens a
//! file; one that is gone says so in the status bar (`docs/design/components.md` › Menus).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::settings::{OnOpen, Settings};
use suhina_ui::{App, OpenAs, Status, kit::theme};

fn window(app: App) -> Harness<'static, App> {
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .allow_missing_glyphs()
        .with_size((1280.0, 800.0))
        .with_step_dt(0.05)
        .with_max_steps(20)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness
}

fn open_recent(harness: &mut Harness<'static, App>) {
    harness.get_by_label("File").click();
    harness.run_steps(4);
    harness.get_by_label_contains("Open recent").hover();
    harness.run_steps(4);
}

#[test]
fn open_recent_lists_the_recent_projects_and_opens_one_as_the_setting_says() {
    let dir = std::env::temp_dir().join(format!("suhina-open-recent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let night = dir.join("Night drive.suhina");
    std::fs::write(&night, "{}").unwrap();
    let gone = dir.join("Gone.suhina");

    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = window(app);
    let ctx = harness.ctx.clone();
    harness.state_mut().set_settings(&ctx, Settings { on_open: OnOpen::NewTab, ..Settings::default() });
    let _ = harness.state_mut().take_lists_wanted();

    // Opening the File menu asks for the list again, so a project saved since shows.
    harness.get_by_label("File").click();
    harness.run_steps(4);
    assert!(harness.state_mut().take_lists_wanted().is_some(), "the menu asks for the recent list");
    harness.state_mut().set_project_lists(vec![night.clone(), gone.clone()], Vec::new());
    harness.get_by_label("File").click();
    harness.run_steps(4);

    open_recent(&mut harness);
    let names: Vec<String> = ["Night drive", "Gone"]
        .iter()
        .filter_map(|n| harness.query_by_label(n).map(|node| node.accesskit_node().label().unwrap_or_default()))
        .collect();
    assert_eq!(names, ["Night drive", "Gone"], "newest first");
    harness.get_by_label("Night drive").click();
    harness.run_steps(4);
    assert_eq!(harness.state_mut().take_open_request(), Some((night, OpenAs::NewTab)));

    // A project that is no longer there does not open, and the status bar says why.
    open_recent(&mut harness);
    harness.get_by_label("Gone").click();
    harness.run_steps(4);
    assert_eq!(harness.state_mut().take_open_request(), None);
    assert!(
        matches!(harness.state().status(), Status::Error(e) if e.contains("Gone.suhina")),
        "{:?}",
        harness.state().status()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn open_recent_is_greyed_out_while_the_list_is_empty() {
    let mut app = App::new();
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = window(app);
    harness.get_by_label("File").click();
    harness.run_steps(4);
    assert!(harness.get_by_label_contains("Open recent").accesskit_node().is_disabled());
}
