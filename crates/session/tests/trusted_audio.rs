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

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-trusted-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::path::absolute(dir).unwrap()
}

fn wav(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let spec =
        hound::WavSpec { channels: 1, sample_rate: 8000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..4000 {
        writer.write_sample(8000i16).unwrap();
    }
    writer.finalize().unwrap();
}

fn run(projects: &mut Projects, id: &str, params: Value) -> Value {
    projects.execute(id, &params).map_err(|e| e.0).unwrap()
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

    let trusted = suhina_media::Trusted { folders: vec![library.clone()], files: [elsewhere.clone()].into() };
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
