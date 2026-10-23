// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `library.*` commands (`docs/spec/browser.md` › Commands): folders, search across files,
//! built-in effects and instruments, and plug-ins, pins, recent items recorded from what was added, tags.
#![allow(clippy::unwrap_used, clippy::panic)] // test helpers outside #[test] functions

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use suhina_library::Library;
use suhina_session::{Projects, Session, blank_project};

use crate::support::run;
use suhina_test_support::{WavFormat, write_wav};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-libcmd-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn wav(path: &Path) {
    write_wav(path, 8000, 1, WavFormat::Int16, &[8000.0 / 32767.0; 4000]);
}

/// Projects with a library kept in `settings` (and its own empty plug-in library) and one open
/// blank project.
fn projects(settings: &Path) -> Projects {
    let plugins = suhina_plugins::library::Library::new(Some(settings.join("plugins")), settings.join("no-host"));
    let mut projects = Projects::with_library(plugins);
    projects.set_file_library(Library::new(Some(settings.to_path_buf())));
    projects.add(Session::new(blank_project()), None).unwrap();
    projects
}

fn names(result: &Value) -> Vec<String> {
    result["items"]
        .as_array()
        .map(|l| l.iter().filter_map(|i| i["name"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

#[test]
fn folders_are_added_listed_searched_and_removed() {
    let settings = temp("folders-settings");
    let samples = temp("folders");
    wav(&samples.join("Drums").join("Kick Hard.wav"));
    wav(&samples.join("Drums").join("Snare.wav"));
    std::fs::write(samples.join("groove.mid"), b"MThd").unwrap();
    let mut p = projects(&settings);
    let folder = samples.display().to_string();
    run(&mut p, "library.addFolder", json!({ "folder": folder })).unwrap();
    let listed = run(&mut p, "library.folders", json!({})).unwrap();
    assert_eq!(listed["folders"][0]["files"], 3);
    assert!(run(&mut p, "library.addFolder", json!({ "folder": folder })).is_err());

    let found = run(&mut p, "library.search", json!({ "text": "kick", "category": "samples" })).unwrap();
    assert_eq!(names(&found), ["Kick Hard.wav"]);
    assert_eq!(found["items"][0]["kind"], "sample");
    let midi = run(&mut p, "library.search", json!({ "category": "midi" })).unwrap();
    assert_eq!(names(&midi), ["groove.mid"]);
    let inside =
        run(&mut p, "library.search", json!({ "place": samples.join("Drums").display().to_string() })).unwrap();
    assert_eq!(names(&inside).len(), 2);

    let top = run(&mut p, "library.list", json!({ "folder": folder })).unwrap();
    assert_eq!(names(&top), ["Drums", "groove.mid"]);
    assert_eq!(top["items"][0]["kind"], "folder");

    run(&mut p, "library.removeFolder", json!({ "folder": folder })).unwrap();
    assert_eq!(run(&mut p, "library.folders", json!({})).unwrap()["folders"], json!([]));
    for bad in [
        ("library.addFolder", json!({ "folder": samples.join("nope").display().to_string() })),
        ("library.addFolder", json!({})),
        ("library.removeFolder", json!({ "folder": folder })),
        ("library.list", json!({ "folder": samples.join("nope").display().to_string() })),
        ("library.search", json!({ "category": "videos" })),
        ("library.search", json!({ "place": "recent:videos" })),
        ("library.search", json!({ "limit": 0 })),
        ("library.refresh", json!({ "folder": folder })),
    ] {
        assert!(run(&mut p, bad.0, bad.1.clone()).is_err(), "{} {}", bad.0, bad.1);
    }
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&samples);
}

#[test]
fn search_finds_built_in_devices_by_category() {
    let settings = temp("devices");
    let mut p = projects(&settings);
    let instruments = run(&mut p, "library.search", json!({ "category": "instruments" })).unwrap();
    assert_eq!(names(&instruments), ["Sampler", "FM Synth"]);
    assert_eq!(instruments["items"][0]["item"], "device:sampler");
    let effects = run(&mut p, "library.search", json!({ "category": "effects", "text": "drive" })).unwrap();
    assert_eq!(names(&effects), ["Overdrive"]);
    assert!(names(&run(&mut p, "library.search", json!({ "category": "plugins" })).unwrap()).is_empty());
    let info = run(&mut p, "library.info", json!({ "item": "device:dj-eq" })).unwrap();
    assert_eq!(info["kind"], "effect");
    let _ = std::fs::remove_dir_all(&settings);
}

#[test]
fn what_is_added_to_a_project_becomes_recent() {
    let settings = temp("recent-settings");
    let samples = temp("recent");
    let kick = samples.join("kick 120bpm.wav");
    wav(&kick);
    let mut p = projects(&settings);
    run(&mut p, "clip.addFiles", json!({ "paths": [kick.display().to_string()], "track": 3, "start": 0.0 })).unwrap();
    run(&mut p, "device.add", json!({ "track": 3, "id": "overdrive" })).unwrap();
    run(&mut p, "track.add", json!({ "device": "fm-synth" })).unwrap();
    // A failed command records nothing.
    let _ = run(&mut p, "device.add", json!({ "track": 3, "id": "fm-synth" }));
    let recent = run(&mut p, "library.recent", json!({})).unwrap();
    let absolute = std::path::absolute(&kick).unwrap().display().to_string();
    assert_eq!(recent["samples"], json!([absolute]));
    assert_eq!(recent["effects"], json!(["device:overdrive"]));
    assert_eq!(recent["instruments"], json!(["device:fm-synth"]));
    let listed = run(&mut p, "library.search", json!({ "place": "recent:samples" })).unwrap();
    assert_eq!(names(&listed), ["kick 120bpm.wav"]);
    // A sample's tempo comes from its name.
    let info = run(&mut p, "library.info", json!({ "item": absolute })).unwrap();
    assert_eq!((info["bpm"].as_f64(), info["bpm_from"].as_str()), (Some(120.0), Some("name")));
    assert_eq!(info["seconds"], 0.5);
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&samples);
}

#[test]
fn pins_and_tags_are_kept_between_runs() {
    let settings = temp("tags");
    let mut p = projects(&settings);
    run(&mut p, "library.pin", json!({ "item": "device:utility" })).unwrap();
    run(&mut p, "library.setTag", json!({ "name": "Mixing", "color": "#336699" })).unwrap();
    run(&mut p, "library.tag", json!({ "item": "device:utility", "tag": "Mixing", "on": true })).unwrap();
    assert!(run(&mut p, "library.tag", json!({ "item": "device:utility", "tag": "Nope", "on": true })).is_err());
    assert!(run(&mut p, "library.setTag", json!({ "name": "Bad", "color": "blue" })).is_err());
    assert!(run(&mut p, "library.setTag", json!({})).is_err());
    assert!(run(&mut p, "library.setTag", json!({ "limit": 2 })).is_err(), "there are more tags than that");

    let mut again = projects(&settings);
    assert_eq!(run(&mut again, "library.pinned", json!({})).unwrap()["items"], json!(["device:utility"]));
    let tagged = run(&mut again, "library.search", json!({ "tags": ["Mixing"] })).unwrap();
    assert_eq!(names(&tagged), ["Utility"]);
    assert_eq!(tagged["items"][0]["tags"], json!(["Mixing"]));
    let pinned = run(&mut again, "library.search", json!({ "place": "pinned" })).unwrap();
    assert_eq!(names(&pinned), ["Utility"]);
    run(&mut again, "library.removeTag", json!({ "name": "Mixing" })).unwrap();
    run(&mut again, "library.unpin", json!({ "item": "device:utility" })).unwrap();
    assert_eq!(run(&mut again, "library.tags", json!({})).unwrap()["tagged"], json!({}));
    let _ = std::fs::remove_dir_all(&settings);
}

/// A plug-in catalog as a scan leaves one, in the settings' plug-in library.
fn catalog(settings: &Path) {
    let text = json!({
        "version": 2,
        "entries": [
            { "path": "/VST3/Alpha Verb.vst3", "modified": 1_760_000_000, "size": 10, "plugins": [
                { "id": "ABCDEF019182FAEB49746D78426E6477", "name": "Alpha Verb", "vendor": "Alpha", "version": "1.0", "features": ["Fx", "Reverb"], "replaces": [] }] },
            { "path": "/VST3/Alpha Delay.vst3", "modified": 1_760_100_000, "size": 10, "plugins": [
                { "id": "ABCDEF019182FAEB49746D78426E6476", "name": "Alpha Delay", "vendor": "Alpha", "version": "1.0", "features": ["Fx", "Delay"], "replaces": [] }] },
            { "path": "/CLAP/Zeta Synth.clap", "modified": 1_760_200_000, "size": 10, "plugins": [
                { "id": "ABCDEF019182FAEB49746D78426E6478", "name": "Zeta Synth", "vendor": "Zeta", "version": "2.0", "features": ["instrument", "synthesizer"], "replaces": [] }] },
        ],
    });
    std::fs::create_dir_all(settings.join("plugins")).unwrap();
    std::fs::write(settings.join("plugins").join("plugin-catalog.json"), text.to_string()).unwrap();
}

#[test]
fn a_plugins_view_is_kept_and_plug_ins_are_pinned_and_tagged_like_files() {
    let settings = temp("views");
    catalog(&settings);
    let verb = "plugin:vst3:/VST3/Alpha Verb.vst3#ABCDEF019182FAEB49746D78426E6477";
    let mut p = projects(&settings);
    // The view choice: a real one is set and kept; junk is an error, not a change.
    assert!(run(&mut p, "library.setPluginsView", json!({ "view": "nope" })).is_err());
    run(&mut p, "library.setPluginsView", json!({ "view": "effects" })).unwrap();
    // Pins and tags work on a plug-in's key like on a file's.
    run(&mut p, "library.pin", json!({ "item": verb })).unwrap();
    run(&mut p, "library.setTag", json!({ "name": "Go-to", "color": "#336699" })).unwrap();
    run(&mut p, "library.tag", json!({ "item": verb, "tag": "Go-to", "on": true })).unwrap();

    let mut again = projects(&settings);
    let pinned = run(&mut again, "library.search", json!({ "place": "pinned" })).unwrap();
    assert_eq!(names(&pinned), ["Alpha Verb"]);
    assert_eq!(pinned["items"][0]["tags"], json!(["Go-to"]));
    let tagged = run(&mut again, "library.search", json!({ "tags": ["Go-to"] })).unwrap();
    assert_eq!(names(&tagged), ["Alpha Verb"]);
    // The view survives a restart (it is kept in browser.json), and the flat search lists the
    // plug-ins with their details.
    let mut view = Library::new(Some(settings.to_path_buf()));
    assert_eq!(view.plugins_view(), suhina_library::store::PluginsView::Effects);
    let all = run(&mut again, "library.search", json!({ "category": "plugins" })).unwrap();
    let mut all_names = names(&all);
    all_names.sort();
    assert_eq!(all_names, ["Alpha Delay", "Alpha Verb", "Zeta Synth"]);
    assert_eq!(all["items"][0]["detail"], json!("vst3 Alpha"));
    assert_eq!(all["items"][0]["kind"], json!("effect"));
    let _ = std::fs::remove_dir_all(&settings);
}

#[test]
fn library_commands_are_listed_and_need_no_project() {
    let settings = temp("listed");
    let mut p = Projects::default();
    p.set_file_library(Library::new(Some(settings.clone())));
    assert!(run(&mut p, "library.folders", json!({})).is_ok(), "no project is open");
    let all = run(&mut p, "command.list", json!({})).unwrap();
    let ids: Vec<&str> = all.as_array().unwrap().iter().filter_map(|c| c["id"].as_str()).collect();
    for id in ["library.search", "library.addFolder", "track.add", "clip.addFiles", "preview.play"] {
        assert!(ids.contains(&id), "{id}");
    }
    let _ = std::fs::remove_dir_all(&settings);
}
