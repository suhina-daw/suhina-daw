// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The shipped scripts' unpack (docs/spec/shipped-scripts.md, #1676): every file lands in a
//! fresh `Scripts/` folder once, a file that is there is never touched, a deleted file comes
//! back, the manifest passes the store's own checks as it lands, and a folder that cannot be
//! written is said without stopping the start.

use super::*;

/// A fresh empty folder for one test; the same name is reused across runs and cleaned first.
fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("suhina-shipped-scripts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// How many files every shipped script carries, together.
fn file_count() -> usize {
    scripts().iter().map(|script| script.files.len()).sum()
}

/// A fresh folder receives every shipped file whole, and a second start writes nothing: from
/// there the files are the user's.
#[test]
fn a_fresh_folder_receives_every_file_once() {
    let dir = temp("fresh");
    let (written, warnings) = unpack(&dir);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(written, file_count(), "the first start writes every file");
    for script in scripts() {
        for (name, bytes) in script.files {
            let target = dir.join(script.id).join(name);
            assert!(target.is_file(), "{target:?} is not there after the unpack");
            assert_eq!(std::fs::read(&target).unwrap(), *bytes, "{name}'s copy is not the shipped bytes");
        }
    }
    let (again, warnings) = unpack(&dir);
    assert_eq!(again, 0, "a second start rewrote the user's files");
    assert!(warnings.is_empty(), "{warnings:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file the user carries is theirs: an edited file keeps its bytes while the missing ones
/// land beside it, and deleting a file unpacks the shipped version again.
#[test]
fn an_existing_file_is_never_overwritten() {
    let dir = temp("kept");
    let folder = dir.join("apc40-mkii");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("script.lua"), b"-- my own script\n").unwrap();
    let (written, warnings) = unpack(&dir);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(written, file_count() - 1, "the kept file is not written, the others are");
    assert_eq!(std::fs::read(folder.join("script.lua")).unwrap(), b"-- my own script\n", "the edit was replaced");
    // Deleting it unpacks the shipped version again, whole.
    std::fs::remove_file(folder.join("script.lua")).unwrap();
    let (again, warnings) = unpack(&dir);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(again, 1, "the deleted file comes back");
    let shipped = scripts()[0].files.iter().find(|(name, _)| *name == "script.lua").unwrap().1;
    assert_eq!(std::fs::read(folder.join("script.lua")).unwrap(), shipped);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A folder that cannot be written is said in the warnings, one line per script, and nothing
/// else happens: no panic, no partial file, and the next start tries again.
#[test]
fn a_blocked_folder_is_said_and_harmless() {
    let dir = temp("blocked");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Scripts"), b"a file where the folder should go").unwrap();
    let (written, warnings) = unpack(&dir.join("Scripts"));
    assert_eq!(written, 0);
    assert_eq!(warnings.len(), scripts().len(), "{warnings:?}");
    assert!(warnings.iter().all(|line| line.contains("tries again")), "{warnings:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// What the unpack writes passes the store's own checks — `Manifest::read` is the scan's
/// reader, and it hashes `script.lua` against the manifest's SHA-256 pin — so the folder is
/// one `extensions.add` would take. Suhina's own content states CC0-1.0, and the provenance
/// README rides beside the files.
#[test]
fn the_unpacked_folder_passes_the_stores_checks() {
    let dir = temp("manifest");
    let (_, warnings) = unpack(&dir);
    assert!(warnings.is_empty(), "{warnings:?}");
    for script in scripts() {
        let folder = dir.join(script.id);
        // The pin holds: the manifest's checksum is the embedded bytes', file by file.
        let (manifest, _) = crate::extensions::Manifest::read(&folder).unwrap();
        assert_eq!(manifest.id, script.id, "the manifest names the folder it unpacks into");
        assert_eq!(manifest.license, serde_json::json!("CC0-1.0"), "Suhina's own content");
        assert!(manifest.provides.iter().any(|entry| entry.is_surface()), "a surface program");
        assert_eq!(manifest.programs.len(), 1, "the one script, pinned");
        let readme = std::fs::read_to_string(folder.join("README.md")).unwrap();
        assert!(readme.contains("CC0-1.0"), "the README records the licence");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The pack folders and the embed hold the same files: a file added to `packs/<id>/` without
/// riding the executable — or the other way — fails here.
#[test]
fn every_file_in_a_pack_folder_is_embedded() {
    let packs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
    for script in scripts() {
        let folder = packs.join(script.id);
        let mut on_disk: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| !name.starts_with('.'))
            .collect();
        on_disk.sort();
        let mut embedded: Vec<String> = script.files.iter().map(|(name, _)| (*name).to_string()).collect();
        embedded.sort();
        assert_eq!(on_disk, embedded, "packs/{} and the embed hold different files", script.id);
    }
}

/// A fresh store receives every shipped script whole — the way `extensions.add` puts one
/// there — the store lists it as usable, and a second start installs nothing.
#[test]
fn a_fresh_store_receives_every_shipped_script_once() {
    let dir = temp("store");
    let mut projects = crate::Projects::default().with_extensions_root(&dir);
    let (installed, warnings) = install(&mut projects.extensions);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(installed, scripts().len());
    for script in scripts() {
        for (name, bytes) in script.files {
            let target = dir.join(script.id).join(name);
            assert!(target.is_file(), "{target:?} is not there after the install");
            assert_eq!(std::fs::read(&target).unwrap(), *bytes, "{name}'s copy is not the shipped bytes");
        }
    }
    // The store reads it, validated and usable.
    let listed = projects.execute("extensions.list", &serde_json::json!({})).unwrap();
    let entry = listed["extensions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "apc40-mkii")
        .expect("the shipped script is listed");
    assert!(entry["reason"].is_null(), "{entry}");
    let (again, warnings) = install(&mut projects.extensions);
    assert_eq!(again, 0, "a second start installed again");
    assert!(warnings.is_empty(), "{warnings:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A store copy the user edited is theirs: a later start never overwrites it, and a copy the
/// record does not recognize is not an install of ours at all.
#[test]
fn an_edited_store_copy_is_left_alone() {
    let dir = temp("store-edited");
    let mut extensions = Extensions::new();
    extensions.force_root(&dir);
    install(&mut extensions);
    std::fs::write(dir.join("apc40-mkii/script.lua"), b"-- my own\n").unwrap();
    let (installed, warnings) = install(&mut extensions);
    assert_eq!(installed, 0);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(std::fs::read(dir.join("apc40-mkii/script.lua")).unwrap(), b"-- my own\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A store as an earlier Suhina left it — the record names the older script and the copy is
/// that, untouched — is replaced whole when the app carries a newer version, and the record
/// follows it.
#[test]
fn a_newer_shipped_version_replaces_an_untouched_copy() {
    let dir = temp("store-update");
    let shipped = &scripts()[0];
    let old_script = b"-- the older shipped script\n";
    let mut files: BTreeMap<String, String> =
        shipped.files.iter().map(|(name, bytes)| ((*name).to_string(), digest(bytes))).collect();
    files.insert("script.lua".into(), digest(old_script));
    let mut record = Record::default();
    record.scripts.insert(shipped.id.into(), RecordEntry { files, removed: false });
    std::fs::create_dir_all(dir.join(shipped.id)).unwrap();
    for &(name, bytes) in shipped.files {
        let bytes = if name == "script.lua" { old_script.as_slice() } else { bytes };
        std::fs::write(dir.join(shipped.id).join(name), bytes).unwrap();
    }
    record.write(&dir).unwrap();

    let mut extensions = Extensions::new();
    extensions.force_root(&dir);
    let (installed, warnings) = install(&mut extensions);
    assert_eq!(installed, 1, "the newer version did not replace the old copy");
    assert!(warnings.is_empty(), "{warnings:?}");
    let new_script = shipped.files.iter().find(|(name, _)| *name == "script.lua").unwrap().1;
    assert_eq!(std::fs::read(dir.join(shipped.id).join("script.lua")).unwrap(), new_script);
    let record = Record::read(&dir);
    assert_eq!(record.scripts[shipped.id].files["script.lua"], digest(new_script));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A copy the user changed is left alone even when the app carries a newer version: the
/// store's copy is replaced only while it still carries exactly the recorded content.
#[test]
fn a_newer_shipped_version_leaves_an_edited_copy_alone() {
    let dir = temp("store-update-edited");
    let shipped = &scripts()[0];
    let mut files: BTreeMap<String, String> =
        shipped.files.iter().map(|(name, bytes)| ((*name).to_string(), digest(bytes))).collect();
    files.insert("script.lua".into(), digest(b"-- the older shipped script\n"));
    let mut record = Record::default();
    record.scripts.insert(shipped.id.into(), RecordEntry { files, removed: false });
    std::fs::create_dir_all(dir.join(shipped.id)).unwrap();
    std::fs::write(dir.join(shipped.id).join("script.lua"), b"-- my own\n").unwrap();
    record.write(&dir).unwrap();

    let mut extensions = Extensions::new();
    extensions.force_root(&dir);
    let (installed, warnings) = install(&mut extensions);
    assert_eq!(installed, 0, "the user's copy was replaced");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(std::fs::read(dir.join(shipped.id).join("script.lua")).unwrap(), b"-- my own\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `extensions.remove` on a shipped script stops it and forgets it: later starts do not
/// install it again, and adding the library's folder back is the way to want it again.
#[test]
fn a_removed_shipped_script_stays_removed() {
    let dir = temp("store-removed");
    let source = temp("store-removed-src");
    let mut projects = crate::Projects::default().with_extensions_root(&dir);
    install(&mut projects.extensions);
    projects.execute("extensions.remove", &serde_json::json!({ "id": "apc40-mkii" })).unwrap();
    assert!(!dir.join("apc40-mkii").exists(), "removed means the store's copy goes");
    let (installed, warnings) = install(&mut projects.extensions);
    assert_eq!(installed, 0, "a removed shipped script came back");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(!dir.join("apc40-mkii").exists());
    // Adding it back from the library's folder — the unpacked copy — is the way to want it
    // again, and the app's own install leaves that copy alone from there.
    let (_, warnings) = unpack(&source);
    assert!(warnings.is_empty(), "{warnings:?}");
    projects
        .execute("extensions.add", &serde_json::json!({ "folder": source.join("apc40-mkii").display().to_string() }))
        .unwrap();
    assert!(dir.join("apc40-mkii").exists(), "the added folder is in the store");
    let (installed, warnings) = install(&mut projects.extensions);
    assert_eq!(installed, 0, "the added copy was replaced");
    assert!(warnings.is_empty(), "{warnings:?}");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&source);
}

/// A store that already holds the id — the user's own extension, added before Suhina shipped
/// one — is theirs: the install never replaces it.
#[test]
fn a_store_that_already_holds_the_id_is_left_alone() {
    let dir = temp("store-theirs");
    std::fs::create_dir_all(dir.join("apc40-mkii")).unwrap();
    std::fs::write(dir.join("apc40-mkii/script.lua"), b"-- mine\n").unwrap();
    let mut extensions = Extensions::new();
    extensions.force_root(&dir);
    let (installed, warnings) = install(&mut extensions);
    assert_eq!(installed, 0);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(std::fs::read(dir.join("apc40-mkii/script.lua")).unwrap(), b"-- mine\n");
    for &(name, _) in scripts()[0].files {
        if name != "script.lua" {
            assert!(!dir.join("apc40-mkii").join(name).exists(), "{name} was installed over the user's");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A store folder that cannot be written is said, one line per script, and the start goes on:
/// nothing panics, and the next start tries again.
#[test]
fn a_blocked_store_is_said_and_harmless() {
    let dir = temp("store-blocked");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("extensions"), b"a file where the store should go").unwrap();
    let mut extensions = Extensions::new();
    extensions.force_root(&dir.join("extensions"));
    let (installed, warnings) = install(&mut extensions);
    assert_eq!(installed, 0);
    assert_eq!(warnings.len(), scripts().len(), "{warnings:?}");
    assert!(warnings.iter().all(|line| line.contains("tries again")), "{warnings:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A corrupted record reads as empty — it is Suhina's own state — and a fresh install heals
/// it: the script lands and the record is written valid.
#[test]
fn a_corrupted_record_is_read_as_empty() {
    let dir = temp("store-record");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(".shipped-scripts.json"), b"not a record").unwrap();
    let mut extensions = Extensions::new();
    extensions.force_root(&dir);
    let (installed, warnings) = install(&mut extensions);
    assert_eq!(installed, scripts().len());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(Record::read(&dir).scripts.contains_key("apc40-mkii"));
    let _ = std::fs::remove_dir_all(&dir);
}
