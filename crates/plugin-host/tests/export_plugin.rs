// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An export through `export_from` with a real plug-in host process: the plug-in's sound is
//! in the file (not passed through), the export finishes as fast as a render without one, and
//! the hosts stay alive through the render (the controls are not dropped early — dropping them
//! there ended every host process and made each block miss its answer, a 96 s export taking
//! an hour of misses; issue #601's export). The hosts end when the job does. And an export
//! from a session plays the running plug-ins' state as it is now (ADR 0006 decision 5), not
//! the state the project was opened with, without changing anything in the project.

#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::json;
use suhina_engine::{AudioPool, DeviceAssets, Renderer, render_offline};
use suhina_media::Trusted;
use suhina_model::*;
use suhina_plugins::scan::{Catalog, PluginInfo};
use suhina_session::{ExportData, Player, PluginEnv, Session, export_from};

/// The gain the VST3 test plug-in starts with, and the one its editor turns it to when it opens.
const DEFAULT_GAIN: f32 = 0.5;
const EDITOR_GAIN: f32 = 0.125;
const SR: u32 = 48_000;
const PATIENT: Duration = Duration::from_secs(20);

/// A catalog with the host's VST3 test plug-in, as an effect with the id `vst3`: a gain.
fn env() -> Arc<PluginEnv> {
    let path = PathBuf::from("test:vst3");
    let info = PluginInfo {
        path: path.clone(),
        id: "vst3".into(),
        name: "Test vst3".into(),
        vendor: "Suhina".into(),
        version: "1".into(),
        features: vec!["audio-effect".into()],
        replaces: Vec::new(),
        ara: false,
    };
    let mut catalog = Catalog::new();
    catalog.insert(path, vec![info]);
    Arc::new(PluginEnv::new(catalog, PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host"))))
}

/// One note track with the test plug-in after its notes, playing two seconds of them.
fn project() -> Project {
    let note = |time: f64, duration: f64, key: u8, velocity: f32| Note {
        time,
        duration,
        key,
        velocity,
        release_velocity: 0.5,
        channel: 0,
        muted: false,
    };
    Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "T")
        }],
        channels: vec![Channel {
            id: ChannelId(1),
            name: "C".into(),
            role: ChannelRole::Regular,
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            cue: false,
            output: None,
            sends: Vec::new(),
            devices: vec![Device {
                name: "Test vst3".into(),
                kind: DeviceKind::Clap { id: "vst3".into() },
                enabled: true,
                state: None,
                params: Vec::new(),
                extensions: Extensions::default(),
                automation: Vec::new(),
            }],
            extensions: Extensions::default(),
            automation: Vec::new(),
        }],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "c".into(),
            length: 4.0,
            lanes: vec![ContentLane {
                target: None,
                data: LaneData::Notes((0..8).map(|i| note(f64::from(i) * 0.5, 0.4, 43, 0.9)).collect()),
            }],
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "l".into(),
                track: Some(TrackId(1)),
                clips: vec![Clip::new(ContentId(1), 0.0, 4.0)],
            }],
            markers: Vec::new(),
        },
        ..Project::default()
    }
}

/// The export's peak, over the dry (passed-through) export's peak: the plug-in's gain, and
/// the blocks the render's plug-ins did not answer in time.
fn export_ratio(mut project: Project, plugins: Option<Arc<PluginEnv>>) -> (f32, u64) {
    assert_eq!(project.assign_ids(), Ok(()));
    let data = ExportData {
        warnings: Vec::new(),
        project,
        audio: AudioPool::new(),
        assets: DeviceAssets::new(),
        plugins,
        folder: None,
        trusted: Trusted::default(),
        title: String::new(),
    };
    let path = std::env::temp_dir().join(format!("suhina-export-plugin-{}.wav", std::process::id()));
    let params = json!({ "path": path.display().to_string() });
    let summary = export_from(&data, &params).unwrap();
    let peak = summary["peak"].as_f64().unwrap_or(0.0) as f32;
    let misses = summary["misses"].as_u64().unwrap_or_default();
    let _ = std::fs::remove_file(&path);
    (peak, misses)
}

#[test]
fn an_export_plays_the_plugin_and_finishes_like_a_render_without_one() {
    let (dry, dry_misses) = export_ratio(project(), None);
    assert!(dry > 0.1, "the notes sound without the plug-in: {dry}");
    let (wet, wet_misses) = export_ratio(project(), Some(env()));
    // The test plug-in is a gain of a half: its sound is in the file.
    let expected = 0.5 * dry;
    assert!((wet - expected).abs() < 1e-4, "{wet} against {expected}");
    // And no block missed its answer — a miss waits `OFFLINE_TIMEOUT` per block, and the
    // export would take an hour of them. The count, not a duration: a busy runner cannot
    // fail the test.
    assert_eq!(dry_misses, 0);
    assert_eq!(wet_misses, 0, "every block the hosted render sent was answered");
}

/// The peak an export from `session` renders, the file gone once its peak is read.
fn export_peak(session: &mut Session) -> f32 {
    static TRY: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "suhina-export-heard-{}-{}.wav",
        std::process::id(),
        TRY.fetch_add(1, Ordering::Relaxed)
    ));
    let params = json!({ "path": path.display().to_string() });
    let summary = export_from(&session.export_data(), &params).unwrap();
    let _ = std::fs::remove_file(&path);
    summary["peak"].as_f64().unwrap_or(0.0) as f32
}

/// The gain a project holds for its plug-in: its state is the gain as 4 bytes.
fn gain(project: &Project) -> Option<f32> {
    let state = project.channels[0].devices[0].state.as_deref()?;
    Some(f32::from_le_bytes(state.try_into().ok()?))
}

/// The gather a session offers an export over frames: pending while the state reads are out,
/// or ready with the data. The engine's blocks are pumped beside it, as the audio thread does
/// while a window gathers, and `deadline` is the one the caller holds across its frames — the
/// bounded wait's settle runs against it.
fn digs(session: &mut Session, renderer: &mut Renderer, deadline: Instant) -> suhina_session::ExportGather {
    let mut block = vec![0.0f32; 256 * 2];
    renderer.process(&mut block);
    session.gather_export_data(deadline)
}

#[test]
fn an_export_gathers_its_states_over_frames_without_the_window_waiting() {
    // Its lanes and clips get ids, as a session gives them, so it compares with the session's.
    let mut opened = project();
    assert_eq!(opened.assign_ids(), Ok(()));
    let mut renderer = Renderer::stopped(&opened, &AudioPool::new(), SR).unwrap();
    let mut session = Session::new(opened.clone());
    session.attach_player(Player::new(renderer.controller(), None));
    session.set_plugin_env(env());
    let device = json!({ "track": 1, "device": 0 });
    let deadline = Instant::now() + Duration::from_secs(30);

    // Before anything runs, a gather settles at once: there is nothing to read.
    assert!(
        matches!(digs(&mut session, &mut renderer, deadline), suhina_session::ExportGather::Ready(_)),
        "nothing runs: the states are as they stand"
    );

    // A frame's work starts the track's plug-in: its control half is out while it loads, and
    // a gather pumped then stays pending without waiting — the caller's frames keep running.
    session.sync_plugins();
    assert!(
        matches!(digs(&mut session, &mut renderer, deadline), suhina_session::ExportGather::Pending),
        "loading keeps the device's control out; the gather does not wait for it"
    );

    // Once it runs, the gather asks for the state (one frame), and the reply is pending
    // without waiting the next one.
    until(&mut session, &mut renderer, "starting", |s| {
        s.execute("device.plugin", &device).unwrap()["status"] == "running"
    });
    assert!(
        matches!(digs(&mut session, &mut renderer, deadline), suhina_session::ExportGather::Pending),
        "the state read is out; the gather does not wait for its reply"
    );
    let data = loop {
        match digs(&mut session, &mut renderer, deadline) {
            suhina_session::ExportGather::Pending => std::thread::sleep(std::time::Duration::from_millis(10)),
            suhina_session::ExportGather::Ready(data) => break data,
        }
    };
    assert_eq!(gain(&data.project), Some(DEFAULT_GAIN), "the state read is in the exported data");
    assert!(data.plugins.is_some());

    // The read changed nothing in the project and recorded no undo step.
    assert!(!session.can_undo());
    assert_eq!(session.project(), &opened);
}

#[test]
fn a_gather_keeps_the_reads_across_a_project_switch() {
    // Its lanes and clips get ids, as a session gives them, so it compares with the session's.
    let mut opened = project();
    assert_eq!(opened.assign_ids(), Ok(()));
    // The project's saved state is the default gain; the plug-in loads with it.
    let gain_bytes = DEFAULT_GAIN.to_le_bytes().to_vec();
    opened.channels[0].devices[0].state = Some(gain_bytes);
    let mut projects = suhina_session::Projects::new();
    let id =
        projects.add(Session::new(opened.clone()), Some("gain".into())).map_err(|e| e.message().to_string()).unwrap();
    let mut renderer = Renderer::stopped(&opened, &AudioPool::new(), SR).unwrap();
    let device = json!({ "track": 1, "device": 0 });
    projects.get_mut(id).unwrap().attach_player(Player::new(renderer.controller(), None));
    projects.get_mut(id).unwrap().set_plugin_env(env());
    projects.get_mut(id).unwrap().sync_plugins();
    until_with_projects(&mut projects, id, &mut renderer, "starting", |s| {
        s.execute("device.plugin", &device).unwrap()["status"] == "running"
    });

    // The user changes the plug-in's gain in its editor without touching the project: the
    // running state (0.125) differs from the saved one (0.5). Editors open on Windows, and on
    // Linux with an X server.
    let editors =
        cfg!(windows) || (cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()));
    if !editors {
        eprintln!("no display for editors; the switch part is skipped");
        return;
    }
    projects.get_mut(id).unwrap().execute("device.openEditor", &device).unwrap();
    until_with_projects(&mut projects, id, &mut renderer, "the edit reaching the plug-in", |s| {
        gain(&s.project_as_heard()) == Some(EDITOR_GAIN)
    });
    projects.get_mut(id).unwrap().execute("device.closeEditor", &device).unwrap();

    // The export's gather goes out, and the project is switched from while the read is out:
    // the switch detaches the player, which stops the plug-ins and takes the instances, the
    // read's worker still holding its control. The gather keeps the read's answer — what is
    // heard — and not the saved state the fallback would hold.
    let deadline = Instant::now() + Duration::from_secs(30);
    assert!(
        matches!(digs(projects.get_mut(id).unwrap(), &mut renderer, deadline), suhina_session::ExportGather::Pending),
        "the read is out"
    );
    let other = projects_add_other(&mut projects);
    projects.switch(other).unwrap();
    let data = loop {
        match digs(projects.get_mut(id).unwrap(), &mut renderer, deadline) {
            suhina_session::ExportGather::Pending => std::thread::sleep(std::time::Duration::from_millis(10)),
            suhina_session::ExportGather::Ready(data) => break data,
        }
    };
    assert_eq!(gain(&data.project), Some(EDITOR_GAIN), "the read's answer, not the saved state");
    assert!(data.warnings.is_empty(), "a successful detached read must not time out: {:?}", data.warnings);
    assert!(!projects.get_mut(id).unwrap().can_undo());
}

/// A second project to switch to: an empty one, with the same display name as the first, so
/// the routing is caught if it ever goes by name instead of by id.
fn projects_add_other(projects: &mut suhina_session::Projects) -> suhina_session::ProjectId {
    projects
        .add(Session::new(suhina_session::blank_project()), Some("gain".into()))
        .map_err(|e| e.message().to_string())
        .unwrap()
}

/// Plays the active project's frames while its session looks after its plug-ins, until `done`
/// holds.
fn until_with_projects(
    projects: &mut suhina_session::Projects,
    id: suhina_session::ProjectId,
    renderer: &mut Renderer,
    what: &str,
    mut done: impl FnMut(&mut Session) -> bool,
) {
    let deadline = Instant::now() + PATIENT;
    let mut block = vec![0.0f32; 256 * 2];
    loop {
        let session = projects.get_mut(id).unwrap();
        session.sync_plugins();
        renderer.process(&mut block);
        if done(session) {
            return;
        }
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_cancelled_gather_starts_nothing_on_a_late_reply() {
    // Its lanes and clips get ids, as a session gives them, so it compares with the session's.
    let mut opened = project();
    assert_eq!(opened.assign_ids(), Ok(()));
    let mut renderer = Renderer::stopped(&opened, &AudioPool::new(), SR).unwrap();
    let mut session = Session::new(opened.clone());
    session.attach_player(Player::new(renderer.controller(), None));
    session.set_plugin_env(env());
    // The gate is known before the plug-in's host process is born, which inherits it.
    let mut gate = StateGate::new();
    let device = json!({ "track": 1, "device": 0 });
    let deadline = Instant::now() + Duration::from_secs(30);
    session.sync_plugins();
    until(&mut session, &mut renderer, "starting", |s| {
        s.execute("device.plugin", &device).unwrap()["status"] == "running"
    });

    // The read is out and held out, so the cancel happens while it is genuinely out, not
    // racing its answer; the late reply then lands and starts nothing — the round is gone
    // with the cancel.
    gate.hold();
    assert!(matches!(digs(&mut session, &mut renderer, deadline), suhina_session::ExportGather::Pending));
    session.cancel_export_gather();
    gate.release();
    std::thread::sleep(Duration::from_millis(400));
    // A later export gathers its own round, and its read answers it: the job was cleaned up.
    let data = loop {
        match digs(&mut session, &mut renderer, deadline) {
            suhina_session::ExportGather::Pending => std::thread::sleep(std::time::Duration::from_millis(10)),
            suhina_session::ExportGather::Ready(data) => break data,
        }
    };
    assert_eq!(gain(&data.project), Some(DEFAULT_GAIN), "the later gather's own read answered");
    assert!(!session.can_undo());
    assert_eq!(session.project(), &opened);
}

#[test]
fn a_gather_past_its_deadline_says_which_plug_in_did_not_answer() {
    // Its lanes and clips get ids, as a session gives them, so it compares with the session's.
    let mut opened = project();
    assert_eq!(opened.assign_ids(), Ok(()));
    let mut renderer = Renderer::stopped(&opened, &AudioPool::new(), SR).unwrap();
    let mut session = Session::new(opened.clone());
    session.attach_player(Player::new(renderer.controller(), None));
    session.set_plugin_env(env());
    // The gate is known before the plug-in's host process is born, which inherits it.
    let mut gate = StateGate::new();
    session.sync_plugins();
    until(&mut session, &mut renderer, "starting", |s| {
        s.execute("device.plugin", &json!({ "track": 1, "device": 0 })).unwrap()["status"] == "running"
    });

    // The read goes out and is held out (the start's own read was answered before the
    // plug-in ran), and the caller's deadline is already past: the round settles on the
    // launch-time fallback, and says which plug-in did not answer.
    gate.hold();
    let gone = Instant::now();
    let mut block = vec![0.0f32; 256 * 2];
    renderer.process(&mut block);
    assert!(
        matches!(session.gather_export_data(gone), suhina_session::ExportGather::Pending),
        "the first call launches the round"
    );
    renderer.process(&mut block);
    let suhina_session::ExportGather::Ready(data) = session.gather_export_data(gone) else {
        panic!("an expired gather must settle");
    };
    gate.release();
    let notices = data.warnings;
    assert!(
        notices
            .iter()
            .any(|n| n.contains("Test vst3 did not answer in time; the settings read are the last ones it gave")),
        "{notices:?}"
    );
    assert!(!session.can_undo());
}

/// The test plug-in's state-read gate (`SUHINA_TEST_PLUGIN_STATE_GATE`): while it is held, the
/// plug-in's state reads keep their replies, so a test holds a read out for as long as it
/// needs, never racing its answer. Hosts inherit the variable at their spawn, so it names one
/// file for the whole test binary, and the tests that hold it take turns. Dropped, it lets
/// every read go.
struct StateGate {
    path: PathBuf,
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl StateGate {
    fn new() -> Self {
        static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
        static SET: std::sync::Once = std::sync::Once::new();
        let turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let path = std::env::temp_dir().join(format!("suhina-state-gate-{}", std::process::id()));
        // SAFETY: set once, to the same value for every test; the tests' hosts inherit it at
        // their spawn, and nothing inside this process reads it.
        SET.call_once(|| unsafe { std::env::set_var("SUHINA_TEST_PLUGIN_STATE_GATE", &path) });
        let _ = std::fs::remove_file(&path);
        Self { path, _turn: turn }
    }

    /// From now on, the plug-in's state reads hold their replies.
    fn hold(&mut self) {
        std::fs::write(&self.path, b"").unwrap();
    }

    /// The held reads answer.
    fn release(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Drop for StateGate {
    fn drop(&mut self) {
        self.release();
    }
}

/// Plays blocks while the session looks after its plug-ins, until `done` holds.
fn until(session: &mut Session, renderer: &mut Renderer, what: &str, mut done: impl FnMut(&mut Session) -> bool) {
    let deadline = Instant::now() + PATIENT;
    let mut block = vec![0.0f32; 256 * 2];
    loop {
        session.sync_plugins();
        renderer.process(&mut block);
        if done(session) {
            return;
        }
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn an_export_plays_the_running_plug_ins_state_without_a_project_change() {
    // Its lanes and clips get ids, as a session gives them, so it compares with the session's.
    let mut opened = project();
    assert_eq!(opened.assign_ids(), Ok(()));
    let dry = render_offline(&opened, SR, 256).unwrap();
    let peak = |samples: &[f32]| samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak(&dry) > 0.1, "the notes sound: {}", peak(&dry));
    let mut renderer = Renderer::stopped(&opened, &AudioPool::new(), SR).unwrap();
    let mut session = Session::new(opened.clone());
    session.attach_player(Player::new(renderer.controller(), None));
    session.set_plugin_env(env());
    let device = json!({ "track": 1, "device": 0 });
    until(&mut session, &mut renderer, "starting", |s| {
        s.execute("device.plugin", &device).unwrap()["status"] == "running"
    });

    // Untouched, the export plays the default gain through the running plug-in, its state read
    // as the export takes its data.
    let ratio = export_peak(&mut session) / peak(&dry);
    assert!((ratio - DEFAULT_GAIN).abs() < 1e-4, "{ratio}");

    // Editors open on Windows, and on Linux with an X server.
    let editors =
        cfg!(windows) || (cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()));
    if !editors {
        eprintln!("no display for editors; the edit part is skipped");
        return;
    }
    // The user turns the gain in the plug-in's editor, never touching the project (the test
    // plug-in does so when its editor opens). The export then plays what is heard, against the
    // project as opened.
    session.execute("device.openEditor", &device).unwrap();
    until(&mut session, &mut renderer, "the edit reaching the data", |s| {
        gain(&s.project_as_heard()) == Some(EDITOR_GAIN)
    });
    session.execute("device.closeEditor", &device).unwrap();
    let ratio = export_peak(&mut session) / peak(&dry);
    assert!((ratio - EDITOR_GAIN).abs() < 1e-4, "{ratio}");
    assert!(
        (render_offline(&opened, SR, 256).unwrap().iter().fold(0.0f32, |m, s| m.max(s.abs())) - peak(&dry)).abs()
            < 1e-4
    );

    // The export read the state; it changed nothing in the project: nothing to undo, the open
    // project as it was, and the dry render again as it was.
    assert!(!session.can_undo());
    assert_eq!(session.project(), &opened);
}
