// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `plugin.*` commands: where scans look, what they found, and how the commands sit next
//! to the project commands. Scanning itself runs the plug-in host, so it is tested with the
//! host in `crates/plugin-host/tests/scan.rs`.
#![allow(clippy::unwrap_used)] // test helpers outside #[test] functions

use std::path::PathBuf;

use serde_json::{Value, json};
use suhina_model::Project;
use suhina_plugins::library::Library;
use suhina_session::{CommandKind, ProjectId, Projects, Session, plugin_commands};

use crate::support::run;

/// Projects whose plug-in library lives in a fresh folder of its own.
fn projects(name: &str) -> (Projects, PathBuf) {
    let dir = std::env::temp_dir().join(format!("suhina-plugin-commands-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("plugins")).unwrap();
    let library = Library::new(Some(dir.join("settings")), dir.join("no-such-host"));
    (Projects::with_library(library), dir)
}

#[test]
fn folders_are_added_listed_removed_and_kept() {
    let (mut p, dir) = projects("folders");
    let folder = dir.join("plugins");
    let added = run(&mut p, "plugin.addFolder", json!({ "folder": folder })).unwrap();
    assert_eq!(added["added"], true);
    assert_eq!(added["folders"].as_array().unwrap().len(), 1);
    let again = run(&mut p, "plugin.addFolder", json!({ "folder": folder })).unwrap();
    assert_eq!(again["added"], false);
    let listed = run(&mut p, "plugin.folders", json!({})).unwrap();
    assert_eq!(listed["added"].as_array().unwrap().len(), 1);
    assert!(listed["standard"].is_array());

    // A new start reads the saved folders back.
    let mut later = Projects::with_library(Library::new(Some(dir.join("settings")), dir.join("no-such-host")));
    let listed = run(&mut later, "plugin.folders", json!({})).unwrap();
    assert_eq!(listed["added"].as_array().unwrap().len(), 1);

    // Each folder with its format and whether it is on; switching it off keeps it listed.
    let entry = |listed: &Value| {
        listed["folders"].as_array().unwrap().iter().find(|f| f["standard"] == false).cloned().unwrap()
    };
    assert_eq!((entry(&listed)["format"].clone(), entry(&listed)["enabled"].clone()), (json!("any"), json!(true)));
    let off = run(&mut later, "plugin.setFolderEnabled", json!({ "folder": folder, "enabled": false })).unwrap();
    assert_eq!(off["changed"], true);
    let listed = run(&mut later, "plugin.folders", json!({})).unwrap();
    assert_eq!(entry(&listed)["enabled"], false);
    // Added again for CLAP only, it keeps its place with the new format.
    let clap = run(&mut later, "plugin.addFolder", json!({ "folder": folder, "format": "clap" })).unwrap();
    assert_eq!(clap["added"], true);
    let listed = run(&mut later, "plugin.folders", json!({})).unwrap();
    assert_eq!(entry(&listed)["format"], "clap");

    let removed = run(&mut later, "plugin.removeFolder", json!({ "folder": folder })).unwrap();
    assert_eq!((removed["removed"].clone(), removed["folders"].clone()), (json!(true), json!([])));
    let not_there = run(&mut later, "plugin.removeFolder", json!({ "folder": folder })).unwrap();
    assert_eq!(not_there["removed"], false);
}

#[test]
fn bad_folders_and_parameters_are_errors() {
    let (mut p, dir) = projects("bad");
    let missing = run(&mut p, "plugin.addFolder", json!({ "folder": dir.join("nope") })).unwrap_err();
    assert!(missing.contains("not a folder"), "{missing}");
    assert!(run(&mut p, "plugin.addFolder", json!({})).is_err());
    assert!(run(&mut p, "plugin.addFolder", json!({ "folder": 5 })).is_err());
    assert!(run(&mut p, "plugin.scan", json!({ "all": "yes" })).is_err());
    assert!(run(&mut p, "plugin.list", json!({ "extra": 1 })).is_err());
    let folder = dir.join("plugins");
    // An unknown format is an error, and so is a folder operation on one that is not there.
    assert!(run(&mut p, "plugin.addFolder", json!({ "folder": folder, "format": "dll" })).is_err());
    assert!(run(&mut p, "plugin.setFolderEnabled", json!({ "folder": folder, "enabled": false })).is_err());
    assert!(run(&mut p, "plugin.setFolderEnabled", json!({ "folder": folder })).is_err());
    // The vst2 format is a folder like the others now (ADR 0022).
    let vst2 = run(&mut p, "plugin.addFolder", json!({ "folder": folder, "format": "vst2" })).unwrap();
    assert_eq!(vst2["added"], true);
    assert!(run(&mut p, "plugin.setEnabled", json!({ "plugin": "nothing", "enabled": false })).is_err());
    assert!(run(&mut p, "plugin.scanFile", json!({ "file": "" })).is_err());
}

#[test]
fn nothing_scanned_lists_nothing_and_a_missing_host_is_an_error() {
    let (mut p, _) = projects("empty");
    assert_eq!(
        run(&mut p, "plugin.list", json!({})).unwrap(),
        json!({ "plugins": [], "failed": [], "incompatible": [] })
    );
    let error = run(&mut p, "plugin.scan", json!({})).unwrap_err();
    assert!(error.contains("plug-in host") && error.contains("missing"), "{error}");
}

#[test]
fn files_that_hold_no_plugins_are_listed_only_when_asked() {
    let dir = std::env::temp_dir().join(format!("suhina-plugin-commands-ignored-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("settings")).unwrap();
    let catalog = r#"{"version":3,"entries":[
        {"path":"/vst3/Empty.vst3","modified":1,"size":0,"tried":5,"plugins":[]},
        {"path":"/vst3/Broken.vst3","modified":1,"size":9,"tried":5,"failed":"the plug-in has no factory"}]}"#;
    std::fs::write(dir.join("settings").join("plugin-catalog.json"), catalog).unwrap();
    let mut p = Projects::with_library(Library::new(Some(dir.join("settings")), dir.join("no-such-host")));
    let file = |path: &str| std::path::Path::new(path).display().to_string();

    let listed = run(&mut p, "plugin.list", json!({})).unwrap();
    assert_eq!(
        listed["failed"],
        json!([{ "file": file("/vst3/Broken.vst3"), "reason": "the plug-in has no factory" }])
    );
    assert!(listed.get("ignored").is_none(), "{listed}");
    let all = run(&mut p, "plugin.list", json!({ "ignored": true })).unwrap();
    assert_eq!(all["ignored"], json!([{ "file": file("/vst3/Empty.vst3"), "reason": "the file holds no plug-ins" }]));
    assert_eq!(all["failed"], listed["failed"]);
    assert!(run(&mut p, "plugin.list", json!({ "ignored": "yes" })).is_err());
}

#[test]
fn plugins_are_switched_off_and_on_and_files_for_another_processor_are_listed() {
    let dir = std::env::temp_dir().join(format!("suhina-plugin-commands-enabled-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("settings")).unwrap();
    let catalog = r#"{"version":3,"entries":[
        {"path":"/clap/Echo.clap","modified":1,"size":9,"tried":5,"plugins":[
            {"id":"com.example.echo","name":"Echo","vendor":"Example","version":"1","features":["audio-effect"]}]},
        {"path":"/vst3/Old.vst3","modified":1,"size":9,"tried":5,"incompatible":"built for x86_64 only; Suhina runs on arm64"}]}"#;
    std::fs::write(dir.join("settings").join("plugin-catalog.json"), catalog).unwrap();
    let mut p = Projects::with_library(Library::new(Some(dir.join("settings")), dir.join("no-such-host")));

    let listed = run(&mut p, "plugin.list", json!({})).unwrap();
    assert_eq!(listed["plugins"][0]["enabled"], true);
    assert_eq!(listed["plugins"][0]["type"], "effect");
    assert_eq!(listed["incompatible"][0]["reason"], "built for x86_64 only; Suhina runs on arm64");
    assert_eq!(listed["failed"], json!([]), "built for another processor is not a failure");

    let off = run(&mut p, "plugin.setEnabled", json!({ "plugin": "Echo", "enabled": false })).unwrap();
    assert_eq!(off["enabled"], false);
    let listed = run(&mut p, "plugin.list", json!({})).unwrap();
    assert_eq!(listed["plugins"][0]["enabled"], false, "still listed, switched off");
    assert_eq!(p.plugin_env().catalog.plugins().count(), 0, "sessions do not see it");
    let spec = listed["plugins"][0]["file"].as_str().map(|f| format!("clap:{f}#com.example.echo")).unwrap();
    run(&mut p, "plugin.setEnabled", json!({ "plugin": spec, "enabled": true })).unwrap();
    assert_eq!(p.plugin_env().catalog.plugins().count(), 1);
}

#[test]
fn plugin_commands_belong_to_suhina_not_to_a_project() {
    assert!(plugin_commands().iter().all(|c| c.id.starts_with("plugin.") && c.kind == CommandKind::Plugins));
    let (mut p, _) = projects("kinds");
    let all = run(&mut p, "command.list", json!({})).unwrap();
    let kinds: Vec<(&str, &str)> = all
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| Some((c["id"].as_str()?, c["kind"].as_str()?)))
        .filter(|(id, _)| id.starts_with("plugin."))
        .collect();
    assert_eq!(kinds.len(), plugin_commands().len());
    assert!(kinds.iter().all(|(_, kind)| *kind == "plugins"));

    // They run without any open project, and refuse a target project.
    assert!(p.is_empty());
    assert!(run(&mut p, "plugin.folders", json!({})).is_ok());
    let id: ProjectId = p.add(Session::new(Project::default()), None).unwrap();
    let error = p.execute_in(Some(id), "plugin.folders", &json!({})).unwrap_err().message().to_owned();
    assert!(error.contains("not run on one project"), "{error}");
}

#[test]
fn a_projects_plugins_are_matched_to_the_installed_ones() {
    use suhina_model::{Channel, ChannelId, Device, DeviceKind};
    let (_, dir) = projects("check");
    // An installed VST3 plug-in that says it replaces the VST2 plug-in `Gain`.
    let settings = dir.join("settings");
    std::fs::create_dir_all(&settings).unwrap();
    let catalog = json!({ "version": 2, "entries": [{ "path": "/vst3/Gain.vst3", "modified": 1, "size": 1, "plugins": [{
        "id": "4C6F6F707765726B5465737447616931", "name": "Gain 3", "vendor": "Suhina", "version": "3",
        "features": ["Fx"], "replaces": ["5653544761696E6761696E0000000000"],
    }]}]});
    std::fs::write(settings.join("plugin-catalog.json"), catalog.to_string()).unwrap();
    let mut p = Projects::with_library(Library::new(Some(settings), dir.join("no-such-host")));

    let device = |name: &str, kind: DeviceKind| Device { state: Some(vec![1, 2, 3]), ..Device::new(name, kind) };
    let mut project = Project::default();
    project.channels.push(Channel {
        devices: vec![
            device("Gain_x64", DeviceKind::Vst2 { id: 0x4761_696E }),
            device("Gain 2", DeviceKind::Vst3 { id: "00000000000000000000000000000001".into() }),
            device("Missing", DeviceKind::Vst2 { id: 7 }),
            device("Utility", DeviceKind::Builtin("utility".into())),
        ],
        ..Channel::new(ChannelId(1), "Lead")
    });
    assert!(run(&mut p, "plugin.check", json!({})).unwrap_err().contains("no project"));
    let id = p.add(Session::new(project), None).unwrap();

    let checked = run(&mut p, "plugin.check", json!({ "project": id.0 })).unwrap();
    assert_eq!((checked["plugins"].clone(), checked["missing"].clone()), (json!(3), json!(1)));
    let devices = checked["devices"].as_array().unwrap();
    assert_eq!(devices[0]["format"], "vst2");
    assert_eq!(devices[0]["id"], "4761696E");
    assert_eq!(devices[0]["found"]["how"], "declared");
    assert_eq!(devices[0]["found"]["keeps_state"], true);
    assert_eq!(devices[1]["found"]["how"], "newer-version");
    assert_eq!(devices[1]["found"]["keeps_state"], false);
    assert_eq!(devices[1]["found"]["name"], "Gain 3");
    assert!(devices[2].get("found").is_none());
    assert_eq!(run(&mut p, "plugin.check", json!({})).unwrap(), checked);
    assert!(run(&mut p, "plugin.check", json!({ "project": 99 })).is_err());

    // Switched off, the plug-in is missing to the project, as sessions load it.
    run(&mut p, "plugin.setEnabled", json!({ "plugin": "Gain 3", "enabled": false })).unwrap();
    let off = run(&mut p, "plugin.check", json!({})).unwrap();
    assert_eq!((off["plugins"].clone(), off["missing"].clone()), (json!(3), json!(3)));
    run(&mut p, "plugin.setEnabled", json!({ "plugin": "Gain 3", "enabled": true })).unwrap();
    assert_eq!(run(&mut p, "plugin.check", json!({})).unwrap(), checked);
}

#[test]
fn hosting_is_chosen_per_plug_in_and_reaches_the_sessions() {
    use suhina_plugins::scan::{Catalog, PluginInfo};
    let (_, dir) = projects("hosting");
    let settings = dir.join("settings");
    std::fs::create_dir_all(&settings).unwrap();
    let mut catalog = Catalog::new();
    let verb = PluginInfo {
        path: PathBuf::from("Verb.vst3"),
        id: "ABCDEF0123456789ABCDEF0123456789".into(),
        name: "Verb".into(),
        vendor: "Vendor".into(),
        version: "1".into(),
        features: vec!["Fx".into()],
        replaces: Vec::new(),
        ara: false,
    };
    catalog.insert(verb.path.clone(), vec![verb.clone()]);
    std::fs::write(settings.join("plugin-catalog.json"), catalog.to_json()).unwrap();
    let mut p = Projects::with_library(Library::new(Some(settings.clone()), dir.join("no-such-host")));
    // By default every instance of a plug-in file shares a process.
    let group = |p: &mut Projects| (p.plugin_env().group)(&verb);
    assert_eq!(group(&mut p), Some("file:Verb.vst3".into()));
    assert_eq!(run(&mut p, "plugin.list", json!({})).unwrap()["plugins"][0]["hosting"], "binary");
    let set = run(&mut p, "plugin.setHosting", json!({ "plugin": "Verb", "hosting": "instance" })).unwrap();
    assert_eq!(set, json!({ "plugin": "Verb", "hosting": "instance" }));
    assert_eq!(group(&mut p), None, "a process of its own from its next start");
    run(&mut p, "plugin.setHosting", json!({ "plugin": "Verb", "hosting": "vendor" })).unwrap();
    assert_eq!(group(&mut p), Some("vendor:Vendor".into()));
    // Kept for the next start.
    let mut later = Projects::with_library(Library::new(Some(settings), dir.join("no-such-host")));
    assert_eq!(run(&mut later, "plugin.list", json!({})).unwrap()["plugins"][0]["hosting"], "vendor");
    assert!(run(&mut p, "plugin.setHosting", json!({ "plugin": "Verb", "hosting": "shared" })).is_err());
    assert!(run(&mut p, "plugin.setHosting", json!({ "plugin": "Nothing", "hosting": "binary" })).is_err());
}

#[test]
fn an_ara_plug_in_is_listed_as_such_and_never_hosted_alone() {
    use suhina_plugins::scan::{Catalog, PluginInfo};
    let (_, dir) = projects("ara-hosting");
    let settings = dir.join("settings");
    std::fs::create_dir_all(&settings).unwrap();
    let tune = PluginInfo {
        path: PathBuf::from("Tune.vst3"),
        id: "ABCDEF0123456789ABCDEF0123456780".into(),
        name: "Tune".into(),
        vendor: "Vendor".into(),
        version: "1".into(),
        features: vec!["Fx".into()],
        replaces: Vec::new(),
        ara: true,
    };
    let mut catalog = Catalog::new();
    catalog.insert(tune.path.clone(), vec![tune.clone()]);
    std::fs::write(settings.join("plugin-catalog.json"), catalog.to_json()).unwrap();
    let mut p = Projects::with_library(Library::new(Some(settings), dir.join("no-such-host")));
    assert_eq!(run(&mut p, "plugin.list", json!({})).unwrap()["plugins"][0]["ara"], true);
    let refused = run(&mut p, "plugin.setHosting", json!({ "plugin": "Tune", "hosting": "instance" })).unwrap_err();
    assert!(refused.contains("ARA"), "{refused}");
    assert_eq!(run(&mut p, "plugin.list", json!({})).unwrap()["plugins"][0]["hosting"], "binary");
    // The other choices stay open.
    run(&mut p, "plugin.setHosting", json!({ "plugin": "Tune", "hosting": "vendor" })).unwrap();
    assert_eq!((p.plugin_env().group)(&tune), Some("vendor:Vendor".into()));
}
