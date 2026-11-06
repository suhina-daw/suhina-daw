// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The app's endpoints end to end (ADR 0028): one instance per build, and an agent that waits
//! for the person's answer and then works on the window's projects through its job queue.

#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use suhina_automation::endpoint::{self, Endpoint};
use suhina_automation::{AppEndpoints, AppEvent, StartError};
use suhina_session::Projects;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// A folder of its own per test, so each test is a build of its own.
fn build_folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-attach-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn start(folder: &std::path::Path) -> AppEndpoints {
    match AppEndpoints::start(folder, Arc::new(|| {})) {
        Ok(endpoints) => endpoints,
        Err(e) => panic!("{e:?}"),
    }
}

/// Runs `step` as the window would, every few milliseconds, until it returns `Some`.
fn frames<T>(mut step: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(value) = step() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_second_start_of_the_build_hands_its_project_to_the_first() {
    let folder = build_folder("instance");
    let first = start(&folder);
    let Err(StartError::Running(running)) = AppEndpoints::start(&folder, Arc::new(|| {})) else {
        panic!("a second start of the same build must find the first");
    };
    let project = folder.join("song.suhina");
    std::fs::write(&project, b"{}").unwrap();
    running.hand_over(std::slice::from_ref(&project)).unwrap();
    let files = frames(|| first.take_opens());
    assert_eq!(files.len(), 1);
    assert!(files[0].ends_with("song.suhina"), "{files:?}");
    // A start with no project only brings the window forward.
    running.hand_over(&[]).unwrap();
    assert_eq!(frames(|| first.take_opens()), Vec::<PathBuf>::new());
    // Another build's folder is another build.
    let other = start(&build_folder("instance-other"));
    drop(other);
    let _ = std::fs::remove_dir_all(&folder);
}

/// A client speaking MCP line by line over the build's MCP endpoint, on the endpoints' runtime.
/// `calls` are sent after the handshake; each answer (or `None` when the connection closed)
/// comes back on the returned channel.
fn agent(endpoints: &AppEndpoints, folder: &std::path::Path, calls: Vec<Value>) -> mpsc::Receiver<Option<Value>> {
    let key = endpoint::build_key(folder);
    let (send, answers) = mpsc::channel();
    endpoints.runtime().spawn(async move {
        let stream = loop {
            match endpoint::connect(&key, Endpoint::Mcp).await {
                Ok(stream) => break stream,
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        };
        let (read, mut write) = tokio::io::split(stream);
        let mut lines = BufReader::new(read).lines();
        let init = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "Test agent", "version": "1"}}});
        send_line(&mut write, init).await;
        let _ = lines.next_line().await;
        send_line(&mut write, json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await;
        for (n, call) in calls.into_iter().enumerate() {
            let id = n + 1;
            let request = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": call});
            if !send_line(&mut write, request).await {
                let _ = send.send(None);
                return;
            }
            let answer = loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        let value: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
                        if value["id"] == json!(id) {
                            break Some(value);
                        }
                    }
                    _ => break None,
                }
            };
            let closed = answer.is_none();
            let _ = send.send(answer);
            if closed {
                return;
            }
        }
    });
    answers
}

/// Sends one JSON-RPC message as a line; false when the connection is gone.
async fn send_line(write: &mut (impl tokio::io::AsyncWrite + Unpin), message: Value) -> bool {
    let mut text = message.to_string();
    text.push('\n');
    write.write_all(text.as_bytes()).await.is_ok() && write.flush().await.is_ok()
}

fn text(answer: &Value) -> String {
    answer["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

fn is_error(answer: &Value) -> bool {
    answer["result"]["isError"] == json!(true)
}

#[test]
fn an_allowed_agent_works_on_the_windows_projects_and_never_closes_a_tab() {
    let folder = build_folder("allow");
    let endpoints = start(&folder);
    endpoints.set_enabled(true);
    let mut projects = Projects::new();
    let calls = vec![
        json!({"name": "command_run", "arguments": {"id": "project.list"}}),
        json!({"name": "command_run", "arguments": {"id": "project.close", "params": {"project": 1}}}),
        json!({"name": "project_open", "arguments": {"path": "song.suhina"}}),
    ];
    let answers = agent(&endpoints, &folder, calls);
    // The agent connects and the window is asked about it; nothing runs before the answer.
    let (connection, client) = frames(|| {
        endpoints.take_events().into_iter().find_map(|e| match e {
            AppEvent::Ask { connection, client } => Some((connection, client)),
            AppEvent::Gone { .. } => None,
        })
    });
    assert_eq!(client, "Test agent");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(endpoints.run_jobs(&mut projects), 0, "a call waits for the person's answer");
    assert!(answers.try_recv().is_err());
    assert!(endpoints.connected().is_empty());
    endpoints.answer(connection, true);
    assert_eq!(endpoints.connected(), vec![(connection, "Test agent".to_string())]);
    // The window runs the agent's work between frames.
    let listed = frames(|| {
        endpoints.run_jobs(&mut projects);
        answers.try_recv().ok()
    })
    .expect("an answer");
    assert!(!is_error(&listed), "{listed}");
    let close = frames(|| answers.try_recv().ok()).expect("an answer");
    assert!(is_error(&close) && text(&close).contains("never close a tab"), "{close}");
    let open = frames(|| answers.try_recv().ok()).expect("an answer");
    assert!(is_error(&open) && text(&open).contains("the person opens projects"), "{open}");
    // The person disconnects it.
    endpoints.disconnect(connection);
    frames(|| endpoints.take_events().into_iter().any(|e| e == AppEvent::Gone { connection }).then_some(()));
    assert!(endpoints.connected().is_empty());
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn a_denied_agent_is_disconnected_and_nothing_listens_while_agents_are_off() {
    let folder = build_folder("deny");
    let endpoints = start(&folder);
    let key = endpoint::build_key(&folder);
    // Off by default: nothing listens.
    let absent = endpoints.runtime().block_on(endpoint::connect(&key, Endpoint::Mcp));
    assert!(absent.is_err_and(|e| endpoint::is_absent(&e)));
    endpoints.set_enabled(true);
    let mut projects = Projects::new();
    let answers = agent(&endpoints, &folder, vec![json!({"name": "command_run", "arguments": {"id": "project.list"}})]);
    let connection = frames(|| {
        endpoints.take_events().into_iter().find_map(|e| match e {
            AppEvent::Ask { connection, .. } => Some(connection),
            AppEvent::Gone { .. } => None,
        })
    });
    endpoints.answer(connection, false);
    let answer = frames(|| {
        endpoints.run_jobs(&mut projects);
        answers.try_recv().ok()
    });
    if let Some(answer) = answer {
        assert!(is_error(&answer) && text(&answer).contains("did not allow"), "{answer}");
    }
    assert!(endpoints.connected().is_empty());
    let _ = std::fs::remove_dir_all(&folder);
}
