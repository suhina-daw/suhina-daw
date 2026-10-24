// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Several open projects: which one is active, where commands go, and that only the active one
//! keeps its player.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use serde_json::json;
use suhina_engine::{AudioPool, Renderer};
use suhina_model::Project;
use suhina_session::{CommandKind, MAX_OPEN_PROJECTS, Player, ProjectId, Projects, Session, commands};

use crate::support::run;

fn untitled(projects: &mut Projects) -> ProjectId {
    projects.add(Session::new(Project::default()), None).unwrap()
}

fn tempo(projects: &mut Projects, project: ProjectId) -> f64 {
    projects.get(project).unwrap().project().transport.tempo_bpm
}

#[test]
fn an_added_project_becomes_active_with_a_fresh_id() {
    let mut projects = Projects::new();
    assert!(projects.is_empty());
    let a = untitled(&mut projects);
    let b = untitled(&mut projects);
    assert_ne!(a, b);
    assert_eq!(projects.active(), Some(b));
    assert_eq!(projects.ids(), vec![a, b]);
    let listed = run(&mut projects, "project.list", json!({})).unwrap();
    assert_eq!(listed["active"], json!(b.0));
    assert_eq!(listed["projects"][0]["name"], json!(format!("Untitled {}", a.0)));
    assert_eq!(listed["projects"][1]["active"], json!(true));
}

#[test]
fn commands_go_to_the_active_project_or_the_one_named() {
    let mut projects = Projects::new();
    let a = untitled(&mut projects);
    let b = untitled(&mut projects);
    run(&mut projects, "transport.setTempo", json!({ "bpm": 90 })).unwrap();
    projects.execute_in(Some(a), "transport.setTempo", &json!({ "bpm": 140 })).unwrap();
    assert_eq!(tempo(&mut projects, a), 140.0);
    assert_eq!(tempo(&mut projects, b), 90.0);
    assert_eq!(projects.active(), Some(b), "naming a project does not switch to it");

    // Each project keeps its own undo history.
    run(&mut projects, "project.switch", json!({ "project": a.0 })).unwrap();
    run(&mut projects, "edit.undo", json!({})).unwrap();
    assert_eq!(tempo(&mut projects, a), Project::default().transport.tempo_bpm);
    assert_eq!(tempo(&mut projects, b), 90.0);
}

#[test]
fn next_and_previous_wrap_around() {
    let mut projects = Projects::new();
    let a = untitled(&mut projects);
    let b = untitled(&mut projects);
    let c = untitled(&mut projects);
    assert_eq!(run(&mut projects, "project.next", json!({})).unwrap()["active"], json!(a.0));
    assert_eq!(run(&mut projects, "project.next", json!({})).unwrap()["active"], json!(b.0));
    assert_eq!(run(&mut projects, "project.previous", json!({})).unwrap()["active"], json!(a.0));
    assert_eq!(run(&mut projects, "project.previous", json!({})).unwrap()["active"], json!(c.0));
    assert_eq!(projects.cycle(-4).unwrap(), b);
}

#[test]
fn closing_the_active_project_activates_a_neighbour() {
    let mut projects = Projects::new();
    let a = untitled(&mut projects);
    let b = untitled(&mut projects);
    let c = untitled(&mut projects);
    projects.switch(b).unwrap();
    run(&mut projects, "project.close", json!({ "project": b.0 })).unwrap();
    assert_eq!(projects.active(), Some(c), "the tab to the right");
    projects.close(c).unwrap();
    assert_eq!(projects.active(), Some(a), "the last tab falls back to the left");
    projects.close(a).unwrap();
    assert_eq!(projects.active(), None);
    assert!(run(&mut projects, "project.info", json!({})).unwrap_err().contains("no project is open"));
    assert!(run(&mut projects, "project.next", json!({})).is_err());
}

#[test]
fn quitting_lets_every_project_go_and_leaves_none_open() {
    let mut projects = Projects::new();
    untitled(&mut projects);
    untitled(&mut projects);
    assert!(projects.close_all(std::time::Duration::from_secs(5)));
    assert!(projects.is_empty());
    assert_eq!(projects.active(), None);
}

#[test]
fn ids_are_not_reused_after_a_close() {
    let mut projects = Projects::new();
    let a = untitled(&mut projects);
    projects.close(a).unwrap();
    let b = untitled(&mut projects);
    assert_ne!(a, b);
    assert!(projects.execute_in(Some(a), "project.info", &json!({})).is_err());
    assert!(run(&mut projects, "project.switch", json!({ "project": a.0 })).unwrap_err().contains("no open project"));
}

#[test]
fn opening_too_many_projects_is_refused() {
    let mut projects = Projects::new();
    for _ in 0..MAX_OPEN_PROJECTS {
        untitled(&mut projects);
    }
    assert!(projects.add(Session::new(Project::default()), None).is_err());
    assert_eq!(projects.len(), MAX_OPEN_PROJECTS);
}

#[test]
fn only_the_active_project_keeps_its_player() {
    let mut projects = Projects::new();
    let a = untitled(&mut projects);
    let project = Project::default();
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), 48_000).unwrap();
    projects.get_mut(a).unwrap().attach_player(Player::new(renderer.controller(), None));
    assert!(run(&mut projects, "launcher.state", json!({})).is_ok());

    let b = untitled(&mut projects);
    assert!(projects.get(a).unwrap().player().is_none(), "switching away stops the old project");
    assert_eq!(projects.active(), Some(b));
    let listed = run(&mut projects, "project.list", json!({})).unwrap();
    assert_eq!(listed["projects"][0]["playing"], json!(false));
}

#[test]
fn commands_list_shows_project_and_session_commands_once() {
    let mut projects = Projects::new();
    let listed = run(&mut projects, "command.list", json!({})).unwrap();
    let ids: Vec<&str> = listed.as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
    for id in ["project.list", "project.switch", "project.next", "project.previous", "project.close"] {
        assert!(ids.contains(&id), "{id} is listed");
    }
    assert!(commands().iter().all(|c| ids.contains(&c.id)));
    assert!(commands().iter().all(|c| c.kind != CommandKind::Projects), "session commands never manage projects");
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), ids.len());
}

#[test]
fn project_commands_check_their_parameters() {
    let mut projects = Projects::new();
    let a = untitled(&mut projects);
    assert!(run(&mut projects, "project.list", json!({ "x": 1 })).unwrap_err().contains("unknown parameter"));
    assert!(run(&mut projects, "project.switch", json!({})).is_err());
    assert!(run(&mut projects, "project.switch", json!({ "project": "one" })).is_err());
    assert!(run(&mut projects, "project.switch", json!([1])).is_err());
    assert!(projects.execute_in(Some(a), "project.list", &json!({})).is_err(), "no target for project commands");
}

#[test]
fn the_commands_document_lists_every_command_once_with_its_parameters() {
    let doc = suhina_session::commands_markdown();
    let ids: Vec<String> = suhina_session::all_commands()
        .as_array()
        .map(|all| all.iter().filter_map(|c| c["id"].as_str().map(String::from)).collect())
        .unwrap_or_default();
    assert!(ids.len() > 20, "only {} commands", ids.len());
    for id in &ids {
        let rows = doc.lines().filter(|l| l.starts_with(&format!("| `{id}` |"))).count();
        assert_eq!(rows, 1, "{id} has {rows} rows");
    }
    assert_eq!(doc.lines().filter(|l| l.starts_with("| `")).count(), ids.len());
    assert!(doc.contains("| `track.setPan` | edit |"));
    assert!(doc.contains("`track` (track):"));
    assert!(doc.contains(", optional):"));
    // Every row has the four columns: a `|` in a description would add one.
    for row in doc.lines().filter(|l| l.starts_with("| `")) {
        assert_eq!(row.replace(r"\|", "").matches('|').count(), 5, "{row}");
    }
}
