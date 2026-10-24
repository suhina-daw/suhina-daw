// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Saving a project as a Suhina project (.suhina) and opening it again.

use serde_json::json;
use suhina_model::{ContentLane, LaneData};
use suhina_session::{Projects, Session, blank_project};

fn folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-save-{name}-{}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn a_saved_project_opens_as_it_was() {
    let dir = folder("round-trip");
    let mut session = Session::new(blank_project());
    session.execute("track.setVolume", &json!({ "tracks": ["1 MIDI"], "db": -6.0 })).unwrap_or_default();
    assert!(session.has_unsaved_changes());
    // Without a path there is nowhere to save it yet.
    assert!(session.execute("project.save", &json!({})).is_err());

    let saved = session.execute("project.save", &json!({ "path": dir.join("song").display().to_string() }));
    let path = dir.join("song.suhina");
    assert_eq!(saved.ok().and_then(|v| v["path"].as_str().map(String::from)), Some(path.display().to_string()));
    assert_eq!(session.source(), Some(path.as_path()));
    assert!(!session.has_unsaved_changes());

    let (opened, warnings) = Session::open(&path).unwrap_or_else(|e| panic!("{e}"));
    assert!(warnings.is_empty());
    assert_eq!(opened.project(), session.project());
    assert_eq!(opened.audio_folder(), Some(dir.as_path()));

    // Saved again to the same file, without a path.
    session.execute("edit.undo", &json!({})).unwrap_or_default();
    assert!(session.has_unsaved_changes());
    assert!(session.execute("project.save", &json!({})).is_ok());
    assert!(!session.has_unsaved_changes());
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_audio_clip_saves_and_opens_with_nothing_to_decode_or_miss() {
    let dir = folder("empty-audio");
    let mut session = Session::new(blank_project());
    session.execute("clip.create", &json!({ "track": "3 Audio", "scene": 1 })).unwrap_or_else(|e| panic!("{e}"));
    session
        .execute("project.save", &json!({ "path": dir.join("song.suhina").display().to_string() }))
        .unwrap_or_else(|e| panic!("{e}"));
    let (opened, warnings) = Session::open(&dir.join("song.suhina")).unwrap_or_else(|e| panic!("{e}"));
    assert!(warnings.is_empty(), "an empty clip names no file, so nothing is tried: {warnings:?}");
    assert!(opened.missing_files().is_empty(), "an empty clip's missing file would be an empty name");
    assert_eq!(opened.project(), session.project());
    let slot = opened
        .project()
        .launcher
        .columns
        .iter()
        .find(|c| c.track.0 == 3)
        .and_then(|c| c.slots.first())
        .and_then(|s| s.clip.clone());
    let content = slot.and_then(|c| opened.project().contents.iter().find(|x| x.id == c.content).cloned());
    assert!(
        matches!(content.map(|c| c.lanes), Some(lanes) if matches!(lanes.as_slice(), [ContentLane { data: LaneData::Audio(region), .. }] if region.file.is_empty())),
        "the empty clip opened as it was saved"
    );
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_copy_leaves_the_project_with_its_own_file() {
    let dir = folder("copy");
    let mut session = Session::new(blank_project());
    session
        .execute("project.save", &json!({ "path": dir.join("song.suhina").display().to_string() }))
        .unwrap_or_default();
    session.execute("track.setMute", &json!({ "tracks": ["2 MIDI"], "mute": true })).unwrap_or_default();
    let copy = dir.join("templates").join("Live set.suhina");
    let result = session.execute("project.save", &json!({ "path": copy.display().to_string(), "copy": true }));
    assert_eq!(result.ok().map(|v| v["copy"].clone()), Some(json!(true)));
    assert!(copy.is_file());
    assert_eq!(session.source(), Some(dir.join("song.suhina").as_path()));
    assert!(session.has_unsaved_changes(), "a copy is not the project saved");

    // A project made from the template is untitled and the same.
    let (made, _) = Session::from_template(&copy).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(made.source(), None);
    assert_eq!(made.project(), session.project());
    assert!(!made.has_unsaved_changes());
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_project_saved_under_a_new_name_is_shown_by_it() {
    let dir = folder("name");
    let mut projects = Projects::new();
    let id = projects.add(Session::new(blank_project()), None).unwrap_or_else(|e| panic!("{e}"));
    let path = dir.join("Night drive.suhina");
    projects.execute("project.save", &json!({ "path": path.display().to_string() })).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(projects.name(id), Some("Night drive"));
    let copy = dir.join("other.suhina");
    projects.execute("project.save", &json!({ "path": copy.display().to_string(), "copy": true })).unwrap_or_default();
    assert_eq!(projects.name(id), Some("Night drive"));
    let _clean = std::fs::remove_dir_all(&dir);
}

#[test]
fn what_cannot_be_saved_or_opened_is_an_error() {
    let dir = folder("errors");
    let mut session = Session::new(blank_project());
    assert!(session.execute("project.save", &json!({ "path": 3 })).is_err());
    assert!(session.execute("project.save", &json!({ "path": "x.suhina", "copy": "yes" })).is_err());
    std::fs::create_dir_all(&dir).unwrap_or_default();
    let bad = dir.join("bad.suhina");
    std::fs::write(&bad, "{ \"format\": \"suhina\", \"version\": 99, \"project\": {} }").unwrap_or_default();
    assert!(Session::open(&bad).is_err());
    // A file that is neither a Suhina project nor a Live Set is refused by its extension, also
    // a good project saved under the extension from before the rename (ADR 0010 decision 12).
    session.save(&dir.join("good.suhina"), true).unwrap_or_else(|e| panic!("{e}"));
    let old = dir.join("good.locr");
    std::fs::copy(dir.join("good.suhina"), &old).unwrap_or_default();
    let error = Session::open(&old).err().map(String::from).unwrap_or_default();
    assert!(error.contains("Suhina opens a Suhina project (.suhina) or a Live Set (.als)"), "{error}");
    assert!(Session::from_template(&old).is_err());
    let _clean = std::fs::remove_dir_all(&dir);
}
