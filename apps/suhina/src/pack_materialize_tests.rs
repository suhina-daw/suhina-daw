// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

/// A start in an empty library writes every pack file once, and a second start writes
/// nothing: the files are the user's from there.
#[test]
fn an_empty_library_receives_the_pack_once() {
    let dir = std::env::temp_dir().join(format!("suhina-pack-unpack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let written = unpack_pack(&dir);
    assert!(written > 0, "the pack unpacks at least one file");
    for (library_folder, pack_folder, name, bytes) in suhina_device_registry::pack_files_flat()
        .into_iter()
        .map(|(_, library_folder, pack_folder, name, bytes)| (library_folder, pack_folder, name, bytes))
    {
        let target = dir.join(library_folder).join(pack_folder).join(name);
        assert!(target.is_file(), "{target:?} is not there after unpack_pack");
        assert_eq!(std::fs::read(&target).unwrap(), bytes, "{name}'s copy is not the pack's bytes");
    }
    // The provenance and licence records land beside the files ([ADR 0036] › 6): the
    // README in both Suhina folders, the two licence texts beside the IRs they cover.
    for (library_folder, pack_folder, name, bytes) in suhina_device_registry::pack_records() {
        let target = dir.join(library_folder).join(pack_folder).join(name);
        assert!(target.is_file(), "{target:?} is not there after unpack_pack");
        assert_eq!(std::fs::read(&target).unwrap(), bytes, "{name}'s copy is not the record's bytes");
    }
    let again = unpack_pack(&dir);
    assert_eq!(again, 0, "a second start overwrote or rewrote the user's files");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An old layout moves into the sounds folders: a pack moves as a pack, loose files move
/// by name, an emptied old folder is removed, and the warnings carry the line.
#[test]
fn an_old_library_moves_into_the_sounds_folders() {
    let dir = std::env::temp_dir().join(format!("suhina-pack-migrate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let old_models = dir.join("NAM models");
    std::fs::create_dir_all(old_models.join("Bright Pack")).unwrap();
    std::fs::write(old_models.join("Bright Pack/One.nam"), b"one").unwrap();
    std::fs::write(old_models.join("Loose Two.nam"), b"two").unwrap();
    let old_irs = dir.join("IRs");
    std::fs::create_dir_all(&old_irs).unwrap();
    std::fs::write(old_irs.join("Dark.wav"), b"three").unwrap();

    let warnings = migrate_old_folders(&dir, &dir);
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings.iter().all(|line| line.contains("moved into the library")));

    let models = dir.join("Amp models");
    assert_eq!(std::fs::read(models.join("Bright Pack/One.nam")).unwrap(), b"one");
    assert_eq!(std::fs::read(models.join("Loose Two.nam")).unwrap(), b"two");
    assert_eq!(std::fs::read(dir.join("Cabinets/Dark.wav")).unwrap(), b"three");
    assert!(!old_models.exists(), "the moved-empty old folder stays behind");
    assert!(!old_irs.exists(), "the moved-empty old folder stays behind");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A name the new tree already holds is never overwritten: that file stays in the old
/// folder, the old folder stays, and the warning says what was kept.
#[test]
fn a_name_the_new_tree_holds_is_never_overwritten() {
    let dir = std::env::temp_dir().join(format!("suhina-pack-keep-old-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let old_models = dir.join("NAM models");
    std::fs::create_dir_all(&old_models).unwrap();
    std::fs::write(old_models.join("Clash.nam"), b"the old bytes").unwrap();
    let models = dir.join("Amp models");
    std::fs::create_dir_all(&models).unwrap();
    std::fs::write(models.join("Clash.nam"), b"the new bytes").unwrap();

    let warnings = migrate_old_folders(&dir, &dir);
    assert_eq!(std::fs::read(models.join("Clash.nam")).unwrap(), b"the new bytes", "overwritten");
    assert_eq!(std::fs::read(old_models.join("Clash.nam")).unwrap(), b"the old bytes", "moved over");
    assert!(old_models.exists(), "a folder with leftovers is not removed");
    assert!(warnings.iter().all(|line| line.contains("kept")), "{warnings:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A folder that lands only in part is reported as the partial move it is: the warning
/// counts the files that moved and the ones kept, and a kept file stays whole in the old
/// tree ([ADR 0036] › 4, the old folder left whole when a name would be written over).
#[test]
fn a_partial_folder_move_is_reported_with_its_kept_files() {
    let dir = std::env::temp_dir().join(format!("suhina-pack-partial-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let old_pack = dir.join("NAM models").join("My Amps");
    std::fs::create_dir_all(&old_pack).unwrap();
    std::fs::write(old_pack.join("Moved.nam"), b"moved bytes").unwrap();
    std::fs::write(old_pack.join("Clash.nam"), b"the old bytes").unwrap();
    let new_pack = dir.join("Amp models").join("My Amps");
    std::fs::create_dir_all(&new_pack).unwrap();
    std::fs::write(new_pack.join("Clash.nam"), b"the new bytes").unwrap();

    let warnings = migrate_old_folders(&dir, &dir);
    assert_eq!(std::fs::read(new_pack.join("Moved.nam")).unwrap(), b"moved bytes", "the movable file went");
    assert_eq!(
        std::fs::read(new_pack.join("Clash.nam")).unwrap(),
        b"the new bytes",
        "the kept name never written over"
    );
    assert_eq!(std::fs::read(old_pack.join("Clash.nam")).unwrap(), b"the old bytes", "the kept file stays whole");
    assert!(
        warnings
            .iter()
            .any(|line| line.contains("mostly moved") && line.contains("1 file(s)") && line.contains("1 kept")),
        "the partial move names both counts: {warnings:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// After the move the unpack sees the carried files: a pack carried over from the old
/// folders skips, and only what is still missing (the own model) writes.
#[test]
fn the_pack_unpack_runs_after_the_move() {
    let dir = std::env::temp_dir().join(format!("suhina-pack-after-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (_, library_folder, pack, name, bytes) in suhina_device_registry::pack_files_flat() {
        let old_folder = if library_folder == "Amp models" { "NAM models" } else { "IRs" };
        let folder = dir.join(old_folder).join(pack);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(name), bytes).unwrap();
    }
    migrate_old_folders(&dir, &dir);
    // The records the move does not carry (they ride the unpack alone) are carried here
    // too, so the count below says nothing over the carried pack was written.
    for (library_folder, pack_folder, name, bytes) in suhina_device_registry::pack_records() {
        let pack_over = dir.join(library_folder).join(pack_folder);
        std::fs::create_dir_all(&pack_over).unwrap();
        std::fs::write(pack_over.join(name), bytes).unwrap();
    }
    let written = unpack_pack(&dir);
    assert_eq!(written, 0, "the move carries the pack: the unpack writes nothing over it");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A move the filesystem refuses is never silent: the file stays in the old folder and
/// the warning says so with its own count, not as a kept name ([ADR 0036] › 4, the move
/// is told in a start-up warning). Unix only: Windows' read-only flag does not hold a
/// directory's children back, so the refusal cannot be staged there this way.
#[cfg(unix)]
#[test]
fn a_refused_move_is_reported_as_stuck() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("suhina-pack-stuck-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let old_models = dir.join("NAM models");
    std::fs::create_dir_all(&old_models).unwrap();
    std::fs::write(old_models.join("One.nam"), b"one").unwrap();
    let fresh = dir.join("Amp models");
    std::fs::create_dir_all(&fresh).unwrap();
    std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o555)).unwrap();

    let warnings = migrate_old_folders(&dir, &dir);
    std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        warnings.iter().any(|line| line.contains("could not move") && line.contains("1 file(s)")),
        "the refused move names its count: {warnings:?}"
    );
    assert_eq!(std::fs::read(old_models.join("One.nam")).unwrap(), b"one", "the stuck file stayed whole");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A symlink in the user's old folder is never followed: a link to an ancestor would
/// make a following walk run forever and overflow the stack at start-up (never crash).
/// The count reads the entries' own kinds and covers the real files only.
#[cfg(unix)]
#[test]
fn a_symlink_in_the_old_folder_is_not_followed() {
    use std::os::unix::fs::symlink;
    let dir = std::env::temp_dir().join(format!("suhina-pack-link-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let old_models = dir.join("NAM models");
    std::fs::create_dir_all(&old_models).unwrap();
    std::fs::write(old_models.join("One.nam"), b"one").unwrap();
    symlink(&old_models, old_models.join("loop")).unwrap();

    assert_eq!(files_in(&old_models), 1, "the walk ends: the link is not followed");
    let warnings = migrate_old_folders(&dir, &dir);
    assert!(
        warnings.iter().any(|line| line.contains("moved") && line.contains("1 file(s)")),
        "the real file moved, the link with it: {warnings:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A link is classified by its own kind, never by what it points at: a self-parenting
/// link inside a pack whose name the new tree holds must neither loop the walk nor drag
/// the real files through the link into nested paths — the file moves to the place the
/// tree expects, the link stays whole, and the counts read the same whatever order the
/// directory's entries come in (unix: needs `symlink`).
#[cfg(unix)]
#[test]
fn a_link_in_a_clashing_pack_is_never_followed_by_the_move() {
    use std::os::unix::fs::symlink;
    let dir = std::env::temp_dir().join(format!("suhina-pack-link-clash-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let old_pack = dir.join("NAM models").join("My Amps");
    std::fs::create_dir_all(&old_pack).unwrap();
    std::fs::write(old_pack.join("One.nam"), b"one").unwrap();
    symlink(&old_pack, old_pack.join("loop")).unwrap();
    let new_pack = dir.join("Amp models").join("My Amps");
    std::fs::create_dir_all(&new_pack).unwrap();
    std::fs::write(new_pack.join("Clash.nam"), b"clash").unwrap();
    std::fs::write(new_pack.join("loop"), b"the user's own link-name file").unwrap();

    let warnings = migrate_old_folders(&dir, &dir);
    assert_eq!(std::fs::read(new_pack.join("One.nam")).unwrap(), b"one", "the real file moved to the tree's place");
    assert_eq!(std::fs::read(new_pack.join("Clash.nam")).unwrap(), b"clash", "the user's file never written over");
    assert_eq!(std::fs::read(new_pack.join("loop")).unwrap(), b"the user's own link-name file", "the link's name kept");
    assert!(
        !new_pack.join("loop").join("One.nam").exists() && !new_pack.join("loop/loop").exists(),
        "nothing landed in nested paths through the link"
    );
    assert!(old_pack.join("loop").is_symlink(), "the link itself stayed with the kept name");
    assert!(
        warnings
            .iter()
            .any(|line| line.contains("mostly moved") && line.contains("1 file(s)") && line.contains("1 kept")),
        "one real file moved, one kept name, the link counted no file: {warnings:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file the user has keeps its own bytes: the unpack writes nothing over it.
#[test]
fn a_carried_file_is_never_overwritten() {
    let dir = std::env::temp_dir().join(format!("suhina-pack-keep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (_, library_folder, pack_folder, name, _) in suhina_device_registry::pack_files_flat() {
        let pack_over = dir.join(library_folder).join(pack_folder);
        std::fs::create_dir_all(&pack_over).unwrap();
        let target = pack_over.join(name);
        std::fs::write(&target, b"the user's own bytes").unwrap();
    }
    // The records the user carries are theirs too: the unpack writes none of them over.
    for (library_folder, pack_folder, name, _) in suhina_device_registry::pack_records() {
        let pack_over = dir.join(library_folder).join(pack_folder);
        std::fs::create_dir_all(&pack_over).unwrap();
        std::fs::write(pack_over.join(name), b"the user's own bytes").unwrap();
    }
    let written = unpack_pack(&dir);
    assert_eq!(written, 0, "the unpack wrote over a file the user carries");
    for (_, library_folder, pack_folder, name, _) in suhina_device_registry::pack_files_flat() {
        let target = dir.join(library_folder).join(pack_folder).join(name);
        let user = std::fs::read(&target).unwrap();
        assert_eq!(user, b"the user's own bytes", "{name} was overwritten");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
