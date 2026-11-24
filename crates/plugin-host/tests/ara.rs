// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A clip host (ADR 0033 › 2, #990), or a device whose saved state holds an ARA archive, is loaded
//! with an ARA document of its track's audio clips: the document crosses to the host process,
//! which binds it, or refuses the load with why when the plug-in cannot be bound. Without audio
//! clips it loads as any other plug-in, and so does a plug-in that supports ARA put in a chain as
//! an insert (ADR 0033, amendment).
#![allow(clippy::panic, clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;
use std::sync::Arc;

use suhina_engine::{AudioData, AudioPool};
use suhina_model::*;
use suhina_plugins::scan::{Catalog, PluginInfo};
use suhina_session::{PluginEnv, start_offline, start_offline_device};

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

/// An audio track with the test plug-in on it as its clip host and, when `clip`, one audio clip
/// of `take.wav` that plays through it.
fn project(clip: bool) -> Project {
    let region = AudioRegion {
        file: "take.wav".into(),
        absolute_file: None,
        sample_rate: Some(SR),
        channels: Some(2),
        duration_seconds: Some(1.0),
        stream: None,
    };
    let mut channel = Channel::new(ChannelId(1), "C");
    channel.devices.push(Device {
        instrument: false,
        inputs: Vec::new(),
        id: Default::default(),
        name: "Test vst3".into(),
        // The catalog lists the built-in test plug-ins as CLAP (their file is not a `.vst3`).
        kind: DeviceKind::Clap { id: "vst3".into() },
        enabled: true,
        state: None,
        params: Vec::new(),
        clip_host: true,
        extensions: Extensions::default(),
        automation: Vec::new(),
    });
    let mut track = Track::new(TrackId(1), "Vocals");
    track.content.audio = true;
    track.channel = Some(ChannelId(1));
    let mut clip_at = Clip::new(ContentId(1), 0.0, 2.0);
    clip_at.id = ClipId(7);
    clip_at.ara = true;
    Project {
        tracks: vec![track],
        channels: vec![channel],
        contents: vec![ClipContent {
            linked: false,
            id: ContentId(1),
            name: "take".into(),
            length: 2.0,
            lanes: vec![ContentLane { target: None, data: LaneData::Audio(region) }].into(),
            extensions: Extensions::default(),
            scale: None,
        }],
        arrangement: Arrangement {
            lanes: vec![ArrangementLane {
                track: Some(TrackId(1)),
                clips: if clip { vec![clip_at] } else { Vec::new() },
                ..ArrangementLane::default()
            }],
            markers: Default::default(),
            takes: Vec::new(),
        },
        ..Project::default()
    }
}

/// [`project`] with the test plug-in an insert in the track's chain, as a plug-in put on a track
/// or an imported project's is, and with `state` saved for it.
fn inserted(state: Option<Vec<u8>>) -> Project {
    let mut project = project(true);
    let device = &mut project.channels[0].devices[0];
    device.clip_host = false;
    device.state = state.map(Into::into);
    project.arrangement.lanes[0].clips[0].ara = false;
    project
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

/// The case a tester's imported project showed: a plug-in that supports ARA, an insert on a
/// track with audio clips, runs as any plug-in, with no document to bind (it was loaded with
/// one, and a crash in it took down every track that shared the document).
#[test]
fn an_ara_plug_in_in_a_chain_is_an_insert() {
    let started = start_offline(&inserted(None), &audio(), &env(), SR);
    assert_eq!(started.running(), 1, "{:?}", started.notices);
    // A state of the plug-in's own, as an imported project carries, changes nothing.
    let started = start_offline(&inserted(Some(vec![1, 2, 3])), &audio(), &env(), SR);
    assert!(started.notices.iter().all(|n| !n.contains("does not support ARA")), "{:?}", started.notices);
}

/// A device bound to its track's clips before the rule changed keeps its edits: its saved state
/// holds an ARA archive, so it is loaded with the document again (the test plug-in refuses it,
/// which shows the document was sent).
#[test]
fn a_saved_ara_archive_keeps_a_device_bound() {
    let state = suhina_plugins::protocol::wrap_ara_state(vec![1, 2, 3], Some(("com.example.doc".into(), vec![9; 4])));
    let started = start_offline(&inserted(Some(state)), &audio(), &env(), SR);
    assert_eq!(started.running(), 0);
    assert!(started.notices.iter().any(|n| n.contains("does not support ARA")), "{:?}", started.notices);
}

/// A device bound before, on a track with no audio clips it can be given, loads without a
/// document; its archive waits in its state for one (the host gives it back with the plug-in's
/// own), so a save or a restart neither loses the edits nor lets it go as an insert: once the
/// track has its clip again, the device loads bound.
#[test]
fn an_archive_waits_in_the_state_of_a_device_without_a_document() {
    let archive = ("com.example.doc".to_string(), vec![9; 4]);
    let own = 0.25f32.to_le_bytes().to_vec();
    let state = suhina_plugins::protocol::wrap_ara_state(own.clone(), Some(archive.clone()));
    let mut project = project(false);
    project.channels[0].devices[0].clip_host = false;
    project.channels[0].devices[0].state = Some(state.into());
    assert_eq!(project.assign_ids(), Ok(()));
    let key = suhina_engine::plugin_devices(&project)[0].key;
    let mut started = start_offline_device(&project, &audio(), &env(), key, SR, None)
        .unwrap()
        .expect("the test plug-in is installed");
    assert!(started.refused.is_none(), "{:?}", started.refused);
    let read = started.control.save_state().unwrap();
    let (component, kept) = suhina_plugins::protocol::unwrap_ara_state(&read.data);
    assert_eq!(component, &own[..], "the plug-in's own state");
    assert_eq!(kept.map(|(id, bytes)| (id, bytes.to_vec())), Some(archive), "the archive with it");

    // That state saved, the track given its audio clip again: the device loads bound to it, with
    // the document (the test plug-in refuses it, which shows the document was sent).
    let mut with_clip = project(true);
    with_clip.channels[0].devices[0].clip_host = false;
    with_clip.channels[0].devices[0].state = Some(read.data.into());
    let started = start_offline(&with_clip, &audio(), &env(), SR);
    assert_eq!(started.running(), 0);
    assert!(started.notices.iter().any(|n| n.contains("does not support ARA")), "{:?}", started.notices);
}
