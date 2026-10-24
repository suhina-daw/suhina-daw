// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Attached exports release the window while a render worker is unavailable.

use super::*;
use std::time::{Duration, Instant};
use suhina_engine::model::*;

fn project() -> Project {
    Project {
        tracks: vec![Track { channel: Some(ChannelId(1)), ..Track::new(TrackId(1), "Bass") }],
        channels: vec![Channel::new(ChannelId(1), "Bass")],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "Note".into(),
            length: 1.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![Note::new(0.0, 1.0, 69, 0.8)]) }],
            extensions: suhina_engine::model::Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "Bass".into(),
                track: Some(TrackId(1)),
                clips: vec![Clip::new(ContentId(1), 0.0, 1.0)],
            }],
            markers: Vec::new(),
        },
        ..Project::default()
    }
}

fn frames(endpoints: &crate::AppEndpoints, projects: &mut Projects, mut done: impl FnMut(usize) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if done(endpoints.run_jobs(projects)) {
            return;
        }
        assert!(Instant::now() < deadline, "the window job did not complete");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn direct_and_batch_exports_leave_the_window_available_while_the_worker_waits() {
    for batch in [false, true] {
        let folder = std::env::temp_dir().join(format!("suhina-export-worker-{}-{batch}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("mix.wav");
        let _ = std::fs::remove_file(&path);
        let endpoints = crate::AppEndpoints::start(&folder, Arc::new(|| {})).unwrap();
        let host = endpoints.local_host();
        let server = SuhinaMcp::attached(host.clone());
        let mut projects = Projects::new();
        projects.add(Session::new(project()), Some("Song".into())).unwrap();
        projects.set_mcp_write_folders(false, std::slice::from_ref(&folder));
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let (release, held) = std::sync::mpsc::channel();
        let (entered, ready) = std::sync::mpsc::channel();
        let blocker = runtime.spawn_blocking(move || {
            entered.send(()).unwrap();
            let _ = held.recv_timeout(Duration::from_secs(30));
        });
        ready.recv_timeout(Duration::from_secs(30)).unwrap();
        let output = path.display().to_string();
        let export = runtime.spawn(async move {
            if batch {
                server
                    .command_batch(Parameters(BatchParams {
                        steps: vec![
                            StepParams { id: "project.export".into(), params: Some(json!({"path": output})) },
                            StepParams { id: "project.list".into(), params: None },
                        ],
                        stop_on_error: Some(true),
                        project: None,
                    }))
                    .await
            } else {
                server
                    .command_run(Parameters(RunParams {
                        id: "project.export".into(),
                        params: Some(json!({"path": output})),
                        project: None,
                    }))
                    .await
            }
        });
        frames(&endpoints, &mut projects, |ran| ran > 0);
        // This independent job must complete even though the sole render worker is held.
        let other = runtime.spawn(async move { host.run(|projects| Ok(projects.active())).await });
        frames(&endpoints, &mut projects, |_| other.is_finished());
        assert!(runtime.block_on(other).unwrap().unwrap().is_some());
        assert!(!path.exists(), "rendering must wait for its worker, not run on the window");
        assert!(!export.is_finished());
        release.send(()).unwrap();
        frames(&endpoints, &mut projects, |_| export.is_finished());
        let result = runtime.block_on(export).unwrap().unwrap();
        assert_ne!(result.is_error, Some(true), "{result:?}");
        let samples: Vec<f32> = hound::WavReader::open(&path).unwrap().samples().map(Result::unwrap).collect();
        assert!(samples.iter().any(|s| s.abs() > 0.01), "the exported project must sound");
        runtime.block_on(blocker).unwrap();
    }
}

#[test]
fn an_agent_cannot_grant_itself_write_permissions() {
    let mut projects = Projects::new();
    projects.add(Session::new(project()), None).unwrap();
    projects.set_mcp_write_folders(false, &[]);
    for id in ["settings.set", "mcp.setWriteFolders", "projects.setMcpWriteFolders"] {
        assert!(projects.execute(id, &json!({"folders": ["/"]})).is_err());
    }
    assert_eq!(projects.mcp_write_folders(), (false, &[][..]));
}

#[test]
fn cancelling_a_pending_window_job_delivers_cleanup_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let folder = std::env::temp_dir().join(format!("suhina-export-cancel-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let endpoints = crate::AppEndpoints::start(&folder, Arc::new(|| {})).unwrap();
    let host = endpoints.local_host();
    let mut projects = Projects::new();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let cleanups = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&cleanups);
    let pending = runtime.spawn(async move {
        host.poll::<()>(move |_, cancelled| {
            if cancelled {
                count.fetch_add(1, Ordering::SeqCst);
                Some(Err("cancelled".into()))
            } else {
                None
            }
        })
        .await
    });
    frames(&endpoints, &mut projects, |ran| ran > 0);
    pending.abort();
    assert!(runtime.block_on(pending).unwrap_err().is_cancelled());
    frames(&endpoints, &mut projects, |_| cleanups.load(Ordering::SeqCst) > 0);
    assert_eq!(endpoints.run_jobs(&mut projects), 0);
    assert_eq!(cleanups.load(Ordering::SeqCst), 1);
}

#[test]
fn a_revoked_folder_stops_an_export_before_preparation() {
    let folder = std::env::temp_dir().join(format!("suhina-export-revoke-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let mut projects = Projects::new();
    projects.add(Session::new(project()), None).unwrap();
    projects.set_mcp_write_folders(false, std::slice::from_ref(&folder));
    let mut request = ExportRequest {
        target: None,
        params: json!({"path": folder.join("mix.wav").display().to_string()}),
        pending: None,
    };
    projects.set_mcp_write_folders(false, &[]);
    let error = request.poll(&mut projects, false).unwrap().err().unwrap();
    assert!(error.contains("no MCP write folder"), "{error}");
    assert!(request.pending.is_none());
    assert!(!folder.join("mix.wav").exists());
}
