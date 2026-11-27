// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The extensions store, its manifest checks, and the `extensions.*` commands against real
//! folders; the supervisor's end-to-end tests are in [`programs`]'s own file.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] functions

use std::path::Path;

use serde_json::{Value, json};
use suhina_session::{Projects, extensions};

/// An extension folder under a temporary root, its manifest written and its script's SHA-256
/// in it: the store's happy case.
struct Fixture {
    root: std::path::PathBuf,
    id: String,
}

/// The SHA-256 of the script every fixture writes, so a manifest can be sealed before the
/// folder exists.
const SCRIPT: &str = "suhina.on_port_message(function() end)\n";

impl Fixture {
    /// Makes `id/` under a fresh root, writes `script.lua` and `LICENCE.txt`, and the manifest
    /// `build` makes (sealed against the script's bytes, so it always checks out).
    fn new(id: &str, build: impl FnOnce(&str) -> Value) -> Self {
        let root = std::env::temp_dir().join(format!(
            "suhina-extensions-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join(id)).expect("the folder is made");
        std::fs::write(root.join(id).join("script.lua"), SCRIPT).expect("the script is written");
        std::fs::write(root.join(id).join("LICENCE.txt"), "MIT").expect("the licence is written");
        std::fs::write(root.join(id).join("extension.json"), build(id).to_string()).expect("the manifest is written");
        Self { root, id: id.to_string() }
    }

    fn folder(&self) -> std::path::PathBuf {
        self.root.join(&self.id)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A manifest for a Lua surface program, sealed against `script`: the one field set the spec's
/// checks pass on.
fn manifest_for(id: &str, script: &str) -> Value {
    json!({
        "id": id,
        "name": "Test Pad",
        "version": "1.0",
        "provides": [{ "id": "surface", "label": "Test Pad surface" }],
        "license": "MIT",
        "license_source": "https://example.com/licence",
        "license_checked": "2026-11-13",
        "source": "user-provided",
        "protocol": 1,
        "programs": [{ "path": "script.lua", "sha256": sha(script.as_bytes()) }],
        "ports": ["Pad*"],
        "runner": "lua",
        "permissions": ["project.read"],
    })
}

fn sha(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// A `Projects` with the fixture as its whole extensions store.
fn projects_with(root: &Path) -> Projects {
    // `Projects::default` reads `SUHINA_EXTENSIONS` first; a root set here wins either way.
    Projects::default().with_extensions_root(root)
}

/// Runs one `extensions.*` command.
fn run(projects: &mut Projects, id: &str, params: Value) -> Result<Value, suhina_session::CommandError> {
    projects.execute(id, &params)
}

#[test]
fn a_manifest_with_its_script_sealed_round_trips() {
    let fixture = Fixture::new("test-pad", |id| manifest_for(id, SCRIPT));
    let (manifest, value) = extensions::Manifest::read(&fixture.folder()).expect("it checks out");
    assert_eq!(manifest.id, "test-pad");
    assert_eq!(manifest.licence_name(&value), "MIT");
    assert_eq!(manifest.script(), Some("script.lua"));
    assert!(manifest.provides.iter().any(|p| p.is_surface()));
}

#[test]
fn every_refusal_names_its_reason() {
    let refuses = |id: &str, patch: &dyn Fn(&mut Value)| {
        let fixture = Fixture::new(id, |id| manifest_for(id, SCRIPT));
        let mut manifest = manifest_for(&fixture.id, SCRIPT);
        patch(&mut manifest);
        std::fs::write(fixture.folder().join("extension.json"), manifest.to_string()).expect("written");
        let reason = extensions::Manifest::read(&fixture.folder()).expect_err("it is refused");
        (fixture, reason)
    };
    // No licence: a blocker, not a blank.
    let (_, reason) = refuses("no-licence", &|m| {
        m["license"] = json!(null);
    });
    assert!(reason.contains("license"), "{reason}");
    // A licence text file that is not UTF-8.
    let fixture = Fixture::new("binary-licence", |id| manifest_for(id, SCRIPT));
    std::fs::write(fixture.folder().join("licence.bin"), [0xff, 0xfe]).expect("written");
    let mut manifest = manifest_for(&fixture.id, SCRIPT);
    manifest["license"] = json!({ "name": "Custom", "text_file": "licence.bin" });
    std::fs::write(fixture.folder().join("extension.json"), manifest.to_string()).expect("written");
    let reason = extensions::Manifest::read(&fixture.folder()).expect_err("it is refused");
    assert!(reason.contains("UTF-8"), "{reason}");
    // A program file whose SHA-256 does not match.
    let (_, reason) = refuses("changed-script", &|m| {
        m["programs"][0]["sha256"] = json!("0".repeat(64));
    });
    assert!(reason.contains("changed"), "{reason}");
    // A script bigger than the cap: the scan reads it bounded, so it is refused by size
    // before any digest is compared (the manifest is still sealed to the small one).
    let fixture = Fixture::new("big-script", |id| manifest_for(id, SCRIPT));
    // surface::MAX_MESSAGE_BYTES plus one block.
    std::fs::write(fixture.folder().join("script.lua"), vec![b'x'; 1024 * 1024 + 1024]).expect("written");
    let reason = extensions::Manifest::read(&fixture.folder()).expect_err("it is refused");
    assert!(reason.contains("bigger than"), "{reason}");
    // A licence text file bigger than the cap.
    let fixture = Fixture::new("big-licence", |id| manifest_for(id, SCRIPT));
    std::fs::write(fixture.folder().join("licence.bin"), vec![b'a'; 256 * 1024 + 1024]).expect("written");
    let mut manifest = manifest_for(&fixture.id, SCRIPT);
    manifest["license"] = json!({ "name": "Custom", "text_file": "licence.bin" });
    std::fs::write(fixture.folder().join("extension.json"), manifest.to_string()).expect("written");
    let reason = extensions::Manifest::read(&fixture.folder()).expect_err("it is refused");
    assert!(reason.contains("bigger than"), "{reason}");
    // A program file that is gone: the manifest names one the folder does not hold.
    let fixture = Fixture::new("gone-script", |id| {
        let mut manifest = manifest_for(id, SCRIPT);
        manifest["programs"][0]["path"] = json!("vanished.lua");
        manifest
    });
    let reason = extensions::Manifest::read(&fixture.folder()).expect_err("it is refused");
    assert!(reason.contains("missing"), "{reason}");
    // A protocol newer than Suhina speaks.
    let (_, reason) = refuses("future-protocol", &|m| {
        m["protocol"] = json!(extensions::programs::PROTOCOL + 1);
    });
    assert!(reason.contains("newer"), "{reason}");
    // A runner Suhina does not ship.
    let (_, reason) = refuses("other-runner", &|m| {
        m["runner"] = json!("python");
    });
    assert!(reason.contains("ships"), "{reason}");
    // A path that leaves the folder.
    let (_, reason) = refuses("escaping-path", &|m| {
        m["programs"][0]["path"] = json!("../other/script.lua");
    });
    assert!(reason.contains("leaves"), "{reason}");
    // An absolute path.
    let (_, reason) = refuses("absolute-path", &|m| {
        m["programs"][0]["path"] = json!("/etc/passwd");
    });
    assert!(reason.contains("absolute"), "{reason}");
    // A link in the folder that resolves outside it: the manifest names the link, and the
    // file it reaches is not the folder's, so the read is refused either way.
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new("linked", |id| manifest_for(id, SCRIPT));
        let outside = std::env::temp_dir().join(format!("suhina-extensions-outside-{}.lua", std::process::id()));
        std::fs::write(&outside, SCRIPT).expect("written");
        std::fs::remove_file(fixture.folder().join("script.lua")).expect("removed");
        symlink(&outside, fixture.folder().join("script.lua")).expect("linked");
        let reason = extensions::Manifest::read(&fixture.folder()).expect_err("it is refused");
        assert!(reason.contains("folder"), "{reason}");
        // A link that stays within the folder is the folder's own content: it reads.
        let second = Fixture::new("linked-in", |id| manifest_for(id, SCRIPT));
        std::fs::remove_file(second.folder().join("script.lua")).expect("removed");
        symlink(second.folder().join("LICENCE.txt"), second.folder().join("script.lua")).expect("linked");
        // The digest is against the linked file's bytes: the manifest is sealed to the
        // script, not the licence, so the read is refused by digest, not by containment —
        // which is what proves the containment check let it through.
        let reason = extensions::Manifest::read(&second.folder()).expect_err("it is refused");
        assert!(reason.contains("changed"), "{reason}");
        let _ = std::fs::remove_file(&outside);
    }
    // A surface entry with a namespace, and a non-surface one without.
    let (_, reason) = refuses("surface-namespace", &|m| {
        m["provides"] = json!([{ "id": "surface", "label": "x", "namespace": "pad" }]);
    });
    assert!(reason.contains("namespace"), "{reason}");
    // Ports past the cap.
    let (_, reason) = refuses("many-ports", &|m| {
        m["ports"] = json!((0..65).map(|i| format!("Port {i}")).collect::<Vec<_>>());
    });
    assert!(reason.contains("at most"), "{reason}");
    // A bad id.
    let (_, reason) = refuses("Bad-Id", &|m| {
        m["id"] = json!("Bad-Id");
    });
    assert!(reason.contains("id"), "{reason}");
    // A bad licence_checked date.
    let (_, reason) = refuses("bad-date", &|m| {
        m["license_checked"] = json!("2026-11-13T10:00");
    });
    assert!(reason.contains("YYYY-MM-DD"), "{reason}");
    // An unknown permission.
    let (_, reason) = refuses("odd-permission", &|m| {
        m["permissions"] = json!(["everything"]);
    });
    assert!(reason.contains("not one Suhina knows"), "{reason}");
}

#[test]
fn the_caps_hold() {
    // A manifest of the cap's size passes, one byte more is refused.
    let fixture = Fixture::new("big-name", |id| manifest_for(id, SCRIPT));
    let mut manifest = manifest_for(&fixture.id, SCRIPT);
    manifest["name"] = json!("x".repeat(128));
    std::fs::write(fixture.folder().join("extension.json"), manifest.to_string()).expect("written");
    assert!(extensions::Manifest::read(&fixture.folder()).is_ok(), "128 characters pass");
    manifest["name"] = json!("x".repeat(129));
    std::fs::write(fixture.folder().join("extension.json"), manifest.to_string()).expect("written");
    let reason = extensions::Manifest::read(&fixture.folder()).expect_err("129 are refused");
    assert!(reason.contains("name"), "{reason}");
}

#[test]
fn add_list_info_remove_round_trip() {
    use crate::temp_dir::TempDir;
    let fixture = Fixture::new("round-trip", |id| manifest_for(id, SCRIPT));
    // A store of its own: the fixture only makes the folder `add` copies from. Both are the
    // tests' guard's, cleaned up while a failed test unwinds too.
    let root = TempDir::new("extensions-store");
    let source = TempDir::new("extensions-source");
    for entry in ["extension.json", "script.lua", "LICENCE.txt"] {
        std::fs::copy(fixture.folder().join(entry), source.join(entry)).expect("copied");
    }
    // The source manifest's script path is right; the SHA-256 was written for the fixture's
    // copy, which is the same bytes.
    let mut projects = projects_with(&root);
    let added = run(&mut projects, "extensions.add", json!({ "folder": source.display().to_string() })).expect("added");
    assert_eq!(added["id"], "round-trip");
    assert_eq!(added["license"], "MIT");
    assert_eq!(added["permissions"], json!(["project.read"]));

    let listed = run(&mut projects, "extensions.list", json!({})).expect("listed");
    let entry = listed["extensions"]
        .as_array()
        .expect("a list")
        .iter()
        .find(|e| e["id"] == "round-trip")
        .expect("the added one is listed")
        .clone();
    assert_eq!(entry["state"], "available");
    assert_eq!(entry["name"], "Test Pad");
    assert_eq!(entry["provides"], json!(["surface"]));

    let info = run(&mut projects, "extensions.info", json!({ "id": "round-trip" })).expect("info");
    assert_eq!(info["manifest"]["name"], "Test Pad");
    assert_eq!(info["licenseText"], "MIT");
    assert_eq!(info["state"], "available");

    // A second add of the same id is refused while it is in the store, whatever folder it
    // comes from.
    let again = run(&mut projects, "extensions.add", json!({ "folder": source.display().to_string() }));
    assert!(again.is_err(), "an id already in the store is refused");

    let removed = run(&mut projects, "extensions.remove", json!({ "id": "round-trip" })).expect("removed");
    assert_eq!(removed["removed"], true);
    let listed = run(&mut projects, "extensions.list", json!({})).expect("listed");
    assert!(
        !listed["extensions"].as_array().expect("a list").iter().any(|e| e["id"] == "round-trip"),
        "the removed one is not listed"
    );
}

#[test]
fn a_failed_add_leaves_the_destination_clear() {
    let fixture = Fixture::new("retry", |id| manifest_for(id, SCRIPT));
    // A store of its own, and a source whose folder nests past the depth limit: the copy
    // fails partway.
    let root = crate::temp_dir::TempDir::new("extensions-retry-store");
    let source = crate::temp_dir::TempDir::new("extensions-retry-source");
    for entry in ["extension.json", "script.lua", "LICENCE.txt"] {
        std::fs::copy(fixture.folder().join(entry), source.join(entry)).expect("copied");
    }
    let mut deep = source.to_path_buf();
    for level in 0..9 {
        deep = deep.join(format!("d{level}"));
    }
    std::fs::create_dir_all(&deep).expect("made");
    std::fs::write(deep.join("last"), "x").expect("written");
    let mut projects = projects_with(&root);
    let error =
        run(&mut projects, "extensions.add", json!({ "folder": source.display().to_string() })).expect_err("refused");
    assert!(error.message().contains("deeper"), "{}", error.message());
    assert!(!root.join("retry").exists(), "the destination is clear: the staging folder was published nowhere");
    // The source fixed, the add works: nothing of the failed one blocks it.
    let _ = std::fs::remove_dir_all(source.join("d0"));
    let added = run(&mut projects, "extensions.add", json!({ "folder": source.display().to_string() })).expect("added");
    assert_eq!(added["id"], "retry");
}

#[test]
fn adding_refuses_a_folder_over_the_byte_or_file_cap_cleanly() {
    let fixture = Fixture::new("capped", |id| manifest_for(id, SCRIPT));
    let root = crate::temp_dir::TempDir::new("extensions-capped-store");
    let source = crate::temp_dir::TempDir::new("extensions-capped-source");
    for entry in ["extension.json", "script.lua", "LICENCE.txt"] {
        std::fs::copy(fixture.folder().join(entry), source.join(entry)).expect("copied");
    }
    // A sparse file says it is over the byte cap without taking the bytes on disk.
    let payload = std::fs::File::create(source.join("payload.bin")).expect("made");
    payload.set_len(256 * 1024 * 1024 + 1).expect("sized");
    let mut projects = projects_with(&root);
    let error =
        run(&mut projects, "extensions.add", json!({ "folder": source.display().to_string() })).expect_err("refused");
    assert!(error.message().contains("256 MiB"), "{}", error.message());
    assert!(!root.join("capped").exists(), "a refused copy leaves no destination");

    // The same for a folder with one file too many.
    std::fs::remove_file(source.join("payload.bin")).expect("removed");
    for index in 0..1025 {
        std::fs::File::create(source.join(format!("file-{index}"))).expect("made");
    }
    let error =
        run(&mut projects, "extensions.add", json!({ "folder": source.display().to_string() })).expect_err("refused");
    assert!(error.message().contains("1024 files"), "{}", error.message());
    assert!(!root.join("capped").exists(), "a refused copy leaves no destination");
}

#[test]
fn a_refused_extension_is_listed_with_its_reason() {
    let fixture = Fixture::new("broken", |id| manifest_for(id, SCRIPT));
    std::fs::write(fixture.folder().join("extension.json"), "{ not json").expect("written");
    let mut projects = projects_with(&fixture.root);
    let listed = run(&mut projects, "extensions.list", json!({})).expect("listed");
    let entry = listed["extensions"]
        .as_array()
        .expect("a list")
        .iter()
        .find(|e| e["id"] == "broken")
        .expect("listed anyway")
        .clone();
    assert_eq!(entry["state"], "refused");
    assert!(entry["reason"].as_str().expect("a reason").contains("JSON"), "{}", entry["reason"]);
    // And `extensions.restart` says why there is nothing to run.
    let error = run(&mut projects, "extensions.restart", json!({ "id": "broken" })).expect_err("refused");
    assert!(error.message().contains("manifest"), "{}", error.message());
}

#[test]
fn restart_refuses_what_is_not_a_surface_program() {
    let fixture = Fixture::new("not-surface", |id| manifest_for(id, SCRIPT));
    let mut manifest = manifest_for(&fixture.id, SCRIPT);
    manifest["provides"] = json!([{ "id": "separation", "label": "Separate stems", "namespace": "separation" }]);
    manifest.as_object_mut().expect("an object").remove("runner");
    manifest.as_object_mut().expect("an object").remove("ports");
    std::fs::write(fixture.folder().join("extension.json"), manifest.to_string()).expect("written");
    let mut projects = projects_with(&fixture.root);
    let error = run(&mut projects, "extensions.restart", json!({ "id": "not-surface" })).expect_err("refused");
    assert!(error.message().contains("not a surface program"), "{}", error.message());
}
