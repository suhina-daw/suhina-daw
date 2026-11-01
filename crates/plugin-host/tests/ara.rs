// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A plug-in the scan marked as one that edits clips (ADR 0028) is loaded with an ARA document
//! of its track's audio clips: the document crosses to the host process, which binds it, or
//! refuses the load with why when the plug-in cannot be bound. Without audio clips it loads as
//! any other plug-in.
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::sync::Arc;

use suhina_engine::{AudioData, AudioPool};
use suhina_model::*;
use suhina_plugins::scan::{Catalog, PluginInfo};
use suhina_session::{PluginEnv, start_offline};

const SR: u32 = 48_000;

/// The host's VST3 test plug-in (a gain, with no ARA in it), listed as one that supports ARA.
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
        ara: true,
    };
    let mut catalog = Catalog::new();
    catalog.insert(path, vec![info]);
    Arc::new(PluginEnv::new(catalog, PathBuf::from(env!("CARGO_BIN_EXE_suhina-plugin-host"))))
}

/// An audio track with the test plug-in on it and, when `clip`, one audio clip of `take.wav`.
fn project(clip: bool) -> Project {
    let region = AudioRegion {
        file: "take.wav".into(),
        absolute_file: None,
        sample_rate: Some(SR),
        channels: Some(2),
        duration_seconds: Some(1.0),
        warped: false,
        stretch: StretchMode::default(),
        warp_markers: Vec::new(),
        gain: 1.0,
        transpose: 0.0,
        stream: None,
    };
    let mut channel = Channel::new(ChannelId(1), "C");
    channel.devices.push(Device {
        name: "Test vst3".into(),
        // The catalog lists the built-in test plug-ins as CLAP (their file is not a `.vst3`).
        kind: DeviceKind::Clap { id: "vst3".into() },
        enabled: true,
        state: None,
        params: Vec::new(),
        clip_host: false,
        extensions: Extensions::default(),
        automation: Vec::new(),
    });
    let mut track = Track::new(TrackId(1), "Vocals");
    track.content.audio = true;
    track.channel = Some(ChannelId(1));
    let mut clip_at = Clip::new(ContentId(1), 0.0, 2.0);
    clip_at.id = ClipId(7);
    Project {
        tracks: vec![track],
        channels: vec![channel],
        contents: vec![ClipContent {
            id: ContentId(1),
            name: "take".into(),
            length: 2.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Audio(region) }],
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                track: Some(TrackId(1)),
                clips: if clip { vec![clip_at] } else { Vec::new() },
                ..ArrangementLane::default()
            }],
            markers: Vec::new(),
            takes: Vec::new(),
        },
        ..Project::default()
    }
}

fn audio() -> AudioPool {
    let mut audio = AudioPool::new();
    let data = AudioData { sample_rate: SR, channels: 2, samples: vec![0.5; SR as usize * 2] };
    audio.files.insert("take.wav".into(), Arc::new(data));
    audio
}

#[test]
fn a_plug_in_that_cannot_be_bound_says_so() {
    let started = start_offline(&project(true), &audio(), &env(), SR);
    assert_eq!(started.running(), 0);
    assert!(
        started.notices.iter().any(|n| n.contains("Test vst3 could not start") && n.contains("does not support ARA")),
        "{:?}",
        started.notices
    );
}

#[test]
fn without_audio_clips_it_loads_as_any_plug_in() {
    let started = start_offline(&project(false), &audio(), &env(), SR);
    assert_eq!(started.running(), 1, "{:?}", started.notices);
    // A clip whose file has no samples is not in a document either.
    let started = start_offline(&project(true), &AudioPool::new(), &env(), SR);
    assert_eq!(started.running(), 1, "{:?}", started.notices);
}
