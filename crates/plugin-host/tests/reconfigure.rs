// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Audio configuration changes retain a live host and plugin state on the same IPC link.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::time::Duration;
use suhina_plugins::{Launch, Plugin};

#[test]
fn rate_changes_keep_clap_and_vst3_state_and_the_audio_link() {
    for spec in ["test:clap", "test:vst3"] {
        let timeout = Duration::from_secs(10);
        let (mut plugin, mut audio) = Plugin::start(Launch {
            host: PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host")),
            plugin: spec.into(),
            channels: 2,
            max_frames: 512,
            sample_rate: 44_100.0,
            timeout,
            group: None,
            ara: None,
        })
        .unwrap();
        plugin.load_state(0.25f32.to_le_bytes().to_vec()).unwrap();
        let before = plugin.save_state().unwrap();
        let process = plugin.process_id();
        let input = [0.5; 256];
        let mut output = [0.0; 256];
        let _ = audio.process(&input, &mut output, timeout);
        let baseline = output;
        assert!(baseline.iter().any(|s| *s != 0.0));
        for rate in [48_000.0, 96_000.0, 44_100.0, 44_100.0] {
            // The output owner has the audio half; the control half remains in the session.
            audio.reconfigure(rate, timeout).unwrap();
            assert_eq!(plugin.process_id(), process, "{spec}");
            assert_eq!(plugin.save_state().unwrap(), before, "{spec}");
            output.fill(f32::NAN);
            let _ = audio.process(&input, &mut output, timeout);
            assert_eq!(output, baseline, "{spec} at {rate}");
        }
    }
}

#[test]
fn retained_processors_update_the_graph_latency_without_reloading() {
    use suhina_engine::{PluginSlotInfo, Plugins};
    use suhina_session::Hosted;
    let timeout = Duration::from_secs(10);
    let mut controls = Vec::new();
    let mut processors = Plugins::new();
    for key in 1..=2 {
        let (plugin, audio) = Plugin::start(Launch {
            host: PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host")),
            plugin: "test:clap".into(),
            channels: 2,
            max_frames: 512,
            sample_rate: 44_100.0,
            timeout,
            group: Some("retained-reconfiguration-test".into()),
            ara: None,
        })
        .unwrap();
        processors.insert(key, PluginSlotInfo { instrument: false, latency: 12345 }, Box::new(Hosted::new(audio)));
        controls.push(plugin);
    }
    let pid = controls[0].process_id();
    assert_eq!(controls[1].process_id(), pid);
    processors.reconfigure(48_000, timeout).unwrap();
    for (index, control) in controls.iter_mut().enumerate() {
        assert_eq!(control.process_id(), pid);
        assert!(control.save_state().is_ok());
        assert_eq!(processors.info()[&(index as u64 + 1)].latency, control.latency());
    }
}
