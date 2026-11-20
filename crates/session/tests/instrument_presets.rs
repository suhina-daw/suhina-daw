// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Preset commands, source identity and relocation use the actual offline renderer.
#![allow(clippy::unwrap_used)] // deterministic test helpers

use super::temp_dir::TempDir;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use suhina_engine::{AudioPool, Renderer};
use suhina_model::*;
use suhina_session::{Player, Session};

fn session() -> Session {
    Session::new(Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Instrument")
        }],
        channels: vec![Channel::new(ChannelId(1), "Instrument")],
        ..Project::default()
    })
}

fn source(path: &Path) {
    let data: Vec<_> = (0..9600).map(|i| (i as f32 * 0.031).sin() * 0.2).collect();
    suhina_test_support::write_wav(path, 48000, 1, suhina_test_support::WavFormat::Int16, &data);
}

fn loaded(library: &Path, sample: Option<&Path>) -> Session {
    let mut session = session();
    session.set_library_base(library.to_path_buf());
    session.execute("device.add", &json!({"track":1,"id":"instrument"})).unwrap();
    if let Some(path) = sample {
        session.execute("device.loadSample", &json!({"track":1,"device":0,"path":path})).unwrap();
    }
    session
        .execute(
            "instrument.adoptModalProfile",
            &json!({"track":1,"device":0,"source":"Hybrid","profile":{
                "referenceKey":60,"referenceHz":263.1,"modes":[
                    {"ratio":1.0,"magnitude":0.7,"decaySeconds":1.2},
                    {"ratio":2.756,"magnitude":0.3,"decaySeconds":0.35}
                ]
            }}),
        )
        .unwrap();
    session
}

fn save(session: &mut Session, collect: bool) -> PathBuf {
    let before = session.project().clone();
    let result =
        session.execute("device.savePreset", &json!({"track":1,"device":0,"name":"Bell","collect":collect})).unwrap();
    assert_eq!(session.project(), &before, "saving changes the library only");
    PathBuf::from(result["preset"].as_str().unwrap())
}

fn render(project: &Project, audio: &AudioPool) -> Vec<f32> {
    let mut renderer = Renderer::stopped(project, audio, 48000, suhina_session::devices()).unwrap();
    let mut controller = renderer.controller();
    let mut live = controller.take_live_input().unwrap();
    let mut session = Session::new(project.clone());
    session.set_audio(audio.clone());
    session.attach_player(Player::new(controller, None));
    session.execute("track.setArm", &json!({"track":1,"armed":true})).unwrap();
    live.send(suhina_engine::LiveEvent::NoteOn { key: 60, velocity: 0.8 });
    let mut output = vec![0.0; 8192];
    for block in output.chunks_mut(256) {
        renderer.process(block);
    }
    output
}

#[test]
fn collected_folder_moves_with_profile_sample_hybrid_and_undo_exact_render() {
    let root = TempDir::new("instrument-collected-preset");
    let original = root.join("original.wav");
    source(&original);
    let library = root.join("library");
    let mut author = loaded(&library, Some(&original));
    for mode in [0, 1, 2] {
        author.execute("device.setParam", &json!({"track":1,"device":0,"param":"source","value":mode})).unwrap();
        let expected = render(author.project(), author.audio());
        assert!(expected.iter().any(|v| v.abs() > 0.001));
        let preset = save(&mut author, true);
        let moved = root.join(format!("moved-{mode}"));
        std::fs::rename(preset.parent().unwrap(), &moved).unwrap();
        let preset = moved.join(preset.file_name().unwrap());
        let mut receiver = session();
        let before = receiver.project().clone();
        receiver.execute("device.add", &json!({"track":1,"preset":preset})).unwrap();
        let added = receiver.project().clone();
        let device = &added.channels[0].devices[0];
        assert!(device.sample_file().unwrap().contains(&format!("moved-{mode}")));
        assert!(device.instrument_source_identity().unwrap().is_some());
        assert_eq!(
            device.instrument_modal_profile(),
            author.project().channels[0].devices[0].instrument_modal_profile()
        );
        assert_eq!(render(&added, receiver.audio()), expected);
        receiver.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(receiver.project(), &before);
        receiver.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(receiver.project(), &added);
        let song = moved.join("song.suhina");
        suhina_project_file::write_file(&song, &added).unwrap();
        let reopened = suhina_project_file::read_file(&song).unwrap();
        let trusted = suhina_media::Trusted::default();
        // The collected source is below this project folder, never an absolute fallback.
        let decoded = suhina_media::decode_project_audio_found(&reopened, &moved, &trusted);
        assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
        assert_eq!(render(&reopened, &decoded.pool), expected);
        let mut new_track = Session::new(Project::default());
        new_track.execute("track.add", &json!({"preset":preset})).unwrap();
        assert_eq!(render(new_track.project(), new_track.audio()), expected);
    }
}

#[test]
fn referenced_source_and_missing_matching_relink_keep_identity() {
    let root = TempDir::new("instrument-reference-preset");
    let original = root.join("own.wav");
    source(&original);
    let mut author = loaded(&root, Some(&original));
    let preset = save(&mut author, false);
    let expected = render(author.project(), author.audio());
    let mut receiver = session();
    receiver.set_library_base(root.to_path_buf());
    receiver.execute("device.add", &json!({"track":1,"preset":preset})).unwrap();
    assert_eq!(render(receiver.project(), receiver.audio()), expected);
    let moved = root.join("renamed.wav");
    std::fs::rename(&original, &moved).unwrap();
    let mut missing = session();
    missing.set_library_base(root.to_path_buf());
    let result = missing.execute("device.add", &json!({"track":1,"preset":preset})).unwrap();
    assert!(result["file_missing"].is_string());
    let before = missing.project().clone();
    let wrong = root.join("wrong.wav");
    suhina_test_support::write_wav(&wrong, 48000, 1, suhina_test_support::WavFormat::Int16, &[0.1; 9600]);
    assert!(missing.execute("project.relinkFiles", &json!({"files":[original],"to":[wrong]})).is_err());
    assert_eq!(missing.project(), &before);
    missing.execute("project.relinkFiles", &json!({"files":[original],"to":[moved]})).unwrap();
    assert_eq!(render(missing.project(), missing.audio()), expected);
    missing.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(missing.project(), &before);
    missing.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(render(missing.project(), missing.audio()), expected);
}

#[test]
fn changed_sources_and_unknown_models_leave_previous_preset_and_project_whole() {
    let root = TempDir::new("instrument-preset-refusals");
    let source_path = root.join("own.wav");
    source(&source_path);
    let mut author = loaded(&root, Some(&source_path));
    let preset = save(&mut author, false);
    let saved = std::fs::read(&preset).unwrap();
    let before = author.project().clone();
    suhina_test_support::write_wav(&source_path, 48000, 1, suhina_test_support::WavFormat::Int16, &[0.1; 9600]);
    assert!(author.execute("device.savePreset", &json!({"track":1,"device":0,"name":"Bell"})).is_err());
    assert_eq!(author.project(), &before);
    assert_eq!(std::fs::read(&preset).unwrap(), saved);
    let mut receiver = session();
    receiver.set_library_base(root.to_path_buf());
    let empty = receiver.project().clone();
    assert!(receiver.execute("device.add", &json!({"track":1,"preset":preset})).is_err());
    assert_eq!(receiver.project(), &empty);
    let decoded = suhina_media::decode_project_audio_found(&before, &root, &suhina_media::Trusted::default());
    assert!(decoded.pool.files.is_empty());
    assert!(!decoded.warnings.is_empty());
    let mut value: Value = serde_json::from_slice(&saved).unwrap();
    value["params"]["model_revision"] = json!(99);
    std::fs::write(&preset, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(receiver.execute("track.add", &json!({"preset":preset})).is_err());
    assert_eq!(receiver.project(), &empty);
}

#[test]
fn profile_only_and_nested_insert_preserve_state_without_a_source() {
    let root = TempDir::new("instrument-profile-only-preset");
    let mut author = loaded(&root, None);
    let preset = save(&mut author, false);
    let expected = render(author.project(), author.audio());
    let mut receiver = session();
    receiver.execute("device.add", &json!({"track":1,"preset":preset})).unwrap();
    assert_eq!(render(receiver.project(), receiver.audio()), expected);
    receiver.execute("device.group", &json!({"track":1,"device":0})).unwrap();
    receiver.execute("device.remove", &json!({"track":1,"device":"0/0/0"})).unwrap();
    receiver.execute("device.add", &json!({"track":1,"position":"0/0/0","preset":preset})).unwrap();
    assert_eq!(render(receiver.project(), receiver.audio()), expected);
}

#[test]
fn outside_sources_require_collection_and_conflicting_assets_do_not_replace_a_saved_preset() {
    let root = TempDir::new("instrument-asset-conflict");
    let sample = root.join("outside.wav");
    source(&sample);
    let library = root.join("library");
    std::fs::create_dir_all(&library).unwrap();
    let mut author = loaded(&library, Some(&sample));
    assert!(
        author
            .execute("device.savePreset", &json!({"track":1,"device":0,"name":"Bell"}))
            .unwrap_err()
            .to_string()
            .contains("outside the library")
    );
    let preset = save(&mut author, true);
    let encoded = std::fs::read(&preset).unwrap();
    let state = suhina_device_registry::preset(&encoded, 1).unwrap().instrument.unwrap();
    let target = preset.parent().unwrap().join(state.sample.unwrap().path);
    std::fs::write(&target, b"conflicting bytes").unwrap();
    assert!(author.execute("device.savePreset", &json!({"track":1,"device":0,"name":"Bell","collect":true})).is_err());
    assert_eq!(std::fs::read(&preset).unwrap(), encoded);
    assert_eq!(std::fs::read(target).unwrap(), b"conflicting bytes");
}
