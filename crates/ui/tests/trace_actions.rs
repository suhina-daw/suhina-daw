// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The trace log (`docs/debugging.md`) has an `action` line for every command the window runs,
//! the `project.*` (on the open projects) ones too, and a `project opened` line for each project opened (#821). The
//! trace file is one per process, so this test has a test binary of its own.

use serde_json::json;
use suhina_session::Session;
use suhina_ui::demo::demo_project;
use suhina_ui::kit::widgets::Action;
use suhina_ui::{App, OpenAs};

#[test]
fn projects_commands_are_written_to_the_trace() {
    let dir = std::env::temp_dir().join(format!("suhina-trace-actions-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("trace.log");
    suhina_plugins::trace::start(&log);

    let mut app = App::new();
    let first = app.open_project(Session::new(demo_project()), &[], OpenAs::NewTab).unwrap();
    let second = app.open_project(Session::new(demo_project()), &[], OpenAs::NewTab).unwrap();
    app.run(&Action::new("project.switch", json!({ "project": first.0 })));
    app.run(&Action::new("project.close", json!({ "project": second.0 })));
    app.run(&Action::new("transport.stop", json!({})));
    let third = app.open_project(Session::new(demo_project()), &[], OpenAs::Replace).unwrap();

    let text = std::fs::read_to_string(&log).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let actions: Vec<&str> = text
        .lines()
        .filter_map(|l| l.split_once("] action ").map(|(_, a)| a.split(' ').next().unwrap_or("")))
        .collect();
    assert_eq!(actions, ["project.switch", "project.close", "transport.stop"], "trace:\n{text}");
    assert!(app.projects().get(second).is_none(), "the second project is closed");
    let opened: Vec<&str> = text.lines().filter_map(|l| l.split_once("] project opened ").map(|(_, o)| o)).collect();
    assert_eq!(
        opened,
        [
            format!("{} \"the project\" in a new tab", first.0),
            format!("{} \"the project\" in a new tab", second.0),
            format!("{} \"the project\" in place of {}", third.0, first.0),
        ],
        "opening is traced too:\n{text}"
    );
    assert!(
        text.contains(&format!("action project.close {{\"project\":{}}}", second.0)),
        "with its parameters:\n{text}"
    );
}
