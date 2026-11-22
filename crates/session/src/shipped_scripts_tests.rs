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
