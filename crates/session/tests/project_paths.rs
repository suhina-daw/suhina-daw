// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The paths a project names are used only where its audio was decoded from (#617,
//! `docs/spec/missing-files.md` › Project file boundaries): saving and backups write the file that
//! was read, Collect and save copies only that file, and a NAM device's file is read only from
//! where the project's audio may be.
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use suhina_engine::Renderer;
use suhina_library::Library;
use suhina_model::*;
use suhina_session::{OpenOptions, Player, Projects, Session};
use suhina_test_support::{WavFormat, write_wav};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-paths-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::path::absolute(dir).unwrap()
}

fn wav(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    write_wav(path, 8000, 1, WavFormat::Int16, &[8000.0 / 32767.0; 4000]);
}

fn run(projects: &mut Projects, id: &str, params: Value) -> Value {
    projects.execute(id, &params).map_err(String::from).unwrap()
}

/// A network share as the platform writes one.
fn share(name: &str) -> String {
    if cfg!(windows) { format!(r"\\server\share\{name}") } else { format!("//server/share/{name}") }
}

/// A project file in `folder` with one Sampler naming `file`, and `absolute` as its absolute path.
fn sampler_song(folder: &Path, file: &str, absolute: &str) -> PathBuf {
    let mut sampler = Device::new("Sampler", DeviceKind::Builtin("sampler".into()));
    sampler.set_sample_reference(file, absolute);
    let mut channel = Channel::new(ChannelId(1), "Keys");
    channel.devices.push(sampler);
    let project = Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Keys")
        }],
        channels: vec![channel],
        ..Project::default()
    };
    let song = folder.join("song.suhina");
    std::fs::create_dir_all(folder).unwrap();
    suhina_project_file::write_file(&song, &project).unwrap();
    song
}

fn projects(settings: &Path) -> Projects {
    let plugins = suhina_plugins::library::Library::new(Some(settings.join("plugins")), settings.join("no-host"));
    let mut projects = Projects::with_library(plugins);
    projects.set_file_library(Library::new(Some(settings.to_path_buf())));
    projects
}

fn opened(projects: &mut Projects, song: &Path) {
    let (session, warnings) = projects.open_path(song, OpenOptions { audio: true }).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    projects.add(session, None).unwrap();
}

fn saved_sample(song: &Path) -> (String, String) {
    let project = suhina_project_file::read_file(song).unwrap();
    let device = &project.channels[0].devices[0];
    (device.sample_file().unwrap().to_string(), device.sample_absolute_file().unwrap().to_string())
}

/// The sample was decoded from the project's folder, though its absolute path names a share:
/// saved elsewhere, the project points at the file that was read, and the share is gone from it.
#[test]
fn saving_writes_the_file_that_was_decoded() {
    let root = temp("decoded");
    let kick = root.join("project").join("kick.wav");
    wav(&kick);
    let song = sampler_song(&root.join("project"), "kick.wav", &share("kick.wav"));
    let mut projects = projects(&root.join("settings"));
    opened(&mut projects, &song);
    let moved = root.join("elsewhere").join("song.suhina");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    run(&mut projects, "project.save", json!({ "path": moved }));
    let (file, absolute) = saved_sample(&moved);
    assert_eq!(file, "../project/kick.wav");
    assert_eq!(Path::new(&absolute), kick);
}

/// Nothing was decoded from a share or a path outside the folder: saving moves the relative path
/// by name and keeps the absolute path as the project had it.
#[test]
fn saving_keeps_a_path_nothing_was_read_from_as_it_was() {
    for (n, (file, absolute)) in [
        ("kick.wav".to_string(), share("kick.wav")),
        (
            "kick.wav".to_string(),
            if cfg!(windows) { r"\\?\C:\nowhere\kick.wav".into() } else { "/nowhere/kick.wav".into() },
        ),
        ("../../kick.wav".to_string(), "../../kick.wav".to_string()),
    ]
    .into_iter()
    .enumerate()
    {
        let root = temp(&format!("kept-{n}"));
        let song = sampler_song(&root.join("project"), &file, &absolute);
        let mut projects = projects(&root.join("settings"));
        let (session, _) = projects.open_path(&song, OpenOptions { audio: true }).unwrap();
        projects.add(session, None).unwrap();
        run(&mut projects, "project.save", json!({}));
        let (_, kept) = saved_sample(&song);
        assert_eq!(kept, absolute, "{file}");
        // Nothing read, so nothing becomes trusted either.
        assert!(projects.file_library_mut().trusted_audio().is_empty(), "{file}");
    }
}

/// The project names a real sample in its folder and, as its absolute path, another file on the
/// disk: the sample is what was decoded, so Collect and save copies nothing and saving trusts
/// only the sample.
#[test]
fn collect_and_trust_use_the_file_that_was_decoded() {
    let root = temp("collect");
    let kick = root.join("project").join("kick.wav");
    wav(&kick);
    let secret = root.join("private").join("secret.wav");
    wav(&secret);
    let song = sampler_song(&root.join("project"), "kick.wav", &secret.display().to_string());
    let mut projects = projects(&root.join("settings"));
    projects.read_local_audio(true);
    opened(&mut projects, &song);
    let active = projects.active().unwrap();
    assert_eq!(projects.get(active).unwrap().loaded_audio_paths(), vec![kick.clone()]);
    let collected = run(&mut projects, "project.collect", json!({}));
    assert_eq!(collected["collected"], json!([]), "{collected}");
    assert!(!root.join("project").join("Samples").exists(), "nothing was copied");
    run(&mut projects, "project.save", json!({}));
    let trusted = projects.file_library_mut().trusted_audio();
    assert!(!trusted.contains(&secret), "the other file is not trusted: {trusted:?}");
    assert!(projects.file_library_mut().project_folders(&song).is_empty(), "no folder outside it is kept");
}

/// A project file in `folder` with a NAM Amp Modeler whose state names `path`.
fn nam_song(folder: &Path, path: &str) -> PathBuf {
    let mut channel = Channel::new(ChannelId(1), "Guitar");
    channel.devices.push(Device {
        state: Some(path.as_bytes().to_vec().into()),
        ..Device::new("Amp", DeviceKind::Builtin("nam-amp-modeler".into()))
    });
    let project = Project {
        tracks: vec![Track { channel: Some(ChannelId(1)), ..Track::new(TrackId(1), "Guitar") }],
        channels: vec![channel],
        ..Project::default()
    };
    let song = folder.join("amp.suhina");
    std::fs::create_dir_all(folder).unwrap();
    suhina_project_file::write_file(&song, &project).unwrap();
    song
}

/// A NAM model a project names is read only from where its audio may be: not from a share or
/// another folder on opening, not when it is not a plain file, and once it is trusted.
#[test]
fn a_nam_file_is_read_only_from_where_the_projects_audio_may_be() {
    let root = temp("nam");
    let outside = root.join("models").join("amp.nam");
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    std::fs::write(&outside, "{not json").unwrap();
    for path in [share("amp.nam"), outside.display().to_string()] {
        let song = nam_song(&root.join("project"), &path);
        let (session, _) = Session::open(&song).unwrap();
        assert!(session.device_asset(&path).is_none(), "{path}");
        let note = session.device_asset_note(&path).unwrap_or_default();
        assert!(note.contains("outside the project's folder"), "{path}: {note}");
    }

    // Trusted, it is read: the note is now the read's own.
    let song = nam_song(&root.join("project"), &outside.display().to_string());
    let (mut session, _) = Session::open(&song).unwrap();
    session.set_trusted(suhina_media::Trusted { files: [outside.clone()].into(), ..Default::default() });
    let note = session.device_asset_note(&outside.display().to_string()).unwrap_or_default();
    assert!(note.contains("not a NAM model"), "{note}");

    // In the project's folder but not a plain file (a folder here, a named pipe elsewhere): not
    // read, so it cannot block the session.
    let folder = root.join("project").join("folder.nam");
    std::fs::create_dir_all(&folder).unwrap();
    let song = nam_song(&root.join("project"), &folder.display().to_string());
    let (session, _) = Session::open(&song).unwrap();
    assert!(session.device_asset_note(&folder.display().to_string()).is_some());
    assert!(session.device_asset(&folder.display().to_string()).is_none());

    // Inside the project's folder it is read on opening, with no trust needed.
    let inside = root.join("project").join("inside.nam");
    std::fs::write(&inside, "{not json").unwrap();
    let song = nam_song(&root.join("project"), &inside.display().to_string());
    let (session, _) = Session::open(&song).unwrap();
    let note = session.device_asset_note(&inside.display().to_string()).unwrap_or_default();
    assert!(note.contains("not a NAM model"), "{note}");
}

/// The sample was decoded from a trusted file outside the project's folder. A file of its name
/// turning up in the folder afterwards changes nothing: saving, Collect and save and the trust a
/// save keeps use the file that was decoded, not the one found at its path now.
#[test]
fn a_file_turning_up_after_opening_does_not_replace_the_one_decoded() {
    let root = temp("later");
    let outside = root.join("samples").join("kick.wav");
    wav(&outside);
    let song = sampler_song(&root.join("project"), "kick.wav", &outside.display().to_string());
    let mut projects = projects(&root.join("settings"));
    projects.read_local_audio(true);
    opened(&mut projects, &song);
    wav(&root.join("project").join("kick.wav"));
    let active = projects.active().unwrap();
    assert_eq!(projects.get(active).unwrap().loaded_audio_paths(), vec![outside.clone()]);
    run(&mut projects, "project.save", json!({}));
    let (_, absolute) = saved_sample(&song);
    assert_eq!(Path::new(&absolute), outside);
    let collected = run(&mut projects, "project.collect", json!({}));
    assert_ne!(collected["collected"], json!([]), "the outside file is collected: {collected}");
}

/// A file in the project's folder that does not decode was read from nowhere: saving keeps the
/// absolute path the project names instead of writing that file's.
#[test]
fn a_file_that_does_not_decode_keeps_the_projects_absolute_path() {
    let root = temp("undecoded");
    let broken = root.join("project").join("kick.wav");
    std::fs::create_dir_all(broken.parent().unwrap()).unwrap();
    std::fs::write(&broken, b"not audio").unwrap();
    let absolute = share("kick.wav");
    let song = sampler_song(&root.join("project"), "kick.wav", &absolute);
    let mut projects = projects(&root.join("settings"));
    let (session, warnings) = projects.open_path(&song, OpenOptions { audio: true }).unwrap();
    assert!(!warnings.is_empty(), "the file does not decode");
    projects.add(session, None).unwrap();
    run(&mut projects, "project.save", json!({}));
    assert_eq!(saved_sample(&song).1, absolute);
}

/// A relative path a project names for a NAM model is read from the project's folder, where it
/// was allowed, never from the working folder.
#[test]
fn a_relative_nam_path_is_read_from_the_projects_folder() {
    let root = temp("relative-nam");
    std::fs::create_dir_all(root.join("project")).unwrap();
    std::fs::write(root.join("project").join("relative-amp.nam"), "{not json").unwrap();
    let song = nam_song(&root.join("project"), "relative-amp.nam");
    let (session, _) = Session::open(&song).unwrap();
    let note = session.device_asset_note("relative-amp.nam").unwrap_or_default();
    assert!(note.contains("not a NAM model"), "{note}");
}

/// A minimal WaveNet model, the fixture the Amp Modeler's own tests run on.
const MODEL: &str = r#"{
    "version": "0.5.4", "architecture": "WaveNet",
    "config": {
        "layers": [{
            "input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
            "kernel_size": 1, "dilations": [1], "activation": "ReLU",
            "gated": false, "head_bias": false
        }],
        "head": null, "head_scale": 1.0
    },
    "weights": [1.0, 2.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0],
    "metadata": { "name": "Test amp 1965" }
}"#;

/// A model at `path`, as an absolute path's text.
fn model(path: &Path) -> String {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, MODEL).unwrap();
    path.display().to_string()
}

/// The project names a model outside where it may be read. Chosen again with the same path, it is
/// read then, though the project does not change. Another model chosen after it is what saving
/// trusts: the first one no device names any more is not.
#[test]
fn a_nam_file_chosen_again_is_read_and_only_a_named_one_is_trusted() {
    let root = temp("chosen");
    let first = model(&root.join("models").join("first.nam"));
    let second = model(&root.join("models").join("second.nam"));
    let song = nam_song(&root.join("project"), &first);
    let mut projects = projects(&root.join("settings"));
    let (session, _) = projects.open_path(&song, OpenOptions { audio: true }).unwrap();
    let active = projects.add(session, None).unwrap();
    assert!(projects.get(active).unwrap().device_asset(&first).is_none());
    run(&mut projects, "device.setNamFile", json!({ "track": 1, "device": 0, "path": &first }));
    assert!(projects.get(active).unwrap().device_asset(&first).is_some(), "read without an edit");
    run(&mut projects, "device.setNamFile", json!({ "track": 1, "device": 0, "path": &second }));
    run(&mut projects, "project.save", json!({}));
    let trusted = projects.file_library_mut().trusted_audio();
    assert!(trusted.contains(&PathBuf::from(&second)), "{trusted:?}");
    assert!(!trusted.contains(&PathBuf::from(&first)), "{trusted:?}");
}

/// A model the person chose in one project runs where its device is pasted in another, which
/// does not trust its folder.
#[test]
fn a_nam_file_chosen_in_one_project_runs_where_its_device_is_pasted() {
    let root = temp("pasted");
    let path = model(&root.join("models").join("amp.nam"));
    let mut projects = projects(&root.join("settings"));
    let (one, _) = projects.open_path(&nam_song(&root.join("one"), &path), OpenOptions { audio: true }).unwrap();
    let one = projects.add(one, None).unwrap();
    let other = share("other.nam");
    let (two, _) = projects.open_path(&nam_song(&root.join("two"), &other), OpenOptions { audio: true }).unwrap();
    let two = projects.add(two, None).unwrap();
    projects.switch(one).unwrap();
    run(&mut projects, "device.setNamFile", json!({ "track": 1, "device": 0, "path": &path }));
    run(&mut projects, "device.copy", json!({ "track": 1, "devices": [0] }));
    projects.switch(two).unwrap();
    assert!(projects.get(two).unwrap().device_asset(&path).is_none());
    run(&mut projects, "device.paste", json!({ "track": 1 }));
    assert!(projects.get(two).unwrap().device_asset(&path).is_some());
}

/// Saving trusts a model the project named outside where it could be read: a playing engine runs
/// it from then on, without waiting for an edit.
#[test]
fn a_model_a_new_trust_reads_reaches_the_playing_engine() {
    let root = temp("playing");
    let path = model(&root.join("models").join("amp.nam"));
    let song = nam_song(&root.join("project"), &path);
    let (mut session, _) = Session::open(&song).unwrap();
    let mut renderer =
        Renderer::stopped(session.project(), &AudioPool::new(), 48_000, suhina_session::devices()).unwrap();
    session.attach_player(Player::new(renderer.controller(), None));
    assert!(session.player().unwrap().device_assets().get(&path).is_none());
    session.set_trusted(suhina_media::Trusted { files: [PathBuf::from(&path)].into(), ..Default::default() });
    assert!(session.device_asset(&path).is_some());
    assert!(session.player().unwrap().device_assets().get(&path).is_some());
}

/// A session made from a project another one opened reads its devices' files as that one does,
/// from its folder, not from the working folder (`suhina-cli play --launcher`).
#[test]
fn a_session_given_another_ones_file_access_reads_its_devices_files_from_there() {
    let root = temp("access");
    model(&root.join("project").join("access-amp.nam"));
    let song = nam_song(&root.join("project"), "access-amp.nam");
    let (opened, _) = Session::open(&song).unwrap();
    assert!(opened.device_asset("access-amp.nam").is_some());
    let mut copy = Session::new(opened.project().clone());
    copy.set_file_access(opened.file_access());
    assert!(copy.device_asset("access-amp.nam").is_some());
}
