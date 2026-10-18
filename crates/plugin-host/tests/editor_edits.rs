// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A change made in a plug-in's editor counts as unsaved (issue #362), noticed by comparing the
//! states the session reads with the one the plug-in reported when it started or the project
//! was saved. These tests run the host's VST3 test plug-in and check the other side: a plug-in
//! that is only running, read again and again, never makes the project unsaved, even when it
//! reports the state it was given in a form of its own.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use suhina_engine::{AudioPool, Renderer};
use suhina_model::*;
use suhina_plugins::scan::{Catalog, PluginInfo};
use suhina_session::{Player, PluginEnv, Session};

const SR: u32 = 48_000;
const PATIENT: Duration = Duration::from_secs(20);

/// A catalog with the host's VST3 test plug-in, an instrument with the id `vst3`.
fn env() -> Arc<PluginEnv> {
    let path = PathBuf::from("test:vst3");
    let info = PluginInfo {
        path: path.clone(),
        id: "vst3".into(),
        name: "Test vst3".into(),
        vendor: "Suhina".into(),
        version: "1".into(),
        features: vec!["Instrument|Synth".into()],
        replaces: Vec::new(),
        ara: false,
    };
    let mut catalog = Catalog::new();
    catalog.insert(path, vec![info]);
    Arc::new(PluginEnv::new(catalog, PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host"))))
}

/// A VST2 chunk bank holding `gain`. The test plug-in reads it and from then on reports its own
/// state (the gain as 4 bytes), so its state never reads back as the bytes it was given.
fn bank(gain: f32) -> Vec<u8> {
    let mut bank = b"CcnK".to_vec();
    bank.extend_from_slice(&(160 - 8 + 4u32).to_be_bytes());
    bank.extend_from_slice(b"FBCh");
    bank.resize(156, 0);
    bank.extend_from_slice(&4u32.to_be_bytes());
    bank.extend_from_slice(&gain.to_le_bytes());
    bank
}

/// One note track with the test plug-in, given a bank.
fn project() -> Project {
    let plugin = Device {
        name: "Test vst3".into(),
        // The catalog lists the built-in test plug-ins as CLAP (their file is not a `.vst3`).
        kind: DeviceKind::Clap { id: "vst3".into() },
        enabled: true,
        state: Some(bank(0.25)),
        params: Vec::new(),
        extensions: Extensions::default(),
        automation: Vec::new(),
    };
    Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "T")
        }],
        channels: vec![Channel { devices: vec![plugin], ..Channel::new(ChannelId(1), "C") }],
        ..Project::default()
    }
}

/// Plays `session` until its plug-in runs.
fn wait_running(session: &mut Session, renderer: &mut Renderer) {
    let device = json!({ "track": 1, "device": 0 });
    let deadline = Instant::now() + PATIENT;
    let mut output = vec![0.0f32; 256 * 2];
    loop {
        session.sync_plugins();
        renderer.process(&mut output);
        let status = session.execute("device.plugin", &device).unwrap();
        if status["status"] == "running" {
            break;
        }
        assert!(Instant::now() < deadline, "starting: {status}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_running_plug_in_read_again_and_again_is_no_unsaved_change() {
    let project = project();
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR).unwrap();
    let mut session = Session::new(project);
    session.attach_player(Player::new(renderer.controller(), None));
    session.set_plugin_env(env());
    wait_running(&mut session, &mut renderer);

    for _ in 0..3 {
        session.check_plugin_edits();
        session.sync_plugins();
    }
    assert!(!session.has_unsaved_changes(), "its own form of the state it was given is not an edit");
    assert!(!session.can_undo());

    // Stopped (its state read as it goes) and started again with that state: still nothing.
    let player = session.detach_player();
    assert!(player.is_some());
    session.sync_plugins();
    std::thread::sleep(Duration::from_millis(200));
    session.attach_player(Player::new(renderer.controller(), None));
    wait_running(&mut session, &mut renderer);
    session.check_plugin_edits();
    assert!(!session.has_unsaved_changes());
}
