// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A real plug-in inside a device group (docs/spec/device-groups.md › Plug-ins and built-in
//! devices inside a group): it starts in its own process by its path, its state is saved from
//! the running plug-in, it restarts by its path, and grouping it keeps its process.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
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
    };
    let mut catalog = Catalog::new();
    catalog.insert(path, vec![info]);
    Arc::new(PluginEnv::new(catalog, PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host"))))
}

/// A VST2 chunk bank holding `gain`; the test plug-in reads it and from then on gives its own
/// state, the gain as 4 bytes.
fn bank(gain: f32) -> Vec<u8> {
    let mut bank = b"CcnK".to_vec();
    bank.extend_from_slice(&(160 - 8 + 4u32).to_be_bytes());
    bank.extend_from_slice(b"FBCh");
    bank.resize(156, 0);
    bank.extend_from_slice(&4u32.to_be_bytes());
    bank.extend_from_slice(&gain.to_le_bytes());
    bank
}

/// One note track whose chain is a device group holding the test plug-in.
fn project() -> Project {
    let plugin = Device { state: Some(bank(0.25)), ..Device::new("Test vst3", DeviceKind::Clap { id: "vst3".into() }) };
    let group = DeviceGroup::with_branches(vec![Branch { devices: vec![plugin], ..Branch::new("Synth") }]);
    Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "T")
        }],
        channels: vec![Channel { devices: vec![Device::group("Layers", group)], ..Channel::new(ChannelId(1), "C") }],
        ..Project::default()
    }
}

fn wait_running(session: &mut Session, renderer: &mut Renderer, device: &Value) {
    let deadline = Instant::now() + PATIENT;
    let mut output = vec![0.0f32; 256 * 2];
    loop {
        session.sync_plugins();
        renderer.process(&mut output);
        let status = session.execute("device.plugin", device).unwrap();
        if status["status"] == "running" {
            return;
        }
        assert!(Instant::now() < deadline, "starting: {status}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_plug_in_in_a_group_runs_saves_and_restarts_by_its_path() {
    let p = project();
    let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR).unwrap();
    let mut session = Session::new(p);
    session.attach_player(Player::new(renderer.controller(), None));
    session.set_plugin_env(env());
    let device = json!({ "track": 1, "device": "0/0/0" });
    wait_running(&mut session, &mut renderer, &device);
    let listed = session.execute("track.devices", &json!({ "track": 1 })).unwrap();
    assert_eq!(listed[0]["branches"][0]["devices"][0]["plugin"]["status"], "running");

    // The state saved is the running plug-in's own, inside the group.
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("plugin-in-group-save");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("song.suhina");
    session.execute("project.save", &json!({ "path": path.display().to_string() })).unwrap();
    let (file, _) = Session::open(&path).unwrap();
    let saved = device_at(&file.project().channels[0].devices, &[0, 0, 0]).unwrap();
    assert_eq!(saved.state, Some(0.25f32.to_le_bytes().to_vec()));

    // Ungrouping keeps the same process: it is running at once at its new place.
    session.execute("device.ungroup", &json!({ "track": 1, "device": 0 })).unwrap();
    let top = json!({ "track": 1, "device": 0 });
    session.sync_plugins();
    assert_eq!(session.execute("device.plugin", &top).unwrap()["status"], "running");

    // Grouped again, it restarts by its path and runs again.
    session.execute("device.group", &json!({ "track": 1, "device": 0 })).unwrap();
    session.execute("device.restart", &device).unwrap();
    wait_running(&mut session, &mut renderer, &device);
}
