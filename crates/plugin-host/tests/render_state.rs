// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A render from an open project plays what is heard (issue #363, ADR 0006 decision 5): it
//! starts its plug-ins with the state the running ones have now, not the state the project was
//! opened with. Reading that state changes nothing in the open project.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use suhina_engine::{RenderInputs, render_offline_from};

use serde_json::json;
use suhina_engine::{AudioPool, Renderer, render_offline};
use suhina_model::*;
use suhina_plugins::scan::{Catalog, PluginInfo};
use suhina_session::{Player, PluginEnv, Session, start_offline};

const SR: u32 = 48_000;
const PATIENT: Duration = Duration::from_secs(20);
/// The gain the VST3 test plug-in starts with, and the one its editor turns it to when it opens.
const DEFAULT_GAIN: f32 = 0.5;
const EDITOR_GAIN: f32 = 0.125;

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

/// One note track without an instrument (its notes play as sine tones) and the test plug-in
/// after them, with no saved state (its default gain), playing a note at beat 0 in the
/// arrangement.
fn project() -> Project {
    let note = Note::new(0.0, 0.5, 69, 1.0);
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
                // The catalog lists the built-in test plug-ins as CLAP (their file is not a `.vst3`).
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
            length: 1.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Notes(vec![note]) }],
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                id: Default::default(),
                name: "l".into(),
                track: Some(TrackId(1)),
                clips: vec![Clip::new(ContentId(1), 0.0, 1.0)],
            }],
            markers: Vec::new(),
        },
        ..Project::default()
    }
}

/// Renders `project` offline as a render from the session does (`project_render`), its
/// plug-ins started with the state the project holds, and returns the gain the plug-in played
/// with: the render's peak over the peak without the plug-in.
fn render(project: &Project) -> f32 {
    let peak = |samples: &[f32]| samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let started = start_offline(project, &AudioPool::new(), &env(), SR);
    assert_eq!(started.running(), 1, "{:?}", started.notices);
    let wet = render_offline_from(
        project,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(started.plugins),
        SR,
        256,
        None,
    )
    .unwrap();
    let dry = render_offline(project, SR, 256).unwrap();
    assert!(peak(&dry) > 0.1);
    peak(&wet) / peak(&dry)
}

/// The gain a project holds for its plug-in: its state is the gain as 4 bytes.
fn gain(project: &Project) -> Option<f32> {
    let state = project.channels[0].devices[0].state.as_deref()?;
    Some(f32::from_le_bytes(state.try_into().ok()?))
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
fn a_render_from_an_open_project_uses_the_running_plug_ins_state() {
    // Its lanes and clips get ids, as a session gives them, so it compares with the session's.
    let mut opened = project();
    assert_eq!(opened.assign_ids(), Ok(()));
    let mut renderer = Renderer::stopped(&opened, &AudioPool::new(), SR).unwrap();
    let mut session = Session::new(opened.clone());
    session.attach_player(Player::new(renderer.controller(), None));
    session.set_plugin_env(env());
    let device = json!({ "track": 1, "device": 0 });
    until(&mut session, &mut renderer, "starting", |s| {
        s.execute("device.plugin", &device).unwrap()["status"] == "running"
    });

    // Untouched, the running plug-in has its default gain, and so has the render.
    let heard = session.project_as_heard();
    assert_eq!(gain(&heard), Some(DEFAULT_GAIN));
    assert!((render(&heard) - DEFAULT_GAIN).abs() < 1e-4, "{}", render(&heard));

    // Editors open on Windows, and on Linux with an X server.
    let editors =
        cfg!(windows) || (cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty()));
    if !editors {
        eprintln!("no display for editors; the edit part is skipped");
        return;
    }
    // The user turns the gain in the plug-in's editor (the test plug-in's does so when it
    // opens). The project does not change; the running plug-in's state does.
    session.execute("device.openEditor", &device).unwrap();
    until(&mut session, &mut renderer, "the edit reaching the plug-in", |s| {
        gain(&s.project_as_heard()) == Some(EDITOR_GAIN)
    });
    session.execute("device.closeEditor", &device).unwrap();

    // A render from the session plays the edit; one from the project as opened would not.
    let heard = session.project_as_heard();
    assert!((render(&heard) - EDITOR_GAIN).abs() < 1e-4, "{}", render(&heard));
    assert!((render(session.project()) - DEFAULT_GAIN).abs() < 1e-4);

    // Reading the plug-in's state is not an edit of the project: nothing to undo, and the open
    // project is as it was. The change made in the editor is unsaved all the same (issue #362).
    assert!(!session.can_undo());
    assert_eq!(session.project(), &opened);
    session.check_plugin_edits();
    assert!(session.has_unsaved_changes());
}
