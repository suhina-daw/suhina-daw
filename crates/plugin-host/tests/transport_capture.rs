// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Opt-in, offline-only capture of a user-owned instrument preset. Never opens an audio
//! device. Private projects, presets and recordings stay outside the repository.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use suhina_engine::{
    AudioPool, DeviceEvent, LaunchCommand, ParamEvent, PluginProcessor, PluginSlotInfo, Plugins, RenderInputs,
    RenderOptions, Renderer, SongPosition, plugin_devices,
};
use suhina_plugins::{Launch, Plugin, SavedState};
use suhina_session::{Hosted, Session};

struct Tap {
    hosted: Hosted,
    samples: Arc<Mutex<Vec<f32>>>,
    mode: String,
}

impl PluginProcessor for Tap {
    fn smooth_transport_stop(&self) -> bool {
        self.hosted.smooth_transport_stop()
    }
    fn process(&mut self, buffer: &mut [f32], events: &[(usize, DeviceEvent)], timeout: Duration) -> bool {
        let frame = self.samples.lock().unwrap().len() / 2;
        if !events.is_empty() {
            eprintln!("frame {frame}: {events:?}");
        }
        let filtered: Vec<_> = events
            .iter()
            .copied()
            .filter(|(_, event)| self.mode != "no_all_off" || !matches!(event, DeviceEvent::AllNotesOff))
            .collect();
        let answered = self.hosted.process(buffer, &filtered, timeout);
        assert!(answered, "offline host missed a block");
        self.samples.lock().unwrap().extend_from_slice(buffer);
        answered
    }
    fn transport(&mut self, song: &SongPosition) {
        let mut song = *song;
        if self.mode == "keep_playing" {
            song.playing = true;
        }
        self.hosted.transport(&song);
    }
    fn automate(&mut self, params: &[ParamEvent]) {
        self.hosted.automate(params);
    }
}

#[test]
#[ignore = "requires an explicitly selected local project and installed instrument"]
fn capture_instrument_and_mix_around_pause() {
    let project_path = PathBuf::from(std::env::var("SUHINA_CAPTURE_PROJECT").unwrap());
    let track_name = std::env::var("SUHINA_CAPTURE_TRACK").unwrap();
    let spec = std::env::var("SUHINA_CAPTURE_PLUGIN").unwrap();
    let destination = PathBuf::from(std::env::var("SUHINA_CAPTURE_OUT").unwrap());
    let rate = std::env::var("SUHINA_CAPTURE_RATE").ok().and_then(|s| s.parse::<u32>().ok()).unwrap_or(48_000);
    let (session, _) = Session::open(&project_path).unwrap();
    let mut project = session.project().clone();
    let track = project.tracks.iter().find(|t| t.name == track_name).unwrap().clone();
    let channel = track.channel.unwrap();
    project.tracks.retain(|t| t.id == track.id);
    project.tracks[0].parent = None;
    project.channels.retain(|c| c.id == channel);
    project.channels[0].output = None;
    project.channels[0].sends.clear();
    project.channels[0].mute = false;
    project.arrangement.lanes.retain(|l| l.track == Some(track.id));
    project.launcher = Default::default();
    let found = plugin_devices(&project);
    assert_eq!(found.len(), 1, "select a track with one instrument and no hosted effects");
    let device = &project.channels[0].devices[found[0].position];
    let timeout = Duration::from_secs(30);
    let (mut control, mut audio) = Plugin::start(Launch {
        host: PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host")),
        plugin: spec,
        channels: 2,
        max_frames: 2048,
        sample_rate: 48_000.0,
        timeout,
        group: None,
        ara: None,
    })
    .unwrap();
    control
        .load_state(SavedState {
            data: device.state.clone().unwrap(),
            controller: device.extensions.0.iter().find(|e| e.key == "Vst3ControllerState").map(|e| e.data.clone()),
        })
        .unwrap();
    if rate != 48_000 {
        let pid = control.process_id();
        let seat = audio.seat();
        audio.reconfigure(f64::from(rate), timeout).unwrap();
        assert_eq!(control.process_id(), pid);
        assert_eq!(audio.seat(), seat, "the retained instance changed");
    }
    let raw = Arc::new(Mutex::new(Vec::new()));
    let mut plugins = Plugins::new();
    plugins.insert(
        found[0].key,
        PluginSlotInfo { instrument: true, latency: control.latency() },
        Box::new(Tap {
            hosted: Hosted::new(audio)
                .with_instrument_transport(std::env::var("SUHINA_CAPTURE_MODE").unwrap_or_default() == "fixed", rate),
            samples: Arc::clone(&raw),
            mode: std::env::var("SUHINA_CAPTURE_MODE").unwrap_or_default(),
        }),
    );
    let mut renderer = Renderer::build(
        &project,
        RenderInputs::from_audio(&AudioPool::new()).with_plugins(plugins),
        rate,
        RenderOptions::STOPPED,
    )
    .unwrap();
    renderer.handle(LaunchCommand::SetPosition { beat: 128.0 });
    let mut mix = Vec::new();
    let mut block = [0.0; 1024];
    for n in 0..160 {
        if n == 8 {
            renderer.handle(LaunchCommand::Play);
        }
        if n == 55 {
            renderer.handle(if std::env::var("SUHINA_CAPTURE_STOP").is_ok() {
                LaunchCommand::Stop
            } else {
                LaunchCommand::StopHere
            });
        }
        renderer.process(&mut block);
        mix.extend_from_slice(&block);
    }
    std::fs::create_dir_all(&destination).unwrap();
    for (name, samples) in [("instrument", raw.lock().unwrap().clone()), ("mix", mix)] {
        assert!(samples.iter().any(|s| s.abs() > 0.001), "capture was silent");
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        std::fs::write(destination.join(format!("{name}.f32")), bytes).unwrap();
    }
    eprintln!("{rate} Hz stereo f32le: Play at frame 4096, stop at frame 28160; latency {}", control.latency());
}
