// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Audio outside a project's folder (`docs/spec/missing-files.md` › Project file boundaries):
//! what the person loaded and saved a project with opens with it again, files in the browser's
//! library folders are read, and anything else a project names outside its folder stays missing.
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use suhina_library::Library;
use suhina_model::*;
use suhina_session::{Projects, Session};
use suhina_test_support::{WavFormat, write_wav};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-trusted-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::path::absolute(dir).unwrap()
}

fn wav(path: &Path) {
    write_wav(path, 8000, 1, WavFormat::Int16, &[8000.0 / 32767.0; 4000]);
}

fn run(projects: &mut Projects, id: &str, params: Value) -> Value {
    projects.execute(id, &params).map_err(String::from).unwrap()
}

#[test]
fn a_saved_projects_own_audio_outside_its_folder_opens_with_it_again() {
    let root = temp("saved");
    let settings = root.join("settings");
    let sample = root.join("samples").join("kick.wav");
    wav(&sample);
    let song = root.join("project").join("song.suhina");
    std::fs::create_dir_all(song.parent().unwrap()).unwrap();

    let plugins = suhina_plugins::library::Library::new(Some(settings.join("plugins")), settings.join("no-host"));
    let mut projects = Projects::with_library(plugins);
    projects.set_file_library(Library::new(Some(settings.clone())));
    let project = Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Keys")
        }],
        channels: vec![Channel::new(ChannelId(1), "Keys")],
        ..Project::default()
    };
    projects.add(Session::new(project), None).unwrap();
    run(&mut projects, "device.add", json!({ "track": 1, "id": "sampler" }));
    run(&mut projects, "device.loadSample", json!({ "track": 1, "device": 0, "path": sample }));
    run(&mut projects, "project.save", json!({ "path": song }));
    assert_eq!(projects.file_library_mut().trusted_audio(), vec![sample.clone()], "the saved sample is trusted");

    // Opened again: outside the folder, so missing by the folder alone, found as trusted.
    let (reopened, _) = Session::open(&song).unwrap();
    let folder = reopened.audio_folder().unwrap().to_path_buf();
    assert_eq!(suhina_media::missing_audio(reopened.project(), &folder).len(), 1);
    let trusted = projects.trusted_audio();
    assert!(suhina_media::missing_audio_trusted(reopened.project(), &folder, &trusted).is_empty());
    let (pool, warnings) = suhina_media::load_project_audio_trusted(reopened.project(), &folder, &trusted);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(pool.files.len(), 1);

    // The trust is kept in the library's file, for the next run.
    let mut again = Library::new(Some(settings));
    assert_eq!(again.trusted_audio(), vec![sample]);
}

#[test]
fn only_trusted_files_and_library_folders_are_read_outside_the_project() {
    let root = temp("rules");
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let library = root.join("library");
    let in_library = library.join("drums").join("snare.wav");
    wav(&in_library);
    let elsewhere = root.join("elsewhere").join("other.wav");
    wav(&elsewhere);
    let locate = |absolute: &Path, trusted: &suhina_media::Trusted| {
        suhina_media::locate("x.wav", Some(absolute.to_str().unwrap()), &project, trusted)
    };

    let none = suhina_media::Trusted::default();
    assert_eq!(locate(&elsewhere, &none), None, "outside the folder and not trusted");
    assert_eq!(locate(&in_library, &none), None);

    let trusted = suhina_media::Trusted {
        folders: vec![library.clone()],
        files: [elsewhere.clone()].into(),
        ..Default::default()
    };
    assert_eq!(locate(&in_library, &trusted), Some(in_library.clone()), "inside a library folder");
    assert_eq!(locate(&elsewhere, &trusted), Some(elsewhere.clone()), "a trusted file");
    // A parent step out of a library folder is not inside it.
    let escape = library.join("..").join("elsewhere").join("other.wav");
    let folder_only = suhina_media::Trusted { folders: vec![library.clone()], ..Default::default() };
    assert_eq!(locate(&escape, &folder_only), None);
    assert_eq!(locate(&elsewhere, &folder_only), None);
    // The relative path a save writes ("../library/...") is found the same way.
    let relative = suhina_media::locate("../library/drums/snare.wav", None, &project, &folder_only);
    assert_eq!(relative, Some(in_library));
    assert_eq!(suhina_media::locate("../elsewhere/other.wav", None, &project, &folder_only), None);
}

/// Projects with a library in `settings` and a project with a sampler that plays `sample`,
/// saved as `song`.
fn saved_with_sample(settings: &Path, sample: &Path, song: &Path) -> Projects {
    let plugins = suhina_plugins::library::Library::new(Some(settings.join("plugins")), settings.join("no-host"));
    let mut projects = Projects::with_library(plugins);
    projects.set_file_library(Library::new(Some(settings.to_path_buf())));
    let project = Project {
        tracks: vec![Track {
            content: ContentKinds { notes: true, audio: false },
            channel: Some(ChannelId(1)),
            ..Track::new(TrackId(1), "Keys")
        }],
        channels: vec![Channel::new(ChannelId(1), "Keys")],
        ..Project::default()
    };
    projects.add(Session::new(project), None).unwrap();
    run(&mut projects, "device.add", json!({ "track": 1, "id": "sampler" }));
    run(&mut projects, "device.loadSample", json!({ "track": 1, "device": 0, "path": sample }));
    std::fs::create_dir_all(song.parent().unwrap()).unwrap();
    run(&mut projects, "project.save", json!({ "path": song }));
    projects
}

/// The folder a sample was dropped from is kept for its project, on this machine, and the
/// missing files search looks there: a sample moved inside it is found by name.
#[test]
fn the_folder_a_sample_came_from_is_searched_when_it_moves() {
    let root = temp("folders");
    let settings = root.join("settings");
    let samples = root.join("samples");
    let sample = samples.join("kick.wav");
    wav(&sample);
    let song = root.join("project").join("song.suhina");
    let mut projects = saved_with_sample(&settings, &sample, &song);
    assert_eq!(projects.file_library_mut().project_folders(&song), vec![samples.clone()]);

    // Moved inside the folder: missing, and found there by name.
    let moved = samples.join("one-shots").join("kick.wav");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::rename(&sample, &moved).unwrap();
    let (reopened, _) = Session::open(&song).unwrap();
    let id = projects.add(reopened, None).unwrap();
    assert_eq!(projects.get(id).unwrap().missing_files().len(), 1);
    let found = run(&mut projects, "library.findFiles", json!({ "names": ["kick.wav"] }));
    let paths: Vec<&str> = found["found"][0]["paths"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(paths.contains(&moved.display().to_string().as_str()), "found in the folder it came from: {paths:?}");
}

/// Collect and save: the audio from outside the project's folder is copied into
/// Samples/Collected there, the project plays the copies, and it opens with the folder alone.
#[test]
fn collect_copies_the_audio_from_outside_into_the_project() {
    let root = temp("collect");
    let settings = root.join("settings");
    let sample = root.join("samples").join("kick.wav");
    wav(&sample);
    let song = root.join("project").join("song.suhina");
    let mut projects = saved_with_sample(&settings, &sample, &song);

    let collected = run(&mut projects, "project.collect", json!({}));
    let copy = song.parent().unwrap().join("Samples").join("Collected").join("kick.wav");
    assert!(copy.is_file(), "{collected}");
    assert_eq!(collected["collected"].as_array().unwrap().len(), 1);
    let active = projects.active().unwrap();
    let file = projects.get(active).unwrap().project().channels[0].devices[0].sample_file().unwrap().to_string();
    assert_eq!(file, copy.display().to_string());
    // Nothing left outside: collecting again copies nothing.
    assert_eq!(run(&mut projects, "project.collect", json!({}))["collected"].as_array().unwrap().len(), 0);
    run(&mut projects, "project.save", json!({}));

    let (reopened, _) = Session::open(&song).unwrap();
    let folder = reopened.audio_folder().unwrap().to_path_buf();
    assert!(suhina_media::missing_audio(reopened.project(), &folder).is_empty(), "inside the folder, no trust needed");
    // One undo step puts the original back.
    run(&mut projects, "edit.undo", json!({}));
    let back = projects.get(active).unwrap().project().channels[0].devices[0].sample_file().unwrap().to_string();
    assert_ne!(back, copy.display().to_string());
}

/// Two kits' `kick.wav`, of one size and other sounds, collected in one run: each gets a copy of
/// its own, so after the collect each sampler still plays its own kick.
#[test]
fn collect_gives_two_files_of_one_name_and_size_a_copy_each() {
    let root = temp("collect-two");
    let settings = root.join("settings");
    let (a, b) = (root.join("KitA").join("kick.wav"), root.join("KitB").join("kick.wav"));
    wav(&a);
    wav(&b);
    // The same size, other samples.
    let mut bytes = std::fs::read(&b).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    std::fs::write(&b, bytes).unwrap();
    let song = root.join("project").join("song.suhina");
    let mut projects = saved_with_sample(&settings, &a, &song);
    run(&mut projects, "track.add", json!({ "kind": "midi", "device": "sampler" }));
    run(&mut projects, "device.loadSample", json!({ "track": 2, "device": 0, "path": b }));

    let collected = run(&mut projects, "project.collect", json!({}));
    assert_eq!(collected["collected"].as_array().unwrap().len(), 2, "{collected}");
    let active = projects.active().unwrap();
    let files: Vec<String> = projects.get(active).unwrap().project().channels[..2]
        .iter()
        .map(|c| c.devices[0].sample_file().unwrap().to_string())
        .collect();
    let into = song.parent().unwrap().join("Samples").join("Collected");
    assert_eq!(files, [into.join("kick.wav").display().to_string(), into.join("kick (2).wav").display().to_string()]);
    for (file, original) in files.iter().zip([&a, &b]) {
        assert_eq!(std::fs::read(file).unwrap(), std::fs::read(original).unwrap(), "{file}");
    }
}
