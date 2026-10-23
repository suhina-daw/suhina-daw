// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Missing files (`docs/spec/missing-files.md`): `project.missingFiles`, `library.findFiles` and
//! `project.relinkFiles`.
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use suhina_library::Library;
use suhina_session::{Projects, Session, blank_project};

use crate::support::run;
use suhina_test_support::{WavFormat, write_wav};

fn temp(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("suhina-relink-{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn wav(path: &Path) {
    let ramp: Vec<f32> = (0..4000).map(|i| ((i % 100) * 100) as f32 / 32767.0).collect();
    write_wav(path, 8000, 1, WavFormat::Int16, &ramp);
}

#[test]
fn missing_files_are_listed_found_and_relinked_in_one_undo_step() {
    let settings = temp("settings");
    let old = temp("old");
    let library_folder = temp("library");
    let kick = old.join("Drums").join("kick.wav");
    let snare = old.join("snare.wav");
    wav(&kick);
    wav(&snare);

    let plugins = suhina_plugins::library::Library::new(Some(settings.join("plugins")), settings.join("no-host"));
    let mut p = Projects::with_library(plugins);
    p.set_file_library(Library::new(Some(settings.clone())));
    p.add(Session::new(blank_project()), None).unwrap();
    let paths = json!([kick.display().to_string(), snare.display().to_string()]);
    run(&mut p, "clip.addFiles", json!({ "paths": paths, "start": 0.0 })).unwrap();
    assert_eq!(run(&mut p, "project.missingFiles", json!({})).unwrap()["files"], json!([]));

    // The samples move: the kick to the library (in a folder of the same name), the snare away.
    let moved_kick = library_folder.join("Drums").join("kick.wav");
    std::fs::create_dir_all(moved_kick.parent().unwrap()).unwrap();
    std::fs::rename(&kick, &moved_kick).unwrap();
    wav(&library_folder.join("Other").join("kick.wav"));
    let moved_snare = temp("elsewhere").join("Snare.WAV");
    std::fs::rename(&snare, &moved_snare).unwrap();

    let missing = run(&mut p, "project.missingFiles", json!({})).unwrap();
    let files = missing["files"].as_array().unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0]["clips"], 1);
    assert!(files[0]["lastPath"].as_str().unwrap().ends_with("kick.wav"));

    // The browser's index finds the kick, the one in a Drums folder first.
    run(&mut p, "library.addFolder", json!({ "folder": library_folder.display().to_string() })).unwrap();
    let found = run(&mut p, "library.findFiles", json!({ "names": ["kick.wav", "snare.wav"] })).unwrap();
    let kick_paths = found["found"][0]["paths"].as_array().unwrap();
    assert_eq!(kick_paths.len(), 2);
    assert_eq!(kick_paths[0], json!(moved_kick.display().to_string()));
    assert_eq!(found["found"][1]["paths"], json!([]));
    // A folder walk finds the snare, whatever the case of its name.
    let walked = run(
        &mut p,
        "library.findFiles",
        json!({ "names": ["snare.wav"], "folder": moved_snare.parent().unwrap().display().to_string() }),
    )
    .unwrap();
    assert_eq!(walked["found"][0]["paths"], json!([moved_snare.display().to_string()]));

    let names: Vec<Value> = files.iter().map(|f| f["file"].clone()).collect();
    let to = json!([moved_kick.display().to_string(), moved_snare.display().to_string()]);
    let relinked = run(&mut p, "project.relinkFiles", json!({ "files": names, "to": to })).unwrap();
    assert_eq!(relinked["relinked"].as_array().unwrap().len(), 2);
    assert_eq!(run(&mut p, "project.missingFiles", json!({})).unwrap()["files"], json!([]));
    // The relinked audio is decoded: the project's pool holds it.
    let session = p.session_mut(None).unwrap();
    assert!(session.audio().files.contains_key(&moved_kick.display().to_string()));

    // One undo step brings both back to missing.
    run(&mut p, "edit.undo", json!({})).unwrap();
    assert_eq!(run(&mut p, "project.missingFiles", json!({})).unwrap()["files"].as_array().unwrap().len(), 2);

    // Bad requests change nothing.
    assert!(run(&mut p, "project.relinkFiles", json!({ "files": names, "to": [] })).is_err());
    assert!(
        run(
            &mut p,
            "project.relinkFiles",
            json!({ "files": ["nothing.wav"], "to": [moved_kick.display().to_string()] })
        )
        .is_err()
    );
    let not_audio = library_folder.join("notes.wav");
    std::fs::write(&not_audio, b"not audio").unwrap();
    assert!(
        run(
            &mut p,
            "project.relinkFiles",
            json!({ "files": [names[0].clone()], "to": [not_audio.display().to_string()] })
        )
        .is_err()
    );
    assert!(
        run(
            &mut p,
            "project.relinkFiles",
            json!({ "files": [names[0].clone()], "to": [old.join("gone.wav").display().to_string()] })
        )
        .is_err()
    );
    assert_eq!(run(&mut p, "project.missingFiles", json!({})).unwrap()["files"].as_array().unwrap().len(), 2);
    for dir in [settings, old, library_folder] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn candidates_from_the_index_and_the_project_folder_are_capped_together() {
    let settings = temp("cap-settings");
    let library_folder = temp("cap-library");
    let project_folder = temp("cap-project");
    for i in 0..40 {
        wav(&library_folder.join(format!("pack{i:02}")).join("kick.wav"));
        wav(&project_folder.join(format!("take{i:02}")).join("kick.wav"));
    }
    let song = project_folder.join("song.suhina");
    Session::new(blank_project()).save(&song, false).unwrap();
    let (session, _) = Session::open(&song).unwrap();

    let plugins = suhina_plugins::library::Library::new(Some(settings.join("plugins")), settings.join("no-host"));
    let mut p = Projects::with_library(plugins);
    p.set_file_library(Library::new(Some(settings.clone())));
    p.add(session, None).unwrap();
    run(&mut p, "library.addFolder", json!({ "folder": library_folder.display().to_string() })).unwrap();

    let found = run(&mut p, "library.findFiles", json!({ "names": ["kick.wav"] })).unwrap();
    let paths: Vec<&str> = found["found"][0]["paths"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert_eq!(paths.len(), suhina_library::find::MAX_CANDIDATES);
    let unique: std::collections::BTreeSet<&str> = paths.iter().copied().collect();
    assert_eq!(unique.len(), paths.len(), "no candidate twice");
    for dir in [&settings, &library_folder, &project_folder] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[test]
fn external_project_sources_require_explicit_relink_after_reopening() {
    let folder = temp("external-project");
    let outside = temp("external-source");
    let audio = outside.join("tone.wav");
    wav(&audio);
    let mut session = Session::new(blank_project());
    session.execute("clip.addFiles", &json!({"paths":[audio],"start":0.})).unwrap();
    assert!(session.missing_files().is_empty());
    let song = folder.join("song.suhina");
    session.save(&song, false).unwrap();
    let (mut reopened, _) = Session::open(&song).unwrap();
    assert!(reopened.audio().files.is_empty());
    let missing = reopened.missing_files();
    assert_eq!(missing.len(), 1);
    reopened.execute("project.relinkFiles", &json!({"files":[missing[0].file],"to":[audio]})).unwrap();
    assert!(reopened.missing_files().is_empty());
    assert!(!reopened.audio().files.is_empty());
    let _ = std::fs::remove_dir_all(folder);
    let _ = std::fs::remove_dir_all(outside);
}

/// A file relinked from outside the project's folder and saved is trusted: the project opens
/// with it again, not missing (`docs/spec/missing-files.md` › Relinking).
#[test]
fn a_file_relinked_from_outside_and_saved_opens_with_the_project_again() {
    let settings = temp("relink-save-settings");
    let folder = temp("relink-save-project");
    let outside = temp("relink-save-source");
    let moved = temp("relink-save-moved");
    let audio = outside.join("tone.wav");
    wav(&audio);
    let song = folder.join("song.suhina");
    let mut session = Session::new(blank_project());
    session.execute("clip.addFiles", &json!({ "paths": [audio], "start": 0.0 })).unwrap();
    session.save(&song, false).unwrap();
    // The file moves, to a folder neither the project nor the library knows.
    let relinked = moved.join("tone.wav");
    std::fs::rename(&audio, &relinked).unwrap();

    let plugins = suhina_plugins::library::Library::new(Some(settings.join("plugins")), settings.join("no-host"));
    let mut p = Projects::with_library(plugins);
    p.set_file_library(Library::new(Some(settings.clone())));
    let (reopened, _) = Session::open(&song).unwrap();
    let id = p.add(reopened, None).unwrap();
    let missing = p.get(id).unwrap().missing_files();
    assert_eq!(missing.len(), 1);
    run(&mut p, "project.relinkFiles", json!({ "files": [missing[0].file], "to": [relinked] })).unwrap();
    assert!(!p.file_library_mut().trusted_audio().contains(&relinked), "trusted by the save, not the relink");
    run(&mut p, "project.save", json!({})).unwrap();
    assert!(p.file_library_mut().trusted_audio().contains(&relinked));

    // Opened again, in a new run with the same settings: found as trusted and decoded.
    let plugins = suhina_plugins::library::Library::new(Some(settings.join("plugins")), settings.join("no-host"));
    let mut again = Projects::with_library(plugins);
    again.set_file_library(Library::new(Some(settings.clone())));
    let (reopened, _) = Session::open(&song).unwrap();
    let id = again.add(reopened, None).unwrap();
    let session = again.get(id).unwrap();
    assert!(session.missing_files().is_empty());
    let folder_of = session.audio_folder().unwrap().to_path_buf();
    let (pool, warnings) = suhina_media::load_project_audio_trusted(session.project(), &folder_of, session.trusted());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(pool.files.len(), 1);
    for dir in [settings, folder, outside, moved] {
        let _ = std::fs::remove_dir_all(dir);
    }
}
