// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The local sample-to-instrument path runs through the same commands and renderer as playback.
#![allow(clippy::unwrap_used)] // deterministic test helpers

use serde_json::json;
use suhina_engine::{AudioPool, Renderer};
use suhina_model::*;
use suhina_session::{OpenOptions, Player, Projects, Session};

fn project() -> Project {
    Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Instrument")
        }],
        channels: vec![Channel::new(ChannelId(1), "Instrument")],
        ..Project::default()
    }
}

fn render(project: &Project, audio: &AudioPool) -> Vec<f32> {
    render_with_transport(project, audio, false)
}

fn render_with_transport(project: &Project, audio: &AudioPool, playing: bool) -> Vec<f32> {
    let mut renderer = Renderer::stopped(project, audio, 48000, suhina_session::devices()).unwrap();
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    let mut session = Session::new(project.clone());
    session.set_audio(audio.clone());
    session.attach_player(Player::new(controller, None));
    session.execute("track.setArm", &json!({"track":1,"armed":true})).unwrap();
    if playing {
        session.execute("transport.play", &json!({})).unwrap();
    }
    live.send(suhina_engine::LiveEvent::NoteOn { key: 60, velocity: 0.8 });
    let mut out = vec![0.0; 8192];
    for block in out.chunks_mut(256) {
        renderer.process(block);
    }
    out
}

#[test]
fn importing_a_sample_overrides_source_automation_as_one_undoable_edit() {
    let path = std::env::temp_dir().join(format!("suhina-instrument-automation-{}.wav", std::process::id()));
    suhina_test_support::write_wav(&path, 48000, 1, suhina_test_support::WavFormat::Int16, &[0.2; 9600]);
    let mut session = Session::new(project());
    session.execute("device.add", &json!({"track":1,"id":"instrument"})).unwrap();
    for beat in [0.0, 0.01] {
        session
            .execute("automation.addPoint", &json!({"track":1,"device":0,"param":"source","beat":beat,"value":0}))
            .unwrap();
    }
    let before = session.project().clone();
    let answer = session.execute("device.loadSample", &json!({"track":1,"device":0,"path":path})).unwrap();
    let imported = session.project().clone();
    let mut reference = imported.clone();
    reference.channels[0].devices[0].automation.clear();
    for playing in [false, true] {
        assert!(
            render_with_transport(&imported, session.audio(), playing)
                == render_with_transport(&reference, session.audio(), playing),
            "import must select audible Sample at graph initialization and during playback"
        );
    }
    assert_eq!(answer["automation"], "off");
    let lane = &imported.channels[0].devices[0].automation[0];
    assert!(!lane.enabled);
    assert_eq!(lane.points, before.channels[0].devices[0].automation[0].points);
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(session.project(), &before, "one undo restores source, reference and automation");
    session.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(session.project(), &imported);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn instrument_import_undo_save_reopen_relink_and_missing_hybrid_are_a_single_lifecycle() {
    let folder = std::env::temp_dir().join(format!("suhina-instrument-lifecycle-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("authored-source.wav");
    let samples: Vec<f32> = (0..9600).map(|i| (i as f32 * 0.031).sin() * 0.2).collect();
    suhina_test_support::write_wav(&path, 48000, 1, suhina_test_support::WavFormat::Int16, &samples);
    let mut session = Session::new(project());
    session.execute("device.add", &json!({"track":1,"id":"instrument"})).unwrap();
    let before = session.project().clone();
    assert_eq!(before.channels[0].devices[0].params.iter().find(|p| p.id == "model_revision").unwrap().value, 1.0);
    session.execute("device.loadSample", &json!({"track":1,"device":0,"path":path})).unwrap();
    let imported = session.project().clone();
    assert_eq!(imported.channels[0].devices[0].params.iter().find(|p| p.id == "source").unwrap().value, 1.0);
    let expected = render(&imported, session.audio());
    assert!(expected.iter().any(|value| value.abs() > 0.001));
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(session.project(), &before, "one undo restores both Source and file reference");
    session.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(session.project(), &imported);
    assert!(
        session.execute("device.loadSample", &json!({"track":1,"device":0,"path":folder.join("absent.wav")})).is_err()
    );
    assert_eq!(session.project(), &imported, "failed import is atomic");

    let plugin_library = suhina_plugins::library::Library::new(Some(folder.join("plugins")), folder.join("no-host"));
    let mut projects = Projects::with_library(plugin_library);
    projects.set_file_library(suhina_library::Library::new(Some(folder.join("settings"))));
    projects.add(session, None).unwrap();
    let song = folder.join("instrument.suhina");
    projects.execute("project.save", &json!({"path":song})).unwrap();
    let stored = suhina_project_file::read_file(&song).unwrap();
    assert_eq!(stored.channels[0].devices[0].sample_file(), Some("authored-source.wav"));
    let (mut reopened, warnings) = projects.open_path(&song, OpenOptions { audio: true }).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        render(reopened.project(), reopened.audio()),
        expected,
        "reopen restores the prepared sound without fitting"
    );

    let moved = folder.join("renamed-source.wav");
    std::fs::rename(&path, &moved).unwrap();
    assert_eq!(suhina_media::missing_audio(reopened.project(), &folder).len(), 1);
    reopened.execute("project.relinkFiles", &json!({"files":["authored-source.wav"],"to":[moved]})).unwrap();
    assert!(suhina_media::missing_audio(reopened.project(), &folder).is_empty());
    assert_eq!(render(reopened.project(), reopened.audio()), expected);
    reopened.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(suhina_media::missing_audio(reopened.project(), &folder).len(), 1);

    reopened.execute("device.setParam", &json!({"track":1,"device":0,"param":"source","value":2})).unwrap();
    let (empty_pool, _) = suhina_media::load_project_audio(reopened.project(), &folder);
    assert!(empty_pool.files.is_empty());
    assert!(
        render(reopened.project(), &empty_pool).iter().any(|value| value.abs() > 0.001),
        "missing sample leaves Hybrid modal layer playable"
    );
    reopened.execute("device.setParam", &json!({"track":1,"device":0,"param":"source","value":1})).unwrap();
    assert!(render(reopened.project(), &empty_pool).iter().all(|value| *value == 0.0));
    std::fs::remove_file(moved).unwrap();
    std::fs::remove_file(song).unwrap();
    // Project save may keep a settings/backup directory; this test owns its complete temp root.
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn source_references_are_versioned_and_invalid_bytes_never_become_paths() {
    let mut instrument = Device::new("Instrument", DeviceKind::Builtin("instrument".into()));
    for bytes in [b"not a sample".to_vec(), b"suhina-sample-v2\npath".to_vec(), vec![255; 4]] {
        instrument.state = Some(bytes.into());
        assert!(instrument.sample_file().is_none());
    }
    instrument.set_sample_reference("source.wav", "/original/source.wav");
    assert_eq!(instrument.sample_file(), Some("source.wav"));
    assert_eq!(instrument.sample_absolute_file(), Some("/original/source.wav"));
}
