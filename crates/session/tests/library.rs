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
fn search_finds_stock_presets_by_name_and_tag() {
    let settings = temp("stock");
    let mut p = projects(&settings);
    let presets = run(&mut p, "library.search", json!({ "category": "presets" })).unwrap();
    assert!(names(&presets).iter().any(|name| name == "Warm FM Bass"), "{presets}");
    let by_tag = run(&mut p, "library.search", json!({ "category": "presets", "text": "trance" })).unwrap();
    assert_eq!(names(&by_tag), ["Wide Trance Pad"]);
    assert_eq!(by_tag["items"][0]["item"], "stock:fm-synth/wide-trance-pad");
    let info = run(&mut p, "library.info", json!({ "item": "stock:fm-synth/wide-trance-pad" })).unwrap();
    assert_eq!((info["kind"].as_str(), info["device"].as_str()), (Some("preset"), Some("fm-synth")));
    assert!(run(&mut p, "library.info", json!({ "item": "stock:fm-synth/nothing" })).is_err());
    let instruments = run(&mut p, "library.search", json!({ "category": "instruments" })).unwrap();
    assert!(!names(&instruments).iter().any(|name| name == "Warm FM Bass"));
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
    assert_eq!(tagged["items"][0]["tags"][0]["name"], json!("Mixing"));
    let pinned = run(&mut again, "library.search", json!({ "place": "pinned" })).unwrap();
    assert_eq!(names(&pinned), ["Utility"]);
    run(&mut again, "library.removeTag", json!({ "name": "Mixing" })).unwrap();
    run(&mut again, "library.unpin", json!({ "item": "device:utility" })).unwrap();
    assert_eq!(run(&mut again, "library.tags", json!({})).unwrap()["tagged"], json!({}));
    let _ = std::fs::remove_dir_all(&settings);
}

#[test]
fn a_tag_place_lists_every_item_that_carries_the_tag_of_any_kind() -> Result<(), Box<dyn std::error::Error>> {
    let settings = temp("tag-place-settings");
    let samples = temp("tag-place");
    wav(&samples.join("Drums").join("Kick Hard.wav"));
    wav(&samples.join("Drums").join("Snare.wav"));
    let mut p = projects(&settings);
    run(&mut p, "library.addFolder", json!({ "folder": samples.display().to_string() })).unwrap();
    let snare = samples.join("Drums").join("Snare.wav").display().to_string();
    run(&mut p, "library.tag", json!({ "item": snare, "tag": "Red", "on": true })).unwrap();
    // Anything tagged anywhere shows: a built-in device and a preset bank's voice.
    run(&mut p, "library.tag", json!({ "item": "device:utility", "tag": "Red", "on": true })).unwrap();
    let voice = format!("{}#2", samples.join("Voices.preset.suhina").display());
    std::fs::write(samples.join("Voices.preset.suhina"), b"").unwrap();
    run(&mut p, "library.tag", json!({ "item": voice, "tag": "Red", "on": true })).unwrap();

    // Natural order by name, whatever the kind: the sample, the device, the voice.
    let place = run(&mut p, "library.search", json!({ "place": "tag:Red" })).unwrap();
    assert_eq!(names(&place), ["Snare.wav", "Utility", "Voices.preset.suhina #2"]);
    assert_eq!(place["items"][0]["tags"][0]["name"], json!("Red"));
    // The words still narrow the list, as the browser's search field does over a place.
    let named = run(&mut p, "library.search", json!({ "text": "utility", "place": "tag:Red" })).unwrap();
    assert_eq!(names(&named), ["Utility"]);
    // A category keeps its kinds only.
    let effects = run(&mut p, "library.search", json!({ "place": "tag:Red", "category": "effects" })).unwrap();
    assert_eq!(names(&effects), ["Utility"]);
    let snares = run(&mut p, "library.search", json!({ "place": "tag:Red", "category": "samples" })).unwrap();
    assert_eq!(names(&snares), ["Snare.wav"]);
    // The tags filter combines on its own terms: a search with no place filters the whole
    // library by the tags (#1196).
    run(&mut p, "library.setTag", json!({ "name": "Hot", "color": "#336699" })).unwrap();
    let files = run(&mut p, "library.search", json!({ "tags": ["Red", "Hot"] })).unwrap();
    assert_eq!(names(&files), ["Utility", "Snare.wav"]);
    // A place of several tags joins their ids into one place (the joined place's own test
    // is below); the `tags` param narrows a listed place to its items that carry one of
    // them, as the footer's dots narrow a one-tag place ([#1250] review).
    let narrowed = run(&mut p, "library.search", json!({ "place": "tag:Red", "tags": ["Hot"] })).unwrap();
    let none: Vec<String> = names(&narrowed);
    assert!(none.is_empty(), "the filter narrows the place, it never widens it: {none:?}");
    // The whole string names its tag first: a tag whose name has a `+` in it is its own
    // place, not a join ([#1250] review).
    run(&mut p, "library.setTag", json!({ "name": "a+b", "color": "#336699" })).unwrap();
    run(&mut p, "library.tag", json!({ "item": "device:utility", "tag": "a+b", "on": true })).unwrap();
    let whole = run(&mut p, "library.search", json!({ "place": "tag:a+b" })).unwrap();
    assert_eq!(names(&whole), ["Utility"]);
    // Every part names its tag: a part that is none refuses rather than quietly searching
    // the rest ([#1250] review).
    assert!(run(&mut p, "library.search", json!({ "place": "tag:Red+Nope" })).is_err());
    // A place of a tag that no longer is one is an error, not an empty list.
    assert!(run(&mut p, "library.search", json!({ "place": "tag:Nope" })).is_err());
    assert!(run(&mut p, "library.search", json!({ "place": "tag:" })).is_err());
    // A tag place sorts by name before the limit cuts: the keys' order would rank
    // `/a/Zebra` over `/z/Apple` otherwise (#1196).
    let deep = temp("tag-place-order");
    std::fs::create_dir_all(deep.join("a"))?;
    std::fs::create_dir_all(deep.join("z"))?;
    wav(&deep.join("a").join("Zebra.wav"));
    wav(&deep.join("z").join("Apple.wav"));
    run(&mut p, "library.addFolder", json!({ "folder": deep.display().to_string() })).unwrap();
    for (letter, name) in [("a", "Zebra.wav"), ("z", "Apple.wav")] {
        let item = deep.join(letter).join(name).display().to_string();
        run(&mut p, "library.tag", json!({ "item": item, "tag": "Hot", "on": true })).unwrap();
    }
    let limited = run(&mut p, "library.search", json!({ "place": "tag:Hot", "limit": 1 })).unwrap();
    assert_eq!(names(&limited), ["Apple.wav"], "the sort leads, the limit cuts after it");
    // A place that keeps its own order cuts at the limit as it walks, so nothing past the
    // cut is read ([#1250] review; only the places that sort before the cut walk on).
    run(&mut p, "library.pin", json!({ "item": deep.join("a").join("Zebra.wav").display().to_string() })).unwrap();
    run(&mut p, "library.pin", json!({ "item": deep.join("z").join("Apple.wav").display().to_string() })).unwrap();
    let first = run(&mut p, "library.search", json!({ "place": "pinned", "limit": 1 })).unwrap();
    assert_eq!(names(&first), ["Zebra.wav"], "the pins' own order, cut at the limit");
    let _ = std::fs::remove_dir_all(&settings);
    Ok(())
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
    assert_eq!(pinned["items"][0]["tags"][0]["name"], json!("Go-to"));
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

#[test]
fn move_to_moves_files_numbering_taken_names_and_carries_their_tags_and_pins() {
    let settings = temp("move-settings");
    let samples = temp("move-samples");
    let out = temp("move-out");
    wav(&samples.join("Kick.wav"));
    wav(&samples.join("Snare.wav"));
    std::fs::write(out.join("Kick.wav"), b"mine").unwrap();
    let mut p = projects(&settings);
    let kick = samples.join("Kick.wav").display().to_string();
    run(&mut p, "library.pin", json!({ "item": kick })).unwrap();
    run(&mut p, "library.tag", json!({ "item": kick, "tag": "Red", "on": true })).unwrap();
    let gone = samples.join("Gone.wav").display().to_string();
    let snare = samples.join("Snare.wav").display().to_string();
    let items = json!([kick, snare, gone]);
    let done = run(&mut p, "library.moveTo", json!({ "items": items, "folder": out.display().to_string() })).unwrap();
    assert_eq!(done["moved"].as_array().unwrap().len(), 2, "{done}");
    assert_eq!(done["missing"], json!([gone]));
    // The originals are gone; the name taken there got a number and the file there is left alone.
    assert!(!samples.join("Kick.wav").exists() && !samples.join("Snare.wav").exists());
    assert_eq!(std::fs::read(out.join("Kick.wav")).unwrap(), b"mine");
    assert!(out.join("Kick 2.wav").is_file() && out.join("Snare.wav").is_file());
    // The pin and the tag went with the file.
    let moved = out.join("Kick 2.wav").display().to_string();
    let library = p.file_library_mut();
    assert_eq!(library.pinned(), std::slice::from_ref(&moved));
    assert_eq!(
        library.item_tags(&moved).iter().map(|id| library.tag_name_of(*id)).collect::<Vec<_>>(),
        [Some("Red".to_string())]
    );
    assert!(library.item_tags(&kick).is_empty());
    // A file already in the folder stays.
    let again = json!({ "items": [moved], "folder": out.display().to_string() });
    let done = run(&mut p, "library.moveTo", again).unwrap();
    assert_eq!(done["kept"].as_array().unwrap().len(), 1, "{done}");
    assert!(out.join("Kick 2.wav").is_file());
}

#[test]
fn copy_to_copies_files_into_a_folder_numbering_taken_names_and_lists_the_missing() {
    let settings = temp("copy-settings");
    let samples = temp("copy-samples");
    let out = temp("copy-out");
    wav(&samples.join("Kick.wav"));
    wav(&samples.join("Snare.wav"));
    std::fs::write(out.join("Kick.wav"), b"mine").unwrap();
    let mut p = projects(&settings);
    let gone = samples.join("Gone.wav");
    let items: Vec<String> = [samples.join("Kick.wav"), samples.join("Snare.wav"), gone.clone()]
        .iter()
        .map(|f| f.display().to_string())
        .collect();
    let done = run(&mut p, "library.copyTo", json!({ "items": items, "folder": out.display().to_string() })).unwrap();
    // The name taken there gets a number; the file that was there is left alone.
    assert_eq!(std::fs::read(out.join("Kick.wav")).unwrap(), b"mine");
    assert_eq!(std::fs::read(out.join("Kick 2.wav")).unwrap(), std::fs::read(samples.join("Kick.wav")).unwrap());
    assert!(out.join("Snare.wav").is_file());
    assert_eq!(done["copied"].as_array().unwrap().len(), 2, "{done}");
    assert_eq!(done["missing"], json!([gone.display().to_string()]));
    // The originals stay where they are.
    assert!(samples.join("Kick.wav").is_file() && samples.join("Snare.wav").is_file());
    // A second copy takes the next number.
    let again =
        json!({ "items": [samples.join("Kick.wav").display().to_string()], "folder": out.display().to_string() });
    let done = run(&mut p, "library.copyTo", again).unwrap();
    assert!(done["copied"][0]["to"].as_str().unwrap().ends_with("Kick 3.wav"), "{done}");
    assert!(run(&mut p, "library.copyTo", json!({ "items": [], "folder": out.display().to_string() })).is_err());
    let nowhere = out.join("no such folder").display().to_string();
    assert!(run(&mut p, "library.copyTo", json!({ "items": items, "folder": nowhere })).is_err());
}

// A place of several tags goes as the ids joined `tag:#<id>+#<id>`: the items carrying any
// of them, and the `tags` filter still narrows it ([#1254] review).
#[test]
fn a_place_of_several_tags_is_the_ids_joined_and_the_filter_narrows_it() {
    let settings = temp("tags-joined-settings");
    let samples = temp("tags-joined");
    wav(&samples.join("Kick.wav"));
    wav(&samples.join("Snare.wav"));
    let mut p = projects(&settings);
    run(&mut p, "library.addFolder", json!({ "folder": samples.display().to_string() })).unwrap();
    let kick = samples.join("Kick.wav").display().to_string();
    let snare = samples.join("Snare.wav").display().to_string();
    run(&mut p, "library.tag", json!({ "item": kick, "tag": "Red", "on": true })).unwrap();
    run(&mut p, "library.tag", json!({ "item": snare, "tag": "Blue", "on": true })).unwrap();
    let library = p.file_library_mut();
    let flat = library.tag_flat();
    let id_of = |name: &str| flat.iter().find(|t| t.name == name).map(|t| t.id).unwrap_or(0);
    let (red, blue) = (id_of("Red"), id_of("Blue"));
    // The ids joined list the items carrying either.
    let either = run(&mut p, "library.search", json!({ "place": format!("tag:#{red}+#{blue}") })).unwrap();
    assert_eq!(names(&either), ["Kick.wav", "Snare.wav"]);
    // A joined place of a tag that is not one refuses.
    let missing = red + 500;
    assert!(run(&mut p, "library.search", json!({ "place": format!("tag:#{red}+#{missing}") })).is_err());
    // A part that is not an id's own number at all refuses rather than searching what
    // parsed ([#1254] review: the caller reads what is asked, not the subset silently).
    for bad in ["oops", "4294967296"] {
        let place = format!("tag:#{red}+{bad}");
        assert!(run(&mut p, "library.search", json!({ "place": place })).is_err(), "{bad} in the join is refused");
        // With its `#` head the part reaches the joined parse: a regression that drops a
        // hash-prefixed component would pass the bare form alone ([#1254] review).
        let place = format!("tag:#{red}+#{bad}");
        assert!(run(&mut p, "library.search", json!({ "place": place })).is_err(), "#{bad} in the join is refused");
    }
    // The `tags` filter narrows the place's list, it does not widen it: the item must also
    // carry a filter tag ([#1254] review).
    run(&mut p, "library.setTag", json!({ "name": "Hot", "color": "#336699" })).unwrap();
    run(&mut p, "library.tag", json!({ "item": snare, "tag": "Hot", "on": true })).unwrap();
    let both =
        run(&mut p, "library.search", json!({ "place": format!("tag:#{red}+#{blue}"), "tags": ["Hot"] })).unwrap();
    assert_eq!(names(&both), ["Snare.wav"], "only one carried a tag of the place AND the filter's");
    // One tag still resolves as a name too.
    let single = run(&mut p, "library.search", json!({ "place": format!("tag:#{red}") })).unwrap();
    assert_eq!(names(&single), ["Kick.wav"]);
    let _ = std::fs::remove_dir_all(&settings);
}

// `library.setTag` makes a tag for an unused name given as its `id` ([#1254] review: the
// help says so), while an id's own form that is not one stays an error.
#[test]
fn set_tag_makes_a_tag_for_an_unused_name_and_an_id_selector_refuses() {
    let settings = temp("set-tag-name-settings");
    let mut p = projects(&settings);
    // `id` as an unused name: made, coloured from the named colours' next turn.
    let made = run(&mut p, "library.setTag", json!({ "id": "Drums" })).unwrap();
    assert!(made["tags"].as_array().unwrap().iter().any(|t| t["name"] == json!("Drums")));
    made_id_tagged(&mut p, "Drums");
    // The rename goes with the make: the id's name is formed, what rename says names it.
    // The whole list is the check: a filtered one would pass an ignored rename by being
    // empty ([#1254] review).
    run(&mut p, "library.setTag", json!({ "id": "Strums", "rename": "Plucks" })).unwrap();
    let listed = run(&mut p, "library.tags", json!({})).unwrap();
    let tags = listed["tags"].as_array().unwrap();
    assert!(tags.iter().any(|t| t["name"] == json!("Plucks")), "the rename named it: {tags:?}");
    assert!(!tags.iter().any(|t| t["name"] == json!("Strums")), "the rename named it instead: {tags:?}");
    // An id's own form that is not a tag: refused, as before.
    assert!(run(&mut p, "library.setTag", json!({ "id": "#999" })).is_err());
    // The `name` form keeps its place when `id` is missing.
    run(&mut p, "library.setTag", json!({ "name": "Beats" })).unwrap();
    let tags = made_id_tagged(&mut p, "Beats");
    assert_eq!(tags.iter().filter(|t| t["name"] == json!("Beats")).count(), 1);
    let _ = std::fs::remove_dir_all(&settings);
}

// The `#`-selector's shapes and their names ([#1254] review): `#<digits>` is the id's own
// shape no tag can take, and any other leading-# name is made and reached as a name.
#[test]
fn a_hash_name_made_and_reached_as_a_name_refuses_the_id_form() {
    let settings = temp("hash-name-settings");
    let mut p = projects(&settings);
    // `#notes` is a name: made, reached by its name in a `tag:` place, and removing it by
    // that name removes it, not an id.
    run(&mut p, "library.setTag", json!({ "name": "#notes" })).unwrap();
    run(&mut p, "library.search", json!({ "place": "tag:#notes" })).unwrap();
    run(&mut p, "library.removeTag", json!({ "name": "#notes" })).unwrap();
    assert!(run(&mut p, "library.removeTag", json!({ "name": "#notes" })).is_err(), "the second remove is none");
    let tags = run(&mut p, "library.tags", json!({})).unwrap();
    assert!(!tags["tags"].as_array().unwrap().iter().any(|t| t["name"] == json!("#notes")), "{tags:?}");
    run(&mut p, "library.setTag", json!({ "name": "#notes" })).unwrap();
    run(&mut p, "library.search", json!({ "place": "tag:#notes" })).unwrap();
    // `#1` is the id's own: no tag by that name, and the make refuses.
    assert!(run(&mut p, "library.setTag", json!({ "name": "#1" })).is_err());
    // A rename into the id's form is refused too, and a folder's.
    run(&mut p, "library.addTagFolder", json!({ "name": "Box" })).unwrap();
    run(&mut p, "library.setTag", json!({ "name": "Notes" })).unwrap();
    run(&mut p, "library.setTag", json!({ "name": "Fret", "color": "#123456" })).unwrap();
    assert!(run(&mut p, "library.setTag", json!({ "name": "Fret", "rename": "#2" })).is_err());
    assert!(run(&mut p, "library.addTagFolder", json!({ "name": "#2" })).is_err());
    // `tag:#notes` resolved above; the id's shape nobody holds stays an error for the
    // place's part too.
    assert!(
        run(&mut p, "library.search", json!({ "place": "tag:#8765432" })).is_err(),
        "the unknown id's place refuses",
    );
    let _ = std::fs::remove_dir_all(&settings);
}

// A folder of the library carries tags as any item does, and its tag's place lists it as a
// folder, as Pinned lists a pinned one: a top folder and a sub-folder alike ([#1254] review:
// the tag's row counted the folder, and its place said no item carried the tag).
#[test]
fn a_tagged_or_pinned_folder_lists_as_a_folder() {
    let settings = temp("folder-tag-settings");
    let samples = temp("folder-tag");
    std::fs::create_dir_all(samples.join("Kicks")).unwrap();
    wav(&samples.join("Kicks").join("Kick.wav"));
    let mut p = projects(&settings);
    run(&mut p, "library.addFolder", json!({ "folder": samples.display().to_string() })).unwrap();
    let kicks = samples.join("Kicks").display().to_string();
    let root = samples.display().to_string();
    let root_name = samples.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    for item in [&kicks, &root] {
        run(&mut p, "library.tag", json!({ "item": item, "tag": "Red", "on": true })).unwrap();
    }
    let listed = |found: &Value| -> Vec<(String, String)> {
        found["items"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .map(|i| {
                        (i["name"].as_str().unwrap_or("").to_string(), i["kind"].as_str().unwrap_or("").to_string())
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let found = run(&mut p, "library.search", json!({ "place": "tag:Red" })).unwrap();
    let mut tagged = listed(&found);
    tagged.sort();
    let mut wanted = vec![("Kicks".to_string(), "folder".to_string()), (root_name.clone(), "folder".to_string())];
    wanted.sort();
    assert_eq!(tagged, wanted, "both folders list, as folders: {found}");
    // A file is no folder: the sample tagged too lists as a sample beside them.
    let kick = samples.join("Kicks").join("Kick.wav").display().to_string();
    run(&mut p, "library.tag", json!({ "item": kick, "tag": "Red", "on": true })).unwrap();
    let found = run(&mut p, "library.search", json!({ "place": "tag:Red" })).unwrap();
    assert!(listed(&found).contains(&("Kick.wav".to_string(), "sample".to_string())), "{found}");
    // A category other than All lists files of its kind, not folders.
    let samples_only = run(&mut p, "library.search", json!({ "place": "tag:Red", "category": "samples" })).unwrap();
    assert_eq!(listed(&samples_only), [("Kick.wav".to_string(), "sample".to_string())]);
    // Pinned lists a pinned folder the same way.
    run(&mut p, "library.pin", json!({ "item": root })).unwrap();
    let pinned = run(&mut p, "library.search", json!({ "place": "pinned" })).unwrap();
    assert_eq!(listed(&pinned), [(root_name, "folder".to_string())], "{pinned}");
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&samples);
}

// The selectors' grammar is checked before a number is read ([#1254] review): `#+1` is a
// name (Rust's number parse took its sign and reached Red, the tag of id 1, so removing
// `#+1` removed Red and what carried it), a bare `#` is a name, `#notes` names a folder as
// it names a tag, and a joined shape (`#1+#2`) is the place of several, never a name.
#[test]
fn a_selectors_shape_is_checked_before_its_number_is_read() {
    let settings = temp("selector-grammar-settings");
    let samples = temp("selector-grammar");
    wav(&samples.join("Kick.wav"));
    let mut p = projects(&settings);
    run(&mut p, "library.addFolder", json!({ "folder": samples.display().to_string() })).unwrap();
    let kick = samples.join("Kick.wav").display().to_string();
    run(&mut p, "library.tag", json!({ "item": kick.clone(), "tag": "#1", "on": true })).unwrap();
    let red = p.file_library_mut().tag_flat().iter().find(|t| t.name == "Red").map(|t| t.id).unwrap_or(0);
    assert_eq!(red, 1, "Red is the first tag");
    for name in ["#+1", "#"] {
        run(&mut p, "library.setTag", json!({ "name": name })).unwrap();
        run(&mut p, "library.tag", json!({ "item": kick.clone(), "tag": name, "on": true })).unwrap();
        let found = run(&mut p, "library.search", json!({ "place": format!("tag:{name}") })).unwrap();
        assert_eq!(names(&found), ["Kick.wav"], "{name} reaches its own tag");
        run(&mut p, "library.removeTag", json!({ "name": name })).unwrap();
        let flat = p.file_library_mut().tag_flat();
        assert!(flat.iter().any(|t| t.name == "Red"), "removing {name} left Red: {flat:?}");
        assert!(!flat.iter().any(|t| t.name == name), "{name} itself went");
    }
    let carried = run(&mut p, "library.search", json!({ "place": "tag:#1" })).unwrap();
    assert_eq!(names(&carried), ["Kick.wav"], "the item still carries Red");
    // A joined shape is the selectors', so no tag is named so.
    assert!(run(&mut p, "library.setTag", json!({ "name": "#1+#2" })).is_err());
    assert!(run(&mut p, "library.setTag", json!({ "name": "Fret", "rename": "#3+#4" })).is_err());
    // A folder named `#notes` (or `#`) is reached by its name in every folder param.
    for name in ["#notes", "#"] {
        let made = run(&mut p, "library.addTagFolder", json!({ "name": name })).unwrap();
        let id = made["folder"].as_u64().unwrap_or(0);
        run(&mut p, "library.setFolderOpen", json!({ "folder": name, "open": false })).unwrap();
        run(&mut p, "library.renameTagFolder", json!({ "folder": name, "rename": format!("{name} box") })).unwrap();
        let renamed = format!("{name} box");
        run(&mut p, "library.setTag", json!({ "name": format!("in {name}"), "folder": renamed.clone() })).unwrap();
        let listed = run(&mut p, "library.tags", json!({})).unwrap();
        let inside = listed["tags"].as_array().unwrap().iter().find(|t| t["name"] == json!(format!("in {name}")));
        assert_eq!(inside.map(|t| t["folder"].clone()), Some(json!(id)), "made in {renamed}: {listed}");
        run(&mut p, "library.removeTagFolder", json!({ "folder": renamed })).unwrap();
    }
    let _ = std::fs::remove_dir_all(&settings);
    let _ = std::fs::remove_dir_all(&samples);
}

// A make with a `folder` lands in the folder from its first byte, its name asked of the
// destination's own level ([#1254] review): a top-level folder's name is no show-stopper.
#[test]
fn set_tag_makes_into_the_folder_the_params_ask() {
    let settings = temp("set-tag-folder-settings");
    let mut p = projects(&settings);
    run(&mut p, "library.addTagFolder", json!({ "name": "Box" })).unwrap();
    // The first folder the command made carries its id in the reply ([#1254]: ids list).
    let tags = run(&mut p, "library.addTagFolder", json!({ "parent": "#8", "name": "Drums" })).unwrap();
    let box_id = tags["folder"].as_u64().unwrap_or(0);
    // A tag named Drums (which a *top-level* folder too may be named) inside Box: the
    // destination's own level is what the make asks its name of. The old flow (made at
    // the top, moved under) refused with the top's saying.
    run(&mut p, "library.addTagFolder", json!({ "name": "Outfit" })).unwrap();
    run(&mut p, "library.setTag", json!({ "name": "Snare", "folder": format!("#{box_id}") })).unwrap();
    run(&mut p, "library.setTag", json!({ "name": "Drums", "folder": format!("#{box_id}") })).unwrap();
    let tags = run(&mut p, "library.tags", json!({})).unwrap();
    // The makes landed in the folder: in the flat list, neither name is among the rows
    // whose `folder` is none ([#1254] review).
    let tops = tags["tags"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| ["Snare", "Drums"].contains(&t["name"].as_str().unwrap_or_default()) && t["folder"].is_null())
        .count();
    assert_eq!(tops, 0, "{tags:?}: the makes landed in the folder, not at the top");
    // A folder the params do not name refuses the same make.
    assert!(run(&mut p, "library.setTag", json!({ "name": "Rim", "folder": "#76543" })).is_err());
    // The folder that has the wanted name is named as the folder it is ([#1254] review:
    // the message said tag, and named the destination's own name).
    let kicked = run(&mut p, "library.addTagFolder", json!({ "name": "Kick", "parent": format!("#{box_id}") }));
    assert!(kicked.is_ok(), "{kicked:?}");
    let refused = run(&mut p, "library.setTag", json!({ "name": "Kick", "folder": format!("#{box_id}") }));
    assert!(matches!(&refused, Err(e) if e.to_string().contains("there is a tag folder")), "{refused:?}");
    // A tag the library has takes no `folder`: its `id` change runs plain, the moves go
    // through `library.moveTag`.
    let made = run(&mut p, "library.setTag", json!({ "name": "Belt" })).unwrap();
    let belt_id = made["tags"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == json!("Belt"))
        .map(|t| t["id"].as_u64().unwrap_or(0))
        .unwrap_or(0);
    assert!(run(&mut p, "library.setTag", json!({ "id": format!("#{belt_id}"), "folder": "#8" })).is_err());
    let _ = std::fs::remove_dir_all(&settings);
}

fn made_id_tagged(p: &mut Projects, want: &str) -> Vec<Value> {
    run(p, "library.tags", json!({})).unwrap()["tags"]
        .as_array()
        .unwrap()
        .clone()
        .into_iter()
        .filter(|t| t["name"] == json!(want))
        .collect()
}

// The folder params are folder-sounding: a tag of the same name does not shadow the
// folder ([#1254] review); `moveTag`'s `id` still takes both.
#[test]
fn folder_params_resolve_folders_even_when_a_tag_shares_the_name() {
    let settings = temp("folder-param-settings");
    let mut p = projects(&settings);
    // A tag named Belt at the top, and a folder named Belt under a folder of its own.
    run(&mut p, "library.setTag", json!({ "name": "Belt" })).unwrap();
    run(&mut p, "library.addTagFolder", json!({ "name": "Box" })).unwrap();
    run(&mut p, "library.addTagFolder", json!({ "name": "Belt", "parent": "Box" })).unwrap();
    // Renaming the folder by name now names the folder, not reaching for the tag.
    run(&mut p, "library.renameTagFolder", json!({ "folder": "Belt", "rename": "Pouch" })).unwrap();
    let tags = run(&mut p, "library.tags", json!({})).unwrap();
    let folders = tags["folders"].as_array().unwrap();
    assert!(folders.iter().any(|f| f["name"] == json!("Pouch")), "{folders:?}");
    assert!(folders.iter().all(|f| f["name"] != json!("Belt")), "{folders:?}");
    // The tag named Belt still stands, unrenamed.
    assert!(tags["tags"].as_array().unwrap().iter().any(|t| t["name"] == json!("Belt")), "the tag kept its name");
    // A same-named tag does not stop a folder command either: removing by name lifts the
    // folder (its tags and folders up), the tag untouched ([#1254] review).
    run(&mut p, "library.removeTagFolder", json!({ "folder": "Pouch" })).unwrap();
    let after = run(&mut p, "library.tags", json!({})).unwrap();
    assert!(after["folders"].as_array().unwrap().iter().all(|f| f["name"] == json!("Box")), "{after:?}");
    assert!(after["tags"].as_array().unwrap().iter().any(|t| t["name"] == json!("Belt")));
    // `library.moveTag`'s own `id` still reads a tag first: the tag named Belt moves, not
    // the folder named Box ([#1254]: the mixed id).
    let library = p.file_library_mut();
    let flat = library.tag_flat();
    let tag = flat.iter().find(|t| t.name == "Belt").map(|t| t.id).unwrap_or(0);
    run(&mut p, "library.moveTag", json!({ "id": format!("#{tag}"), "folder": "Box" })).unwrap();
    let after = run(&mut p, "library.tags", json!({})).unwrap();
    // The tag Belt now sits under Box's level: its folder is Box's id.
    let box_id = after["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == json!("Box"))
        .map(|f| f["id"].as_u64().unwrap_or(0))
        .unwrap_or(0);
    assert!(
        after["tags"].as_array().unwrap().iter().any(|t| t["name"] == json!("Belt") && t["folder"] == json!(box_id)),
        "{after:?}"
    );
    let _ = std::fs::remove_dir_all(&settings);
}

// Reordering the sidebar keeps whether a section showed open ([#1254] review: both
// reorderings list every section, and a collapse survived nothing before).
#[test]
fn set_sections_keeps_whether_a_section_showed_open() {
    let settings = temp("sections-state-settings");
    let mut p = projects(&settings);
    run(&mut p, "library.setSectionOpen", json!({ "section": "tags", "open": false })).unwrap();
    // The order the UI's drag and menu send: every section, each once.
    run(&mut p, "library.setSections", json!({ "order": ["tags", "pinned", "library"] })).unwrap();
    let sections = |p: &mut Projects| -> Vec<(String, bool)> {
        run(p, "library.tags", json!({})).unwrap()["sections"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| (s["kind"].as_str().unwrap_or_default().to_string(), s["open"].as_bool().unwrap_or(true)))
            .collect()
    };
    assert_eq!(
        sections(&mut p),
        [("tags".to_string(), false), ("pinned".to_string(), true), ("library".to_string(), true)],
        "Tags stayed closed through the reorder"
    );
    // A second reorder, back over, keeps it closed still.
    run(&mut p, "library.setSections", json!({ "order": ["pinned", "library", "tags"] })).unwrap();
    assert_eq!(sections(&mut p).last().map(|s| s.1), Some(false), "Tags closed through both reorders");
    let _ = std::fs::remove_dir_all(&settings);
}

// A `limit` and a make in the one command both run: the limit does not stand over the
// tag's own work ([#1254] review: `done` is the none-of-them marker, not the one-only).
#[test]
fn set_tag_runs_the_limit_and_the_make_together() {
    let settings = temp("set-tag-limit-settings");
    let mut p = projects(&settings);
    // The seven first tags would stop a lower limit; a raise makes both halves.
    run(&mut p, "library.setTag", json!({ "limit": 10, "name": "Warm" })).unwrap();
    let tags = run(&mut p, "library.tags", json!({})).unwrap();
    assert_eq!(tags["limit"], json!(10), "the limit set");
    assert!(tags["tags"].as_array().unwrap().iter().any(|t| t["name"] == json!("Warm")), "the tag made too");
    // The id's own keeps its place beside the limit.
    run(&mut p, "library.setTag", json!({ "limit": 11, "id": "#1", "color": "#123456" })).unwrap();
    let tags = run(&mut p, "library.tags", json!({})).unwrap();
    assert_eq!(tags["limit"], json!(11));
    assert_eq!(tags["tags"][0]["color"], json!("#123456"), "the id's colour with the limit");
    // The one command does not go half in: a make that fails leaves the limit what it
    // was ([#1254] review). A top-level folder named as the make's name is the failing
    // make.
    run(&mut p, "library.addTagFolder", json!({ "name": "Spare" })).unwrap();
    assert!(run(&mut p, "library.setTag", json!({ "limit": 12, "name": "Spare" })).is_err());
    let tags = run(&mut p, "library.tags", json!({})).unwrap();
    assert_eq!(tags["limit"], json!(11), "the limit stayed with the failed make");
    assert!(!tags["tags"].as_array().unwrap().iter().any(|t| t["name"] == json!("Spare")));
    // The id's own too: a colour that is not one changes nothing of the limit's.
    assert!(run(&mut p, "library.setTag", json!({ "limit": 12, "id": "#1", "color": "nope" })).is_err());
    let tags = run(&mut p, "library.tags", json!({})).unwrap();
    assert_eq!(tags["limit"], json!(11), "the limit stayed");
    assert_eq!(tags["tags"][0]["color"], json!("#123456"), "the colour stayed");
    // The nothing form still refuses.
    assert!(run(&mut p, "library.setTag", json!({})).is_err());
    // The one command, whole: a lowered limit the made tags would not carry refuses with
    // nothing set or made ([#1254] review).
    run(&mut p, "library.addTagFolder", json!({ "name": "Next" })).unwrap();
    let before = run(&mut p, "library.tags", json!({})).unwrap();
    let count = before["tags"].as_array().unwrap().len();
    assert!(run(&mut p, "library.setTag", json!({ "name": "Stand", "limit": count })).is_err());
    let after = run(&mut p, "library.tags", json!({})).unwrap();
    assert_eq!(after["limit"], json!(11), "the limit the make would break came back");
    assert!(!after["tags"].as_array().unwrap().iter().any(|t| t["name"] == json!("Stand")));
    assert_eq!(after["tags"].as_array().unwrap().len(), count);
    let _ = std::fs::remove_dir_all(&settings);
}

// A folder name that more than one folder has refuses a name-based command: the forest
// order would pick one, and a reordering can change which ([#1254] review).
#[test]
fn an_ambiguous_folder_name_refuses_a_name_command() {
    let settings = temp("ambiguous-folder-settings");
    let mut p = projects(&settings);
    run(&mut p, "library.addTagFolder", json!({ "name": "Box" })).unwrap();
    run(&mut p, "library.addTagFolder", json!({ "name": "Kick" })).unwrap();
    let folders = run(&mut p, "library.tags", json!({})).unwrap();
    let one = folders["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == json!("Box"))
        .map(|f| f["id"].as_u64().unwrap_or(0))
        .unwrap_or(0);
    run(&mut p, "library.addTagFolder", json!({ "name": "Kick", "parent": format!("#{one}") })).unwrap();
    // Two Kick-named folders now: a name-based rename refuses.
    let refused = run(&mut p, "library.renameTagFolder", json!({ "folder": "Kick", "rename": "Other" }));
    assert!(refused.is_err(), "{refused:?}");
    assert!(matches!(&refused, Err(e) if e.to_string().contains("2 tag folders named")), "{refused:?}");
    // The commands by id still take their folder.
    let folders = run(&mut p, "library.tags", json!({})).unwrap();
    let kick = folders["folders"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == json!("Kick"))
        .map(|f| f["id"].as_u64().unwrap_or(0))
        .unwrap_or(0);
    run(&mut p, "library.renameTagFolder", json!({ "folder": format!("#{kick}"), "rename": "Drums" })).unwrap();
    let folders = run(&mut p, "library.tags", json!({})).unwrap();
    assert!(folders["folders"].as_array().unwrap().iter().any(|f| f["name"] == json!("Drums")));
    // A single-named folder still works by name, and the ambiguity lifts when it is one's own again.
    run(&mut p, "library.renameTagFolder", json!({ "folder": "Drums", "rename": "Toe" })).unwrap();
    let folders = run(&mut p, "library.tags", json!({})).unwrap();
    assert!(folders["folders"].as_array().unwrap().iter().any(|f| f["name"] == json!("Toe")));
    let _ = std::fs::remove_dir_all(&settings);
}
