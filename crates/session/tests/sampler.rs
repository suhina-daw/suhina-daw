// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::json;
use suhina_model::*;
use suhina_session::Session;

#[test]
fn sampler_loading_is_atomic_undoable_persistent_and_relinkable() {
    let folder = std::env::temp_dir().join(format!("suhina-sampler-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("source.wav");
    let mut writer = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for i in 0..4800 {
        writer.write_sample(((i as f32 * 0.03).sin() * 10000.) as i16).unwrap();
    }
    writer.finalize().unwrap();
    let project = Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Keys")
        }],
        channels: vec![Channel::new(ChannelId(1), "Keys")],
        ..Project::default()
    };
    let mut s = Session::new(project);
    s.execute("device.add", &json!({"track":1,"id":"sampler"})).unwrap();
    s.execute("device.loadSample", &json!({"track":1,"device":0,"path":path})).unwrap();
    let loaded = s.project().clone();
    let file = loaded.channels[0].devices[0].sample_file().unwrap();
    assert!(s.audio().files.contains_key(file));
    let loaded = suhina_project_file::from_json(&suhina_project_file::to_json(&loaded)).unwrap();
    let (pool, warnings) = suhina_media::load_project_audio(&loaded, &folder);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(pool.files[file].frames(), 4800);
    let mut renderer = suhina_engine::Renderer::stopped(&loaded, &pool, 48000).unwrap();
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    let mut playback = Session::new(loaded.clone());
    playback.set_audio(pool.clone());
    playback.attach_player(suhina_session::Player::new(controller, None));
    playback.execute("track.setArm", &json!({"track":1,"armed":true})).unwrap();
    live.send(suhina_engine::LiveEvent::NoteOn { key: 60, velocity: 1. });
    let mut block = vec![0.; 2048];
    renderer.process(&mut block);
    assert!(block.iter().any(|s| s.abs() > 0.001), "the engine must play the loaded sample");
    assert!(s.execute("device.loadSample", &json!({"track":1,"device":0,"path":folder.join("missing.wav")})).is_err());
    assert_eq!(s.project(), &loaded);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.project().channels[0].devices[0].sample_file().is_none());
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.project(), &loaded);
    let mut rebased = loaded.clone();
    suhina_project_file::rebase_audio(&mut rebased, &folder, &folder);
    assert_eq!(rebased.channels[0].devices[0].sample_file(), Some("source.wav"));
    let (pool, warnings) = suhina_media::load_project_audio(&rebased, &folder);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(pool.files.contains_key("source.wav"));
    let moved = folder.join("moved.wav");
    std::fs::copy(&path, &moved).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(suhina_media::missing_audio(s.project(), &folder).len(), 1);
    s.execute("project.relinkFiles", &json!({"files":[file], "to":[moved]})).unwrap();
    assert!(suhina_media::missing_audio(s.project(), &folder).is_empty());
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(suhina_media::missing_audio(s.project(), &folder).len(), 1);
    std::fs::remove_file(&moved).unwrap();
    std::fs::remove_dir(&folder).unwrap();
}

#[test]
fn sampler_editor_tab_persists_and_undoes_without_changing_other_parameters() {
    let mut s = Session::new(Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Keys")
        }],
        channels: vec![Channel::new(ChannelId(1), "Keys")],
        ..Project::default()
    });
    s.execute("device.add", &json!({"track":1,"id":"sampler"})).unwrap();
    let before = s.project().clone();
    s.execute("device.setParam", &json!({"track":1,"device":0,"param":"panel_view","value":2})).unwrap();
    let changed = s.project().clone();
    let saved = suhina_project_file::from_json(&suhina_project_file::to_json(&changed)).unwrap();
    assert_eq!(saved, changed);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.project(), &before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.project(), &changed);
}

#[test]
fn lfo_destinations_persist_and_undo() {
    let mut s = Session::new(Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Keys")
        }],
        channels: vec![Channel::new(ChannelId(1), "Keys")],
        ..Project::default()
    });
    s.execute("device.add", &json!({"track":1,"id":"sampler"})).unwrap();
    for (param, value) in [
        ("lfo_filter", 2.),
        ("lfo_volume", 0.7),
        ("lfo_pan", 1.),
        ("lfo_shape", 5.),
        ("fade_in", 10.),
        ("fade_out", 20.),
        ("playback", 3.),
        ("sensitivity", 0.8),
        ("slice_by", 3.),
        ("slice_1", 0.25),
    ] {
        let before = s.project().clone();
        s.execute("device.setParam", &json!({"track":1,"device":0,"param":param,"value":value})).unwrap();
        let changed = s.project().clone();
        assert_eq!(suhina_project_file::from_json(&suhina_project_file::to_json(&changed)).unwrap(), changed);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.project(), &before);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(s.project(), &changed);
    }
}

#[test]
fn dynamic_manual_slices_round_trip_validate_and_group_undo() {
    let mut s = Session::new(Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Keys")
        }],
        channels: vec![Channel::new(ChannelId(1), "Keys")],
        ..Project::default()
    });
    s.execute("device.add", &json!({"track":1,"id":"sampler"})).unwrap();
    let before = s.project().clone();
    let points: Vec<_> = (1..=300).rev().map(|i| i as f64 / 301.).collect();
    s.execute("device.setSampleSlices", &json!({"track":1,"device":0,"points":points})).unwrap();
    assert_eq!(s.project().channels[0].devices[0].sample_slices().unwrap().len(), 300);
    assert_eq!(suhina_project_file::from_json(&suhina_project_file::to_json(s.project())).unwrap(), *s.project());
    let changed = s.project().clone();
    let oversized = vec![0.5; suhina_model::MAX_SAMPLE_SLICES + 1];
    assert!(s.execute("device.setSampleSlices", &json!({"track":1,"device":0,"points":oversized})).is_err());
    let mut stored = s.project().channels[0].devices[0].clone();
    assert!(stored.set_sample_slices(&oversized).is_err());
    assert_eq!(&stored, &s.project().channels[0].devices[0]);
    stored.extensions.0.iter_mut().find(|e| e.key == "sample_slices").unwrap().data =
        vec![0; (suhina_model::MAX_SAMPLE_SLICES + 1) * 8];
    assert!(stored.sample_slices().is_none());
    for points in [json!([0.]), json!([1.]), json!([-0.1]), json!(["bad"]), json!(null)] {
        assert!(s.execute("device.setSampleSlices", &json!({"track":1,"device":0,"points":points})).is_err());
        assert_eq!(*s.project(), changed);
    }
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.project(), before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(*s.project(), changed);
    s.execute("device.setSampleSlices", &json!({"track":1,"device":0,"points":[0.8,0.2,0.2]})).unwrap();
    s.execute("device.setSampleSlices", &json!({"track":1,"device":0,"points":[0.9,0.2],"continue":true})).unwrap();
    assert_eq!(s.project().channels[0].devices[0].sample_slices().unwrap(), [0.2, 0.9]);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(*s.project(), changed);
    s.execute("device.setSampleSlices", &json!({"track":1,"device":0,"points":[]})).unwrap();
    assert_eq!(s.project().channels[0].devices[0].sample_slices(), Some(vec![]));
    let mut malformed = s.project().channels[0].devices[0].clone();
    malformed.extensions.0.iter_mut().find(|e| e.key == "sample_slices").unwrap().data = vec![0; 9];
    assert_eq!(malformed.sample_slices(), None);
}

#[test]
fn manual_map_updates_reach_live_playback_and_keep_the_held_voice() {
    use std::sync::Arc;
    let mut session = Session::new(Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Keys")
        }],
        channels: vec![Channel::new(ChannelId(1), "Keys")],
        ..Project::default()
    });
    session.execute("device.add", &json!({"track":1,"id":"sampler"})).unwrap();
    for (param, value) in [("playback", 3.), ("slice_by", 3.)] {
        session.execute("device.setParam", &json!({"track":1,"device":0,"param":param,"value":value})).unwrap();
    }
    session.execute("device.setSampleSlices", &json!({"track":1,"device":0,"points":[0.2]})).unwrap();
    let mut project = session.project().clone();
    project.channels[0].devices[0].set_sample_file("source.wav");
    let mut pool = suhina_engine::AudioPool::default();
    pool.files.insert(
        "source.wav".into(),
        Arc::new(suhina_engine::AudioData { sample_rate: 48000, channels: 1, samples: vec![1.; 48000] }),
    );
    let mut renderer = suhina_engine::Renderer::stopped(&project, &pool, 48000).unwrap();
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    let mut session = Session::new(project);
    session.set_audio(pool);
    session.attach_player(suhina_session::Player::new(controller, None));
    session.execute("track.setArm", &json!({"track":1,"armed":true})).unwrap();
    live.send(suhina_engine::LiveEvent::NoteOn { key: 61, velocity: 1. });
    let mut buffer = [0.; 128];
    renderer.process(&mut buffer);
    let heads = || session.player().unwrap().meter_monitor().sample_positions(TrackId(1), 0);
    assert!(heads().into_iter().flatten().any(|p| (p - 0.2).abs() < 0.01));
    session.execute("device.setSampleSlices", &json!({"track":1,"device":0,"points":[0.7]})).unwrap();
    renderer.process(&mut buffer);
    let heads = session.player().unwrap().meter_monitor().sample_positions(TrackId(1), 0);
    assert!(heads.into_iter().flatten().any(|p| (p - 0.2).abs() < 0.01), "map adoption keeps the held voice");
    live.send(suhina_engine::LiveEvent::NoteOn { key: 61, velocity: 1. });
    renderer.process(&mut buffer);
    let heads = session.player().unwrap().meter_monitor().sample_positions(TrackId(1), 0);
    assert!(heads.into_iter().flatten().any(|p| (p - 0.7).abs() < 0.01), "new notes use the edited map");
}
