// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Saving a project with running plug-ins (issue #111, ADR 0010 decision 7): `project.save`
//! asks each running plug-in for its state and writes that into the file, the component's
//! state into `Device::state` and a VST3 controller's into its extension, without recording a
//! change or an undo step. A plug-in that is not installed is saved with the state it came with.
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
const CONTROLLER: &str = "Vst3ControllerState";

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

/// A VST2 chunk bank holding `gain`, as a project imported from another format keeps a
/// plug-in's state. The test plug-in reads it and from then on gives its own state: the gain
/// as 4 bytes. So a saved file holding 4 bytes got them from the running plug-in.
fn bank(gain: f32) -> Vec<u8> {
    let mut bank = b"CcnK".to_vec();
    bank.extend_from_slice(&(160 - 8 + 4u32).to_be_bytes());
    bank.extend_from_slice(b"FBCh");
    bank.resize(156, 0);
    bank.extend_from_slice(&4u32.to_be_bytes());
    bank.extend_from_slice(&gain.to_le_bytes());
    bank
}

/// One note track with the test plug-in, with a bank and a controller state, and a plug-in that
/// is not installed.
fn project() -> Project {
    let plugin = Device {
        name: "Test vst3".into(),
        // The catalog lists the built-in test plug-ins as CLAP (their file is not a `.vst3`).
        kind: DeviceKind::Clap { id: "vst3".into() },
        enabled: true,
        state: Some(bank(0.25)),
        params: Vec::new(),
        clip_host: false,
        extensions: Extensions(vec![Extension {
            format: "vst3".into(),
            key: CONTROLLER.into(),
            data: b"controller".to_vec(),
        }]),
        automation: Vec::new(),
    };
    let missing =
        Device { state: Some(vec![1, 2, 3]), ..Device::new("Gone", DeviceKind::Clap { id: "not-installed".into() }) };
    Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "T")
        }],
        channels: vec![Channel { devices: vec![plugin, missing], ..Channel::new(ChannelId(1), "C") }],
        ..Project::default()
    }
}

/// A scratch folder of this test's own under the target directory.
fn folder(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_save_writes_the_state_the_running_plug_in_gives() {
    let imported = project();
    let mut renderer = Renderer::stopped(&imported, &AudioPool::new(), SR).unwrap();
    let mut session = Session::new(imported.clone());
    session.attach_player(Player::new(renderer.controller(), None));
    session.rack_mut().set_env(env());

    // Wait for the plug-in to run with the bank it was given.
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

    let path = folder("plugin-state-save").join("song.suhina");
    let saved = session.execute("project.save", &json!({ "path": path.display().to_string() })).unwrap();
    assert_eq!(saved["path"], path.display().to_string());
    // Opening a file only reads it; its plug-ins start once a catalog is set.
    let (file, _) = Session::open(&path).unwrap();
    let devices = &file.project().channels[0].devices;

    // The plug-in's own state as it runs, not the bank the project came with.
    assert_eq!(devices[0].state, Some(0.25f32.to_le_bytes().to_vec()));
    let controllers: Vec<&Extension> = devices[0].extensions.0.iter().filter(|e| e.key == CONTROLLER).collect();
    assert_eq!(controllers.len(), 1, "one controller state, not the imported one and the new one");
    assert_eq!(controllers[0].data, b"controller");
    // The one that is not installed keeps what it came with.
    assert_eq!(devices[1], imported.channels[0].devices[1]);

    // Reading a plug-in's state is not an edit: nothing to undo, nothing unsaved, and the open
    // project is as it was.
    assert!(!session.can_undo());
    assert!(!session.has_unsaved_changes());
    assert_eq!(session.project().channels[0].devices, imported.channels[0].devices);
}
