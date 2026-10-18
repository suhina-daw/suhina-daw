// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Right-click menus in the Session view: a track's strip and a return's column have the track's
//! menu as its header does, every track menu inserts tracks, and the empty space after the
//! tracks offers the same (`docs/spec/ui-session-view.md`).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::{App, kit::theme};

fn window() -> Harness<'static, App> {
    let mut app = App::new();
    app.set_browser_open(false);
    app.set_session(Session::new(demo_project()), &[]);
    let mut harness = Harness::builder()
        .with_size((1280.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(60)
        .build_ui_state(|ui, app: &mut App| app.ui(ui), app);
    theme::install(&harness.ctx);
    harness.run();
    harness
}

fn track_names(harness: &Harness<'static, App>) -> Vec<String> {
    harness.state().session().unwrap().project().tracks.iter().map(|t| t.name.clone()).collect()
}

#[test]
fn the_strip_the_return_column_and_the_empty_space_have_menus_that_insert_tracks() {
    let mut harness = window();
    let before = track_names(&harness).len();

    // The strip's background has the track's menu; it inserts a return track.
    harness.get_by_label("Drums channel strip").click_secondary();
    harness.run();
    harness.get_by_label("Insert return track").click();
    harness.run();
    let names = track_names(&harness);
    assert_eq!(names.len(), before + 1);
    let added = names.last().unwrap().clone();
    assert!(added.contains("Return"), "{added}");

    // The return's column (no slots) has its menu too, and it inserts a MIDI track.
    harness.get_by_label(&format!("{added} column")).click_secondary();
    harness.run();
    assert!(harness.query_by_label("Delete track").is_some(), "the return's track menu");
    harness.get_by_label_contains("Insert MIDI track").click();
    harness.run();
    assert_eq!(track_names(&harness).len(), before + 2);

    // The empty space after the tracks offers the same.
    harness.get_by_label("Empty space").click_secondary();
    harness.run();
    harness.get_by_label_contains("Insert audio track").click();
    harness.run();
    assert_eq!(track_names(&harness).len(), before + 3);
}
